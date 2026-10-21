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
    GetForegroundWindow, GetWindowThreadProcessId, CHILDID_SELF, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MENUPOPUPSTART,
    EVENT_SYSTEM_MINIMIZESTART, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
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

type GoneSink = Box<dyn FnMut(Hwnd)>;

thread_local! {
    /// The windows watched and what to call when one goes.
    static GONE: RefCell<Option<(Vec<Hwnd>, GoneSink)>> = const { RefCell::new(None) };
}

/// Calls the sink when one of some windows is destroyed, hidden or minimised
/// (`EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`, `EVENT_SYSTEM_MINIMIZESTART`), from hooks
/// limited to their processes, until dropped: the games holding game mode, so their going
/// ends it at once whatever comes in front after (DECISIONS D87). One per thread: drop
/// the old watch before starting the next.
pub struct GoneWatch {
    hooks: Vec<HWINEVENTHOOK>,
}

unsafe extern "system" fn on_gone(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || event == EVENT_OBJECT_SHOW {
        return;
    }
    let h = Hwnd::from_raw(hwnd);
    crate::msgloop::contain((), || {
        GONE.with(|g| {
            let Ok(mut g) = g.try_borrow_mut() else { return };
            if let Some((windows, sink)) = g.as_mut() {
                if windows.contains(&h) {
                    sink(h);
                }
            }
        });
    });
}

impl GoneWatch {
    pub fn start(windows: Vec<Hwnd>, sink: impl FnMut(Hwnd) + 'static) -> std::io::Result<GoneWatch> {
        let mut pids: Vec<u32> = windows.iter().filter_map(|w| w.pid()).collect();
        pids.sort_unstable();
        pids.dedup();
        let mut hooks = Vec::new();
        for pid in pids {
            for (first, last) in [
                (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZESTART),
            ] {
                // SAFETY: out-of-context hook limited to one process (no DLL injection);
                // the callback only touches this thread's GONE. Unhooked in Drop.
                let h = unsafe { SetWinEventHook(first, last, None, Some(on_gone), pid, 0, FLAGS) };
                if h.is_invalid() {
                    // Whatever was hooked goes with the watch.
                    drop(GoneWatch { hooks });
                    return Err(std::io::Error::other("SetWinEventHook failed"));
                }
                hooks.push(h);
            }
        }
        GONE.with(|g| *g.borrow_mut() = Some((windows, Box::new(sink))));
        Ok(GoneWatch { hooks })
    }
}

impl Drop for GoneWatch {
    fn drop(&mut self) {
        for h in self.hooks.drain(..) {
            // SAFETY: unhooks a hook installed in `start`.
            let _ = unsafe { UnhookWinEvent(h) };
        }
        GONE.with(|g| {
            if let Ok(mut g) = g.try_borrow_mut() {
                *g = None;
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
