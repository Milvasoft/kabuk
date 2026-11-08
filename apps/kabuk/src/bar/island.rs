//! Damla's dynamic island (DECISIONS D99): a black pill at the top of each monitor in
//! place of a bar. At rest it is two dots; the mouse opens it into a panel of pages; a
//! change of level, a notice or a new track grows it for a moment. It moves on springs,
//! its contents cross-fade, and nothing runs while it rests. Pure: the window is
//! `island_host`'s.

use kabuk_ui::geom::{Insets, Point, Rect};
use kabuk_ui::motion::{Level as MotionLevel, Spring};
use kabuk_ui::node::{layout, Align, Draw, FontRole, Layout, Node, Paint, Role, Span, TextMeasure, TextSpec};

use std::time::{Duration, Instant};

use crate::notify::view::Level;

/// What a press on the island lands on.
pub mod hit {
    pub const PREV: &str = "island-prev";
    pub const NEXT: &str = "island-next";
    pub const DOT: &str = "island-dot";
    pub const LAUNCHER: &str = "island-launcher";
    pub const GRID: &str = "island-grid";
    pub const OVERVIEW: &str = "island-overview";
    pub const VOLUME: &str = "island-volume";
    pub const QUICK: &str = "island-quick";
    pub const POWER: &str = "island-power";
    pub const TRACK_PREV: &str = "island-track-prev";
    pub const TRACK_PLAY: &str = "island-track-play";
    pub const TRACK_NEXT: &str = "island-track-next";
}

/// How many pages the open island has: what is in front, what plays, the figures.
pub const PAGES: usize = 3;
/// The page of the figures: they are read only while it shows.
pub const SYSTEM_PAGE: usize = 2;

/// Sizes (DIPs): width and height of each state, its distance from the screen's top and
/// the corner of a tall island. A short one is a full capsule.
const IDLE: (f32, f32) = (112.0, 32.0);
const OPEN: (f32, f32) = (560.0, 78.0);
const LEVEL: (f32, f32) = (280.0, 44.0);
const TRACK: (f32, f32) = (340.0, 48.0);
const NOTICE: (f32, f32) = (400.0, 56.0);
pub const TOP: f32 = 6.0;
const CORNER: f32 = 28.0;
const DOT_SIZE: f32 = 10.0;
const DOTS_GAP: f32 = 8.0;
const SIDE_PAD: f32 = 14.0;
const ARROW: f32 = 14.0;
const ARROW_BOX: f32 = 24.0;
const BADGE: f32 = 46.0;
const BADGE_ICON: f32 = 22.0;
const BUTTON: f32 = 30.0;
const BUTTON_ICON: f32 = 15.0;
const BUTTON_GAP: f32 = 4.0;
const TITLE_GAP: f32 = 12.0;
const TITLE_SIZE: f32 = 14.0;
const SUB_SIZE: f32 = 12.0;
const PAGE_DOT: f32 = 5.0;
const PAGE_DOT_ON: f32 = 14.0;
const PAGE_DOT_GAP: f32 = 5.0;
const PAGE_DOTS_FROM_BOTTOM: f32 = 11.0;
const TRACK_H: f32 = 4.0;
const FIGURE_ICON: f32 = 16.0;
const FIGURE_SIZE: f32 = 17.0;
const FIGURE_LABEL: f32 = 11.0;
const FIGURE_GAP: f32 = 6.0;
const FIGURES: f32 = 4.0;
const LEVEL_ICON: f32 = 17.0;
const LEVEL_VALUE_W: f32 = 30.0;
const INNER_PAD: f32 = 20.0;

/// How far the mouse is dragged, button down, for the page to turn (DIPs).
const SWIPE: f32 = 36.0;
/// One notch of a wheel; a touchpad sends the same in many small steps.
const WHEEL_NOTCH: i32 = 120;
/// How long a wheel has to be still before it can turn the page again: a touchpad's swipe
/// runs on for a moment after the fingers have left.
const WHEEL_PAUSE: Duration = Duration::from_millis(320);

/// Spare room around the largest island in its window, for a spring's overshoot (DIPs).
pub const ROOM: f32 = 12.0;

/// The width and height of the island's window (DIPs): it never changes size, only what
/// is drawn in it, so nothing but the drawing moves.
pub fn window_size() -> (f32, f32) {
    (OPEN.0 + 2.0 * ROOM, OPEN.1 + TOP + 2.0 * ROOM)
}

/// How long a passing event stays (ms).
pub const LEVEL_MS: u32 = 1400;
pub const TRACK_MS: u32 = 2600;
pub const NOTICE_MS: u32 = 3600;
/// How long the mouse may be away before the open island closes (ms).
pub const LEAVE_MS: u32 = 260;

// The size follows an underdamped spring, so it lands with a small overshoot; the contents
// fade on a stiffer, critically damped one.
const SIZE_STIFFNESS: f32 = 420.0;
const SIZE_DAMPING: f32 = 30.0;
const FADE_STIFFNESS: f32 = 2200.0;
const FADE_DAMPING: f32 = 94.0;
/// The contents change once the old ones have faded this far.
const SWAP_AT: f32 = 0.06;

/// What the island shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Idle,
    /// A page of the open island.
    Page(usize),
    Level {
        level: Level,
        percent: u8,
    },
    Track {
        title: String,
    },
    Notice {
        title: String,
        sub: String,
        icon: &'static str,
    },
}

impl Content {
    fn size(&self) -> (f32, f32) {
        match self {
            Content::Idle => IDLE,
            Content::Page(_) => OPEN,
            Content::Level { .. } => LEVEL,
            Content::Track { .. } => TRACK,
            Content::Notice { .. } => NOTICE,
        }
    }

    /// The same kind of thing with another value: it changes in place, without a fade
    /// (a level dragged must follow at once).
    fn same_kind(&self, other: &Content) -> bool {
        matches!(
            (self, other),
            (Content::Level { .. }, Content::Level { .. }) | (Content::Track { .. }, Content::Track { .. })
        )
    }
}

/// The island's state and its motion.
#[derive(Clone, Debug)]
pub struct Island {
    w: Spring,
    h: Spring,
    fade: Spring,
    hover: bool,
    page: usize,
    event: Option<Content>,
    shown: Content,
}

impl Default for Island {
    fn default() -> Self {
        Island::new()
    }
}

impl Island {
    pub fn new() -> Island {
        Island {
            w: Spring::at(IDLE.0, SIZE_STIFFNESS, SIZE_DAMPING),
            h: Spring::at(IDLE.1, SIZE_STIFFNESS, SIZE_DAMPING),
            fade: Spring::at(1.0, FADE_STIFFNESS, FADE_DAMPING),
            hover: false,
            page: 0,
            event: None,
            shown: Content::Idle,
        }
    }

    /// What it should show now: the open panel under the mouse, else a passing event.
    fn wanted(&self) -> Content {
        if self.hover {
            Content::Page(self.page)
        } else {
            self.event.clone().unwrap_or(Content::Idle)
        }
    }

    /// Heads for what it should show now.
    fn retarget(&mut self) {
        let wanted = self.wanted();
        let (w, h) = wanted.size();
        self.w.target = w;
        self.h.target = h;
        if self.shown == wanted {
            self.fade.target = 1.0;
        } else if self.shown.same_kind(&wanted) {
            self.shown = wanted;
            self.fade.target = 1.0;
        } else {
            self.fade.target = 0.0;
        }
    }

    /// The mouse came or went.
    pub fn set_hover(&mut self, hover: bool) {
        self.hover = hover;
        self.retarget();
    }

    pub fn hovered(&self) -> bool {
        self.hover
    }

    /// Turns `by` pages, round the ends.
    pub fn turn(&mut self, by: isize) {
        let n = PAGES as isize;
        self.go((self.page as isize + by).rem_euclid(n) as usize);
    }

    pub fn go(&mut self, page: usize) {
        self.page = page.min(PAGES - 1);
        self.retarget();
    }

    pub fn page(&self) -> usize {
        self.page
    }

    /// The panel shows the figures, so they are worth reading.
    pub fn shows_figures(&self) -> bool {
        self.hover && self.page == SYSTEM_PAGE
    }

    /// A passing event begins (or changes).
    pub fn event(&mut self, content: Content) {
        self.event = Some(content);
        self.retarget();
    }

    /// The passing event's time is up.
    pub fn end_event(&mut self) {
        self.event = None;
        self.retarget();
    }

    /// Moves on by `dt` seconds at this motion level; true while something still moves.
    pub fn step(&mut self, dt: f32, level: MotionLevel) -> bool {
        if level != MotionLevel::Full {
            self.w.snap();
            self.h.snap();
        } else {
            self.w.step(dt);
            self.h.step(dt);
        }
        if level == MotionLevel::Off {
            self.fade.value = 0.0;
        } else {
            self.fade.step(dt);
        }
        let wanted = self.wanted();
        if self.shown != wanted && self.fade.value <= SWAP_AT {
            self.shown = wanted;
            self.fade.target = 1.0;
        }
        if level == MotionLevel::Off {
            self.fade.snap();
            self.fade.value = 1.0;
        }
        !self.at_rest()
    }

    /// Nothing moves and what it shows is what it should.
    pub fn at_rest(&self) -> bool {
        self.w.settled() && self.h.settled() && self.fade.settled() && self.shown == self.wanted()
    }

    /// What is drawn now.
    pub fn shown(&self) -> &Content {
        &self.shown
    }

    /// The contents' opacity, 0-1.
    pub fn fade(&self) -> f32 {
        self.fade.value.clamp(0.0, 1.0)
    }

    /// The pill's width and height now.
    pub fn size(&self) -> (f32, f32) {
        (self.w.value, self.h.value)
    }

    /// The largest the pill is or is heading to: what the window must let through.
    pub fn extent(&self) -> (f32, f32) {
        (self.w.value.max(self.w.target), self.h.value.max(self.h.target))
    }

    /// Where it ends up.
    pub fn target(&self) -> (f32, f32) {
        (self.w.target, self.h.target)
    }
}

/// The words and pictures of the pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {
    pub title: String,
    pub subtitle: String,
    /// The picture of the app in front.
    pub app: Option<u64>,
    pub media: Option<String>,
    pub media_sub: String,
    pub playing: bool,
    /// The figures: value and label, in order.
    pub figures: Vec<(&'static str, String, String)>,
}

fn span(text: impl Into<String>, color: Role, weight: u16) -> Span {
    Span {
        text: text.into(),
        color,
        weight,
        after: 0.0,
        underline: false,
        italic: false,
        spacing: 0.0,
    }
}

fn words(text: impl Into<String>, color: Role, weight: u16, size: f32, max: Option<f32>, font: FontRole) -> Node {
    Node::text(TextSpec {
        spans: vec![span(text, color, weight)],
        font,
        size,
        max_width: max,
    })
}

fn dot(w: f32, h: f32, fill: Role) -> Node {
    Node::layer(Vec::new()).size(Some(w), Some(h)).paint(Paint {
        fill,
        border: None,
        radius: h,
    })
}

fn button(icon: &'static str, module: &'static str, size: f32, icon_size: f32) -> Node {
    Node::row(vec![Node::icon(icon, icon_size, Role::Fg)])
        .align(Align::Center)
        .padding(Insets::all((size - icon_size) / 2.0))
        .paint(Paint {
            fill: Role::SurfaceSel,
            border: None,
            radius: size,
        })
        .hit(module, 0)
}

fn arrow(icon: &'static str, module: &'static str) -> Node {
    Node::row(vec![Node::icon(icon, ARROW, Role::Dim)])
        .align(Align::Center)
        .padding(Insets::all((ARROW_BOX - ARROW) / 2.0))
        .hit(module, 0)
}

fn badge(inner: Node) -> Node {
    Node::row(vec![inner])
        .align(Align::Center)
        .padding(Insets::all((BADGE - BADGE_ICON) / 2.0))
        .paint(Paint {
            fill: Role::SurfaceSel,
            border: Some((Role::Accent, 2.0)),
            radius: BADGE,
        })
}

/// A title over the line under it, as wide as they are, at most `width`.
fn snug_lines(title: &str, sub: &str, width: f32) -> Node {
    let mut kids = vec![words(title, Role::Fg, 600, TITLE_SIZE, Some(width), FontRole::Ui)];
    if !sub.is_empty() {
        kids.push(words(sub, Role::Dim, 400, SUB_SIZE, Some(width), FontRole::Ui));
    }
    Node::column(kids).gap(2.0).align(Align::Start)
}

/// A title over the line under it, cut to `width` and filling it.
fn lines(title: &str, sub: &str, width: f32) -> Node {
    snug_lines(title, sub, width).size(Some(width), None)
}

fn page_dots(page: usize) -> Node {
    Node::row(
        (0..PAGES)
            .map(|i| {
                let on = i == page;
                dot(
                    if on { PAGE_DOT_ON } else { PAGE_DOT },
                    PAGE_DOT,
                    if on { Role::Fg } else { Role::Faint },
                )
                .hit(hit::DOT, u32::try_from(i).unwrap_or(0))
            })
            .collect(),
    )
    .gap(PAGE_DOT_GAP)
    .align(Align::Center)
}

/// Between the arrows of the open island, what a page has.
const fn page_room() -> f32 {
    OPEN.0 - 2.0 * (SIDE_PAD + ARROW_BOX)
}

fn now_page(d: &Data) -> Node {
    let picture = match d.app {
        Some(key) => Node::image(key, BADGE_ICON),
        None => Node::icon("bolt", BADGE_ICON, Role::Accent),
    };
    let buttons: [(&'static str, &'static str); 6] = [
        ("search", hit::LAUNCHER),
        ("app", hit::GRID),
        ("tile", hit::OVERVIEW),
        ("vol", hit::VOLUME),
        ("qs", hit::QUICK),
        ("power", hit::POWER),
    ];
    let count = buttons.len() as f32;
    let width = page_room() - BADGE - 2.0 * TITLE_GAP - count * BUTTON - (count - 1.0) * BUTTON_GAP;
    Node::row(vec![
        badge(picture),
        lines(&d.title, &d.subtitle, width),
        Node::row(
            buttons
                .iter()
                .map(|(icon, module)| button(icon, module, BUTTON, BUTTON_ICON))
                .collect(),
        )
        .gap(BUTTON_GAP)
        .align(Align::Center),
    ])
    .gap(TITLE_GAP)
    .align(Align::Center)
}

fn media_page(d: &Data, nothing: &str) -> Node {
    let controls: [(&'static str, &'static str); 3] = [
        ("prev", hit::TRACK_PREV),
        (if d.playing { "pause" } else { "play" }, hit::TRACK_PLAY),
        ("next", hit::TRACK_NEXT),
    ];
    let count = controls.len() as f32;
    let width = page_room() - BADGE - 2.0 * TITLE_GAP - count * BUTTON - (count - 1.0) * BUTTON_GAP;
    Node::row(vec![
        badge(Node::icon("music", BADGE_ICON, Role::Accent)),
        lines(d.media.as_deref().unwrap_or(nothing), &d.media_sub, width),
        Node::row(
            controls
                .iter()
                .map(|(icon, module)| button(icon, module, BUTTON, BUTTON_ICON))
                .collect(),
        )
        .gap(BUTTON_GAP)
        .align(Align::Center),
    ])
    .gap(TITLE_GAP)
    .align(Align::Center)
}

fn system_page(d: &Data) -> Node {
    Node::row(
        d.figures
            .iter()
            .map(|(icon, value, label)| {
                // A layer centres what is aligned to the centre: each in the middle of its cell.
                Node::layer(vec![Node::row(vec![
                    Node::icon(icon, FIGURE_ICON, Role::Accent),
                    Node::column(vec![
                        words(value.clone(), Role::Fg, 600, FIGURE_SIZE, None, FontRole::Num),
                        words(label.clone(), Role::Dim, 400, FIGURE_LABEL, None, FontRole::Ui),
                    ]),
                ])
                .gap(8.0)
                .align(Align::Center)])
                .size(Some((page_room() - (FIGURES - 1.0) * FIGURE_GAP) / FIGURES), None)
            })
            .collect(),
    )
    .gap(FIGURE_GAP)
    .align(Align::Center)
}

fn level_track(share: f32, width: f32) -> Node {
    let done = (width * share.clamp(0.0, 1.0)).max(TRACK_H);
    Node::layer(vec![Node::layer(Vec::new())
        .size(Some(done), Some(TRACK_H))
        .paint(Paint {
            fill: Role::Accent,
            border: None,
            radius: TRACK_H,
        })
        .offset((done - width) / 2.0, 0.0)])
    .size(Some(width), Some(TRACK_H))
    .paint(Paint {
        fill: Role::Faint,
        border: None,
        radius: TRACK_H,
    })
}

/// Turning the pages with the mouse: a drag with the button down, or a wheel or a
/// touchpad's two fingers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gesture {
    pressed_at: Option<f32>,
    swiped: bool,
    wheel: i32,
    wheel_last: Option<Instant>,
}

impl Gesture {
    pub fn press(&mut self, x: f32) {
        self.pressed_at = Some(x);
        self.swiped = false;
    }

    /// The mouse is at `x`: the pages to turn, once a drag (a drag to the left goes on to
    /// the next page, as a finger would).
    pub fn moved(&mut self, x: f32) -> Option<isize> {
        let from = self.pressed_at?;
        if self.swiped || (x - from).abs() < SWIPE {
            return None;
        }
        self.swiped = true;
        Some(if x < from { 1 } else { -1 })
    }

    /// The button came up (or the mouse left): true if it was a swipe, not a click.
    pub fn release(&mut self) -> bool {
        self.pressed_at = None;
        std::mem::take(&mut self.swiped)
    }

    /// The wheel moved by `amount` (positive: on to the next page): the pages to turn.
    pub fn wheeled(&mut self, amount: i32, now: Instant) -> Option<isize> {
        if self.wheel_last.is_some_and(|t| now.duration_since(t) < WHEEL_PAUSE) {
            self.wheel_last = Some(now);
            self.wheel = 0;
            return None;
        }
        self.wheel += amount;
        if self.wheel.abs() < WHEEL_NOTCH {
            return None;
        }
        let by = self.wheel.signum() as isize;
        self.wheel = 0;
        self.wheel_last = Some(now);
        Some(by)
    }
}

/// The contents of `content`, the size of its pill.
pub fn view(content: &Content, page: usize, d: &Data, nothing_plays: &str) -> Node {
    let (w, h) = content.size();
    let inner = match content {
        Content::Idle => Node::row(vec![
            dot(DOT_SIZE, DOT_SIZE, Role::White),
            dot(DOT_SIZE, DOT_SIZE, Role::White),
        ])
        .gap(DOTS_GAP)
        .align(Align::Center),
        Content::Page(p) => {
            let body = match *p {
                0 => now_page(d),
                1 => media_page(d, nothing_plays),
                _ => system_page(d),
            };
            let row = Node::row(vec![
                arrow("left", hit::PREV),
                Node::row(vec![body]).align(Align::Center).size(Some(page_room()), None),
                arrow("right", hit::NEXT),
            ])
            .align(Align::Center);
            return Node::layer(vec![
                // A layer centres what is aligned to the centre: both move from there.
                Node::layer(vec![row])
                    .size(Some(w), Some(h - PAGE_DOTS_FROM_BOTTOM))
                    .offset(0.0, -PAGE_DOTS_FROM_BOTTOM / 2.0),
                page_dots(page).offset(0.0, h / 2.0 - PAGE_DOTS_FROM_BOTTOM),
            ])
            .size(Some(w), Some(h));
        }
        Content::Level { level, percent } => Node::row(vec![
            Node::icon(level.icon(), LEVEL_ICON, Role::Fg),
            level_track(
                f32::from(*percent) / 100.0,
                w - 2.0 * INNER_PAD - LEVEL_ICON - LEVEL_VALUE_W - 2.0 * TITLE_GAP,
            ),
            // At the cell's end, so one digit or three end level with the pill's padding.
            Node::column(vec![words(
                percent.to_string(),
                Role::Fg,
                600,
                SUB_SIZE,
                None,
                FontRole::Num,
            )])
            .align(Align::End)
            .size(Some(LEVEL_VALUE_W), None),
        ])
        .gap(TITLE_GAP)
        .align(Align::Center),
        Content::Track { title } => Node::row(vec![
            Node::icon("music", LEVEL_ICON, Role::Accent),
            words(
                title.clone(),
                Role::Fg,
                600,
                TITLE_SIZE,
                Some(w - 2.0 * INNER_PAD - LEVEL_ICON - TITLE_GAP),
                FontRole::Ui,
            ),
        ])
        .gap(TITLE_GAP)
        .align(Align::Center),
        Content::Notice { title, sub, icon } => Node::row(vec![
            Node::icon(icon, BADGE_ICON, Role::Accent),
            snug_lines(title, sub, w - 2.0 * INNER_PAD - BADGE_ICON - TITLE_GAP),
        ])
        .gap(TITLE_GAP)
        .align(Align::Center),
    };
    Node::layer(vec![inner]).size(Some(w), Some(h))
}

/// The island drawn at its size now: the black pill centred in a window `window` wide,
/// and those parts of the contents the pill covers, so nothing shows outside it while it
/// grows or shrinks.
pub fn compose(island: &Island, content: &Node, window: f32, m: &dyn TextMeasure) -> Layout {
    let (w, h) = island.size();
    let pill = Rect::new((window - w) / 2.0, TOP, w, h);
    let mut out = Layout {
        size: kabuk_ui::geom::Size { w: window, h: TOP + h },
        draws: vec![Draw::Panel {
            rect: pill,
            paint: Paint {
                fill: Role::Black,
                border: None,
                radius: (h / 2.0).min(CORNER),
            },
        }],
        hits: Vec::new(),
    };
    let (tw, th) = island.shown().size();
    let inner = layout(
        content,
        Point {
            x: (window - tw) / 2.0,
            y: TOP + (h - th) / 2.0,
        },
        m,
    );
    let covered = |r: &Rect| {
        r.x >= pill.x && r.right() <= pill.right() && r.y >= pill.y - 0.5 && r.bottom() <= pill.bottom() + 0.5
    };
    out.draws.extend(inner.draws.into_iter().filter(|d| match d {
        Draw::Panel { rect, .. } | Draw::Text { rect, .. } | Draw::Icon { rect, .. } | Draw::Image { rect, .. } => {
            covered(rect)
        }
    }));
    out.hits = inner.hits.into_iter().filter(|(r, _)| covered(r)).collect();
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: at rest it is two dots and asks for no frames; the mouse opens it and
    //! leaving closes it; a level changing keeps its place without a fade; an event under
    //! the mouse waits; the pages go round; animations off land at once; while it grows
    //! nothing shows outside the pill; only the figures' page wants the figures.
    use super::*;
    use kabuk_ui::node::TextMetrics;

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: (chars as f32 * t.size * 0.5).min(t.max_width.unwrap_or(f32::MAX)),
                height: t.size * 1.3,
                baseline: t.size,
            }
        }
    }

    const FRAME: f32 = 1.0 / 60.0;

    fn settle(i: &mut Island) -> usize {
        let mut frames = 0;
        while i.step(FRAME, MotionLevel::Full) {
            frames += 1;
            assert!(frames < 600, "it never came to rest");
        }
        frames
    }

    fn data() -> Data {
        Data {
            title: "kabuk".into(),
            subtitle: "Workspace 2".into(),
            figures: vec![
                ("cpu", "12%".into(), "CPU".into()),
                ("mem", "9.0G".into(), "RAM".into()),
            ],
            ..Data::default()
        }
    }

    #[test]
    fn at_rest_it_is_small_and_wants_no_frames() {
        let i = Island::new();
        assert!(i.at_rest());
        assert_eq!(i.size(), IDLE);
        assert_eq!(i.shown(), &Content::Idle);
    }

    #[test]
    fn the_mouse_opens_it_and_leaving_closes_it() {
        let mut i = Island::new();
        i.set_hover(true);
        assert!(!i.at_rest());
        let frames = settle(&mut i);
        assert!(frames > 5, "it jumped: {frames}");
        assert_eq!(i.shown(), &Content::Page(0));
        assert!((i.size().0 - OPEN.0).abs() < 0.5);
        i.set_hover(false);
        settle(&mut i);
        assert_eq!(i.shown(), &Content::Idle);
        assert!((i.size().0 - IDLE.0).abs() < 0.5);
    }

    #[test]
    fn it_overshoots_a_little_on_its_way_open() {
        let mut i = Island::new();
        i.set_hover(true);
        let mut most = 0.0f32;
        while i.step(FRAME, MotionLevel::Full) {
            most = most.max(i.size().0);
        }
        assert!(most > OPEN.0, "no spring: {most}");
        assert!(most < OPEN.0 + 2.0 * ROOM, "past its window: {most}");
    }

    #[test]
    fn a_level_changing_stays_where_it_is() {
        let mut i = Island::new();
        i.event(Content::Level {
            level: Level::Volume,
            percent: 40,
        });
        settle(&mut i);
        i.event(Content::Level {
            level: Level::Volume,
            percent: 45,
        });
        assert_eq!(
            i.shown(),
            &Content::Level {
                level: Level::Volume,
                percent: 45
            }
        );
        assert_eq!(i.fade(), 1.0, "it faded");
    }

    #[test]
    fn an_event_under_the_mouse_waits_and_one_that_ended_goes() {
        let mut i = Island::new();
        i.set_hover(true);
        settle(&mut i);
        i.event(Content::Track { title: "Song".into() });
        settle(&mut i);
        assert_eq!(i.shown(), &Content::Page(0));
        i.set_hover(false);
        settle(&mut i);
        assert_eq!(i.shown(), &Content::Track { title: "Song".into() });
        i.end_event();
        settle(&mut i);
        assert_eq!(i.shown(), &Content::Idle);
    }

    #[test]
    fn the_pages_go_round_and_only_one_wants_the_figures() {
        let mut i = Island::new();
        assert!(!i.shows_figures());
        i.set_hover(true);
        i.turn(-1);
        assert_eq!(i.page(), PAGES - 1);
        assert_eq!(PAGES - 1, SYSTEM_PAGE);
        assert!(i.shows_figures());
        i.turn(1);
        assert_eq!(i.page(), 0);
        assert!(!i.shows_figures());
        i.go(99);
        assert_eq!(i.page(), PAGES - 1);
    }

    #[test]
    fn with_animations_off_it_lands_at_once() {
        let mut i = Island::new();
        i.set_hover(true);
        assert!(!i.step(FRAME, MotionLevel::Off));
        assert_eq!(i.size(), OPEN);
        assert_eq!(i.shown(), &Content::Page(0));
        assert_eq!(i.fade(), 1.0);
    }

    #[test]
    fn reduced_motion_only_fades() {
        let mut i = Island::new();
        i.set_hover(true);
        i.step(FRAME, MotionLevel::Reduced);
        assert_eq!(i.size(), OPEN, "it moved");
    }

    #[test]
    fn nothing_shows_outside_the_pill_while_it_grows() {
        let mut i = Island::new();
        i.set_hover(true);
        let window = window_size().0;
        let mut seen = 0;
        for _ in 0..40 {
            i.step(FRAME, MotionLevel::Full);
            let node = view(i.shown(), i.page(), &data(), "-");
            let l = compose(&i, &node, window, &Fake);
            let Draw::Panel { rect: pill, .. } = l.draws[0] else {
                panic!()
            };
            for d in &l.draws[1..] {
                seen += 1;
                let r = match d {
                    Draw::Panel { rect, .. }
                    | Draw::Text { rect, .. }
                    | Draw::Icon { rect, .. }
                    | Draw::Image { rect, .. } => rect,
                };
                assert!(r.x >= pill.x && r.right() <= pill.right(), "{r:?} outside {pill:?}");
            }
        }
        assert!(seen > 0, "nothing was ever drawn in it");
    }

    #[test]
    fn a_drag_turns_one_page_and_is_not_a_click() {
        let mut g = Gesture::default();
        assert_eq!(g.moved(0.0), None, "no button, no swipe");
        g.press(300.0);
        assert_eq!(g.moved(300.0 - SWIPE + 1.0), None, "not far enough");
        assert_eq!(g.moved(300.0 - SWIPE), Some(1), "to the left: the next page");
        assert_eq!(g.moved(100.0), None, "once a drag");
        assert!(g.release());
        g.press(300.0);
        assert_eq!(g.moved(300.0 + SWIPE), Some(-1));
        assert!(g.release());
        g.press(300.0);
        assert!(!g.release(), "a press and a release are a click");
    }

    #[test]
    fn a_wheel_turns_by_the_notch_and_a_touchpad_once() {
        let t = Instant::now();
        let later = |ms| t + Duration::from_millis(ms);
        let mut g = Gesture::default();
        assert_eq!(g.wheeled(WHEEL_NOTCH, t), Some(1));
        assert_eq!(g.wheeled(WHEEL_NOTCH, later(10)), None, "still running on");
        assert_eq!(g.wheeled(-WHEEL_NOTCH, later(1000)), Some(-1));
        // A touchpad: small steps add up, and what runs on after the page has turned is lost.
        let mut g = Gesture::default();
        let mut turns = Vec::new();
        for i in 0..40 {
            turns.extend(g.wheeled(20, later(i * 10)));
        }
        assert_eq!(turns, vec![1]);
    }

    /// How far off the pill's middle the contents' middle is.
    fn off_middle(l: &Layout, keep: &dyn Fn(&Rect) -> bool) -> f32 {
        let Draw::Panel { rect: pill, .. } = l.draws[0] else {
            panic!("the pill comes first")
        };
        let rects: Vec<Rect> = l.draws[1..]
            .iter()
            .map(|d| match d {
                Draw::Panel { rect, .. }
                | Draw::Text { rect, .. }
                | Draw::Icon { rect, .. }
                | Draw::Image { rect, .. } => *rect,
            })
            .filter(|r| keep(r))
            .collect();
        let from = rects.iter().map(|r| r.x).fold(f32::MAX, f32::min);
        let to = rects.iter().map(|r| r.x + r.w).fold(f32::MIN, f32::max);
        (from + to) / 2.0 - (pill.x + pill.w / 2.0)
    }

    /// Found by the user: the island's contents were not in its middle.
    #[test]
    fn its_contents_stand_in_the_middle_of_the_pill() {
        let window = window_size().0;
        let d = Data {
            figures: (0..4).map(|_| ("cpu", "12%".to_string(), "CPU".to_string())).collect(),
            ..data()
        };
        let drawn = |i: &Island| compose(i, &view(i.shown(), i.page(), &d, "-"), window, &Fake);
        let idle = Island::new();
        assert!(off_middle(&drawn(&idle), &|_| true).abs() < 1.0);
        for p in 0..PAGES {
            let mut i = Island::new();
            i.go(p);
            i.set_hover(true);
            settle(&mut i);
            let l = drawn(&i);
            assert!(
                off_middle(&l, &|_| true).abs() < 1.0,
                "page {p}: {}",
                off_middle(&l, &|_| true)
            );
            // Between the arrows too.
            let side = (window - OPEN.0) / 2.0 + SIDE_PAD + ARROW_BOX;
            let between = |r: &Rect| r.x >= side && r.x + r.w <= window - side;
            assert!(
                off_middle(&l, &between).abs() < 8.0,
                "page {p}: {}",
                off_middle(&l, &between)
            );
        }
        for content in [
            Content::Track { title: "Short".into() },
            Content::Notice {
                title: "Short".into(),
                sub: "Line".into(),
                icon: "tile",
            },
            Content::Level {
                level: Level::Volume,
                percent: 50,
            },
        ] {
            let mut i = Island::new();
            i.event(content);
            settle(&mut i);
            let l = drawn(&i);
            assert!(
                off_middle(&l, &|_| true).abs() < 1.0,
                "{:?}: {}",
                i.shown(),
                off_middle(&l, &|_| true)
            );
        }
    }

    #[test]
    fn every_page_fits_its_panel_and_offers_its_controls() {
        let window = window_size().0;
        for p in 0..PAGES {
            let mut i = Island::new();
            i.go(p);
            i.set_hover(true);
            settle(&mut i);
            let long = Data {
                title: "A very long window title that goes on and on and on and on and on".into(),
                ..data()
            };
            let l = compose(&i, &view(i.shown(), i.page(), &long, "-"), window, &Fake);
            let modules: Vec<&str> = l.hits.iter().map(|(_, h)| h.module).collect();
            assert!(
                modules.contains(&hit::PREV) && modules.contains(&hit::NEXT),
                "page {p}: {modules:?}"
            );
            assert_eq!(modules.iter().filter(|m| **m == hit::DOT).count(), PAGES, "page {p}");
            if p == 0 {
                assert!(
                    modules.contains(&hit::LAUNCHER) && modules.contains(&hit::POWER),
                    "{modules:?}"
                );
            }
        }
    }

    #[test]
    fn the_idle_island_is_two_dots() {
        let i = Island::new();
        let l = compose(&i, &view(i.shown(), 0, &data(), "-"), window_size().0, &Fake);
        assert_eq!(l.draws.iter().filter(|d| matches!(d, Draw::Panel { .. })).count(), 3);
        assert!(l.hits.is_empty());
    }
}
