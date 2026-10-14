//! Keeps Explorer's taskbars hidden while Kabuk runs: Explorer shows them again on its
//! own (after the auto-hide setting changes, when the mouse touches the screen edge), so
//! every time one is shown it is hidden again. An out-of-context WinEvent hook limited to
//! Explorer's process (CONTRIBUTING.md rule 4). A taskbar is known by its class, not by the
//! windows found at the start: Explorer makes a monitor's taskbar anew after sleep or a
//! display change (reported by the user: Monitor 2's showed at the screen's edge).

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{CHILDID_SELF, EVENT_OBJECT_SHOW, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT};

use crate::window::Hwnd;

const TRAY_CLASS: &str = "Shell_TrayWnd";
const TASKBAR_CLASSES: [&str; 2] = [TRAY_CLASS, "Shell_SecondaryTrayWnd"];

/// Whether a window of Explorer's with this class is one of its taskbars.
fn is_taskbar(class: &str) -> bool {
    TASKBAR_CLASSES.contains(&class)
}

struct Guarded {
    bars: Vec<Hwnd>,
    front: Option<Hwnd>,
    /// Told (window, message) when `front` was found behind Explorer's tray window.
    behind: Option<(Hwnd, u32)>,
}

thread_local! {
    /// The taskbars being kept hidden on this thread, and the window kept in front of
    /// Explorer's tray (Kabuk's tray: programs pick the topmost `Shell_TrayWnd`).
    static TASKBARS: RefCell<Guarded> = const { RefCell::new(Guarded { bars: Vec::new(), front: None, behind: None }) };
}

unsafe extern "system" fn on_show(_: HWINEVENTHOOK, _: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    crate::msgloop::contain((), || guard(Hwnd::from_raw(hwnd)));
}

fn guard(h: Hwnd) {
    let guarded = TASKBARS.with(|t| {
        let t = t.borrow();
        // Only while guarding (the hook is Explorer's process only, so its class says it).
        (!t.bars.is_empty() && (t.bars.contains(&h) || is_taskbar(&h.class_name()))).then_some((t.front, t.behind))
    });
    let Some((front, behind)) = guarded else { return };
    h.hide_async();
    // Showing put Explorer's tray window on top; put Kabuk's back in front, and say so if
    // it was behind (icons added meanwhile went to Explorer only).
    if let Some(f) = front {
        let first = crate::window::windows_of_class(TRAY_CLASS).first().copied();
        crate::msgloop::raise_topmost(f);
        if first != Some(f) {
            if let Some((to, id)) = behind {
                let _ = crate::msgloop::post_message(
                    to,
                    crate::msgloop::Msg {
                        id,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            }
        }
    }
}

/// Hides the taskbars now and whenever Explorer shows them again, until dropped; each
/// time, `keep_in_front` (Kabuk's tray window) goes back in front of Explorer's, and
/// `behind` gets a message if it had fallen behind.
pub struct TaskbarGuard {
    hook: HWINEVENTHOOK,
}

impl TaskbarGuard {
    pub fn start(keep_in_front: Option<Hwnd>, behind: Option<(Hwnd, u32)>) -> std::io::Result<TaskbarGuard> {
        let bars = crate::shell::taskbars();
        let Some(pid) = bars.first().and_then(|b| b.pid()) else {
            return Err(std::io::Error::other("no taskbar found"));
        };
        TASKBARS.with(|t| {
            *t.borrow_mut() = Guarded {
                bars: bars.clone(),
                front: keep_in_front,
                behind,
            }
        });
        // SAFETY: out-of-context hook (no DLL injection) for Explorer's process only; the
        // callback reads this thread's TASKBARS. Unhooked in Drop.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(on_show),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return Err(std::io::Error::other("SetWinEventHook failed"));
        }
        for b in &bars {
            b.hide_async();
        }
        if let Some(f) = keep_in_front {
            crate::msgloop::raise_topmost(f);
        }
        Ok(TaskbarGuard { hook })
    }
}

impl TaskbarGuard {
    /// Hides every taskbar there is now (a monitor came or went: Explorer may have made
    /// new ones while they were not shown).
    pub fn hide_all(&self) {
        let bars = crate::shell::taskbars();
        for b in &bars {
            b.hide_async();
        }
        TASKBARS.with(|t| {
            let mut t = t.borrow_mut();
            for b in bars {
                if !t.bars.contains(&b) {
                    t.bars.push(b);
                }
            }
        });
    }

    /// Changes the window kept in front of Explorer's tray (Kabuk's tray started later).
    pub fn keep_in_front(&self, front: Option<Hwnd>) {
        TASKBARS.with(|t| t.borrow_mut().front = front);
        if let Some(f) = front {
            crate::msgloop::raise_topmost(f);
        }
    }
}

impl Drop for TaskbarGuard {
    fn drop(&mut self) {
        // SAFETY: unhooks the hook installed in `start`.
        let _ = unsafe { UnhookWinEvent(self.hook) };
        TASKBARS.with(|t| {
            *t.borrow_mut() = Guarded {
                bars: Vec::new(),
                front: None,
                behind: None,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: both of Explorer's taskbar classes are taskbars; its other windows
    //! (the desktop, a folder) are not.
    use super::*;

    #[test]
    fn explorers_taskbars_are_known_by_class() {
        assert!(is_taskbar("Shell_TrayWnd") && is_taskbar("Shell_SecondaryTrayWnd"));
        for other in ["Progman", "WorkerW", "CabinetWClass", "Shell_TrayWndX", ""] {
            assert!(!is_taskbar(other), "{other}");
        }
    }
}
