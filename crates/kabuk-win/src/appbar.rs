//! Reserving screen space for the bar with the shell's AppBar protocol
//! (`SHAppBarMessage`): maximised windows then stop at the bar's edge (ARCHITECTURE §5).
//!
//! The work area changes when a bar is placed, so the caller records the monitor's
//! work area in the state file before the first [`AppBar::place`] (CONTRIBUTING.md rule 3).

use kabuk_core::config::Edge;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_ACTIVATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, ABM_WINDOWPOSCHANGED, APPBARDATA,
};

use crate::msgloop::{PxRect, Window};
use crate::window::Hwnd;

/// Notifications the shell sends with the AppBar callback message (in `wparam`).
pub mod notify {
    pub const STATE_CHANGE: usize = 0;
    pub const POS_CHANGED: usize = 1;
    pub const FULLSCREEN_APP: usize = 2;
    pub const WINDOW_ARRANGE: usize = 3;
}

/// A registered AppBar; removed on drop, which gives the space back.
pub struct AppBar {
    hwnd: Hwnd,
    registered: bool,
}

fn data(hwnd: Hwnd) -> APPBARDATA {
    APPBARDATA {
        cbSize: u32::try_from(std::mem::size_of::<APPBARDATA>()).unwrap_or_default(),
        hWnd: hwnd.raw(),
        ..Default::default()
    }
}

fn edge_code(edge: Edge) -> u32 {
    match edge {
        Edge::Top => ABE_TOP,
        Edge::Bottom => ABE_BOTTOM,
        Edge::Left => ABE_LEFT,
        Edge::Right => ABE_RIGHT,
    }
}

/// The strip of `monitor` that a bar of `thickness` px takes on `edge`.
pub fn strip(edge: Edge, monitor: PxRect, thickness: i32) -> PxRect {
    match edge {
        Edge::Top => PxRect {
            h: thickness,
            ..monitor
        },
        Edge::Bottom => PxRect {
            y: monitor.y + monitor.h - thickness,
            h: thickness,
            ..monitor
        },
        Edge::Left => PxRect {
            w: thickness,
            ..monitor
        },
        Edge::Right => PxRect {
            x: monitor.x + monitor.w - thickness,
            w: thickness,
            ..monitor
        },
    }
}

fn to_rect(r: PxRect) -> RECT {
    RECT {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}

fn from_rect(r: RECT) -> PxRect {
    PxRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

impl AppBar {
    /// Registers `window` as an AppBar; the shell sends `callback_message` to it.
    pub fn register(window: &Window, callback_message: u32) -> std::io::Result<AppBar> {
        let mut d = data(window.hwnd());
        d.uCallbackMessage = callback_message;
        // SAFETY: `d` is a correctly sized APPBARDATA naming our window.
        let ok = crate::dpi::physical(|| crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_NEW, &mut d) }));
        if ok == 0 {
            return Err(std::io::Error::other("ABM_NEW refused (already registered?)"));
        }
        Ok(AppBar {
            hwnd: window.hwnd(),
            registered: true,
        })
    }

    /// Reserves `thickness` px on `edge` of `monitor` and returns the rectangle the
    /// bar window must take (the shell may move it off other AppBars such as the taskbar).
    pub fn place(&self, edge: Edge, monitor: PxRect, thickness: i32) -> PxRect {
        crate::dpi::physical(|| {
            let mut d = data(self.hwnd);
            d.uEdge = edge_code(edge);
            d.rc = to_rect(strip(edge, monitor, thickness));
            // SAFETY: `d` is valid; the shell adjusts `rc` in place.
            crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_QUERYPOS, &mut d) });
            let proposed = from_rect(d.rc);
            // Keep the requested thickness from the edge the shell settled on.
            let fixed = match edge {
                Edge::Top => PxRect {
                    h: thickness,
                    ..proposed
                },
                Edge::Bottom => PxRect {
                    y: proposed.y + proposed.h - thickness,
                    h: thickness,
                    ..proposed
                },
                Edge::Left => PxRect {
                    w: thickness,
                    ..proposed
                },
                Edge::Right => PxRect {
                    x: proposed.x + proposed.w - thickness,
                    w: thickness,
                    ..proposed
                },
            };
            d.rc = to_rect(fixed);
            // SAFETY: as above.
            crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_SETPOS, &mut d) });
            from_rect(d.rc)
        })
    }

    /// Tells the shell the bar window moved or was activated (WM_WINDOWPOSCHANGED / WM_ACTIVATE).
    pub fn window_pos_changed(&self) {
        let mut d = data(self.hwnd);
        // SAFETY: `d` is valid.
        crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut d) });
    }

    pub fn activated(&self) {
        let mut d = data(self.hwnd);
        d.lParam = LPARAM(1);
        // SAFETY: `d` is valid.
        crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_ACTIVATE, &mut d) });
    }

    /// Gives the space back now (also done on drop).
    pub fn remove(&mut self) {
        if self.registered {
            let mut d = data(self.hwnd);
            // SAFETY: `d` is valid.
            crate::dpi::physical(|| crate::tray::explorer_first(|| unsafe { SHAppBarMessage(ABM_REMOVE, &mut d) }));
            self.registered = false;
        }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        self.remove();
    }
}

/// Where an auto-hiding bar's window is when `shown` of it (0 hidden, 1 all) is on screen:
/// `rect` (its place when shown) slid off over `edge`, with at least `peek` px left on
/// screen for the mouse to reach (SPEC §2.1).
pub fn slid(edge: Edge, rect: PxRect, peek: i32, shown: f32) -> PxRect {
    let size = match edge {
        Edge::Top | Edge::Bottom => rect.h,
        Edge::Left | Edge::Right => rect.w,
    };
    let hidden = (size - peek.min(size)).max(0);
    let off = (hidden as f32 * (1.0 - shown.clamp(0.0, 1.0))).round() as i32;
    match edge {
        Edge::Top => PxRect {
            y: rect.y - off,
            ..rect
        },
        Edge::Bottom => PxRect {
            y: rect.y + off,
            ..rect
        },
        Edge::Left => PxRect {
            x: rect.x - off,
            ..rect
        },
        Edge::Right => PxRect {
            x: rect.x + off,
            ..rect
        },
    }
}

#[cfg(test)]
mod tests {
    //! Pure geometry only; registering an AppBar changes the desktop and is covered by
    //! the integration tests. Edge cases: each edge, monitors at negative coordinates; an
    //! auto-hiding bar shown, half shown and hidden on each edge, a peek larger than the
    //! bar.
    use super::*;

    #[test]
    fn an_auto_hiding_bar_slides_off_its_edge_and_leaves_a_peek() {
        let m = PxRect {
            x: -2560,
            y: 0,
            w: 2560,
            h: 1440,
        };
        let peek = 2;
        for (edge, off) in [
            (Edge::Top, (0, -(48 - peek))),
            (Edge::Bottom, (0, 48 - peek)),
            (Edge::Left, (-(48 - peek), 0)),
            (Edge::Right, (48 - peek, 0)),
        ] {
            let r = strip(edge, m, 48);
            assert_eq!(slid(edge, r, peek, 1.0), r, "{edge:?} shown");
            let h = slid(edge, r, peek, 0.0);
            assert_eq!(
                (h.x - r.x, h.y - r.y, h.w, h.h),
                (off.0, off.1, r.w, r.h),
                "{edge:?} hidden"
            );
            let half = slid(edge, r, peek, 0.5);
            assert_eq!((half.x - r.x, half.y - r.y), (off.0 / 2, off.1 / 2), "{edge:?} half");
        }
        let r = strip(Edge::Top, m, 48);
        assert_eq!(slid(Edge::Top, r, 100, 0.0), r, "a peek larger than the bar");
    }

    #[test]
    fn strips_per_edge() {
        let m = PxRect {
            x: -2560,
            y: 0,
            w: 2560,
            h: 1440,
        };
        assert_eq!(
            strip(Edge::Top, m, 83),
            PxRect {
                x: -2560,
                y: 0,
                w: 2560,
                h: 83
            }
        );
        assert_eq!(
            strip(Edge::Bottom, m, 83),
            PxRect {
                x: -2560,
                y: 1357,
                w: 2560,
                h: 83
            }
        );
        assert_eq!(
            strip(Edge::Left, m, 103),
            PxRect {
                x: -2560,
                y: 0,
                w: 103,
                h: 1440
            }
        );
        assert_eq!(
            strip(Edge::Right, m, 103),
            PxRect {
                x: -103,
                y: 0,
                w: 103,
                h: 1440
            }
        );
    }
}
