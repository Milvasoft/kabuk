//! The notice (`.toast`: the icon's box, the title and the line under it) and the volume
//! OSD (`.osd`: the speaker, the level's track and the number). Pure: node trees.

use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::style::{NotifyStyle, Track};

fn span(text: impl Into<String>, color: Role, weight: u16) -> Span {
    Span {
        text: text.into(),
        color,
        weight,
        after: 0.0,
        underline: false,
        italic: false,
        spacing: 0.0,
    }
}

fn text(spans: Vec<Span>, font: FontRole, size: f32, max_width: Option<f32>) -> Node {
    Node::text(TextSpec {
        spans,
        font,
        size,
        max_width,
    })
}

/// A notice: `title` and `sub` (empty: title only) beside `icon` in its box.
pub fn toast(title: &str, sub: &str, icon: &'static str, s: &NotifyStyle, text_scale: f32, i18n: &I18n) -> Node {
    let b = &s.icon_box;
    let inset = (b.size - s.icon_size) / 2.0;
    let icon_box = Node::row(vec![Node::icon(icon, s.icon_size, b.icon)])
        .align(Align::Center)
        .size(Some(b.size), Some(b.size))
        .padding(Insets::symmetric(inset, 0.0))
        .paint(b.paint);
    let words_width = s.toast_width - s.toast_pad.left - s.toast_pad.right - b.size - s.toast_gap;
    let title_size = s.title_size * text_scale;
    let mut words = vec![text(
        vec![s.title_typo.span(title, Role::Fg, s.title_weight, title_size, i18n)],
        FontRole::Ui,
        title_size,
        Some(words_width),
    )];
    if !sub.is_empty() {
        let sub_size = s.sub_size * text_scale;
        words.push(text(
            vec![s.sub_typo.span(sub, Role::Dim, 400, sub_size, i18n)],
            FontRole::Ui,
            sub_size,
            Some(words_width),
        ));
    }
    Node::row(vec![icon_box, Node::column(words).align(Align::Start)])
        .gap(s.toast_gap)
        .align(Align::Center)
        .padding(s.toast_pad)
        .size(Some(s.toast_width), None)
        .paint(s.toast_panel)
}

fn fill(role: Role, radius: f32) -> Paint {
    Paint {
        fill: role,
        border: None,
        radius,
    }
}

/// The level's track, `level` (0-1) of it filled; Konsol's in dashes.
pub(crate) fn track(t: &Track, level: f32) -> Node {
    let level = level.clamp(0.0, 1.0);
    if let Some((dash, gap)) = t.dashes {
        let count = ((t.width + gap) / (dash + gap)).floor().max(1.0) as usize;
        let lit = (level * count as f32).round() as usize;
        let dashes = (0..count)
            .map(|i| Node::spacer(dash, t.height).paint(fill(if i < lit { t.fill } else { t.back }, 0.0)))
            .collect();
        return Node::row(dashes).gap(gap).size(Some(t.width), Some(t.height));
    }
    Node::row(vec![
        Node::spacer(t.width * level, t.height).paint(fill(t.fill, t.radius))
    ])
    .size(Some(t.width), Some(t.height))
    .paint(fill(t.back, t.radius))
}

/// The volume OSD at `percent` (0-100).
pub fn osd(percent: u8, s: &NotifyStyle, text_scale: f32) -> Node {
    let value = Node::row(vec![text(
        vec![span(percent.to_string(), Role::Fg, s.value_weight)],
        FontRole::Num,
        s.value_size * text_scale,
        None,
    )])
    .size(Some(s.value_width), None);
    Node::row(vec![
        Node::icon("vol", s.icon_size, Role::Fg),
        track(&s.track, f32::from(percent) / 100.0),
        value,
    ])
    .gap(s.osd_gap)
    .align(Align::Center)
    .padding(s.osd_pad)
    .paint(s.osd_panel)
}

#[cfg(test)]
mod tests {
    //! Edge cases: the notice's width and parts, with and without the line under the
    //! title; the OSD's fill at 0, 40 and 100 %; Konsol's dashes.
    use super::*;
    use kabuk_core::config::Look;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: chars as f32 * t.size * 0.5,
                height: t.size * 1.3,
                baseline: t.size,
            }
        }
    }

    fn panels(n: &Node) -> Vec<(f32, Role)> {
        layout(n, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Panel { rect, paint } => Some((rect.w, paint.fill)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_notice_has_the_designs_parts() {
        let s = NotifyStyle::of(Look::Ada);
        let l = layout(
            &toast(
                "Game mode off",
                "Previous layout restored",
                "pad",
                &s,
                1.0,
                &kabuk_i18n::I18n::new(kabuk_i18n::Lang::En),
            ),
            Point::default(),
            &Fake,
        );
        assert_eq!(l.size.w, 380.0);
        let texts = l.draws.iter().filter(|d| matches!(d, Draw::Text { .. })).count();
        assert_eq!(texts, 2);
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Icon { spec, .. } if spec.name == "pad")));
        let short = layout(
            &toast(
                "Copied to clipboard",
                "",
                "check",
                &s,
                1.0,
                &kabuk_i18n::I18n::new(kabuk_i18n::Lang::En),
            ),
            Point::default(),
            &Fake,
        );
        assert_eq!(short.draws.iter().filter(|d| matches!(d, Draw::Text { .. })).count(), 1);
        // The icon box in the accent tint.
        assert!(panels(&toast(
            "x",
            "",
            "check",
            &s,
            1.0,
            &kabuk_i18n::I18n::new(kabuk_i18n::Lang::En)
        ))
        .iter()
        .any(|(w, r)| *w == 34.0 && *r == Role::AccentTint(14)));
    }

    #[test]
    fn the_osd_fills_its_track_by_the_level() {
        let s = NotifyStyle::of(Look::Ada);
        let filled = |p: u8| {
            panels(&osd(p, &s, 1.0))
                .iter()
                .find(|(_, r)| *r == Role::AccentFill)
                .map(|(w, _)| *w)
        };
        assert_eq!(filled(40), Some(88.0));
        assert_eq!(filled(100), Some(220.0));
        assert_eq!(filled(0), Some(0.0));
        let k = NotifyStyle::of(Look::Konsol);
        let dashes = panels(&osd(50, &k, 1.0));
        let lit = dashes.iter().filter(|(w, r)| *w == 6.0 && *r == Role::Accent).count();
        let dark = dashes.iter().filter(|(w, r)| *w == 6.0 && *r == Role::Faint).count();
        assert_eq!(lit + dark, 24);
        assert_eq!(lit, 12);
    }
}
