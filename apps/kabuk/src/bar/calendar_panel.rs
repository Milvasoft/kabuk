//! The calendar and notifications panel (DECISIONS D88): what a click on the bar's clock
//! opens, Kabuk's own in place of Windows' notification centre: the notices Windows
//! keeps (each dismissed alone, or all at once), and a month calendar that moves back and
//! forward with today marked. Drawn as the design page's `calPanelHtml`. Pure.

use kabuk_core::calendar;
use kabuk_i18n::I18n;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::tooltip::TipStyle;

/// Hit-test id of the panel's parts; the value is one of the constants below.
pub const HIT: &str = "calendar-panel";
pub const CLEAR: u32 = 0;
pub const PREVIOUS: u32 = 1;
pub const NEXT: u32 = 2;
pub const SETTINGS: u32 = 3;
pub const TODAY: u32 = 4;
/// A notice's close mark, in the list's order.
pub const FIRST_DISMISS: u32 = 100;
/// Windows' notification settings.
pub const SETTINGS_PAGE: &str = "ms-settings:notifications";

/// The content width (DIPs), as the network panel's.
const WIDTH: f32 = 356.0;
/// The most notices listed.
pub const MAX_NOTICES: usize = 5;
/// A notice's icon and close mark, and the space beside them.
const ICON: f32 = 16.0;
const MARK: f32 = 14.0;
const GAP: f32 = 10.0;
const CARD_PAD: f32 = 8.0;
/// A day's cell height and corner radius.
const DAY_H: f32 = 28.0;
const DAY_RADIUS: f32 = 6.0;
/// A header row's height.
const HEAD: f32 = 28.0;
/// Second lines are drawn this much smaller.
const SUB_SCALE: f32 = 0.9;

/// A notice as the panel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeRow {
    pub id: u32,
    pub app: String,
    pub when: String,
    pub title: String,
    pub body: String,
}

/// What the panel shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    /// `None` while Windows does not let Kabuk read them (or they are being read).
    pub notices: Option<Vec<NoticeRow>>,
    /// The month shown, and today.
    pub year: i32,
    pub month: u32,
    pub today: (i32, u32, u32),
}

/// What the host has to do after a click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Dismiss(u32),
    ClearAll,
    Open(&'static str),
}

impl Panel {
    pub fn new(today: (i32, u32, u32)) -> Panel {
        Panel {
            notices: None,
            year: today.0,
            month: today.1,
            today,
        }
    }

    pub fn click(&mut self, value: u32) -> Action {
        match value {
            PREVIOUS | NEXT => {
                let by = if value == NEXT { 1 } else { -1 };
                (self.year, self.month) = calendar::shifted(self.year, self.month, by);
                Action::Redraw
            }
            TODAY => {
                (self.year, self.month) = (self.today.0, self.today.1);
                Action::Redraw
            }
            CLEAR => Action::ClearAll,
            SETTINGS => Action::Open(SETTINGS_PAGE),
            v if v >= FIRST_DISMISS => self
                .notices
                .as_ref()
                .and_then(|n| n.get((v - FIRST_DISMISS) as usize))
                .map_or(Action::None, |n| Action::Dismiss(n.id)),
            _ => Action::None,
        }
    }
}

fn words(text: String, color: Role, weight: u16, font: FontRole, size: f32, max_width: f32) -> Node {
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
        font,
        size,
        max_width: Some(max_width),
    })
}

/// `left` from the start, `right` against the end.
fn ends(left: Node, right: Node, right_w: f32) -> Node {
    Node::row(vec![
        Node::column(vec![left])
            .align(Align::Start)
            .size(Some(WIDTH - right_w), None),
        Node::column(vec![right]).align(Align::End).size(Some(right_w), None),
    ])
    .align(Align::Center)
    .size(None, Some(HEAD))
}

fn notice(n: &NoticeRow, i: usize, size: f32) -> Node {
    let sub = size * SUB_SCALE;
    let text_w = WIDTH - ICON - MARK - 2.0 * GAP - 2.0 * CARD_PAD;
    let head = words(
        format!("{} · {}", n.app, n.when),
        Role::Dim,
        400,
        FontRole::Ui,
        sub,
        text_w,
    );
    let mut lines = vec![head];
    if !n.title.is_empty() {
        lines.push(words(n.title.clone(), Role::Fg, 600, FontRole::Ui, size, text_w));
    }
    if !n.body.is_empty() {
        lines.push(words(n.body.clone(), Role::Dim, 400, FontRole::Ui, sub, text_w));
    }
    let close = Node::icon("x", MARK, Role::Dim).hit(HIT, FIRST_DISMISS + u32::try_from(i).unwrap_or(0));
    Node::row(vec![
        Node::icon("bell", ICON, Role::Dim),
        Node::column(lines)
            .align(Align::Start)
            .gap(2.0)
            .size(Some(text_w), None),
        close,
    ])
    .gap(GAP)
    .align(Align::Start)
    .padding(Insets::all(CARD_PAD))
    .paint(Paint {
        fill: Role::SurfaceSel,
        border: None,
        radius: 8.0,
    })
}

fn month_grid(p: &Panel, i18n: &I18n, size: f32) -> Vec<Node> {
    let cell = WIDTH / 7.0;
    let sub = size * SUB_SCALE;
    let title = words(
        format!(
            "{} {}",
            i18n.tr(&format!("months-long-{}", p.month.clamp(1, 12) - 1)),
            p.year
        ),
        Role::Fg,
        600,
        FontRole::Ui,
        size,
        WIDTH / 2.0,
    )
    .hit(HIT, TODAY);
    let arrows = Node::row(vec![
        Node::icon("left", ICON, Role::Dim).hit(HIT, PREVIOUS),
        Node::icon("right", ICON, Role::Dim).hit(HIT, NEXT),
    ])
    .gap(GAP);
    let mut rows = vec![ends(title, arrows, 2.0 * ICON + GAP)];
    // Monday first, as the design: Windows' day names are from Sunday.
    let names: Vec<Node> = [1, 2, 3, 4, 5, 6, 0]
        .iter()
        .map(|d| {
            Node::row(vec![words(
                i18n.tr(&format!("days-{d}")),
                Role::Dim,
                400,
                FontRole::Ui,
                sub,
                cell,
            )])
            .size(Some(cell), Some(DAY_H))
        })
        .collect();
    rows.push(Node::row(names));
    let cells = calendar::cells(p.year, p.month);
    for week in cells.chunks(7) {
        let days: Vec<Node> = week
            .iter()
            .map(|c| match c {
                Some(day) => {
                    let today = (p.year, p.month, *day) == p.today;
                    let node = Node::row(vec![words(
                        day.to_string(),
                        if today { Role::OnAccent } else { Role::Fg },
                        if today { 600 } else { 400 },
                        FontRole::Num,
                        size,
                        cell,
                    )])
                    .size(Some(cell), Some(DAY_H));
                    if today {
                        node.paint(Paint {
                            fill: Role::AccentFill,
                            border: None,
                            radius: DAY_RADIUS,
                        })
                    } else {
                        node
                    }
                }
                None => Node::spacer(cell, DAY_H),
            })
            .collect();
        rows.push(Node::row(days));
    }
    rows
}

/// The panel's tree.
pub fn view(p: &Panel, i18n: &I18n, text_scale: f32, s: &TipStyle) -> Node {
    let size = s.text * text_scale;
    let link = |text: String, value: u32| {
        words(text, Role::Accent, 400, FontRole::Ui, size * SUB_SCALE, WIDTH / 2.0).hit(HIT, value)
    };
    let title = words(i18n.tr("notif-title"), Role::Fg, 600, FontRole::Ui, size, WIDTH / 2.0);
    let mut rows = Vec::new();
    match &p.notices {
        Some(list) if !list.is_empty() => {
            rows.push(ends(title, link(i18n.tr("notif-clear"), CLEAR), WIDTH / 2.0));
            rows.extend(
                list.iter()
                    .take(MAX_NOTICES)
                    .enumerate()
                    .map(|(i, n)| notice(n, i, size)),
            );
        }
        Some(_) => {
            rows.push(ends(title, link(i18n.tr("notif-settings"), SETTINGS), WIDTH / 2.0));
            rows.push(words(
                i18n.tr("notif-none"),
                Role::Dim,
                400,
                FontRole::Ui,
                size * SUB_SCALE,
                WIDTH,
            ));
        }
        None => rows.push(ends(title, link(i18n.tr("notif-settings"), SETTINGS), WIDTH / 2.0)),
    }
    rows.push(Node::spacer(WIDTH, 1.0).paint(Paint {
        fill: Role::SurfaceLine,
        border: None,
        radius: 0.0,
    }));
    rows.extend(month_grid(p, i18n, size));
    Node::column(rows)
        .align(Align::Start)
        .gap(6.0)
        .padding(Insets::all(s.pad_h + s.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: the arrows over a year's end and back to today; a notice dismissed by
    //! its place, a place past the list; no notices, notices not readable; today marked
    //! in its month only; every string in both languages.
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

    fn row(id: u32, app: &str) -> NoticeRow {
        NoticeRow {
            id,
            app: app.into(),
            when: "20:12".into(),
            title: "Ayşe".into(),
            body: "Akşam yemeğe geliyor musun?".into(),
        }
    }

    #[test]
    fn the_arrows_move_over_the_year_and_the_title_comes_back() {
        let mut p = Panel::new((2026, 12, 22));
        assert_eq!(p.click(NEXT), Action::Redraw);
        assert_eq!((p.year, p.month), (2027, 1));
        p.click(PREVIOUS);
        p.click(PREVIOUS);
        assert_eq!((p.year, p.month), (2026, 11));
        p.click(TODAY);
        assert_eq!((p.year, p.month), (2026, 12));
    }

    #[test]
    fn a_notice_is_dismissed_by_its_place() {
        let mut p = Panel::new((2026, 10, 22));
        p.notices = Some(vec![row(7, "WhatsApp"), row(9, "Outlook")]);
        assert_eq!(p.click(FIRST_DISMISS + 1), Action::Dismiss(9));
        assert_eq!(p.click(FIRST_DISMISS + 5), Action::None);
        assert_eq!(p.click(CLEAR), Action::ClearAll);
        assert_eq!(p.click(SETTINGS), Action::Open(SETTINGS_PAGE));
    }

    #[test]
    fn the_panel_lists_notices_and_the_month() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let mut p = Panel::new((2026, 10, 22));
            p.notices = Some(vec![row(7, "WhatsApp")]);
            let n = view(&p, &i18n, 1.0, &TIP);
            let t = texts(&n);
            assert!(t.contains(&"WhatsApp · 20:12".to_string()) && t.contains(&"Ayşe".to_string()));
            assert!(t.contains(&i18n.tr("notif-clear")));
            assert!(t.contains(&format!("{} 2026", i18n.tr("months-long-9"))));
            assert_eq!(t.iter().filter(|x| x.parse::<u32>().is_ok()).count(), 31);
            // Today, and only today, on the accent.
            let l = layout(&n, Point::default(), &Fake);
            let marked = l
                .draws
                .iter()
                .filter(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::AccentFill))
                .count();
            assert_eq!(marked, 1);
            p.click(NEXT);
            let l = layout(&view(&p, &i18n, 1.0, &TIP), Point::default(), &Fake);
            assert!(!l
                .draws
                .iter()
                .any(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::AccentFill)));
            // None, and none readable: said, with Windows' settings at hand.
            p.notices = Some(Vec::new());
            let t = texts(&view(&p, &i18n, 1.0, &TIP));
            assert!(t.contains(&i18n.tr("notif-none")) && t.contains(&i18n.tr("notif-settings")));
            p.notices = None;
            assert!(texts(&view(&p, &i18n, 1.0, &TIP)).contains(&i18n.tr("notif-settings")));
        }
    }
}
