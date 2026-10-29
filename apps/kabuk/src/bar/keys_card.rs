//! The shortcut card (suggested as a feature and asked for by the user): Kabuk's main
//! shortcuts on one card in the middle of the monitor, as the user bound them. It greets
//! a new user on Kabuk's first start and opens again with its own shortcut (`Win+F1`);
//! any key or click closes it. Pure: the rows and the tree.

use kabuk_core::config::shortcut::{Chord, Key, Mods};
use kabuk_core::config::Shortcuts;
use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

/// The card's sizes (DIPs): its padding, corners, the space between its groups, between
/// a group's words and its keys, a row's height, the title's, the words' and the keys'
/// text, a key cap's padding and corners, and the space between the card's parts.
const PAD: f32 = 28.0;
const RADIUS: f32 = 16.0;
const COLUMN_GAP: f32 = 36.0;
const ROW_GAP: f32 = 8.0;
const KEYS_GAP: f32 = 16.0;
const ROW_HEIGHT: f32 = 30.0;
const TITLE: f32 = 20.0;
const SUBTITLE: f32 = 13.0;
const LABEL: f32 = 13.0;
const KEYS: f32 = 12.0;
const CAP_PAD: (f32, f32) = (7.0, 3.0);
const CAP_RADIUS: f32 = 5.0;
const SECTION_GAP: f32 = 22.0;

/// The way out that cannot be changed (SPEC §6).
const EMERGENCY: &str = "Ctrl+Alt+Shift+End";

/// A row: its words' key and the chord to press, as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: &'static str,
    pub keys: String,
}

/// The first chord of a list, as written; empty when none is bound.
fn first(list: &[String]) -> String {
    list.first().cloned().unwrap_or_default()
}

/// The four arrows of one action (focus or move) on one row: with the same modifiers,
/// "Win+← ↑ ↓ →"; otherwise the four chords as they are.
fn arrows(lists: [&[String]; 4]) -> String {
    let chords: Vec<Option<Chord>> = lists
        .iter()
        .map(|l| l.first().and_then(|c| Chord::parse(c).ok()))
        .collect();
    let mods: Vec<Mods> = chords.iter().flatten().map(|c| c.mods).collect();
    let all_arrows = chords.iter().all(|c| {
        c.as_ref()
            .is_some_and(|c| matches!(c.key, Key::Left | Key::Up | Key::Down | Key::Right))
    });
    if all_arrows && mods.windows(2).all(|w| w[0] == w[1]) {
        let glyph = |k: Key| match k {
            Key::Left => "\u{2190}",
            Key::Up => "\u{2191}",
            Key::Down => "\u{2193}",
            _ => "\u{2192}",
        };
        let keys: Vec<&str> = chords.iter().flatten().map(|c| glyph(c.key)).collect();
        let prefix = mods.first().map(|m| format!("{m}+")).unwrap_or_default();
        return format!("{prefix}{}", keys.join(" "));
    }
    lists
        .iter()
        .map(|l| first(l))
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("  ")
}

/// The card's groups, each a title's key and its rows, as `s` binds them.
pub fn groups(s: &Shortcuts) -> Vec<(&'static str, Vec<Row>)> {
    let row = |label, keys: String| Row { label, keys };
    vec![
        (
            "keys-g-windows",
            vec![
                row(
                    "keys-focus",
                    arrows([&s.focus_left, &s.focus_up, &s.focus_down, &s.focus_right]),
                ),
                row(
                    "keys-move",
                    arrows([&s.move_left, &s.move_up, &s.move_down, &s.move_right]),
                ),
                row("set-key-toggle-float", first(&s.toggle_float)),
                row("set-key-cycle-layout", first(&s.cycle_layout)),
                row("set-key-toggle-group", first(&s.toggle_group)),
                row("set-key-next-tab", first(&s.next_tab)),
                row("set-key-close-window", first(&s.close_window)),
            ],
        ),
        (
            "keys-g-workspaces",
            vec![
                row("keys-workspace", format!("{}+1\u{2026}9", s.workspace)),
                row("keys-move-to", format!("{}+1\u{2026}9", s.move_to_workspace)),
                row("set-key-overview", first(&s.overview)),
                row("set-key-toggle-scratchpad", first(&s.toggle_scratchpad)),
                row("set-key-to-scratchpad", first(&s.to_scratchpad)),
                row("set-key-drag", s.drag.clone()),
            ],
        ),
        (
            "keys-g-kabuk",
            vec![
                row("set-key-launcher", first(&s.launcher)),
                row("set-key-app-grid", first(&s.app_grid)),
                row("set-key-dropdown", first(&s.dropdown)),
                row("set-key-start-saver", first(&s.start_saver)),
                row("set-key-keys-card", first(&s.keys_card)),
                row("keys-emergency", EMERGENCY.to_string()),
            ],
        ),
    ]
}

fn text(words: String, color: Role, size: f32, weight: u16) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text: words,
            color,
            weight,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Ui,
        size,
        max_width: None,
    })
}

/// A chord in a key cap.
fn cap(keys: &str, scale: f32) -> Node {
    Node::layer(vec![Node::text(TextSpec {
        spans: vec![Span {
            text: keys.to_string(),
            color: Role::Fg,
            weight: 500,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Num,
        size: KEYS * scale,
        max_width: None,
    })])
    .padding(Insets::symmetric(CAP_PAD.0, CAP_PAD.1))
    .paint(Paint {
        fill: Role::SurfaceAlt,
        border: Some((Role::SurfaceLine, 1.0)),
        radius: CAP_RADIUS,
    })
}

/// The card: a greeting on the first start (`welcome`), else its title; the groups side
/// by side; what closes it and where the shortcuts change under them.
pub fn view(groups: &[(&'static str, Vec<Row>)], i18n: &I18n, text_scale: f32, welcome: bool) -> Node {
    let (title, sub) = if welcome {
        ("keys-welcome", "keys-welcome-h")
    } else {
        ("keys-title", "keys-title-h")
    };
    let head = Node::column(vec![
        text(i18n.tr(title), Role::Fg, TITLE * text_scale, 600),
        text(i18n.tr(sub), Role::Dim, SUBTITLE * text_scale, 400),
    ])
    .gap(ROW_GAP)
    .align(Align::Start);
    let columns = groups
        .iter()
        .map(|(title, rows)| {
            // The words in one column, their keys in the next, row by row.
            let shown: Vec<&Row> = rows.iter().filter(|r| !r.keys.is_empty()).collect();
            let line = |n: Node| Node::row(vec![n]).align(Align::Center).size(None, Some(ROW_HEIGHT));
            let words = shown
                .iter()
                .map(|r| line(text(i18n.tr(r.label), Role::Fg, LABEL * text_scale, 400)))
                .collect();
            let keys = shown.iter().map(|r| line(cap(&r.keys, text_scale))).collect();
            Node::column(vec![
                text(i18n.tr(title), Role::Accent, LABEL * text_scale, 600),
                Node::row(vec![
                    Node::column(words).align(Align::Start),
                    Node::column(keys).align(Align::Start),
                ])
                .gap(KEYS_GAP)
                .align(Align::Start),
            ])
            .gap(ROW_GAP)
            .align(Align::Start)
        })
        .collect();
    let foot = text(i18n.tr("keys-foot"), Role::Dim, SUBTITLE * text_scale, 400);
    Node::column(vec![head, Node::row(columns).gap(COLUMN_GAP).align(Align::Start), foot])
        .gap(SECTION_GAP)
        .align(Align::Start)
        .padding(Insets::all(PAD))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, 1.0)),
            radius: RADIUS,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: the defaults on their rows (the arrows on one row each); arrows bound
    //! with different modifiers or to other keys shown as they are; an action bound to
    //! nothing left out; every word in both languages; the greeting and the title.
    use super::*;
    use kabuk_i18n::Lang;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: chars as f32 * t.size * 0.5,
                height: t.size * 1.2,
                baseline: t.size,
            }
        }
    }

    fn texts(n: &Node) -> Vec<String> {
        layout(n, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.spans.iter().map(|s| s.text.as_str()).collect()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_defaults_fill_the_card() {
        let g = groups(&Shortcuts::default());
        let all: Vec<&Row> = g.iter().flat_map(|(_, r)| r).collect();
        let keys = |label: &str| all.iter().find(|r| r.label == label).map(|r| r.keys.clone());
        assert_eq!(
            keys("keys-focus").as_deref(),
            Some("Win+\u{2190} \u{2191} \u{2193} \u{2192}")
        );
        assert_eq!(
            keys("keys-move").as_deref(),
            Some("Win+Shift+\u{2190} \u{2191} \u{2193} \u{2192}")
        );
        assert_eq!(keys("keys-workspace").as_deref(), Some("Win+1\u{2026}9"));
        assert_eq!(keys("set-key-app-grid").as_deref(), Some("Win+Shift+A"));
        assert_eq!(keys("set-key-keys-card").as_deref(), Some("Win+F1"));
        // PLAN M19 U20: the card left these out.
        assert_eq!(keys("set-key-next-tab").as_deref(), Some("Win+Shift+T"));
        assert_eq!(keys("set-key-to-scratchpad").as_deref(), Some("Win+Shift+Z"));
        assert_eq!(keys("set-key-dropdown").as_deref(), Some("Win+Enter"));
        assert_eq!(keys("keys-emergency").as_deref(), Some(EMERGENCY));
        assert!(all.iter().all(|r| !r.keys.is_empty()));
    }

    #[test]
    fn arrows_bound_otherwise_are_shown_as_they_are() {
        let mixed = [
            vec!["Win+Left".to_string()],
            vec!["Alt+K".to_string()],
            vec![],
            vec!["Win+Right".to_string()],
        ];
        assert_eq!(
            arrows([&mixed[0], &mixed[1], &mixed[2], &mixed[3]]),
            "Win+Left  Alt+K  Win+Right"
        );
        let other_mods = [
            vec!["Win+Left".to_string()],
            vec!["Alt+Up".to_string()],
            vec!["Win+Down".to_string()],
            vec!["Win+Right".to_string()],
        ];
        assert!(!arrows([&other_mods[0], &other_mods[1], &other_mods[2], &other_mods[3]]).contains('\u{2190}'));
        // Nothing bound: the row is left out of the card.
        let s = Shortcuts {
            toggle_group: Vec::new(),
            ..Shortcuts::default()
        };
        let g = groups(&s);
        let t = texts(&view(&g, &I18n::new(Lang::En), 1.0, false));
        assert!(!t.contains(&I18n::new(Lang::En).tr("set-key-toggle-group")));
    }

    #[test]
    fn every_word_is_there_in_both_languages() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let g = groups(&Shortcuts::default());
            for key in g
                .iter()
                .flat_map(|(t, rows)| std::iter::once(*t).chain(rows.iter().map(|r| r.label)))
                .chain([
                    "keys-welcome",
                    "keys-welcome-h",
                    "keys-title",
                    "keys-title-h",
                    "keys-foot",
                ])
            {
                let w = i18n.tr(key);
                assert!(!w.is_empty() && w != key, "{key} {lang:?}");
            }
            let welcome = texts(&view(&g, &i18n, 1.0, true));
            assert!(welcome.contains(&i18n.tr("keys-welcome")));
            let card = texts(&view(&g, &i18n, 1.0, false));
            assert!(card.contains(&i18n.tr("keys-title")) && !card.contains(&i18n.tr("keys-welcome")));
            assert!(card.contains(&"Win+Shift+A".to_string()));
        }
    }
}
