//! Rıhtım's dock: a row of app tiles on the primary monitor's edge opposite the bar
//! (design `.screen[data-dir="f"] .dock`). Pure; the window lives in the manager.
//!
//! It holds the apps module's apps (DECISIONS D17, D25): the apps pinned to Windows'
//! taskbar, then the other running ones, each with the design's running dot; after a
//! separator the last tile opens the launcher.

use kabuk_core::config::{Edge, Look};
use kabuk_ui::geom::{Insets, Scale};
use kabuk_ui::node::{Node, Paint, Role};
use kabuk_win::msgloop::PxRect;

use super::geometry::vertical;

/// The last tile (`dockHtml()`): its icon and tile colour number.
pub const LAST: (&str, u8) = ("app", 7);
/// The design's tile colour numbers (`dock-a1` … `dock-a7`); the last tile uses [`LAST`]'s.
pub fn tiles() -> std::ops::RangeInclusive<u8> {
    1..=LAST.1
}

/// An app tile (DECISIONS D17): the program's picture and whether it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockApp {
    pub image: u64,
    pub running: bool,
}

/// What a click on the dock lands on.
pub mod hit {
    /// An app; the value is its index in the list given to [`super::view`].
    pub const APP: &str = "dock-app";
    /// The last tile, which opens the launcher.
    pub const LAUNCHER: &str = "dock-launcher";
}

/// Sizes from the design's CSS (DIPs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockStyle {
    /// `.dock { gap; padding; border-radius; border }`
    pub gap: f32,
    pub padding: f32,
    pub radius: f32,
    pub border: f32,
    /// Distance from the screen edge (`bottom: 8px`).
    pub distance: f32,
    /// `.dock .app { width; height; border-radius }`
    pub tile: f32,
    pub tile_radius: f32,
    /// `.dock .app svg.i { width; stroke-width }`
    pub icon: f32,
    pub icon_stroke: f32,
    /// `.dock .sepr { width: 1px; margin: 4px 2px }`: thickness, margin along the row and across it.
    pub separator: f32,
    pub separator_along: f32,
    pub separator_across: f32,
    /// `.dock .app.run::after { width: 4px; bottom: -6px }`: the running dot and its
    /// distance from the picture (kept inside the tile, so the dock keeps its size).
    pub run_dot: f32,
    pub run_gap: f32,
}

pub const STYLE: DockStyle = DockStyle {
    gap: 8.0,
    padding: 8.0,
    radius: 20.0,
    border: 1.0,
    distance: 8.0,
    tile: 50.0,
    tile_radius: 13.0,
    icon: 26.0,
    icon_stroke: 1.8,
    separator: 1.0,
    separator_along: 2.0,
    separator_across: 4.0,
    run_dot: 4.0,
    run_gap: 2.0,
};

impl DockStyle {
    /// The side of an app's picture inside its tile, leaving room for the running dot.
    pub fn app_icon(&self) -> f32 {
        self.tile - self.run_dot - self.run_gap
    }
}

/// Only Rıhtım has a dock.
pub fn shown(look: Look) -> bool {
    look == Look::Rihtim
}

/// The dock goes on the edge opposite the bar.
pub fn edge(bar: Edge) -> Edge {
    match bar {
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
        Edge::Left => Edge::Right,
        Edge::Right => Edge::Left,
    }
}

fn tile(icon: &'static str, n: u8, s: &DockStyle) -> Node {
    Node::row(vec![Node::icon(icon, s.icon, Role::White).stroke(s.icon_stroke)])
        .align(kabuk_ui::node::Align::Center)
        .size(Some(s.tile), Some(s.tile))
        .padding(Insets::symmetric((s.tile - s.icon) / 2.0, 0.0))
        .paint(Paint {
            fill: Role::Tile(n),
            border: None,
            radius: s.tile_radius,
        })
}

/// An app's tile: its picture with the running dot on the screen's side of it, below on a
/// top or bottom dock (`.dock .app.run::after`, turned with the dock on the left or right).
fn app_tile(a: &DockApp, dock_edge: Edge, s: &DockStyle) -> Node {
    let dot = Node::spacer(s.run_dot, s.run_dot);
    let dot = if a.running {
        dot.paint(Paint {
            fill: Role::Dim,
            border: None,
            radius: s.run_dot / 2.0,
        })
    } else {
        dot
    };
    let picture = Node::image(a.image, s.app_icon());
    let node = match dock_edge {
        Edge::Top | Edge::Bottom => Node::column(vec![picture, dot]),
        Edge::Right => Node::row(vec![picture, dot]),
        Edge::Left => Node::row(vec![dot, picture]),
    };
    node.gap(s.run_gap)
        .align(kabuk_ui::node::Align::Center)
        .size(Some(s.tile), Some(s.tile))
}

/// The dock's tree; along the edge's direction (a column on the left or right).
pub fn view(dock_edge: Edge, s: &DockStyle, apps: &[DockApp]) -> Node {
    let v = vertical(dock_edge);
    let line = s.tile - 2.0 * s.separator_across;
    let (w, h, margin) = if v {
        (line, s.separator, Insets::symmetric(0.0, s.separator_along))
    } else {
        (s.separator, line, Insets::symmetric(s.separator_along, 0.0))
    };
    let separator = Node::row(vec![Node::spacer(w, h).paint(Paint {
        fill: Role::GlassLine,
        border: None,
        radius: 0.0,
    })])
    .padding(margin);
    let mut children: Vec<Node> = apps
        .iter()
        .enumerate()
        .map(|(i, a)| app_tile(a, dock_edge, s).hit(hit::APP, u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    if !children.is_empty() {
        children.push(separator);
    }
    children.push(tile(LAST.0, LAST.1, s).hit(hit::LAUNCHER, 0));
    let inner = s.padding + s.border;
    let node = if v { Node::column(children) } else { Node::row(children) };
    node.gap(s.gap)
        .padding(Insets::symmetric(inner, inner))
        .align(kabuk_ui::node::Align::Center)
        .paint(Paint {
            fill: Role::Glass,
            border: Some((Role::GlassLine, s.border)),
            radius: s.radius,
        })
}

/// The dock window in physical pixels: `w`×`h` centred along the monitor `mon` and
/// `distance` inside the outer side of the reserved `strip` (which other AppBars, such as
/// the Windows taskbar, may have pushed inwards).
pub fn window_rect(dock_edge: Edge, mon: PxRect, strip: PxRect, (w, h): (i32, i32), distance: i32) -> PxRect {
    let cx = mon.x + (mon.w - w) / 2;
    let cy = mon.y + (mon.h - h) / 2;
    let (x, y) = match dock_edge {
        Edge::Bottom => (cx, strip.y + strip.h - distance - h),
        Edge::Top => (cx, strip.y + distance),
        Edge::Right => (strip.x + strip.w - distance - w, cy),
        Edge::Left => (strip.x + distance, cy),
    };
    PxRect { x, y, w, h }
}

/// Physical pixels the dock reserves from its edge (`--dock`).
pub fn reserved_px(dock_token: f32, scale: Scale) -> i32 {
    scale.px(dock_token)
}

#[cfg(test)]
mod tests {
    //! Edge cases: all four edges, the separator turning with the dock, the reservation
    //! at 125 %.
    use super::*;
    use kabuk_ui::geom::{Point, Rect, Size};
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics, TextSpec};

    struct NoText;
    impl TextMeasure for NoText {
        fn measure(&self, _: &TextSpec) -> TextMetrics {
            unreachable!("the dock has no text")
        }
    }

    /// Six apps, as many as the design's fixed list; the second to fourth run.
    fn apps() -> Vec<DockApp> {
        (0..6)
            .map(|i| DockApp {
                image: 100 + i,
                running: (1..4).contains(&i),
            })
            .collect()
    }

    fn panels(edge: Edge) -> (Size, Vec<(Rect, Role)>) {
        let l = layout(&view(edge, &STYLE, &apps()), Point::default(), &NoText);
        let panels = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Panel { rect, paint } => Some((*rect, paint.fill)),
                _ => None,
            })
            .collect();
        (l.size, panels)
    }

    #[test]
    fn horizontal_dock_matches_the_design() {
        let (size, p) = panels(Edge::Bottom);
        // 1 + 8 + 7 × 50 + 7 gaps × 8 + (2 + 1 + 2) separator + 8 + 1.
        assert_eq!(
            size,
            Size {
                w: 1.0 + 8.0 + 350.0 + 56.0 + 5.0 + 8.0 + 1.0,
                h: 68.0
            }
        );
        assert_eq!(p[0].1, Role::Glass);
        let l = layout(&view(Edge::Bottom, &STYLE, &apps()), Point::default(), &NoText);
        let app_hits: Vec<Rect> = l
            .hits
            .iter()
            .filter(|(_, h)| h.module == hit::APP)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(app_hits.len(), 6);
        assert_eq!(app_hits[0], Rect::new(9.0, 9.0, 50.0, 50.0));
        // Each picture fills its tile but for the dot below it, centred across.
        let pictures: Vec<Rect> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Image { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(pictures[0], Rect::new(12.0, 9.0, 44.0, 44.0));
        // Running ones carry the design's dim dot, 2 px under the picture.
        let dots: Vec<Rect> = p.iter().filter(|(_, r)| *r == Role::Dim).map(|(r, _)| *r).collect();
        assert_eq!(dots.len(), 3);
        assert_eq!((dots[0].w, dots[0].y), (4.0, 55.0));
        // The last tile is the design's, and opens the launcher.
        let last: Vec<_> = p.iter().filter(|(_, r)| matches!(r, Role::Tile(_))).collect();
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].1, Role::Tile(LAST.1));
        let middle = Point {
            x: last[0].0.x + last[0].0.w / 2.0,
            y: last[0].0.y + last[0].0.h / 2.0,
        };
        assert_eq!(l.hit_test(middle).map(|h| h.module), Some(hit::LAUNCHER));
        let sep = p.iter().find(|(_, r)| *r == Role::GlassLine).unwrap().0;
        assert_eq!((sep.w, sep.h, sep.y), (1.0, 42.0, 13.0));
        // The separator sits between the sixth app and the last tile, 2 + gap from each.
        assert_eq!(sep.x - app_hits[5].right(), STYLE.gap + 2.0);
        assert_eq!(last[0].0.x - sep.right(), STYLE.gap + 2.0);
    }

    #[test]
    fn with_no_apps_only_the_launcher_tile_shows() {
        let l = layout(&view(Edge::Bottom, &STYLE, &[]), Point::default(), &NoText);
        assert_eq!(l.size, Size { w: 68.0, h: 68.0 });
        assert!(!l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::GlassLine)));
        assert_eq!(l.hits.len(), 1);
    }

    #[test]
    fn vertical_dock_turns_the_separator() {
        let (size, p) = panels(Edge::Right);
        assert_eq!(size.w, 68.0);
        let sep = p.iter().find(|(_, r)| *r == Role::GlassLine).unwrap().0;
        assert_eq!((sep.w, sep.h, sep.x), (42.0, 1.0, 13.0));
        // On the right edge the dot is on the screen's side, right of the picture.
        let dot = p.iter().find(|(_, r)| *r == Role::Dim).unwrap().0;
        assert_eq!(dot.x, 9.0 + 44.0 + 2.0);
        let (_, left) = panels(Edge::Left);
        assert_eq!(left.iter().find(|(_, r)| *r == Role::Dim).unwrap().0.x, 9.0);
    }

    #[test]
    fn opposite_edges_and_placement() {
        assert_eq!(edge(Edge::Top), Edge::Bottom);
        assert_eq!(edge(Edge::Bottom), Edge::Top);
        assert_eq!(edge(Edge::Left), Edge::Right);
        assert_eq!(edge(Edge::Right), Edge::Left);
        // Monitor 2 sits left of Monitor 1 (ana); the taskbar pushed the bottom strip up by 60.
        let mon = PxRect {
            x: -2560,
            y: 0,
            w: 2560,
            h: 1440,
        };
        let bottom = PxRect {
            x: -2560,
            y: 1282,
            w: 2560,
            h: 98,
        };
        assert_eq!(
            window_rect(Edge::Bottom, mon, bottom, (540, 90), 10),
            PxRect {
                x: -1550,
                y: 1280,
                w: 540,
                h: 90
            }
        );
        let top = PxRect {
            x: -2560,
            y: 65,
            w: 2560,
            h: 98,
        };
        assert_eq!(
            window_rect(Edge::Top, mon, top, (540, 90), 10),
            PxRect {
                x: -1550,
                y: 75,
                w: 540,
                h: 90
            }
        );
        let right = PxRect {
            x: -98,
            y: 0,
            w: 98,
            h: 1440,
        };
        assert_eq!(
            window_rect(Edge::Right, mon, right, (90, 540), 10),
            PxRect {
                x: -100,
                y: 450,
                w: 90,
                h: 540
            }
        );
        let left = PxRect {
            x: -2560,
            y: 0,
            w: 98,
            h: 1440,
        };
        assert_eq!(
            window_rect(Edge::Left, mon, left, (90, 540), 10),
            PxRect {
                x: -2550,
                y: 450,
                w: 90,
                h: 540
            }
        );
        // `--dock: 78px` at 125 %.
        assert_eq!(reserved_px(78.0, Scale(1.25)), 98);
        assert!(shown(Look::Rihtim) && !shown(Look::Ada));
    }
}
