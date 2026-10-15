//! What the app grid draws, pure. Two layers, each in a window of its own (DECISIONS
//! D56): under, the chrome (the blurred wallpaper, the search field, the page dots, the
//! words for no match), which only fades; over it, a strip of pages side by side, which
//! zooms as it comes and goes and slides from page to page. Both are the monitor's size;
//! the strip is three pages wide.

use std::collections::HashMap;

use kabuk_i18n::I18n;
use kabuk_ui::geom::{Insets, Rect, Size};
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use super::model::Grid;
use super::style;

/// Hit-test ids: an app (the value is its place among those shown), a page dot (the
/// value is the page) and the search field.
pub mod hit {
    pub const APP: &str = "grid-app";
    pub const DOT: &str = "grid-dot";
    pub const SEARCH: &str = "grid-search";
    /// Editing: an app's badge that hides it (the value is its place), and Done.
    pub const HIDE: &str = "grid-hide";
    pub const DONE: &str = "grid-done";
}

/// An app being moved: from its place to the place it will drop at, and the mouse (DIPs
/// on the monitor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub from: usize,
    pub to: usize,
    pub at: kabuk_ui::geom::Point,
}

/// The sign on the badge that hides an app.
const MINUS: &str = "−";

/// What a drawing needs besides the grid.
pub struct Frame<'a> {
    /// The monitor (DIPs).
    pub size: Size,
    pub i18n: &'a I18n,
    pub text_scale: f32,
    /// The apps' pictures loaded so far, by what opening them runs.
    pub pictures: &'a HashMap<String, u64>,
    /// The blurred wallpaper's picture; `None` draws the look's desk colour.
    pub backdrop: Option<u64>,
    /// The user edits the grid: each app shows a badge that hides it, Done stands where
    /// the search field is.
    pub editing: bool,
    pub drag: Option<Drag>,
    /// Seconds into editing, while the icons wiggle (`None`: still).
    pub wiggle: Option<f32>,
    /// Apps sliding to new places: where each was (by what opens it), and how far along
    /// they are (0-1, eased).
    pub reflow: Option<(&'a HashMap<String, Rect>, f32)>,
}

/// Where each app of page `p` is drawn (by what opens it): its cell, the gap a dragged
/// app makes taken into account; the dragged app under the mouse.
pub fn places(grid: &Grid, p: usize, drag: Option<Drag>, size: Size) -> HashMap<String, Rect> {
    let cells: Vec<Option<(usize, usize)>> = match drag {
        Some(d) => grid.on_page_dragging(p, d.from, d.to),
        None => grid.on_page(p).into_iter().map(Some).collect(),
    };
    let mut out: HashMap<String, Rect> = cells
        .into_iter()
        .enumerate()
        .filter_map(|(local, place)| {
            let (_, index) = place?;
            Some((grid.entry(index)?.open.clone(), cell(grid, local, size)))
        })
        .collect();
    if let Some(d) = drag {
        if let Some(e) = grid.index_at(d.from).and_then(|i| grid.entry(i)) {
            let r = cell(grid, 0, size);
            out.insert(
                e.open.clone(),
                Rect::new(d.at.x - r.w / 2.0, d.at.y - r.h / 2.0, r.w, r.h),
            );
        }
    }
    out
}

/// `a` moved `t` (0-1) of the way to `b`.
pub fn between(a: Rect, b: Rect, t: f32) -> Rect {
    let m = |x: f32, y: f32| x + (y - x) * t;
    Rect::new(m(a.x, b.x), m(a.y, b.y), m(a.w, b.w), m(a.h, b.h))
}

/// The eased share (0-1) of a slide `elapsed` seconds into it.
pub fn eased(elapsed: f32) -> f32 {
    let t = (elapsed / style::REFLOW_SECONDS).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn text(words: String, color: Role, size: f32, max_width: Option<f32>) -> Node {
    Node::text(TextSpec {
        spans: vec![Span {
            text: words,
            color,
            weight: 400,
            after: 0.0,
            underline: false,
        }],
        font: FontRole::Ui,
        size,
        max_width,
    })
}

/// `node` given the size of `r` and moved to `r` inside a layer of `parent` size (a
/// layer centres its children across and puts them at its top, then moves them).
fn at(node: Node, r: Rect, parent: Size) -> Node {
    node.size(Some(r.w), Some(r.h))
        .align(Align::Start)
        .offset(r.x - (parent.w - r.w) / 2.0, r.y)
}

/// Where the cell of an entry `local`-th on its page is.
pub fn cell(grid: &Grid, local: usize, size: Size) -> Rect {
    let shape = grid.shape();
    let w = (size.w - 2.0 * style::SIDE) / shape.columns as f32;
    let h = (size.h - style::TOP - style::BOTTOM) / shape.rows as f32;
    let (col, row) = (local % shape.columns, local / shape.columns);
    Rect::new(style::SIDE + col as f32 * w, style::TOP + row as f32 * h, w, h)
}

/// The cell of the page on screen under `p` (DIPs on the monitor), counted on the page;
/// a point outside the cells gives the nearest one.
pub fn cell_at(grid: &Grid, p: kabuk_ui::geom::Point, size: Size) -> usize {
    let shape = grid.shape();
    let w = (size.w - 2.0 * style::SIDE) / shape.columns as f32;
    let h = (size.h - style::TOP - style::BOTTOM) / shape.rows as f32;
    let along =
        |v: f32, start: f32, step: f32, n: usize| ((v - start) / step).floor().clamp(0.0, (n - 1) as f32) as usize;
    let (col, row) = (
        along(p.x, style::SIDE, w, shape.columns),
        along(p.y, style::TOP, h, shape.rows),
    );
    row * shape.columns + col
}

/// An app's icon and name; framed when the keyboard selected it. While editing, a badge
/// over its icon's corner hides it.
fn app(grid: &Grid, at_shown: usize, index: usize, f: &Frame) -> Node {
    let Some(e) = grid.entry(index) else {
        return Node::spacer(0.0, 0.0);
    };
    let picture = match f.pictures.get(&e.open) {
        Some(key) => {
            let turn = f.wiggle.map_or(0.0, |s| super::model::wiggle(s, at_shown));
            Node::image(*key, style::ICON).turned(turn)
        }
        None => Node::layer(vec![Node::icon("app", style::PLACEHOLDER_GLYPH, Role::White)])
            .size(Some(style::ICON), Some(style::ICON))
            .paint(Paint {
                fill: Role::Glass,
                border: Some((Role::GlassLine, 1.0)),
                radius: style::PLACEHOLDER_RADIUS,
            }),
    };
    let icon = if f.editing {
        // A layer puts its children at its top, centred across: the badge goes right
        // (asked for by the user: on the icon's top-right corner).
        let over = style::BADGE * style::BADGE_OVER;
        let badge = Node::layer(vec![text(MINUS.into(), Role::White, style::BADGE_TEXT, None)])
            .size(Some(style::BADGE), Some(style::BADGE))
            .paint(Paint {
                fill: Role::Glass,
                border: Some((Role::GlassLine, 1.0)),
                radius: style::BADGE / 2.0,
            })
            .offset((style::ICON - style::BADGE) / 2.0 + over, -over)
            .hit(hit::HIDE, u32::try_from(at_shown).unwrap_or(u32::MAX));
        Node::layer(vec![picture, badge]).size(Some(style::ICON), Some(style::ICON))
    } else {
        picture
    };
    let label = text(
        e.name.clone(),
        Role::White,
        style::LABEL * f.text_scale,
        Some(style::LABEL_WIDTH),
    );
    let content = Node::column(vec![icon, label])
        .gap(style::LABEL_GAP)
        .align(Align::Center)
        .padding(Insets::all(style::SELECTION_PAD))
        .hit(hit::APP, u32::try_from(at_shown).unwrap_or(u32::MAX));
    if grid.selected() == Some(at_shown) {
        content.paint(Paint {
            fill: Role::Glass,
            border: Some((Role::GlassLine, 1.0)),
            radius: style::SELECTION_RADIUS,
        })
    } else {
        content
    }
}

/// Page `p` of the grid, the monitor's size; `None` an empty page (past the ends). While
/// an app is dragged, the others close up around the gap it will drop into, and on the
/// page on screen it follows the mouse.
pub fn page(grid: &Grid, p: Option<usize>, f: &Frame) -> Node {
    let mut layers = vec![Node::spacer(f.size.w, f.size.h)];
    let Some(p) = p else {
        return Node::layer(layers);
    };
    let cells: Vec<Option<(usize, usize)>> = match f.drag {
        Some(d) => grid.on_page_dragging(p, d.from, d.to),
        None => grid.on_page(p).into_iter().map(Some).collect(),
    };
    for (local, place) in cells.into_iter().enumerate() {
        let Some((at_shown, index)) = place else { continue };
        let mut r = cell(grid, local, f.size);
        if let (Some((from, t)), Some(e)) = (f.reflow, grid.entry(index)) {
            if let Some(was) = from.get(&e.open) {
                r = between(*was, r, t);
            }
        }
        layers.push(at(Node::layer(vec![app(grid, at_shown, index, f)]), r, f.size));
    }
    if let Some(d) = f.drag.filter(|_| p == grid.page()) {
        if let Some(index) = grid.index_at(d.from) {
            let r = cell(grid, 0, f.size);
            let moving = Rect::new(d.at.x - r.w / 2.0, d.at.y - r.h / 2.0, r.w, r.h);
            layers.push(at(Node::layer(vec![app(grid, d.from, index, f)]), moving, f.size));
        }
    }
    Node::layer(layers)
}

/// The strip: the pages in `slots` side by side, left to right.
pub fn strip(grid: &Grid, slots: [Option<usize>; 3], f: &Frame) -> Node {
    Node::row(slots.into_iter().map(|p| page(grid, p, f)).collect())
}

/// The search field: the typed words, or the placeholder. Once clicked or typed into it
/// shows that it takes the keys: its frame in the accent colour and a caret.
fn search(grid: &Grid, f: &Frame) -> Node {
    let active = grid.searching();
    let caret_w = if active { style::CARET } else { 0.0 };
    let room = style::SEARCH.w - 2.0 * style::SEARCH_PAD - style::SEARCH_ICON - style::SEARCH_GAP - caret_w;
    let size = style::SEARCH_TEXT * f.text_scale;
    let (words, color) = if grid.query().is_empty() {
        (f.i18n.tr("grid-search"), Role::Dim)
    } else {
        (grid.query().to_string(), Role::White)
    };
    let words = text(words, color, size, Some(room));
    let typed = if active {
        let caret = Node::spacer(style::CARET, size * style::CARET_LINE).paint(Paint {
            fill: Role::Accent,
            border: None,
            radius: 0.0,
        });
        // The caret sits before the placeholder, as in a text box.
        let row = if grid.query().is_empty() {
            vec![caret, words]
        } else {
            vec![words, caret]
        };
        Node::row(row).align(Align::Center)
    } else {
        words
    };
    let border = if active {
        (Role::Accent, style::SEARCH_ACTIVE_BORDER)
    } else {
        (Role::GlassLine, 1.0)
    };
    Node::row(vec![Node::icon("search", style::SEARCH_ICON, Role::White), typed])
        .gap(style::SEARCH_GAP)
        .align(Align::Center)
        .padding(Insets::symmetric(style::SEARCH_PAD, 0.0))
        .paint(Paint {
            fill: Role::Glass,
            border: Some(border),
            radius: style::SEARCH_RADIUS,
        })
        .hit(hit::SEARCH, 0)
}

/// A dot per page, the shown one bright; none for a single page.
fn dots(grid: &Grid) -> Option<Node> {
    let pages = grid.pages();
    if pages < 2 {
        return None;
    }
    let list = (0..pages)
        .map(|p| {
            let dot = Node::spacer(style::DOT, style::DOT).paint(Paint {
                fill: if p == grid.page() { Role::White } else { Role::Dim },
                border: None,
                radius: style::DOT / 2.0,
            });
            Node::layer(vec![dot])
                .padding(Insets::all(style::DOT_HIT))
                .hit(hit::DOT, u32::try_from(p).unwrap_or(u32::MAX))
        })
        .collect();
    Some(Node::row(list).gap(style::DOT_GAP).align(Align::Center))
}

/// The chrome under the pages.
pub fn chrome(grid: &Grid, f: &Frame) -> Node {
    let base = match f.backdrop {
        Some(key) => Node::picture(key, f.size.w, f.size.h),
        None => Node::spacer(f.size.w, f.size.h).paint(Paint {
            fill: Role::Desk,
            border: None,
            radius: 0.0,
        }),
    };
    let mut layers = vec![base];
    let field = Rect::new(
        (f.size.w - style::SEARCH.w) / 2.0,
        style::SEARCH_TOP,
        style::SEARCH.w,
        style::SEARCH.h,
    );
    if f.editing {
        // Done where the search field is, and what to do under it.
        let done = Rect::new(
            (f.size.w - style::DONE_WIDTH) / 2.0,
            field.y,
            style::DONE_WIDTH,
            field.h,
        );
        let button = Node::layer(vec![text(
            f.i18n.tr("grid-done"),
            Role::OnAccent,
            style::SEARCH_TEXT * f.text_scale,
            None,
        )])
        .size(Some(done.w), Some(done.h))
        .paint(Paint {
            fill: Role::AccentFill,
            border: None,
            radius: style::SEARCH_RADIUS,
        })
        .hit(hit::DONE, 0);
        layers.push(at(Node::layer(vec![button]), done, f.size));
        let hint = text(
            f.i18n.tr("grid-edit-h"),
            Role::White,
            style::EDIT_HINT_TEXT * f.text_scale,
            None,
        );
        layers.push(hint.offset(0.0, field.y + field.h + style::EDIT_HINT_GAP));
    } else {
        // In a layer of its own: placed by `at`, the field itself keeps its contents
        // centred across (found by the user: its icon and words sat at its top).
        let sized = search(grid, f).size(Some(field.w), Some(field.h));
        layers.push(at(Node::layer(vec![sized]), field, f.size));
    }
    if let Some(d) = dots(grid) {
        layers.push(Node::column(vec![d]).align(Align::End).offset(0.0, -style::DOTS_BOTTOM));
    }
    if grid.count() == 0 {
        layers.push(text(
            f.i18n.tr("grid-empty"),
            Role::White,
            style::EMPTY_TEXT * f.text_scale,
            None,
        ));
    }
    Node::layer(layers)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a page's apps in their cells, left to right then down, each clickable
    //! with its place; the selection framed; a picture once loaded, a tile before; an
    //! empty slot; the strip three pages wide; the chrome's search (placeholder and typed
    //! words), its dots (none for one page, the shown one bright, each clickable), the
    //! words for no match, the wallpaper or the desk colour.
    use super::*;
    use crate::appgrid::model::{Entry, Shape};
    use kabuk_i18n::Lang;
    use kabuk_search::Casing;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, Layout, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, spec: &TextSpec) -> TextMetrics {
            let n: usize = spec.spans.iter().map(|s| s.text.chars().count()).sum();
            TextMetrics {
                width: n as f32 * spec.size * 0.5,
                height: spec.size * 1.2,
                baseline: spec.size,
            }
        }
    }

    const SIZE: Size = Size { w: 1600.0, h: 900.0 };
    const SHAPE: Shape = Shape { columns: 3, rows: 2 };

    fn grid(n: usize) -> Grid {
        let entries = (0..n)
            .map(|i| Entry {
                name: format!("App {i}"),
                open: format!("open {i}"),
            })
            .collect();
        Grid::new(entries, Casing::Default, SHAPE)
    }

    fn hits(l: &Layout, module: &str) -> Vec<(Rect, u32)> {
        l.hits
            .iter()
            .filter(|(_, h)| h.module == module)
            .map(|(r, h)| (*r, h.value))
            .collect()
    }

    fn texts(l: &Layout) -> Vec<String> {
        l.draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.spans.iter().map(|s| s.text.as_str()).collect()),
                _ => None,
            })
            .collect()
    }

    fn frame<'a>(i18n: &'a I18n, pictures: &'a HashMap<String, u64>, backdrop: Option<u64>) -> Frame<'a> {
        Frame {
            size: SIZE,
            i18n,
            text_scale: 1.0,
            pictures,
            backdrop,
            editing: false,
            drag: None,
            wiggle: None,
            reflow: None,
        }
    }

    #[test]
    fn a_page_puts_its_apps_in_their_cells() {
        let (i18n, none) = (I18n::new(Lang::En), HashMap::new());
        let g = grid(10);
        let l = layout(&page(&g, Some(1), &frame(&i18n, &none, None)), Point::default(), &Fake);
        assert_eq!((l.size.w, l.size.h), (SIZE.w, SIZE.h));
        let apps = hits(&l, hit::APP);
        assert_eq!(apps.iter().map(|(_, v)| *v).collect::<Vec<_>>(), [6, 7, 8, 9]);
        // Left to right, then down; each inside its cell.
        for (local, (r, _)) in apps.iter().enumerate() {
            let c = cell(&g, local, SIZE);
            assert!(
                c.contains(Point {
                    x: r.x + r.w / 2.0,
                    y: r.y + r.h / 2.0
                }),
                "{local}"
            );
        }
        assert!(apps[0].0.x < apps[1].0.x && apps[2].0.y < apps[3].0.y);
        assert_eq!(texts(&l), ["App 6", "App 7", "App 8", "App 9"]);
        let empty = layout(&page(&g, None, &frame(&i18n, &none, None)), Point::default(), &Fake);
        assert!(hits(&empty, hit::APP).is_empty());
        assert_eq!(empty.size.w, SIZE.w);
    }

    #[test]
    fn pictures_replace_tiles_and_the_selection_is_framed() {
        let i18n = I18n::new(Lang::En);
        let pictures = HashMap::from([("open 1".to_string(), 42u64)]);
        let mut g = grid(3);
        g.step(crate::appgrid::model::Dir::Right);
        let l = layout(
            &page(&g, Some(0), &frame(&i18n, &pictures, None)),
            Point::default(),
            &Fake,
        );
        let images: Vec<u64> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Image { spec, .. } => Some(spec.key),
                _ => None,
            })
            .collect();
        assert_eq!(images, [42]);
        let tiles = l
            .draws
            .iter()
            .filter(|d| matches!(d, Draw::Icon { spec, .. } if spec.name == "app"))
            .count();
        assert_eq!(tiles, 2);
        // Entry 0 is selected: one framed box, around its icon and name.
        let framed: Vec<Rect> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Panel { rect, paint } if paint.radius == style::SELECTION_RADIUS => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(framed, [hits(&l, hit::APP)[0].0]);
    }

    #[test]
    fn the_strip_is_three_pages_wide() {
        let (i18n, none) = (I18n::new(Lang::En), HashMap::new());
        let g = grid(10);
        let l = layout(
            &strip(&g, [None, Some(0), Some(1)], &frame(&i18n, &none, None)),
            Point::default(),
            &Fake,
        );
        assert_eq!(l.size.w, 3.0 * SIZE.w);
        let apps = hits(&l, hit::APP);
        assert_eq!(apps.len(), 10);
        assert!(apps.iter().all(|(r, _)| r.x >= SIZE.w), "nothing in the empty slot");
        assert!(apps.iter().any(|(r, v)| *v == 6 && r.x >= 2.0 * SIZE.w));
    }

    #[test]
    fn the_chrome_has_the_search_the_dots_and_the_wallpaper() {
        let (i18n, none) = (I18n::new(Lang::En), HashMap::new());
        let mut g = grid(10);
        let l = layout(&chrome(&g, &frame(&i18n, &none, Some(9))), Point::default(), &Fake);
        assert_eq!((l.size.w, l.size.h), (SIZE.w, SIZE.h));
        assert!(matches!(&l.draws[0], Draw::Image { spec, rect } if spec.key == 9 && rect.w == SIZE.w));
        assert!(texts(&l).contains(&i18n.tr("grid-search")));
        let search = hits(&l, hit::SEARCH);
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].0.y, style::SEARCH_TOP);
        // Its icon and words in its middle, top to bottom (found by the user: at its top).
        let field = search[0].0;
        let middle = |r: &Rect| r.y + r.h / 2.0;
        let inside: Vec<Rect> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Icon { rect, spec } if spec.name == "search" => Some(*rect),
                Draw::Text { rect, spec } if spec.spans[0].text == i18n.tr("grid-search") => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(inside.len(), 2);
        for r in &inside {
            assert!((middle(r) - middle(&field)).abs() < 1.0, "{r:?} in {field:?}");
        }
        assert!(
            (search[0].0.x + search[0].0.w / 2.0 - SIZE.w / 2.0).abs() < 0.5,
            "centred"
        );
        let dots = hits(&l, hit::DOT);
        assert_eq!(dots.iter().map(|(_, v)| *v).collect::<Vec<_>>(), [0, 1]);
        assert!(dots.iter().all(|(r, _)| r.y + r.h <= SIZE.h - style::DOTS_BOTTOM + 0.5));
        let bright = |l: &Layout| {
            l.draws
                .iter()
                .filter(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::White))
                .count()
        };
        assert_eq!(bright(&l), 1);
        // Typing: the words show, nothing matches, no dots, the words for no match.
        g.type_text("zzz");
        let l = layout(&chrome(&g, &frame(&i18n, &none, None)), Point::default(), &Fake);
        let t = texts(&l);
        assert!(t.contains(&"zzz".to_string()) && t.contains(&i18n.tr("grid-empty")));
        assert!(hits(&l, hit::DOT).is_empty());
        assert!(matches!(&l.draws[0], Draw::Panel { paint, .. } if paint.fill == Role::Desk));
    }

    /// Found by the user: a click on the search field did nothing one could see. Before,
    /// no caret and a plain frame; once clicked, the accent frame and a caret before the
    /// placeholder; typed into, the caret after the words, all in the field's middle.
    #[test]
    fn the_search_field_shows_when_it_takes_the_keys() {
        let i18n = I18n::new(Lang::En);
        let none = HashMap::new();
        let mut g = grid(0);
        let field = |l: &Layout| {
            l.draws
                .iter()
                .find_map(|d| match d {
                    Draw::Panel { rect, paint, .. }
                        if paint.border.is_some_and(|(_, w)| w > 0.0) && rect.y == style::SEARCH_TOP =>
                    {
                        Some((*rect, *paint))
                    }
                    _ => None,
                })
                .expect("the field")
        };
        let caret = |l: &Layout| {
            l.draws.iter().find_map(|d| match d {
                Draw::Panel { rect, paint, .. } if paint.fill == Role::Accent && rect.w == style::CARET => Some(*rect),
                _ => None,
            })
        };
        let words = |l: &Layout, w: &str| {
            l.draws.iter().find_map(|d| match d {
                Draw::Text { rect, spec } if spec.spans[0].text == w => Some(*rect),
                _ => None,
            })
        };
        let l = layout(&chrome(&g, &frame(&i18n, &none, None)), Point::default(), &Fake);
        assert_eq!(field(&l).1.border.map(|(r, _)| r), Some(Role::GlassLine));
        assert_eq!(caret(&l), None);

        assert!(g.focus_search());
        let l = layout(&chrome(&g, &frame(&i18n, &none, None)), Point::default(), &Fake);
        let (f, paint) = field(&l);
        assert_eq!(paint.border, Some((Role::Accent, style::SEARCH_ACTIVE_BORDER)));
        let c = caret(&l).expect("a caret");
        let placeholder = words(&l, &i18n.tr("grid-search")).expect("the placeholder");
        assert!(
            c.x + c.w <= placeholder.x + 0.5,
            "before the placeholder: {c:?} {placeholder:?}"
        );
        assert!(
            (c.y + c.h / 2.0 - (f.y + f.h / 2.0)).abs() < 1.0,
            "in the middle: {c:?} {f:?}"
        );

        g.type_text("ab");
        let l = layout(&chrome(&g, &frame(&i18n, &none, None)), Point::default(), &Fake);
        let c = caret(&l).expect("a caret");
        let typed = words(&l, "ab").expect("the words");
        assert!(c.x >= typed.x + typed.w - 0.5, "after the words: {c:?} {typed:?}");
        assert!(c.x + c.w <= f.x + f.w, "inside the field");
    }

    /// Asked for by the user: apps slide to their new places. Each app's place on the
    /// page (the dragged one under the mouse); a slide halfway; easing from 0 to 1; a
    /// drawn app between its old and new place.
    #[test]
    fn apps_slide_to_their_new_places() {
        let i18n = I18n::new(Lang::En);
        let g = grid(6);
        let mouse = Point { x: 900.0, y: 500.0 };
        let p = places(
            &g,
            0,
            Some(Drag {
                from: 1,
                to: 4,
                at: mouse,
            }),
            SIZE,
        );
        assert_eq!(p.len(), 6);
        assert!(p["open 1"].contains(mouse), "the dragged one under the mouse");
        assert_eq!(p["open 2"], cell(&g, 1, SIZE), "closed up into the gap");
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(100.0, 50.0, 10.0, 10.0);
        assert_eq!(between(a, b, 0.5), Rect::new(50.0, 25.0, 10.0, 10.0));
        assert_eq!((eased(0.0), eased(10.0)), (0.0, 1.0));
        assert!(eased(style::REFLOW_SECONDS / 2.0) > 0.4 && eased(style::REFLOW_SECONDS / 2.0) < 0.6);
        // Drawn mid-slide: app 2 between where it was and its cell.
        let from: HashMap<String, Rect> = [("open 2".to_string(), cell(&g, 4, SIZE))].into();
        let none = HashMap::new();
        let mut f = frame(&i18n, &none, None);
        f.reflow = Some((&from, 0.5));
        let l = layout(&page(&g, Some(0), &f), Point::default(), &Fake);
        let drawn = hits(&l, hit::APP).into_iter().find(|(_, v)| *v == 2).unwrap().0;
        let (old, new) = (cell(&g, 4, SIZE), cell(&g, 2, SIZE));
        assert!(
            drawn.x > new.x.min(old.x) && drawn.x < new.x.max(old.x) || drawn.y != new.y,
            "{drawn:?}"
        );
    }

    /// Asked for by the user: while editing, the loaded icons wiggle, each by its own
    /// turn; still, none is turned.
    #[test]
    fn edited_icons_wiggle() {
        let i18n = I18n::new(Lang::En);
        let g = grid(4);
        let pictures: HashMap<String, u64> = (0..4).map(|i| (format!("open {i}"), 100 + i)).collect();
        let turns = |wiggle: Option<f32>| -> Vec<f32> {
            let mut f = frame(&i18n, &pictures, None);
            f.editing = true;
            f.wiggle = wiggle;
            layout(&page(&g, Some(0), &f), Point::default(), &Fake)
                .draws
                .iter()
                .filter_map(|d| match d {
                    Draw::Image { spec, .. } => Some(spec.turn),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(turns(None), [0.0; 4]);
        let moving = turns(Some(0.05));
        assert_eq!(moving.len(), 4);
        assert!(moving.iter().all(|t| *t != 0.0) && moving[0] != moving[1], "{moving:?}");
        assert_eq!(moving[2], crate::appgrid::model::wiggle(0.05, 2));
    }

    /// Asked for by the user (hide, show and move the grid's apps). Editing, every app
    /// carries a badge that hides it, found before the app under it, over its icon's
    /// top-right corner; Done and the words that tell what to do take the search's place.
    #[test]
    fn editing_shows_a_badge_on_every_app_and_done() {
        let i18n = I18n::new(Lang::En);
        let none = HashMap::new();
        let g = grid(8);
        let mut f = frame(&i18n, &none, None);
        f.editing = true;
        let l = layout(&page(&g, Some(0), &f), Point::default(), &Fake);
        let badges = hits(&l, hit::HIDE);
        assert_eq!(badges.iter().map(|(_, v)| *v).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
        for (badge, at) in &badges {
            let middle = Point {
                x: badge.x + badge.w / 2.0,
                y: badge.y + badge.h / 2.0,
            };
            let found = l.hit_test(middle).expect("something");
            assert_eq!((found.module, found.value), (hit::HIDE, *at), "the badge wins");
            let app = hits(&l, hit::APP).into_iter().find(|(_, v)| v == at).unwrap().0;
            assert!(
                badge.x + badge.w > app.x + app.w / 2.0 && badge.y < app.y + app.h / 2.0,
                "top-right"
            );
        }
        let chrome_l = layout(&chrome(&g, &f), Point::default(), &Fake);
        assert!(hits(&chrome_l, hit::SEARCH).is_empty());
        assert_eq!(hits(&chrome_l, hit::DONE).len(), 1);
        let t = texts(&chrome_l);
        assert!(t.contains(&i18n.tr("grid-done")) && t.contains(&i18n.tr("grid-edit-h")));
        // Not editing: no badge, the search.
        let l = layout(&page(&g, Some(0), &frame(&i18n, &none, None)), Point::default(), &Fake);
        assert!(hits(&l, hit::HIDE).is_empty());
    }

    /// A dragged app follows the mouse over a gap where it will drop; the cell under the
    /// mouse is found on the page, the nearest one outside the cells.
    #[test]
    fn a_dragged_app_follows_the_mouse_over_its_gap() {
        let i18n = I18n::new(Lang::En);
        let none = HashMap::new();
        let g = grid(8);
        let mut f = frame(&i18n, &none, None);
        f.editing = true;
        let mouse = Point { x: 700.0, y: 400.0 };
        f.drag = Some(Drag {
            from: 1,
            to: 4,
            at: mouse,
        });
        let l = layout(&page(&g, Some(0), &f), Point::default(), &Fake);
        let apps = hits(&l, hit::APP);
        // Five in their cells (the gap at the fifth), and the dragged one last, under
        // the mouse.
        assert_eq!(apps.len(), 6);
        let (moving, value) = apps[5];
        assert_eq!(value, 1);
        assert!(moving.contains(mouse), "{moving:?}");
        let gap = cell(&g, 4, SIZE);
        assert!(apps[..5].iter().all(|(r, _)| !gap.contains(Point {
            x: r.x + r.w / 2.0,
            y: r.y + r.h / 2.0
        })));
        // Another page shows its own apps, without the dragged one.
        let l = layout(&page(&g, Some(1), &f), Point::default(), &Fake);
        assert_eq!(hits(&l, hit::APP).len(), 2);
        // The cell under the mouse.
        for local in 0..6 {
            let r = cell(&g, local, SIZE);
            let middle = Point {
                x: r.x + r.w / 2.0,
                y: r.y + r.h / 2.0,
            };
            assert_eq!(cell_at(&g, middle, SIZE), local);
        }
        assert_eq!(cell_at(&g, Point { x: 0.0, y: 0.0 }, SIZE), 0, "the nearest");
        assert_eq!(cell_at(&g, Point { x: SIZE.w, y: SIZE.h }, SIZE), 5);
    }
}
