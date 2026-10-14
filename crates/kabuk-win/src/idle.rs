//! Idle dimming's Windows side (ARCHITECTURE §11): how long since the last input, whether
//! a program keeps the display on, the click-through black overlay, and telling a window
//! about the next input (raw input, only while dimmed, so nothing runs while idle).

use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SW_SHOWNOACTIVATE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::msgloop::PxRect;
use crate::sync::io_error;
use crate::window::Hwnd;

/// How long since the last keyboard or mouse input anywhere in the session.
pub fn idle_time() -> Duration {
    let mut info = LASTINPUTINFO {
        cbSize: u32::try_from(std::mem::size_of::<LASTINPUTINFO>()).unwrap_or_default(),
        dwTime: 0,
    };
    // SAFETY: `info` is a valid out struct with its size set.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return Duration::ZERO;
    }
    // SAFETY: plain call.
    let now = unsafe { GetTickCount() };
    Duration::from_millis(u64::from(now.wrapping_sub(info.dwTime)))
}

/// A program asked Windows to keep the display on (a video player, a presentation).
pub fn display_required() -> bool {
    use windows::Win32::System::Power::{CallNtPowerInformation, SystemExecutionState, EXECUTION_STATE};
    /// `ES_DISPLAY_REQUIRED`.
    const DISPLAY: u32 = 0x0000_0002;
    let mut state = EXECUTION_STATE(0);
    // SAFETY: SystemExecutionState writes one EXECUTION_STATE into the buffer given.
    let r = unsafe {
        CallNtPowerInformation(
            SystemExecutionState,
            None,
            0,
            Some((&mut state as *mut EXECUTION_STATE).cast()),
            u32::try_from(std::mem::size_of::<EXECUTION_STATE>()).unwrap_or_default(),
        )
    };
    r.is_ok() && state.0 & DISPLAY != 0
}

const OVERLAY_CLASS: PCWSTR = w!("Kabuk.Dim");

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing with the arguments we received.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A black, click-through window over `rect` at `alpha` (0-1), above everything; gone
/// when dropped.
pub struct Overlay(HWND);

impl Overlay {
    pub fn new(rect: PxRect, alpha: f32) -> std::io::Result<Overlay> {
        // SAFETY: (whole block) a plain window of our own class; the class is registered
        // once per process (a second registration fails harmlessly).
        unsafe {
            let instance = GetModuleHandleW(None).map_err(io_error)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance.into(),
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: OVERLAY_CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                OVERLAY_CLASS,
                w!(""),
                WS_POPUP,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(io_error)?;
            let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA).map_err(io_error)?;
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), rect.x, rect.y, rect.w, rect.h, SWP_NOACTIVATE);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(Overlay(hwnd))
        }
    }

    pub fn hwnd(&self) -> Hwnd {
        Hwnd::from_raw(self.0)
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // SAFETY: our own window, destroyed once on the thread that made it.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// Keyboard and mouse (HID usage page 1: mouse 2, keyboard 6).
const DEVICES: [(u16, u16); 2] = [(1, 2), (1, 6)];

/// `window` gets `WM_INPUT` for every keyboard and mouse input, even in the background
/// (`true`), or no more (`false`).
pub fn watch_input(window: Hwnd, on: bool) -> std::io::Result<()> {
    let devices: Vec<RAWINPUTDEVICE> = DEVICES
        .iter()
        .map(|&(page, usage)| RAWINPUTDEVICE {
            usUsagePage: page,
            usUsage: usage,
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if on { window.raw() } else { HWND::default() },
        })
        .collect();
    // SAFETY: a valid array of the size passed.
    unsafe {
        RegisterRawInputDevices(
            &devices,
            u32::try_from(std::mem::size_of::<RAWINPUTDEVICE>()).unwrap_or_default(),
        )
    }
    .map_err(io_error)
}

#[cfg(test)]
mod tests {
    //! Edge cases: the idle time is a plausible duration; the display state answers; an
    //! overlay is a real, click-through, topmost window that goes when dropped.
    use super::*;

    #[test]
    fn idle_time_and_display_state_answer() {
        assert!(idle_time() < Duration::from_secs(60 * 60 * 24 * 49));
        let _ = display_required();
    }

    #[test]
    fn an_overlay_is_click_through_and_goes_away() {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_EXSTYLE};
        let o = Overlay::new(
            PxRect {
                x: -32000,
                y: -32000,
                w: 10,
                h: 10,
            },
            0.45,
        )
        .unwrap();
        let h = o.hwnd();
        assert!(h.exists());
        // SAFETY: reading our own window's style.
        let ex = unsafe { GetWindowLongPtrW(h.raw(), GWL_EXSTYLE) } as u32;
        assert!(ex & WS_EX_TRANSPARENT.0 != 0 && ex & WS_EX_TOPMOST.0 != 0);
        drop(o);
        assert!(!h.exists());
    }
}
