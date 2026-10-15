//! The media card (suggested as a feature and asked for by the user, DECISIONS D66): a
//! right click on the bar's media module shows what plays — its cover, title, artist and
//! album — with previous, play or pause, and next, in the tray panel's surface (D21,
//! D26). Pure.

use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of a button; the value is a [`Button`].
pub const HIT: &str = "media-card";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Previous = 0,
    Toggle = 1,
    Next = 2,
}

impl Button {
    pub fn from_value(v: u32) -> Option<Button> {
        [Button::Previous, Button::Toggle, Button::Next]
            .into_iter()
            .find(|b| *b as u32 == v)
    }
}

/// What the card shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub can_previous: bool,
    pub can_next: bool,
    /// The cover's key in the renderer's pictures; `None` shows a note instead.
    pub cover: Option<u64>,
}

/// The card's sizes (DIPs): the cover, the space beside it, the text's widest, the title's
/// and the other lines' size, the buttons' size and the space between them, and the
/// cover's corners.
const COVER: f32 = 96.0;
const GAP: f32 = 14.0;
const TEXT_WIDTH: f32 = 240.0;
const TITLE: f32 = 14.0;
const LINE: f32 = 12.5;
const BUTTON: f32 = 16.0;
const BUTTON_PAD: f32 = 8.0;
const COVER_RADIUS: f32 = 8.0;
const ROW_GAP: f32 = 4.0;
const BUTTONS_GAP: f32 = 10.0;

/// The buttons' signs (the system's symbol face has them).
const PREVIOUS: &str = "\u{23ee}";
const PLAY: &str = "\u{25b6}";
const PAUSE: &str = "\u{23f8}";
const NEXT: &str = "\u{23ed}";

fn text(words: &str, color: Role, size: f32, weight: u16, max: Option<f32>) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text: words.to_string(),
            color,
            weight,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Ui,
        size,
        max_width: max,
    })
}

/// A button: dim and without a hit when the session does not take it now.
fn button(sign: &str, b: Button, on: bool, scale: f32) -> Node {
    let n = Node::layer(vec![text(
        sign,
        if on { Role::Fg } else { Role::Faint },
        BUTTON * scale,
        400,
        None,
    )])
    .padding(Insets::all(BUTTON_PAD))
    .paint(Paint {
        fill: Role::Clear,
        border: None,
        radius: BUTTON_PAD,
    });
    if on {
        n.hit(HIT, b as u32)
    } else {
        n
    }
}

/// The card's tree.
pub fn view(card: &Card, i18n: &I18n, text_scale: f32, s: &TipStyle) -> Node {
    let cover = match card.cover {
        Some(key) => Node::picture(key, COVER, COVER),
        None => Node::layer(vec![Node::icon("music", COVER / 3.0, Role::Dim)])
            .size(Some(COVER), Some(COVER))
            .paint(Paint {
                fill: Role::SurfaceAlt,
                border: None,
                radius: COVER_RADIUS,
            }),
    };
    let title = if card.title.is_empty() {
        i18n.tr("media-nothing")
    } else {
        card.title.clone()
    };
    let mut lines = vec![text(&title, Role::Fg, TITLE * text_scale, 600, Some(TEXT_WIDTH))];
    for (words, color) in [(&card.artist, Role::Dim), (&card.album, Role::Faint)] {
        if !words.trim().is_empty() {
            lines.push(text(words, color, LINE * text_scale, 400, Some(TEXT_WIDTH)));
        }
    }
    let toggle = if card.playing { PAUSE } else { PLAY };
    lines.push(
        Node::row(vec![
            button(PREVIOUS, Button::Previous, card.can_previous, text_scale),
            button(toggle, Button::Toggle, !card.title.is_empty(), text_scale),
            button(NEXT, Button::Next, card.can_next, text_scale),
        ])
        .gap(BUTTONS_GAP)
        .align(Align::Center),
    );
    Node::row(vec![cover, Node::column(lines).gap(ROW_GAP).align(Align::Start)])
        .gap(GAP)
        .align(Align::Center)
        .padding(Insets::all(s.pad_v * 2.0 + s.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: the cover, title, artist and album shown, the three buttons clickable
    //! by their values; a session that takes no previous or next, a paused one, an empty
    //! artist and album, no cover, nothing playing.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_i18n::Lang;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, Layout, TextMeasure, TextMetrics};

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

    fn texts(l: &Layout) -> Vec<String> {
        l.draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.spans[0].text.clone()),
                _ => None,
            })
            .collect()
    }

    fn hits(l: &Layout) -> Vec<u32> {
        l.hits
            .iter()
            .filter(|(_, h)| h.module == HIT)
            .map(|(_, h)| h.value)
            .collect()
    }

    #[test]
    fn what_plays_with_its_cover_and_buttons() {
        let i18n = I18n::new(Lang::En);
        let card = Card {
            title: "Mezdeke".into(),
            artist: "Barış Manço".into(),
            album: "Sarı Çizmeli".into(),
            playing: true,
            can_previous: true,
            can_next: true,
            cover: Some(42),
        };
        let l = layout(&view(&card, &i18n, 1.0, &TIP), Point::default(), &Fake);
        let t = texts(&l);
        assert!(t.contains(&"Mezdeke".to_string()) && t.contains(&"Barış Manço".to_string()));
        assert!(t.contains(&"Sarı Çizmeli".to_string()) && t.contains(&PAUSE.to_string()));
        assert_eq!(hits(&l), [0, 1, 2]);
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Image { spec, .. } if spec.key == 42)));
        for v in 0..3 {
            assert_eq!(Button::from_value(v).map(|b| b as u32), Some(v));
        }
        assert_eq!(Button::from_value(3), None);
    }

    #[test]
    fn what_the_session_does_not_take_is_not_clickable() {
        let i18n = I18n::new(Lang::Tr);
        let paused = Card {
            title: "Podcast".into(),
            ..Card::default()
        };
        let l = layout(&view(&paused, &i18n, 1.0, &TIP), Point::default(), &Fake);
        assert_eq!(hits(&l), [1], "only play");
        let t = texts(&l);
        assert!(t.contains(&PLAY.to_string()));
        assert_eq!(t.len(), 4, "the title and three signs: {t:?}");
        assert!(!l.draws.iter().any(|d| matches!(d, Draw::Image { .. })), "no cover");
        let none = layout(&view(&Card::default(), &i18n, 1.0, &TIP), Point::default(), &Fake);
        assert!(texts(&none).contains(&i18n.tr("media-nothing")));
        assert!(hits(&none).is_empty());
        for lang in [Lang::Tr, Lang::En] {
            assert_ne!(I18n::new(lang).tr("media-nothing"), "media-nothing");
        }
    }
}
