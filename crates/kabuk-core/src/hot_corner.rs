//! Hot corners (asked for by the user, off by default): the mouse pushed into a corner
//! of the screen does what the user chose for it, as on macOS. Only a corner the mouse
//! stops at counts: one where no other monitor goes on (between two monitors side by
//! side the mouse passes through). A corner acts once as the mouse reaches it, and again
//! only after the mouse left it; never while a button is held (a drag).

use crate::config::{HotCornerAction, HotCorners};
use crate::grab::Point;
use crate::state::Rect;

/// How close to a corner's pixel the mouse counts as in it (physical pixels).
pub const REACH: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// What the user chose for this corner.
    pub fn action(self, c: &HotCorners) -> HotCornerAction {
        match self {
            Corner::TopLeft => c.top_left,
            Corner::TopRight => c.top_right,
            Corner::BottomLeft => c.bottom_left,
            Corner::BottomRight => c.bottom_right,
        }
    }
}

fn on_a_monitor(p: Point, monitors: &[Rect]) -> bool {
    monitors
        .iter()
        .any(|m| p.x >= m.left && p.x < m.right && p.y >= m.top && p.y < m.bottom)
}

/// The corner `p` is in, of the monitor it is on, if the mouse stops there: the points
/// just past the corner, across and down, are on no monitor.
pub fn corner_at(p: Point, monitors: &[Rect]) -> Option<Corner> {
    let m = monitors
        .iter()
        .find(|m| p.x >= m.left && p.x < m.right && p.y >= m.top && p.y < m.bottom)?;
    let left = p.x < m.left + REACH;
    let right = p.x >= m.right - REACH;
    let top = p.y < m.top + REACH;
    let bottom = p.y >= m.bottom - REACH;
    let (corner, at, out_x, out_y) = match (left, right, top, bottom) {
        (true, _, true, _) => (Corner::TopLeft, (m.left, m.top), -1, -1),
        (_, true, true, _) => (Corner::TopRight, (m.right - 1, m.top), 1, -1),
        (true, _, _, true) => (Corner::BottomLeft, (m.left, m.bottom - 1), -1, 1),
        (_, true, _, true) => (Corner::BottomRight, (m.right - 1, m.bottom - 1), 1, 1),
        _ => return None,
    };
    let past = |dx: i32, dy: i32| Point {
        x: at.0 + dx,
        y: at.1 + dy,
    };
    let open = [past(out_x, 0), past(0, out_y), past(out_x, out_y)]
        .iter()
        .all(|q| !on_a_monitor(*q, monitors));
    open.then_some(corner)
}

/// Follows the mouse: which corner to act on as it moves.
#[derive(Debug, Default)]
pub struct Watch {
    /// The corner the mouse is in now (it acted already).
    inside: Option<Corner>,
}

impl Watch {
    /// The mouse moved to `p`: the corner to act on now, if it just reached one whose
    /// action is not none and no button is held.
    pub fn moved(&mut self, p: Point, monitors: &[Rect], held: bool, chosen: &HotCorners) -> Option<Corner> {
        let now = corner_at(p, monitors);
        let reached = now.filter(|c| self.inside != Some(*c));
        self.inside = now;
        reached.filter(|c| !held && c.action(chosen) != HotCornerAction::None)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: each corner of one monitor; the edges between corners; two monitors
    //! side by side (their shared corners pass) and one above the other; a corner acts
    //! once until left; not while a button is held; not with no action; off the monitors.
    use super::*;

    const M1: Rect = Rect {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    /// Monitor 2 at negative x, as on the user's desk.
    const M2: Rect = Rect {
        left: -2560,
        top: 0,
        right: 0,
        bottom: 1440,
    };

    fn p(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    fn all(a: HotCornerAction) -> HotCorners {
        HotCorners {
            top_left: a,
            top_right: a,
            bottom_left: a,
            bottom_right: a,
        }
    }

    #[test]
    fn the_four_corners_of_one_monitor() {
        let one = [M1];
        assert_eq!(corner_at(p(0, 0), &one), Some(Corner::TopLeft));
        assert_eq!(corner_at(p(2559, 0), &one), Some(Corner::TopRight));
        assert_eq!(corner_at(p(0, 1439), &one), Some(Corner::BottomLeft));
        assert_eq!(corner_at(p(2559, 1439), &one), Some(Corner::BottomRight));
        assert_eq!(corner_at(p(1, 1), &one), Some(Corner::TopLeft), "within reach");
        for q in [
            p(1280, 0),
            p(0, 700),
            p(REACH, REACH),
            p(1280, 700),
            p(-5, 0),
            p(3000, 0),
        ] {
            assert_eq!(corner_at(q, &one), None, "{q:?}");
        }
    }

    #[test]
    fn corners_shared_by_two_monitors_pass() {
        let two = [M1, M2];
        // Monitor 2's left corners and Monitor 1's right ones are outside.
        assert_eq!(corner_at(p(-2560, 0), &two), Some(Corner::TopLeft));
        assert_eq!(corner_at(p(2559, 1439), &two), Some(Corner::BottomRight));
        // Where they meet, the mouse goes on.
        assert_eq!(corner_at(p(0, 0), &two), None);
        assert_eq!(corner_at(p(-1, 0), &two), None);
        assert_eq!(corner_at(p(-1, 1439), &two), None);
        // One above the other.
        let above = Rect {
            left: 0,
            top: -1440,
            right: 2560,
            bottom: 0,
        };
        let stacked = [M1, above];
        assert_eq!(corner_at(p(0, 0), &stacked), None);
        assert_eq!(corner_at(p(0, -1440), &stacked), Some(Corner::TopLeft));
        assert_eq!(corner_at(p(0, 1439), &stacked), Some(Corner::BottomLeft));
    }

    #[test]
    fn a_corner_acts_once_until_left_and_never_during_a_drag() {
        let one = [M1];
        let chosen = all(HotCornerAction::AppGrid);
        let mut w = Watch::default();
        assert_eq!(w.moved(p(500, 500), &one, false, &chosen), None);
        assert_eq!(w.moved(p(0, 0), &one, false, &chosen), Some(Corner::TopLeft));
        assert_eq!(w.moved(p(1, 0), &one, false, &chosen), None, "still there");
        assert_eq!(w.moved(p(40, 40), &one, false, &chosen), None);
        assert_eq!(w.moved(p(0, 0), &one, false, &chosen), Some(Corner::TopLeft), "again");
        // Straight from one corner to another.
        assert_eq!(w.moved(p(2559, 0), &one, false, &chosen), Some(Corner::TopRight));
        // A drag into a corner does nothing, and letting go there neither.
        let mut w = Watch::default();
        assert_eq!(w.moved(p(0, 1439), &one, true, &chosen), None);
        assert_eq!(w.moved(p(0, 1439), &one, false, &chosen), None);
        // A corner with no action.
        let mut chosen = all(HotCornerAction::None);
        let mut w = Watch::default();
        assert_eq!(w.moved(p(0, 0), &one, false, &chosen), None);
        chosen.bottom_right = HotCornerAction::Overview;
        assert_eq!(w.moved(p(2559, 1439), &one, false, &chosen), Some(Corner::BottomRight));
        assert_eq!(Corner::BottomRight.action(&chosen), HotCornerAction::Overview);
    }
}
