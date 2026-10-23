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
pub const VOLUME: u32 = 21;
pub const SETTINGS: u32 = 30;
pub const SETTINGS_PAGE: &str = "ms-settings:";
/// The switch that keeps every monitor's brightness together (D91).
pub const SYNC: u32 = 31;
/// The first brightness level; monitor `screens[i]`'s is `SCREEN + i` (D91).
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
/// A monitor level's number at its icon's lower right (`.kp-slider .scr sub`): its size
/// against the text's, its padding and corners, how far it hangs past the icon.
const BADGE_SCALE: f32 = 0.75;
const BADGE_PAD: f32 = 2.0;
const BADGE_RADIUS: f32 = 3.0;
const BADGE_OUT: f32 = 4.0;
/// The sync row's inset (`.kp-sync`).
const SYNC_INSET: f32 = 4.0;

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
    /// The monitors whose brightness answers, each with its level, and whether a level
    /// moves them all (D91).
    pub screens: Vec<Screen>,
    pub sync: bool,
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
    /// Keep every monitor's brightness together (`true`, at the main one's) or not.
    Sync(bool),
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

    /// Sets monitor `screens[i]`'s brightness, every monitor's while they are kept
    /// together; the monitors changed.
    pub fn set_brightness(&mut self, i: usize, percent: u8) -> Vec<usize> {
        let changed: Vec<usize> = if self.sync {
            (0..self.screens.len()).collect()
        } else if i < self.screens.len() {
            vec![i]
        } else {
            Vec::new()
        };
        for j in &changed {
            self.screens[*j].percent = percent;
        }
        changed
    }

    /// Keeps every monitor's brightness together (`on`) or not; turned on, they all take
    /// the main one's (else the first's). The monitors changed.
    pub fn set_sync(&mut self, on: bool) -> Vec<usize> {
        self.sync = on;
        let main = self
            .screens
            .iter()
            .find(|s| s.main)
            .or(self.screens.first())
            .map(|s| s.percent);
        let Some(p) = main.filter(|_| on) else {
            return Vec::new();
        };
        let changed: Vec<usize> = (0..self.screens.len())
            .filter(|j| self.screens[*j].percent != p)
            .collect();
        for j in &changed {
            self.screens[*j].percent = p;
        }
        changed
    }

    /// Takes the monitors read now; while level `dragging` is held, the value under the
    /// mouse stays (on every monitor while they are kept together).
    pub fn take_screens(&mut self, screens: Vec<Screen>, dragging: Option<usize>) {
        let held = dragging.and_then(|i| self.screens.get(i)).copied();
        self.screens = screens;
        let sync = self.sync;
        if let Some(h) = held {
            for s in self.screens.iter_mut().filter(|s| sync || s.number == h.number) {
                s.percent = h.percent;
            }
        }
    }

    /// The monitor whose brightness level is part `value`.
    pub fn level_of(&self, value: u32) -> Option<usize> {
        let i = value.checked_sub(SCREEN)? as usize;
        (i < self.screens.len()).then_some(i)
    }

    /// A click on part `value`.
    pub fn click(&self, value: u32) -> Action {
        if value == SETTINGS {
            return Action::Open(SETTINGS_PAGE);
        }
        if value == SYNC {
            return Action::Sync(!self.sync);
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

fn level(icon: Node, percent: u32, value: u32, notify: &NotifyStyle, size: f32) -> Node {
    let track = Track {
        width: WIDTH - LEVEL_ICON - LEVEL_VALUE_W - 2.0 * GAP,
        ..notify.track
    };
    Node::row(vec![
        icon,
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

/// A monitor's level icon: a monitor with a sun, its number at the lower right
/// (`.kp-slider .scr`).
fn screen_icon(number: u32, size: f32) -> Node {
    let badge = Node::row(vec![Node::text(TextSpec {
        spans: vec![Span {
            text: number.to_string(),
            color: Role::Fg,
            weight: 700,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Num,
        size: size * BADGE_SCALE,
        max_width: None,
    })])
    .padding(Insets::symmetric(BADGE_PAD, 0.0))
    .paint(Paint {
        fill: Role::Surface,
        border: None,
        radius: BADGE_RADIUS,
    });
    let half = LEVEL_ICON / 2.0;
    Node::layer(vec![
        Node::icon("screen-sun", LEVEL_ICON, Role::Fg),
        badge.offset(half, half - BADGE_OUT / 2.0),
    ])
    .size(Some(LEVEL_ICON), Some(LEVEL_ICON))
}

/// The row over the monitors' levels: its words and the switch that keeps them together.
fn sync_row(q: &Quick, i18n: &I18n, size: f32) -> Node {
    let (switch_w, _) = super::network_panel::SWITCH;
    Node::row(vec![
        Node::column(vec![words(i18n.tr("qs-sync"), Role::Fg, 400, size, WIDTH - switch_w)])
            .align(Align::Start)
            .size(Some(WIDTH - switch_w - 2.0 * SYNC_INSET), None),
        super::network_panel::switch(q.sync, HIT, SYNC),
    ])
    .align(Align::Center)
    .padding(Insets::symmetric(SYNC_INSET, 0.0))
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
    // A level for each monitor whose brightness answers, under the switch that keeps
    // them together; one alone keeps the sun (D91).
    let many = q.screens.len() > 1;
    if many {
        rows.push(sync_row(q, i18n, size));
    }
    for (i, screen) in q.screens.iter().enumerate() {
        let icon = if many {
            screen_icon(screen.number, size)
        } else {
            Node::icon("sun", LEVEL_ICON, Role::Fg)
        };
        rows.push(level(icon, u32::from(screen.percent), SCREEN + i as u32, notify, size));
    }
    if let Some((v, muted)) = q.volume {
        rows.push(level(
            Node::icon("vol", LEVEL_ICON, Role::Fg),
            if muted { 0 } else { v.min(100) },
            VOLUME,
            notify,
            size,
        ));
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
    //! and plugged; every string in both languages; one monitor's level (the sun, no
    //! switch) and two (each its own, or together at the main one's).
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
            sync: false,
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

    /// Asked for by the user: a level for each monitor, one under the other, each with a
    /// monitor icon and its number, under a switch that keeps them together (D91).
    #[test]
    fn each_monitor_has_its_level_and_the_switch_keeps_them_together() {
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
            sync: false,
            ..full()
        };
        let hit = |l: &kabuk_ui::node::Layout, value: u32| {
            l.hits
                .iter()
                .find(|(_, h)| h.module == HIT && h.value == value)
                .map(|(r, _)| *r)
        };
        let icons = |l: &kabuk_ui::node::Layout, name: &str| {
            l.draws
                .iter()
                .filter(|d| matches!(d, Draw::Icon { spec, .. } if spec.name == name))
                .count()
        };
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let sync = i18n.tr("qs-sync");
            // One monitor: its level with the sun, no switch.
            let n = view(&full(), &i18n, 1.0, &TIP, &notify);
            let l = layout(&n, Point::default(), &Fake);
            assert!(!texts(&n).contains(&sync));
            assert!(hit(&l, SCREEN).is_some() && hit(&l, SYNC).is_none());
            assert_eq!((icons(&l, "sun"), icons(&l, "screen-sun")), (1, 0));
            // Two: the switch, then a level for each with its number and value.
            let n = view(&two, &i18n, 1.0, &TIP, &notify);
            let t = texts(&n);
            for x in [sync.as_str(), "1", "2", "18", "20"] {
                assert!(t.contains(&x.to_string()), "{x} in {t:?}");
            }
            let l = layout(&n, Point::default(), &Fake);
            let (switch, first, second) = (
                hit(&l, SYNC).unwrap(),
                hit(&l, SCREEN).unwrap(),
                hit(&l, SCREEN + 1).unwrap(),
            );
            assert!(switch.bottom() <= first.y && first.bottom() <= second.y);
            assert_eq!((icons(&l, "sun"), icons(&l, "screen-sun")), (0, 2));
        }
        // Off: a level moves its own monitor.
        let mut q = two.clone();
        let percents = |q: &Quick| q.screens.iter().map(|s| s.percent).collect::<Vec<_>>();
        assert_eq!(q.set_brightness(1, 50), vec![1]);
        assert_eq!(percents(&q), vec![18, 50]);
        // Turned on: every monitor at the main one's, then a level moves them all.
        assert_eq!(q.click(SYNC), Action::Sync(true));
        assert_eq!(q.set_sync(true), vec![1]);
        assert_eq!(percents(&q), vec![18, 18]);
        assert_eq!(q.set_brightness(1, 60), vec![0, 1]);
        assert_eq!(percents(&q), vec![60, 60]);
        // Turned off: nothing moves.
        assert_eq!(q.click(SYNC), Action::Sync(false));
        assert!(q.set_sync(false).is_empty());
        assert_eq!(q.set_brightness(0, 10), vec![0]);
        // The main one leads wherever it is listed.
        let mut m = two.clone();
        m.screens[0].main = false;
        m.screens[1].main = true;
        m.set_sync(true);
        assert_eq!(percents(&m), vec![20, 20]);
        // Read again: a level held keeps the value under the mouse (every monitor's while
        // they are together); let go, Windows' values.
        let screens = two.screens.clone();
        let mut d = two.clone();
        d.screens[1].percent = 55;
        d.take_screens(screens.clone(), Some(1));
        assert_eq!(percents(&d), vec![18, 55]);
        d.sync = true;
        d.screens[1].percent = 40;
        d.take_screens(screens.clone(), Some(1));
        assert_eq!(percents(&d), vec![40, 40]);
        d.take_screens(screens, None);
        assert_eq!(percents(&d), vec![18, 20]);
        assert_eq!(two.level_of(SCREEN + 1), Some(1));
        assert_eq!(two.level_of(SCREEN + 2), None);
        assert_eq!(two.level_of(VOLUME), None);
        assert_eq!(two.click(SCREEN + 1), Action::None);
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
            for v in [SCREEN, VOLUME, SETTINGS, Tile::Wifi.value(), Tile::Project.value()] {
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
