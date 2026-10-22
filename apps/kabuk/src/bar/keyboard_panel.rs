//! The input language panel (DECISIONS D88): what a click on the bar's keyboard language
//! opens, as Windows' input switcher: the languages installed, the one in use marked,
//! and Windows' language settings. In the volume panel's surface. Pure.

use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of the panel's rows: a language's place in the list, or [`SETTINGS`].
pub const HIT: &str = "keyboard-panel";
pub const SETTINGS: u32 = 1000;
/// Windows' page for the languages.
pub const SETTINGS_PAGE: &str = "ms-settings:regionlanguage";

/// A row's height, its mark's size and the space after it; the short code's width.
const ROW: f32 = 30.0;
const MARK: f32 = 16.0;
const GAP: f32 = 10.0;
const CODE_W: f32 = 28.0;
/// The widest a language's name may be before it is cut.
const NAME_W: f32 = 260.0;

/// A language as the panel lists it: its short code, its name, whether it is in use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub code: String,
    pub name: String,
    pub current: bool,
}

fn words(text: String, color: Role, font: FontRole, size: f32, max_width: f32) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text,
            color,
            weight: 400,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font,
        size,
        max_width: Some(max_width),
    })
}

/// The panel's tree.
pub fn view(rows: &[Row], i18n: &I18n, text_scale: f32, s: &TipStyle) -> Node {
    let size = s.text * text_scale;
    let mut out: Vec<Node> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mark = if r.current {
                Node::icon("check", MARK, Role::Accent)
            } else {
                Node::spacer(MARK, MARK)
            };
            let code = Node::column(vec![words(r.code.clone(), Role::Dim, FontRole::Num, size, CODE_W)])
                .align(Align::Start)
                .size(Some(CODE_W), None);
            Node::row(vec![
                mark,
                code,
                words(r.name.clone(), Role::Fg, FontRole::Ui, size, NAME_W),
            ])
            .gap(GAP)
            .align(Align::Center)
            .padding(Insets::symmetric(s.pad_h, 0.0))
            .size(None, Some(ROW))
            .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect();
    out.push(
        Node::row(vec![
            Node::icon("gear", MARK, Role::Dim),
            words(i18n.tr("kb-settings"), Role::Fg, FontRole::Ui, size, NAME_W),
        ])
        .gap(GAP)
        .align(Align::Center)
        .padding(Insets::symmetric(s.pad_h, 0.0))
        .size(None, Some(ROW))
        .hit(HIT, SETTINGS),
    );
    Node::column(out)
        .align(Align::Start)
        .padding(Insets::all(s.pad_v + s.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: the language in use marked and no other; every row and the settings
    //! row clickable; the settings row worded in both languages.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_i18n::Lang;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: chars as f32 * 6.0,
                height: 16.0,
                baseline: 12.0,
            }
        }
    }

    #[test]
    fn the_languages_are_listed_with_the_one_in_use_marked() {
        let rows = vec![
            Row {
                code: "TR".into(),
                name: "Türkçe (Türkiye)".into(),
                current: true,
            },
            Row {
                code: "EN".into(),
                name: "English (United States)".into(),
                current: false,
            },
        ];
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let l = layout(&view(&rows, &i18n, 1.0, &TIP), Point::default(), &Fake);
            let checks = l
                .draws
                .iter()
                .filter(|d| matches!(d, Draw::Icon { spec, .. } if spec.name == "check"))
                .count();
            assert_eq!(checks, 1);
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            assert_eq!(hits, vec![0, 1, SETTINGS]);
            let texts: Vec<String> = l
                .draws
                .iter()
                .filter_map(|d| match d {
                    Draw::Text { spec, .. } => Some(spec.plain()),
                    _ => None,
                })
                .collect();
            assert!(texts.contains(&i18n.tr("kb-settings")) && texts.contains(&"EN".to_string()));
        }
    }
}
