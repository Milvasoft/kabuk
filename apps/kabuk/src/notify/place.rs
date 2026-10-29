//! Where a notice and the OSD show on a monitor (the design's `.toast` and `.osd`
//! positions), in DIPs relative to the monitor. Pure.

use kabuk_core::config::Edge;
use kabuk_ui::geom::{Point, Size};

use super::style::{NotifyStyle, ToastPlace};
use crate::launcher::place::BarSpace;

/// The notice's top-left corner: under a top bar (or `pad` from the top), `pad` from
/// the right edge or beside a right-hand bar; Etkinlik centred; Pano's above a bottom
/// bar.
pub fn toast(s: &NotifyStyle, monitor: Size, bar: Option<BarSpace>, pad: f32, size: Size) -> Point {
    let reserved = |edge: Edge| bar.filter(|b| b.edge == edge).map_or(0.0, |b| b.reserved);
    let top = match reserved(Edge::Top) {
        0.0 => pad + s.toast_margin,
        r => r + s.toast_margin,
    };
    if let (ToastPlace::NearBar, Some(b)) = (s.toast_place, bar.filter(|b| b.edge == Edge::Bottom)) {
        // `bottom: calc(var(--bar-h) + 12px); right: 12px`.
        return Point {
            x: monitor.w - b.distance - s.toast_margin - size.w,
            y: monitor.h - b.reserved - s.toast_margin - size.h,
        };
    }
    let x = match s.toast_place {
        ToastPlace::Centre => (monitor.w - size.w) / 2.0,
        ToastPlace::Corner | ToastPlace::NearBar => {
            let right = match reserved(Edge::Right) {
                0.0 => pad.max(s.toast_margin),
                r => r + s.toast_margin,
            };
            monitor.w - right - size.w
        }
    };
    Point { x, y: top }
}

/// The OSD's top-left corner: centred, `osd_bottom` above the bottom edge, or above a
/// bottom bar.
pub fn osd(s: &NotifyStyle, monitor: Size, bar: Option<BarSpace>, size: Size) -> Point {
    let bottom = match bar.filter(|b| b.edge == Edge::Bottom) {
        Some(b) => b.reserved + s.osd_above_bar,
        None => s.osd_bottom,
    };
    Point {
        x: (monitor.w - size.w) / 2.0,
        y: monitor.h - bottom - size.h,
    }
}

/// Which monitor a notice or the OSD comes on (DECISIONS D95): the bar a change was made
/// on, else the one of the window in front, else the main one.
pub fn monitor_in_use(from_bar: Option<usize>, front: Option<usize>, main: Option<usize>) -> Option<usize> {
    from_bar.or(front).or(main)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a top, right, bottom and no bar; Etkinlik centred.
    use super::*;
    use kabuk_core::config::Look;

    const MON: Size = Size { w: 1920.0, h: 1080.0 };
    const T: Size = Size { w: 380.0, h: 70.0 };

    fn bar(edge: Edge, reserved: f32) -> Option<BarSpace> {
        Some(BarSpace {
            edge,
            distance: 14.0,
            reserved,
        })
    }

    #[test]
    fn notices_go_under_the_bar_at_the_right() {
        let s = NotifyStyle::of(Look::Ada);
        assert_eq!(
            toast(&s, MON, bar(Edge::Top, 52.0), 16.0, T),
            Point { x: 1524.0, y: 64.0 }
        );
        assert_eq!(toast(&s, MON, None, 16.0, T), Point { x: 1524.0, y: 28.0 });
        assert_eq!(
            toast(&s, MON, bar(Edge::Right, 68.0), 16.0, T),
            Point { x: 1460.0, y: 28.0 }
        );
        let g = NotifyStyle::of(Look::Etkinlik);
        assert_eq!(toast(&g, MON, bar(Edge::Top, 32.0), 16.0, T).x, 770.0);
    }

    /// `.screen[data-dir="h"][data-pos="bottom"] .toast`: above the bar at the right;
    /// with the bar elsewhere, where every look puts it.
    #[test]
    fn pano_notices_sit_above_a_bottom_bar() {
        let h = NotifyStyle::of(Look::Pano);
        let bottom = Some(BarSpace {
            edge: Edge::Bottom,
            distance: 0.0,
            reserved: 48.0,
        });
        assert_eq!(
            toast(&h, MON, bottom, 10.0, T),
            Point {
                x: 1920.0 - 12.0 - 380.0,
                y: 1080.0 - 48.0 - 12.0 - 70.0
            }
        );
        let a = NotifyStyle::of(Look::Ada);
        assert_eq!(
            toast(&h, MON, bar(Edge::Top, 48.0), 10.0, T),
            toast(&a, MON, bar(Edge::Top, 48.0), 10.0, T)
        );
    }

    #[test]
    fn the_osd_sits_low_and_clears_a_bottom_bar() {
        let s = NotifyStyle::of(Look::Ada);
        let size = Size { w: 330.0, h: 46.0 };
        assert_eq!(osd(&s, MON, bar(Edge::Top, 52.0), size), Point { x: 795.0, y: 964.0 });
        assert_eq!(osd(&s, MON, bar(Edge::Bottom, 30.0), size).y, 1080.0 - 70.0 - 46.0);
    }

    /// DECISIONS D95: the bar the change was made on first, then the window in front's
    /// monitor, then the main one.
    #[test]
    fn a_notice_comes_on_the_monitor_in_use() {
        assert_eq!(monitor_in_use(Some(1), Some(2), Some(0)), Some(1));
        assert_eq!(monitor_in_use(None, Some(2), Some(0)), Some(2));
        assert_eq!(monitor_in_use(None, None, Some(0)), Some(0));
        assert_eq!(monitor_in_use(None, None, None), None);
    }
}
