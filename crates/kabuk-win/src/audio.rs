//! The default output device's volume through Core Audio, with change notifications
//! posted to a window (no polling), the active output devices and which one is the
//! default. Call [`crate::system::com_init`] first.

use windows::core::{implement, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl,
    MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

use crate::msgloop::{post_message, Msg};
use crate::sync::io_error;
use crate::window::Hwnd;

/// Where change notifications go. `wparam` is [`VOLUME_CHANGED`] or [`DEVICE_CHANGED`].
#[derive(Clone, Copy, Debug)]
pub struct Notify {
    pub window: Hwnd,
    pub message: u32,
}

pub const VOLUME_CHANGED: usize = 0;
pub const DEVICE_CHANGED: usize = 1;

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

/// An active output device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// Core Audio's endpoint id, which [`set_default_output`] takes.
    pub id: String,
    /// The name Windows shows ("Headphones (Bluetooth)").
    pub name: String,
    pub default: bool,
}

/// `PKEY_Device_FriendlyName` (Win32_Devices_FunctionDiscovery, not enabled here).
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

/// Windows' own client for the default device (`CPolicyConfigClient`), and the interface
/// its sound settings use to change it (`IPolicyConfig`): undocumented, unchanged since
/// Windows 7 (DECISIONS D54).
const POLICY_CONFIG_CLIENT: windows::core::GUID =
    windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IPOLICY_CONFIG: windows::core::GUID = windows::core::GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
/// `IPolicyConfig::SetDefaultEndpoint`'s place in its vtable (after IUnknown's three and
/// ten methods of its own).
const SET_DEFAULT_ENDPOINT: usize = 13;

/// Makes the output `id` Windows' default for every role (console, multimedia,
/// communications), as Windows' sound settings do.
pub fn set_default_output(id: &str) -> std::io::Result<()> {
    use windows::core::{IUnknown, Interface, HRESULT};
    use windows::Win32::Media::Audio::{eCommunications, eMultimedia};
    type SetDefault = unsafe extern "system" fn(*mut core::ffi::c_void, PCWSTR, ERole) -> HRESULT;
    let wide = windows::core::HSTRING::from(id);
    // SAFETY: (whole block) Windows' policy client, created here; the interface pointer
    // QueryInterface gives is called through the slot of SetDefaultEndpoint, which has
    // this signature, and released (taken back as an IUnknown) before returning.
    unsafe {
        let client: IUnknown = CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL).map_err(io_error)?;
        let mut raw = std::ptr::null_mut();
        client.query(&IPOLICY_CONFIG, &mut raw).ok().map_err(io_error)?;
        let policy = IUnknown::from_raw(raw);
        let vtable = *(raw as *const *const usize);
        let set: SetDefault = std::mem::transmute(*vtable.add(SET_DEFAULT_ENDPOINT));
        for role in [eConsole, eMultimedia, eCommunications] {
            set(raw, PCWSTR(wide.as_ptr()), role).ok().map_err(io_error)?;
        }
        drop(policy);
    }
    tracing::info!("default audio output changed");
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    pub percent: u32,
    pub muted: bool,
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    notify: Notify,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        self.notify.post(VOLUME_CHANGED);
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceCallback {
    notify: Notify,
}

impl IMMNotificationClient_Impl for DeviceCallback_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            self.notify.post(DEVICE_CHANGED);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Volume control of the default output device.
pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    device_client: IMMNotificationClient,
    endpoint: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    notify: Notify,
}

impl Audio {
    pub fn new(notify: Notify) -> std::io::Result<Audio> {
        // SAFETY: COM is initialised on this thread by the caller.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(io_error)?;
        let device_client: IMMNotificationClient = DeviceCallback { notify }.into();
        // SAFETY: the client stays alive in `self` until it is unregistered in Drop.
        unsafe { enumerator.RegisterEndpointNotificationCallback(&device_client) }.map_err(io_error)?;
        let mut audio = Audio {
            enumerator,
            device_client,
            endpoint: None,
            notify,
        };
        audio.reconnect();
        Ok(audio)
    }

    /// Re-attaches to the current default device (after [`DEVICE_CHANGED`]).
    pub fn reconnect(&mut self) {
        self.detach();
        // SAFETY: plain COM calls; the callback is unregistered before it is released.
        let attached = unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .and_then(|device| {
                    let endpoint: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
                    let callback: IAudioEndpointVolumeCallback = VolumeCallback { notify: self.notify }.into();
                    endpoint.RegisterControlChangeNotify(&callback)?;
                    Ok((endpoint, callback))
                })
        };
        match attached {
            Ok(e) => self.endpoint = Some(e),
            // No output device (e.g. all unplugged): the module hides until one appears.
            Err(e) => tracing::info!(error = %e, "no default audio output"),
        }
    }

    fn detach(&mut self) {
        if let Some((endpoint, callback)) = self.endpoint.take() {
            // SAFETY: unregisters the callback registered in `reconnect`.
            let _ = unsafe { endpoint.UnregisterControlChangeNotify(&callback) };
        }
    }

    /// The active output devices, the default one marked, in Windows' order.
    pub fn outputs(&self) -> Vec<Output> {
        use windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE;
        use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
        use windows::Win32::System::Com::{CoTaskMemFree, STGM_READ};
        let take = |p: windows::core::PWSTR| {
            // SAFETY: a string Core Audio allocated, copied and then freed once.
            unsafe {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            }
        };
        // SAFETY: plain COM calls on the enumerator and the devices it gives.
        unsafe {
            let default = self
                .enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .and_then(|d| d.GetId())
                .map(take)
                .ok();
            let Ok(all) = self.enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            (0..all.GetCount().unwrap_or(0))
                .filter_map(|i| {
                    let device = all.Item(i).ok()?;
                    let id = take(device.GetId().ok()?);
                    let name = device
                        .OpenPropertyStore(STGM_READ)
                        .and_then(|s| s.GetValue(&FRIENDLY_NAME))
                        .and_then(|v| PropVariantToStringAlloc(&v))
                        .map(take)
                        .unwrap_or_else(|_| id.clone());
                    Some(Output {
                        default: default.as_deref() == Some(id.as_str()),
                        id,
                        name,
                    })
                })
                .collect()
        }
    }

    pub fn volume(&self) -> Option<Volume> {
        let (endpoint, _) = self.endpoint.as_ref()?;
        // SAFETY: plain COM getters.
        unsafe {
            let level = endpoint.GetMasterVolumeLevelScalar().ok()?;
            let muted = endpoint.GetMute().ok()?.as_bool();
            Some(Volume {
                percent: (level * 100.0).round().clamp(0.0, 100.0) as u32,
                muted,
            })
        }
    }

    /// Sets the volume to `percent` (clamped to 0-100).
    pub fn set_percent(&self, percent: u32) {
        let Some((endpoint, _)) = self.endpoint.as_ref() else {
            return;
        };
        let target = percent.min(100) as f32 / 100.0;
        // SAFETY: plain COM setter; no event context GUID.
        let _ = unsafe { endpoint.SetMasterVolumeLevelScalar(target, std::ptr::null()) };
    }

    /// Mutes or unmutes the output.
    pub fn toggle_mute(&self) {
        let (Some((endpoint, _)), Some(now)) = (self.endpoint.as_ref(), self.volume()) else {
            return;
        };
        // SAFETY: plain COM setter; no event context GUID.
        let _ = unsafe { endpoint.SetMute(!now.muted, std::ptr::null()) };
    }

    /// Changes the volume by `delta` percent points (clamped to 0-100).
    pub fn step(&self, delta: i32) {
        let (Some((endpoint, _)), Some(now)) = (self.endpoint.as_ref(), self.volume()) else {
            return;
        };
        let target = (i64::from(now.percent) + i64::from(delta)).clamp(0, 100) as f32 / 100.0;
        // SAFETY: plain COM setter; no event context GUID.
        let _ = unsafe { endpoint.SetMasterVolumeLevelScalar(target, std::ptr::null()) };
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.detach();
        // SAFETY: unregisters the client registered in `new`.
        let _ = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.device_client)
        };
    }
}

#[cfg(test)]
mod tests {
    //! Reads the volume when a device exists; changing it would disturb the user, so
    //! `step` is exercised with a zero delta only.
    use super::*;

    /// Reads the outputs (nothing is changed: setting the default would switch the user's
    /// sound): one at most is the default, and it is the one the volume belongs to.
    #[test]
    fn outputs_are_listed_with_the_default_marked() {
        crate::system::com_init();
        let notify = Notify {
            window: Hwnd(0),
            message: 0,
        };
        let audio = Audio::new(notify).expect("device enumerator");
        let outputs = audio.outputs();
        assert!(outputs.iter().filter(|o| o.default).count() <= 1);
        assert!(outputs.iter().all(|o| !o.id.is_empty() && !o.name.is_empty()));
        if audio.volume().is_some() {
            assert_eq!(outputs.iter().filter(|o| o.default).count(), 1, "{outputs:?}");
        }
        // The undocumented call works: the default made the default again (nothing
        // changes for the user), and it still is.
        if let Some(current) = outputs.iter().find(|o| o.default) {
            set_default_output(&current.id).expect("IPolicyConfig answers");
            assert_eq!(
                audio.outputs().into_iter().find(|o| o.default).map(|o| o.id),
                Some(current.id.clone())
            );
        }
    }

    #[test]
    fn reads_volume_if_a_device_exists() {
        crate::system::com_init();
        let notify = Notify {
            window: Hwnd(0),
            message: 0,
        };
        let audio = Audio::new(notify).expect("device enumerator");
        if let Some(v) = audio.volume() {
            assert!(v.percent <= 100);
            audio.step(0);
            assert_eq!(audio.volume().map(|v| v.percent), Some(v.percent));
        }
    }
}
