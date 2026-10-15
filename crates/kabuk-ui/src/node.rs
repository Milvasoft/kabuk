//! The small layout tree modules return (ARCHITECTURE §5 "Module interface"). A node
//! names roles, not colours; the renderer maps roles to the theme's colours.
//!
//! Layout is a flexbox subset: a box lays its children along one axis with a gap and
//! aligns them on the other axis (start, centre, end, or text baseline for rows).
//! Text is measured through [`TextMeasure`], so layout is testable without DirectWrite.

use crate::geom::{Insets, Point, Rect, Size};

/// Colour roles; the look's palette decides the actual colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Fg,
    Dim,
    Faint,
    Accent,
    AccentFill,
    OnAccent,
    /// The look's second accent (`accent2`), e.g. Konsol's layout tag.
    Accent2,
    Surface,
    SurfaceSel,
    SurfaceLine,
    /// The second surface (`surf-alt`), e.g. Etkinlik's launcher result list.
    SurfaceAlt,
    Line,
    /// Devre's circuit traces (`trace`, `trace-strong`).
    Trace,
    TraceStrong,
    /// Rıhtım's translucent bar (`glass`, `glass-line`).
    Glass,
    GlassLine,
    /// Text of an occupied, inactive workspace where a look tints it (Çizgi).
    Busy,
    /// The desktop colour (`desk`), under bars that sit on a wallpaper.
    Desk,
    /// Always black (Etkinlik's bar in both modes).
    Black,
    /// Always white (Rıhtım's dock icons).
    White,
    /// Nothing (a panel with only a border).
    Clear,
    /// Rıhtım's dock tiles, numbered as in the design (`.dock .a1` …), usually gradients.
    Tile(u8),
    /// The accent mixed into the desk colour, this many percent of it (the design's
    /// `color-mix(in srgb, var(--accent-s) n%, var(--desk))`).
    AccentTint(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontRole {
    /// The look's UI face.
    Ui,
    /// The look's numeric (tabular) face.
    Num,
}

/// A run of text with its own colour and weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub color: Role,
    pub weight: u16,
    /// Extra space after the span's last character, like CSS `margin-right` on an inline element.
    pub after: f32,
    /// Drawn with a line under it (Çizgi's matched letters in the launcher).
    pub underline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    pub spans: Vec<Span>,
    pub font: FontRole,
    pub size: f32,
    /// Longer text is cut with an ellipsis.
    pub max_width: Option<f32>,
}

// Sizes are compared bit for bit, so a spec can key a cache.
impl Eq for TextSpec {}

impl std::hash::Hash for TextSpec {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        for s in &self.spans {
            s.text.hash(h);
            s.color.hash(h);
            s.weight.hash(h);
            s.after.to_bits().hash(h);
        }
        self.font.hash(h);
        self.size.to_bits().hash(h);
        self.max_width.map(f32::to_bits).hash(h);
    }
}

impl TextSpec {
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IconSpec {
    pub name: &'static str,
    pub size: f32,
    pub color: Role,
    /// Stroke width in view-box units; `None` is the design's default (`svg.i`).
    pub stroke: Option<f32>,
}

/// A picture from [`crate::render::Images`] (e.g. another program's tray icon), drawn
/// `size` DIPs wide and `height` high (square but for [`Node::picture`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageSpec {
    pub key: u64,
    pub size: f32,
    pub height: f32,
}

/// Fill, optional border and corner radius of a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub fill: Role,
    pub border: Option<(Role, f32)>,
    /// Corner radius; larger than half the short side means a full capsule.
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    /// Rows only: line up the first text baselines.
    Baseline,
}

/// Identifies a clickable region (module id and a module-defined value).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitId {
    pub module: &'static str,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Box {
        axis: Axis,
        children: Vec<Node>,
    },
    Text(TextSpec),
    Icon(IconSpec),
    Image(ImageSpec),
    Spacer,
    /// Children drawn over each other in the same box, centred horizontally and placed
    /// vertically by their own `align` (start, centre, end), then moved by `offset`.
    Layer {
        children: Vec<Node>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub padding: Insets,
    pub gap: f32,
    pub align: Align,
    pub paint: Option<Paint>,
    pub hit: Option<HitId>,
    /// Extra displacement inside a layer (CSS `position: relative` offsets).
    pub offset: Point,
    /// Inside a layer: stretch along this axis to the layer's size minus an inset on
    /// each side (CSS `left: 6px; right: 6px`).
    pub fill: Option<(Axis, f32)>,
}

impl Node {
    fn with_kind(kind: Kind) -> Node {
        Node {
            kind,
            width: None,
            height: None,
            padding: Insets::default(),
            gap: 0.0,
            align: Align::Center,
            paint: None,
            hit: None,
            offset: Point::default(),
            fill: None,
        }
    }

    pub fn row(children: Vec<Node>) -> Node {
        Node::with_kind(Kind::Box {
            axis: Axis::Row,
            children,
        })
    }

    pub fn column(children: Vec<Node>) -> Node {
        Node::with_kind(Kind::Box {
            axis: Axis::Column,
            children,
        })
    }

    pub fn text(spec: TextSpec) -> Node {
        Node::with_kind(Kind::Text(spec))
    }

    pub fn icon(name: &'static str, size: f32, color: Role) -> Node {
        Node::with_kind(Kind::Icon(IconSpec {
            name,
            size,
            color,
            stroke: None,
        }))
    }

    pub fn image(key: u64, size: f32) -> Node {
        Node::with_kind(Kind::Image(ImageSpec {
            key,
            size,
            height: size,
        }))
    }

    /// A stored picture stretched to `w` × `h` DIPs (e.g. a blurred wallpaper).
    pub fn picture(key: u64, w: f32, h: f32) -> Node {
        Node::with_kind(Kind::Image(ImageSpec {
            key,
            size: w,
            height: h,
        }))
    }

    /// An icon's stroke width in view-box units (no effect on other nodes).
    pub fn stroke(mut self, width: f32) -> Node {
        if let Kind::Icon(spec) = &mut self.kind {
            spec.stroke = Some(width);
        }
        self
    }

    pub fn layer(children: Vec<Node>) -> Node {
        Node::with_kind(Kind::Layer { children })
    }

    pub fn fill(mut self, axis: Axis, inset: f32) -> Node {
        self.fill = Some((axis, inset));
        self
    }

    pub fn offset(mut self, dx: f32, dy: f32) -> Node {
        self.offset = Point { x: dx, y: dy };
        self
    }

    pub fn spacer(w: f32, h: f32) -> Node {
        Node {
            width: Some(w),
            height: Some(h),
            ..Node::with_kind(Kind::Spacer)
        }
    }

    pub fn gap(mut self, gap: f32) -> Node {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: Insets) -> Node {
        self.padding = padding;
        self
    }

    pub fn align(mut self, align: Align) -> Node {
        self.align = align;
        self
    }

    pub fn size(mut self, width: Option<f32>, height: Option<f32>) -> Node {
        self.width = width;
        self.height = height;
        self
    }

    pub fn paint(mut self, paint: Paint) -> Node {
        self.paint = Some(paint);
        self
    }

    pub fn hit(mut self, module: &'static str, value: u32) -> Node {
        self.hit = Some(HitId { module, value });
        self
    }
}

/// Size and first baseline (distance from the top) of a laid-out text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

pub trait TextMeasure {
    fn measure(&self, text: &TextSpec) -> TextMetrics;
}

/// One thing to draw, in paint order.
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Panel { rect: Rect, paint: Paint },
    Text { rect: Rect, spec: TextSpec },
    Icon { rect: Rect, spec: IconSpec },
    Image { rect: Rect, spec: ImageSpec },
}

/// The result of laying out a tree at an origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub size: Size,
    pub draws: Vec<Draw>,
    pub hits: Vec<(Rect, HitId)>,
}

impl Layout {
    /// The innermost clickable region at `p` (later regions are on top).
    pub fn hit_test(&self, p: Point) -> Option<&HitId> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, id)| id)
    }
}

/// What changed between two layouts of the same surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Damage {
    None,
    /// Only this area (DIPs, in layout coordinates) needs redrawing.
    Area(Rect),
    /// The structure changed: redraw everything.
    Full,
}

/// Glyphs and stroked icons may paint slightly outside their boxes (overhangs, half
/// a stroke, antialiasing), so damaged areas grow by this much.
pub const DAMAGE_MARGIN: f32 = 2.0;

impl Draw {
    pub fn rect(&self) -> Rect {
        match self {
            Draw::Panel { rect, .. } | Draw::Text { rect, .. } | Draw::Icon { rect, .. } | Draw::Image { rect, .. } => {
                *rect
            }
        }
    }
}

/// The area to redraw so that `new` replaces `old`.
pub fn damage(old: &Layout, new: &Layout) -> Damage {
    if old.draws.len() != new.draws.len() {
        return Damage::Full;
    }
    let mut area: Option<Rect> = None;
    for (a, b) in old.draws.iter().zip(&new.draws) {
        if a != b {
            let r = a.rect().union(&b.rect());
            area = Some(area.map_or(r, |u| u.union(&r)));
        }
    }
    match area {
        None => Damage::None,
        Some(r) => Damage::Area(Rect::new(
            r.x - DAMAGE_MARGIN,
            r.y - DAMAGE_MARGIN,
            r.w + 2.0 * DAMAGE_MARGIN,
            r.h + 2.0 * DAMAGE_MARGIN,
        )),
    }
}

/// Sizes of a node and its subtree, computed once per layout pass.
struct Measured {
    size: Size,
    baseline: Option<f32>,
    text: Option<TextMetrics>,
    children: Vec<Measured>,
}

fn measure(node: &Node, m: &dyn TextMeasure) -> Measured {
    let mut text = None;
    let mut kids = Vec::new();
    let (content, baseline) = match &node.kind {
        Kind::Text(spec) => {
            let t = m.measure(spec);
            text = Some(t);
            let width = spec.max_width.map_or(t.width, |max| t.width.min(max));
            (Size { w: width, h: t.height }, Some(t.baseline))
        }
        Kind::Icon(IconSpec { size, .. }) => (Size { w: *size, h: *size }, None),
        Kind::Image(ImageSpec { size, height, .. }) => (Size { w: *size, h: *height }, None),
        Kind::Spacer => (Size::default(), None),
        Kind::Layer { children } => {
            kids = children.iter().map(|c| measure(c, m)).collect();
            let sizes = &kids;
            let w = sizes.iter().map(|s| s.size.w).fold(0.0, f32::max);
            let h = sizes.iter().map(|s| s.size.h).fold(0.0, f32::max);
            let first = sizes.first().and_then(|s| s.baseline.map(|b| b + (h - s.size.h) / 2.0));
            (Size { w, h }, first)
        }
        Kind::Box { axis, children } => {
            kids = children.iter().map(|c| measure(c, m)).collect();
            let sizes = &kids;
            let gaps = node.gap * sizes.len().saturating_sub(1) as f32;
            match axis {
                Axis::Row => {
                    let w = sizes.iter().map(|s| s.size.w).sum::<f32>() + gaps;
                    if node.align == Align::Baseline {
                        let (above, below) = baseline_extent(sizes);
                        (Size { w, h: above + below }, Some(above))
                    } else {
                        let h = sizes.iter().map(|s| s.size.h).fold(0.0, f32::max);
                        let first = sizes.iter().find_map(|s| s.baseline.map(|b| b + (h - s.size.h) / 2.0));
                        (Size { w, h }, first)
                    }
                }
                Axis::Column => {
                    let w = sizes.iter().map(|s| s.size.w).fold(0.0, f32::max);
                    let h = sizes.iter().map(|s| s.size.h).sum::<f32>() + gaps;
                    (Size { w, h }, sizes.first().and_then(|s| s.baseline))
                }
            }
        }
    };
    let w = node.width.unwrap_or(content.w + node.padding.horizontal());
    let h = node.height.unwrap_or(content.h + node.padding.vertical());
    // A fixed height centres the content vertically, so the baseline moves with it.
    let inner_h = h - node.padding.vertical();
    let baseline = baseline.map(|b| node.padding.top + b + (inner_h - content.h) / 2.0);
    Measured {
        size: Size { w, h },
        baseline,
        text,
        children: kids,
    }
}

/// Space above and below the shared baseline of children aligned on it. Children
/// without text are centred on the baseline's line box.
fn baseline_extent(sizes: &[Measured]) -> (f32, f32) {
    let mut above: f32 = 0.0;
    let mut below: f32 = 0.0;
    for s in sizes {
        let b = s.baseline.unwrap_or(s.size.h);
        above = above.max(b);
        below = below.max(s.size.h - b);
    }
    (above, below)
}

/// Lays out `node` with its top-left corner at `origin`.
pub fn layout(node: &Node, origin: Point, m: &dyn TextMeasure) -> Layout {
    let mut out = Layout::default();
    let measured = measure(node, m);
    place(
        node,
        &measured,
        Rect::new(origin.x, origin.y, measured.size.w, measured.size.h),
        &mut out,
    );
    out.size = measured.size;
    out
}

impl Layout {
    /// The same layout moved by `(dx, dy)`.
    pub fn translated(mut self, dx: f32, dy: f32) -> Layout {
        for d in &mut self.draws {
            match d {
                Draw::Panel { rect, .. }
                | Draw::Text { rect, .. }
                | Draw::Icon { rect, .. }
                | Draw::Image { rect, .. } => *rect = rect.offset(dx, dy),
            }
        }
        for (r, _) in &mut self.hits {
            *r = r.offset(dx, dy);
        }
        self
    }
}

fn place(node: &Node, me: &Measured, rect: Rect, out: &mut Layout) {
    if let Some(paint) = node.paint {
        out.draws.push(Draw::Panel { rect, paint });
    }
    if let Some(hit) = &node.hit {
        out.hits.push((rect, hit.clone()));
    }
    let inner = Rect::new(
        rect.x + node.padding.left,
        rect.y + node.padding.top,
        rect.w - node.padding.horizontal(),
        rect.h - node.padding.vertical(),
    );
    match &node.kind {
        Kind::Text(spec) => {
            // Centred in a node given a fixed size (CSS `place-items: center`).
            let t = me.text.unwrap_or(TextMetrics {
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            });
            let w = spec.max_width.map_or(t.width, |max| t.width.min(max));
            let x = inner.x + ((inner.w - w) / 2.0).max(0.0);
            let y = inner.y + (inner.h - t.height) / 2.0;
            out.draws.push(Draw::Text {
                rect: Rect::new(x, y, w.min(inner.w), t.height),
                spec: spec.clone(),
            });
        }
        Kind::Image(spec) => {
            let x = inner.x + (inner.w - spec.size) / 2.0;
            let y = inner.y + (inner.h - spec.height) / 2.0;
            out.draws.push(Draw::Image {
                rect: Rect::new(x, y, spec.size, spec.height),
                spec: *spec,
            });
        }
        Kind::Icon(spec) => {
            // Centred in the node in case the node was given a larger fixed size.
            let x = inner.x + (inner.w - spec.size) / 2.0;
            let y = inner.y + (inner.h - spec.size) / 2.0;
            out.draws.push(Draw::Icon {
                rect: Rect::new(x, y, spec.size, spec.size),
                spec: spec.clone(),
            });
        }
        Kind::Spacer => {}
        Kind::Layer { children } => {
            for (child, s) in children.iter().zip(&me.children) {
                // A filled child takes the layer's size on that axis; its own children keep
                // their sizes (place reads only them, the text and the rectangle given).
                let (w, h) = match child.fill {
                    Some((Axis::Row, inset)) => ((inner.w - 2.0 * inset).max(0.0), s.size.h),
                    Some((Axis::Column, inset)) => (s.size.w, (inner.h - 2.0 * inset).max(0.0)),
                    None => (s.size.w, s.size.h),
                };
                let x = inner.x + (inner.w - w) / 2.0 + child.offset.x;
                let y = match child.align {
                    Align::Start => inner.y,
                    Align::End => inner.bottom() - h,
                    Align::Center | Align::Baseline => inner.y + (inner.h - h) / 2.0,
                } + child.offset.y;
                place(child, s, Rect::new(x, y, w, h), out);
            }
        }
        Kind::Box { axis, children } => {
            let sizes = &me.children;
            let (above, below) = baseline_extent(sizes);
            // Baseline-aligned children are centred as one line box.
            let baseline_top = inner.y + (inner.h - (above + below)) / 2.0;
            // Children pack from the start of the main axis.
            let mut cursor = match axis {
                Axis::Row => inner.x,
                Axis::Column => inner.y,
            };
            for (child, s) in children.iter().zip(sizes) {
                let r = match axis {
                    Axis::Row => {
                        let y = match node.align {
                            Align::Start => inner.y,
                            Align::Center => inner.y + (inner.h - s.size.h) / 2.0,
                            Align::End => inner.bottom() - s.size.h,
                            Align::Baseline => baseline_top + above - s.baseline.unwrap_or(s.size.h),
                        };
                        Rect::new(cursor, y, s.size.w, s.size.h)
                    }
                    Axis::Column => {
                        let x = match node.align {
                            Align::Start | Align::Baseline => inner.x,
                            Align::Center => inner.x + (inner.w - s.size.w) / 2.0,
                            Align::End => inner.right() - s.size.w,
                        };
                        Rect::new(x, cursor, s.size.w, s.size.h)
                    }
                };
                place(child, s, r, out);
                cursor += node.gap
                    + match axis {
                        Axis::Row => s.size.w,
                        Axis::Column => s.size.h,
                    };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: empty box, single child, gaps only between children, padding,
    //! fixed sizes larger and smaller than content, centre/start/end alignment on
    //! both axes, baseline alignment of different font sizes, max width with
    //! ellipsis, icons centred in fixed boxes, hit regions nested (innermost wins),
    //! panel before content in paint order.
    use super::*;

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

    fn text(s: &str, size: f32) -> Node {
        Node::text(TextSpec {
            spans: vec![Span {
                text: s.into(),
                color: Role::Fg,
                weight: 400,
                after: 0.0,
                underline: false,
            }],
            font: FontRole::Ui,
            size,
            max_width: None,
        })
    }

    /// Found by the overview's numbers and window titles, which never showed: a layer's
    /// child that is a row or column lost its children's sizes, so nothing in it was laid
    /// out.
    #[test]
    fn a_layers_row_lays_out_its_children() {
        let row = Node::row(vec![text("Notes", 10.0), Node::icon("app", 16.0, Role::Dim)])
            .size(Some(200.0), Some(40.0))
            .align(Align::Start)
            .offset(10.0, 20.0);
        let l = layout(
            &Node::layer(vec![row]).size(Some(400.0), Some(300.0)),
            Point::default(),
            &Fake,
        );
        let texts = l.draws.iter().filter(|d| matches!(d, Draw::Text { .. })).count();
        let icons = l.draws.iter().filter(|d| matches!(d, Draw::Icon { .. })).count();
        assert_eq!((texts, icons), (1, 1));
    }

    fn rects(l: &Layout) -> Vec<Rect> {
        l.draws
            .iter()
            .map(|d| match d {
                Draw::Panel { rect, .. }
                | Draw::Text { rect, .. }
                | Draw::Icon { rect, .. }
                | Draw::Image { rect, .. } => *rect,
            })
            .collect()
    }

    #[test]
    fn empty_box_is_its_padding() {
        let l = layout(&Node::row(vec![]).padding(Insets::all(3.0)), Point::default(), &Fake);
        assert_eq!(l.size, Size { w: 6.0, h: 6.0 });
        assert!(l.draws.is_empty());
    }

    #[test]
    fn row_places_children_with_gaps_between_only() {
        let row = Node::row(vec![
            text("ab", 16.0),
            text("c", 16.0),
            Node::icon("cpu", 16.0, Role::Dim),
        ])
        .gap(6.0);
        let l = layout(&row, Point { x: 10.0, y: 5.0 }, &Fake);
        assert_eq!(
            l.size,
            Size {
                w: 16.0 + 6.0 + 8.0 + 6.0 + 16.0,
                h: 20.0
            }
        );
        assert_eq!(
            rects(&l),
            vec![
                Rect::new(10.0, 5.0, 16.0, 20.0),
                Rect::new(32.0, 5.0, 8.0, 20.0),
                Rect::new(46.0, 7.0, 16.0, 16.0)
            ]
        );
    }

    #[test]
    fn fixed_height_centres_content_and_padding_applies() {
        let capsule = Node::row(vec![text("12%", 13.0)])
            .padding(Insets::symmetric(14.0, 0.0))
            .size(None, Some(38.0))
            .paint(Paint {
                fill: Role::Surface,
                border: Some((Role::Line, 1.0)),
                radius: 999.0,
            });
        let l = layout(&capsule, Point::default(), &Fake);
        assert_eq!(
            l.size,
            Size {
                w: 24.0 + 28.0,
                h: 38.0
            }
        );
        // Panel first, then the text, vertically centred.
        assert!(matches!(l.draws[0], Draw::Panel { .. }));
        assert_eq!(rects(&l)[1], Rect::new(14.0, (38.0 - 16.25) / 2.0, 24.0, 16.25));
    }

    #[test]
    fn fixed_width_smaller_than_content_is_kept() {
        let n = Node::row(vec![text("abcdef", 10.0)]).size(Some(20.0), None);
        assert_eq!(layout(&n, Point::default(), &Fake).size.w, 20.0);
    }

    #[test]
    fn column_alignment() {
        let col = |a| Node::column(vec![text("a", 10.0), text("abc", 10.0)]).gap(2.0).align(a);
        let start = rects(&layout(&col(Align::Start), Point::default(), &Fake));
        let center = rects(&layout(&col(Align::Center), Point::default(), &Fake));
        let end = rects(&layout(&col(Align::End), Point::default(), &Fake));
        assert_eq!((start[0].x, center[0].x, end[0].x), (0.0, 8.0, 16.0));
        assert_eq!(start[1].y, 12.5 + 2.0);
    }

    #[test]
    fn row_start_and_end_alignment() {
        let row = |a| Node::row(vec![text("a", 10.0), text("b", 20.0)]).align(a);
        assert_eq!(rects(&layout(&row(Align::Start), Point::default(), &Fake))[0].y, 0.0);
        assert_eq!(
            rects(&layout(&row(Align::End), Point::default(), &Fake))[0].y,
            25.0 - 12.5
        );
    }

    #[test]
    fn baseline_alignment_lines_up_text_of_different_sizes() {
        // Clock (15) and date (13) as in the design's `.clock { align-items: baseline }`.
        let clock = Node::row(vec![text("20:31", 15.0), text("Pzt 12 Eki", 13.0)])
            .gap(10.0)
            .align(Align::Baseline);
        let l = layout(&clock, Point::default(), &Fake);
        let r = rects(&l);
        let base0 = r[0].y + 15.0 * 0.8;
        let base1 = r[1].y + 13.0 * 0.8;
        assert!((base0 - base1).abs() < 1e-4, "{base0} vs {base1}");
        // Inside a fixed-height capsule the shared baseline stays aligned.
        let capsule = Node::row(vec![clock]).size(None, Some(38.0));
        let r = rects(&layout(&capsule, Point::default(), &Fake));
        assert!((r[0].y + 12.0 - (r[1].y + 10.4)).abs() < 1e-4);
    }

    #[test]
    fn max_width_truncates() {
        let mut spec = TextSpec {
            spans: vec![Span {
                text: "a".repeat(100),
                color: Role::Dim,
                weight: 400,
                after: 0.0,
                underline: false,
            }],
            font: FontRole::Ui,
            size: 13.5,
            max_width: Some(460.0),
        };
        assert_eq!(layout(&Node::text(spec.clone()), Point::default(), &Fake).size.w, 460.0);
        spec.max_width = Some(1000.0);
        assert_eq!(layout(&Node::text(spec), Point::default(), &Fake).size.w, 800.0);
    }

    #[test]
    fn icon_is_centred_in_a_fixed_box() {
        let n = Node::icon("cpu", 16.0, Role::Dim).size(Some(24.0), Some(24.0));
        assert_eq!(
            rects(&layout(&n, Point::default(), &Fake))[0],
            Rect::new(4.0, 4.0, 16.0, 16.0)
        );
    }

    #[test]
    fn hit_test_finds_innermost() {
        let ws = |n: u32| text(&n.to_string(), 12.5).size(Some(24.0), Some(20.0)).hit("ws", n);
        let row = Node::row(vec![ws(1), ws(2)]).gap(6.0).hit("bar", 0);
        let l = layout(&row, Point::default(), &Fake);
        assert_eq!(l.hit_test(Point { x: 5.0, y: 5.0 }).map(|h| h.value), Some(1));
        assert_eq!(
            l.hit_test(Point { x: 31.0, y: 5.0 }).map(|h| (h.module, h.value)),
            Some(("ws", 2))
        );
        // In the gap: only the row itself.
        assert_eq!(l.hit_test(Point { x: 26.0, y: 5.0 }).map(|h| h.module), Some("bar"));
        assert_eq!(l.hit_test(Point { x: 500.0, y: 5.0 }), None);
    }

    #[test]
    fn layer_stacks_children_with_their_own_alignment_and_offset() {
        let dot = Node::spacer(3.0, 3.0)
            .paint(Paint {
                fill: Role::Dim,
                border: None,
                radius: 1.5,
            })
            .align(Align::End)
            .offset(0.0, 1.0);
        let ws = Node::layer(vec![text("2", 12.5).size(Some(24.0), Some(20.0)), dot]);
        let l = layout(&ws, Point::default(), &Fake);
        assert_eq!(l.size, Size { w: 24.0, h: 20.0 });
        let r = rects(&l);
        // Text first, then the dot centred horizontally, 1 DIP below the bottom edge.
        assert_eq!(r[1], Rect::new(10.5, 18.0, 3.0, 3.0));
    }

    #[test]
    fn text_is_centred_in_a_fixed_box() {
        let n = text("1", 12.5).size(Some(40.0), Some(20.0));
        let r = rects(&layout(&n, Point::default(), &Fake))[0];
        assert_eq!(r, Rect::new(16.0, (20.0 - 15.625) / 2.0, 8.0, 15.625));
    }

    #[test]
    fn damage_covers_only_what_changed() {
        let row = |cpu: &str| Node::row(vec![text("1", 10.0), text(cpu, 10.0), text("ram", 10.0)]).gap(4.0);
        let a = layout(&row("12%"), Point::default(), &Fake);
        assert_eq!(damage(&a, &a.clone()), Damage::None);
        // Same width: only the second text.
        let b = layout(&row("13%"), Point::default(), &Fake);
        let Damage::Area(r) = damage(&a, &b) else { panic!() };
        assert_eq!(
            r,
            Rect::new(
                12.0 - DAMAGE_MARGIN,
                -DAMAGE_MARGIN,
                24.0 + 2.0 * DAMAGE_MARGIN,
                12.5 + 2.0 * DAMAGE_MARGIN
            )
        );
        // Wider text moves the next one too: both old and new boxes are covered.
        let c = layout(&row("100%"), Point::default(), &Fake);
        let Damage::Area(r) = damage(&a, &c) else { panic!() };
        assert!(r.right() >= rects(&c)[2].right() && r.x <= rects(&a)[1].x);
        // A module appearing changes the structure.
        let d = layout(&Node::row(vec![text("1", 10.0)]), Point::default(), &Fake);
        assert_eq!(damage(&a, &d), Damage::Full);
    }

    #[test]
    fn layer_children_can_fill_an_axis() {
        let marker = Node::spacer(0.0, 2.0)
            .paint(Paint {
                fill: Role::Accent,
                border: None,
                radius: 0.0,
            })
            .align(Align::End)
            .fill(Axis::Row, 6.0);
        let ws = Node::layer(vec![
            text("12", 10.0)
                .size(None, Some(30.0))
                .padding(Insets::symmetric(8.0, 0.0)),
            marker,
        ]);
        let l = layout(&ws, Point::default(), &Fake);
        assert_eq!(rects(&l)[1], Rect::new(6.0, 28.0, 32.0 - 12.0, 2.0));
    }

    #[test]
    fn spacer_takes_space_but_draws_nothing() {
        let l = layout(
            &Node::row(vec![Node::spacer(10.0, 4.0), text("a", 10.0)]),
            Point::default(),
            &Fake,
        );
        assert_eq!(l.draws.len(), 1);
        assert_eq!(rects(&l)[0].x, 10.0);
    }
}
