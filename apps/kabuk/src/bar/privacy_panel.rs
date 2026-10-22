//! The privacy panel (DECISIONS D88): what a click on a privacy indicator opens, as
//! Windows' own tells: which program uses the microphone, the camera or the location,
//! and Windows' privacy settings. In the volume panel's surface. Pure.

use kabuk_core::privacy::Use;
use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of the panel's link.
pub const HIT: &str = "privacy-panel";
pub const SETTINGS: u32 = 0;
/// Windows' privacy page.
pub const SETTINGS_PAGE: &str = "ms-settings:privacy";

/// A row's height, its icon and the space after it; the widest a line may be.
const ROW: f32 = 30.0;
const ICON: f32 = 16.0;
const GAP: f32 = 10.0;
const TEXT_W: f32 = 300.0;

fn words(text: String, color: Role, size: f32) -> Node {
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
        font: FontRole::Ui,
        size,
        max_width: Some(TEXT_W),
    })
}

fn row(icon: Node, text: Node, s: &TipStyle) -> Node {
    Node::row(vec![icon, text])
        .gap(GAP)
        .align(Align::Center)
        .padding(Insets::symmetric(s.pad_h, 0.0))
        .size(None, Some(ROW))
}

/// The panel's tree: a line per program and what it uses, then the settings link.
pub fn view(uses: &[Use], i18n: &I18n, text_scale: f32, s: &TipStyle) -> Node {
    let size = s.text * text_scale;
    let mut rows: Vec<Node> = uses
        .iter()
        .map(|u| {
            let line = i18n.tr_args(u.kind.message_key(), &[("value", u.program.clone().into())]);
            row(
                Node::icon(u.kind.icon(), ICON, Role::Accent),
                words(line, Role::Fg, size),
                s,
            )
        })
        .collect();
    rows.push(
        row(
            Node::icon("gear", ICON, Role::Dim),
            words(i18n.tr("priv-settings"), Role::Fg, size),
            s,
        )
        .hit(HIT, SETTINGS),
    );
    Node::column(rows)
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
    //! Edge cases: each kind worded with its program in both languages; the link alone
    //! when nothing is in use; only the link clickable.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_core::privacy::Kind;
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
    fn each_use_is_said_with_its_program() {
        let uses = vec![
            Use {
                kind: Kind::Microphone,
                program: "Zoom".into(),
            },
            Use {
                kind: Kind::Location,
                program: "Maps".into(),
            },
        ];
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let l = layout(&view(&uses, &i18n, 1.0, &TIP), Point::default(), &Fake);
            let texts: Vec<String> = l
                .draws
                .iter()
                .filter_map(|d| match d {
                    Draw::Text { spec, .. } => Some(spec.plain()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), 3);
            assert!(texts[0].contains("Zoom") && texts[1].contains("Maps"), "{texts:?}");
            assert_eq!(texts[2], i18n.tr("priv-settings"));
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            assert_eq!(hits, vec![SETTINGS]);
        }
        let i18n = I18n::new(Lang::En);
        let l = layout(&view(&[], &i18n, 1.0, &TIP), Point::default(), &Fake);
        assert_eq!(l.hits.len(), 1);
    }
}
