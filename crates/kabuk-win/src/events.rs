//! The focused window and its title, and one process's popup menus, from out-of-context
//! WinEvents (no code runs in other processes: CONTRIBUTING.md rule 4). Events arrive on
//! the installing thread's message loop.
//!
//! Title changes are watched only for the focused window's process: a system-wide
//! name-change hook would wake Kabuk for every title change of every window (busy
//! terminals and browsers send hundreds per second).

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, CHILDID_SELF, EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MENUPOPUPSTART, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

use crate::window::Hwnd;

type Sink = Box<dyn FnMut(Option<Hwnd>)>;

struct Watch {
    sink: Sink,
    /// The name-change hook for the focused window's process, and that process id.
    title_hook: Option<(HWINEVENTHOOK, u32)>,
}

thread_local! {
    static WATCH: RefCell<Option<Watch>> = const { RefCell::new(None) };
}

/// The current foreground window, if any.
pub fn foreground() -> Option<Hwnd> {
    // SAFETY: plain query.
    let h = unsafe { GetForegroundWindow() };
    (!h.is_invalid()).then(|| Hwnd::from_raw(h))
}

/// Calls the sink whenever the foreground window changes or retitles itself. One per thread.
pub struct ForegroundWatch {
    foreground_hook: HWINEVENTHOOK,
}

const FLAGS: u32 = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    let fg = foreground();
    if event == EVENT_OBJECT_NAMECHANGE && fg != Some(Hwnd::from_raw(hwnd)) {
        return;
    }
    crate::msgloop::contain((), || {
        WATCH.with(|w| {
            let Ok(mut w) = w.try_borrow_mut() else { return };
            let Some(w) = w.as_mut() else { return };
            if event == EVENT_SYSTEM_FOREGROUND {
                follow_titles(w, fg);
            }
            (w.sink)(fg);
        });
    });
}

/// Moves the name-change hook to the process owning `fg`.
fn follow_titles(w: &mut Watch, fg: Option<Hwnd>) {
    let pid = fg.and_then(|h| {
        let mut pid = 0u32;
        // SAFETY: valid out pointer; an invalid window yields 0.
        unsafe { GetWindowThreadProcessId(h.raw(), Some(&mut pid)) };
        (pid != 0).then_some(pid)
    });
    if w.title_hook.map(|(_, p)| Some(p)) == Some(pid) {
        return;
    }
    if let Some((h, _)) = w.title_hook.take() {
        // SAFETY: unhooks a hook installed below.
        let _ = unsafe { UnhookWinEvent(h) };
    }
    if let Some(pid) = pid {
        // SAFETY: out-of-context hook limited to one process; unhooked when focus moves or on drop.
        let h = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_NAMECHANGE,
                EVENT_OBJECT_NAMECHANGE,
                None,
                Some(on_event),
                pid,
                0,
                FLAGS,
            )
        };
        if !h.is_invalid() {
            w.title_hook = Some((h, pid));
        }
    }
}

impl ForegroundWatch {
    pub fn start(sink: impl FnMut(Option<Hwnd>) + 'static) -> std::io::Result<ForegroundWatch> {
        // SAFETY: out-of-context hook (no DLL injection); the callback only touches this
        // thread's WATCH. Unhooked in Drop.
        let foreground_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_event),
                0,
                0,
                FLAGS,
            )
        };
        if foreground_hook.is_invalid() {
            return Err(std::io::Error::other("SetWinEventHook failed"));
        }
        WATCH.with(|w| {
            let mut watch = Watch {
                sink: Box::new(sink),
                title_hook: None,
            };
            follow_titles(&mut watch, foreground());
            *w.borrow_mut() = Some(watch);
        });
        Ok(ForegroundWatch { foreground_hook })
    }
}

type MenuSink = Box<dyn FnMut(Hwnd)>;

thread_local! {
    static MENU_SINK: RefCell<Option<MenuSink>> = const { RefCell::new(None) };
}

/// Calls the sink with each popup menu one process opens (`EVENT_SYSTEM_MENUPOPUPSTART`),
/// until dropped: set up for a tray icon's program just before a click reaches it. One
/// per thread.
pub struct MenuWatch {
    hook: HWINEVENTHOOK,
}

unsafe extern "system" fn on_menu(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    crate::msgloop::contain((), || {
        MENU_SINK.with(|s| {
            let Ok(mut s) = s.try_borrow_mut() else { return };
            if let Some(sink) = s.as_mut() {
                sink(Hwnd::from_raw(hwnd));
            }
        });
    });
}

impl MenuWatch {
    pub fn start(pid: u32, sink: impl FnMut(Hwnd) + 'static) -> std::io::Result<MenuWatch> {
        // SAFETY: out-of-context hook limited to one process (no DLL injection); the
        // callback only touches this thread's MENU_SINK. Unhooked in Drop.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MENUPOPUPSTART,
                EVENT_SYSTEM_MENUPOPUPSTART,
                None,
                Some(on_menu),
                pid,
                0,
                FLAGS,
            )
        };
        if hook.is_invalid() {
            return Err(std::io::Error::other("SetWinEventHook failed"));
        }
        MENU_SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
        Ok(MenuWatch { hook })
    }
}

impl Drop for MenuWatch {
    fn drop(&mut self) {
        // SAFETY: unhooks the hook installed in `start`.
        let _ = unsafe { UnhookWinEvent(self.hook) };
        MENU_SINK.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                *s = None;
            }
        });
    }
}

impl Drop for ForegroundWatch {
    fn drop(&mut self) {
        // SAFETY: unhooks the hook installed in `start`.
        let _ = unsafe { UnhookWinEvent(self.foreground_hook) };
        WATCH.with(|w| {
            if let Some(watch) = w.borrow_mut().take() {
                if let Some((h, _)) = watch.title_hook {
                    // SAFETY: unhooks the per-process hook.
                    let _ = unsafe { UnhookWinEvent(h) };
                }
            }
        });
    }
}
