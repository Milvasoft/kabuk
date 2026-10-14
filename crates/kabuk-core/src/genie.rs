//! The genie (asked for by the user: macOS's minimise effect): a window pours into its
//! icon on the bar and back out. The window is drawn as horizontal strips of its picture,
//! each a rectangle (a DWM thumbnail of one band of the window); together they bend into
//! a funnel toward the icon, then slide down it. Pure: where each strip goes at each
//! moment.
//!
//! Two overlapping phases over the progress `u` (0: the window, 1: inside the icon): the
//! funnel forms (the rows nearer the icon narrow toward its width), and the window slides
//! into it, its rows squeezed into the icon's height. Restoring plays it backwards.

use crate::state::Rect;

/// The share of the progress over which the funnel forms, and where the slide starts.
const FUNNEL_END: f32 = 0.45;
const SLIDE_START: f32 = 0.25;

/// One strip: the band of the window's picture it shows (top and bottom, shares of its
/// height, 0 at the window's top) and where it is drawn (screen pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub source: (f32, f32),
    pub dest: Rect,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A rectangle in floats, `y` growing toward the icon.
#[derive(Clone, Copy)]
struct Box {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

/// A rectangle Windows gives in workspace coordinates (a minimised window's restored
/// place) in screen coordinates: workspace coordinates start at the primary monitor's
/// work area, after the bars and taskbars reserved there.
pub fn workspace_to_screen(r: Rect, primary_monitor: Rect, primary_work: Rect) -> Rect {
    let (dx, dy) = (
        primary_work.left - primary_monitor.left,
        primary_work.top - primary_monitor.top,
    );
    Rect {
        left: r.left + dx,
        top: r.top + dy,
        right: r.right + dx,
        bottom: r.bottom + dy,
    }
}

/// The inverse of [`workspace_to_screen`]: a screen rectangle in workspace coordinates,
/// as Windows takes a window's restored place.
pub fn screen_to_workspace(r: Rect, primary_monitor: Rect, primary_work: Rect) -> Rect {
    let (dx, dy) = (
        primary_work.left - primary_monitor.left,
        primary_work.top - primary_monitor.top,
    );
    Rect {
        left: r.left - dx,
        top: r.top - dy,
        right: r.right - dx,
        bottom: r.bottom - dy,
    }
}

/// The funnel at one moment: where each row of the window goes and how wide it is there.
/// Worked out for an icon below; one above is the same upside down (`down` false).
struct Funnel {
    down: bool,
    w: Box,
    i: Box,
    funnel: f32,
    slide: f32,
}

impl Funnel {
    fn new(window: Rect, icon: Rect, u: f32) -> Funnel {
        let down = icon.top + icon.bottom >= window.top + window.bottom;
        let flip = |r: Rect| -> Box {
            if down {
                Box {
                    l: r.left as f32,
                    t: r.top as f32,
                    r: r.right as f32,
                    b: r.bottom as f32,
                }
            } else {
                Box {
                    l: r.left as f32,
                    t: -(r.bottom as f32),
                    r: r.right as f32,
                    b: -(r.top as f32),
                }
            }
        };
        let u = u.clamp(0.0, 1.0);
        Funnel {
            down,
            w: flip(window),
            i: flip(icon),
            funnel: smooth(u / FUNNEL_END),
            slide: smooth((u - SLIDE_START) / (1.0 - SLIDE_START)),
        }
    }

    /// Where row `s` (0 top, 1 bottom, toward the icon) is: slid toward the icon,
    /// squeezed past its top.
    fn row(&self, s: f32) -> f32 {
        let (w, i) = (self.w, self.i);
        let icon_h = (i.b - i.t).max(1.0);
        let y = lerp(w.t, w.b, s) + self.slide * (i.t - w.t);
        let squeezed = if y > i.t {
            i.t + icon_h * (1.0 - (-(y - i.t) / icon_h).exp())
        } else {
            y
        };
        lerp(y, squeezed, self.slide)
    }

    /// The left and right edges at height `y`: how far the funnel has narrowed there.
    fn edges(&self, y: f32) -> (f32, f32) {
        let (w, i) = (self.w, self.i);
        let reach = i.t - w.t;
        let f = if reach > 1.0 { smooth((y - w.t) / reach) } else { 1.0 } * self.funnel;
        (lerp(w.l, i.l, f), lerp(w.r, i.r, f))
    }

    /// `y` back in screen coordinates.
    fn screen(&self, y: f32) -> f32 {
        if self.down {
            y
        } else {
            -y
        }
    }
}

/// The `n` strips of `window` on its way into `icon` at progress `u` (0-1). The icon
/// may be below the window (a bar at the bottom) or above it (at the top).
pub fn strips(window: Rect, icon: Rect, u: f32, n: usize) -> Vec<Strip> {
    let n = n.max(1);
    let f = Funnel::new(window, icon, u);
    (0..n)
        .map(|k| {
            let (s0, s1) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
            let (y0, y1) = (f.row(s0), f.row(s1));
            let (l, r) = f.edges((y0 + y1) / 2.0);
            let (top, bottom, source) = if f.down {
                (y0, y1, (s0, s1))
            } else {
                (-y1, -y0, (1.0 - s1, 1.0 - s0))
            };
            Strip {
                source,
                dest: Rect {
                    left: l.round() as i32,
                    top: top.round() as i32,
                    right: r.round() as i32,
                    bottom: bottom.round() as i32,
                },
            }
        })
        .collect()
}

/// One boundary between rows of the genie's mesh: the row `v` of the window's picture
/// (0 its top, 1 its bottom) is drawn at height `y`, from `left` to `right` (screen
/// pixels, not rounded). Between two boundaries the picture is stretched straight, so
/// with enough of them the funnel's edges are smooth lines (drawn by the GPU).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub v: f32,
    pub y: f32,
    pub left: f32,
    pub right: f32,
}

/// The `n + 1` boundaries of `n` rows of `window` on its way into `icon` at progress
/// `u` (0-1), from the window's top row to its bottom one.
pub fn rows(window: Rect, icon: Rect, u: f32, n: usize) -> Vec<Row> {
    let n = n.max(1);
    let f = Funnel::new(window, icon, u);
    let mut out: Vec<Row> = (0..=n)
        .map(|k| {
            // Share `s` is measured toward the icon: from the bottom when it is above.
            let s = k as f32 / n as f32;
            let y = f.row(s);
            let (left, right) = f.edges(y);
            Row {
                v: if f.down { s } else { 1.0 - s },
                y: f.screen(y),
                left,
                right,
            }
        })
        .collect();
    if !f.down {
        out.reverse();
    }
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: at the start the strips are the window exactly, at the end inside the
    //! icon, in between they stay in order and the funnel narrows toward the icon; an icon
    //! above the window is the same upside down; a window over its icon and a single strip.
    use super::*;

    /// The mesh's boundaries (for the genie drawn by the GPU): at the start they are the
    /// window's own rows, at the end inside the icon; they always run from the window's
    /// top row to its bottom one, and match the strips' edges at the strips' middles.
    #[test]
    fn rows_run_from_the_windows_top_to_its_bottom_and_follow_the_funnel() {
        let icon = Rect {
            left: 450,
            top: 1380,
            right: 500,
            bottom: 1430,
        };
        let start = rows(WINDOW, icon, 0.0, 4);
        assert_eq!(start.len(), 5);
        for (k, r) in start.iter().enumerate() {
            let s = k as f32 / 4.0;
            assert!((r.v - s).abs() < 1e-6);
            assert!((r.y - (WINDOW.top as f32 + s * 600.0)).abs() < 0.5, "{r:?}");
            assert_eq!((r.left, r.right), (WINDOW.left as f32, WINDOW.right as f32));
        }
        let end = rows(WINDOW, icon, 1.0, 8);
        for r in &end {
            assert!(r.y >= icon.top as f32 - 0.5 && r.y <= icon.bottom as f32 + 0.5, "{r:?}");
            assert!(
                (r.left - icon.left as f32).abs() < 0.5 && (r.right - icon.right as f32).abs() < 0.5,
                "{r:?}"
            );
        }
        // Mid-way, with the icon above: still top row first, from the top of the screen.
        let above = Rect {
            top: 10,
            bottom: 60,
            ..icon
        };
        let mid = rows(WINDOW, above, 0.5, 16);
        assert!((mid[0].v).abs() < 1e-6 && (mid[16].v - 1.0).abs() < 1e-6);
        assert!(mid.windows(2).all(|p| p[0].y <= p[1].y + 0.01), "{mid:?}");
        // The strips' edges are the funnel's at their middles.
        let s = strips(WINDOW, icon, 0.5, 16);
        let r = rows(WINDOW, icon, 0.5, 32);
        for (k, strip) in s.iter().enumerate() {
            let m = r[2 * k + 1];
            assert!((strip.dest.left as f32 - m.left).abs() <= 1.0, "{strip:?} vs {m:?}");
        }
    }

    const WINDOW: Rect = Rect {
        left: 100,
        top: 100,
        right: 900,
        bottom: 700,
    };
    const BELOW: Rect = Rect {
        left: 480,
        top: 1040,
        right: 520,
        bottom: 1076,
    };
    const ABOVE: Rect = Rect {
        left: 480,
        top: 4,
        right: 520,
        bottom: 40,
    };
    const N: usize = 48;

    fn inside(r: Rect, outer: Rect) -> bool {
        r.left >= outer.left && r.right <= outer.right && r.top >= outer.top && r.bottom <= outer.bottom
    }

    #[test]
    fn it_starts_as_the_window_and_ends_in_the_icon() {
        for icon in [BELOW, ABOVE] {
            let s = strips(WINDOW, icon, 0.0, N);
            assert_eq!(s.len(), N);
            assert!(s
                .iter()
                .all(|x| x.dest.left == WINDOW.left && x.dest.right == WINDOW.right));
            let (top, bottom) = (
                s.iter().map(|x| x.dest.top).min().unwrap(),
                s.iter().map(|x| x.dest.bottom).max().unwrap(),
            );
            assert_eq!((top, bottom), (WINDOW.top, WINDOW.bottom), "{icon:?}");
            // The bands cover the picture once, in order.
            let mut bands: Vec<(f32, f32)> = s.iter().map(|x| x.source).collect();
            bands.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(bands[0].0, 0.0);
            assert_eq!(bands[N - 1].1, 1.0);
            assert!(bands.windows(2).all(|p| (p[0].1 - p[1].0).abs() < 1e-6));
            let end = strips(WINDOW, icon, 1.0, N);
            assert!(end.iter().all(|x| inside(x.dest, icon)), "{icon:?}: {:?}", end[N - 1]);
        }
    }

    #[test]
    fn strips_stay_in_order_and_the_funnel_narrows_toward_the_icon() {
        for u in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let s = strips(WINDOW, BELOW, u, N);
            assert!(s.windows(2).all(|p| p[0].dest.top <= p[1].dest.top), "u {u}");
            assert!(
                s.windows(2).all(|p| p[0].dest.bottom <= p[1].dest.top + 1),
                "u {u}: no overlap"
            );
            let width = |x: &Strip| x.dest.right - x.dest.left;
            assert!(width(&s[N - 1]) <= width(&s[0]), "u {u}: narrower near the icon");
            assert!(s.iter().all(|x| width(x) >= BELOW.right - BELOW.left - 1));
        }
        // Early on, the window has barely moved; late, it is mostly in the funnel's neck.
        let early = strips(WINDOW, BELOW, 0.05, N);
        assert!((early[0].dest.top - WINDOW.top).abs() <= 2);
        let late = strips(WINDOW, BELOW, 0.9, N);
        assert!(late[N / 2].dest.top > WINDOW.bottom);
    }

    #[test]
    fn an_icon_above_is_the_same_upside_down() {
        let mirror = |r: Rect| Rect {
            left: r.left,
            top: -r.bottom,
            right: r.right,
            bottom: -r.top,
        };
        for u in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let up = strips(mirror(WINDOW), mirror(BELOW), u, N);
            let down = strips(WINDOW, BELOW, u, N);
            // Strip k is as far from the icon either way, and shows the band as far from it.
            for (a, b) in up.iter().zip(&down) {
                assert_eq!(a.dest, mirror(b.dest), "u {u}");
                assert!((a.source.0 - (1.0 - b.source.1)).abs() < 1e-6);
                assert!((a.source.1 - (1.0 - b.source.0)).abs() < 1e-6);
            }
        }
    }

    /// Found by the user: a window minimised on Monitor 1 poured in on Monitor 2, from
    /// where the window manager had last put it. Its place now comes from Windows, in
    /// workspace coordinates.
    #[test]
    fn a_restored_place_moves_by_what_the_primary_monitor_reserves() {
        let monitor = Rect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        let work = Rect { top: 56, ..monitor };
        let normal = Rect {
            left: -2400,
            top: 10,
            right: -1400,
            bottom: 610,
        };
        assert_eq!(
            workspace_to_screen(normal, monitor, work),
            Rect {
                left: -2400,
                top: 66,
                right: -1400,
                bottom: 666
            }
        );
        assert_eq!(workspace_to_screen(normal, monitor, monitor), normal);
        // And back.
        assert_eq!(
            screen_to_workspace(workspace_to_screen(normal, monitor, work), monitor, work),
            normal
        );
    }

    #[test]
    fn odd_cases_stay_finite() {
        // The window over its icon, and a single strip.
        let over = Rect {
            left: 400,
            top: 1000,
            right: 600,
            bottom: 1100,
        };
        for u in [0.0, 0.5, 1.0] {
            for s in strips(over, BELOW, u, 8).into_iter().chain(strips(WINDOW, BELOW, u, 1)) {
                assert!(s.dest.left <= s.dest.right && s.dest.top <= s.dest.bottom, "{s:?}");
            }
        }
        assert_eq!(strips(WINDOW, BELOW, 0.0, 0).len(), 1);
    }
}
