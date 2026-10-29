//! What the overview draws on a monitor: the desk colour over the work area, a panel per
//! workspace cell (the shown one framed with the accent, as the looks mark state) with
//! its number under it (as Windows' Task View names its desktops) and, in the cells of
//! hidden workspaces, a box per window in its place with the program's icon and title.
//! The live thumbnails of the shown workspace go on top (DWM, not here), inside its frame.
//! The design page has no overview: sizes follow the tray tooltip (D21). Pure.

use kabuk_ui::geom::{Insets, Rect, Size};
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

use crate::bar::tooltip::TIP;

/// Hit-test id of a cell; the value is the workspace's index (0-based).
pub const HIT: &str = "overview-cell";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewStyle {
    /// Between cells and from the work area's edge (DIPs).
    pub gap: f32,
    pub radius: f32,
    pub border: f32,
    /// The shown workspace's frame.
    pub active_border: f32,
    /// The ring round the cell the keyboard selected, and its distance from the cell.
    pub focus: f32,
    pub focus_gap: f32,
    pub icon: f32,
    pub text: f32,
    pub pad: f32,
}

/// Cells a few tooltip gaps apart; window boxes with a tooltip's padding and text, and
/// an icon of the bar's app size.
pub const STYLE: OverviewStyle = OverviewStyle {
    gap: TIP.gap * 4.0,
    radius: TIP.radius,
    border: TIP.border,
    active_border: TIP.border * 2.0,
    focus: TIP.border * 2.0,
    focus_gap: TIP.gap / 2.0,
    icon: 16.0,
    text: TIP.text,
    pad: TIP.pad_v,
};

/// A window of a hidden workspace: its place (DIPs, in the overview), its picture and title.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowBox {
    pub rect: Rect,
    pub image: Option<u64>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub rect: Rect,
    /// The workspace's number, written under the cell.
    pub number: u32,
    /// The user's name for it, written after the number (PLAN M19 U20).
    pub name: Option<String>,
    pub shown: bool,
    pub boxes: Vec<WindowBox>,
}

/// Between a cell's number and its name, as the bar's groups are set apart.
const NAME_GLYPH: &str = "\u{00b7}";

/// The number under a cell, and its name: centred in the gap below it, in the accent
/// when shown, cut to the cell's width.
fn label(c: &Cell, s: &OverviewStyle, text_scale: f32) -> Node {
    let text = match &c.name {
        Some(name) => format!("{} {NAME_GLYPH} {name}", c.number),
        None => c.number.to_string(),
    };
    Node::text(TextSpec {
        spans: vec![Span {
            text,
            color: if c.shown { Role::Accent } else { Role::Dim },
            weight: if c.shown { 600 } else { 400 },
            after: 0.0,
            underline: false,
            italic: false,
            spacing: 0.0,
        }],
        font: FontRole::Ui,
        size: s.text * text_scale,
        max_width: Some(c.rect.w),
    })
}

/// `node` given the size of `r` and moved to `r` inside a layer of `parent` size (a
/// layer centres its children across and puts them at its top, then moves them).
fn at(node: Node, r: Rect, parent: Size) -> Node {
    node.size(Some(r.w), Some(r.h))
        .align(Align::Start)
        .offset(r.x - (parent.w - r.w) / 2.0, r.y)
}

fn window_box(b: &WindowBox, s: &OverviewStyle, text_scale: f32) -> Node {
    let mut row = Vec::new();
    // A box too small for the icon shows only its colour.
    if b.rect.w >= s.icon + 2.0 * s.pad && b.rect.h >= s.icon + 2.0 * s.pad {
        if let Some(key) = b.image {
            row.push(Node::image(key, s.icon));
        }
        let room = b.rect.w - 2.0 * s.pad - s.icon - s.pad;
        if room > s.text * 2.0 {
            row.push(Node::text(TextSpec {
                spans: vec![Span {
                    text: b.title.clone(),
                    color: Role::Fg,
                    weight: 400,
                    after: 0.0,
                    underline: false,
                    italic: false,
                    spacing: 0.0,
                }],
                font: FontRole::Ui,
                size: s.text * text_scale,
                max_width: Some(room),
            }));
        }
    }
    Node::row(row)
        .gap(s.pad)
        .align(Align::Center)
        .padding(Insets::all(s.pad))
        .paint(Paint {
            fill: Role::SurfaceAlt,
            border: Some((Role::SurfaceLine, s.border)),
            radius: s.radius,
        })
}

/// The overview of a work area of `size` DIPs; the cell the keyboard selected (by index)
/// ringed in the text colour, apart from the accent frame of the shown one.
pub fn scene(size: Size, cells: &[Cell], selected: Option<usize>, s: &OverviewStyle, text_scale: f32) -> Node {
    let mut layers = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        let frame = if c.shown {
            (Role::Accent, s.active_border)
        } else {
            (Role::SurfaceLine, s.border)
        };
        let panel = Node::spacer(0.0, 0.0)
            .paint(Paint {
                fill: Role::Surface,
                border: Some(frame),
                radius: s.radius,
            })
            .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX));
        layers.push(at(panel, c.rect, size));
        for b in &c.boxes {
            layers.push(at(window_box(b, s, text_scale), b.rect, size));
        }
        if selected == Some(i) {
            let out = s.focus_gap + s.focus;
            let ring = Rect::new(
                c.rect.x - out,
                c.rect.y - out,
                c.rect.w + 2.0 * out,
                c.rect.h + 2.0 * out,
            );
            let paint = Paint {
                fill: Role::Clear,
                border: Some((Role::Fg, s.focus)),
                radius: s.radius + out,
            };
            layers.push(at(Node::spacer(0.0, 0.0).paint(paint), ring, size));
        }
        let under = Rect::new(c.rect.x, c.rect.y + c.rect.h, c.rect.w, s.gap);
        layers.push(at(Node::layer(vec![label(c, s, text_scale)]), under, size));
    }
    Node::layer(layers).size(Some(size.w), Some(size.h)).paint(Paint {
        fill: Role::Desk,
        border: None,
        radius: 0.0,
    })
}

#[cfg(test)]
mod tests {
    //! Edge cases: cells land where they were put and are clickable by index; a box too
    //! small for its icon or title; the shown cell's frame; the keyboard's ring on any
    //! cell, the shown one too, and on none.
    use super::*;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Kind, TextMeasure, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars: usize = t.spans.iter().map(|s| s.text.chars().count()).sum();
            let w = chars as f32 * 7.0;
            TextMetrics {
                width: t.max_width.map_or(w, |m| w.min(m)),
                height: 16.0,
                baseline: 12.0,
            }
        }
    }

    const SIZE: Size = Size { w: 1000.0, h: 600.0 };

    fn cells() -> Vec<Cell> {
        vec![
            Cell {
                rect: Rect::new(20.0, 20.0, 470.0, 270.0),
                number: 1,
                name: None,
                shown: true,
                boxes: Vec::new(),
            },
            Cell {
                rect: Rect::new(510.0, 20.0, 470.0, 270.0),
                number: 3,
                name: None,
                shown: false,
                boxes: vec![WindowBox {
                    rect: Rect::new(515.0, 25.0, 200.0, 120.0),
                    image: Some(7),
                    title: "Notes".into(),
                }],
            },
        ]
    }

    #[test]
    fn cells_land_in_their_places_and_are_clickable() {
        let l = layout(&scene(SIZE, &cells(), None, &STYLE, 1.0), Point::default(), &Fake);
        assert_eq!((l.size.w, l.size.h), (SIZE.w, SIZE.h));
        let at = |x: f32, y: f32| l.hit_test(Point { x, y }).filter(|h| h.module == HIT).map(|h| h.value);
        assert_eq!(at(30.0, 30.0), Some(0));
        assert_eq!(at(970.0, 280.0), Some(1));
        assert_eq!(at(500.0, 30.0), None, "between cells");
        assert_eq!(at(500.0, 500.0), None, "below the cells");
    }

    #[test]
    fn boxes_show_what_fits_and_the_shown_cell_is_framed() {
        let s = &STYLE;
        let big = window_box(&cells()[1].boxes[0], s, 1.0);
        let Kind::Box { children, .. } = &big.kind else {
            panic!("a row")
        };
        assert_eq!(children.len(), 2, "icon and title");
        let tiny = window_box(
            &WindowBox {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                image: Some(7),
                title: "Notes".into(),
            },
            s,
            1.0,
        );
        let Kind::Box { children, .. } = &tiny.kind else {
            panic!("a row")
        };
        assert!(children.is_empty());
        let node = scene(SIZE, &cells(), None, s, 1.0);
        let Kind::Layer { children } = &node.kind else {
            panic!("a layer")
        };
        assert_eq!(children[0].paint.unwrap().border, Some((Role::Accent, s.active_border)));
        // The first cell: its panel, its number; the second: its panel, a box, its number.
        assert_eq!(children[2].paint.unwrap().border, Some((Role::SurfaceLine, s.border)));
    }

    #[test]
    fn the_cell_the_keyboard_selected_is_ringed() {
        let rings = |selected| -> Vec<Rect> {
            layout(&scene(SIZE, &cells(), selected, &STYLE, 1.0), Point::default(), &Fake)
                .draws
                .iter()
                .filter_map(|d| match d {
                    kabuk_ui::node::Draw::Panel { rect, paint } if paint.border == Some((Role::Fg, STYLE.focus)) => {
                        Some(*rect)
                    }
                    _ => None,
                })
                .collect()
        };
        assert!(rings(None).is_empty());
        let out = STYLE.focus_gap + STYLE.focus;
        for (i, c) in cells().iter().enumerate() {
            let r = rings(Some(i));
            assert_eq!(r.len(), 1, "cell {i}");
            // Round the cell, outside it, within the gap to the next one.
            assert_eq!(
                r[0],
                Rect::new(
                    c.rect.x - out,
                    c.rect.y - out,
                    c.rect.w + 2.0 * out,
                    c.rect.h + 2.0 * out
                )
            );
            assert!(out < STYLE.gap / 2.0);
        }
        assert!(rings(Some(9)).is_empty(), "a cell that is not there");
        // The ring takes no clicks from the cell.
        let l = layout(&scene(SIZE, &cells(), Some(0), &STYLE, 1.0), Point::default(), &Fake);
        assert_eq!(l.hit_test(Point { x: 30.0, y: 30.0 }).map(|h| h.value), Some(0));
    }

    /// Each cell is numbered under itself with its workspace's number (a cell's place
    /// is not its number: workspaces 1 and 3 here), the shown one in the accent.
    #[test]
    fn every_cell_is_numbered_under_itself() {
        let l = layout(&scene(SIZE, &cells(), None, &STYLE, 1.0), Point::default(), &Fake);
        let numbers: Vec<(String, Role, f32, f32)> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                kabuk_ui::node::Draw::Text { spec, rect } => {
                    Some((spec.spans[0].text.clone(), spec.spans[0].color, rect.x, rect.y))
                }
                _ => None,
            })
            .filter(|(t, ..)| t.len() == 1)
            .collect();
        assert_eq!(numbers.len(), 2);
        let (one, three) = (&numbers[0], &numbers[1]);
        assert_eq!((one.0.as_str(), one.1), ("1", Role::Accent));
        assert_eq!((three.0.as_str(), three.1), ("3", Role::Dim));
        // Under the cells (which end at y = 290), centred under each (a digit is 7 wide).
        assert!(one.3 >= 290.0 && three.3 >= 290.0);
        assert!((one.2 + 3.5 - (20.0 + 235.0)).abs() < 1.0, "{one:?}");
        assert!((three.2 + 3.5 - (510.0 + 235.0)).abs() < 1.0, "{three:?}");
    }

    /// PLAN M19 U20: a workspace the user named has its name after its number.
    #[test]
    fn a_named_cell_has_its_name_after_its_number() {
        let mut named = cells();
        named[1].name = Some("Web".into());
        let l = layout(&scene(SIZE, &named, None, &STYLE, 1.0), Point::default(), &Fake);
        let texts: Vec<String> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                kabuk_ui::node::Draw::Text { spec, .. } => Some(spec.spans[0].text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"3 \u{00b7} Web".to_string()), "{texts:?}");
        assert!(texts.contains(&"1".to_string()), "{texts:?}");
    }
}
