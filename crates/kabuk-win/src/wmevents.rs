//! Window lifecycle events for the window manager, from out-of-context WinEvents
//! (CONTRIBUTING.md rule 4: no code runs in other processes). Events arrive on the installing
//! thread's message loop; Kabuk's own windows are skipped.

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_NAMECHANGE,
    EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

use crate::window::Hwnd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    Shown(Hwnd),
    Hidden(Hwnd),
    Destroyed(Hwnd),
    Cloaked(Hwnd),
    Uncloaked(Hwnd),
    Foreground(Hwnd),
    MinimizeStart(Hwnd),
    MinimizeEnd(Hwnd),
    MoveSizeStart(Hwnd),
    MoveSizeEnd(Hwnd),
    /// Its title changed: a window ignored for having none when it showed (Unity, some
    /// Electron apps) is looked at again.
    Renamed(Hwnd),
}

/// The event ranges hooked (each `SetWinEventHook` takes a contiguous range).
const RANGES: [(u32, u32); 6] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

const FLAGS: u32 = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

type Sink = Box<dyn FnMut(WindowEvent)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

/// Turns a raw WinEvent into a [`WindowEvent`] for top-level windows only.
fn translate(event: u32, hwnd: Hwnd, object: i32, child: i32, top_level: impl Fn(Hwnd) -> bool) -> Option<WindowEvent> {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || hwnd.0 == 0 {
        return None;
    }
    let e = match event {
        EVENT_SYSTEM_FOREGROUND => WindowEvent::Foreground(hwnd),
        EVENT_SYSTEM_MOVESIZESTART => WindowEvent::MoveSizeStart(hwnd),
        EVENT_SYSTEM_MOVESIZEEND => WindowEvent::MoveSizeEnd(hwnd),
        EVENT_SYSTEM_MINIMIZESTART => WindowEvent::MinimizeStart(hwnd),
        EVENT_SYSTEM_MINIMIZEEND => WindowEvent::MinimizeEnd(hwnd),
        // A destroyed window can no longer be asked whether it was top-level.
        EVENT_OBJECT_DESTROY => return Some(WindowEvent::Destroyed(hwnd)),
        EVENT_OBJECT_SHOW => WindowEvent::Shown(hwnd),
        EVENT_OBJECT_HIDE => WindowEvent::Hidden(hwnd),
        EVENT_OBJECT_CLOAKED => WindowEvent::Cloaked(hwnd),
        EVENT_OBJECT_UNCLOAKED => WindowEvent::Uncloaked(hwnd),
        EVENT_OBJECT_NAMECHANGE => WindowEvent::Renamed(hwnd),
        _ => return None,
    };
    top_level(hwnd).then_some(e)
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    let Some(e) = translate(event, Hwnd::from_raw(hwnd), object, child, Hwnd::is_top_level) else {
        return;
    };
    crate::msgloop::contain((), || {
        SINK.with(|s| {
            // A nested event while the sink runs is dropped; the window manager re-checks
            // windows it knows when it acts on them.
            if let Ok(mut s) = s.try_borrow_mut() {
                if let Some(sink) = s.as_mut() {
                    sink(e);
                }
            }
        });
    });
}

/// Delivers [`WindowEvent`]s to a sink on this thread until dropped. One per thread.
pub struct WindowEvents {
    hooks: Vec<HWINEVENTHOOK>,
}

impl WindowEvents {
    pub fn start(sink: impl FnMut(WindowEvent) + 'static) -> std::io::Result<WindowEvents> {
        SINK.with(|s| *s.borrow_mut() = Some(Box::new(sink)));
        let mut hooks = Vec::with_capacity(RANGES.len());
        for (min, max) in RANGES {
            // SAFETY: out-of-context hook (no DLL injection); the callback only touches
            // this thread's SINK. Unhooked in Drop.
            let h = unsafe { SetWinEventHook(min, max, None, Some(on_event), 0, 0, FLAGS) };
            if h.is_invalid() {
                for h in hooks {
                    // SAFETY: unhooks hooks installed above.
                    let _ = unsafe { UnhookWinEvent(h) };
                }
                SINK.with(|s| *s.borrow_mut() = None);
                return Err(std::io::Error::other("SetWinEventHook failed"));
            }
            hooks.push(h);
        }
        Ok(WindowEvents { hooks })
    }
}

impl Drop for WindowEvents {
    fn drop(&mut self) {
        for h in self.hooks.drain(..) {
            // SAFETY: unhooks a hook installed in `start`.
            let _ = unsafe { UnhookWinEvent(h) };
        }
        SINK.with(|s| *s.borrow_mut() = None);
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: child objects and non-window objects are dropped, a null handle is
    //! dropped, destroy needs no top-level check, unknown events are dropped.
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{EVENT_OBJECT_LOCATIONCHANGE, OBJID_CARET};

    const W: Hwnd = Hwnd(0x40);

    #[test]
    fn only_top_level_window_events_pass() {
        let top = |_| true;
        let not_top = |_| false;
        let me = OBJID_WINDOW.0;
        let selfid = CHILDID_SELF as i32;
        assert_eq!(
            translate(EVENT_OBJECT_SHOW, W, me, selfid, top),
            Some(WindowEvent::Shown(W))
        );
        assert_eq!(translate(EVENT_OBJECT_SHOW, W, me, selfid, not_top), None);
        assert_eq!(translate(EVENT_OBJECT_SHOW, W, OBJID_CARET.0, selfid, top), None);
        assert_eq!(translate(EVENT_OBJECT_SHOW, W, me, 3, top), None);
        assert_eq!(translate(EVENT_OBJECT_SHOW, Hwnd(0), me, selfid, top), None);
        assert_eq!(
            translate(EVENT_OBJECT_DESTROY, W, me, selfid, not_top),
            Some(WindowEvent::Destroyed(W))
        );
        assert_eq!(translate(EVENT_OBJECT_LOCATIONCHANGE, W, me, selfid, top), None);
        for (event, expected) in [
            (EVENT_SYSTEM_FOREGROUND, WindowEvent::Foreground(W)),
            (EVENT_SYSTEM_MOVESIZESTART, WindowEvent::MoveSizeStart(W)),
            (EVENT_SYSTEM_MOVESIZEEND, WindowEvent::MoveSizeEnd(W)),
            (EVENT_SYSTEM_MINIMIZESTART, WindowEvent::MinimizeStart(W)),
            (EVENT_SYSTEM_MINIMIZEEND, WindowEvent::MinimizeEnd(W)),
            (EVENT_OBJECT_HIDE, WindowEvent::Hidden(W)),
            (EVENT_OBJECT_CLOAKED, WindowEvent::Cloaked(W)),
            (EVENT_OBJECT_UNCLOAKED, WindowEvent::Uncloaked(W)),
            (EVENT_OBJECT_NAMECHANGE, WindowEvent::Renamed(W)),
        ] {
            assert_eq!(translate(event, W, me, selfid, top), Some(expected));
        }
    }
}
