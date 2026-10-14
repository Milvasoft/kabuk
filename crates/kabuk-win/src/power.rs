//! The launcher's system commands (SPEC §2.5): lock, sleep, the screen saver, restart and
//! shut down. Never called by tests: each one acts on the user's session. Also the power
//! facts the animated wallpaper pauses for: the battery and the displays turned off.

use crate::msgloop::Window;

/// Locks the session, like Win+L.
pub fn lock() -> std::io::Result<()> {
    // SAFETY: plain call.
    unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map_err(crate::sync::io_error)
}

/// Puts the computer to sleep (not hibernation), letting programs and wake timers have
/// their say as Start's "Sleep" does.
pub fn sleep() -> std::io::Result<()> {
    // SAFETY: plain call.
    let ok = unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) };
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Starts the screen saver Windows is set to, through a window of ours.
pub fn start_screen_saver(window: &Window) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::SC_SCREENSAVE;
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_SYSCOMMAND};
    // SAFETY: the default handling of SC_SCREENSAVE for our own window.
    unsafe {
        DefWindowProcW(
            window.hwnd().raw(),
            WM_SYSCOMMAND,
            WPARAM(SC_SCREENSAVE as usize),
            LPARAM(0),
        );
    }
}

/// Turns the displays off now (they come back on with input), through a window of ours.
pub fn displays_off(window: &Window) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, SC_MONITORPOWER, WM_SYSCOMMAND};
    /// `SC_MONITORPOWER`'s "off".
    const OFF: isize = 2;
    // SAFETY: the default handling of SC_MONITORPOWER for our own window.
    unsafe {
        DefWindowProcW(
            window.hwnd().raw(),
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(OFF),
        );
    }
}

/// Signs the user out, as Start's "Sign out" does (programs are asked to close).
pub fn sign_out() -> std::io::Result<()> {
    use windows::Win32::System::Shutdown::{ExitWindowsEx, EWX_LOGOFF, SHUTDOWN_REASON};
    // SAFETY: plain call; signing out needs no privilege.
    unsafe { ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(0)) }.map_err(crate::sync::io_error)
}

/// Restarts or shuts down Windows as Start's power menu does (programs are asked to close).
pub fn restart(shut_down: bool) -> std::io::Result<()> {
    let flag = if shut_down { "/s" } else { "/r" };
    let windows_dir = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let exe = std::path::Path::new(&windows_dir).join("System32").join("shutdown.exe");
    std::process::Command::new(exe)
        .args([flag, "/t", "0"])
        .spawn()
        .map(|_| ())
}

/// True while the computer runs on its battery (SPEC §8.21: the animated wallpaper stops).
pub fn on_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: a valid out struct.
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.ACLineStatus == 0
}

/// The console's display state (`GUID_CONSOLE_DISPLAY_STATE`): 0 off, 1 on, 2 dimmed.
const DISPLAY_STATE: windows::core::GUID = windows::core::GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
const DISPLAY_OFF: u32 = 0;
/// `PBT_POWERSETTINGCHANGE`.
const SETTING_CHANGED: u32 = 0x8013;

/// Whether Windows' display state value means something can be seen (on, or dimmed).
pub fn displays_on_for(state: u32) -> bool {
    state != DISPLAY_OFF
}

/// Watches whether the displays are on (off when Windows turns them off for idling), from
/// when it is made until dropped. Windows tells the current state at once, then each
/// change, on a thread of its own; no window is needed.
pub struct DisplayWatch {
    handle: windows::Win32::System::Power::HPOWERNOTIFY,
    // Read by the callback until the registration ends; kept here for that long.
    _params: Box<windows::Win32::System::Power::DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    state: Box<std::sync::atomic::AtomicBool>,
}

unsafe extern "system" fn on_display_state(
    context: *const core::ffi::c_void,
    kind: u32,
    setting: *const core::ffi::c_void,
) -> u32 {
    use windows::Win32::System::Power::POWERBROADCAST_SETTING;
    if kind != SETTING_CHANGED || context.is_null() || setting.is_null() {
        return 0;
    }
    // SAFETY: for PBT_POWERSETTINGCHANGE Windows passes a POWERBROADCAST_SETTING whose
    // data (`DataLength` bytes, a DWORD for this setting) follows it; `context` is the
    // AtomicBool the watch keeps alive while registered.
    unsafe {
        let s = &*(setting as *const POWERBROADCAST_SETTING);
        if s.PowerSetting == DISPLAY_STATE && s.DataLength as usize >= std::mem::size_of::<u32>() {
            let value = std::ptr::read_unaligned(s.Data.as_ptr() as *const u32);
            (*(context as *const std::sync::atomic::AtomicBool))
                .store(displays_on_for(value), std::sync::atomic::Ordering::Relaxed);
        }
    }
    0
}

impl DisplayWatch {
    pub fn start() -> std::io::Result<DisplayWatch> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Power::{
            PowerSettingRegisterNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
        };
        use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;
        let state = Box::new(std::sync::atomic::AtomicBool::new(true));
        let params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_display_state),
            Context: (&*state as *const std::sync::atomic::AtomicBool).cast_mut().cast(),
        });
        let mut handle: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: the parameters and the state they point to are boxed and live as long
        // as the registration, which `Drop` ends.
        let r = unsafe {
            PowerSettingRegisterNotification(
                &DISPLAY_STATE,
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(
                    (&*params as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                        .cast_mut()
                        .cast(),
                ),
                &mut handle,
            )
        };
        if r.is_err() {
            return Err(std::io::Error::from_raw_os_error(r.0 as i32));
        }
        Ok(DisplayWatch {
            handle: HPOWERNOTIFY(handle as isize),
            _params: params,
            state,
        })
    }

    /// False while Windows has the displays turned off.
    pub fn displays_on(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for DisplayWatch {
    fn drop(&mut self) {
        // SAFETY: our registration, ended once; the callback is not called after it.
        let _ = unsafe { windows::Win32::System::Power::PowerSettingUnregisterNotification(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: Windows' display states (off, on, dimmed); a watch starts, says
    //! something, and ends.
    use super::*;

    #[test]
    fn only_off_hides_the_displays() {
        assert!(!displays_on_for(0));
        assert!(displays_on_for(1));
        assert!(displays_on_for(2), "dimmed is still seen");
    }

    #[test]
    fn a_display_watch_starts_and_stops() {
        let w = DisplayWatch::start().expect("registered");
        // Whatever the displays are doing now, the answer is a plain bool.
        let _ = w.displays_on();
        drop(w);
    }
}
