//! Whether a monitor shows a full-screen program (a game, a video), so its bar steps
//! aside instead of drawing over it. Pure.

use crate::state::Rect;

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn covers(a: &Rect, b: &Rect) -> bool {
    a.left <= b.left && a.top <= b.top && a.right >= b.right && a.bottom >= b.bottom
}

/// `windows` are the visible top-level windows front first (Kabuk's own and the desktop
/// left out). The front one on `monitor` (holding its centre there) decides: full screen
/// if it covers the whole monitor. A maximised window covers only the work area, so it does not count.
pub fn is_fullscreen(windows: impl IntoIterator<Item = Rect>, monitor: Rect) -> bool {
    is_fullscreen_front(windows.into_iter().map(|r| (r, false)), monitor)
}

/// As [`is_fullscreen`], with each window's rectangle and whether it is maximised: a
/// maximised window does not count, even where no taskbar or bar is reserved and it fills
/// the whole monitor with its borders hanging over the edges; one sized exactly to the
/// monitor, borders gone, does (Chrome's full screen keeps its window maximised). A
/// window is on the monitor that holds its centre, so one on the next monitor hanging its
/// borders over the edge does not hide what is in front here.
pub fn is_fullscreen_front(windows: impl IntoIterator<Item = (Rect, bool)>, monitor: Rect) -> bool {
    let windows: Vec<(Rect, bool)> = windows.into_iter().collect();
    front_on(windows.iter().map(|(r, _)| *r), monitor)
        .and_then(|i| windows.get(i))
        .is_some_and(|(w, maximized)| covers(w, &monitor) && (!*maximized || *w == monitor))
}

/// Which of `windows` (front first) is the front one on `monitor`, the one holding its
/// centre there: the window that decides whether the monitor is full screen.
pub fn front_on(windows: impl IntoIterator<Item = Rect>, monitor: Rect) -> Option<usize> {
    let on_monitor = |w: &Rect| {
        let (cx, cy) = (
            (i64::from(w.left) + i64::from(w.right)) / 2,
            (i64::from(w.top) + i64::from(w.bottom)) / 2,
        );
        i64::from(monitor.left) <= cx
            && cx < i64::from(monitor.right)
            && i64::from(monitor.top) <= cy
            && cy < i64::from(monitor.bottom)
    };
    windows.into_iter().position(|w| on_monitor(&w))
}

/// Monitor `device`'s rectangle now: the one Windows gives for it among `live` (device,
/// rectangle), else the one it had when its bar was made (`made`). A game in exclusive
/// full screen changes the resolution, and the bar keeps the old size until it is made
/// again (found on another computer: such a game was not seen as full screen; D72).
pub fn rect_now<'a>(device: &str, live: impl IntoIterator<Item = (&'a str, Rect)>, made: Rect) -> Rect {
    live.into_iter().find(|(d, _)| *d == device).map_or(made, |(_, r)| r)
}

/// The parts of `area` that `cut` leaves: up to four rectangles.
fn subtract(area: Rect, cut: &Rect) -> Vec<Rect> {
    if !intersects(&area, cut) {
        return vec![area];
    }
    let mut out = Vec::new();
    if cut.top > area.top {
        out.push(Rect {
            bottom: cut.top,
            ..area
        });
    }
    if cut.bottom < area.bottom {
        out.push(Rect {
            top: cut.bottom,
            ..area
        });
    }
    let (top, bottom) = (area.top.max(cut.top), area.bottom.min(cut.bottom));
    if cut.left > area.left {
        out.push(Rect {
            top,
            bottom,
            right: cut.left,
            ..area
        });
    }
    if cut.right < area.right {
        out.push(Rect {
            top,
            bottom,
            left: cut.right,
            ..area
        });
    }
    out
}

/// True when `windows` together leave nothing of `area` to see: the animated wallpaper
/// there can stop (SPEC §8.21).
pub fn hides(windows: impl IntoIterator<Item = Rect>, area: Rect) -> bool {
    let mut left = vec![area];
    for w in windows {
        left = left.into_iter().flat_map(|r| subtract(r, &w)).collect();
        if left.is_empty() {
            return true;
        }
    }
    left.is_empty()
}

#[cfg(test)]
mod tests {
    //! Edge cases: a full-screen window in front, behind a normal one, on the other
    //! monitor, larger than the monitor (borders off-screen), a maximised window, no
    //! windows, Monitor 2 at negative x; covering: one window over all, two halves, a gap
    //! left between them, none.
    use super::*;

    const M1: Rect = Rect {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    const M2: Rect = Rect {
        left: -2560,
        top: 0,
        right: 0,
        bottom: 1440,
    };
    const NORMAL: Rect = Rect {
        left: 100,
        top: 100,
        right: 900,
        bottom: 700,
    };
    const MAXIMISED: Rect = Rect {
        left: 0,
        top: 65,
        right: 2560,
        bottom: 1440,
    };

    #[test]
    fn the_front_window_on_the_monitor_decides() {
        assert!(is_fullscreen([M1], M1));
        assert!(!is_fullscreen([NORMAL, M1], M1), "a normal window in front");
        assert!(is_fullscreen([M1, NORMAL], M1));
        assert!(!is_fullscreen([MAXIMISED], M1));
        assert!(!is_fullscreen([], M1));
        // Borders hanging off the screen still cover it.
        let larger = Rect {
            left: -8,
            top: -8,
            right: 2568,
            bottom: 1448,
        };
        assert!(is_fullscreen([larger], M1));
        // The deciding window is the front one whose centre is on the monitor (the log
        // names it).
        let on_m2 = Rect {
            left: -2000,
            right: -1000,
            ..NORMAL
        };
        assert_eq!(front_on([on_m2, M1, NORMAL], M1), Some(1));
        assert_eq!(front_on([on_m2], M1), None);
    }

    /// Found by the full integration run: on a monitor with no taskbar or bar reserved
    /// (Kabuk starting, Windows' taskbar hidden), a maximised window fills the whole
    /// monitor and was taken for a full-screen program; the bar stepped aside for it.
    /// Found by the user watching a video: Chrome's full screen keeps its window maximised,
    /// sized exactly to the monitor without the borders a maximised window hangs over it,
    /// and the bar stayed over the video.
    #[test]
    fn a_maximised_window_exactly_the_monitor_is_full_screen() {
        assert!(is_fullscreen_front([(M1, true)], M1));
        assert!(is_fullscreen_front([(M2, true)], M2));
        assert!(!is_fullscreen_front([(MAXIMISED, true)], M1));
    }

    #[test]
    fn a_maximised_window_never_counts_even_where_it_fills_the_monitor() {
        let filling = Rect {
            left: -9,
            top: -9,
            right: 2569,
            bottom: 1449,
        };
        assert!(!is_fullscreen_front([(filling, true)], M1));
        assert!(is_fullscreen_front([(filling, false)], M1));
        // The front one decides: a maximised window in front of a full-screen one.
        assert!(!is_fullscreen_front([(filling, true), (M1, false)], M1));
        assert!(is_fullscreen_front([(M1, false), (filling, true)], M1));
    }

    /// Found by the full integration run: a window maximised on Monitor 2 hangs its
    /// invisible borders a few pixels into Monitor 1 and, being in front, hid the game
    /// behind it on Monitor 1, so game mode never came.
    #[test]
    fn a_window_on_the_next_monitor_does_not_hide_a_full_screen_one() {
        let maximised_on_m2 = Rect {
            left: -2569,
            top: 56,
            right: 9,
            bottom: 1449,
        };
        assert!(is_fullscreen_front([(maximised_on_m2, true), (M1, false)], M1));
        let normal_hanging_over = Rect {
            left: -1000,
            top: 100,
            right: 4,
            bottom: 900,
        };
        assert!(is_fullscreen_front([(normal_hanging_over, false), (M1, false)], M1));
        // A window really in front on Monitor 1 still decides.
        let half_on_m1 = Rect {
            left: -500,
            top: 100,
            right: 800,
            bottom: 900,
        };
        assert!(!is_fullscreen_front([(half_on_m1, false), (M1, false)], M1));
    }

    #[test]
    fn each_monitor_is_judged_alone() {
        // Full screen on Monitor 1, a normal window on Monitor 2.
        let on_m2 = Rect {
            left: -2000,
            top: 100,
            right: -1000,
            bottom: 900,
        };
        let windows = [M1, on_m2];
        assert!(is_fullscreen(windows, M1));
        assert!(!is_fullscreen(windows, M2));
        assert!(!is_fullscreen([on_m2, M2], M2));
        assert!(is_fullscreen([M2], M2));
    }

    #[test]
    fn windows_together_hide_an_area_or_leave_a_gap() {
        let area = Rect {
            left: 0,
            top: 40,
            right: 2560,
            bottom: 1440,
        };
        let r = |left, top, right, bottom| Rect {
            left,
            top,
            right,
            bottom,
        };
        assert!(hides([r(-10, 0, 2600, 1500)], area));
        assert!(hides([r(0, 40, 1280, 1440), r(1280, 40, 2560, 1440)], area));
        assert!(
            !hides([r(0, 40, 1270, 1440), r(1290, 40, 2560, 1440)], area),
            "a 20 px gap"
        );
        assert!(!hides([r(0, 40, 2560, 1430)], area), "the bottom row shows");
        assert!(!hides([], area));
        // Overlapping windows, in any order.
        assert!(hides(
            [r(1000, 0, 2560, 1440), r(0, 40, 1500, 900), r(0, 800, 1200, 1440)],
            area
        ));
    }

    /// D72: a game at 1920 x 1080 on a 2560 x 1440 monitor covers it as Windows gives it
    /// now, not as the bar was made; a monitor Windows no longer lists keeps its old size.
    #[test]
    fn a_monitors_rectangle_is_the_one_windows_gives_now() {
        let made = Rect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        let now = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let live = [("\\\\.\\DISPLAY2", M1), ("\\\\.\\DISPLAY1", now)];
        assert_eq!(rect_now("\\\\.\\DISPLAY1", live, made), now);
        assert_eq!(rect_now("\\\\.\\DISPLAY3", live, made), made);
        // The game at its own resolution is full screen on the monitor as it is now.
        assert!(is_fullscreen([now], rect_now("\\\\.\\DISPLAY1", live, made)));
        assert!(!is_fullscreen([now], made));
    }
}
