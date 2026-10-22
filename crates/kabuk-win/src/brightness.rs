//! A monitor's brightness for the quick settings panel (DECISIONS D88): a laptop's own
//! panel through Windows' WMI brightness class (what Windows' slider moves), an external
//! monitor through DDC/CI (the monitor's own setting, over its cable). Both are slow
//! (tens to hundreds of milliseconds): call them off the bar's thread. A monitor that
//! answers neither has no slider.

use std::io;

use windows::core::{BSTR, HSTRING};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

/// Where a brightness is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// The computer's own display (WMI).
    Internal,
    /// An external monitor (DDC/CI) on this monitor handle.
    External(isize),
}

fn wmi() -> io::Result<IWbemServices> {
    // SAFETY: plain COM calls on a thread with COM; the proxy is set as WMI asks.
    unsafe {
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(crate::sync::io_error)?;
        let services = locator
            .ConnectServer(
                &BSTR::from(r"root\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .map_err(crate::sync::io_error)?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .map_err(crate::sync::io_error)?;
        Ok(services)
    }
}

/// The first object a WQL query gives.
fn first(services: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
    // SAFETY: plain COM calls; the enumerator yields at most one object here.
    unsafe {
        let e: IEnumWbemClassObject = services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut objects = [None];
        let mut got = 0u32;
        let _ = e.Next(WBEM_INFINITE, &mut objects, &mut got);
        if got == 0 {
            return None;
        }
        objects[0].take()
    }
}

fn get(o: &IWbemClassObject, name: &str) -> Option<VARIANT> {
    let mut v = VARIANT::default();
    // SAFETY: a valid name and an out VARIANT.
    unsafe { o.Get(&HSTRING::from(name), 0, &mut v, None, None) }.ok()?;
    Some(v)
}

/// The computer's own display's brightness (0 to 100), when it has one WMI can set.
fn internal() -> Option<u8> {
    let s = wmi().ok()?;
    let o = first(
        &s,
        "SELECT CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE",
    )?;
    let v = get(&o, "CurrentBrightness")?;
    u8::try_from(u32::try_from(&v).ok()?).ok()
}

fn set_internal(percent: u8) -> io::Result<()> {
    let s = wmi()?;
    let methods = first(&s, "SELECT * FROM WmiMonitorBrightnessMethods WHERE Active=TRUE")
        .ok_or_else(|| io::Error::other("no WMI brightness"))?;
    let path = get(&methods, "__PATH")
        .and_then(|v| BSTR::try_from(&v).ok())
        .ok_or_else(|| io::Error::other("no path"))?;
    // SAFETY: plain COM calls with valid names; the in-parameters are this method's own.
    unsafe {
        let mut class = None;
        s.GetObject(
            &BSTR::from("WmiMonitorBrightnessMethods"),
            Default::default(),
            None,
            Some(&mut class),
            None,
        )
        .map_err(crate::sync::io_error)?;
        let class = class.ok_or_else(|| io::Error::other("no class"))?;
        let mut params = None;
        class
            .GetMethod(&HSTRING::from("WmiSetBrightness"), 0, &mut params, std::ptr::null_mut())
            .map_err(crate::sync::io_error)?;
        let params = params
            .ok_or_else(|| io::Error::other("no parameters"))?
            .SpawnInstance(0)
            .map_err(crate::sync::io_error)?;
        params
            .Put(&HSTRING::from("Timeout"), 0, &VARIANT::from(0u32), 0)
            .map_err(crate::sync::io_error)?;
        params
            .Put(&HSTRING::from("Brightness"), 0, &VARIANT::from(percent), 0)
            .map_err(crate::sync::io_error)?;
        s.ExecMethod(
            &path,
            &BSTR::from("WmiSetBrightness"),
            Default::default(),
            None,
            &params,
            None,
            None,
        )
        .map_err(crate::sync::io_error)?;
    }
    Ok(())
}

/// The physical monitors behind a monitor handle, destroyed when dropped.
struct Physical(Vec<PHYSICAL_MONITOR>);

impl Physical {
    fn of(monitor: isize) -> Option<Physical> {
        let h = HMONITOR(monitor as *mut _);
        let mut n = 0u32;
        // SAFETY: the two-call form with a buffer of the size counted.
        unsafe {
            GetNumberOfPhysicalMonitorsFromHMONITOR(h, &mut n).ok()?;
            let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
            GetPhysicalMonitorsFromHMONITOR(h, &mut list).ok()?;
            Some(Physical(list))
        }
    }
}

impl Drop for Physical {
    fn drop(&mut self) {
        // SAFETY: the monitors got in `of`.
        let _ = unsafe { DestroyPhysicalMonitors(&self.0) };
    }
}

/// The share of a DDC/CI range a value is at, 0 to 100.
pub fn percent_of(min: u32, value: u32, max: u32) -> u8 {
    if max <= min {
        return 0;
    }
    let p = (u64::from(value.clamp(min, max) - min) * 100 + u64::from(max - min) / 2) / u64::from(max - min);
    u8::try_from(p).unwrap_or(100)
}

/// The value of a DDC/CI range at `percent`.
pub fn value_at(min: u32, percent: u8, max: u32) -> u32 {
    let span = u64::from(max.saturating_sub(min));
    min + u32::try_from((span * u64::from(percent.min(100)) + 50) / 100).unwrap_or(0)
}

fn external(monitor: isize) -> Option<u8> {
    let p = Physical::of(monitor)?;
    let m = p.0.first()?;
    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
    // SAFETY: a physical monitor handle and out values.
    if unsafe { GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) } == 0 {
        return None;
    }
    Some(percent_of(min, cur, max))
}

fn set_external(monitor: isize, percent: u8) -> io::Result<()> {
    let p = Physical::of(monitor).ok_or_else(|| io::Error::other("no physical monitor"))?;
    for m in &p.0 {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        // SAFETY: physical monitor handles and out values.
        unsafe {
            if GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0
                || SetMonitorBrightness(m.hPhysicalMonitor, value_at(min, percent, max)) == 0
            {
                return Err(io::Error::other("the monitor did not take the brightness"));
            }
        }
    }
    Ok(())
}

/// The brightness of monitor `monitor` (its handle) and where it is set: the computer's own
/// display when WMI has one, else the monitor over DDC/CI; `None` when neither answers.
pub fn read(monitor: isize) -> Option<(Panel, u8)> {
    internal()
        .map(|p| (Panel::Internal, p))
        .or_else(|| external(monitor).map(|p| (Panel::External(monitor), p)))
}

pub fn set(panel: Panel, percent: u8) -> io::Result<()> {
    match panel {
        Panel::Internal => set_internal(percent),
        Panel::External(m) => set_external(m, percent),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a range not starting at 0, its ends, an empty range, values past it;
    //! reading the machine's own monitor never fails (none answering is fine).
    use super::*;

    #[test]
    fn a_ddc_range_maps_to_percent_and_back() {
        assert_eq!(percent_of(0, 50, 100), 50);
        assert_eq!(percent_of(20, 20, 120), 0);
        assert_eq!(percent_of(20, 120, 120), 100);
        assert_eq!(percent_of(20, 70, 120), 50);
        assert_eq!(percent_of(5, 5, 5), 0);
        assert_eq!(percent_of(0, 500, 100), 100);
        assert_eq!(value_at(20, 50, 120), 70);
        assert_eq!(value_at(0, 100, 255), 255);
        assert_eq!(value_at(0, 200, 100), 100);
    }

    #[test]
    fn the_machines_brightness_reads() {
        crate::system::com_init_mta();
        let primary = crate::shell::monitors()
            .into_iter()
            .find(|m| m.primary)
            .map(|m| m.handle);
        if let Some(h) = primary {
            if let Some((_, p)) = read(h) {
                assert!(p <= 100);
            }
        }
    }
}
