//! Behind the desktop icons (DECISIONS D13): Explorer's `Progman` is asked (message
//! `0x052C`, undocumented, no injection) to split the wallpaper from the icons; Kabuk's
//! animated wallpaper is a layered child put between them. On Windows 11 24H2 and later
//! the icons (`SHELLDLL_DefView`) and the wallpaper (`WorkerW`) are children of
//! `Progman`; before, a top-level `WorkerW` behind the one holding the icons takes the
//! window. Proven by `examples/workerw_spike.rs`.

use std::time::Duration;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, SendMessageTimeoutW, SetWindowPos, SMTO_NORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

use crate::window::Hwnd;

/// `Progman`'s private message that splits the wallpaper from the icons.
const SPAWN_WORKERW: u32 = 0x052C;
const SPAWN_WPARAM: usize = 0xD;
const SPAWN_LPARAM: isize = 1;
/// Explorer gets this long to answer; a hung Explorer cannot hang Kabuk.
const ANSWER: Duration = Duration::from_millis(1000);

fn find(parent: Option<HWND>, after: Option<HWND>, class: &str) -> Option<HWND> {
    // SAFETY: plain lookup.
    unsafe { FindWindowExW(parent, after, &HSTRING::from(class), PCWSTR::null()) }
        .ok()
        .filter(|h| !h.is_invalid())
}

/// Explorer's desktop window (the wallpaper and the icons, and Kabuk's animated wallpaper
/// among them), when Explorer runs.
pub(crate) fn progman() -> Option<Hwnd> {
    find(None, None, "Progman").map(Hwnd::from_raw)
}

/// Where the wallpaper window goes: its parent, and the window it sits right behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Host {
    pub parent: Hwnd,
    pub behind: Option<Hwnd>,
    /// The 24H2 structure (children of `Progman`).
    pub progman: bool,
}

/// Asks Explorer for the place behind the icons; `None` when there is none (Explorer not
/// running, or its windows changed beyond the two structures known).
pub fn host() -> Option<Host> {
    let progman = find(None, None, "Progman")?;
    let mut answer = 0usize;
    // SAFETY: a message to Explorer's window with a timeout.
    unsafe {
        let _ = SendMessageTimeoutW(
            progman,
            SPAWN_WORKERW,
            WPARAM(SPAWN_WPARAM),
            LPARAM(SPAWN_LPARAM),
            SMTO_NORMAL,
            u32::try_from(ANSWER.as_millis()).unwrap_or(u32::MAX),
            Some(&mut answer),
        );
    }
    if let (Some(view), Some(_)) = (
        find(Some(progman), None, "SHELLDLL_DefView"),
        find(Some(progman), None, "WorkerW"),
    ) {
        return Some(Host {
            parent: Hwnd::from_raw(progman),
            behind: Some(Hwnd::from_raw(view)),
            progman: true,
        });
    }
    let mut w = None;
    while let Some(next) = find(None, w, "WorkerW") {
        if find(Some(next), None, "SHELLDLL_DefView").is_some() {
            return find(None, Some(next), "WorkerW").map(|ww| Host {
                parent: Hwnd::from_raw(ww),
                behind: None,
                progman: false,
            });
        }
        w = Some(next);
    }
    None
}

/// Puts `child` (a child of the host) right behind the icons.
pub fn place_behind(child: Hwnd, host: &Host) {
    if let Some(view) = host.behind {
        // SAFETY: z-order change of our own child window.
        let _ = unsafe {
            SetWindowPos(
                child.raw(),
                Some(view.raw()),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
    }
}

#[cfg(test)]
mod tests {
    //! Edge case: on this machine Explorer answers with a host (it runs during tests).
    use super::*;

    #[test]
    fn explorer_has_a_place_behind_the_icons() {
        let h = host().expect("Explorer's desktop");
        assert!(h.parent.exists());
        if h.progman {
            assert!(h.behind.is_some_and(|v| v.exists()));
        }
    }
}
