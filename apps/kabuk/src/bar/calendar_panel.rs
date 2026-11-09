//! The calendar and notifications panel (DECISIONS D88): what a click on the bar's clock
//! opens, Kabuk's own in place of Windows' notification centre: the notices Windows
//! keeps (each dismissed alone, or all at once), and a month calendar: six weeks with their
//! numbers and the neighbouring months' days, today marked, a day chosen to see how far off
//! it is, and the months of a year to jump between (D103). Drawn as the design page's
//! `calPanelHtml`. Pure.

use kabuk_core::calendar;
use kabuk_i18n::{Arg, I18n};
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
/// The month's name over the sheet, which opens the months.
pub const TITLE: u32 = 5;
/// A notice's close mark, in the list's order.
pub const FIRST_DISMISS: u32 = 100;
/// A month on the months' sheet, January first.
pub const FIRST_MONTH: u32 = 200;
/// Hit-test id of a day on the sheet; the value is its place on it. Apart from [`HIT`] so
/// the keyboard's Tab goes over the panel's links, not 42 days.
pub const DAY: &str = "calendar-day";
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
/// Between the panel's rows.
const ROW_GAP: f32 = 6.0;
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

/// Which sheet is under the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Days,
    Months,
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
    pub page: Page,
    /// The day clicked, if one is.
    pub chosen: Option<(i32, u32, u32)>,
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
            page: Page::Days,
            chosen: None,
        }
    }

    /// Whether there is anything for "today" to go back from.
    pub fn away(&self) -> bool {
        (self.year, self.month) != (self.today.0, self.today.1) || self.page == Page::Months || self.chosen.is_some()
    }

    /// Moves by `by` months, or years on the months' sheet.
    pub fn turn(&mut self, by: i32) -> Action {
        if by == 0 {
            return Action::None;
        }
        let months = if self.page == Page::Months { by * 12 } else { by };
        (self.year, self.month) = calendar::shifted(self.year, self.month, months);
        Action::Redraw
    }

    /// The day at place `index` on the sheet: chosen, or no longer chosen if it was; a
    /// neighbouring month's day brings its month.
    pub fn click_day(&mut self, index: u32) -> Action {
        let Some(d) = calendar::grid(self.year, self.month).get(index as usize).copied() else {
            return Action::None;
        };
        (self.year, self.month) = (d.year, d.month);
        let date = (d.year, d.month, d.day);
        self.chosen = (self.chosen != Some(date)).then_some(date);
        Action::Redraw
    }

    pub fn click(&mut self, value: u32) -> Action {
        match value {
            PREVIOUS => self.turn(-1),
            NEXT => self.turn(1),
            TODAY => {
                (self.year, self.month) = (self.today.0, self.today.1);
                self.page = Page::Days;
                self.chosen = None;
                Action::Redraw
            }
            TITLE => {
                self.page = match self.page {
                    Page::Days => Page::Months,
                    Page::Months => Page::Days,
                };
                Action::Redraw
            }
            v if (FIRST_MONTH..FIRST_MONTH + 12).contains(&v) => {
                self.month = v - FIRST_MONTH + 1;
                self.page = Page::Days;
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

/// `node` on `fill`, when it has one.
fn on(node: Node, fill: Option<Role>) -> Node {
    match fill {
        Some(fill) => node.paint(Paint {
            fill,
            border: None,
            radius: DAY_RADIUS,
        }),
        None => node,
    }
}

/// How far from today a date is, in words.
fn from_today(i18n: &I18n, days: i64) -> String {
    let count = |key: &str, n: i64| i18n.tr_args(key, &[("value", Arg::Num(n as f64))]);
    match days {
        0 => i18n.tr("cal-today"),
        1 => i18n.tr("cal-tomorrow"),
        -1 => i18n.tr("cal-yesterday"),
        d if d > 0 => count("cal-in-days", d),
        d => count("cal-days-ago", -d),
    }
}

/// The line under the sheet: the chosen day, or today, by name, how far off and its week.
fn summary(p: &Panel, i18n: &I18n) -> String {
    let (year, month, day) = p.chosen.unwrap_or(p.today);
    let date = i18n.date_long(
        u8::try_from((calendar::weekday(year, month, day) + 1) % 7).unwrap_or(0),
        u8::try_from(day).unwrap_or(1),
        u8::try_from(month).unwrap_or(1),
    );
    let days = calendar::days_between(p.today, (year, month, day));
    let week = i18n.tr_args(
        "cal-week",
        &[("value", Arg::Num(f64::from(calendar::iso_week(year, month, day))))],
    );
    format!("{date} · {} · {week}", from_today(i18n, days))
}

/// The header over the sheet: the month (a click opens the months), back to today when
/// away from it, and the arrows.
fn sheet_head(p: &Panel, i18n: &I18n, size: f32) -> Node {
    let text = match p.page {
        Page::Days => format!(
            "{} {}",
            i18n.tr(&format!("months-long-{}", p.month.clamp(1, 12) - 1)),
            p.year
        ),
        Page::Months => p.year.to_string(),
    };
    let title = Node::row(vec![
        words(text, Role::Fg, 600, FontRole::Ui, size, WIDTH / 2.0 - ICON - GAP),
        Node::icon("down", MARK, Role::Dim),
    ])
    .gap(GAP / 2.0)
    .hit(HIT, TITLE);
    let mut right = Vec::new();
    if p.away() {
        right.push(
            words(
                i18n.tr("cal-today"),
                Role::Accent,
                400,
                FontRole::Ui,
                size * SUB_SCALE,
                WIDTH / 4.0,
            )
            .hit(HIT, TODAY),
        );
    }
    right.push(Node::icon("left", ICON, Role::Dim).hit(HIT, PREVIOUS));
    right.push(Node::icon("right", ICON, Role::Dim).hit(HIT, NEXT));
    ends(title, Node::row(right).gap(GAP), WIDTH / 2.0)
}

/// The days' sheet: a row of day names, then six weeks, each with its number first.
fn days_sheet(p: &Panel, i18n: &I18n, size: f32) -> Vec<Node> {
    let cell = WIDTH / 8.0;
    let sub = size * SUB_SCALE;
    let label =
        |text: String, color: Role| words(text, color, 400, FontRole::Ui, sub, cell).size(Some(cell), Some(DAY_H));
    // Monday first, as the design: Windows' day names are from Sunday.
    let names = std::iter::once(label(i18n.tr("cal-week-head"), Role::Faint))
        .chain(
            [1, 2, 3, 4, 5, 6, 0]
                .iter()
                .map(|d| label(i18n.tr(&format!("days-{d}")), Role::Dim)),
        )
        .collect();
    let mut rows = vec![Node::row(names)];
    let sheet = calendar::grid(p.year, p.month);
    for (w, week) in sheet.chunks(7).enumerate() {
        let number = label(
            calendar::iso_week(week[0].year, week[0].month, week[0].day).to_string(),
            Role::Faint,
        );
        let days = week.iter().enumerate().map(|(d, g)| {
            let date = (g.year, g.month, g.day);
            let today = g.inside && date == p.today;
            let chosen = p.chosen == Some(date);
            let color = match (today, g.inside) {
                (true, _) => Role::OnAccent,
                (false, true) => Role::Fg,
                (false, false) => Role::Faint,
            };
            let weight = if today || chosen { 600 } else { 400 };
            let node = words(g.day.to_string(), color, weight, FontRole::Num, size, cell)
                .size(Some(cell), Some(DAY_H))
                .hit(DAY, u32::try_from(w * 7 + d).unwrap_or(0));
            let fill = if today {
                Some(Role::AccentFill)
            } else if chosen {
                Some(Role::SurfaceSel)
            } else {
                None
            };
            on(node, fill)
        });
        rows.push(Node::row(std::iter::once(number).chain(days).collect()));
    }
    rows
}

/// The months' sheet, as tall as the days' so the panel does not change height.
fn months_sheet(p: &Panel, i18n: &I18n, size: f32) -> Vec<Node> {
    let cell = WIDTH / 4.0;
    let day_rows = 7.0;
    let height = ((day_rows * DAY_H + (day_rows - 1.0) * ROW_GAP) - 2.0 * ROW_GAP) / 3.0;
    let names: Vec<u32> = (1..=12).collect();
    names
        .chunks(4)
        .map(|row| {
            let cells = row
                .iter()
                .map(|m| {
                    let now = (p.year, *m) == (p.today.0, p.today.1);
                    let color = if now { Role::OnAccent } else { Role::Fg };
                    let node = words(
                        i18n.tr(&format!("months-long-{}", m - 1)),
                        color,
                        if now { 600 } else { 400 },
                        FontRole::Ui,
                        size,
                        cell,
                    )
                    .size(Some(cell), Some(height))
                    .hit(HIT, FIRST_MONTH + m - 1);
                    let fill = if now {
                        Some(Role::AccentFill)
                    } else if *m == p.month {
                        Some(Role::SurfaceSel)
                    } else {
                        None
                    };
                    on(node, fill)
                })
                .collect();
            Node::row(cells)
        })
        .collect()
}

fn month_grid(p: &Panel, i18n: &I18n, size: f32) -> Vec<Node> {
    let mut rows = vec![sheet_head(p, i18n, size)];
    rows.extend(match p.page {
        Page::Days => days_sheet(p, i18n, size),
        Page::Months => months_sheet(p, i18n, size),
    });
    rows.push(words(
        summary(p, i18n),
        Role::Dim,
        400,
        FontRole::Ui,
        size * SUB_SCALE,
        WIDTH,
    ));
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
        .gap(ROW_GAP)
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
    //! in its month only; every string in both languages; a day centred in its cell; a
    //! neighbour's day bringing its month; a day chosen twice; the months' sheet over a
    //! year; dates far from today in words.
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

    /// Found by the user: the numbers sat at the left of their cells, today's in the
    /// corner of its mark.
    #[test]
    fn a_day_is_centred_in_its_cell_and_its_mark() {
        let i18n = I18n::new(Lang::En);
        let p = Panel::new((2026, 11, 9));
        let l = layout(&view(&p, &i18n, 1.0, &TIP), Point::default(), &Fake);
        let mark = l
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Panel { rect, paint } if paint.fill == Role::AccentFill => Some(*rect),
                _ => None,
            })
            .expect("today's mark");
        let number = l
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { rect, spec } if spec.plain() == "9" && spec.spans[0].color == Role::OnAccent => {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("today's number");
        assert!(
            ((mark.x + mark.w / 2.0) - (number.x + number.w / 2.0)).abs() < 0.01,
            "{mark:?} {number:?}"
        );
        assert!(
            ((mark.y + mark.h / 2.0) - (number.y + number.h / 2.0)).abs() < 0.01,
            "{mark:?} {number:?}"
        );
        // The day names stand over their columns.
        let name_x = l.draws.iter().find_map(|d| match d {
            Draw::Text { rect, spec } if spec.plain() == i18n.tr("days-1") => Some(rect.x + rect.w / 2.0),
            _ => None,
        });
        let monday = l
            .hits
            .iter()
            .find(|(_, h)| h.module == DAY && h.value == 0)
            .map(|(r, _)| r.x + r.w / 2.0);
        assert!((name_x.unwrap() - monday.unwrap()).abs() < 0.01);
    }

    #[test]
    fn every_month_keeps_the_panels_height() {
        let i18n = I18n::new(Lang::Tr);
        let mut p = Panel::new((2026, 11, 9));
        let height = |p: &Panel| layout(&view(p, &i18n, 1.0, &TIP), Point::default(), &Fake).size.h;
        let first = height(&p);
        for _ in 0..24 {
            p.click(NEXT);
            assert_eq!(height(&p), first);
        }
        p.click(TITLE);
        assert!(
            (height(&p) - first).abs() < 0.01,
            "the months' sheet changed the height"
        );
    }

    #[test]
    fn a_day_is_chosen_and_a_neighbours_brings_its_month() {
        let mut p = Panel::new((2026, 11, 9));
        assert!(!p.away());
        // November 2026's sheet starts on 26 October.
        assert_eq!(p.click_day(8), Action::Redraw);
        assert_eq!(p.chosen, Some((2026, 11, 3)));
        assert!(p.away());
        // Again: no longer chosen.
        p.click_day(8);
        assert_eq!(p.chosen, None);
        p.click_day(0);
        assert_eq!((p.year, p.month, p.chosen), (2026, 10, Some((2026, 10, 26))));
        assert_eq!(p.click_day(99), Action::None);
        // Today goes back from all of it.
        p.click(TODAY);
        assert_eq!((p.year, p.month, p.chosen, p.page), (2026, 11, None, Page::Days));
    }

    #[test]
    fn the_months_sheet_moves_by_years_and_picks_a_month() {
        let mut p = Panel::new((2026, 11, 9));
        assert_eq!(p.click(TITLE), Action::Redraw);
        assert_eq!(p.page, Page::Months);
        p.click(NEXT);
        assert_eq!((p.year, p.month), (2027, 11));
        p.turn(-2);
        assert_eq!(p.year, 2025);
        p.click(FIRST_MONTH + 2);
        assert_eq!((p.year, p.month, p.page), (2025, 3, Page::Days));
        // The last month, and past it.
        p.click(TITLE);
        p.click(FIRST_MONTH + 11);
        assert_eq!(p.month, 12);
        assert_eq!(p.click(FIRST_MONTH + 12), Action::None);
        // On the days' sheet the wheel turns months, and no notch is no turn.
        p.turn(1);
        assert_eq!((p.year, p.month), (2026, 1));
        assert_eq!(p.turn(0), Action::None);
    }

    #[test]
    fn the_line_under_the_sheet_says_how_far_off_a_day_is() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for (days, key) in [(0, "cal-today"), (1, "cal-tomorrow"), (-1, "cal-yesterday")] {
                assert_eq!(from_today(&i18n, days), i18n.tr(key));
            }
            assert!(from_today(&i18n, 12).contains("12"));
            assert!(from_today(&i18n, -12).contains("12"));
            assert_ne!(from_today(&i18n, 12), from_today(&i18n, -12));
        }
        let i18n = I18n::new(Lang::En);
        let mut p = Panel::new((2026, 11, 9));
        assert_eq!(
            summary(&p, &i18n),
            format!("{} · Today · Week 46", i18n.date_long(1, 9, 11))
        );
        p.chosen = Some((2026, 11, 12));
        assert!(summary(&p, &i18n).contains("In 3 days"));
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
            let month_days = layout(&n, Point::default(), &Fake)
                .draws
                .iter()
                .filter(|d| {
                    matches!(d, Draw::Text { spec, .. }
                        if spec.plain().parse::<u32>().is_ok()
                            && matches!(spec.spans[0].color, Role::Fg | Role::OnAccent))
                })
                .count();
            assert_eq!(month_days, 31);
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
