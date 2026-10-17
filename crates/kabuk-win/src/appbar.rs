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

/// Where an auto-hiding window that floats in from its edge (Rıhtım's dock, a distance
/// off the edge; DECISIONS D70) is when `shown` of it is on screen: `rect` (its place
/// when shown, inside `monitor`) slid over `edge` until only `peek` px of it are left at
/// the monitor's edge for the mouse to reach.
pub fn slid_to_edge(edge: Edge, rect: PxRect, monitor: PxRect, peek: i32, shown: f32) -> PxRect {
    let away = 1.0 - shown.clamp(0.0, 1.0);
    let toward = |from: i32, to: i32| from + ((to - from) as f32 * away).round() as i32;
    match edge {
        Edge::Top => PxRect {
            y: toward(rect.y, monitor.y + peek - rect.h),
            ..rect
        },
        Edge::Bottom => PxRect {
            y: toward(rect.y, monitor.y + monitor.h - peek),
            ..rect
        },
        Edge::Left => PxRect {
            x: toward(rect.x, monitor.x + peek - rect.w),
            ..rect
        },
        Edge::Right => PxRect {
            x: toward(rect.x, monitor.x + monitor.w - peek),
            ..rect
        },
    }
}

/// The work area `monitor` will have once a bar of `thickness` px on `edge` reserves its
/// strip (`reserved`) or gives it back, from `work`, the work area now: reserving cuts
/// the strip off it, giving back lets it reach the monitor's edge again when it ended at
/// the strip. Said ahead, so the windows can slide as the bar does (DECISIONS D75); the
/// shell's own answer comes a moment later and wins.
pub fn work_area_with(edge: Edge, monitor: PxRect, work: PxRect, thickness: i32, reserved: bool) -> PxRect {
    let (mut l, mut t, mut r, mut b) = (work.x, work.y, work.x + work.w, work.y + work.h);
    let (ml, mt, mr, mb) = (monitor.x, monitor.y, monitor.x + monitor.w, monitor.y + monitor.h);
    match (edge, reserved) {
        (Edge::Top, true) => t = t.max(mt + thickness),
        (Edge::Bottom, true) => b = b.min(mb - thickness),
        (Edge::Left, true) => l = l.max(ml + thickness),
        (Edge::Right, true) => r = r.min(mr - thickness),
        (Edge::Top, false) if t == mt + thickness => t = mt,
        (Edge::Bottom, false) if b == mb - thickness => b = mb,
        (Edge::Left, false) if l == ml + thickness => l = ml,
        (Edge::Right, false) if r == mr - thickness => r = mr,
        _ => {}
    }
    PxRect {
        x: l,
        y: t,
        w: r - l,
        h: b - t,
    }
}

#[cfg(test)]
mod tests {
    //! Pure geometry only; registering an AppBar changes the desktop and is covered by
    //! the integration tests. Edge cases: each edge, monitors at negative coordinates; an
    //! auto-hiding bar shown, half shown and hidden on each edge, a peek larger than the
    //! bar; the work area said ahead when it is already cut, or not ours to give back.
    use super::*;

    /// DECISIONS D75: the work area an auto-hiding bar's reservation leaves, said before
    /// the shell changes it, on each edge of Monitor 2 left of the main one; reserving
    /// twice cuts once, and giving back leaves a work area that does not end at the strip
    /// (another bar's) alone.
    #[test]
    fn the_work_area_a_reservation_leaves_is_known_ahead() {
        let m = PxRect {
            x: -2560,
            y: 0,
            w: 2560,
            h: 1440,
        };
        for (edge, cut) in [
            (Edge::Top, PxRect { y: 48, h: 1392, ..m }),
            (Edge::Bottom, PxRect { h: 1392, ..m }),
            (Edge::Left, PxRect { x: -2512, w: 2512, ..m }),
            (Edge::Right, PxRect { w: 2512, ..m }),
        ] {
            assert_eq!(work_area_with(edge, m, m, 48, true), cut, "{edge:?} reserved");
            assert_eq!(work_area_with(edge, m, cut, 48, true), cut, "{edge:?} reserved again");
            assert_eq!(work_area_with(edge, m, cut, 48, false), m, "{edge:?} given back");
            assert_eq!(
                work_area_with(edge, m, m, 48, false),
                m,
                "{edge:?} nothing to give back"
            );
        }
        let other = PxRect { h: 1380, ..m };
        assert_eq!(
            work_area_with(Edge::Bottom, m, other, 48, false),
            other,
            "another bar's room"
        );
    }

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

    /// DECISIONS D70, found by the user: Rıhtım's dock did not hide. Floating a distance
    /// off its edge, it slides until only the peek is left at the monitor's edge, from
    /// any edge, Monitor 2 left of the main one too; half way is half way.
    #[test]
    fn a_floating_dock_slides_off_to_its_monitors_edge() {
        let m = PxRect {
            x: -2560,
            y: 0,
            w: 2560,
            h: 1440,
        };
        // 600 x 90 px, 12 px above the bottom, centred.
        let dock = PxRect {
            x: -1580,
            y: 1338,
            w: 600,
            h: 90,
        };
        assert_eq!(slid_to_edge(Edge::Bottom, dock, m, 2, 1.0), dock);
        assert_eq!(slid_to_edge(Edge::Bottom, dock, m, 2, 0.0).y, 1438);
        assert_eq!(slid_to_edge(Edge::Bottom, dock, m, 2, 0.5).y, 1388);
        let top = PxRect { y: 12, ..dock };
        assert_eq!(slid_to_edge(Edge::Top, top, m, 2, 0.0).y, 2 - 90);
        let left = PxRect {
            x: -2548,
            y: 600,
            w: 90,
            h: 400,
        };
        assert_eq!(slid_to_edge(Edge::Left, left, m, 2, 0.0).x, -2560 + 2 - 90);
        let right = PxRect { x: -102, ..left };
        assert_eq!(slid_to_edge(Edge::Right, right, m, 2, 0.0).x, -2);
        // Out of reach of nothing: the peek stays inside the monitor.
        let hidden = slid_to_edge(Edge::Bottom, dock, m, 2, 0.0);
        assert!(hidden.y < m.y + m.h && hidden.y + hidden.h > m.y + m.h);
    }
}
