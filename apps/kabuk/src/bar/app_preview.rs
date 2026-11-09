//! Previews of an app's windows over the bar (asked for by the user: as Windows' taskbar
//! shows them on hover): one card per window with its title, a × that closes it and a
//! live picture of it, side by side, in the tray panel's surface (D21, D26). The pictures are DWM thumbnails
//! the host lays over the empty frames this tree leaves for them. Pure.

use std::time::{Duration, Instant};

use kabuk_core::config::Edge;
use kabuk_ui::geom::Insets;
use kabuk_ui::motion::ease_out;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};
use kabuk_win::msgloop::PxRect;

use super::tooltip::TipStyle;

/// Hit-test id of a card; the value is its index in the windows given to [`view`].
pub const HIT: &str = "app-preview";
/// Hit-test id of a card's picture frame (where its thumbnail goes); same values.
pub const FRAME: &str = "app-preview-frame";
/// Hit-test id of a card's ×, which closes its window; same values.
pub const CLOSE: &str = "app-preview-close";
/// The × drawn as text (the design's icon set has no close icon), as Windows' previews.
const CLOSE_MARK: &str = "×";

/// Sizes in DIPs, and how long the previews wait for the mouse that left the app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewStyle {
    /// A picture's height; its width follows the window's shape within `min_w`-`max_w`.
    pub frame_h: f32,
    pub min_w: f32,
    pub max_w: f32,
    /// Between cards, and between a card's title and its picture.
    pub gap: f32,
    pub title_gap: f32,
    /// Around a card's contents, and the frame's rounding.
    pub card_pad: f32,
    /// The ×'s square, and its text size against the title's.
    pub close: f32,
    pub close_text: f32,
    /// How much larger the × gets under the mouse, in how many seconds, and in how many
    /// even steps (a new text size each frame would be a new font each frame).
    pub close_grown: f32,
    pub close_seconds: f32,
    pub close_steps: f32,
    /// After the cards changed, a click on a × waits this long: the next window's × has
    /// just come under the mouse, and a double click must not close it too.
    pub close_settle: Duration,
    /// How often the windows being closed are looked for, and for how long (a program
    /// asking whether to save stays open as long as it likes; its card stays).
    pub close_poll: Duration,
    pub close_wait: Duration,
    pub frame_radius: f32,
    /// The most windows previewed (Windows' taskbar turns to a list beyond its width).
    pub max_windows: usize,
    pub grace: Duration,
}

pub const PREVIEW: PreviewStyle = PreviewStyle {
    frame_h: 120.0,
    min_w: 90.0,
    max_w: 220.0,
    gap: 6.0,
    title_gap: 6.0,
    card_pad: 6.0,
    close: 18.0,
    close_text: 1.3,
    close_grown: 1.4,
    close_seconds: 0.12,
    close_steps: 8.0,
    close_settle: Duration::from_millis(250),
    close_poll: Duration::from_millis(50),
    close_wait: Duration::from_secs(10),
    frame_radius: 4.0,
    max_windows: 6,
    grace: Duration::from_millis(300),
};

/// A window as its card needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shot {
    pub title: String,
    /// The window's size (pixels; its restored size when it is minimised).
    pub size: (i32, i32),
    /// The workspace it is on, named (PLAN M19 U20), under its title; `None` when the
    /// app's windows are all on one (see [`on_several_workspaces`]).
    pub workspace: Option<String>,
}

/// Whether an app's windows (each one's workspace, `None` for a window the window manager
/// does not hold) are on more than one workspace: only then do the cards say which.
pub fn on_several_workspaces(workspaces: &[Option<u32>]) -> bool {
    let mut known = workspaces.iter().flatten();
    known.next().is_some_and(|first| known.any(|other| other != first))
}

/// The frame for a window of `size`: `frame_h` high and as wide as its shape, within
/// the width limits (a very wide window gets a lower frame instead).
pub fn frame_size(size: (i32, i32), s: &PreviewStyle) -> (f32, f32) {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let wide = s.frame_h * w / h;
    if wide > s.max_w {
        (s.max_w, (s.max_w * h / w).max(1.0))
    } else {
        (wide.max(s.min_w), s.frame_h)
    }
}

/// Where a picture of `size` goes inside a frame at `frame` (x, y, w, h): as large as
/// fits, its shape kept, centred.
pub fn fit(size: (i32, i32), frame: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let (x, y, fw, fh) = frame;
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let k = (fw / w).min(fh / h);
    let (pw, ph) = (w * k, h * k);
    (x + (fw - pw) / 2.0, y + (fh - ph) / 2.0, pw, ph)
}

/// One ×'s growth: on its way from `from` to `to` (0 at rest, 1 full) since `since`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mark {
    from: f32,
    to: f32,
    since: Instant,
}

impl Mark {
    fn at(&self, now: Instant, seconds: f32) -> f32 {
        let u = if seconds > 0.0 {
            (now.saturating_duration_since(self.since).as_secs_f32() / seconds).min(1.0)
        } else {
            1.0
        };
        self.from + (self.to - self.from) * ease_out(0.0, 1.0, 1.0).at(u)
    }
}

/// Each card's ×: the one under the mouse grows, the others go back from where they are.
#[derive(Clone, Debug, PartialEq)]
pub struct CloseMarks {
    marks: Vec<Mark>,
    over: Option<usize>,
}

impl CloseMarks {
    pub fn new(cards: usize, now: Instant) -> CloseMarks {
        CloseMarks {
            marks: vec![
                Mark {
                    from: 0.0,
                    to: 0.0,
                    since: now,
                };
                cards
            ],
            over: None,
        }
    }

    /// The mouse is over card `over`'s × (or none); true when that is news.
    pub fn point(&mut self, over: Option<usize>, now: Instant, seconds: f32) -> bool {
        if over == self.over {
            return false;
        }
        self.over = over;
        for (i, m) in self.marks.iter_mut().enumerate() {
            let to = if over == Some(i) { 1.0 } else { 0.0 };
            if m.to != to {
                *m = Mark {
                    from: m.at(now, seconds),
                    to,
                    since: now,
                };
            }
        }
        true
    }

    /// How far each has grown, in `close_steps` even steps.
    pub fn grown(&self, now: Instant, s: &PreviewStyle) -> Vec<f32> {
        self.marks
            .iter()
            .map(|m| (m.at(now, s.close_seconds) * s.close_steps).round() / s.close_steps)
            .collect()
    }

    pub fn settled(&self, now: Instant, seconds: f32) -> bool {
        self.marks
            .iter()
            .all(|m| now.saturating_duration_since(m.since).as_secs_f32() >= seconds)
    }
}

/// Whether a click on a × counts `since` the cards last changed (`None`: they have not).
pub fn close_click_counts(since: Option<Instant>, now: Instant, s: &PreviewStyle) -> bool {
    since.is_none_or(|t| now.saturating_duration_since(t) >= s.close_settle)
}

/// The cards once the app's windows are `live`: those still there stay in their order (a
/// closing window sends another to the front, and the cards must not swap under the
/// mouse), and any new one follows, up to `most`.
pub fn cards_after(shown: &[u64], live: &[u64], most: usize) -> Vec<u64> {
    shown
        .iter()
        .filter(|h| live.contains(h))
        .chain(live.iter().filter(|h| !shown.contains(h)))
        .copied()
        .take(most)
        .collect()
}

/// `placed` moved along the bar to `x`, so the next × is where the mouse is: never over
/// the bar it stands beside, and inside `monitor`.
pub fn at_the_mouse(edge: Edge, placed: PxRect, x: i32, monitor: PxRect) -> PxRect {
    let x = match edge {
        Edge::Left => x.max(placed.x),
        Edge::Right => x.min(placed.x),
        Edge::Top | Edge::Bottom => x,
    };
    PxRect {
        x: x.max(monitor.x).min((monitor.x + monitor.w - placed.w).max(monitor.x)),
        ..placed
    }
}

/// `r` (in a panel of `panel` pixels) as it is when the panel is scaled by `k` about its
/// centre, so the live pictures keep to the panel's own entrance.
pub fn scaled_in_panel(r: PxRect, panel: (i32, i32), k: f32) -> PxRect {
    let along = |at: i32, side: i32| side as f32 / 2.0 + (at as f32 - side as f32 / 2.0) * k;
    PxRect {
        x: along(r.x, panel.0).round() as i32,
        y: along(r.y, panel.1).round() as i32,
        w: (r.w as f32 * k).round() as i32,
        h: (r.h as f32 * k).round() as i32,
    }
}

/// The previews' tree: a card per window (at most `max_windows`), each clickable, each ×
/// as far grown as `grown` says (0 where it says nothing).
pub fn view(text_scale: f32, tip: &TipStyle, s: &PreviewStyle, shots: &[Shot], grown: &[f32]) -> Node {
    let cards = shots
        .iter()
        .take(s.max_windows)
        .enumerate()
        .map(|(i, shot)| {
            let (w, h) = frame_size(shot.size, s);
            let index = u32::try_from(i).unwrap_or(u32::MAX);
            let line = |text: &str, color: Role, size: f32, max_width: Option<f32>| {
                Node::text(TextSpec {
                    spans: vec![Span {
                        text: text.to_string(),
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
            };
            let title_w = (w - s.close - s.title_gap).max(0.0);
            let title = Node::row(vec![line(&shot.title, Role::Fg, tip.text * text_scale, Some(title_w))])
                .size(Some(title_w), None);
            let close_size = tip.text
                * s.close_text
                * text_scale
                * (1.0 + (s.close_grown - 1.0) * grown.get(i).copied().unwrap_or(0.0));
            let close = line(CLOSE_MARK, Role::Dim, close_size, None)
                .size(Some(s.close), Some(s.close))
                .hit(CLOSE, index);
            let title = Node::row(vec![title, close]).gap(s.title_gap).align(Align::Center);
            // The workspace under the title, dimmed, as wide as the picture.
            let workspace = shot
                .workspace
                .as_ref()
                .map(|ws| Node::row(vec![line(ws, Role::Dim, tip.text * text_scale, Some(w))]).size(Some(w), None));
            let frame = Node::spacer(w, h)
                .paint(Paint {
                    fill: Role::SurfaceAlt,
                    border: None,
                    radius: s.frame_radius,
                })
                .hit(FRAME, index);
            Node::column(
                std::iter::once(title)
                    .chain(workspace)
                    .chain(std::iter::once(frame))
                    .collect(),
            )
            .gap(s.title_gap)
            .align(Align::Start)
            .padding(Insets::all(s.card_pad))
            .hit(HIT, index)
        })
        .collect();
    Node::row(cards)
        .gap(s.gap)
        .align(Align::Start)
        .padding(Insets::all(tip.pad_v + tip.border))
        .paint(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, tip.border)),
            radius: tip.radius,
        })
}

#[cfg(test)]
mod tests {
    //! Edge cases: a tall, a wide, a very wide and a zero-sized window; pictures fitted
    //! both ways; more windows than shown; every card clickable with its frame inside it;
    //! a × growing, interrupted and going back; a window gone from the middle, from the
    //! end and all of them; the panel kept at the mouse at every edge of the monitor.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: (chars as f32 * 6.0).min(t.max_width.unwrap_or(f32::MAX)),
                height: 16.0,
                baseline: 12.0,
            }
        }
    }

    #[test]
    fn frames_follow_the_windows_shape_within_limits() {
        let s = PREVIEW;
        // 16:9 at 120 high is 213 wide.
        let (w, h) = frame_size((1920, 1080), &s);
        assert!((w - 213.333).abs() < 0.01 && h == s.frame_h, "{w} {h}");
        // A tall window is no narrower than the least width.
        assert_eq!(frame_size((400, 1600), &s), (s.min_w, s.frame_h));
        // A very wide one gets a lower frame at the widest width.
        let (w, h) = frame_size((4000, 1000), &s);
        assert_eq!(w, s.max_w);
        assert!((h - 55.0).abs() < 0.01, "{h}");
        // Nothing to measure: no division by zero.
        let (w, h) = frame_size((0, 0), &s);
        assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0);
    }

    #[test]
    fn pictures_fit_their_frame_centred() {
        assert_eq!(fit((200, 100), (10.0, 20.0, 100.0, 100.0)), (10.0, 45.0, 100.0, 50.0));
        assert_eq!(fit((100, 200), (0.0, 0.0, 100.0, 100.0)), (25.0, 0.0, 50.0, 100.0));
        let (_, _, w, h) = fit((0, 0), (0.0, 0.0, 100.0, 50.0));
        assert!(w.is_finite() && h.is_finite());
    }

    #[test]
    fn every_card_has_a_close_mark_inside_its_title_line() {
        let shots = [Shot {
            title: "A long window title that has to be cut".into(),
            size: (1600, 900),
            workspace: None,
        }];
        let l = layout(&view(1.0, &TIP, &PREVIEW, &shots, &[]), Point::default(), &Fake);
        let of = |m: &str| {
            l.hits
                .iter()
                .filter(|(_, h)| h.module == m)
                .map(|(r, _)| *r)
                .collect::<Vec<_>>()
        };
        let (close, frame) = (of(CLOSE), of(FRAME));
        assert_eq!(close.len(), 1);
        // Above the picture, at its right edge.
        assert!(close[0].bottom() <= frame[0].y);
        assert!((close[0].right() - frame[0].right()).abs() < 1.0, "{close:?} {frame:?}");
    }

    #[test]
    fn every_card_is_clickable_with_its_frame_inside() {
        let shots: Vec<Shot> = (0..9)
            .map(|i| Shot {
                title: format!("Window {i} with a long title that is cut"),
                size: (1600, 900),
                workspace: None,
            })
            .collect();
        let l = layout(&view(1.0, &TIP, &PREVIEW, &shots, &[]), Point::default(), &Fake);
        let of = |m: &str| -> Vec<_> { l.hits.iter().filter(|(_, h)| h.module == m).cloned().collect() };
        let (cards, frames) = (of(HIT), of(FRAME));
        assert_eq!(cards.len(), PREVIEW.max_windows);
        assert_eq!(frames.len(), PREVIEW.max_windows);
        for ((card, c), (frame, f)) in cards.iter().zip(&frames) {
            assert_eq!(c.value, f.value);
            assert!(frame.x >= card.x && frame.x + frame.w <= card.x + card.w + 0.01);
            assert!(frame.y > card.y && frame.y + frame.h <= card.y + card.h + 0.01);
        }
        // Side by side, in order.
        assert!(cards.windows(2).all(|p| p[0].0.x < p[1].0.x));
        let empty = layout(&view(1.0, &TIP, &PREVIEW, &[], &[]), Point::default(), &Fake);
        assert!(empty.hits.is_empty());
    }

    /// Asked for by the user: the × under the mouse grows, about its own centre, and
    /// nothing else on the card moves.
    #[test]
    fn a_grown_close_mark_is_larger_and_moves_nothing_else() {
        let shots = [Shot {
            title: "Notes".into(),
            size: (1600, 900),
            workspace: None,
        }];
        let at = |grown: &[f32]| layout(&view(1.0, &TIP, &PREVIEW, &shots, grown), Point::default(), &Fake);
        let mark = |l: &kabuk_ui::node::Layout| {
            l.draws
                .iter()
                .find_map(|d| match d {
                    kabuk_ui::node::Draw::Text { spec, rect } if spec.spans[0].text == CLOSE_MARK => {
                        Some((spec.size, *rect))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let (rest, full) = (at(&[]), at(&[1.0]));
        let ((small, a), (large, b)) = (mark(&rest), mark(&full));
        assert!((large - small * PREVIEW.close_grown).abs() < 0.01, "{small} {large}");
        assert!(((a.x + a.w / 2.0) - (b.x + b.w / 2.0)).abs() < 0.01, "{a:?} {b:?}");
        assert!(((a.y + a.h / 2.0) - (b.y + b.h / 2.0)).abs() < 0.01, "{a:?} {b:?}");
        assert_eq!(rest.size, full.size);
        assert_eq!(rest.hits, full.hits);
    }

    #[test]
    fn a_close_mark_grows_under_the_mouse_and_goes_back_from_where_it_is() {
        let s = PREVIEW;
        let start = Instant::now();
        let later = |seconds: f32| start + Duration::from_secs_f32(seconds);
        let mut m = CloseMarks::new(2, start);
        assert_eq!(m.grown(start, &s), [0.0, 0.0]);
        assert!(m.settled(start, 0.0));

        assert!(m.point(Some(1), start, s.close_seconds));
        // The same news twice is none.
        assert!(!m.point(Some(1), start, s.close_seconds));
        assert_eq!(m.grown(start, &s), [0.0, 0.0]);
        let part = m.grown(later(s.close_seconds / 3.0), &s)[1];
        assert!(part > 0.0 && part < 1.0, "{part}");
        assert!(!m.settled(later(s.close_seconds / 3.0), s.close_seconds));
        assert_eq!(m.grown(later(s.close_seconds), &s), [0.0, 1.0]);
        assert!(m.settled(later(s.close_seconds), s.close_seconds));

        // The mouse goes on to the other × part of the way: the first starts back from there.
        let part_way = later(s.close_seconds / 3.0);
        let mut m = CloseMarks::new(2, start);
        m.point(Some(1), start, s.close_seconds);
        m.point(Some(0), part_way, s.close_seconds);
        assert_eq!(m.grown(part_way, &s)[1], part);
        assert_eq!(
            m.grown(part_way + Duration::from_secs_f32(s.close_seconds), &s),
            [1.0, 0.0]
        );

        // Only so many sizes are ever asked of the fonts.
        let mut m = CloseMarks::new(1, start);
        m.point(Some(0), start, s.close_seconds);
        for i in 0..100 {
            let g = m.grown(later(s.close_seconds * i as f32 / 100.0), &s)[0];
            assert_eq!((g * s.close_steps).fract(), 0.0, "{g}");
        }

        // No time at all: there at once.
        let mut m = CloseMarks::new(1, start);
        m.point(Some(0), start, 0.0);
        let no_time = PreviewStyle {
            close_seconds: 0.0,
            ..s
        };
        assert_eq!(m.grown(start, &no_time), [1.0]);
    }

    #[test]
    fn a_double_click_on_a_close_mark_closes_one_window() {
        let s = PREVIEW;
        let now = Instant::now();
        assert!(close_click_counts(None, now, &s));
        assert!(!close_click_counts(Some(now), now, &s));
        assert!(!close_click_counts(Some(now), now + s.close_settle / 2, &s));
        assert!(close_click_counts(Some(now), now + s.close_settle, &s));
    }

    #[test]
    fn the_cards_keep_their_order_as_windows_go_and_come() {
        // Gone from the middle; the rest stay, whatever order they are in now.
        assert_eq!(cards_after(&[1, 2, 3], &[3, 1], 6), [1, 3]);
        assert_eq!(cards_after(&[1, 2, 3], &[3, 2, 1], 6), [1, 2, 3]);
        // One that did not fit takes the free place.
        assert_eq!(cards_after(&[1, 2], &[2, 9, 8], 2), [2, 9]);
        assert_eq!(cards_after(&[1, 2, 3], &[], 6), Vec::<u64>::new());
    }

    #[test]
    fn the_panel_keeps_the_next_close_mark_at_the_mouse() {
        let monitor = PxRect {
            x: 0,
            y: 0,
            w: 1000,
            h: 800,
        };
        let placed = PxRect {
            x: 400,
            y: 700,
            w: 200,
            h: 50,
        };
        let x_for = |edge, x| at_the_mouse(edge, placed, x, monitor).x;
        // Along a bar at the top or bottom it goes where it is asked, inside the monitor.
        assert_eq!(x_for(Edge::Bottom, 300), 300);
        assert_eq!(x_for(Edge::Top, 500), 500);
        assert_eq!(x_for(Edge::Bottom, -40), 0);
        assert_eq!(x_for(Edge::Bottom, 950), 800);
        // Beside a bar at the side it may leave the bar but never cover it.
        assert_eq!(x_for(Edge::Left, 300), 400);
        assert_eq!(x_for(Edge::Left, 500), 500);
        assert_eq!(x_for(Edge::Right, 500), 400);
        assert_eq!(x_for(Edge::Right, 300), 300);
        // Its height and place across the bar are the placement's.
        let moved = at_the_mouse(Edge::Bottom, placed, 300, monitor);
        assert_eq!((moved.y, moved.w, moved.h), (placed.y, placed.w, placed.h));
        // A monitor narrower than the panel, at negative x (Monitor 2).
        let small = PxRect {
            x: -500,
            y: 0,
            w: 100,
            h: 800,
        };
        assert_eq!(at_the_mouse(Edge::Bottom, placed, 300, small).x, -500);
    }

    #[test]
    fn a_picture_scales_with_its_panel_about_the_panels_centre() {
        let r = PxRect {
            x: 0,
            y: 0,
            w: 100,
            h: 40,
        };
        // At rest, in place.
        assert_eq!(scaled_in_panel(r, (100, 40), 1.0), r);
        let half = scaled_in_panel(r, (100, 40), 0.5);
        assert_eq!((half.x, half.y, half.w, half.h), (25, 10, 50, 20));
    }

    /// PLAN M19 U20: the cards say which workspace each window is on only when the app's
    /// windows are on more than one; a window the window manager does not hold counts for
    /// none.
    #[test]
    fn the_workspace_is_said_when_an_apps_windows_are_on_several() {
        assert!(!on_several_workspaces(&[]));
        assert!(!on_several_workspaces(&[Some(2)]));
        assert!(!on_several_workspaces(&[Some(2), Some(2)]));
        assert!(!on_several_workspaces(&[None, Some(2), None]));
        assert!(on_several_workspaces(&[Some(1), None, Some(3)]));
        assert!(on_several_workspaces(&[Some(1), Some(1), Some(3)]));
    }

    #[test]
    fn a_cards_workspace_is_under_its_title_over_its_picture() {
        let shot = |workspace: Option<&str>| Shot {
            title: "Notes".into(),
            size: (1600, 900),
            workspace: workspace.map(Into::into),
        };
        let texts = |l: &kabuk_ui::node::Layout| -> Vec<(String, f32)> {
            l.draws
                .iter()
                .filter_map(|d| match d {
                    kabuk_ui::node::Draw::Text { spec, rect } => Some((spec.spans[0].text.clone(), rect.y)),
                    _ => None,
                })
                .collect()
        };
        let l = layout(
            &view(1.0, &TIP, &PREVIEW, &[shot(Some("Workspace 2 · Web")), shot(None)], &[]),
            Point::default(),
            &Fake,
        );
        let t = texts(&l);
        let title = t.iter().find(|(s, _)| s == "Notes").unwrap().1;
        let ws = t
            .iter()
            .find(|(s, _)| s == "Workspace 2 · Web")
            .expect("the workspace")
            .1;
        let frame = l
            .hits
            .iter()
            .find(|(_, h)| h.module == FRAME && h.value == 0)
            .unwrap()
            .0;
        assert!(title < ws && ws < frame.y, "{t:?} {frame:?}");
        assert_eq!(t.iter().filter(|(s, _)| s.starts_with("Workspace")).count(), 1);
    }
}
