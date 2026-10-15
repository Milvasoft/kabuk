//! The volume panel and the system icons' menus (reported by the user: a click on the
//! volume icon showed nothing, and every icon should do what Windows' taskbar does): a
//! left click on the bar's volume opens the output's level as the look's OSD track,
//! which a click or a drag sets, the speaker that mutes, the output devices to choose
//! from (as Windows 11's sound flyout; DECISIONS D54), and Windows' volume mixer and
//! sound settings; a right click on the volume, the clock, the bell or a system figure
//! opens that icon's entries, as Windows' taskbar does. In the tray panel's surface
//! (D21, D26). Pure.

use kabuk_i18n::I18n;
use kabuk_ui::geom::{Insets, Rect};
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;
use crate::notify::style::NotifyStyle;

/// Hit-test id of the panel's parts; the value is one of the constants below.
pub const HIT: &str = "volume-panel";
pub const MUTE: u32 = 0;
pub const TRACK: u32 = 1;
/// The entries' values start here, in [`ENTRIES`]' order.
pub const FIRST_ENTRY: u32 = 2;
/// The output devices' values start here, in the order given to [`view`].
pub const FIRST_OUTPUT: u32 = 100;

/// An entry: icon, words and what it opens (a Windows settings page or a program).
pub type Entry = (&'static str, &'static str, &'static str);

/// Whose entries a menu shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Volume,
    Clock,
    Notifications,
    Stats,
    Network,
}

/// The volume's entries.
pub const ENTRIES: [Entry; 3] = [
    ("vol", "vol-mixer", "ms-settings:apps-volume"),
    ("app", "vol-settings", "ms-settings:sound"),
    ("app", "vol-troubleshoot", "ms-settings:troubleshoot"),
];
const CLOCK: [Entry; 2] = [
    ("app", "clock-adjust", "ms-settings:dateandtime"),
    ("bell", "notif-settings", "ms-settings:notifications"),
];
const NOTIFICATIONS: [Entry; 2] = [
    ("bell", "notif-settings", "ms-settings:notifications"),
    ("app", "notif-focus", "ms-settings:quiethours"),
];
const STATS: [Entry; 2] = [
    ("cpu", "stats-task-manager", "taskmgr.exe"),
    ("mem", "stats-resource-monitor", "resmon.exe"),
];
const NETWORK: [Entry; 2] = [
    ("down", "net-settings", "ms-settings:network"),
    ("app", "net-troubleshoot", "ms-settings:troubleshoot"),
];

impl Menu {
    pub fn entries(self) -> &'static [Entry] {
        match self {
            Menu::Volume => &ENTRIES,
            Menu::Clock => &CLOCK,
            Menu::Notifications => &NOTIFICATIONS,
            Menu::Stats => &STATS,
            Menu::Network => &NETWORK,
        }
    }
}

/// An entry's height and the space between its icon and its words (DIPs), as the power
/// menu's.
const ROW: f32 = 30.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 10.0;

/// The level a press at `x` (DIPs) on the track at `track` sets, 0-100.
pub fn percent_at(x: f32, track: Rect) -> u32 {
    if track.w <= 0.0 {
        return 0;
    }
    (((x - track.x) / track.w).clamp(0.0, 1.0) * 100.0).round() as u32
}

/// A row of an icon and words, clickable as `value`.
fn entry_row(
    icon: &'static str,
    icon_role: Role,
    words: String,
    color: Role,
    size: f32,
    s: &TipStyle,
    value: u32,
) -> Node {
    Node::row(vec![
        Node::icon(icon, ICON, icon_role),
        Node::text(TextSpec {
            spans: vec![Span {
                text: words,
                color,
                weight: 400,
                after: 0.0,
                underline: false,
            }],
            font: FontRole::Ui,
            size,
            max_width: Some(MAX_TEXT),
        }),
    ])
    .gap(ICON_GAP)
    .align(Align::Center)
    .padding(Insets::symmetric(s.pad_h, 0.0))
    .size(None, Some(ROW))
    .hit(HIT, value)
}

/// The widest an output's name may be before it is cut (DIPs).
const MAX_TEXT: f32 = 300.0;

/// The panel's tree; `level` is `(percent, muted)` for the track (`None`: no track, the
/// right click's menu); `outputs` the output devices' names, the one in use `true`,
/// listed under the track.
pub fn view(
    i18n: &I18n,
    text_scale: f32,
    s: &TipStyle,
    notify: &NotifyStyle,
    level: Option<(u32, bool)>,
    outputs: &[(String, bool)],
    entries: &[Entry],
) -> Node {
    let mut rows = Vec::new();
    if let Some((percent, muted)) = level {
        let shown = if muted { 0 } else { percent.min(100) };
        let value = Node::row(vec![Node::text(TextSpec {
            spans: vec![Span {
                text: shown.to_string(),
                color: Role::Fg,
                weight: 400,
                after: 0.0,
                underline: false,
            }],
            font: FontRole::Num,
            size: notify.value_size * text_scale,
            max_width: None,
        })])
        .size(Some(notify.value_width), None);
        rows.push(
            Node::row(vec![
                Node::icon("vol", notify.icon_size, if muted { Role::Dim } else { Role::Fg }).hit(HIT, MUTE),
                crate::notify::view::track(&notify.track, shown as f32 / 100.0).hit(HIT, TRACK),
                value,
            ])
            .gap(notify.osd_gap)
            .align(Align::Center)
            .padding(Insets::symmetric(s.pad_h, s.pad_v)),
        );
    }
    if level.is_some() && outputs.len() > 1 {
        for (i, (name, current)) in outputs.iter().enumerate() {
            let (icon, role, color) = if *current {
                ("check", Role::Accent, Role::Fg)
            } else {
                ("vol", Role::Dim, Role::Dim)
            };
            let value = FIRST_OUTPUT + u32::try_from(i).unwrap_or(0);
            rows.push(entry_row(
                icon,
                role,
                name.clone(),
                color,
                s.text * text_scale,
                s,
                value,
            ));
        }
    }
    for (i, (icon, key, _)) in entries.iter().enumerate() {
        let value = FIRST_ENTRY + u32::try_from(i).unwrap_or(0);
        rows.push(entry_row(
            icon,
            Role::Dim,
            i18n.tr(key),
            Role::Fg,
            s.text * text_scale,
            s,
            value,
        ));
    }
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
    //! Edge cases: a press at each end of the track, past them and in the middle; the
    //! left click's panel with the track, the speaker and every entry clickable, each
    //! icon's menu with its entries only; a muted level shows 0; every entry worded in
    //! both languages.
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

    const TRACK_AT: Rect = Rect {
        x: 40.0,
        y: 10.0,
        w: 200.0,
        h: 4.0,
    };

    #[test]
    fn a_press_on_the_track_sets_its_level() {
        assert_eq!(percent_at(40.0, TRACK_AT), 0);
        assert_eq!(percent_at(240.0, TRACK_AT), 100);
        assert_eq!(percent_at(140.0, TRACK_AT), 50);
        assert_eq!(percent_at(-5.0, TRACK_AT), 0);
        assert_eq!(percent_at(900.0, TRACK_AT), 100);
        assert_eq!(percent_at(10.0, Rect { w: 0.0, ..TRACK_AT }), 0);
    }

    fn hits(n: &Node) -> Vec<u32> {
        layout(n, Point::default(), &Fake)
            .hits
            .iter()
            .filter(|(_, h)| h.module == HIT)
            .map(|(_, h)| h.value)
            .collect()
    }

    fn texts(n: &Node) -> Vec<String> {
        layout(n, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.spans.iter().map(|s| s.text.clone()).collect()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_panel_has_the_track_and_the_menu_has_the_entries() {
        let notify = NotifyStyle::of(Look::Ada);
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for menu in [
                Menu::Volume,
                Menu::Clock,
                Menu::Notifications,
                Menu::Stats,
                Menu::Network,
            ] {
                for (_, key, _) in menu.entries() {
                    let w = i18n.tr(key);
                    assert!(!w.is_empty() && w != *key, "{key} {lang:?}");
                }
                let m = view(&i18n, 1.0, &TIP, &notify, None, &[], menu.entries());
                let n = u32::try_from(menu.entries().len()).unwrap();
                assert_eq!(hits(&m), (FIRST_ENTRY..FIRST_ENTRY + n).collect::<Vec<_>>(), "{menu:?}");
            }
            let panel = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &[], &ENTRIES);
            assert_eq!(hits(&panel), vec![MUTE, TRACK, 2, 3, 4]);
            assert!(texts(&panel).contains(&"40".to_string()));
        }
        let muted = view(
            &I18n::new(Lang::En),
            1.0,
            &TIP,
            &notify,
            Some((40, true)),
            &[],
            &ENTRIES,
        );
        assert!(texts(&muted).contains(&"0".to_string()));
        assert_eq!(I18n::new(Lang::En).tr("vol-mixer"), "Open volume mixer");
    }

    /// The output devices come under the track, the one in use marked, each clickable;
    /// one device alone is not offered (nothing to choose), nor in a right click's menu.
    #[test]
    fn the_outputs_are_listed_under_the_track() {
        let notify = NotifyStyle::of(Look::Ada);
        let i18n = I18n::new(Lang::En);
        let outputs = vec![
            ("Speakers".to_string(), false),
            ("Headphones (Bluetooth)".to_string(), true),
        ];
        let panel = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &outputs, &ENTRIES);
        assert_eq!(hits(&panel), vec![MUTE, TRACK, FIRST_OUTPUT, FIRST_OUTPUT + 1, 2, 3, 4]);
        let t = texts(&panel);
        let (speakers, headphones) = (
            t.iter().position(|x| x == "Speakers").unwrap(),
            t.iter().position(|x| x == "Headphones (Bluetooth)").unwrap(),
        );
        assert!(speakers < headphones && headphones < t.iter().position(|x| x == "Open volume mixer").unwrap());
        let icons: Vec<&str> = layout(&panel, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Icon { spec, .. } => Some(spec.name),
                _ => None,
            })
            .collect();
        assert_eq!(icons.iter().filter(|n| **n == "check").count(), 1, "{icons:?}");
        let one = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &outputs[..1], &ENTRIES);
        assert_eq!(hits(&one), vec![MUTE, TRACK, 2, 3, 4]);
        let menu = view(&i18n, 1.0, &TIP, &notify, None, &outputs, &ENTRIES);
        assert_eq!(hits(&menu), vec![2, 3, 4]);
    }
}
