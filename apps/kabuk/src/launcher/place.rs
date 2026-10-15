//! Where the launcher opens on a monitor (the design page's `.launcher` positions per
//! look and bar edge). Pure; DIPs relative to the monitor's top-left corner.

use kabuk_core::config::Edge;
use kabuk_ui::geom::{Point, Size};

use super::style::{LauncherStyle, Place};

/// The bar on the launcher's monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarSpace {
    pub edge: Edge,
    /// The bar's distance from the screen edge (`--edge`).
    pub distance: f32,
    /// What the bar takes from the edge: its distance plus its thickness.
    pub reserved: f32,
}

/// How the launcher is laid out before its height is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub width: f32,
    /// Fixed height (beside a vertical bar for Çizgi and Ray).
    pub height: Option<f32>,
    pub columns: u8,
    /// Konsol below its input: results grow upwards.
    pub bottom_up: bool,
}

fn vertical(edge: Edge) -> bool {
    matches!(edge, Edge::Left | Edge::Right)
}

pub fn shape(style: &LauncherStyle, monitor: Size, bar: Option<BarSpace>) -> Shape {
    let plain = Shape {
        width: style.width,
        height: None,
        columns: 1,
        bottom_up: false,
    };
    match (style.place, bar) {
        (Place::Docked { side_width }, Some(b)) if vertical(b.edge) => Shape {
            width: side_width,
            height: Some(monitor.h),
            ..plain
        },
        (Place::Docked { .. }, _) => Shape {
            width: monitor.w,
            columns: 2,
            ..plain
        },
        (Place::Corner { .. }, Some(b)) if vertical(b.edge) => Shape {
            height: Some(monitor.h - 2.0 * b.distance),
            ..plain
        },
        (Place::Prompt { .. }, Some(b)) if b.edge == Edge::Top => plain,
        (Place::Prompt { .. }, _) => Shape {
            bottom_up: true,
            ..plain
        },
        _ => plain,
    }
}

/// The top-left corner for a launcher of `size`.
pub fn position(style: &LauncherStyle, monitor: Size, bar: Option<BarSpace>, size: Size) -> Point {
    let centre_x = (monitor.w - size.w) / 2.0;
    let edge = bar.map(|b| b.edge);
    let reserved = bar.map_or(0.0, |b| b.reserved);
    let distance = bar.map_or(0.0, |b| b.distance);
    let (x, y) = match style.place {
        Place::Centered { top } => (centre_x, top),
        Place::Docked { .. } => match edge {
            Some(Edge::Bottom) => (0.0, monitor.h - reserved - size.h),
            Some(Edge::Left) => (reserved, 0.0),
            Some(Edge::Right) => (monitor.w - reserved - size.w, 0.0),
            Some(Edge::Top) | None => (0.0, reserved),
        },
        Place::Corner { gap } => match edge {
            Some(Edge::Bottom) => (distance, monitor.h - reserved - gap - size.h),
            Some(Edge::Left) => (reserved + gap, distance),
            Some(Edge::Right) => (monitor.w - reserved - gap - size.w, distance),
            Some(Edge::Top) | None => (distance, reserved + gap),
        },
        Place::Prompt { gap, bottom } => match edge {
            Some(Edge::Top) => (centre_x, reserved + gap),
            Some(Edge::Bottom) => (centre_x, monitor.h - reserved - gap - size.h),
            _ => (centre_x, monitor.h - bottom - size.h),
        },
        Place::Menu { gap } => match edge {
            Some(Edge::Bottom) => (distance + gap, monitor.h - reserved - gap - size.h),
            Some(Edge::Left) => (reserved + gap, monitor.h - distance - gap - size.h),
            Some(Edge::Right) => (monitor.w - reserved - gap - size.w, monitor.h - distance - gap - size.h),
            Some(Edge::Top) | None => (distance + gap, reserved + gap),
        },
        Place::Flush => match edge {
            Some(Edge::Bottom) => (distance, monitor.h - reserved - size.h),
            Some(Edge::Left) => (reserved, distance),
            Some(Edge::Right) => (monitor.w - reserved - size.w, distance),
            Some(Edge::Top) | None => (distance, reserved),
        },
    };
    Point { x, y }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every look on its default edge, Çizgi and Ray beside vertical bars,
    //! Konsol above a bottom bar and below a top one, a monitor without a bar.
    use super::*;
    use kabuk_core::config::Look;

    const MONITOR: Size = Size { w: 2048.0, h: 1152.0 };
    const SIZE: Size = Size { w: 700.0, h: 380.0 };

    fn bar(edge: Edge, distance: f32, thickness: f32) -> Option<BarSpace> {
        Some(BarSpace {
            edge,
            distance,
            reserved: distance + thickness,
        })
    }

    fn at(look: Look, b: Option<BarSpace>, size: Size) -> (f32, f32) {
        let p = position(&LauncherStyle::of(look), MONITOR, b, size);
        (p.x, p.y)
    }

    #[test]
    fn centred_looks_open_at_their_height() {
        for (look, top) in [
            (Look::Ada, 250.0),
            (Look::Devre, 230.0),
            (Look::Rihtim, 210.0),
            (Look::Etkinlik, 120.0),
        ] {
            assert_eq!(at(look, bar(Edge::Top, 12.0, 36.0), SIZE), (674.0, top), "{look:?}");
            let s = shape(&LauncherStyle::of(look), MONITOR, bar(Edge::Top, 12.0, 36.0));
            assert_eq!((s.height, s.columns, s.bottom_up), (None, 1, false));
        }
    }

    #[test]
    fn cizgi_spans_under_a_horizontal_bar_and_runs_beside_a_vertical_one() {
        let st = LauncherStyle::of(Look::Cizgi);
        let top = bar(Edge::Top, 0.0, 30.0);
        assert_eq!(shape(&st, MONITOR, top).width, 2048.0);
        assert_eq!(shape(&st, MONITOR, top).columns, 2);
        assert_eq!(at(Look::Cizgi, top, SIZE), (0.0, 30.0));
        assert_eq!(
            at(Look::Cizgi, bar(Edge::Bottom, 0.0, 30.0), SIZE),
            (0.0, 1152.0 - 30.0 - 380.0)
        );
        let left = bar(Edge::Left, 0.0, 56.0);
        let s = shape(&st, MONITOR, left);
        assert_eq!((s.width, s.height, s.columns), (720.0, Some(1152.0), 1));
        assert_eq!(at(Look::Cizgi, left, SIZE), (56.0, 0.0));
        assert_eq!(
            at(Look::Cizgi, bar(Edge::Right, 0.0, 56.0), SIZE),
            (2048.0 - 56.0 - 700.0, 0.0)
        );
        // No bar on this monitor: under the top edge.
        assert_eq!(at(Look::Cizgi, None, SIZE), (0.0, 0.0));
    }

    #[test]
    fn ray_opens_at_the_bars_start_corner() {
        let st = LauncherStyle::of(Look::Ray);
        let left = bar(Edge::Left, 12.0, 58.0);
        assert_eq!(at(Look::Ray, left, SIZE), (12.0 + 58.0 + 12.0, 12.0));
        assert_eq!(shape(&st, MONITOR, left).height, Some(1152.0 - 24.0));
        assert_eq!(
            at(Look::Ray, bar(Edge::Top, 12.0, 36.0), SIZE),
            (12.0, 12.0 + 36.0 + 12.0)
        );
        assert_eq!(shape(&st, MONITOR, bar(Edge::Top, 12.0, 36.0)).height, None);
        assert_eq!(
            at(Look::Ray, bar(Edge::Bottom, 12.0, 36.0), SIZE),
            (12.0, 1152.0 - 60.0 - 380.0)
        );
        assert_eq!(
            at(Look::Ray, bar(Edge::Right, 12.0, 58.0), SIZE),
            (2048.0 - 82.0 - 700.0, 12.0)
        );
    }

    /// `.screen[data-dir="h"] .launcher`: 8 px from the bar's start corner, at the bottom
    /// beside a vertical bar.
    #[test]
    fn pano_opens_like_a_start_menu() {
        let bottom = bar(Edge::Bottom, 0.0, 48.0);
        assert_eq!(at(Look::Pano, bottom, SIZE), (8.0, 1152.0 - 48.0 - 8.0 - 380.0));
        assert_eq!(at(Look::Pano, bar(Edge::Top, 0.0, 48.0), SIZE), (8.0, 56.0));
        assert_eq!(
            at(Look::Pano, bar(Edge::Left, 0.0, 64.0), SIZE),
            (72.0, 1152.0 - 8.0 - 380.0)
        );
        assert_eq!(
            at(Look::Pano, bar(Edge::Right, 0.0, 64.0), SIZE),
            (2048.0 - 72.0 - 700.0, 1152.0 - 8.0 - 380.0)
        );
        let s = shape(&LauncherStyle::of(Look::Pano), MONITOR, bottom);
        assert_eq!((s.width, s.height, s.bottom_up), (600.0, None, false));
    }

    /// `.screen[data-dir="i"] .launcher`: against the bar, at the top beside a side bar.
    #[test]
    fn izgara_opens_against_its_bar() {
        assert_eq!(at(Look::Izgara, bar(Edge::Top, 0.0, 52.0), SIZE), (0.0, 52.0));
        assert_eq!(
            at(Look::Izgara, bar(Edge::Bottom, 0.0, 52.0), SIZE),
            (0.0, 1152.0 - 52.0 - 380.0)
        );
        assert_eq!(at(Look::Izgara, bar(Edge::Left, 0.0, 76.0), SIZE), (76.0, 0.0));
        assert_eq!(
            at(Look::Izgara, bar(Edge::Right, 0.0, 76.0), SIZE),
            (2048.0 - 76.0 - 700.0, 0.0)
        );
        // Defter is centred, 200 px down.
        assert_eq!(at(Look::Defter, bar(Edge::Top, 0.0, 50.0), SIZE), (674.0, 200.0));
    }

    #[test]
    fn konsol_opens_next_to_its_bar_like_fzf() {
        let st = LauncherStyle::of(Look::Konsol);
        let size = Size { w: 900.0, h: 232.0 };
        let bottom = bar(Edge::Bottom, 0.0, 30.0);
        assert!(shape(&st, MONITOR, bottom).bottom_up);
        assert_eq!(at(Look::Konsol, bottom, size), (574.0, 1152.0 - 30.0 - 14.0 - 232.0));
        let top = bar(Edge::Top, 0.0, 30.0);
        assert!(!shape(&st, MONITOR, top).bottom_up);
        assert_eq!(at(Look::Konsol, top, size), (574.0, 44.0));
        assert_eq!(
            at(Look::Konsol, bar(Edge::Left, 0.0, 64.0), size),
            (574.0, 1152.0 - 44.0 - 232.0)
        );
        assert!(shape(&st, MONITOR, None).bottom_up);
    }
}
