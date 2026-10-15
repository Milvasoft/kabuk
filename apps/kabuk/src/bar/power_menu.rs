//! The power menu (SPEC §2.1 `power`): Kabuk's settings (asked for by the user, with
//! Kabuk's mark), then lock, sleep, sign out, restart, shut down, in a list in the same
//! surface as the tray tooltip and panel (D21, D26), opened from the bar's power button.
//! The design shows the button only, so the list takes Windows' order and the
//! launcher's words. Pure.

use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of an entry; the value is its index in [`ENTRIES`].
pub const HIT: &str = "power-menu";

/// What an entry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Kabuk's settings app.
    Settings,
    Lock,
    Sleep,
    SignOut,
    Restart,
    ShutDown,
}

/// The icon name that stands for Kabuk's mark (a picture, not an icon).
pub const MARK: &str = "kabuk-mark";

/// The entries: icon, words and action, as Start's power menu orders them, Kabuk's
/// settings first.
pub const ENTRIES: [(&str, &str, Action); 6] = [
    (MARK, "launcher-settings", Action::Settings),
    ("lock", "i-lock", Action::Lock),
    ("power", "i-sleep", Action::Sleep),
    ("app", "power-sign-out", Action::SignOut),
    ("power", "launcher-restart", Action::Restart),
    ("power", "launcher-shutdown", Action::ShutDown),
];

/// An entry's height and the space between its icon and its words (DIPs).
const ROW: f32 = 30.0;
const ICON: f32 = 16.0;
/// The icons' size (DIPs), for drawing the mark's picture at it.
pub const ICON_SIZE: f32 = ICON;
const ICON_GAP: f32 = 10.0;

/// The menu's tree; `mark` is the picture key of Kabuk's mark.
pub fn view(i18n: &I18n, text_scale: f32, s: &TipStyle, mark: u64) -> Node {
    let rows = ENTRIES
        .iter()
        .enumerate()
        .map(|(i, (icon, key, _))| {
            let picture = if *icon == MARK {
                Node::image(mark, ICON)
            } else {
                Node::icon(icon, ICON, Role::Dim)
            };
            Node::row(vec![
                picture,
                Node::text(TextSpec {
                    spans: vec![Span {
                        text: i18n.tr(key),
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
                }),
            ])
            .gap(ICON_GAP)
            .align(Align::Center)
            .padding(Insets::symmetric(s.pad_h, 0.0))
            .size(None, Some(ROW))
            .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect();
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
    //! Edge cases: every entry has words in both languages and is clickable by its index.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_i18n::Lang;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, TextMeasure, TextMetrics};

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
    fn every_entry_is_worded_and_clickable() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for (_, key, _) in ENTRIES {
                let w = i18n.tr(key);
                assert!(!w.is_empty() && w != key, "{key} {lang:?}");
            }
            let l = layout(&view(&i18n, 1.0, &TIP, 7), Point::default(), &Fake);
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            assert_eq!(hits, vec![0, 1, 2, 3, 4, 5]);
            // Kabuk's settings come first, with the mark's picture.
            assert!(l
                .draws
                .iter()
                .any(|d| matches!(d, kabuk_ui::node::Draw::Image { spec, .. } if spec.key == 7)));
        }
        assert_eq!(I18n::new(Lang::En).tr("power-sign-out"), "Sign out");
    }
}
