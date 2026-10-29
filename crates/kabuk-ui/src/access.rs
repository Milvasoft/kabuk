//! What screen readers are told of a drawn layout (DECISIONS D97): Kabuk draws everything
//! itself, so its clickable regions and the words drawn in them are all a reader can
//! name. Pure: the platform's provider (`kabuk_win::uia`) gets these as its parts.

use crate::geom::{Point, Rect};
use crate::node::{Draw, HitId, Layout};

/// One clickable region of a layout and the words drawn inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub rect: Rect,
    pub hit: HitId,
    /// The texts drawn in it (not in a region nested in it), in paint order, joined by
    /// a space; empty for a region that only holds icons or pictures.
    pub text: String,
}

/// The clickable regions of `layout`, in their order, each with the words drawn in it. A
/// text belongs to the innermost region its middle lies in (as a click there would
/// reach that region), so a row's words are not repeated by the switch inside it.
pub fn parts(layout: &Layout) -> Vec<Part> {
    let mut words: Vec<Vec<String>> = vec![Vec::new(); layout.hits.len()];
    for d in &layout.draws {
        let Draw::Text { rect, spec } = d else { continue };
        let text = spec.plain();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let middle = Point {
            x: rect.x + rect.w / 2.0,
            y: rect.y + rect.h / 2.0,
        };
        if let Some(i) = layout.hits.iter().rposition(|(r, _)| r.contains(middle)) {
            words[i].push(text.to_string());
        }
    }
    layout
        .hits
        .iter()
        .zip(words)
        .map(|((rect, hit), words)| Part {
            rect: *rect,
            hit: hit.clone(),
            text: words.join(" "),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: a region with only an icon has no words; a text outside every region
    //! is nobody's; a region nested in another takes its own words and the outer one
    //! keeps the rest; several texts in one region are joined in paint order; blank
    //! texts are left out.
    use super::*;
    use crate::node::{layout, FontRole, Node, Role, Span, TextMeasure, TextMetrics, TextSpec};

    /// 8 DIPs per character, height 1.25 × size, baseline at 0.8 × size.
    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars = t.plain().chars().count() as f32;
            TextMetrics {
                width: chars * 8.0,
                height: t.size * 1.25,
                baseline: t.size * 0.8,
            }
        }
    }

    fn text(s: &str) -> Node {
        Node::text(TextSpec {
            spans: vec![Span {
                text: s.into(),
                color: Role::Fg,
                weight: 400,
                after: 0.0,
                underline: false,
                italic: false,
                spacing: 0.0,
            }],
            font: FontRole::Ui,
            size: 12.0,
            max_width: None,
        })
    }

    #[test]
    fn each_region_gets_the_words_drawn_in_it() {
        let bar = Node::row(vec![
            text("1").hit("workspace", 1),
            Node::icon("vol", 16.0, Role::Fg).hit("vol", 0),
            text("loose"),
            Node::row(vec![text("20:31"), text("Mon 12 Oct")])
                .gap(4.0)
                .hit("clock", 0),
            text("  ").hit("blank", 0),
        ])
        .gap(10.0);
        let p = parts(&layout(&bar, Point::default(), &Fake));
        let got: Vec<(&str, &str)> = p.iter().map(|p| (p.hit.module, p.text.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("workspace", "1"),
                ("vol", ""),
                ("clock", "20:31 Mon 12 Oct"),
                ("blank", "")
            ]
        );
        // The rectangles are the regions' own.
        let l = layout(&bar, Point::default(), &Fake);
        assert_eq!(
            p.iter().map(|p| p.rect).collect::<Vec<_>>(),
            l.hits.iter().map(|h| h.0).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_nested_region_keeps_its_own_words() {
        let row = Node::row(vec![text("Wi-Fi"), text("On").hit("switch", 7)])
            .gap(20.0)
            .hit("row", 3);
        let p = parts(&layout(&Node::column(vec![row]), Point::default(), &Fake));
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].hit.module, p[0].text.as_str()), ("row", "Wi-Fi"));
        assert_eq!(
            (p[1].hit.module, p[1].hit.value, p[1].text.as_str()),
            ("switch", 7, "On")
        );
    }

    #[test]
    fn a_layout_without_regions_has_no_parts() {
        assert!(parts(&layout(&text("alone"), Point::default(), &Fake)).is_empty());
        assert!(parts(&Layout::default()).is_empty());
    }
}
