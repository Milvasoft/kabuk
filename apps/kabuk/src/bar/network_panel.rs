//! The network panel (DECISIONS D88): what a click on the bar's network indicator opens,
//! Kabuk's own in the look's surface rather than Windows' flyout. Wi-Fi on and off, the
//! networks around (the connected one first), joining one (a key asked under its row
//! for a new protected network), leaving it, the Ethernet line and Windows' network
//! settings. Drawn as the design page's `netPanelHtml`. The keyboard moves over its parts
//! as in every popup ([`crate::bar::popup_keys`], PLAN M19 U10); while a key is asked,
//! what is typed goes into its field (Space too), Enter joins unless a button is
//! selected, and Esc leaves the field before the panel. Pure: state and keys in, what to
//! do and the tree out.

use kabuk_core::network::{key_fits, wifi_icon, Network};
use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::popup_keys::{self, Step};
use super::tooltip::TipStyle;

/// Hit-test id of the panel's parts; the value is one of the constants below.
pub const HIT: &str = "network-panel";
pub const WIFI_SWITCH: u32 = 0;
pub const KEY_FIELD: u32 = 1;
pub const CANCEL: u32 = 2;
pub const CONNECT: u32 = 3;
pub const SETTINGS: u32 = 4;
pub const LOCATION: u32 = 5;
pub const DISCONNECT: u32 = 6;
/// The networks' rows, in the panel's order.
pub const FIRST_NETWORK: u32 = 100;

/// Windows' pages the panel's links open.
pub const SETTINGS_PAGE: &str = "ms-settings:network";
pub const LOCATION_PAGE: &str = "ms-settings:privacy-location";

/// The panel's content width (DIPs): the design's `.kpanel` (380 wide, 12 padding).
const WIDTH: f32 = 356.0;
/// A network row's height, its icon, the space between the icon and the words.
const ROW: f32 = 44.0;
const ICON: f32 = 20.0;
const ICON_GAP: f32 = 10.0;
/// The slot at a row's end for its button or mark.
const TRAIL: f32 = 120.0;
/// A small mark at a row's end (the lock, the chevron).
const MARK: f32 = 14.0;
/// The switch: its track and knob.
pub(super) const SWITCH: (f32, f32) = (36.0, 20.0);
const KNOB: f32 = 14.0;
/// A button's height and side padding; the key field's height.
const BUTTON_H: f32 = 28.0;
const BUTTON_PAD: f32 = 10.0;
const FIELD_H: f32 = 32.0;
/// The bullet shown for each character of a key.
const BULLET: char = '•';
/// The second line of a row is drawn this much smaller.
const SUB_SCALE: f32 = 0.9;
/// The most networks the panel lists.
pub const MAX_NETWORKS: usize = 12;

/// What the panel knows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Panel {
    /// The Wi-Fi radio: `None` without Wi-Fi hardware.
    pub wifi: Option<bool>,
    /// As [`kabuk_core::network::listed`] orders them.
    pub networks: Vec<Network>,
    pub scanning: bool,
    /// Windows refuses the names without location access for desktop apps.
    pub denied: bool,
    pub ethernet: bool,
    pub internet: bool,
    /// The network whose key is being typed, and the key so far.
    pub asking: Option<String>,
    pub key: String,
    /// The key typed is not a WPA key: said under the field.
    pub short_key: bool,
    /// Being joined, and the last one that could not be.
    pub joining: Option<String>,
    pub failed: Option<String>,
    /// The part the keyboard is on (its click value).
    pub selected: Option<u32>,
}

/// What the host has to do after a click or a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    /// Join the network, with the key typed for a new protected one.
    Join(Network, Option<String>),
    Disconnect,
    /// Turn the Wi-Fi radio on or off.
    Wifi(bool),
    Open(&'static str),
    Close,
}

/// A key the panel takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A key every popup takes: the selection's moves, Enter and Space.
    Popup(popup_keys::Key),
    Escape,
    Backspace,
    Text(String),
}

impl Panel {
    /// A click on part `value` of [`HIT`].
    pub fn click(&mut self, value: u32) -> Action {
        match value {
            WIFI_SWITCH => match self.wifi {
                Some(on) => Action::Wifi(!on),
                None => Action::None,
            },
            CANCEL => {
                self.stop_asking();
                Action::Redraw
            }
            CONNECT => self.join_asked(),
            SETTINGS => Action::Open(SETTINGS_PAGE),
            LOCATION => Action::Open(LOCATION_PAGE),
            DISCONNECT => Action::Disconnect,
            KEY_FIELD => Action::None,
            v if v >= FIRST_NETWORK => {
                self.selected = Some(v);
                self.choose((v - FIRST_NETWORK) as usize)
            }
            _ => Action::None,
        }
    }

    /// The network at `i` chosen: joined at once when Windows knows it or it is open,
    /// else its key asked; the connected one stays.
    fn choose(&mut self, i: usize) -> Action {
        let Some(n) = self.networks.get(i).cloned() else {
            return Action::None;
        };
        if n.connected || self.joining.as_deref() == Some(n.ssid.as_str()) {
            return Action::None;
        }
        if n.secured && !n.known {
            self.asking = Some(n.ssid);
            self.key.clear();
            self.short_key = false;
            return Action::Redraw;
        }
        self.stop_asking();
        Action::Join(n, None)
    }

    fn join_asked(&mut self) -> Action {
        let Some(n) = self
            .asking
            .as_ref()
            .and_then(|s| self.networks.iter().find(|n| &n.ssid == s))
            .cloned()
        else {
            return Action::None;
        };
        if !key_fits(&self.key) {
            self.short_key = true;
            return Action::Redraw;
        }
        let key = std::mem::take(&mut self.key);
        self.stop_asking();
        Action::Join(n, Some(key))
    }

    fn stop_asking(&mut self) {
        self.asking = None;
        self.key.clear();
        self.short_key = false;
    }

    /// A key, with `entries` the panel's parts as drawn (their click values, in order):
    /// the selection moves over them and Enter or Space clicks the one selected, as in
    /// every popup. While a key is asked, typing goes to its field and Enter joins (or
    /// clicks the button selected: Cancel, Connect). Esc leaves the field, then the panel.
    pub fn key(&mut self, key: Key, entries: &[u32]) -> Action {
        if self.asking.is_some() {
            return match key {
                Key::Escape => {
                    self.stop_asking();
                    Action::Redraw
                }
                Key::Popup(popup_keys::Key::Activate) => match self.selected {
                    Some(v) if v != KEY_FIELD && v < FIRST_NETWORK && entries.contains(&v) => self.click(v),
                    _ => self.join_asked(),
                },
                Key::Popup(k) => self.select(entries, k),
                Key::Backspace => {
                    self.short_key = false;
                    if self.key.pop().is_some() {
                        Action::Redraw
                    } else {
                        Action::None
                    }
                }
                Key::Text(t) => {
                    let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                    if clean.is_empty() {
                        return Action::None;
                    }
                    self.short_key = false;
                    self.key.push_str(&clean);
                    Action::Redraw
                }
            };
        }
        match key {
            Key::Escape => Action::Close,
            Key::Popup(k) => match popup_keys::step(entries, self.selected, k) {
                Step::Activate(v) => self.click(v),
                _ => self.select(entries, k),
            },
            Key::Backspace | Key::Text(_) => Action::None,
        }
    }

    /// Moves the selection with `key` over `entries`.
    fn select(&mut self, entries: &[u32], key: popup_keys::Key) -> Action {
        match popup_keys::step(entries, self.selected, key) {
            Step::Select(v) => {
                self.selected = Some(v);
                Action::Redraw
            }
            Step::Same | Step::Activate(_) | Step::Adjust(..) => Action::None,
        }
    }

    /// A selection kept only while its part is still listed (a network gone from the
    /// list, or past the ones shown, lets it go).
    pub fn keep_selection(&mut self) {
        let shown = u32::try_from(self.shown()).unwrap_or(u32::MAX);
        if self
            .selected
            .is_some_and(|v| v >= FIRST_NETWORK && v - FIRST_NETWORK >= shown)
        {
            self.selected = None;
        }
    }

    /// How many networks are listed.
    pub fn shown(&self) -> usize {
        if self.wifi == Some(true) && !self.denied {
            self.networks.len().min(MAX_NETWORKS)
        } else {
            0
        }
    }
}

fn span(text: String, color: Role, weight: u16) -> Span {
    Span {
        text,
        color,
        weight,
        after: 0.0,
        underline: false,
        italic: false,
        spacing: 0.0,
    }
}

fn words(text: String, color: Role, weight: u16, size: f32, max_width: f32) -> Node {
    Node::text(TextSpec {
        spans: vec![span(text, color, weight)],
        font: FontRole::Ui,
        size,
        max_width: Some(max_width),
    })
}

/// A two-slot row: `left` from the start, `right` against the end.
fn ends(left: Node, right: Node, right_w: f32) -> Node {
    Node::row(vec![
        Node::column(vec![left])
            .align(Align::Start)
            .size(Some(WIDTH - right_w), None),
        Node::column(vec![right]).align(Align::End).size(Some(right_w), None),
    ])
    .align(Align::Center)
}

/// The switch: the accent's track with the knob at its end when on; part `value` of
/// `module` (the quick settings' sync switch is one too, D91).
pub(super) fn switch(on: bool, module: &'static str, value: u32) -> Node {
    let (w, h) = SWITCH;
    let track = Node::spacer(w, h).paint(Paint {
        fill: if on { Role::AccentFill } else { Role::Surface },
        border: (!on).then_some((Role::SurfaceLine, 1.0)),
        radius: h / 2.0,
    });
    let travel = (w - h) / 2.0;
    let knob = Node::spacer(KNOB, KNOB)
        .paint(Paint {
            fill: if on { Role::OnAccent } else { Role::Dim },
            border: None,
            radius: KNOB / 2.0,
        })
        .offset(if on { travel } else { -travel }, 0.0);
    Node::layer(vec![track, knob]).size(Some(w), Some(h)).hit(module, value)
}

/// A button: its words in the middle of an outlined (or, `primary`, filled) box.
fn button(text: String, primary: bool, size: f32, value: u32) -> Node {
    let color = if primary { Role::OnAccent } else { Role::Fg };
    Node::row(vec![words(text, color, 400, size, TRAIL - 2.0 * BUTTON_PAD)])
        .padding(Insets::symmetric(BUTTON_PAD, 0.0))
        .size(None, Some(BUTTON_H))
        .paint(Paint {
            fill: if primary { Role::AccentFill } else { Role::Surface },
            border: (!primary).then_some((Role::SurfaceLine, 1.0)),
            radius: 6.0,
        })
        .hit(HIT, value)
}

/// A link: an icon and the accent's words.
fn link(icon: &'static str, text: String, size: f32, value: u32) -> Node {
    Node::row(vec![
        Node::icon(icon, MARK, Role::Accent),
        words(text, Role::Accent, 400, size, WIDTH / 2.0),
    ])
    .gap(6.0)
    .align(Align::Center)
    .hit(HIT, value)
}

/// What a network's second line says.
fn status_of(p: &Panel, n: &Network, i18n: &I18n) -> String {
    let key = if n.connected {
        if p.internet {
            "net-connected"
        } else {
            "net-no-internet"
        }
    } else if p.joining.as_deref() == Some(n.ssid.as_str()) {
        "net-connecting"
    } else if p.failed.as_deref() == Some(n.ssid.as_str()) {
        "net-failed"
    } else if n.secured {
        "net-secured"
    } else {
        "net-open"
    };
    i18n.tr(key)
}

/// The field a key is typed in, its buttons and what is wrong with it.
fn key_rows(p: &Panel, i18n: &I18n, size: f32) -> Vec<Node> {
    let indent = ICON + ICON_GAP;
    let field_w = WIDTH - indent;
    let shown = if p.key.is_empty() {
        words(i18n.tr("net-key"), Role::Dim, 400, size, field_w - 2.0 * BUTTON_PAD)
    } else {
        let dots: String = std::iter::repeat_n(BULLET, p.key.chars().count()).collect();
        words(dots, Role::Fg, 400, size, field_w - 2.0 * BUTTON_PAD)
    };
    let field = Node::column(vec![shown])
        .align(Align::Start)
        .padding(Insets::symmetric(BUTTON_PAD, 0.0))
        .size(Some(field_w), Some(FIELD_H))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::Accent, 1.0)),
            radius: 6.0,
        })
        .hit(HIT, KEY_FIELD);
    let mut rows = vec![Node::row(vec![Node::spacer(indent, 0.0), field])];
    if p.short_key {
        rows.push(Node::row(vec![
            Node::spacer(indent, 0.0),
            words(i18n.tr("net-key-short"), Role::Accent, 400, size * SUB_SCALE, field_w),
        ]));
    }
    let buttons = Node::row(vec![
        button(i18n.tr("net-cancel"), false, size, CANCEL),
        button(i18n.tr("net-connect"), true, size, CONNECT),
    ])
    .gap(8.0);
    rows.push(ends(Node::spacer(0.0, 0.0), buttons, 2.0 * TRAIL + 8.0));
    rows
}

/// The panel's tree.
pub fn view(p: &Panel, i18n: &I18n, text_scale: f32, s: &TipStyle) -> Node {
    let size = s.text * text_scale;
    let sub = size * SUB_SCALE;
    let mut rows = Vec::new();
    if let Some(on) = p.wifi {
        rows.push(
            ends(
                words(i18n.tr("net-wifi"), Role::Fg, 600, size, WIDTH - SWITCH.0),
                switch(on, HIT, WIFI_SWITCH),
                SWITCH.0,
            )
            .size(None, Some(ROW * 0.75)),
        );
        let note = |key: &str| words(i18n.tr(key), Role::Dim, 400, sub, WIDTH);
        if !on {
            rows.push(note("net-wifi-off"));
        } else if p.denied {
            rows.push(
                Node::column(vec![
                    words(i18n.tr("net-location"), Role::Dim, 400, sub, WIDTH),
                    link("gear", i18n.tr("net-location-link"), sub, LOCATION),
                ])
                .gap(6.0)
                .align(Align::Start),
            );
        } else if p.networks.is_empty() {
            rows.push(note(if p.scanning { "net-scanning" } else { "net-no-networks" }));
        }
        for (i, n) in p.networks.iter().take(p.shown()).enumerate() {
            let value = FIRST_NETWORK + u32::try_from(i).unwrap_or(0);
            let title = words(n.ssid.clone(), Role::Fg, 600, size, WIDTH - ICON - ICON_GAP - TRAIL);
            let second = words(
                status_of(p, n, i18n),
                Role::Dim,
                400,
                sub,
                WIDTH - ICON - ICON_GAP - TRAIL,
            );
            let trail = if n.connected {
                button(i18n.tr("net-disconnect"), false, sub, DISCONNECT)
            } else if n.secured && !n.known {
                Node::icon("lock", MARK, Role::Dim)
            } else {
                Node::icon("right", MARK, Role::Dim)
            };
            let left = Node::row(vec![
                Node::icon(
                    wifi_icon(n.signal),
                    ICON,
                    if n.connected { Role::Accent } else { Role::Fg },
                ),
                Node::column(vec![title, second]).align(Align::Start).gap(1.0),
            ])
            .gap(ICON_GAP)
            .align(Align::Center);
            let mut row = ends(left, trail, TRAIL).size(None, Some(ROW)).hit(HIT, value);
            // The keyboard's selection is drawn over it by the host (`popup_keys::marked`).
            if p.asking.as_deref() == Some(n.ssid.as_str()) {
                row = row.paint(Paint {
                    fill: Role::SurfaceSel,
                    border: None,
                    radius: 8.0,
                });
            }
            rows.push(row);
            if p.asking.as_deref() == Some(n.ssid.as_str()) {
                rows.extend(key_rows(p, i18n, size));
            }
        }
    }
    let ethernet = format!(
        "{} · {}",
        i18n.tr("net-ethernet"),
        i18n.tr(if p.ethernet { "net-connected" } else { "net-none" })
    );
    let foot_left = Node::row(vec![
        Node::icon("eth", MARK, Role::Dim),
        words(ethernet, Role::Dim, 400, sub, WIDTH / 2.0),
    ])
    .gap(6.0)
    .align(Align::Center);
    rows.push(Node::spacer(WIDTH, 1.0).paint(Paint {
        fill: Role::SurfaceLine,
        border: None,
        radius: 0.0,
    }));
    rows.push(
        ends(
            foot_left,
            link("gear", i18n.tr("net-settings"), sub, SETTINGS),
            WIDTH / 2.0,
        )
        .size(None, Some(ROW * 0.7)),
    );
    Node::column(rows)
        .align(Align::Start)
        .gap(4.0)
        .padding(Insets::all(s.pad_h + s.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: a known, an open and a new protected network chosen; a key too short,
    //! then long enough; Esc leaving the field, then the panel; the popups' keys over every
    //! part (from nothing, at the ends, round with Tab), Enter or Space on a network, the
    //! switch and a link; while a key is asked, a space typed into it, Enter joining, or
    //! clicking the button selected, the arrows still moving; a selected network gone;
    //! Wi-Fi off, no Wi-Fi hardware, names refused, nothing found; every string in both
    //! languages; every part clickable.
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

    fn net(ssid: &str, secured: bool, known: bool, connected: bool) -> Network {
        Network {
            ssid: ssid.into(),
            signal: 70,
            secured,
            wpa3: false,
            known,
            connected,
        }
    }

    fn panel() -> Panel {
        Panel {
            wifi: Some(true),
            networks: vec![
                net("home", true, true, true),
                net("office", true, true, false),
                net("cafe", false, false, false),
                net("new", true, false, false),
            ],
            internet: true,
            ..Panel::default()
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

    fn hits(n: &Node) -> Vec<u32> {
        layout(n, Point::default(), &Fake)
            .hits
            .iter()
            .filter(|(_, h)| h.module == HIT)
            .map(|(_, h)| h.value)
            .collect()
    }

    #[test]
    fn a_network_is_joined_or_its_key_asked() {
        let mut p = panel();
        assert_eq!(p.click(FIRST_NETWORK), Action::None, "the connected one stays");
        assert_eq!(
            p.click(FIRST_NETWORK + 1),
            Action::Join(net("office", true, true, false), None)
        );
        assert_eq!(
            p.click(FIRST_NETWORK + 2),
            Action::Join(net("cafe", false, false, false), None)
        );
        assert_eq!(p.click(FIRST_NETWORK + 3), Action::Redraw);
        assert_eq!(p.asking.as_deref(), Some("new"));
        // Too short: said, nothing joined.
        let e = entries(&p);
        assert_eq!(p.key(Key::Text("1234".into()), &e), Action::Redraw);
        assert_eq!(p.key(Key::Popup(popup_keys::Key::Activate), &e), Action::Redraw);
        assert!(p.short_key);
        assert_eq!(p.key(Key::Text("5678".into()), &e), Action::Redraw);
        assert!(!p.short_key);
        assert_eq!(
            p.click(CONNECT),
            Action::Join(net("new", true, false, false), Some("12345678".into()))
        );
        assert_eq!((p.asking.as_deref(), p.key.as_str()), (None, ""), "the key is not kept");
    }

    /// The panel's parts as drawn, in order.
    fn entries(p: &Panel) -> Vec<u32> {
        let l = layout(&view(p, &I18n::new(Lang::En), 1.0, &TIP), Point::default(), &Fake);
        popup_keys::entries(&l, HIT)
    }

    fn press(p: &mut Panel, k: popup_keys::Key) -> Action {
        let e = entries(p);
        p.key(Key::Popup(k), &e)
    }

    #[test]
    fn escape_leaves_the_field_then_the_panel() {
        let mut p = panel();
        let e = entries(&p);
        p.click(FIRST_NETWORK + 3);
        p.key(Key::Text("abc".into()), &e);
        assert_eq!(p.key(Key::Backspace, &e), Action::Redraw);
        assert_eq!(p.key, "ab");
        assert_eq!(p.key(Key::Escape, &e), Action::Redraw);
        assert_eq!(p.asking, None);
        assert_eq!(p.key(Key::Escape, &e), Action::Close);
    }

    /// PLAN M19 U10: the panel kept its own keys (Up and Down over the networks only).
    /// It takes every popup's: the selection goes over each part, Tab round them, and
    /// Enter or Space clicks the one selected.
    #[test]
    fn the_popups_keys_go_over_every_part() {
        use popup_keys::Key as K;
        let mut p = panel();
        let e = entries(&p);
        assert_eq!(e.first(), Some(&WIFI_SWITCH));
        assert_eq!(e.last(), Some(&SETTINGS));
        assert_eq!(press(&mut p, K::Down), Action::Redraw);
        assert_eq!(p.selected, Some(WIFI_SWITCH), "from nothing, the first");
        assert_eq!(
            press(&mut p, K::Activate),
            Action::Wifi(false),
            "Enter or Space clicks it"
        );
        assert_eq!(press(&mut p, K::Down), Action::Redraw);
        assert_eq!(p.selected, Some(FIRST_NETWORK), "the connected network");
        assert_eq!(press(&mut p, K::Down), Action::Redraw);
        assert_eq!(p.selected, Some(DISCONNECT), "its button");
        assert_eq!(press(&mut p, K::Activate), Action::Disconnect);
        assert_eq!(press(&mut p, K::End), Action::Redraw);
        assert_eq!(p.selected, Some(SETTINGS));
        assert_eq!(press(&mut p, K::Down), Action::None, "it stops at the last");
        assert_eq!(press(&mut p, K::Activate), Action::Open(SETTINGS_PAGE));
        assert_eq!(press(&mut p, K::Next), Action::Redraw);
        assert_eq!(p.selected, Some(WIFI_SWITCH), "Tab goes round");
        assert_eq!(press(&mut p, K::Previous), Action::Redraw);
        assert_eq!(p.selected, Some(SETTINGS));
        // A network chosen: a new protected one asks its key.
        p.selected = Some(FIRST_NETWORK + 3);
        assert_eq!(press(&mut p, K::Activate), Action::Redraw);
        assert_eq!(p.asking.as_deref(), Some("new"));
        assert_eq!(press(&mut p, K::Home), Action::Redraw);
        assert_eq!(p.selected, Some(WIFI_SWITCH));
        // Nothing selected: Enter does nothing; the slider keys never do.
        p = panel();
        assert_eq!(press(&mut p, K::Activate), Action::None);
        assert_eq!(press(&mut p, K::More), Action::None);
    }

    /// What the network panel did that the other popups do not stays: typing a key
    /// (spaces too, as WPA keys may hold them), Enter joining, Esc leaving the field.
    #[test]
    fn while_a_key_is_asked_typing_goes_to_it() {
        use popup_keys::Key as K;
        let mut p = panel();
        p.click(FIRST_NETWORK + 3);
        let e = entries(&p);
        assert!(e.contains(&KEY_FIELD) && e.contains(&CONNECT) && e.contains(&CANCEL));
        p.key(Key::Text("my ".into()), &e);
        p.key(Key::Text("key 1".into()), &e);
        assert_eq!(p.key, "my key 1");
        // Enter on the network's row, the field or nothing joins.
        assert_eq!(
            p.key(Key::Popup(K::Activate), &e),
            Action::Join(net("new", true, false, false), Some("my key 1".into()))
        );
        // The arrows still move; Enter on Cancel cancels.
        p.click(FIRST_NETWORK + 3);
        let e = entries(&p);
        p.key(Key::Text("abc".into()), &e);
        p.selected = Some(CONNECT);
        assert_eq!(p.key(Key::Popup(K::Up), &e), Action::Redraw);
        assert_eq!(p.selected, Some(CANCEL));
        assert_eq!(p.key(Key::Popup(K::Activate), &e), Action::Redraw);
        assert_eq!((p.asking.as_deref(), p.key.as_str()), (None, ""));
        // Enter on Connect with a short key says so.
        p.click(FIRST_NETWORK + 3);
        let e = entries(&p);
        p.key(Key::Text("abc".into()), &e);
        p.selected = Some(CONNECT);
        assert_eq!(p.key(Key::Popup(K::Activate), &e), Action::Redraw);
        assert!(p.short_key);
    }

    #[test]
    fn a_selected_network_gone_is_let_go() {
        let mut p = panel();
        p.selected = Some(FIRST_NETWORK + 3);
        p.keep_selection();
        assert_eq!(p.selected, Some(FIRST_NETWORK + 3));
        p.networks.truncate(2);
        p.keep_selection();
        assert_eq!(p.selected, None);
        p.selected = Some(SETTINGS);
        p.wifi = Some(false);
        p.keep_selection();
        assert_eq!(p.selected, Some(SETTINGS), "a part that is no network stays");
    }

    #[test]
    fn the_switch_and_the_links_say_what_to_do() {
        let mut p = panel();
        assert_eq!(p.click(WIFI_SWITCH), Action::Wifi(false));
        p.wifi = Some(false);
        assert_eq!(p.click(WIFI_SWITCH), Action::Wifi(true));
        p.wifi = None;
        assert_eq!(p.click(WIFI_SWITCH), Action::None);
        assert_eq!(p.click(SETTINGS), Action::Open(SETTINGS_PAGE));
        assert_eq!(p.click(LOCATION), Action::Open(LOCATION_PAGE));
        assert_eq!(p.click(DISCONNECT), Action::Disconnect);
    }

    #[test]
    fn the_panel_draws_the_networks_and_their_states() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let mut p = panel();
            p.joining = Some("office".into());
            p.failed = Some("cafe".into());
            let t = texts(&view(&p, &i18n, 1.0, &TIP));
            for (name, key) in [
                ("home", "net-connected"),
                ("office", "net-connecting"),
                ("cafe", "net-failed"),
                ("new", "net-secured"),
            ] {
                let i = t.iter().position(|x| x == name).expect(name);
                assert_eq!(t[i + 1], i18n.tr(key), "{name}");
            }
            assert!(t.contains(&i18n.tr("net-disconnect")));
            assert!(t.contains(&i18n.tr("net-settings")));
            let hits = hits(&view(&p, &i18n, 1.0, &TIP));
            for v in [WIFI_SWITCH, DISCONNECT, SETTINGS, FIRST_NETWORK, FIRST_NETWORK + 3] {
                assert!(hits.contains(&v), "{v} not clickable");
            }
            // Asked: the field (bullets, never the key), the buttons.
            p.click(FIRST_NETWORK + 3);
            p.key(Key::Text("secret".into()), &[]);
            let t = texts(&view(&p, &i18n, 1.0, &TIP));
            assert!(t.contains(&"••••••".to_string()) && !t.iter().any(|x| x.contains("secret")));
            assert!(t.contains(&i18n.tr("net-connect")) && t.contains(&i18n.tr("net-cancel")));
            p.key(Key::Popup(popup_keys::Key::Activate), &[]);
            assert!(texts(&view(&p, &i18n, 1.0, &TIP)).contains(&i18n.tr("net-key-short")));
        }
    }

    #[test]
    fn without_networks_the_panel_says_why() {
        let i18n = I18n::new(Lang::En);
        let say = |p: &Panel| texts(&view(p, &i18n, 1.0, &TIP));
        let mut p = Panel {
            wifi: Some(false),
            ..panel()
        };
        assert!(say(&p).contains(&i18n.tr("net-wifi-off")));
        assert!(!say(&p).contains(&"home".to_string()));
        p = Panel {
            denied: true,
            ..panel()
        };
        assert!(say(&p).contains(&i18n.tr("net-location")) && say(&p).contains(&i18n.tr("net-location-link")));
        p = Panel {
            networks: Vec::new(),
            scanning: true,
            ..panel()
        };
        assert!(say(&p).contains(&i18n.tr("net-scanning")));
        p.scanning = false;
        assert!(say(&p).contains(&i18n.tr("net-no-networks")));
        // No Wi-Fi hardware: no Wi-Fi line or switch, the Ethernet line and the link stay.
        p = Panel {
            wifi: None,
            ethernet: true,
            ..panel()
        };
        let t = say(&p);
        assert!(!t.contains(&i18n.tr("net-wifi")) && t.iter().any(|x| x.starts_with(&i18n.tr("net-ethernet"))));
        assert!(!hits(&view(&p, &i18n, 1.0, &TIP)).contains(&WIFI_SWITCH));
    }
}
