//! The quick settings panel (DECISIONS D88): what a click on the bar's quick settings
//! button or the battery opens, Kabuk's own as Windows' Win+A panel. Tiles that switch
//! (Wi-Fi, Bluetooth, airplane mode, the mobile hotspot) and tiles that open Windows' own
//! page where Windows gives no public switch (night light, battery saver, focus,
//! accessibility, project), the brightness and the volume to drag, the battery and every
//! setting. Drawn as the design page's `qsPanelHtml`. Pure.

use kabuk_core::battery::Battery;
use kabuk_i18n::I18n;
use kabuk_ui::geom::{Insets, Rect};
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;
use super::volume_panel::{percent_at, slider};
use crate::notify::style::{NotifyStyle, Track};

/// Hit-test id of the panel's parts: a tile ([`Tile`] as its value), a level, the link.
pub const HIT: &str = "quick-panel";
pub const BRIGHTNESS: u32 = 20;
pub const VOLUME: u32 = 21;
pub const SETTINGS: u32 = 30;
pub const SETTINGS_PAGE: &str = "ms-settings:";

/// The content width (DIPs), as the other panels'; tiles in three columns.
const WIDTH: f32 = 356.0;
const COLUMNS: usize = 3;
const TILE_GAP: f32 = 6.0;
const TILE_H: f32 = 64.0;
const TILE_ICON: f32 = 18.0;
const TILE_RADIUS: f32 = 8.0;
/// A level's icon and the width of its figure.
const LEVEL_ICON: f32 = 18.0;
const LEVEL_VALUE_W: f32 = 30.0;
const GAP: f32 = 10.0;
const SUB_SCALE: f32 = 0.88;

/// A tile, in the design's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    Airplane,
    Hotspot,
    NightLight,
    BatterySaver,
    Focus,
    Accessibility,
    Project,
}

impl Tile {
    pub const ALL: [Tile; 9] = [
        Tile::Wifi,
        Tile::Bluetooth,
        Tile::Airplane,
        Tile::Hotspot,
        Tile::NightLight,
        Tile::BatterySaver,
        Tile::Focus,
        Tile::Accessibility,
        Tile::Project,
    ];

    fn value(self) -> u32 {
        Tile::ALL.iter().position(|t| *t == self).map_or(u32::MAX, |i| i as u32)
    }

    fn icon(self) -> &'static str {
        match self {
            Tile::Wifi => "wifi",
            Tile::Bluetooth => "bt",
            Tile::Airplane => "plane",
            Tile::Hotspot => "hotspot",
            Tile::NightLight => "moon",
            Tile::BatterySaver => "leaf",
            Tile::Focus => "bell",
            Tile::Accessibility => "access",
            Tile::Project => "cast",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tile::Wifi => "net-wifi",
            Tile::Bluetooth => "qs-bluetooth",
            Tile::Airplane => "qs-airplane",
            Tile::Hotspot => "qs-hotspot",
            Tile::NightLight => "qs-night",
            Tile::BatterySaver => "qs-saver",
            Tile::Focus => "qs-focus",
            Tile::Accessibility => "qs-access",
            Tile::Project => "qs-project",
        }
    }

    /// Windows' page for the tiles Kabuk cannot switch itself.
    pub fn page(self) -> Option<&'static str> {
        match self {
            Tile::NightLight => Some("ms-settings:nightlight"),
            Tile::BatterySaver => Some("ms-settings:batterysaver"),
            Tile::Focus => Some("ms-settings:focus"),
            Tile::Accessibility => Some("ms-settings:easeofaccess"),
            Tile::Project => Some("ms-settings-displays-topology:"),
            _ => None,
        }
    }
}

/// What the panel knows; `None` for what the computer has none of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quick {
    pub wifi: Option<bool>,
    pub ssid: Option<String>,
    pub bluetooth: Option<bool>,
    pub airplane: Option<bool>,
    pub hotspot: Option<bool>,
    pub brightness: Option<u8>,
    pub volume: Option<(u32, bool)>,
    pub battery: Option<Battery>,
}

/// What the host has to do after a click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Switch a tile's radio or the hotspot on (`true`) or off.
    Switch(Tile, bool),
    Open(&'static str),
}

impl Quick {
    /// The tiles shown: the switches the computer has, the pages always (battery saver
    /// only with a battery).
    pub fn tiles(&self) -> Vec<Tile> {
        Tile::ALL
            .into_iter()
            .filter(|t| match t {
                Tile::Wifi => self.wifi.is_some(),
                Tile::Bluetooth => self.bluetooth.is_some(),
                Tile::Airplane => self.airplane.is_some(),
                Tile::Hotspot => self.hotspot.is_some(),
                Tile::BatterySaver => self.battery.is_some(),
                _ => true,
            })
            .collect()
    }

    fn on(&self, t: Tile) -> Option<bool> {
        match t {
            Tile::Wifi => self.wifi,
            Tile::Bluetooth => self.bluetooth,
            Tile::Airplane => self.airplane,
            Tile::Hotspot => self.hotspot,
            _ => None,
        }
    }

    /// A click on part `value`.
    pub fn click(&self, value: u32) -> Action {
        if value == SETTINGS {
            return Action::Open(SETTINGS_PAGE);
        }
        let Some(t) = Tile::ALL.get(value as usize).copied() else {
            return Action::None;
        };
        match (t.page(), self.on(t)) {
            (Some(page), _) => Action::Open(page),
            (None, Some(on)) => Action::Switch(t, !on),
            (None, None) => Action::None,
        }
    }
}

/// The level a press at `x` (DIPs) on level `which` sets, from its track's place.
pub fn level_at(x: f32, track: Rect) -> u32 {
    percent_at(x, track)
}

fn words(text: String, color: Role, weight: u16, size: f32, max_width: f32) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text,
            color,
            weight,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Ui,
        size,
        max_width: Some(max_width),
    })
}

fn tile(q: &Quick, t: Tile, i18n: &I18n, size: f32, width: f32) -> Node {
    let on = q.on(t);
    let lit = on == Some(true);
    let (fg, dim) = if lit {
        (Role::OnAccent, Role::OnAccent)
    } else {
        (Role::Fg, Role::Dim)
    };
    let sub = match (t, on) {
        (Tile::Wifi, Some(true)) => q.ssid.clone().unwrap_or_else(|| i18n.tr("qs-on")),
        (_, Some(true)) => i18n.tr("qs-on"),
        (_, Some(false)) => i18n.tr("qs-off"),
        (_, None) => String::new(),
    };
    let text_w = width - 2.0 * TILE_GAP;
    let mut lines = vec![
        Node::icon(t.icon(), TILE_ICON, fg),
        words(i18n.tr(t.label()), fg, 600, size, text_w),
    ];
    if !sub.is_empty() {
        lines.push(words(sub, dim, 400, size * SUB_SCALE, text_w));
    }
    // A layer centres its content both ways: one line or two, a tile's content sits in
    // its middle (found by the user: the ones without a second line hung at the top).
    Node::layer(vec![Node::column(lines).gap(4.0).align(Align::Center)])
        .size(Some(width), Some(TILE_H))
        .paint(Paint {
            fill: if lit { Role::AccentFill } else { Role::Surface },
            border: (!lit).then_some((Role::SurfaceLine, 1.0)),
            radius: TILE_RADIUS,
        })
        .hit(HIT, t.value())
}

fn level(icon: &'static str, percent: u32, value: u32, notify: &NotifyStyle, size: f32) -> Node {
    let track = Track {
        width: WIDTH - LEVEL_ICON - LEVEL_VALUE_W - 2.0 * GAP,
        ..notify.track
    };
    Node::row(vec![
        Node::icon(icon, LEVEL_ICON, Role::Fg),
        slider(&track, percent as f32 / 100.0).hit(HIT, value),
        Node::column(vec![Node::text(TextSpec {
            spans: vec![Span {
                text: percent.to_string(),
                color: Role::Dim,
                weight: 400,
                after: 0.0,
                underline: false,
                italic: false,
                spacing: 0.0,
            }],
            font: FontRole::Num,
            size,
            max_width: None,
        })])
        .align(Align::End)
        .size(Some(LEVEL_VALUE_W), None),
    ])
    .gap(GAP)
    .align(Align::Center)
}

/// The battery's line: its charge and the time left (hours and minutes when known).
fn battery_line(b: &Battery, i18n: &I18n) -> String {
    let pct = i18n.tr_args("bar-percent", &[("value", u32::from(b.percent).into())]);
    match (b.charging, b.left()) {
        (true, _) => format!("{pct} · {}", i18n.tr("batt-charging")),
        (false, Some((h, m))) => {
            let left = i18n.tr_args("qs-hours-minutes", &[("hours", h.into()), ("minutes", m.into())]);
            format!("{pct} · {}", i18n.tr_args("batt-left", &[("value", left.into())]))
        }
        (false, None) if b.plugged => format!("{pct} · {}", i18n.tr("batt-full")),
        (false, None) => pct,
    }
}

/// The panel's tree.
pub fn view(q: &Quick, i18n: &I18n, text_scale: f32, s: &TipStyle, notify: &NotifyStyle) -> Node {
    let size = s.text * text_scale;
    let width = (WIDTH - TILE_GAP * (COLUMNS as f32 - 1.0)) / COLUMNS as f32;
    let tiles = q.tiles();
    let mut rows: Vec<Node> = tiles
        .chunks(COLUMNS)
        .map(|row| Node::row(row.iter().map(|t| tile(q, *t, i18n, size, width)).collect()).gap(TILE_GAP))
        .collect();
    if let Some(b) = q.brightness {
        rows.push(level("sun", u32::from(b), BRIGHTNESS, notify, size));
    }
    if let Some((v, muted)) = q.volume {
        rows.push(level("vol", if muted { 0 } else { v.min(100) }, VOLUME, notify, size));
    }
    rows.push(Node::spacer(WIDTH, 1.0).paint(Paint {
        fill: Role::SurfaceLine,
        border: None,
        radius: 0.0,
    }));
    let settings = Node::row(vec![
        Node::icon("gear", LEVEL_ICON * 0.8, Role::Accent),
        words(i18n.tr("qs-settings"), Role::Accent, 400, size * SUB_SCALE, WIDTH / 2.0),
    ])
    .gap(6.0)
    .align(Align::Center)
    .hit(HIT, SETTINGS);
    let left = match &q.battery {
        Some(b) => Node::row(vec![
            Node::icon("batt", LEVEL_ICON * 0.8, Role::Dim),
            words(battery_line(b, i18n), Role::Dim, 400, size * SUB_SCALE, WIDTH * 0.6),
        ])
        .gap(6.0)
        .align(Align::Center),
        None => Node::spacer(0.0, 0.0),
    };
    rows.push(
        Node::row(vec![
            Node::column(vec![left])
                .align(Align::Start)
                .size(Some(WIDTH * 0.6), None),
            Node::column(vec![settings])
                .align(Align::End)
                .size(Some(WIDTH * 0.4), None),
        ])
        .align(Align::Center),
    );
    Node::column(rows)
        .align(Align::Start)
        .gap(8.0)
        .padding(Insets::all(s.pad_h + s.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: tiles for what the computer has only (no Bluetooth, no battery, no
    //! hotspot); a switch flips its state, a page tile opens its page; the levels only
    //! when known, a muted volume at 0; the battery's line charging, with time left, full
    //! and plugged; every string in both languages.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_core::config::Look;
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

    fn full() -> Quick {
        Quick {
            wifi: Some(true),
            ssid: Some("milva-5g".into()),
            bluetooth: Some(false),
            airplane: Some(false),
            hotspot: Some(false),
            brightness: Some(70),
            volume: Some((40, false)),
            battery: Some(Battery {
                percent: 82,
                charging: false,
                plugged: false,
                seconds_left: Some(3 * 3600 + 12 * 60),
            }),
        }
    }

    fn texts(n: &Node) -> Vec<String> {
        layout(n, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.plain()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_what_the_computer_has_is_a_tile() {
        assert_eq!(full().tiles().len(), 9);
        let desktop = Quick {
            bluetooth: None,
            hotspot: None,
            battery: None,
            ..full()
        };
        let tiles = desktop.tiles();
        assert!(!tiles.contains(&Tile::Bluetooth) && !tiles.contains(&Tile::Hotspot));
        assert!(!tiles.contains(&Tile::BatterySaver) && tiles.contains(&Tile::NightLight));
    }

    #[test]
    fn a_switch_flips_and_a_page_opens() {
        let q = full();
        assert_eq!(q.click(Tile::Wifi.value()), Action::Switch(Tile::Wifi, false));
        assert_eq!(q.click(Tile::Bluetooth.value()), Action::Switch(Tile::Bluetooth, true));
        assert_eq!(q.click(Tile::Airplane.value()), Action::Switch(Tile::Airplane, true));
        assert_eq!(
            q.click(Tile::NightLight.value()),
            Action::Open("ms-settings:nightlight")
        );
        assert_eq!(
            q.click(Tile::Project.value()),
            Action::Open("ms-settings-displays-topology:")
        );
        assert_eq!(q.click(SETTINGS), Action::Open(SETTINGS_PAGE));
        assert_eq!(q.click(99), Action::None);
    }

    /// Found by the user with a photo: a tile without a second line had its icon and
    /// name at its top, under its neighbours' and over empty space. Every tile's content
    /// sits in its middle.
    #[test]
    fn a_tiles_content_sits_in_its_middle() {
        let i18n = I18n::new(Lang::Tr);
        let l = layout(
            &view(&full(), &i18n, 1.0, &TIP, &NotifyStyle::of(Look::Ada)),
            Point::default(),
            &Fake,
        );
        let mid = |r: &Rect| Point {
            x: r.x + r.w / 2.0,
            y: r.y + r.h / 2.0,
        };
        let tile_of = |value: u32| {
            l.hits
                .iter()
                .find(|(_, h)| h.module == HIT && h.value == value)
                .unwrap()
                .0
        };
        let icon_in = |r: Rect| {
            l.draws
                .iter()
                .find_map(|d| match d {
                    Draw::Icon { rect, .. } if r.contains(mid(rect)) => Some(*rect),
                    _ => None,
                })
                .unwrap()
        };
        let last_text_in = |r: Rect| {
            l.draws
                .iter()
                .rev()
                .find_map(|d| match d {
                    Draw::Text { rect, .. } if r.contains(mid(rect)) => Some(*rect),
                    _ => None,
                })
                .unwrap()
        };
        for tile in [Tile::Wifi, Tile::NightLight, Tile::Project] {
            let r = tile_of(tile.value());
            let (top, bottom) = (icon_in(r).y - r.y, r.bottom() - last_text_in(r).bottom());
            assert!((top - bottom).abs() < 0.5, "{tile:?}: {top} above, {bottom} below");
        }
    }

    #[test]
    fn the_panel_draws_tiles_levels_and_the_battery() {
        let notify = NotifyStyle::of(Look::Ada);
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let q = full();
            let n = view(&q, &i18n, 1.0, &TIP, &notify);
            let t = texts(&n);
            assert!(t.contains(&"milva-5g".to_string()), "the network under Wi-Fi");
            for key in ["qs-bluetooth", "qs-airplane", "qs-night", "qs-settings", "qs-off"] {
                assert!(t.contains(&i18n.tr(key)), "{key}");
            }
            assert!(t.contains(&"70".to_string()) && t.contains(&"40".to_string()));
            assert!(
                t.iter()
                    .any(|x| x.starts_with("82%") && x.contains('3') && x.contains("12")),
                "{t:?}"
            );
            let l = layout(&n, Point::default(), &Fake);
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            for v in [BRIGHTNESS, VOLUME, SETTINGS, Tile::Wifi.value(), Tile::Project.value()] {
                assert!(hits.contains(&v), "{v} not clickable");
            }
            // The lit tile on the accent.
            assert!(l
                .draws
                .iter()
                .any(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::AccentFill)));
            // Charging and full.
            let mut c = q.clone();
            c.battery = Some(Battery {
                percent: 50,
                charging: true,
                plugged: true,
                seconds_left: None,
            });
            assert!(texts(&view(&c, &i18n, 1.0, &TIP, &notify))
                .iter()
                .any(|x| x.contains(&i18n.tr("batt-charging"))));
            c.battery = Some(Battery {
                percent: 100,
                charging: false,
                plugged: true,
                seconds_left: None,
            });
            assert!(texts(&view(&c, &i18n, 1.0, &TIP, &notify))
                .iter()
                .any(|x| x.contains(&i18n.tr("batt-full"))));
            // Unknown levels are not drawn; a muted volume is 0.
            let quiet = Quick {
                brightness: None,
                volume: Some((40, true)),
                ..q.clone()
            };
            let t = texts(&view(&quiet, &i18n, 1.0, &TIP, &notify));
            assert!(!t.contains(&"70".to_string()) && t.contains(&"0".to_string()));
        }
    }
}
