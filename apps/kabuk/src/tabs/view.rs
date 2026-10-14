//! A tabbed group's strip: one tab per member, equal widths across the strip, each with
//! its window's title; the shown one in the text colour with an accent line along its
//! bottom (the bar's workspace marker pattern). Pure.

use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, Axis, FontRole, Node, Paint, Span, TextSpec};

use super::style::TabStyle;

/// Hit-test id of a tab; the value is its index in the strip.
pub const HIT: &str = "tab";

/// A tab: its window's title and whether it is the shown member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    pub shown: bool,
}

/// The strip's tree for a strip `width` DIPs wide.
pub fn strip(tabs: &[Tab], width: f32, s: &TabStyle, text_scale: f32) -> Node {
    let n = tabs.len().max(1) as f32;
    let tab_w = ((width - s.gap * (n - 1.0)) / n).max(0.0);
    let children = tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (color, weight) = if t.shown {
                (s.shown_text, 600)
            } else {
                (s.tab_text, 400)
            };
            let title = Node::text(TextSpec {
                spans: vec![Span {
                    text: t.title.clone(),
                    color,
                    weight,
                    after: 0.0,
                    underline: false,
                }],
                font: FontRole::Ui,
                size: s.text * text_scale,
                max_width: Some((tab_w - 2.0 * (s.pad_h + s.border)).max(0.0)),
            });
            let mut layers = vec![title];
            if t.shown {
                layers.push(
                    Node::spacer(0.0, s.mark)
                        .align(Align::End)
                        .fill(Axis::Row, s.radius)
                        .paint(Paint {
                            fill: s.shown_mark,
                            border: None,
                            radius: s.mark / 2.0,
                        }),
                );
            }
            Node::layer(layers)
                .size(Some(tab_w), Some(s.height))
                .padding(Insets::symmetric(s.pad_h + s.border, 0.0))
                .paint(Paint {
                    fill: s.tab,
                    border: Some((s.tab_line, s.border)),
                    radius: s.radius,
                })
                .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect();
    Node::row(children).gap(s.gap).align(Align::Center)
}

/// A title for a window that has none.
pub fn title_or<'a>(title: &'a str, untitled: &'a str) -> &'a str {
    if title.trim().is_empty() {
        untitled
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: one tab and several fill the strip without passing it, each tab is
    //! clickable by its index, only the shown one is marked, an empty title.
    use super::*;
    use crate::tabs::style::TABS;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Kind, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            let w = chars as f32 * 7.0;
            TextMetrics {
                width: t.max_width.map_or(w, |m| w.min(m)),
                height: 16.0,
                baseline: 12.0,
            }
        }
    }

    fn tabs(n: usize, shown: usize) -> Vec<Tab> {
        (0..n)
            .map(|i| Tab {
                title: format!("Window number {i} with a long title"),
                shown: i == shown,
            })
            .collect()
    }

    #[test]
    fn tabs_share_the_strip_and_are_clickable_by_index() {
        for n in [1, 2, 5] {
            let l = layout(&strip(&tabs(n, n - 1), 600.0, &TABS, 1.0), Point::default(), &Fake);
            assert!(l.size.w <= 600.0 + 0.01, "{n}: {}", l.size.w);
            assert!(l.size.w >= 600.0 - 1.0, "{n}: does not fill: {}", l.size.w);
            assert_eq!(l.size.h, TABS.height);
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            assert_eq!(hits, (0..n as u32).collect::<Vec<_>>());
        }
    }

    #[test]
    fn only_the_shown_tab_is_marked() {
        let node = strip(&tabs(3, 1), 600.0, &TABS, 1.0);
        let Kind::Box { children, .. } = &node.kind else {
            panic!("a row")
        };
        let marked: Vec<bool> = children
            .iter()
            .map(|c| match &c.kind {
                Kind::Layer { children } => children
                    .iter()
                    .any(|n| n.paint.is_some_and(|p| p.fill == TABS.shown_mark)),
                _ => false,
            })
            .collect();
        assert_eq!(marked, vec![false, true, false]);
        assert_eq!(title_or("  ", "Untitled"), "Untitled");
        assert_eq!(title_or("Notes", "Untitled"), "Notes");
    }
}
