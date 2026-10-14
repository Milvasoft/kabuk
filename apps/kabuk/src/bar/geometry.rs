//! Where a bar goes on its monitor, in physical pixels and DIPs. Pure.

use kabuk_core::config::Edge;
use kabuk_ui::geom::{Rect, Scale, Size};

/// Sizes of a bar from the look's tokens (DIPs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarSize {
    /// Distance between the bar and the screen edge (`--edge`).
    pub distance: f32,
    /// Bar height on top/bottom (`--bar-h`).
    pub height: f32,
    /// Bar width on left/right (`--rail`).
    pub rail: f32,
}

impl BarSize {
    pub fn thickness(&self, edge: Edge) -> f32 {
        if vertical(edge) {
            self.rail
        } else {
            self.height
        }
    }

    /// Physical pixels reserved from the screen edge: the distance plus the bar.
    pub fn reserved_px(&self, edge: Edge, scale: Scale) -> i32 {
        scale.px(self.distance + self.thickness(edge))
    }
}

pub fn vertical(edge: Edge) -> bool {
    matches!(edge, Edge::Left | Edge::Right)
}

/// The content rectangle (DIPs) inside a bar window of `window` DIPs.
pub fn content(edge: Edge, window: Size, size: &BarSize) -> Rect {
    let d = size.distance;
    let t = size.thickness(edge);
    match edge {
        Edge::Top => Rect::new(d, d, window.w - 2.0 * d, t),
        Edge::Bottom => Rect::new(d, window.h - d - t, window.w - 2.0 * d, t),
        Edge::Left => Rect::new(d, d, t, window.h - 2.0 * d),
        Edge::Right => Rect::new(window.w - d - t, d, t, window.h - 2.0 * d),
    }
}

/// [`content`] centred across the window, for a bar on a background that fills the whole
/// window (DECISIONS D28): half the distance on the screen's side, half on the other.
pub fn centred(edge: Edge, window: Size, size: &BarSize) -> Rect {
    let c = content(edge, window, size);
    let half = size.distance / 2.0;
    match edge {
        Edge::Top => Rect { y: half, ..c },
        Edge::Bottom => Rect {
            y: window.h - half - c.h,
            ..c
        },
        Edge::Left => Rect { x: half, ..c },
        Edge::Right => Rect {
            x: window.w - half - c.w,
            ..c
        },
    }
}

/// The OLED shift turned so that, across the bar, it moves toward the screen edge
/// (into the free distance) and never past the reserved strip.
pub fn shift_toward_edge(edge: Edge, (sx, sy): (i32, i32)) -> (i32, i32) {
    match edge {
        Edge::Top => (sx, -sy),
        Edge::Bottom => (sx, sy),
        Edge::Left => (-sx, sy),
        Edge::Right => (sx, sy),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: each edge, 100 % and 125 % scale, shifts on every edge stay inside
    //! the window.
    use super::*;

    const ADA: BarSize = BarSize {
        distance: 14.0,
        height: 38.0,
        rail: 54.0,
    };

    #[test]
    fn reserved_strip() {
        assert_eq!(ADA.reserved_px(Edge::Top, Scale(1.0)), 52);
        assert_eq!(ADA.reserved_px(Edge::Top, Scale(1.25)), 65);
        assert_eq!(ADA.reserved_px(Edge::Left, Scale(1.25)), 85);
    }

    #[test]
    fn content_per_edge() {
        let h = Size { w: 2048.0, h: 52.0 };
        assert_eq!(content(Edge::Top, h, &ADA), Rect::new(14.0, 14.0, 2020.0, 38.0));
        assert_eq!(content(Edge::Bottom, h, &ADA), Rect::new(14.0, 0.0, 2020.0, 38.0));
        let v = Size { w: 68.0, h: 1152.0 };
        assert_eq!(content(Edge::Left, v, &ADA), Rect::new(14.0, 14.0, 54.0, 1124.0));
        assert_eq!(content(Edge::Right, v, &ADA), Rect::new(0.0, 14.0, 54.0, 1124.0));
    }

    /// Reported by the user: on the strip the capsules sat against its inner side.
    #[test]
    fn content_on_a_background_is_centred_across() {
        let h = Size { w: 2048.0, h: 52.0 };
        assert_eq!(centred(Edge::Top, h, &ADA), Rect::new(14.0, 7.0, 2020.0, 38.0));
        assert_eq!(centred(Edge::Bottom, h, &ADA), Rect::new(14.0, 7.0, 2020.0, 38.0));
        let v = Size { w: 68.0, h: 1152.0 };
        assert_eq!(centred(Edge::Left, v, &ADA), Rect::new(7.0, 14.0, 54.0, 1124.0));
        assert_eq!(centred(Edge::Right, v, &ADA), Rect::new(7.0, 14.0, 54.0, 1124.0));
        // A look on the screen's edge has nothing to centre.
        let flush = BarSize { distance: 0.0, ..ADA };
        let h = Size { w: 2048.0, h: 38.0 };
        assert_eq!(centred(Edge::Top, h, &flush), content(Edge::Top, h, &flush));
    }

    #[test]
    fn shifts_stay_inside_the_window() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let window = if vertical(edge) {
                Size { w: 68.0, h: 1152.0 }
            } else {
                Size { w: 2048.0, h: 52.0 }
            };
            for c in [content(edge, window, &ADA), centred(edge, window, &ADA)] {
                for step in 0..8 {
                    let (dx, dy) = shift_toward_edge(edge, kabuk_core::oled::shift_at(step));
                    let moved = c.offset(dx as f32, dy as f32);
                    assert!(moved.x >= 0.0 && moved.y >= 0.0, "{edge:?} {step}");
                    assert!(
                        moved.right() <= window.w && moved.bottom() <= window.h,
                        "{edge:?} {step}"
                    );
                }
            }
        }
    }
}
