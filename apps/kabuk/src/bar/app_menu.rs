//! An app's menu on the bar (asked for by the user: the same as Windows' taskbar on a
//! right click): the program's jump list (its pinned entries, its own categories, its
//! frequent or recent files, its tasks; [`kabuk_core::jumplist`]), then its name (a new
//! window), pin or unpin, and close its window or all of them, in Windows' order, in the
//! tray panel's surface (D21, D26). Pure.

use kabuk_core::jumplist::{Group, Heading, Launch};
use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of an entry; the value is its index in [`entries`].
pub const HIT: &str = "app-menu";

/// What an entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// An entry of the jump list: a file opened with the program, or a task run.
    Launch(Launch),
    /// Another window of the program (or the pinned app started).
    Open,
    Pin,
    Unpin,
    /// Close its one window, or all of them.
    Close,
}

/// The most entries a group of the jump list shows (Windows' own default is 10).
pub const MAX_PER_GROUP: usize = 10;
/// An entry's height and the space between its icon and its words (DIPs), as the power
/// menu's.
const ROW: f32 = 30.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 10.0;
/// The widest an entry's words may be before they are cut (DIPs).
const MAX_TEXT: f32 = 320.0;
const HEADING_SIZE: f32 = 0.85;

/// An app as the menu needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subject {
    /// The name shown for "a new window" (the program's or the pin's).
    pub name: String,
    pub pinned: bool,
    pub windows: usize,
    /// Its jump list, in Windows' order.
    pub groups: Vec<Group>,
    /// The pictures of the jump list's entries in [`entries`]' order (a key of the
    /// renderer's pictures); an entry without one shows a plain icon.
    pub pictures: Vec<Option<u64>>,
    /// The app's own picture, for its name's entry (as on Windows' taskbar).
    pub app_picture: Option<u64>,
}

/// The side of an entry's picture (DIPs).
pub const PICTURE: f32 = ICON;

/// An entry's icon: a file or folder, or a program run.
fn launch_icon(l: &Launch) -> &'static str {
    match l {
        Launch::File(_) => "file",
        Launch::Run { .. } => "app",
    }
}

/// The clickable entries in Windows' order: the jump list's, then the name, pin or
/// unpin, close.
pub fn entries(s: &Subject) -> Vec<(&'static str, String, Action)> {
    let mut out: Vec<(&'static str, String, Action)> = s
        .groups
        .iter()
        .flat_map(|g| g.entries.iter().take(MAX_PER_GROUP))
        .map(|e| {
            (
                launch_icon(&e.launch),
                e.title.clone(),
                Action::Launch(e.launch.clone()),
            )
        })
        .collect();
    out.push(("app", s.name.clone(), Action::Open));
    if s.pinned {
        out.push(("tile", String::new(), Action::Unpin));
    } else {
        out.push(("tile", String::new(), Action::Pin));
    }
    if s.windows > 0 {
        out.push(("power", String::new(), Action::Close));
    }
    out
}

fn words(i18n: &I18n, text: &str, action: &Action, windows: usize) -> String {
    match action {
        Action::Pin => i18n.tr("app-pin"),
        Action::Unpin => i18n.tr("app-unpin"),
        Action::Close if windows > 1 => i18n.tr("app-close-all"),
        Action::Close => i18n.tr("app-close"),
        _ => text.to_string(),
    }
}

fn line(text: String, color: Role, size: f32) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text,
            color,
            weight: 400,
            after: 0.0,
            underline: false,
        }],
        font: FontRole::Ui,
        size,
        max_width: Some(MAX_TEXT),
    })
}

/// A group's heading in the user's words (a program names its own categories).
fn heading(i18n: &I18n, h: &Heading) -> String {
    match h {
        Heading::Pinned => i18n.tr("app-pinned"),
        Heading::Frequent => i18n.tr("app-frequent"),
        Heading::Recent => i18n.tr("app-recent"),
        Heading::Tasks => i18n.tr("app-tasks"),
        Heading::Named(name) => name.clone(),
    }
}

/// The menu's tree.
pub fn view(i18n: &I18n, text_scale: f32, s: &TipStyle, subject: &Subject) -> Node {
    let mut rows = Vec::new();
    // Each group's heading goes before its first entry.
    let mut starts = Vec::new();
    let mut at = 0;
    for g in &subject.groups {
        starts.push((at, heading(i18n, &g.heading)));
        at += g.entries.len().min(MAX_PER_GROUP);
    }
    for (i, (icon, text, action)) in entries(subject).iter().enumerate() {
        let own = (*action == Action::Open).then_some(subject.app_picture).flatten();
        let picture = match own.or_else(|| subject.pictures.get(i).copied().flatten()) {
            Some(key) => Node::image(key, ICON),
            None => Node::icon(icon, ICON, Role::Dim),
        };
        for (_, h) in starts.iter().filter(|(start, _)| *start == i) {
            rows.push(
                line(h.clone(), Role::Dim, s.text * text_scale * HEADING_SIZE)
                    .padding(Insets::symmetric(s.pad_h, 0.0))
                    .size(None, Some(ROW)),
            );
        }
        rows.push(
            Node::row(vec![
                picture,
                line(
                    words(i18n, text, action, subject.windows),
                    Role::Fg,
                    s.text * text_scale,
                ),
            ])
            .gap(ICON_GAP)
            .align(Align::Center)
            .padding(Insets::symmetric(s.pad_h, 0.0))
            .size(None, Some(ROW))
            .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX)),
        );
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
    //! Edge cases: a pinned app with no window (no close), a running one with one window
    //! and with several (close all), a jump list's groups capped and each under its
    //! heading; every entry clickable and worded in both languages.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_core::jumplist::Entry;
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

    fn subject(pinned: bool, windows: usize, recent: usize) -> Subject {
        let recent = Group {
            heading: Heading::Recent,
            entries: (0..recent)
                .map(|i| Entry {
                    title: format!("f{i}.txt"),
                    launch: Launch::File(format!(r"C:\f{i}.txt")),
                    icon: None,
                })
                .collect(),
        };
        Subject {
            name: "Visual Studio Code".into(),
            pinned,
            windows,
            groups: if recent.entries.is_empty() {
                Vec::new()
            } else {
                vec![recent]
            },
            pictures: Vec::new(),
            app_picture: None,
        }
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
    fn the_entries_follow_windows_taskbar() {
        let actions = |s: &Subject| entries(s).into_iter().map(|(_, _, a)| a).collect::<Vec<_>>();
        assert_eq!(actions(&subject(true, 0, 0)), vec![Action::Open, Action::Unpin]);
        assert_eq!(
            actions(&subject(false, 1, 0)),
            vec![Action::Open, Action::Pin, Action::Close]
        );
        let many = actions(&subject(false, 2, 14));
        assert_eq!(many.len(), MAX_PER_GROUP + 3);
        assert_eq!(many[0], Action::Launch(Launch::File(r"C:\f0.txt".into())));
    }

    /// Chrome's list as Windows shows it: its categories, then its tasks, each under its
    /// heading; then its name, unpin, close.
    #[test]
    fn each_group_of_the_jump_list_comes_under_its_heading() {
        let i18n = I18n::new(Lang::En);
        let run = |t: &str| Entry {
            title: t.into(),
            launch: Launch::Run {
                path: "chrome.exe".into(),
                args: t.into(),
                dir: String::new(),
            },
            icon: None,
        };
        let s = Subject {
            name: "Google Chrome".into(),
            pinned: true,
            windows: 1,
            groups: vec![
                Group {
                    heading: Heading::Named("Most visited".into()),
                    entries: vec![run("YouTube"), run("GitHub")],
                },
                Group {
                    heading: Heading::Tasks,
                    entries: vec![run("New window"), run("New Incognito window")],
                },
            ],
            // A favicon for the first site; the rest show a plain icon.
            pictures: vec![Some(77), None, None, None],
            app_picture: Some(99),
        };
        let t = texts(&view(&i18n, 1.0, &TIP, &s));
        let expected = [
            "Most visited".to_string(),
            "YouTube".into(),
            "GitHub".into(),
            i18n.tr("app-tasks"),
            "New window".into(),
            "New Incognito window".into(),
            "Google Chrome".into(),
            i18n.tr("app-unpin"),
            i18n.tr("app-close"),
        ];
        assert_eq!(t, expected);
        assert_eq!(entries(&s)[3].2, Action::Launch(run("New Incognito window").launch));
        let images: Vec<u64> = layout(&view(&i18n, 1.0, &TIP, &s), Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Image { spec, .. } => Some(spec.key),
                _ => None,
            })
            .collect();
        assert_eq!(images, vec![77, 99], "the first site's and the app's own pictures");
    }

    #[test]
    fn every_entry_is_worded_and_clickable() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for key in [
                "app-pin",
                "app-unpin",
                "app-close",
                "app-close-all",
                "app-recent",
                "app-pinned",
                "app-frequent",
                "app-tasks",
            ] {
                let w = i18n.tr(key);
                assert!(!w.is_empty() && w != key, "{key} {lang:?}");
            }
            let s = subject(false, 2, 2);
            let n = view(&i18n, 1.0, &TIP, &s);
            let l = layout(&n, Point::default(), &Fake);
            let hits: Vec<u32> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == HIT)
                .map(|(_, h)| h.value)
                .collect();
            assert_eq!(hits, vec![0, 1, 2, 3, 4]);
            let t = texts(&n);
            assert_eq!(t[0], i18n.tr("app-recent"));
            assert!(t.contains(&"Visual Studio Code".to_string()));
            assert!(t.contains(&i18n.tr("app-close-all")));
        }
    }
}
