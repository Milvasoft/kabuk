//! The network the computer is on, for the bar's network indicator and its panel
//! (DECISIONS D88): the link (Ethernet before Wi-Fi, as Windows' taskbar shows it), the
//! Wi-Fi networks around, joining and leaving them, and a watch that tells a window when
//! any of it changes, from the network list manager's and the WLAN service's own
//! notifications (no polling).
//!
//! Since Windows 11 24H2, Wi-Fi names reach a desktop program only with location access
//! for desktop apps; without it the names are empty and the list is refused
//! ([`Wlan::networks`] says so).

use std::ffi::c_void;
use std::io;

use kabuk_core::network::{Link, Network};
use windows::core::{implement, Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, MediaConnectStateConnected};
use windows::Win32::NetworkManagement::WiFi::*;
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, INetworkListManagerEvents, INetworkListManagerEvents_Impl, NetworkListManager,
    NLM_CONNECTIVITY, NLM_CONNECTIVITY_IPV4_INTERNET, NLM_CONNECTIVITY_IPV6_INTERNET,
};
use windows::Win32::System::Com::{CoCreateInstance, IConnectionPoint, IConnectionPointContainer, CLSCTX_ALL};

use crate::msgloop::{post_message, Msg};
use crate::window::Hwnd;

/// `wparam` of the watch's message: something about the link changed.
pub const CHANGED: usize = 0;
/// `wparam` of the watch's message: a Wi-Fi scan finished (the panel's list is ready).
pub const SCANNED: usize = 1;
/// `wparam` of the watch's message: joining a Wi-Fi network failed (a wrong key).
pub const FAILED: usize = 2;

/// The WLAN client version for Windows Vista and later.
const WLAN_CLIENT_VERSION: u32 = 2;
/// `IF_TYPE_ETHERNET_CSMACD`.
const ETHERNET: u32 = 6;
/// `MIB_IF_ROW2.InterfaceAndOperStatusFlags`: a hardware interface, not a filter.
const HARDWARE_INTERFACE: u8 = 1 << 0;
const FILTER_INTERFACE: u8 = 1 << 1;

fn wlan_error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(code as i32)
}

/// A connection to the WLAN service.
pub struct Wlan(HANDLE);

// SAFETY: a WLAN client handle may be used from any thread.
unsafe impl Send for Wlan {}

impl Wlan {
    /// `Err` where there is no WLAN service (no Wi-Fi hardware, the service stopped).
    pub fn open() -> io::Result<Wlan> {
        let (mut version, mut handle) = (0u32, HANDLE::default());
        // SAFETY: valid out pointers.
        let code = unsafe { WlanOpenHandle(WLAN_CLIENT_VERSION, None, &mut version, &mut handle) };
        if code != 0 {
            return Err(wlan_error(code));
        }
        Ok(Wlan(handle))
    }

    /// The Wi-Fi interfaces: their ids and whether each is connected.
    fn interfaces(&self) -> Vec<(GUID, bool)> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        // SAFETY: the service allocates the list; it is freed below.
        unsafe {
            if WlanEnumInterfaces(self.0, None, &mut list) != 0 || list.is_null() {
                return Vec::new();
            }
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), (*list).dwNumberOfItems as usize);
            let out = items
                .iter()
                .map(|i| (i.InterfaceGuid, i.isState == wlan_interface_state_connected))
                .collect();
            WlanFreeMemory(list as *const c_void);
            out
        }
    }

    /// The first Wi-Fi interface, the one the panel and the bar follow.
    fn interface(&self) -> Option<GUID> {
        self.interfaces().first().map(|(g, _)| *g)
    }

    /// The connected Wi-Fi network's name and signal quality (0 to 100).
    pub fn current(&self) -> Option<(String, u8)> {
        let (guid, _) = self.interfaces().into_iter().find(|(_, connected)| *connected)?;
        let (mut size, mut data) = (0u32, std::ptr::null_mut::<c_void>());
        // SAFETY: the service allocates the attributes; they are freed below.
        unsafe {
            if WlanQueryInterface(
                self.0,
                &guid,
                wlan_intf_opcode_current_connection,
                None,
                &mut size,
                &mut data,
                None,
            ) != 0
                || data.is_null()
            {
                return None;
            }
            let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            let assoc = &a.wlanAssociationAttributes;
            let out = (
                ssid(&assoc.dot11Ssid),
                u8::try_from(assoc.wlanSignalQuality.min(100)).unwrap_or(100),
            );
            WlanFreeMemory(data);
            Some(out)
        }
    }

    /// Whether the Wi-Fi radio is on (its software and hardware switches); `None`
    /// without a Wi-Fi interface.
    pub fn radio_on(&self) -> Option<bool> {
        let guid = self.interface()?;
        let (mut size, mut data) = (0u32, std::ptr::null_mut::<c_void>());
        // SAFETY: the service allocates the state; it is freed below.
        unsafe {
            if WlanQueryInterface(
                self.0,
                &guid,
                wlan_intf_opcode_radio_state,
                None,
                &mut size,
                &mut data,
                None,
            ) != 0
                || data.is_null()
            {
                return None;
            }
            let state = &*(data as *const WLAN_RADIO_STATE);
            let n = (state.dwNumberOfPhys as usize).min(state.PhyRadioState.len());
            let on = state.PhyRadioState[..n].iter().any(|p| {
                p.dot11SoftwareRadioState == dot11_radio_state_on && p.dot11HardwareRadioState == dot11_radio_state_on
            });
            WlanFreeMemory(data);
            Some(on)
        }
    }

    /// Asks the first Wi-Fi interface to look for networks; [`SCANNED`] comes when it
    /// has.
    pub fn scan(&self) -> io::Result<()> {
        let guid = self.interface().ok_or_else(|| io::Error::other("no Wi-Fi interface"))?;
        // SAFETY: a valid interface id.
        let code = unsafe { WlanScan(self.0, &guid, None, None, None) };
        if code != 0 {
            return Err(wlan_error(code));
        }
        Ok(())
    }

    /// The networks the first Wi-Fi interface sees, as the service reports them (see
    /// [`kabuk_core::network::listed`] for the panel's order). `PermissionDenied` without
    /// location access for desktop apps.
    pub fn networks(&self) -> io::Result<Vec<Network>> {
        let guid = self.interface().ok_or_else(|| io::Error::other("no Wi-Fi interface"))?;
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        // SAFETY: the service allocates the list; it is freed below.
        unsafe {
            let code = WlanGetAvailableNetworkList(self.0, &guid, 0, None, &mut list);
            if code == ERROR_ACCESS_DENIED.0 {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            if code != 0 || list.is_null() {
                return Err(wlan_error(code));
            }
            let items = std::slice::from_raw_parts((*list).Network.as_ptr(), (*list).dwNumberOfItems as usize);
            let out = items
                .iter()
                .map(|n| Network {
                    ssid: ssid(&n.dot11Ssid),
                    signal: u8::try_from(n.wlanSignalQuality.min(100)).unwrap_or(100),
                    secured: n.bSecurityEnabled.as_bool(),
                    wpa3: n.dot11DefaultAuthAlgorithm == DOT11_AUTH_ALGO_WPA3_SAE
                        || n.dot11DefaultAuthAlgorithm == DOT11_AUTH_ALGO_WPA3,
                    known: n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
                    connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                })
                .collect();
            WlanFreeMemory(list as *const c_void);
            Ok(out)
        }
    }

    /// Joins `network`: with the profile Windows keeps for it, or, for a new one, with a
    /// profile made from `key` (WPA2- or WPA3-Personal; none for an open network). The
    /// result of the join comes as [`CHANGED`].
    pub fn connect(&self, network: &Network, key: Option<&str>) -> io::Result<()> {
        let guid = self.interface().ok_or_else(|| io::Error::other("no Wi-Fi interface"))?;
        let profile = HSTRING::from(network.ssid.as_str());
        if !network.known {
            let xml = HSTRING::from(kabuk_core::network::profile_xml(
                &network.ssid,
                key.filter(|_| network.secured),
                network.wpa3,
            ));
            let mut reason = 0u32;
            // SAFETY: valid strings for the duration of the call.
            let code = unsafe { WlanSetProfile(self.0, &guid, 0, &xml, PCWSTR::null(), true, None, &mut reason) };
            if code != 0 {
                return Err(io::Error::other(format!("profile refused ({code}, reason {reason})")));
            }
        }
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(profile.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        // SAFETY: the parameters and the profile name outlive the call.
        let code = unsafe { WlanConnect(self.0, &guid, &params, None) };
        if code != 0 {
            return Err(wlan_error(code));
        }
        Ok(())
    }

    /// Leaves the connected Wi-Fi network.
    pub fn disconnect(&self) -> io::Result<()> {
        let guid = self.interface().ok_or_else(|| io::Error::other("no Wi-Fi interface"))?;
        // SAFETY: a valid interface id.
        let code = unsafe { WlanDisconnect(self.0, &guid, None) };
        if code != 0 {
            return Err(wlan_error(code));
        }
        Ok(())
    }

    /// Forgets the profile Windows keeps for `ssid` (a join that failed with a wrong key
    /// leaves one behind).
    pub fn forget(&self, ssid: &str) {
        if let Some(guid) = self.interface() {
            let name = HSTRING::from(ssid);
            // SAFETY: a valid interface id and name.
            let _ = unsafe { WlanDeleteProfile(self.0, &guid, &name, None) };
        }
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: the handle opened in `open`; closing also ends its notifications and
        // waits for a callback in progress.
        let _ = unsafe { WlanCloseHandle(self.0, None) };
    }
}

/// A network name as Windows reports it (its bytes, UTF-8 as a rule).
fn ssid(s: &DOT11_SSID) -> String {
    let n = (s.uSSIDLength as usize).min(s.ucSSID.len());
    String::from_utf8_lossy(&s.ucSSID[..n]).into_owned()
}

/// Whether a wired Ethernet adapter is up and connected.
pub fn ethernet() -> bool {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table; it is freed with FreeMibTable below.
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return false;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let up = rows.iter().any(|r| {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            r.Type == ETHERNET
                && r.OperStatus == IfOperStatusUp
                && r.MediaConnectState == MediaConnectStateConnected
                && flags & HARDWARE_INTERFACE != 0
                && flags & FILTER_INTERFACE == 0
        });
        FreeMibTable(table.cast());
        up
    }
}

/// How the computer reaches the network now: Ethernet before Wi-Fi.
pub fn link(wlan: Option<&Wlan>) -> Link {
    if ethernet() {
        return Link::Ethernet;
    }
    match wlan.and_then(Wlan::current) {
        Some((ssid, signal)) => Link::Wifi { ssid, signal },
        None => Link::None,
    }
}

fn manager() -> io::Result<INetworkListManager> {
    // SAFETY: plain COM creation on a thread with COM.
    unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) }.map_err(crate::sync::io_error)
}

/// Whether Windows finds the internet through the link.
pub fn internet() -> bool {
    let Ok(m) = manager() else { return false };
    // SAFETY: a plain query on the manager.
    unsafe { m.GetConnectivity() }
        .is_ok_and(|c| c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0) != 0)
}

/// Where the watch's notices go.
#[derive(Clone, Copy)]
struct Notify {
    window: Hwnd,
    message: u32,
}

impl Notify {
    fn post(&self, what: usize) {
        let _ = post_message(
            self.window,
            Msg {
                id: self.message,
                wparam: what,
                lparam: 0,
            },
        );
    }
}

#[implement(INetworkListManagerEvents)]
struct Connectivity(Notify);

impl INetworkListManagerEvents_Impl for Connectivity_Impl {
    fn ConnectivityChanged(&self, _: NLM_CONNECTIVITY) -> windows::core::Result<()> {
        self.0.post(CHANGED);
        Ok(())
    }
}

unsafe extern "system" fn on_wlan(data: *mut L2_NOTIFICATION_DATA, context: *mut c_void) {
    if data.is_null() || context.is_null() {
        return;
    }
    // SAFETY: the service passes its notification and the context given at
    // registration, a `Notify` the watch keeps until the handle is closed.
    let (data, notify) = unsafe { (&*data, &*(context as *const Notify)) };
    let acm = data.NotificationSource == WLAN_NOTIFICATION_SOURCE_ACM;
    let code = data.NotificationCode;
    let scan = acm
        && (code == wlan_notification_acm_scan_complete.0 as u32 || code == wlan_notification_acm_scan_fail.0 as u32);
    let failed = acm
        && (code == wlan_notification_acm_connection_attempt_fail.0 as u32
            || (code == wlan_notification_acm_connection_complete.0 as u32 && {
                let size = std::mem::size_of::<WLAN_CONNECTION_NOTIFICATION_DATA>();
                // SAFETY: the service's data for a completed connection, read only when
                // it is as large as the structure.
                !data.pData.is_null()
                    && data.dwDataSize as usize >= size
                    && unsafe { (*(data.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA)).wlanReasonCode } != 0
            }));
    notify.post(if failed {
        FAILED
    } else if scan {
        SCANNED
    } else {
        CHANGED
    });
}

/// Tells `window` with `message` ([`CHANGED`] or [`SCANNED`]) whenever the link, the
/// connectivity or the Wi-Fi networks change, until dropped. Made on a thread with COM
/// and a message loop (the network list manager's events come through it).
pub struct Watch {
    wlan: Option<Wlan>,
    /// The WLAN callback's context; it lives as long as the handle.
    _notify: Box<Notify>,
    advised: Option<(IConnectionPoint, u32)>,
}

impl Watch {
    pub fn start(window: Hwnd, message: u32) -> Watch {
        let notify = Box::new(Notify { window, message });
        let wlan = Wlan::open().ok().filter(|w| {
            // SAFETY: the context outlives the registration (it is dropped after the
            // handle is closed, in this struct's field order).
            let code = unsafe {
                WlanRegisterNotification(
                    w.0,
                    WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM,
                    true,
                    Some(on_wlan),
                    Some(&*notify as *const Notify as *const c_void),
                    None,
                    None,
                )
            };
            if code != 0 {
                tracing::warn!(code, "Wi-Fi changes not watched");
            }
            true
        });
        let advised = manager().ok().and_then(|m| {
            let sink: INetworkListManagerEvents = Connectivity(*notify).into();
            // SAFETY: plain COM calls; the cookie is used to unadvise in Drop.
            unsafe {
                let point = m
                    .cast::<IConnectionPointContainer>()
                    .ok()?
                    .FindConnectionPoint(&INetworkListManagerEvents::IID)
                    .ok()?;
                let cookie = point.Advise(&sink).ok()?;
                Some((point, cookie))
            }
        });
        if advised.is_none() {
            tracing::warn!("network connectivity changes not watched");
        }
        Watch {
            wlan,
            _notify: notify,
            advised,
        }
    }

    /// The watch's WLAN connection, for the panel (none without Wi-Fi).
    pub fn wlan(&self) -> Option<&Wlan> {
        self.wlan.as_ref()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some((point, cookie)) = self.advised.take() {
            // SAFETY: the cookie Advise gave.
            let _ = unsafe { point.Unadvise(cookie) };
        }
        // The handle goes before the context (field order): no callback outlives it.
        self.wlan = None;
    }
}

#[cfg(test)]
mod tests {
    //! On the machine running the tests: whatever its network, the link reads without
    //! failing, and a watch starts and stops (with or without Wi-Fi hardware).
    use super::*;

    #[test]
    fn the_link_reads_and_a_watch_starts_and_stops() {
        crate::system::com_init();
        let wlan = Wlan::open().ok();
        let link = link(wlan.as_ref());
        // Ethernet wins over Wi-Fi, as on Windows' taskbar.
        if ethernet() {
            assert_eq!(link, Link::Ethernet);
        }
        let _ = internet();
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.NetWatch", "", |_| None).unwrap();
        let watch = Watch::start(w.hwnd(), crate::msgloop::msg::APP + 901);
        assert_eq!(watch.wlan().is_some(), wlan.is_some());
        drop(watch);
    }

    #[test]
    fn a_name_is_its_bytes() {
        let mut s = DOT11_SSID {
            uSSIDLength: 5,
            ..Default::default()
        };
        s.ucSSID[..5].copy_from_slice(b"milva");
        assert_eq!(ssid(&s), "milva");
        s.uSSIDLength = 99;
        assert_eq!(ssid(&s).len(), 32, "a length past the buffer is cut");
    }
}
