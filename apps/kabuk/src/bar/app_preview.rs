//! Previews of an app's windows over the bar (asked for by the user: as Windows' taskbar
//! shows them on hover): one card per window with its title and a live picture of it,
//! side by side, in the tray panel's surface (D21, D26). The pictures are DWM thumbnails
//! the host lays over the empty frames this tree leaves for them. Pure.

use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of a card; the value is its index in the windows given to [`view`].
pub const HIT: &str = "app-preview";
/// Hit-test id of a card's picture frame (where its thumbnail goes); same values.
pub const FRAME: &str = "app-preview-frame";

/// Sizes in DIPs, and how long the previews wait for the mouse that left the app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewStyle {
    /// A picture's height; its width follows the window's shape within `min_w`-`max_w`.
    pub frame_h: f32,
    pub min_w: f32,
    pub max_w: f32,
    /// Between cards, and between a card's title and its picture.
    pub gap: f32,
    pub title_gap: f32,
    /// Around a card's contents, and the frame's rounding.
    pub card_pad: f32,
    pub frame_radius: f32,
    /// The most windows previewed (Windows' taskbar turns to a list beyond its width).
    pub max_windows: usize,
    pub grace: std::time::Duration,
}

pub const PREVIEW: PreviewStyle = PreviewStyle {
    frame_h: 120.0,
    min_w: 90.0,
    max_w: 220.0,
    gap: 6.0,
    title_gap: 6.0,
    card_pad: 6.0,
    frame_radius: 4.0,
    max_windows: 6,
    grace: std::time::Duration::from_millis(300),
};

/// A window as its card needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shot {
    pub title: String,
    /// The window's size (pixels; its restored size when it is minimised).
    pub size: (i32, i32),
}

/// The frame for a window of `size`: `frame_h` high and as wide as its shape, within
/// the width limits (a very wide window gets a lower frame instead).
pub fn frame_size(size: (i32, i32), s: &PreviewStyle) -> (f32, f32) {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let wide = s.frame_h * w / h;
    if wide > s.max_w {
        (s.max_w, (s.max_w * h / w).max(1.0))
    } else {
        (wide.max(s.min_w), s.frame_h)
    }
}

/// Where a picture of `size` goes inside a frame at `frame` (x, y, w, h): as large as
/// fits, its shape kept, centred.
pub fn fit(size: (i32, i32), frame: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let (x, y, fw, fh) = frame;
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let k = (fw / w).min(fh / h);
    let (pw, ph) = (w * k, h * k);
    (x + (fw - pw) / 2.0, y + (fh - ph) / 2.0, pw, ph)
}

/// The previews' tree: a card per window (at most `max_windows`), each clickable.
pub fn view(text_scale: f32, tip: &TipStyle, s: &PreviewStyle, shots: &[Shot]) -> Node {
    let cards = shots
        .iter()
        .take(s.max_windows)
        .enumerate()
        .map(|(i, shot)| {
            let (w, h) = frame_size(shot.size, s);
            let index = u32::try_from(i).unwrap_or(u32::MAX);
            let title = Node::text(TextSpec {
                spans: vec![Span {
                    text: shot.title.clone(),
                    color: Role::Fg,
                    weight: 400,
                    after: 0.0,
                    underline: false,
                }],
                font: FontRole::Ui,
                size: tip.text * text_scale,
                max_width: Some(w),
            });
            let frame = Node::spacer(w, h)
                .paint(Paint {
                    fill: Role::SurfaceAlt,
                    border: None,
                    radius: s.frame_radius,
                })
                .hit(FRAME, index);
            Node::column(vec![title, frame])
                .gap(s.title_gap)
                .align(Align::Start)
                .padding(Insets::all(s.card_pad))
                .hit(HIT, index)
        })
        .collect();
    Node::row(cards)
        .gap(s.gap)
        .align(Align::Start)
        .padding(Insets::all(tip.pad_v + tip.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, tip.border)),
            radius: tip.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: a tall, a wide, a very wide and a zero-sized window; pictures fitted
    //! both ways; more windows than shown; every card clickable with its frame inside it.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: (chars as f32 * 6.0).min(t.max_width.unwrap_or(f32::MAX)),
                height: 16.0,
                baseline: 12.0,
            }
        }
    }

    #[test]
    fn frames_follow_the_windows_shape_within_limits() {
        let s = PREVIEW;
        // 16:9 at 120 high is 213 wide.
        let (w, h) = frame_size((1920, 1080), &s);
        assert!((w - 213.333).abs() < 0.01 && h == s.frame_h, "{w} {h}");
        // A tall window is no narrower than the least width.
        assert_eq!(frame_size((400, 1600), &s), (s.min_w, s.frame_h));
        // A very wide one gets a lower frame at the widest width.
        let (w, h) = frame_size((4000, 1000), &s);
        assert_eq!(w, s.max_w);
        assert!((h - 55.0).abs() < 0.01, "{h}");
        // Nothing to measure: no division by zero.
        let (w, h) = frame_size((0, 0), &s);
        assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0);
    }

    #[test]
    fn pictures_fit_their_frame_centred() {
        assert_eq!(fit((200, 100), (10.0, 20.0, 100.0, 100.0)), (10.0, 45.0, 100.0, 50.0));
        assert_eq!(fit((100, 200), (0.0, 0.0, 100.0, 100.0)), (25.0, 0.0, 50.0, 100.0));
        let (_, _, w, h) = fit((0, 0), (0.0, 0.0, 100.0, 50.0));
        assert!(w.is_finite() && h.is_finite());
    }

    #[test]
    fn every_card_is_clickable_with_its_frame_inside() {
        let shots: Vec<Shot> = (0..9)
            .map(|i| Shot {
                title: format!("Window {i} with a long title that is cut"),
                size: (1600, 900),
            })
            .collect();
        let l = layout(&view(1.0, &TIP, &PREVIEW, &shots), Point::default(), &Fake);
        let of = |m: &str| -> Vec<_> { l.hits.iter().filter(|(_, h)| h.module == m).cloned().collect() };
        let (cards, frames) = (of(HIT), of(FRAME));
        assert_eq!(cards.len(), PREVIEW.max_windows);
        assert_eq!(frames.len(), PREVIEW.max_windows);
        for ((card, c), (frame, f)) in cards.iter().zip(&frames) {
            assert_eq!(c.value, f.value);
            assert!(frame.x >= card.x && frame.x + frame.w <= card.x + card.w + 0.01);
            assert!(frame.y > card.y && frame.y + frame.h <= card.y + card.h + 0.01);
        }
        // Side by side, in order.
        assert!(cards.windows(2).all(|p| p[0].0.x < p[1].0.x));
        let empty = layout(&view(1.0, &TIP, &PREVIEW, &[]), Point::default(), &Fake);
        assert!(empty.hits.is_empty());
    }
}
