//! What screen readers are told of the bar, the launcher and the bar's popups (DECISIONS
//! D97): each clickable part of a drawing, named in the user's language, with its role
//! and its place in the window. Pure: the bar thread hands the parts to its windows
//! ([`kabuk_win::msgloop::Window::set_accessible`]) after drawing them.

use kabuk_i18n::I18n;
use kabuk_ui::access::Part as Drawn;
use kabuk_ui::geom::{Point, Rect, Scale};
use kabuk_ui::node::Layout;
use kabuk_win::msgloop::PxRect;
use kabuk_win::uia::{Part, Role};

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
    let mut out: Vec<Part> = Vec::new();
    for d in kabuk_ui::access::parts(layout) {
        if d.hit.module != module {
            continue;
        }
        let Some(said) = name(d.hit.value, &d.text).filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        out.push(Part {
            id: kabuk_win::uia::part_id(d.hit.module, d.hit.value),
            name: said,
            role,
            rect: client_rect(d.rect, Point::default(), scale),
            click: client_point(d.reach, Point::default(), scale),
            focused: selected == Some(d.hit.value),
        });
    }
    out
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
    //! selected one holds the keyboard.
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
}
