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
}

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

/// An app's icon and name; framed when the keyboard selected it.
fn app(grid: &Grid, at_shown: usize, index: usize, f: &Frame) -> Node {
    let Some(e) = grid.entry(index) else {
        return Node::spacer(0.0, 0.0);
    };
    let icon = match f.pictures.get(&e.open) {
        Some(key) => Node::image(*key, style::ICON),
        None => Node::layer(vec![Node::icon("app", style::PLACEHOLDER_GLYPH, Role::White)])
            .size(Some(style::ICON), Some(style::ICON))
            .paint(Paint {
                fill: Role::Glass,
                border: Some((Role::GlassLine, 1.0)),
                radius: style::PLACEHOLDER_RADIUS,
            }),
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

/// Page `p` of the grid, the monitor's size; `None` an empty page (past the ends).
pub fn page(grid: &Grid, p: Option<usize>, f: &Frame) -> Node {
    let mut layers = vec![Node::spacer(f.size.w, f.size.h)];
    if let Some(p) = p {
        for (local, (at_shown, index)) in grid.on_page(p).into_iter().enumerate() {
            let r = cell(grid, local, f.size);
            layers.push(at(Node::layer(vec![app(grid, at_shown, index, f)]), r, f.size));
        }
    }
    Node::layer(layers)
}

/// The strip: the pages in `slots` side by side, left to right.
pub fn strip(grid: &Grid, slots: [Option<usize>; 3], f: &Frame) -> Node {
    Node::row(slots.into_iter().map(|p| page(grid, p, f)).collect())
}

/// The search field: the typed words, or the placeholder.
fn search(grid: &Grid, f: &Frame) -> Node {
    let room = style::SEARCH.w - 2.0 * style::SEARCH_PAD - style::SEARCH_ICON - style::SEARCH_GAP;
    let (words, color) = if grid.query().is_empty() {
        (f.i18n.tr("grid-search"), Role::Dim)
    } else {
        (grid.query().to_string(), Role::White)
    };
    Node::row(vec![
        Node::icon("search", style::SEARCH_ICON, Role::White),
        text(words, color, style::SEARCH_TEXT * f.text_scale, Some(room)),
    ])
    .gap(style::SEARCH_GAP)
    .align(Align::Center)
    .padding(Insets::symmetric(style::SEARCH_PAD, 0.0))
    .paint(Paint {
        fill: Role::Glass,
        border: Some((Role::GlassLine, 1.0)),
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
    // In a layer of its own: placed by `at`, the field itself keeps its contents centred
    // across (found by the user: its icon and words sat at its top).
    let sized = search(grid, f).size(Some(field.w), Some(field.h));
    layers.push(at(Node::layer(vec![sized]), field, f.size));
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
}
