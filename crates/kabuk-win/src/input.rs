//! Mouse input for grabbing windows (`kabuk_core::grab`): a low-level mouse hook
//! (`WH_MOUSE_LL`), the modifier keys, the window under the cursor and Alt masking.
//!
//! A low-level hook loads no code into other processes (CONTRIBUTING.md rule 4): Windows calls
//! it on the installing thread, which must pump messages and answer quickly, or every
//! mouse movement on the machine waits for it. That thread therefore does nothing else.

use std::cell::RefCell;
use std::time::Duration;

use kabuk_core::grab::{Button, Keys, Point};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LMENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetAncestor, GetSystemMetrics, SendMessageTimeoutW, SetWindowsHookExW, UnhookWindowsHookEx,
    WindowFromPoint, GA_ROOT, HC_ACTION, HHOOK, HTCAPTION, LLMHF_INJECTED, MSLLHOOKSTRUCT, SMTO_ABORTIFHUNG,
    SM_CXDOUBLECLK, SM_CYDOUBLECLK, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_RBUTTONDOWN, WM_RBUTTONUP,
};

use crate::window::Hwnd;

/// An unassigned virtual key (`0xE8`), pressed to keep a released Alt from opening the
/// program's menu, as AutoHotkey's "mask key" does.
const MASK_KEY: u16 = 0xE8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down(Button),
    Up(Button),
    Move,
}

/// One mouse event as the hook sees it, in physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseInput {
    pub kind: MouseKind,
    pub at: Point,
    /// Milliseconds (the hook's clock, wraps around).
    pub time: u32,
}

type Sink = Box<dyn FnMut(MouseInput) -> bool>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

fn kind(message: u32) -> Option<MouseKind> {
    Some(match message {
        WM_LBUTTONDOWN => MouseKind::Down(Button::Left),
        WM_LBUTTONUP => MouseKind::Up(Button::Left),
        WM_RBUTTONDOWN => MouseKind::Down(Button::Right),
        WM_RBUTTONUP => MouseKind::Up(Button::Right),
        WM_MOUSEMOVE => MouseKind::Move,
        _ => return None,
    })
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to the event's MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        // Input made by programs (remote tools, automation) is left alone.
        let injected = info.flags & LLMHF_INJECTED != 0;
        let event = kind(wparam.0 as u32).filter(|_| !injected).map(|kind| MouseInput {
            kind,
            at: Point {
                x: info.pt.x,
                y: info.pt.y,
            },
            time: info.time,
        });
        if let Some(e) = event {
            let swallow = crate::msgloop::contain(false, || {
                SINK.with(|s| match s.try_borrow_mut() {
                    Ok(mut s) => s.as_mut().is_some_and(|sink| sink(e)),
                    Err(_) => false,
                })
            });
            if swallow {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on unchanged, as the hook contract requires.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The low-level mouse hook of this thread, until dropped. `sink` returns true to keep
/// an event from the program under the cursor (moves should never be kept). One per
/// thread; the thread must run a message loop.
pub struct MouseHook {
    hook: HHOOK,
}

impl MouseHook {
    pub fn start(sink: impl FnMut(MouseInput) -> bool + 'static) -> std::io::Result<MouseHook> {
        SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
        // SAFETY: a low-level hook needs no module and runs on this thread; the callback
        // only touches this thread's SINK. Unhooked in Drop.
        match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), None, 0) } {
            Ok(hook) => Ok(MouseHook { hook }),
            Err(e) => {
                SINK.with(|s| *s.borrow_mut() = None);
                Err(crate::sync::io_error(e))
            }
        }
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        // SAFETY: unhooks the hook installed in `start`.
        let _ = unsafe { UnhookWindowsHookEx(self.hook) };
        SINK.with(|s| *s.borrow_mut() = None);
    }
}

/// The modifier keys now (the physical state, as a hook sees it).
/// Where the mouse cursor is, in screen pixels (this thread's DPI awareness).
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = windows::Win32::Foundation::POINT::default();
    // SAFETY: a valid out pointer.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) }
        .ok()
        .map(|()| (p.x, p.y))
}

/// Hides or shows the mouse cursor over this program's windows (the screen saver).
pub fn show_cursor(show: bool) {
    // SAFETY: plain call; the display counter is this thread's.
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::ShowCursor(show) };
}

pub fn keys() -> Keys {
    // SAFETY: GetAsyncKeyState only reads the key state.
    let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0;
    Keys {
        left_alt: down(VK_LMENU),
        ctrl: down(VK_CONTROL),
    }
}

/// The virtual keys of the keyboard (mouse buttons, 1 to 6, left out).
const KEYBOARD_KEYS: std::ops::RangeInclusive<u16> = 0x08..=0xFE;

/// Whether any key of the keyboard is held down now.
pub fn any_key_down() -> bool {
    // SAFETY: GetAsyncKeyState only reads the key state.
    KEYBOARD_KEYS
        .into_iter()
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk)) } < 0)
}

/// The top-level window under a screen point.
pub fn root_window_at(at: Point) -> Option<Hwnd> {
    // SAFETY: plain value arguments; no messages are sent to other threads' windows.
    let h = unsafe { WindowFromPoint(POINT { x: at.x, y: at.y }) };
    if h.is_invalid() {
        return None;
    }
    // SAFETY: GetAncestor on a window handle we just got; an invalid one gives null.
    let root = unsafe { GetAncestor(h, GA_ROOT) };
    (!root.is_invalid()).then(|| Hwnd::from_raw(root))
}

/// Whether a screen point (physical pixels) is on the window's title bar, by asking the
/// window (`WM_NCHITTEST`) for at most `timeout`; a hung window answers no. The caller's
/// thread must be DPI aware: Windows then translates the point for a window that is not.
pub fn on_caption(window: Hwnd, at: Point, timeout: Duration) -> bool {
    let lparam = ((at.y as u16 as isize) << 16) | (at.x as u16 as isize);
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let mut result = 0usize;
    // SAFETY: plain values in wparam/lparam; `result` is a valid out pointer.
    let answered = unsafe {
        SendMessageTimeoutW(
            window.raw(),
            WM_NCHITTEST,
            WPARAM(0),
            LPARAM(lparam),
            SMTO_ABORTIFHUNG,
            ms,
            Some(&mut result),
        )
    };
    answered.0 != 0 && result == HTCAPTION as usize
}

/// Windows' double-click time (ms) and rectangle (width, height).
pub fn double_click() -> (u32, (i32, i32)) {
    // SAFETY: these only read system settings.
    unsafe {
        (
            GetDoubleClickTime(),
            (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)),
        )
    }
}

/// Presses and releases [`MASK_KEY`], so that releasing a held Alt does not open the
/// program's menu.
pub fn mask_alt() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_KEY),
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    // SAFETY: two fully initialised keyboard inputs and their size.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        tracing::debug!(sent, "mask key not sent");
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the hook installs and uninstalls (no input is sent, CONTRIBUTING.md test
    //! rule); messages the hook ignores; the double-click metrics are sane; a point off
    //! every window. `on_caption` is tried on a real window in `apps/kabuk/tests/wm.rs`.
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEWHEEL, WM_XBUTTONDOWN};

    #[test]
    fn the_hook_installs_and_goes_away() {
        let hook = MouseHook::start(|_| false).unwrap();
        assert!(SINK.with(|s| s.borrow().is_some()));
        drop(hook);
        assert!(SINK.with(|s| s.borrow().is_none()));
    }

    #[test]
    fn only_buttons_and_moves_reach_the_sink() {
        assert_eq!(kind(WM_LBUTTONDOWN), Some(MouseKind::Down(Button::Left)));
        assert_eq!(kind(WM_RBUTTONUP), Some(MouseKind::Up(Button::Right)));
        assert_eq!(kind(WM_MOUSEMOVE), Some(MouseKind::Move));
        assert_eq!(kind(WM_MOUSEWHEEL), None);
        assert_eq!(kind(WM_XBUTTONDOWN), None);
    }

    #[test]
    fn double_click_metrics_are_sane() {
        let (ms, (w, h)) = double_click();
        assert!((100..=5000).contains(&ms), "{ms}");
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn no_window_far_off_screen() {
        assert_eq!(
            root_window_at(Point {
                x: -100_000,
                y: -100_000
            }),
            None
        );
    }
}
