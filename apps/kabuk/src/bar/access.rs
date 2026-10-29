//! What screen readers are told of the bar, the launcher and the bar's popups (DECISIONS
//! D97): each clickable part of a drawing, named in the user's language, with its role
//! and its place in the window. Pure: the bar thread hands the parts to its windows
//! ([`kabuk_win::msgloop::Window::set_accessible`]) after drawing them.

use kabuk_i18n::I18n;
use kabuk_ui::access::Part as Drawn;
use kabuk_ui::geom::{Point, Rect, Scale};
use kabuk_ui::node::Layout;
use kabuk_win::msgloop::PxRect;
use kabuk_win::uia::{Part, Range, Role};

use super::popup_keys::FULL;
use super::view::hit;

/// What the bar knows of a module besides the words it draws.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Said {
    /// The module's tooltip, when it has one.
    pub tip: Option<String>,
    /// A value the bar has that is drawn as a picture (the volume's level, an app's name,
    /// a tray icon's tip, the layout the workspace is in).
    pub own: Option<String>,
}

/// The module's name in Settings' list, said before its value (`value`: which figure).
fn label(module: &str, value: u32) -> Option<&'static str> {
    Some(match (module, value) {
        (hit::STATS, hit::STAT_CPU) => "mod-cpu",
        (hit::STATS, hit::STAT_RAM) => "mod-ram",
        (hit::STATS, hit::STAT_GPU) => "mod-gpu",
        (hit::STATS, hit::STAT_NET) => "mod-net",
        (module, _) => return label_of(module),
    })
}

fn label_of(module: &str) -> Option<&'static str> {
    Some(match module {
        hit::TITLE => "mod-title",
        hit::CLOCK => "mod-clock",
        hit::MEDIA => "mod-media",
        hit::VOLUME => "mod-vol",
        hit::TRAY | hit::TRAY_MORE => "mod-tray",
        hit::NOTIF => "mod-notif",
        hit::POWER => "mod-power",
        hit::APP_GRID | hit::APP_MORE => "hc-app-grid",
        hit::NETWORK => "mod-network",
        hit::BATTERY => "mod-battery",
        hit::KEYBOARD => "mod-keyboard",
        hit::PRIVACY => "mod-privacy",
        hit::SHOW_DESKTOP => "mod-show-desktop",
        hit::QUICK_SETTINGS => "mod-quick-settings",
        _ => return None,
    })
}

/// Modules whose value names them whole: a workspace's tooltip, an app's or a tray icon's
/// own name, the layout's state.
fn whole(module: &str) -> bool {
    matches!(module, hit::WORKSPACE | hit::APP | hit::TRAY | hit::LAYOUT)
}

/// What a screen reader says of bar module `module` (`value`: which one of it), drawn with
/// the words `drawn`; `None` for a part with nothing to say.
pub fn bar_name(i18n: &I18n, (module, value): (&str, u32), drawn: &str, said: Said) -> Option<String> {
    let drawn = Some(drawn.trim().to_string()).filter(|d| !d.is_empty());
    let label = label(module, value).map(|key| i18n.tr(key));
    let value = if module == hit::CLOCK {
        // The time is drawn; the tooltip only has the date.
        said.own.or(drawn).or(said.tip)
    } else {
        said.own.or(said.tip).or(drawn)
    }
    .filter(|v| !v.trim().is_empty());
    match (label, value) {
        (_, Some(v)) if whole(module) => Some(v),
        (Some(l), Some(v)) => Some(i18n.tr_args("acc-part", &[("label", l.into()), ("value", v.into())])),
        (Some(l), None) => Some(l),
        (None, v) => v,
    }
}

/// `r` (DIPs of a layout drawn at `origin`) in the window's physical pixels.
pub fn client_rect(r: Rect, origin: Point, scale: Scale) -> PxRect {
    PxRect {
        x: scale.px(r.x + origin.x),
        y: scale.px(r.y + origin.y),
        w: scale.px(r.w),
        h: scale.px(r.h),
    }
}

/// `p` (DIPs of a layout drawn at `origin`) in the window's physical pixels.
pub fn client_point(p: Point, origin: Point, scale: Scale) -> (i32, i32) {
    (scale.px(p.x + origin.x), scale.px(p.y + origin.y))
}

/// The parts of a popup drawn as `layout` at its window's corner: its regions of
/// `module`, each named by `name` (from its value and the words drawn in it; `None` or
/// an empty name leaves it out), the one valued `selected` holding the keyboard.
pub fn popup_parts(
    layout: &Layout,
    scale: Scale,
    (module, role): (&str, Role),
    selected: Option<u32>,
    name: impl Fn(u32, &str) -> Option<String>,
) -> Vec<Part> {
    popup_levels(layout, scale, (module, role), selected, name, |_| None)
}

/// [`popup_parts`] where some regions are levels (PLAN M19 U17): `level` gives the range
/// of the one valued `v`, which becomes a slider; the others (`None`) are `role`s.
pub fn popup_levels(
    layout: &Layout,
    scale: Scale,
    (module, role): (&str, Role),
    selected: Option<u32>,
    name: impl Fn(u32, &str) -> Option<String>,
    level: impl Fn(u32) -> Option<Range>,
) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for d in kabuk_ui::access::parts(layout) {
        if d.hit.module != module {
            continue;
        }
        let Some(said) = name(d.hit.value, &d.text).filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        let range = level(d.hit.value);
        out.push(Part {
            id: kabuk_win::uia::part_id(d.hit.module, d.hit.value),
            name: said,
            role: if range.is_some() { Role::Slider } else { role },
            rect: client_rect(d.rect, Point::default(), scale),
            click: client_point(d.reach, Point::default(), scale),
            focused: selected == Some(d.hit.value),
            range,
        });
    }
    out
}

/// A level as a screen reader is offered it: `percent` as drawn (0 when muted), between
/// none and full, a key moving it `step` points.
pub fn level_range(percent: u32, step: i32) -> Range {
    let full = i32::try_from(FULL).unwrap_or(i32::MAX);
    Range {
        value: i32::try_from(percent).unwrap_or(full).min(full),
        min: 0,
        max: full,
        step,
    }
}

/// The region of `module` in `layout` a screen reader's part `id` stands for: a SetValue
/// names the level it sets so ([`kabuk_win::uia::set_value_message`]).
pub fn region_of(layout: &Layout, module: &str, id: u64) -> Option<u32> {
    layout
        .hits
        .iter()
        .find(|(_, h)| h.module == module && kabuk_win::uia::part_id(h.module, h.value) == id)
        .map(|(_, h)| h.value)
}

/// What a screen reader calls a brightness level: "Brightness" when it is the only one
/// (`alone`), else its monitor's, as the user knows it ("Monitor 1 (main)", D91).
pub fn brightness_name(i18n: &I18n, number: u32, main: bool, alone: bool) -> String {
    let key = match (alone, main) {
        (true, _) => return i18n.tr("acc-brightness"),
        (false, true) => "acc-brightness-main",
        (false, false) => "acc-brightness-of",
    };
    i18n.tr_args(key, &[("value", number.into())])
}

/// What a screen reader says of an overview cell: its workspace, then the titles of the
/// windows in it ("Workspace 2: Firefox, Terminal"), or that it is empty.
pub fn workspace_cell(i18n: &I18n, number: u32, titles: &[String]) -> String {
    let label = i18n.tr_args("tip-workspace", &[("value", number.into())]);
    let titles: Vec<&str> = titles.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    let value = if titles.is_empty() {
        i18n.tr("acc-empty")
    } else {
        titles.join(TITLES_APART)
    };
    i18n.tr_args("acc-part", &[("label", label.into()), ("value", value.into())])
}

/// Between two window titles of an overview cell.
const TITLES_APART: &str = ", ";

/// The overview's cells for screen readers: each workspace's (its number, its place in
/// the sheet's pixels and its words) a list item clicked in its middle, the one the
/// arrows ringed (`selected`) holding the keyboard.
pub fn cell_parts(cells: &[(u32, PxRect, String)], selected: Option<usize>) -> Vec<Part> {
    cells
        .iter()
        .enumerate()
        .map(|(i, (number, rect, said))| Part {
            id: kabuk_win::uia::part_id(CELL, *number),
            name: said.clone(),
            role: Role::ListItem,
            rect: *rect,
            click: (rect.x + rect.w / 2, rect.y + rect.h / 2),
            focused: selected == Some(i),
            range: None,
        })
        .collect()
}

/// The kind of an overview cell's part (its value: the workspace's number).
const CELL: &str = "overview-cell";

/// What a screen reader reads of a notice: its title, then its second line when it has
/// one.
pub fn notice_words(i18n: &I18n, title: &str, sub: &str) -> String {
    if sub.trim().is_empty() {
        return title.to_string();
    }
    i18n.tr_args("acc-part", &[("label", title.into()), ("value", sub.into())])
}

/// A notice's window as a screen reader finds it: its `words` to read, over the whole of
/// it (`size`, its pixels).
pub fn notice_part(words: &str, (w, h): (u32, u32)) -> Part {
    let (w, h) = (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    );
    Part {
        id: kabuk_win::uia::part_id(NOTICE, 0),
        name: words.to_string(),
        role: Role::Text,
        rect: PxRect { x: 0, y: 0, w, h },
        click: (w / 2, h / 2),
        focused: false,
        range: None,
    }
}

/// The kind of a notice's part.
const NOTICE: &str = "notice";

/// What a screen reader calls a mute button (`app`: an app's in the mixer, else the
/// output's), by what pressing it does now.
pub fn mute_name(i18n: &I18n, app: Option<&str>, muted: bool) -> String {
    match (app, muted) {
        (None, false) => i18n.tr("acc-mute"),
        (None, true) => i18n.tr("acc-unmute"),
        (Some(a), false) => i18n.tr_args("acc-mute-app", &[("value", a.into())]),
        (Some(a), true) => i18n.tr_args("acc-unmute-app", &[("value", a.into())]),
    }
}

/// The words drawn in a part, as its name.
pub fn words(_: u32, drawn: &str) -> Option<String> {
    Some(drawn.to_string())
}

/// The bar's parts as drawn in `layout` at `origin`, each named by `name` (`None` leaves
/// it out).
pub fn bar_parts(
    layout: &Layout,
    origin: Point,
    scale: Scale,
    mut name: impl FnMut(&Drawn) -> Option<String>,
) -> Vec<Part> {
    kabuk_ui::access::parts(layout)
        .iter()
        .filter_map(|d| {
            Some(Part {
                id: kabuk_win::uia::part_id(d.hit.module, d.hit.value),
                name: name(d)?,
                role: Role::Button,
                rect: client_rect(d.rect, origin, scale),
                click: client_point(d.reach, origin, scale),
                focused: false,
                range: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: a module named whole by its value (a workspace, an app, a tray icon, a
    //! figure); a module said as its label and its value; the clock said by its drawn
    //! time rather than its tooltip's date; a value drawn as a picture (the volume); an
    //! icon-only module with no value said by its label; a tray icon without a tip falls
    //! back to the tray's label; nothing to say; both languages; DIPs to pixels at 150 %;
    //! a popup's parts of another module, without words or skipped are left out, and the
    //! selected one holds the keyboard. Levels: a popup's levels are sliders with their
    //! range and the rest keep their role; a level past full or at none; a SetValue's part
    //! found back among its regions (not another module's, not one gone); a brightness
    //! alone, of the main monitor and of another; the mute buttons by what they do, in
    //! both languages.
    use super::*;
    use kabuk_i18n::Lang;
    use kabuk_ui::node::{layout, FontRole, Node, Role as Colour, Span, TextMeasure, TextMetrics, TextSpec};

    fn said(tip: Option<&str>, own: Option<&str>) -> Said {
        Said {
            tip: tip.map(str::to_string),
            own: own.map(str::to_string),
        }
    }

    #[test]
    fn each_module_is_said_by_its_label_and_value() {
        let en = I18n::new(Lang::En);
        let tr = I18n::new(Lang::Tr);
        let none = Said::default();
        let tip = |t: &str| said(Some(t), None);
        let own = |o: &str| said(None, Some(o));
        let name = |i18n: &I18n, module: &'static str, drawn: &str, s: Said| bar_name(i18n, (module, 0), drawn, s);
        assert_eq!(
            name(&en, hit::WORKSPACE, "2", tip("Workspace 2")).as_deref(),
            Some("Workspace 2")
        );
        assert_eq!(
            name(&tr, hit::WORKSPACE, "2", tip("Çalışma alanı 2")).as_deref(),
            Some("Çalışma alanı 2")
        );
        assert_eq!(
            name(&en, hit::CLOCK, " 20:31 Mon 12 Oct ", tip("Monday 12 October 2026")).as_deref(),
            Some("Clock: 20:31 Mon 12 Oct")
        );
        assert_eq!(
            name(&tr, hit::CLOCK, "20:31 Pzt 12 Eki", none.clone()).as_deref(),
            Some("Saat: 20:31 Pzt 12 Eki")
        );
        assert_eq!(name(&en, hit::VOLUME, "", own("45%")).as_deref(), Some("Volume: 45%"));
        assert_eq!(
            name(&en, hit::NETWORK, "", tip("Home Wi-Fi")).as_deref(),
            Some("Network status: Home Wi-Fi")
        );
        // A figure by its own label, whether or not the look draws one.
        assert_eq!(
            bar_name(&en, (hit::STATS, hit::STAT_CPU), "CPU 12%", own("12%")).as_deref(),
            Some("CPU: 12%")
        );
        assert_eq!(
            bar_name(&tr, (hit::STATS, hit::STAT_GPU), "", own("46°")).as_deref(),
            Some("GPU sıcaklığı: 46°")
        );
        assert_eq!(name(&en, hit::APP, "", own("Firefox")).as_deref(), Some("Firefox"));
        assert_eq!(name(&en, hit::TRAY, "", own("Discord")).as_deref(), Some("Discord"));
        assert_eq!(name(&en, hit::TRAY, "", own("")).as_deref(), Some("System tray"));
        assert_eq!(name(&en, hit::POWER, "", none.clone()).as_deref(), Some("Power menu"));
        assert_eq!(name(&tr, hit::POWER, "", none.clone()).as_deref(), Some("Güç menüsü"));
        assert_eq!(
            name(&en, hit::TRAY_MORE, "+3", none.clone()).as_deref(),
            Some("System tray: +3")
        );
        assert_eq!(name(&en, "unknown", "", none.clone()), None);
        assert_eq!(name(&en, "unknown", "words", none).as_deref(), Some("words"));
    }

    #[test]
    fn dips_become_the_windows_pixels() {
        let r = client_rect(Rect::new(10.0, 2.0, 24.0, 20.0), Point { x: 4.0, y: 6.0 }, Scale(1.5));
        assert_eq!(
            r,
            PxRect {
                x: 21,
                y: 12,
                w: 36,
                h: 30
            }
        );
    }

    /// 8 DIPs per character, height 1.25 × size, baseline at 0.8 × size.
    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars = t.plain().chars().count() as f32;
            TextMetrics {
                width: chars * 8.0,
                height: t.size * 1.25,
                baseline: t.size * 0.8,
            }
        }
    }

    fn text(s: &str) -> Node {
        Node::text(TextSpec {
            spans: vec![Span {
                text: s.into(),
                color: Colour::Fg,
                weight: 400,
                after: 0.0,
                underline: false,
                italic: false,
                spacing: 0.0,
            }],
            font: FontRole::Ui,
            size: 12.0,
            max_width: None,
        })
    }

    #[test]
    fn a_popups_entries_are_its_parts() {
        let menu = Node::column(vec![
            text("Lock").hit("menu", 0),
            text("Sleep").hit("menu", 1),
            Node::icon("x", 16.0, Colour::Fg).hit("menu", 2),
            text("Level").hit("menu", 3),
            text("Elsewhere").hit("other", 4),
        ]);
        let l = layout(&menu, Point::default(), &Fake);
        let parts = popup_parts(&l, Scale(2.0), ("menu", Role::MenuItem), Some(1), |v, t| {
            (v != 3).then(|| t.to_string())
        });
        let names: Vec<(&str, bool)> = parts.iter().map(|p| (p.name.as_str(), p.focused)).collect();
        assert_eq!(names, vec![("Lock", false), ("Sleep", true)]);
        assert!(parts.iter().all(|p| p.role == Role::MenuItem));
        assert_eq!(parts[0].rect, client_rect(l.hits[0].0, Point::default(), Scale(2.0)));
        // Named by the bar where nothing is drawn: the icon-only entry is kept.
        let named = popup_parts(&l, Scale(1.0), ("menu", Role::Button), None, |v, t| {
            Some(if v == 2 { "Close".into() } else { t.to_string() })
        });
        assert_eq!(named.len(), 4);
        assert_eq!(named[2].name, "Close");
        assert_eq!(words(7, "Wi-Fi").as_deref(), Some("Wi-Fi"));
    }

    #[test]
    fn the_bars_parts_are_buttons_where_drawn() {
        let bar = Node::row(vec![text("1").hit(hit::WORKSPACE, 1), text("20:31").hit(hit::CLOCK, 0)]).gap(10.0);
        let l = layout(&bar, Point::default(), &Fake);
        let origin = Point { x: 14.0, y: 0.0 };
        let parts = bar_parts(&l, origin, Scale(1.0), |d| {
            (d.hit.module == hit::CLOCK).then(|| format!("Clock: {}", d.text))
        });
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "Clock: 20:31");
        assert_eq!(parts[0].role, Role::Button);
        assert_eq!(parts[0].rect, client_rect(l.hits[1].0, origin, Scale(1.0)));
    }

    /// PLAN M19 U17: the levels were left out (Invoke would have set them to half). They
    /// are sliders now, with the level drawn, set by a screen reader within 0-100.
    #[test]
    fn a_popups_levels_are_sliders_with_their_range() {
        let panel = Node::column(vec![
            text("Mute").hit("p", 0),
            text("track").hit("p", 1),
            text("Sound settings").hit("p", 2),
        ]);
        let l = layout(&panel, Point::default(), &Fake);
        let parts = popup_levels(
            &l,
            Scale(1.0),
            ("p", Role::Button),
            Some(1),
            |v, t| Some(if v == 1 { "Volume".into() } else { t.to_string() }),
            |v| (v == 1).then(|| level_range(40, 2)),
        );
        let got: Vec<(&str, Role, Option<Range>, bool)> = parts
            .iter()
            .map(|p| (p.name.as_str(), p.role, p.range, p.focused))
            .collect();
        let range = Range {
            value: 40,
            min: 0,
            max: 100,
            step: 2,
        };
        assert_eq!(
            got,
            vec![
                ("Mute", Role::Button, None, false),
                ("Volume", Role::Slider, Some(range), true),
                ("Sound settings", Role::Button, None, false),
            ]
        );
        // Without levels, as before.
        assert!(popup_parts(&l, Scale(1.0), ("p", Role::Button), None, words)
            .iter()
            .all(|p| p.role == Role::Button && p.range.is_none()));
        // A SetValue names its part: found back among the regions.
        assert_eq!(region_of(&l, "p", parts[1].id), Some(1));
        assert_eq!(region_of(&l, "other", parts[1].id), None, "another module's");
        assert_eq!(region_of(&l, "p", kabuk_win::uia::part_id("p", 9)), None, "gone");
    }

    #[test]
    fn a_level_is_offered_within_none_and_full() {
        assert_eq!(level_range(0, 2).value, 0);
        assert_eq!(level_range(100, 2).value, 100);
        assert_eq!(level_range(250, 2).value, 100, "past full is full");
        assert_eq!(
            (level_range(40, 5).min, level_range(40, 5).max, level_range(40, 5).step),
            (0, 100, 5)
        );
    }

    #[test]
    fn levels_and_mute_buttons_are_named_by_what_they_are() {
        let en = I18n::new(Lang::En);
        let tr = I18n::new(Lang::Tr);
        assert_eq!(brightness_name(&en, 1, true, true), "Brightness");
        assert_eq!(brightness_name(&en, 1, true, false), "Brightness, Monitor 1 (main)");
        assert_eq!(brightness_name(&en, 2, false, false), "Brightness, Monitor 2");
        assert_eq!(brightness_name(&tr, 1, true, false), "Parlaklık, Monitör 1 (ana)");
        assert_eq!(brightness_name(&tr, 2, false, false), "Parlaklık, Monitör 2");
        assert_eq!(mute_name(&en, None, false), "Mute");
        assert_eq!(mute_name(&en, None, true), "Unmute");
        assert_eq!(mute_name(&en, Some("Firefox"), false), "Mute Firefox");
        assert_eq!(mute_name(&tr, Some("Firefox"), true), "Firefox sesini aç");
        assert_eq!(mute_name(&tr, None, false), "Sesi kapat");
    }

    #[test]
    fn an_overview_cell_is_said_by_its_workspace_and_windows() {
        let en = I18n::new(Lang::En);
        let tr = I18n::new(Lang::Tr);
        let titles = vec!["Firefox".to_string(), "  ".to_string(), "Terminal".to_string()];
        assert_eq!(workspace_cell(&en, 2, &titles), "Workspace 2: Firefox, Terminal");
        assert_eq!(workspace_cell(&en, 3, &[]), "Workspace 3: empty");
        assert_eq!(workspace_cell(&tr, 3, &["  ".to_string()]), "Çalışma alanı 3: boş");
        let r = |x: i32| PxRect {
            x,
            y: 10,
            w: 100,
            h: 60,
        };
        let cells = vec![(1, r(0), "a".to_string()), (4, r(110), "b".to_string())];
        let parts = cell_parts(&cells, Some(1));
        assert_eq!(parts.len(), 2);
        assert_eq!(
            (parts[1].click, parts[1].focused, parts[0].focused),
            ((160, 40), true, false)
        );
        assert_eq!(parts[0].role, Role::ListItem);
        // Who a cell is follows its workspace, not its place.
        assert_eq!(parts[1].id, cell_parts(&cells[1..], None)[0].id);
        assert!(cell_parts(&cells, None).iter().all(|p| !p.focused));
    }

    #[test]
    fn a_notice_is_read_whole() {
        let en = I18n::new(Lang::En);
        assert_eq!(
            notice_words(&en, "Kabuk started again", "Its log says why."),
            "Kabuk started again: Its log says why."
        );
        assert_eq!(notice_words(&en, "Window is floating", " "), "Window is floating");
        let p = notice_part("Window is floating", (300, 80));
        assert_eq!(
            (p.role, p.rect, p.click),
            (
                Role::Text,
                PxRect {
                    x: 0,
                    y: 0,
                    w: 300,
                    h: 80
                },
                (150, 40)
            )
        );
        assert_eq!(p.range, None);
    }
}
