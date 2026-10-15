//! Tooltips of the bar's tray icons: what they look like and where they go. Pure.
//!
//! The design page has no tooltip (DECISIONS D21): a small panel in the look's own surface
//! colours, on the free side of the bar, next to the icon and inside the monitor.

use kabuk_core::config::Edge;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};
use kabuk_win::msgloop::PxRect;

/// Sizes in DIPs, and the hover time before the tooltip shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TipStyle {
    pub pad_h: f32,
    pub pad_v: f32,
    pub radius: f32,
    pub border: f32,
    pub text: f32,
    /// Space between the bar and the tooltip.
    pub gap: f32,
    pub delay: std::time::Duration,
    /// How the tooltip, the tray panel and the power menu come (and the tooltip goes).
    pub entrance: kabuk_ui::motion::Entrance,
}

pub const TIP: TipStyle = TipStyle {
    pad_h: 10.0,
    pad_v: 6.0,
    radius: 8.0,
    border: 1.0,
    text: 12.5,
    gap: 6.0,
    delay: std::time::Duration::from_millis(500),
    entrance: kabuk_ui::motion::MENU,
};

/// The tooltip's tree: one line of text per line of the program's tip.
pub fn view(tip: &str, text_scale: f32, s: &TipStyle) -> Option<Node> {
    let lines: Vec<&str> = tip.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    let rows = lines
        .into_iter()
        .map(|l| {
            Node::text(TextSpec {
                spans: vec![Span {
                    text: l.to_string(),
                    color: Role::Fg,
                    weight: 400,
                    after: 0.0,
                    underline: false,
                    italic: false,
                    spacing: 0.0,
                }],
                font: FontRole::Ui,
                size: s.text * text_scale,
                max_width: None,
            })
        })
        .collect();
    Some(
        Node::column(rows)
            .align(Align::Start)
            .padding(Insets::symmetric(s.pad_h + s.border, s.pad_v + s.border))
            .paint(Paint {
                fill: Role::Surface,
                border: Some((Role::SurfaceLine, s.border)),
                radius: s.radius,
            }),
    )
}

/// Where a tooltip of `size` goes for an icon at `icon` (screen pixels) on a bar at `bar`
/// on edge `edge`: beyond the bar by `gap`, centred on the icon, kept inside `monitor`.
pub fn place(edge: Edge, bar: PxRect, icon: PxRect, (w, h): (i32, i32), gap: i32, monitor: PxRect) -> PxRect {
    let clamp = |v: i32, lo: i32, hi: i32| v.max(lo).min(hi.max(lo));
    let cx = icon.x + icon.w / 2 - w / 2;
    let cy = icon.y + icon.h / 2 - h / 2;
    let (x, y) = match edge {
        Edge::Top => (cx, bar.y + bar.h + gap),
        Edge::Bottom => (cx, bar.y - gap - h),
        Edge::Left => (bar.x + bar.w + gap, cy),
        Edge::Right => (bar.x - gap - w, cy),
    };
    PxRect {
        x: clamp(x, monitor.x, monitor.x + monitor.w - w),
        y: clamp(y, monitor.y, monitor.y + monitor.h - h),
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: an empty tip, a two-line tip with Windows line ends, every edge, an
    //! icon at the monitor's right end and on Monitor 2 at negative x.
    use super::*;
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let w = t.plain().chars().count() as f32 * t.size * 0.5;
            TextMetrics {
                width: w,
                height: t.size * 1.55,
                baseline: t.size,
            }
        }
    }

    #[test]
    fn a_tip_becomes_one_line_per_line() {
        assert!(view("", 1.0, &TIP).is_none());
        assert!(view(" \r\n", 1.0, &TIP).is_none());
        let n = view("OneDrive - Personal\r\nUp to date", 1.0, &TIP).unwrap();
        let l = layout(&n, kabuk_ui::geom::Point::default(), &Fake);
        let lines: Vec<String> = l
            .draws
            .iter()
            .filter_map(|d| {
                if let Draw::Text { spec, .. } = d {
                    Some(spec.plain())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lines, vec!["OneDrive - Personal", "Up to date"]);
        // Lines start at the same x (left-aligned).
        let xs: Vec<f32> = l
            .draws
            .iter()
            .filter_map(|d| {
                if let Draw::Text { rect, .. } = d {
                    Some(rect.x)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(xs[0], xs[1]);
        let panel = l
            .draws
            .iter()
            .find_map(|d| {
                if let Draw::Panel { paint, .. } = d {
                    Some(*paint)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            (panel.fill, panel.border),
            (Role::Surface, Some((Role::SurfaceLine, 1.0)))
        );
        // The text size follows the text-size setting.
        let big = view("x", 1.15, &TIP).unwrap();
        let t = layout(&big, kabuk_ui::geom::Point::default(), &Fake);
        assert!(t
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Text { spec, .. } if (spec.size - 12.5 * 1.15).abs() < 1e-4)));
    }

    const MON: PxRect = PxRect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1440,
    };
    const ICON: PxRect = PxRect {
        x: 2000,
        y: 22,
        w: 20,
        h: 20,
    };

    #[test]
    fn tooltips_go_beyond_the_bar_and_stay_on_the_monitor() {
        let top = PxRect {
            x: 0,
            y: 0,
            w: 2560,
            h: 65,
        };
        assert_eq!(
            place(Edge::Top, top, ICON, (200, 40), 8, MON),
            PxRect {
                x: 1910,
                y: 73,
                w: 200,
                h: 40
            }
        );
        let bottom = PxRect {
            x: 0,
            y: 1375,
            w: 2560,
            h: 65,
        };
        let icon = PxRect { y: 1397, ..ICON };
        assert_eq!(
            place(Edge::Bottom, bottom, icon, (200, 40), 8, MON),
            PxRect {
                x: 1910,
                y: 1327,
                w: 200,
                h: 40
            }
        );
        let left = PxRect {
            x: 0,
            y: 0,
            w: 85,
            h: 1440,
        };
        let icon = PxRect {
            x: 30,
            y: 900,
            w: 20,
            h: 20,
        };
        assert_eq!(
            place(Edge::Left, left, icon, (200, 40), 8, MON),
            PxRect {
                x: 93,
                y: 890,
                w: 200,
                h: 40
            }
        );
        let right = PxRect {
            x: 2475,
            y: 0,
            w: 85,
            h: 1440,
        };
        let icon = PxRect {
            x: 2510,
            y: 900,
            w: 20,
            h: 20,
        };
        assert_eq!(
            place(Edge::Right, right, icon, (200, 40), 8, MON),
            PxRect {
                x: 2267,
                y: 890,
                w: 200,
                h: 40
            }
        );
        // At the monitor's right end it is pushed back inside.
        let far = PxRect { x: 2540, ..ICON };
        assert_eq!(place(Edge::Top, top, far, (200, 40), 8, MON).x, 2360);
        // Monitor 2 at negative x.
        let m2 = PxRect { x: -2560, ..MON };
        let near = PxRect { x: -2555, ..ICON };
        assert_eq!(
            place(Edge::Top, PxRect { x: -2560, ..top }, near, (200, 40), 8, m2).x,
            -2560
        );
    }
}
