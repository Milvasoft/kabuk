//! The Windows taskbar and monitor work areas.

use kabuk_core::state::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    SendNotifyMessageW, SystemParametersInfoW, HWND_BROADCAST, MONITORINFOF_PRIMARY, SPI_SETWORKAREA,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_SETTINGCHANGE,
};

use crate::sync::io_error;
use crate::window::{windows_of_class, Hwnd};

const PRIMARY_TASKBAR_CLASS: &str = "Shell_TrayWnd";
const SECONDARY_TASKBAR_CLASS: &str = "Shell_SecondaryTrayWnd";

/// The process that owns the real taskbars. Kabuk's own tray window has the same class
/// (DECISIONS D19), so taskbars are told apart by their owner.
const TASKBAR_OWNER: &str = "explorer.exe";

fn owned_by_explorer(w: &Hwnd) -> bool {
    w.pid()
        .and_then(|pid| crate::process::Process::query(pid).ok())
        .and_then(|p| p.image_name())
        .is_some_and(|name| name.eq_ignore_ascii_case(TASKBAR_OWNER))
}

/// Explorer's primary taskbar.
pub fn primary_taskbar() -> Option<Hwnd> {
    windows_of_class(PRIMARY_TASKBAR_CLASS)
        .into_iter()
        .find(owned_by_explorer)
}

/// Explorer's primary taskbar and the taskbars of other monitors.
pub fn taskbars() -> Vec<Hwnd> {
    let mut out = windows_of_class(PRIMARY_TASKBAR_CLASS);
    out.extend(windows_of_class(SECONDARY_TASKBAR_CLASS));
    out.retain(owned_by_explorer);
    out
}

/// Hides every taskbar. Callers record the taskbar in the state file first and turn on
/// auto-hide, so the hidden taskbars give their space back. Returns how many were found.
pub fn hide_taskbars() -> usize {
    let bars = taskbars();
    for bar in &bars {
        bar.hide_async();
    }
    bars.len()
}

/// Shows every taskbar without activating it. Harmless when they are already visible.
/// Returns how many taskbars were found.
pub fn show_taskbars() -> usize {
    let bars = taskbars();
    for bar in &bars {
        bar.show_async();
    }
    bars.len()
}

fn appbar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: u32::try_from(std::mem::size_of::<APPBARDATA>()).unwrap_or_default(),
        hWnd: primary_taskbar().map(|h| h.raw()).unwrap_or_default(),
        ..Default::default()
    }
}

/// The taskbar's own auto-hide setting.
pub fn taskbar_autohide() -> bool {
    let mut data = appbar_data();
    // SAFETY: `data` is a correctly sized APPBARDATA.
    let state = crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) });
    state & (ABS_AUTOHIDE as usize) != 0
}

/// Sets the taskbar's auto-hide setting.
pub fn set_taskbar_autohide(on: bool) {
    let mut data = appbar_data();
    data.lParam = LPARAM(if on { ABS_AUTOHIDE as isize } else { 0 });
    // SAFETY: `data` is a correctly sized APPBARDATA naming the taskbar.
    crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub rect: Rect,
    pub work_area: Rect,
    pub primary: bool,
    /// `\\.\DISPLAYn`, stable while the monitor stays connected.
    pub device: String,
    /// Windows' display number (the `n` of the device name), shown as "Monitor n".
    pub number: u32,
    /// Effective DPI (96 = 100 %).
    pub dpi: u32,
    /// The `HMONITOR` value.
    pub handle: isize,
}

/// All monitors, primary first, in physical pixels.
pub fn monitors() -> Vec<Monitor> {
    crate::dpi::physical(monitors_in_current_context)
}

fn monitors_in_current_context() -> Vec<Monitor> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the address of the Vec below, alive for the whole call.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = u32::try_from(std::mem::size_of::<MONITORINFOEXW>()).unwrap_or_default();
        // SAFETY: `info` is a correctly sized MONITORINFOEXW; `h` comes from the enumeration.
        if unsafe { GetMonitorInfoW(h, &mut info.monitorInfo) }.as_bool() {
            let end = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            let device = String::from_utf16_lossy(&info.szDevice[..end]);
            let (mut dx, mut dy) = (0u32, 0u32);
            // SAFETY: valid monitor handle and out pointers.
            let dpi = match unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } {
                Ok(()) if dx > 0 => dx,
                _ => BASE_DPI,
            };
            out.push(Monitor {
                rect: rect(info.monitorInfo.rcMonitor),
                work_area: rect(info.monitorInfo.rcWork),
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                number: display_number(&device).unwrap_or(0),
                device,
                dpi,
                handle: h.0 as isize,
            });
        }
        BOOL(1)
    }
    let mut out: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only touches `out`, which outlives the call.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut out as *mut Vec<Monitor> as isize),
        )
    };
    out.sort_by_key(|m| !m.primary);
    out
}

/// Sets the work area (physical pixels) of the monitor containing `area` and tells
/// running apps, without waiting for them (a hung app cannot block this).
pub fn set_work_area(area: Rect) -> std::io::Result<()> {
    crate::dpi::physical(|| set_work_area_in_current_context(area))
}

fn set_work_area_in_current_context(area: Rect) -> std::io::Result<()> {
    let mut r = RECT {
        left: area.left,
        top: area.top,
        right: area.right,
        bottom: area.bottom,
    };
    // SAFETY: `r` is a valid RECT for the duration of the call.
    unsafe {
        SystemParametersInfoW(
            SPI_SETWORKAREA,
            0,
            Some(&mut r as *mut RECT as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .map_err(io_error)?;
    // SAFETY: SendNotifyMessage to HWND_BROADCAST queues the notification and returns at once.
    let _ = unsafe {
        SendNotifyMessageW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(SPI_SETWORKAREA.0 as usize),
            LPARAM(0),
        )
    };
    Ok(())
}

/// 100 % scaling.
pub const BASE_DPI: u32 = 96;

/// `\\.\DISPLAY2` → 2.
pub fn display_number(device: &str) -> Option<u32> {
    let digits: String = device
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

#[cfg(test)]
mod tests {
    //! Read-only checks; nothing here changes the desktop. Edge cases: exactly one
    //! primary monitor, listed first; work area inside the monitor.
    use super::*;

    #[test]
    fn monitors_have_one_primary_first() {
        let m = monitors();
        assert!(!m.is_empty());
        assert!(m[0].primary);
        assert_eq!(m.iter().filter(|m| m.primary).count(), 1);
        for mon in &m {
            assert!(mon.work_area.left >= mon.rect.left && mon.work_area.right <= mon.rect.right);
            assert!(mon.work_area.top >= mon.rect.top && mon.work_area.bottom <= mon.rect.bottom);
        }
    }

    /// Regression: an unaware caller got scaled coordinates while `kabuk.exe` (aware)
    /// applied them as physical ones, shrinking the work area on a 125 % display.
    #[test]
    fn monitors_report_physical_pixels_whatever_the_caller_awareness() {
        use windows::Win32::UI::HiDpi::{
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_UNAWARE,
        };
        let on_thread = |aware: bool| {
            std::thread::spawn(move || {
                let ctx = if aware {
                    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                } else {
                    DPI_AWARENESS_CONTEXT_UNAWARE
                };
                // SAFETY: changes the awareness of this short-lived test thread only.
                unsafe { SetThreadDpiAwarenessContext(ctx) };
                monitors()
            })
            .join()
            .unwrap()
        };
        let aware = on_thread(true);
        let unaware = on_thread(false);
        assert_eq!(aware, unaware);
    }

    #[test]
    fn monitor_details() {
        for m in monitors() {
            assert!(m.device.starts_with(r"\\.\DISPLAY"), "{}", m.device);
            assert!(m.number >= 1);
            assert!(m.dpi >= BASE_DPI);
            assert!(m.handle != 0);
        }
        assert_eq!(display_number(r"\\.\DISPLAY12"), Some(12));
        assert_eq!(display_number("nothing"), None);
    }

    #[test]
    fn taskbar_is_found() {
        // Explorer runs on the development machine and on CI runners.
        assert!(!taskbars().is_empty());
    }
}
