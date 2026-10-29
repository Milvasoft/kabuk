//! The volume panel and the system icons' menus (reported by the user: a click on the
//! volume icon showed nothing, and every icon should do what Windows' taskbar does): a
//! left click on the bar's volume opens the output's level as the look's OSD track,
//! which a click or a drag sets, the speaker that mutes, the output devices to choose
//! from (as Windows 11's sound flyout; DECISIONS D54), the apps playing with a level of
//! their own (Windows' volume mixer; D69), and Windows' volume mixer and sound settings;
//! a right click on the volume, the clock, the bell or a system figure opens that icon's
//! entries, as Windows' taskbar does. Every level has a round thumb to drag (D69). In
//! the tray panel's surface (D21, D26). Pure.

use kabuk_i18n::I18n;
use kabuk_ui::geom::{Insets, Rect};
use kabuk_ui::node::{Align, Axis, FontRole, Node, Paint, Role, Span, TextSpec};
use kabuk_win::audio::Session;

use super::tooltip::TipStyle;
use crate::notify::style::{NotifyStyle, Track};

/// Hit-test id of the panel's parts; the value is one of the constants below.
pub const HIT: &str = "volume-panel";
pub const MUTE: u32 = 0;
pub const TRACK: u32 = 1;
/// The entries' values start here, in [`ENTRIES`]' order.
pub const FIRST_ENTRY: u32 = 2;
/// The output devices' values start here, in the order given to [`view`].
pub const FIRST_OUTPUT: u32 = 100;
/// The mixer's apps: their levels' values start here, their icons' (which mute) there.
pub const FIRST_APP: u32 = 200;
pub const FIRST_APP_MUTE: u32 = 300;
/// The most apps the mixer lists (the ones playing first).
pub const MAX_APPS: usize = 8;

/// A level's thumb (DIPs): its diameter, the accent dot in it, and its ring's width, as
/// Windows 11's sliders.
const THUMB: f32 = 14.0;
const THUMB_DOT: f32 = 6.0;
const THUMB_RING: f32 = 1.0;
/// The space between an app's name and its level (DIPs).
const NAME_GAP: f32 = 4.0;
/// An app's name is drawn this much smaller than an entry's words.
const NAME_SCALE: f32 = 0.85;
/// A muted app's icon carries the speaker this much of its size, at its corner.
const MUTED_BADGE: f32 = 0.6;

/// A row of the volume mixer: one program, all its sessions (a browser has several).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixerRow {
    pub exe: String,
    pub pids: Vec<u32>,
    /// The name its first session gave itself (often empty).
    pub session_name: String,
    pub percent: u32,
    pub muted: bool,
    pub active: bool,
}

/// The mixer's rows from the sessions: one per program, the loudest of its sessions'
/// levels, muted when all of them are; the ones playing first, then in Windows' order;
/// at most [`MAX_APPS`]. Sessions without a known program are left out.
pub fn mixer_rows(sessions: &[Session]) -> Vec<MixerRow> {
    let mut rows: Vec<MixerRow> = Vec::new();
    for s in sessions {
        let Some(exe) = &s.exe else { continue };
        match rows.iter_mut().find(|r| r.exe.eq_ignore_ascii_case(exe)) {
            Some(r) => {
                r.pids.push(s.pid);
                r.percent = r.percent.max(s.percent);
                r.muted &= s.muted;
                r.active |= s.active;
            }
            None => rows.push(MixerRow {
                exe: exe.clone(),
                pids: vec![s.pid],
                session_name: s.name.clone(),
                percent: s.percent,
                muted: s.muted,
                active: s.active,
            }),
        }
    }
    rows.sort_by_key(|r| !r.active);
    rows.truncate(MAX_APPS);
    rows
}

/// A mixer row as drawn: the app's name, its icon's picture (`None`: the generic icon)
/// and its level.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerView {
    pub name: String,
    pub image: Option<u64>,
    pub percent: u32,
    pub muted: bool,
}

/// A level to drag: the track `level` (0-1) filled and a round thumb at its end.
pub fn slider(t: &Track, level: f32) -> Node {
    let level = level.clamp(0.0, 1.0);
    let ring = Node::spacer(THUMB, THUMB).paint(Paint {
        fill: Role::Surface,
        border: Some((Role::SurfaceLine, THUMB_RING)),
        radius: THUMB / 2.0,
    });
    let dot = Node::spacer(THUMB_DOT, THUMB_DOT).paint(Paint {
        fill: Role::AccentFill,
        border: None,
        radius: THUMB_DOT / 2.0,
    });
    // A layer centres its children: the thumb moves from the middle to the level.
    let thumb = Node::layer(vec![ring, dot])
        .size(Some(THUMB), Some(THUMB))
        .offset(t.width * level - t.width / 2.0, 0.0);
    Node::layer(vec![crate::notify::view::track(t, level), thumb]).size(Some(t.width), Some(THUMB.max(t.height)))
}

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
    Battery,
    /// An empty part of the bar: what Windows' taskbar menu holds (asked for by the user
    /// on 2026-10-29).
    Bar,
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
/// Opens Kabuk's settings: the bar's own, as the taskbar's menu opens the taskbar's.
pub const KABUK_SETTINGS: &str = "kabuk:settings";
/// Windows 11's taskbar menu: Task Manager, then the taskbar's settings (Kabuk's here).
const BAR: [Entry; 2] = [
    ("cpu", "stats-task-manager", "taskmgr.exe"),
    ("app", "launcher-settings", KABUK_SETTINGS),
];
const BATTERY: [Entry; 1] = [("batt", "batt-settings", "ms-settings:batterysaver")];
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
            Menu::Battery => &BATTERY,
            Menu::Bar => &BAR,
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
                italic: false,
                spacing: 0.0,
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
    // As wide as the panel: a click anywhere on it counts, the keyboard's fill spans it.
    .fill(Axis::Row, 0.0)
}

/// The widest an output's name may be before it is cut (DIPs).
const MAX_TEXT: f32 = 300.0;

/// Words in the panel's font.
fn words(text: String, color: Role, size: f32, max_width: Option<f32>) -> Node {
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
        max_width,
    })
}

/// A level's figure, 0 when muted, in the OSD's width.
fn value_text(shown: u32, notify: &NotifyStyle, text_scale: f32) -> Node {
    Node::row(vec![Node::text(TextSpec {
        spans: vec![Span {
            text: shown.to_string(),
            color: Role::Fg,
            weight: 400,
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Num,
        size: notify.value_size * text_scale,
        max_width: None,
    })])
    .size(Some(notify.value_width), None)
}

/// The mixer: its title, then each app's icon (a click mutes it), name and level.
fn mixer(i18n: &I18n, text_scale: f32, s: &TipStyle, notify: &NotifyStyle, apps: &[MixerView]) -> Vec<Node> {
    let mut rows = vec![Node::row(vec![words(
        i18n.tr("vol-apps"),
        Role::Dim,
        s.text * text_scale,
        Some(MAX_TEXT),
    )])
    .padding(Insets::symmetric(s.pad_h, 0.0))
    .size(None, Some(ROW))
    .align(Align::Center)];
    for (i, app) in apps.iter().enumerate() {
        let i = u32::try_from(i).unwrap_or(0);
        let shown = if app.muted { 0 } else { app.percent.min(100) };
        let picture = match app.image {
            Some(key) => Node::image(key, notify.icon_size),
            None => Node::icon("app", notify.icon_size, Role::Dim),
        };
        let icon = if app.muted {
            // The speaker on a disk of the panel's colour, at the icon's lower left corner
            // (the level's thumb is to its right).
            let badge = notify.icon_size * MUTED_BADGE;
            let corner = notify.icon_size / 2.0;
            let disk = Node::spacer(badge + 2.0 * THUMB_RING, badge + 2.0 * THUMB_RING).paint(Paint {
                fill: Role::Surface,
                border: Some((Role::SurfaceLine, THUMB_RING)),
                radius: badge / 2.0 + THUMB_RING,
            });
            Node::layer(vec![
                picture,
                disk.offset(-corner, corner),
                Node::icon("vol", badge, Role::Dim).offset(-corner, corner),
            ])
            .size(Some(notify.icon_size), Some(notify.icon_size))
        } else {
            picture
        };
        let color = if app.muted { Role::Dim } else { Role::Fg };
        let name = words(
            app.name.clone(),
            color,
            s.text * NAME_SCALE * text_scale,
            Some(notify.track.width),
        );
        // The name over the level and its figure, which line up with the output's.
        let level = Node::row(vec![
            slider(&notify.track, shown as f32 / 100.0).hit(HIT, FIRST_APP + i),
            value_text(shown, notify, text_scale),
        ])
        .gap(notify.osd_gap)
        .align(Align::Center);
        rows.push(
            Node::row(vec![
                icon.hit(HIT, FIRST_APP_MUTE + i),
                Node::column(vec![name, level]).gap(NAME_GAP).align(Align::Start),
            ])
            .gap(notify.osd_gap)
            .align(Align::Center)
            .padding(Insets::symmetric(s.pad_h, s.pad_v / 2.0)),
        );
    }
    rows
}

/// The panel's tree; `level` is `(percent, muted)` for the track (`None`: no track, the
/// right click's menu); `outputs` the output devices' names, the one in use `true`,
/// listed under the track; `apps` the mixer's rows, under the outputs.
#[allow(clippy::too_many_arguments)]
pub fn view(
    i18n: &I18n,
    text_scale: f32,
    s: &TipStyle,
    notify: &NotifyStyle,
    level: Option<(u32, bool)>,
    outputs: &[(String, bool)],
    apps: &[MixerView],
    entries: &[Entry],
) -> Node {
    let mut rows = Vec::new();
    if let Some((percent, muted)) = level {
        let shown = if muted { 0 } else { percent.min(100) };
        rows.push(
            Node::row(vec![
                Node::icon("vol", notify.icon_size, if muted { Role::Dim } else { Role::Fg }).hit(HIT, MUTE),
                slider(&notify.track, shown as f32 / 100.0).hit(HIT, TRACK),
                value_text(shown, notify, text_scale),
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
    if level.is_some() && !apps.is_empty() {
        rows.extend(mixer(i18n, text_scale, s, notify, apps));
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
                Menu::Bar,
            ] {
                for (_, key, _) in menu.entries() {
                    let w = i18n.tr(key);
                    assert!(!w.is_empty() && w != *key, "{key} {lang:?}");
                }
                let m = view(&i18n, 1.0, &TIP, &notify, None, &[], &[], menu.entries());
                let n = u32::try_from(menu.entries().len()).unwrap();
                assert_eq!(hits(&m), (FIRST_ENTRY..FIRST_ENTRY + n).collect::<Vec<_>>(), "{menu:?}");
            }
            let panel = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &[], &[], &ENTRIES);
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
            &[],
            &ENTRIES,
        );
        assert!(texts(&muted).contains(&"0".to_string()));
        assert_eq!(I18n::new(Lang::En).tr("vol-mixer"), "Open volume mixer");
    }

    fn session(pid: u32, exe: Option<&str>, percent: u32, muted: bool, active: bool) -> Session {
        Session {
            pid,
            exe: exe.map(Into::into),
            name: String::new(),
            percent,
            muted,
            active,
        }
    }

    /// Asked for by the user (Windows' volume mixer): one row per program, a browser's
    /// several sessions together at the loudest of their levels and muted only when all
    /// are; the ones playing first; sessions of no known program left out; a cap.
    #[test]
    fn the_mixer_has_a_row_per_program() {
        const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        const SPOTIFY: &str = r"C:\Users\u\AppData\Roaming\Spotify\Spotify.exe";
        let sessions = vec![
            session(10, Some(SPOTIFY), 80, false, false),
            session(20, Some(CHROME), 40, true, false),
            session(21, Some(&CHROME.to_uppercase()), 60, false, true),
            session(30, None, 50, false, true),
        ];
        let rows = mixer_rows(&sessions);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].exe, CHROME, "the playing one first");
        assert_eq!(
            (rows[0].pids.clone(), rows[0].percent, rows[0].muted, rows[0].active),
            (vec![20, 21], 60, false, true)
        );
        assert_eq!(
            (rows[1].exe.as_str(), rows[1].percent, rows[1].active),
            (SPOTIFY, 80, false)
        );
        let all_muted = mixer_rows(&[
            session(1, Some(CHROME), 40, true, true),
            session(2, Some(CHROME), 30, true, false),
        ]);
        assert!(all_muted[0].muted);
        let many: Vec<Session> = (0..20)
            .map(|i| session(i, Some(&format!("app{i}.exe")), 50, false, false))
            .collect();
        assert_eq!(mixer_rows(&many).len(), MAX_APPS);
        assert!(mixer_rows(&[]).is_empty());
    }

    /// The mixer comes under the outputs with its title, each app's icon (which mutes)
    /// and level clickable, its name and figure shown; a muted app shows 0. Only the left
    /// click's panel has it.
    #[test]
    fn the_mixer_lists_the_apps_under_the_outputs() {
        let notify = NotifyStyle::of(Look::Ada);
        let outputs = vec![("Speakers".to_string(), true), ("Headphones".to_string(), false)];
        let apps = vec![
            MixerView {
                name: "Google Chrome".into(),
                image: Some(7),
                percent: 60,
                muted: false,
            },
            MixerView {
                name: "Spotify".into(),
                image: None,
                percent: 80,
                muted: true,
            },
        ];
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let title = i18n.tr("vol-apps");
            assert!(!title.is_empty() && title != "vol-apps", "{lang:?}");
            let panel = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &outputs, &apps, &ENTRIES);
            assert_eq!(
                hits(&panel),
                vec![
                    MUTE,
                    TRACK,
                    FIRST_OUTPUT,
                    FIRST_OUTPUT + 1,
                    FIRST_APP_MUTE,
                    FIRST_APP,
                    FIRST_APP_MUTE + 1,
                    FIRST_APP + 1,
                    2,
                    3,
                    4
                ]
            );
            let t = texts(&panel);
            let at = |x: &str| t.iter().position(|y| y == x).unwrap_or_else(|| panic!("{x} in {t:?}"));
            assert!(
                at("Headphones") < at(&title)
                    && at(&title) < at("Google Chrome")
                    && at("Google Chrome") < at("Spotify")
            );
            assert!(
                t.contains(&"60".to_string()) && !t.contains(&"80".to_string()),
                "a muted app shows 0: {t:?}"
            );
            let menu = view(&i18n, 1.0, &TIP, &notify, None, &outputs, &apps, &ENTRIES);
            assert_eq!(hits(&menu), vec![2, 3, 4]);
        }
        assert_eq!(I18n::new(Lang::En).tr("vol-apps"), "Volume mixer");
    }

    /// Asked for by the user: every level has a round thumb to hold and drag, at the
    /// level's end; the track's clickable region stays the track's width.
    #[test]
    fn a_level_has_a_round_thumb_at_its_end() {
        let notify = NotifyStyle::of(Look::Ada);
        let t = notify.track;
        for (level, x) in [(0.0, 0.0), (0.4, 0.4 * t.width), (1.0, t.width)] {
            let l = layout(&slider(&t, level).hit(HIT, TRACK), Point::default(), &Fake);
            let thumbs: Vec<Rect> = l
                .draws
                .iter()
                .filter_map(|d| match d {
                    // The ring: round, the thumb's size (the accent dot sits in it).
                    Draw::Panel { rect, paint }
                        if paint.radius == rect.w / 2.0 && rect.w == THUMB && rect.h == THUMB =>
                    {
                        Some(*rect)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(thumbs.len(), 1, "one round thumb at {level}: {:?}", l.draws);
            let dot = l.draws.iter().find_map(|d| match d {
                Draw::Panel { rect, paint } if paint.fill == Role::AccentFill && rect.w == THUMB_DOT => Some(*rect),
                _ => None,
            });
            let dot = dot.expect("the accent dot in the thumb");
            assert!(
                (dot.x + dot.w / 2.0 - x).abs() < 0.01,
                "the dot is in the thumb's middle"
            );
            let centre = thumbs[0].x + thumbs[0].w / 2.0;
            assert!((centre - x).abs() < 0.01, "thumb at {centre}, level end at {x}");
            let (track, _) = l.hits[0];
            assert_eq!((track.x, track.w), (0.0, t.width));
            assert!(track.h >= thumbs[0].h, "the thumb is held where it is drawn");
        }
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
        let panel = view(&i18n, 1.0, &TIP, &notify, Some((40, false)), &outputs, &[], &ENTRIES);
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
        let one = view(
            &i18n,
            1.0,
            &TIP,
            &notify,
            Some((40, false)),
            &outputs[..1],
            &[],
            &ENTRIES,
        );
        assert_eq!(hits(&one), vec![MUTE, TRACK, 2, 3, 4]);
        let menu = view(&i18n, 1.0, &TIP, &notify, None, &outputs, &[], &ENTRIES);
        assert_eq!(hits(&menu), vec![2, 3, 4]);
    }
}
