//! The launcher's system commands (SPEC §2.5): lock, sleep, the screen saver, restart and
//! shut down. Never called by tests: each one acts on the user's session. Also the power
//! facts the animated wallpaper pauses for: the battery and the displays turned off.

use crate::msgloop::Window;
use crate::window::Hwnd;

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

/// `SYSTEM_POWER_STATUS.BatteryFlag`: charging, no battery, unknown.
const BATTERY_CHARGING: u8 = 8;
const NO_BATTERY: u8 = 128;
const BATTERY_UNKNOWN: u8 = 255;
/// `SYSTEM_POWER_STATUS` values Windows gives when it does not know.
const PERCENT_UNKNOWN: u8 = 255;
const SECONDS_UNKNOWN: u32 = u32::MAX;

/// The battery now (DECISIONS D88); `None` on a computer without one.
pub fn battery() -> Option<kabuk_core::battery::Battery> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: a valid out struct.
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    if s.BatteryFlag == NO_BATTERY || s.BatteryFlag == BATTERY_UNKNOWN || s.BatteryLifePercent == PERCENT_UNKNOWN {
        return None;
    }
    Some(kabuk_core::battery::Battery {
        percent: s.BatteryLifePercent.min(100),
        charging: s.BatteryFlag & BATTERY_CHARGING != 0,
        plugged: s.ACLineStatus == 1,
        seconds_left: (s.BatteryLifeTime != SECONDS_UNKNOWN).then_some(s.BatteryLifeTime),
    })
}

/// `GUID_BATTERY_PERCENTAGE_REMAINING` and `GUID_ACDC_POWER_SOURCE`.
const BATTERY_PERCENT: windows::core::GUID = windows::core::GUID::from_u128(0xa7ad8041_b45a_4cae_87a3_eecbb468a9e1);
const POWER_SOURCE: windows::core::GUID = windows::core::GUID::from_u128(0x5d3e9a59_e9d5_4b00_a6bd_ff34ff516548);

/// Where a battery watch's notices go.
struct BatteryNotify {
    window: crate::window::Hwnd,
    message: u32,
}

unsafe extern "system" fn on_battery_change(
    context: *const core::ffi::c_void,
    kind: u32,
    _setting: *const core::ffi::c_void,
) -> u32 {
    if kind != SETTING_CHANGED || context.is_null() {
        return 0;
    }
    // SAFETY: `context` is the BatteryNotify the watch keeps alive while registered.
    let n = unsafe { &*(context as *const BatteryNotify) };
    let _ = crate::msgloop::post_message(
        n.window,
        crate::msgloop::Msg {
            id: n.message,
            wparam: 0,
            lparam: 0,
        },
    );
    0
}

/// Tells `window` with `message` whenever the battery's charge or the power source
/// changes (DECISIONS D88), from Windows' power-setting notifications on a thread of
/// their own, until dropped. Windows also tells once at the start.
pub struct BatteryWatch {
    handles: Vec<windows::Win32::System::Power::HPOWERNOTIFY>,
    // Read by the callback until the registrations end; kept here for that long.
    _params: Box<windows::Win32::System::Power::DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    _notify: Box<BatteryNotify>,
}

impl BatteryWatch {
    pub fn start(window: crate::window::Hwnd, message: u32) -> std::io::Result<BatteryWatch> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Power::{
            PowerSettingRegisterNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
        };
        use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;
        let notify = Box::new(BatteryNotify { window, message });
        let params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_battery_change),
            Context: (&*notify as *const BatteryNotify).cast_mut().cast(),
        });
        let mut watch = BatteryWatch {
            handles: Vec::new(),
            _params: params,
            _notify: notify,
        };
        for setting in [BATTERY_PERCENT, POWER_SOURCE] {
            let mut handle: *mut core::ffi::c_void = std::ptr::null_mut();
            // SAFETY: the parameters and the notice they point to are boxed in the watch
            // and live as long as the registrations, which `Drop` ends.
            let r = unsafe {
                PowerSettingRegisterNotification(
                    &setting,
                    DEVICE_NOTIFY_CALLBACK,
                    HANDLE(
                        (&*watch._params as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
                            .cast_mut()
                            .cast(),
                    ),
                    &mut handle,
                )
            };
            if r.is_err() {
                return Err(std::io::Error::from_raw_os_error(r.0 as i32));
            }
            watch.handles.push(HPOWERNOTIFY(handle as isize));
        }
        Ok(watch)
    }
}

impl Drop for BatteryWatch {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            // SAFETY: our registrations, each ended once; the callback is not called
            // after them.
            let _ = unsafe { windows::Win32::System::Power::PowerSettingUnregisterNotification(h) };
        }
    }
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
    state: Box<DisplayState>,
}

/// What the callback writes: whether the displays are on, and where to post a change.
struct DisplayState {
    on: std::sync::atomic::AtomicBool,
    notify: Option<(Hwnd, u32)>,
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
    // DisplayState the watch keeps alive while registered.
    let (state, on) = unsafe {
        let s = &*(setting as *const POWERBROADCAST_SETTING);
        if s.PowerSetting != DISPLAY_STATE || (s.DataLength as usize) < std::mem::size_of::<u32>() {
            return 0;
        }
        let value = std::ptr::read_unaligned(s.Data.as_ptr() as *const u32);
        (&*(context as *const DisplayState), displays_on_for(value))
    };
    let was = state.on.swap(on, std::sync::atomic::Ordering::Relaxed);
    if let (Some((window, message)), true) = (state.notify, was != on) {
        let _ = crate::msgloop::post_message(
            window,
            crate::msgloop::Msg {
                id: message,
                wparam: usize::from(on),
                lparam: 0,
            },
        );
    }
    0
}

impl DisplayWatch {
    pub fn start() -> std::io::Result<DisplayWatch> {
        Self::with(None)
    }

    /// As [`DisplayWatch::start`], and posts `message` to `window` when the displays go off
    /// or come on (`wparam`: 1 on, 0 off).
    pub fn notifying(window: Hwnd, message: u32) -> std::io::Result<DisplayWatch> {
        Self::with(Some((window, message)))
    }

    fn with(notify: Option<(Hwnd, u32)>) -> std::io::Result<DisplayWatch> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Power::{
            PowerSettingRegisterNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
        };
        use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;
        let state = Box::new(DisplayState {
            on: std::sync::atomic::AtomicBool::new(true),
            notify,
        });
        let params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_display_state),
            Context: (&*state as *const DisplayState).cast_mut().cast(),
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
        self.state.on.load(std::sync::atomic::Ordering::Relaxed)
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

    /// M19 P4: the bar stops its figures while the displays are off, so it must hear
    /// the change. Windows tells the current state at the start; only a change is posted.
    #[test]
    fn a_display_watch_posts_only_changes() {
        const MSG: u32 = crate::msgloop::msg::APP + 903;
        let told = std::rc::Rc::new(std::cell::Cell::new(0));
        let t = told.clone();
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Display", "", move |m| {
            (m.id == MSG).then(|| {
                t.set(t.get() + 1);
                0
            })
        })
        .unwrap();
        let watch = DisplayWatch::notifying(w.hwnd(), MSG).expect("registered");
        crate::msgloop::run_until(&[], Some(std::time::Duration::from_millis(500)));
        // The displays are on while the tests run: the first answer is no change.
        assert!(watch.displays_on());
        assert_eq!(told.get(), 0, "no change, nothing posted");
        drop(watch);
    }

    #[test]
    fn a_display_watch_starts_and_stops() {
        let w = DisplayWatch::start().expect("registered");
        // Whatever the displays are doing now, the answer is a plain bool.
        let _ = w.displays_on();
        drop(w);
    }

    /// D88: the battery reads on any computer (none on a desktop), and a watch on it
    /// starts, tells the window once as Windows does at the start, and stops.
    #[test]
    fn the_battery_reads_and_a_watch_tells_its_window() {
        if let Some(b) = battery() {
            assert!(b.percent <= 100);
        }
        const MSG: u32 = crate::msgloop::msg::APP + 902;
        let told = std::rc::Rc::new(std::cell::Cell::new(0));
        let t = told.clone();
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Battery", "", move |m| {
            (m.id == MSG).then(|| {
                t.set(t.get() + 1);
                0
            })
        })
        .unwrap();
        let watch = BatteryWatch::start(w.hwnd(), MSG).expect("registered");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while told.get() == 0 && std::time::Instant::now() < deadline {
            crate::msgloop::run_until(&[], Some(std::time::Duration::from_millis(100)));
        }
        assert!(told.get() > 0, "Windows tells the current state at the start");
        drop(watch);
    }
}
