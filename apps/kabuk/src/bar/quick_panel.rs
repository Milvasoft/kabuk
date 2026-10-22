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
/// The monitor chooser's first button; monitor `screens[i]` is `SCREEN + i` (D91).
pub const SCREEN: u32 = 40;

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
/// The monitor chooser's buttons (`.kp-screens .kp-btn`).
const CHIP_PAD: (f32, f32) = (10.0, 5.0);
const CHIP_RADIUS: f32 = 6.0;
const CHIP_GAP: f32 = 6.0;
const CHIP_INSET: f32 = 4.0;

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

/// A monitor whose brightness answers: Windows' number for it ("Monitor 1 (main)"),
/// whether it is the main one, and its brightness (DECISIONS D91).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub number: u32,
    pub main: bool,
    pub percent: u8,
}

/// What the panel knows; `None` (or none) for what the computer has none of.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quick {
    pub wifi: Option<bool>,
    pub ssid: Option<String>,
    pub bluetooth: Option<bool>,
    pub airplane: Option<bool>,
    pub hotspot: Option<bool>,
    /// The monitors whose brightness answers, and which one the level sets (D91).
    pub screens: Vec<Screen>,
    pub screen: usize,
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
    /// The brightness level sets monitor `screens[i]` from now on.
    Screen(usize),
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

    /// The brightness of the monitor the level sets.
    pub fn brightness(&self) -> Option<u8> {
        self.screens.get(self.screen).map(|s| s.percent)
    }

    /// Takes the monitors read now: the level keeps setting the monitor it set (by its
    /// number), at first the one under the panel (`home`), else the first. While
    /// `dragging`, the chosen monitor keeps the value under the mouse.
    pub fn take_screens(&mut self, screens: Vec<Screen>, home: u32, dragging: bool) {
        let before = self.screens.get(self.screen).copied();
        let number = before.map_or(home, |s| s.number);
        self.screen = screens.iter().position(|s| s.number == number).unwrap_or(0);
        self.screens = screens;
        if let (true, Some(b), Some(s)) = (dragging, before, self.screens.get_mut(self.screen)) {
            if s.number == b.number {
                s.percent = b.percent;
            }
        }
    }

    /// A click on part `value`.
    pub fn click(&self, value: u32) -> Action {
        if value == SETTINGS {
            return Action::Open(SETTINGS_PAGE);
        }
        if let Some(i) = value.checked_sub(SCREEN).map(|i| i as usize) {
            return if i < self.screens.len() {
                Action::Screen(i)
            } else {
                Action::None
            };
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

/// The monitor chooser: a button for each monitor whose brightness answers, "1 (main)"
/// for the main one, the one the level sets lit (`.kp-screens`).
fn chooser(q: &Quick, i18n: &I18n, size: f32) -> Node {
    let buttons = q
        .screens
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let lit = i == q.screen;
            let label = if s.main {
                i18n.tr_args("qs-screen-main", &[("value", s.number.into())])
            } else {
                s.number.to_string()
            };
            Node::row(vec![words(
                label,
                if lit { Role::OnAccent } else { Role::Fg },
                400,
                size,
                WIDTH,
            )])
            .padding(Insets::symmetric(CHIP_PAD.0, CHIP_PAD.1))
            .paint(Paint {
                fill: if lit { Role::AccentFill } else { Role::Surface },
                border: (!lit).then_some((Role::SurfaceLine, 1.0)),
                radius: CHIP_RADIUS,
            })
            .hit(HIT, SCREEN + i as u32)
        })
        .collect();
    Node::row(buttons)
        .gap(CHIP_GAP)
        .padding(Insets::symmetric(CHIP_INSET, 0.0))
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
    // With more than one monitor answering, which one the level sets (D91).
    if q.screens.len() > 1 {
        rows.push(chooser(q, i18n, size));
    }
    if let Some(b) = q.brightness() {
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
    //! and plugged; every string in both languages; the monitor chooser with one monitor
    //! (none), two (the main one named, the chosen one lit) and a click past its buttons.
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
            screens: vec![Screen {
                number: 1,
                main: true,
                percent: 70,
            }],
            screen: 0,
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

    /// Asked for by the user: each monitor's brightness, one level and a chooser above it
    /// (D91).
    #[test]
    fn the_level_sets_the_chosen_monitors_brightness() {
        let notify = NotifyStyle::of(Look::Ada);
        let two = Quick {
            screens: vec![
                Screen {
                    number: 1,
                    main: true,
                    percent: 18,
                },
                Screen {
                    number: 2,
                    main: false,
                    percent: 20,
                },
            ],
            screen: 1,
            ..full()
        };
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let main = i18n.tr_args("qs-screen-main", &[("value", 1u32.into())]);
            assert!(main.contains('1') && main.len() > 1, "{main}");
            // One monitor: no chooser.
            let t = texts(&view(&full(), &i18n, 1.0, &TIP, &notify));
            assert!(!t.contains(&main), "{t:?}");
            // Two: both named, the level at the chosen one's brightness.
            let n = view(&two, &i18n, 1.0, &TIP, &notify);
            let t = texts(&n);
            assert!(t.contains(&main) && t.contains(&"2".to_string()), "{t:?}");
            assert!(t.contains(&"20".to_string()) && !t.contains(&"18".to_string()), "{t:?}");
            let l = layout(&n, Point::default(), &Fake);
            let button = |i: u32| {
                l.hits
                    .iter()
                    .find(|(_, h)| h.module == HIT && h.value == SCREEN + i)
                    .map(|(r, _)| *r)
                    .expect("a chooser button")
            };
            // The chosen one lit, the other not.
            let lit = |r: Rect| {
                l.draws.iter().any(
                    |d| matches!(d, Draw::Panel { rect, paint, .. } if *rect == r && paint.fill == Role::AccentFill),
                )
            };
            assert!(lit(button(1)) && !lit(button(0)));
            // Above the level.
            let level = l
                .hits
                .iter()
                .find(|(_, h)| h.module == HIT && h.value == BRIGHTNESS)
                .unwrap()
                .0;
            assert!(button(0).bottom() <= level.y);
        }
        // Read again: the chosen monitor stays chosen; at first the panel's own; one gone,
        // the first; while dragged, the value under the mouse stays.
        let screens = two.screens.clone();
        let mut q = Quick::default();
        q.take_screens(screens.clone(), 2, false);
        assert_eq!(q.screen, 1);
        q.take_screens(screens.iter().rev().copied().collect(), 1, false);
        assert_eq!(q.screens[q.screen].number, 2, "kept by its number");
        q.take_screens(screens[..1].to_vec(), 2, false);
        assert_eq!(q.screen, 0);
        let mut d = two.clone();
        d.screens[1].percent = 55;
        d.take_screens(screens.clone(), 1, true);
        assert_eq!(d.brightness(), Some(55));
        d.take_screens(screens, 1, false);
        assert_eq!(d.brightness(), Some(20));
        assert_eq!(two.click(SCREEN), Action::Screen(0));
        assert_eq!(two.click(SCREEN + 1), Action::Screen(1));
        assert_eq!(two.click(SCREEN + 2), Action::None);
        assert_eq!(two.brightness(), Some(20));
        assert_eq!(Quick::default().brightness(), None);
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
                screens: Vec::new(),
                volume: Some((40, true)),
                ..q.clone()
            };
            let t = texts(&view(&quiet, &i18n, 1.0, &TIP, &notify));
            assert!(!t.contains(&"70".to_string()) && t.contains(&"0".to_string()));
        }
    }
}
