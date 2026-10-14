//! Kabuk's mouse cursors (DECISIONS D20, D41): one drawing per cursor role on a 32-unit
//! grid, in a style and colour of the user's or the look's, the `.cur` / `.ani` files
//! Windows loads them from and the pictures Settings previews them with. Pure: the marks
//! are rasterised by [`crate::render::Gpu::render_marks`].

use kabuk_core::config;
use kabuk_theme::{Color, Tokens};

mod cartoon;
pub use cartoon::BADGES as CARTOON_BADGES;

use crate::geom::Point;
use crate::svg::{Seg, Shape};

/// Raised whenever a drawing changes, so sets drawn before are drawn again.
pub const DRAWINGS_VERSION: u32 = 2;
/// The grid the cursors are drawn on.
pub const UNITS: f32 = 32.0;
/// Sizes in every file; Windows picks one by the user's cursor size and the DPI (up to
/// the huge size, D41, at 200 %).
pub const SIZES: [u32; 5] = [32, 48, 64, 96, 128];
/// Frames of a turning ring, and how long each shows (in 1/60 s, the `.ani` unit).
pub const FRAMES: u32 = 12;
pub const FRAME_JIFFIES: u32 = 4;
/// The ring's arc, in degrees.
const ARC: f32 = 120.0;
/// How much of the body's colour the ring's track carries over the outline's, so the
/// accent arc shows even where the accent is the body's colour (Çizgi).
const TRACK_SHARE: f64 = 0.45;

/// The cursor roles Windows has, in the order of `HKCU\Control Panel\Cursors`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Arrow,
    Help,
    Working,
    Busy,
    Precision,
    Text,
    Handwriting,
    Unavailable,
    SizeNS,
    SizeWE,
    SizeNWSE,
    SizeNESW,
    Move,
    Alternate,
    Link,
    Pin,
    Person,
}

impl Role {
    pub const ALL: [Role; 17] = [
        Role::Arrow,
        Role::Help,
        Role::Working,
        Role::Busy,
        Role::Precision,
        Role::Text,
        Role::Handwriting,
        Role::Unavailable,
        Role::SizeNS,
        Role::SizeWE,
        Role::SizeNWSE,
        Role::SizeNESW,
        Role::Move,
        Role::Alternate,
        Role::Link,
        Role::Pin,
        Role::Person,
    ];

    /// Its value under `HKCU\Control Panel\Cursors`.
    pub fn registry_name(self) -> &'static str {
        match self {
            Role::Arrow => "Arrow",
            Role::Help => "Help",
            Role::Working => "AppStarting",
            Role::Busy => "Wait",
            Role::Precision => "Crosshair",
            Role::Text => "IBeam",
            Role::Handwriting => "NWPen",
            Role::Unavailable => "No",
            Role::SizeNS => "SizeNS",
            Role::SizeWE => "SizeWE",
            Role::SizeNWSE => "SizeNWSE",
            Role::SizeNESW => "SizeNESW",
            Role::Move => "SizeAll",
            Role::Alternate => "UpArrow",
            Role::Link => "Hand",
            Role::Pin => "Pin",
            Role::Person => "Person",
        }
    }

    /// The busy and working cursors turn.
    pub fn animated(self) -> bool {
        matches!(self, Role::Working | Role::Busy)
    }

    pub fn file_name(self) -> String {
        let ext = if self.animated() { "ani" } else { "cur" };
        format!("{}.{ext}", self.registry_name().to_lowercase())
    }
}

/// How a look draws its cursors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorStyle {
    pub fill: Color,
    pub outline: Color,
    /// The outline's stroke, in grid units (half of it shows outside the fill).
    pub outline_width: f32,
    /// Round joins (soft looks) or mitred ones (line and terminal looks).
    pub round: bool,
    /// The turning ring and the small badges.
    pub accent: Color,
    /// The "unavailable" sign.
    pub warning: Color,
    /// A narrower arrow (Çizgi).
    pub slim: bool,
    /// A block text cursor instead of the I-beam (Konsol).
    pub block_text: bool,
    /// The accent via near the arrow's tip (Devre).
    pub via: bool,
    /// The cartoon sets' own drawings instead of these (D41).
    pub cartoon: Option<Cartoon>,
}

/// The two cartoon sets: soft or pointed corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cartoon {
    Round,
    Sharp,
}

/// The drawing style for `style` in `color` from a look's tokens (`fg-s` and `desk` for
/// the light and the dark colour, `accent-s` for the ring and the accent body, `err-fg`
/// for "unavailable"); the outline is whichever reads better on the body.
pub fn style_for(style: config::CursorStyle, color: config::CursorColor, tokens: &Tokens) -> CursorStyle {
    use config::{CursorColor as C, CursorStyle as S};
    let pick = |name: &str, fallback: Color| tokens.color(name).unwrap_or(fallback);
    let (light, dark) = (pick("fg-s", Color::WHITE), pick("desk", Color::BLACK));
    let accent = pick("accent-s", light);
    let (fill, outline) = match color {
        C::Light => (light, dark),
        C::Dark => (dark, light),
        C::Accent => {
            use kabuk_theme::contrast::contrast;
            let on_light = contrast(rgba(accent), rgba(light));
            let on_dark = contrast(rgba(accent), rgba(dark));
            (accent, if on_dark >= on_light { dark } else { light })
        }
    };
    CursorStyle {
        fill,
        outline,
        outline_width: if style == S::Bold {
            2.0
        } else if style == S::Slim {
            1.0
        } else {
            1.5
        },
        round: matches!(style, S::Round | S::Bold),
        accent,
        warning: pick("err-fg", fill),
        slim: style == S::Slim,
        block_text: style == S::Block,
        via: style == S::Circuit,
        cartoon: match style {
            S::Cartoon => Some(Cartoon::Round),
            S::Comic => Some(Cartoon::Sharp),
            _ => None,
        },
    }
}

/// The roles Settings previews a style with: the arrow, the text cursor (where the
/// styles differ most) and the link hand.
pub const PREVIEW_ROLES: [Role; 3] = [Role::Arrow, Role::Text, Role::Link];

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn path_data(segs: &[Seg]) -> String {
    let n = |v: f32| format!("{}", (v * 100.0).round() / 100.0);
    segs.iter()
        .map(|s| match s {
            Seg::Move(p) => format!("M{} {}", n(p.x), n(p.y)),
            Seg::Line(p) => format!("L{} {}", n(p.x), n(p.y)),
            Seg::Cubic(a, b, p) => format!("C{} {} {} {} {} {}", n(a.x), n(a.y), n(b.x), n(b.y), n(p.x), n(p.y)),
            Seg::Quad(a, p) => format!("Q{} {} {} {}", n(a.x), n(a.y), n(p.x), n(p.y)),
            Seg::Arc {
                rx,
                ry,
                rotation,
                large,
                sweep,
                to,
            } => format!(
                "A{} {} {} {} {} {} {}",
                n(*rx),
                n(*ry),
                n(*rotation),
                u8::from(*large),
                u8::from(*sweep),
                n(to.x),
                n(to.y)
            ),
            Seg::Close => "Z".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A drawing as an SVG picture on the cursors' grid (Settings' previews).
pub fn svg(d: &Drawing) -> String {
    let marks: String = d
        .marks
        .iter()
        .map(|m| {
            let fill = m.fill.map_or_else(|| "none".to_string(), hex);
            let stroke = m.stroke.map_or_else(String::new, |(c, w)| {
                let join = if m.round { "round" } else { "miter" };
                let cap = if m.round { "round" } else { "butt" };
                format!(
                    r#" stroke="{}" stroke-width="{w}" stroke-linejoin="{join}" stroke-linecap="{cap}""#,
                    hex(c)
                )
            });
            let paint = format!(r#"fill="{fill}"{stroke}"#);
            match &m.shape {
                Shape::Path(segs) => format!(r#"<path d="{}" {paint}/>"#, path_data(segs)),
                Shape::Rect { x, y, w, h, rx, ry } => {
                    format!(r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" ry="{ry}" {paint}/>"#)
                }
                Shape::Circle { cx, cy, r } => format!(r#"<circle cx="{cx}" cy="{cy}" r="{r}" {paint}/>"#),
            }
        })
        .collect();
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {UNITS} {UNITS}">{marks}</svg>"#)
}

/// One shape, outlined and then filled.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub shape: Shape,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, f32)>,
    pub round: bool,
}

/// A cursor drawing: its marks, back to front, and its hot spot (grid units).
#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub marks: Vec<Mark>,
    pub hotspot: Point,
}

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn polygon(points: &[(f32, f32)]) -> Shape {
    let mut segs = Vec::with_capacity(points.len() + 1);
    for (i, &(x, y)) in points.iter().enumerate() {
        segs.push(if i == 0 { Seg::Move(p(x, y)) } else { Seg::Line(p(x, y)) });
    }
    segs.push(Seg::Close);
    Shape::Path(segs)
}

/// `points` turned by `degrees` around the grid's centre.
fn turned(points: &[(f32, f32)], degrees: f32) -> Vec<(f32, f32)> {
    let (s, c) = degrees.to_radians().sin_cos();
    let m = UNITS / 2.0;
    points
        .iter()
        .map(|&(x, y)| (m + (x - m) * c - (y - m) * s, m + (x - m) * s + (y - m) * c))
        .collect()
}

const ARROW: [(f32, f32); 7] = [
    (6.0, 3.0),
    (6.0, 25.0),
    (11.0, 20.0),
    (14.5, 28.0),
    (18.0, 26.5),
    (14.5, 18.8),
    (21.5, 18.8),
];
const SLIM_ARROW: [(f32, f32); 7] = [
    (6.0, 3.0),
    (6.0, 23.0),
    (10.5, 18.8),
    (13.5, 26.0),
    (16.0, 25.0),
    (13.0, 18.0),
    (19.0, 18.0),
];
/// A double arrow along the vertical axis; turned for the other directions.
const DOUBLE: [(f32, f32); 10] = [
    (16.0, 3.0),
    (22.0, 10.0),
    (18.0, 10.0),
    (18.0, 22.0),
    (22.0, 22.0),
    (16.0, 29.0),
    (10.0, 22.0),
    (14.0, 22.0),
    (14.0, 10.0),
    (10.0, 10.0),
];
const FOUR_WAY: [(f32, f32); 24] = [
    (16.0, 2.0),
    (21.0, 7.0),
    (18.0, 7.0),
    (18.0, 14.0),
    (25.0, 14.0),
    (25.0, 11.0),
    (30.0, 16.0),
    (25.0, 21.0),
    (25.0, 18.0),
    (18.0, 18.0),
    (18.0, 25.0),
    (21.0, 25.0),
    (16.0, 30.0),
    (11.0, 25.0),
    (14.0, 25.0),
    (14.0, 18.0),
    (7.0, 18.0),
    (7.0, 21.0),
    (2.0, 16.0),
    (7.0, 11.0),
    (7.0, 14.0),
    (14.0, 14.0),
    (14.0, 7.0),
    (11.0, 7.0),
];
const UP: [(f32, f32); 7] = [
    (16.0, 2.0),
    (24.0, 12.0),
    (19.0, 12.0),
    (19.0, 29.0),
    (13.0, 29.0),
    (13.0, 12.0),
    (8.0, 12.0),
];
const HAND: [(f32, f32); 18] = [
    (12.0, 3.5),
    (15.0, 3.5),
    (15.0, 13.0),
    (17.0, 12.5),
    (19.0, 13.0),
    (20.0, 14.0),
    (22.0, 13.8),
    (23.5, 14.8),
    (25.0, 15.0),
    (26.5, 16.5),
    (26.5, 22.0),
    (24.0, 28.0),
    (13.0, 28.0),
    (9.0, 21.0),
    (7.0, 17.5),
    (8.5, 16.0),
    (10.0, 16.5),
    (12.0, 19.0),
];
const PEN: [(f32, f32); 5] = [(5.0, 27.0), (8.0, 20.0), (22.0, 6.0), (26.0, 10.0), (12.0, 24.0)];

fn rgba(c: Color) -> kabuk_theme::contrast::Rgba {
    kabuk_theme::contrast::Rgba {
        r: f64::from(c.r),
        g: f64::from(c.g),
        b: f64::from(c.b),
        a: f64::from(c.a),
    }
}

impl CursorStyle {
    /// The body or the outline colour, whichever reads better on the accent (a badge's
    /// sign).
    fn on_accent(&self) -> Color {
        use kabuk_theme::contrast::contrast;
        let a = rgba(self.accent);
        if contrast(a, rgba(self.fill)) >= contrast(a, rgba(self.outline)) {
            self.fill
        } else {
            self.outline
        }
    }

    /// The ring's track: between the body and the outline colour.
    fn track(&self) -> Color {
        let top = kabuk_theme::contrast::Rgba {
            a: TRACK_SHARE,
            ..rgba(self.fill)
        };
        let mixed = kabuk_theme::contrast::over(top, rgba(self.outline));
        let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        Color {
            r: byte(mixed.r),
            g: byte(mixed.g),
            b: byte(mixed.b),
            a: 1.0,
        }
    }

    fn solid(&self, shape: Shape) -> Mark {
        Mark {
            shape,
            fill: Some(self.fill),
            stroke: Some((self.outline, self.outline_width)),
            round: self.round,
        }
    }

    fn badge(&self, shape: Shape) -> Mark {
        Mark {
            shape,
            fill: Some(self.accent),
            stroke: Some((self.outline, self.outline_width)),
            round: true,
        }
    }

    /// A line in `color` with the outline around it.
    fn lines(&self, shape: Shape, color: Color, width: f32) -> [Mark; 2] {
        [
            Mark {
                shape: shape.clone(),
                fill: None,
                stroke: Some((self.outline, width + self.outline_width)),
                round: self.round,
            },
            Mark {
                shape,
                fill: None,
                stroke: Some((color, width)),
                round: self.round,
            },
        ]
    }

    fn arrow(&self) -> Vec<Mark> {
        let mut out = vec![self.solid(polygon(if self.slim { &SLIM_ARROW } else { &ARROW }))];
        if self.via {
            out.push(Mark {
                shape: Shape::Circle {
                    cx: 7.8,
                    cy: 8.0,
                    r: 1.6,
                },
                // On an accent body the accent would not show: the outline's colour.
                fill: Some(if self.accent == self.fill {
                    self.outline
                } else {
                    self.accent
                }),
                stroke: None,
                round: true,
            });
        }
        out
    }

    /// The turning ring around (`cx`, `cy`) at frame `frame` of [`FRAMES`].
    fn ring(&self, cx: f32, cy: f32, r: f32, width: f32, frame: u32) -> Vec<Mark> {
        let track = Shape::Circle { cx, cy, r };
        let start = 360.0 * frame as f32 / FRAMES as f32 - 90.0;
        let at = |deg: f32| {
            let (s, c) = deg.to_radians().sin_cos();
            p(cx + r * c, cy + r * s)
        };
        let arc = Shape::Path(vec![
            Seg::Move(at(start)),
            Seg::Arc {
                rx: r,
                ry: r,
                rotation: 0.0,
                large: ARC > 180.0,
                sweep: true,
                to: at(start + ARC),
            },
        ]);
        let mut out = self.lines(track, self.track(), width).to_vec();
        out.push(Mark {
            shape: arc,
            fill: None,
            stroke: Some((self.accent, width)),
            round: true,
        });
        out
    }

    /// The drawing of `role`; `frame` turns the animated ones.
    pub fn drawing(&self, role: Role, frame: u32) -> Drawing {
        if let Some(shape) = self.cartoon {
            return cartoon::drawing(self, shape, role, frame);
        }
        let tip = p(6.0, 3.0);
        let centre = p(UNITS / 2.0, UNITS / 2.0);
        let (marks, hotspot) = match role {
            Role::Arrow => (self.arrow(), tip),
            Role::Help => {
                let mut m = self.arrow();
                m.push(self.badge(Shape::Circle {
                    cx: 24.0,
                    cy: 24.0,
                    r: 6.0,
                }));
                m.push(Mark {
                    shape: Shape::Path(vec![
                        Seg::Move(p(21.8, 22.4)),
                        Seg::Arc {
                            rx: 2.3,
                            ry: 2.3,
                            rotation: 0.0,
                            large: true,
                            sweep: true,
                            to: p(25.0, 24.4),
                        },
                        Seg::Line(p(24.0, 25.6)),
                    ]),
                    fill: None,
                    stroke: Some((self.on_accent(), 1.6)),
                    round: true,
                });
                m.push(Mark {
                    shape: Shape::Circle {
                        cx: 24.0,
                        cy: 27.6,
                        r: 0.9,
                    },
                    fill: Some(self.on_accent()),
                    stroke: None,
                    round: true,
                });
                (m, tip)
            }
            Role::Working => {
                let mut m = self.arrow();
                m.extend(self.ring(24.0, 24.0, 4.5, 2.4, frame));
                (m, tip)
            }
            Role::Busy => (self.ring(16.0, 16.0, 9.0, 3.2, frame), centre),
            Role::Precision => {
                let plus = Shape::Path(vec![
                    Seg::Move(p(16.0, 3.0)),
                    Seg::Line(p(16.0, 13.0)),
                    Seg::Move(p(16.0, 19.0)),
                    Seg::Line(p(16.0, 29.0)),
                    Seg::Move(p(3.0, 16.0)),
                    Seg::Line(p(13.0, 16.0)),
                    Seg::Move(p(19.0, 16.0)),
                    Seg::Line(p(29.0, 16.0)),
                ]);
                (self.lines(plus, self.fill, 2.0).to_vec(), centre)
            }
            Role::Text if self.block_text => (
                vec![self.solid(Shape::Rect {
                    x: 11.0,
                    y: 6.0,
                    w: 10.0,
                    h: 20.0,
                    rx: 0.0,
                    ry: 0.0,
                })],
                centre,
            ),
            Role::Text => {
                let beam = Shape::Path(vec![
                    Seg::Move(p(12.0, 5.0)),
                    Seg::Line(p(20.0, 5.0)),
                    Seg::Move(p(16.0, 5.0)),
                    Seg::Line(p(16.0, 27.0)),
                    Seg::Move(p(12.0, 27.0)),
                    Seg::Line(p(20.0, 27.0)),
                ]);
                (self.lines(beam, self.fill, 2.0).to_vec(), centre)
            }
            Role::Handwriting => (vec![self.solid(polygon(&PEN))], p(5.0, 27.0)),
            Role::Unavailable => {
                let sign = Shape::Path(vec![
                    Seg::Move(p(26.0, 16.0)),
                    Seg::Arc {
                        rx: 10.0,
                        ry: 10.0,
                        rotation: 0.0,
                        large: true,
                        sweep: true,
                        to: p(25.99, 15.6),
                    },
                    Seg::Close,
                    Seg::Move(p(9.0, 23.0)),
                    Seg::Line(p(23.0, 9.0)),
                ]);
                (self.lines(sign, self.warning, 3.0).to_vec(), centre)
            }
            Role::SizeNS => (vec![self.solid(polygon(&DOUBLE))], centre),
            Role::SizeWE => (vec![self.solid(polygon(&turned(&DOUBLE, 90.0)))], centre),
            Role::SizeNWSE => (vec![self.solid(polygon(&turned(&DOUBLE, -45.0)))], centre),
            Role::SizeNESW => (vec![self.solid(polygon(&turned(&DOUBLE, 45.0)))], centre),
            Role::Move => (vec![self.solid(polygon(&FOUR_WAY))], centre),
            Role::Alternate => (vec![self.solid(polygon(&UP))], p(16.0, 2.0)),
            Role::Link => (vec![self.solid(polygon(&HAND))], p(13.5, 3.5)),
            Role::Pin => {
                let mut m = self.arrow();
                m.push(self.badge(Shape::Path(vec![
                    Seg::Move(p(24.0, 30.0)),
                    Seg::Line(p(20.2, 23.5)),
                    Seg::Arc {
                        rx: 4.5,
                        ry: 4.5,
                        rotation: 0.0,
                        large: true,
                        sweep: true,
                        to: p(27.8, 23.5),
                    },
                    Seg::Close,
                ])));
                (m, tip)
            }
            Role::Person => {
                let mut m = self.arrow();
                m.push(self.badge(Shape::Circle {
                    cx: 24.0,
                    cy: 20.0,
                    r: 2.8,
                }));
                m.push(self.badge(Shape::Path(vec![
                    Seg::Move(p(19.0, 29.5)),
                    Seg::Arc {
                        rx: 5.0,
                        ry: 5.0,
                        rotation: 0.0,
                        large: false,
                        sweep: true,
                        to: p(29.0, 29.5),
                    },
                    Seg::Close,
                ])));
                (m, tip)
            }
        };
        Drawing { marks, hotspot }
    }
}

/// One picture of a cursor: straight-alpha RGBA, `side`×`side`, and its hot spot in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorImage {
    pub side: u32,
    pub hotspot: (u16, u16),
    pub rgba: Vec<u8>,
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An image as a `.cur` entry's DIB: a 32-bit BGRA bitmap, bottom row first, then the
/// 1-bit AND mask (set where fully transparent).
fn dib(img: &CursorImage) -> Vec<u8> {
    let side = img.side as usize;
    let mask_stride = side.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + side * side * 4 + mask_stride * side);
    le32(&mut out, 40);
    le32(&mut out, img.side);
    le32(&mut out, img.side * 2);
    le16(&mut out, 1);
    le16(&mut out, 32);
    le32(&mut out, 0);
    le32(
        &mut out,
        u32::try_from(side * side * 4 + mask_stride * side).unwrap_or(0),
    );
    out.extend_from_slice(&[0; 16]);
    for y in (0..side).rev() {
        for x in 0..side {
            let i = (y * side + x) * 4;
            let px = &img.rgba[i..i + 4];
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    for y in (0..side).rev() {
        let mut row = vec![0u8; mask_stride];
        for x in 0..side {
            if img.rgba[(y * side + x) * 4 + 3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    out
}

/// A `.cur` file holding `images` (one per size).
pub fn cur_file(images: &[CursorImage]) -> Vec<u8> {
    const CURSOR: u16 = 2;
    const HEADER: usize = 6;
    const ENTRY: usize = 16;
    let dibs: Vec<Vec<u8>> = images.iter().map(dib).collect();
    let mut out = Vec::new();
    le16(&mut out, 0);
    le16(&mut out, CURSOR);
    le16(&mut out, u16::try_from(images.len()).unwrap_or(0));
    let mut offset = HEADER + ENTRY * images.len();
    for (img, d) in images.iter().zip(&dibs) {
        // 256 is written as 0.
        let side = u8::try_from(img.side).unwrap_or(0);
        out.extend_from_slice(&[side, side, 0, 0]);
        le16(&mut out, img.hotspot.0);
        le16(&mut out, img.hotspot.1);
        le32(&mut out, u32::try_from(d.len()).unwrap_or(0));
        le32(&mut out, u32::try_from(offset).unwrap_or(0));
        offset += d.len();
    }
    for d in dibs {
        out.extend_from_slice(&d);
    }
    out
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(id);
    le32(out, u32::try_from(data.len()).unwrap_or(0));
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// An `.ani` file: `frames` (each a `.cur` file) shown `jiffies`/60 s each, in a loop.
pub fn ani_file(frames: &[Vec<u8>], jiffies: u32) -> Vec<u8> {
    /// `AF_ICON`: the frames are icon (cursor) files.
    const AF_ICON: u32 = 1;
    const ANIH_SIZE: u32 = 36;
    let n = u32::try_from(frames.len()).unwrap_or(0);
    let mut anih = Vec::new();
    for v in [ANIH_SIZE, n, n, 0, 0, 0, 0, jiffies, AF_ICON] {
        le32(&mut anih, v);
    }
    let mut list = b"fram".to_vec();
    for f in frames {
        chunk(&mut list, b"icon", f);
    }
    let mut body = b"ACON".to_vec();
    chunk(&mut body, b"anih", &anih);
    chunk(&mut body, b"LIST", &list);
    let mut out = Vec::new();
    chunk(&mut out, b"RIFF", &body);
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: every role draws something with its hot spot on the grid; the
    //! registry names are Windows' own and unique; the styles change the drawings; the
    //! ring turns with the frame; the file formats' headers, offsets, hot spots, the
    //! bottom-up rows and the transparency mask; an odd-sized `.ani` chunk is padded;
    //! every style in every colour of every look (D41) and their SVG previews.
    use super::*;

    fn style() -> CursorStyle {
        CursorStyle {
            fill: Color::parse("#ffffff").unwrap(),
            outline: Color::parse("#000000").unwrap(),
            outline_width: 1.5,
            round: true,
            accent: Color::parse("#8fd3ff").unwrap(),
            warning: Color::parse("#ff8b8b").unwrap(),
            slim: false,
            block_text: false,
            via: false,
            cartoon: None,
        }
    }

    #[test]
    fn every_role_has_a_drawing_and_windows_name() {
        let mut names = std::collections::HashSet::new();
        for role in Role::ALL {
            let d = style().drawing(role, 0);
            assert!(!d.marks.is_empty(), "{role:?}");
            assert!((0.0..UNITS).contains(&d.hotspot.x) && (0.0..UNITS).contains(&d.hotspot.y));
            assert!(names.insert(role.registry_name()));
        }
        assert_eq!(Role::Busy.file_name(), "wait.ani");
        assert_eq!(Role::Arrow.file_name(), "arrow.cur");
        assert!(Role::Working.animated() && !Role::Link.animated());
    }

    #[test]
    fn looks_change_the_drawings_and_the_ring_turns() {
        let plain = style();
        let konsol = CursorStyle {
            block_text: true,
            round: false,
            ..plain
        };
        assert_ne!(plain.drawing(Role::Text, 0), konsol.drawing(Role::Text, 0));
        let devre = CursorStyle { via: true, ..plain };
        assert_eq!(devre.drawing(Role::Arrow, 0).marks.len(), 2);
        let cizgi = CursorStyle { slim: true, ..plain };
        assert_ne!(cizgi.drawing(Role::Arrow, 0), plain.drawing(Role::Arrow, 0));
        assert_ne!(plain.drawing(Role::Busy, 0), plain.drawing(Role::Busy, 1));
        assert_eq!(plain.drawing(Role::Busy, 0), plain.drawing(Role::Busy, FRAMES));
    }

    /// Seen in Çizgi, whose accent is its body's colour: the help sign takes the colour
    /// that reads on the accent, and the ring's track is not the accent.
    #[test]
    fn signs_and_rings_stay_visible_when_the_accent_is_the_body_colour() {
        let cizgi = CursorStyle {
            accent: style().fill,
            ..style()
        };
        assert_eq!(cizgi.on_accent(), cizgi.outline);
        assert_eq!(style().on_accent(), style().outline, "dark on the light blue accent");
        let track = cizgi.track();
        assert!(track != cizgi.accent && track != cizgi.outline, "{track:?}");
    }

    fn image(side: u32, hotspot: (u16, u16)) -> CursorImage {
        // Opaque red top-left pixel, the rest transparent.
        let mut rgba = vec![0u8; (side * side * 4) as usize];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        CursorImage { side, hotspot, rgba }
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn cur_files_hold_every_size_with_its_hot_spot() {
        let f = cur_file(&[image(32, (6, 3)), image(64, (12, 6))]);
        assert_eq!((u16_at(&f, 0), u16_at(&f, 2), u16_at(&f, 4)), (0, 2, 2));
        // First entry: 32 px, hot spot, size and offset of its DIB.
        assert_eq!((f[6], f[7]), (32, 32));
        assert_eq!((u16_at(&f, 10), u16_at(&f, 12)), (6, 3));
        let (len, off) = (u32_at(&f, 14) as usize, u32_at(&f, 18) as usize);
        assert_eq!(off, 6 + 2 * 16);
        assert_eq!(len, 40 + 32 * 32 * 4 + 4 * 32);
        assert_eq!(u32_at(&f, 22 + 12) as usize, off + len, "second DIB follows the first");
        // The DIB: double height, 32 bits; the top-left pixel is in the last row, BGRA.
        assert_eq!((u32_at(&f, off + 4), u32_at(&f, off + 8)), (32, 64));
        let last_row = off + 40 + 31 * 32 * 4;
        assert_eq!(&f[last_row..last_row + 4], &[0, 0, 255, 255]);
        // Mask: the opaque pixel's bit is clear, the next one's set.
        let mask_last_row = off + 40 + 32 * 32 * 4 + 31 * 4;
        assert_eq!(f[mask_last_row], 0x7f);
        assert_eq!(f.len(), off + len + 40 + 64 * 64 * 4 + 8 * 64);
    }

    #[test]
    fn ani_files_wrap_the_frames() {
        let frames = vec![vec![1u8, 2, 3], vec![4u8, 5]];
        let f = ani_file(&frames, FRAME_JIFFIES);
        assert_eq!(&f[0..4], b"RIFF");
        assert_eq!(u32_at(&f, 4) as usize, f.len() - 8);
        assert_eq!(&f[8..12], b"ACON");
        assert_eq!(&f[12..16], b"anih");
        assert_eq!(u32_at(&f, 16), 36);
        assert_eq!((u32_at(&f, 24), u32_at(&f, 48)), (2, FRAME_JIFFIES));
        assert_eq!(&f[56..60], b"LIST");
        assert_eq!(&f[64..68], b"fram");
        // The odd 3-byte frame is padded to 4.
        assert_eq!((&f[68..72], u32_at(&f, 72)), (&b"icon"[..], 3));
        assert_eq!(&f[76..80], &[1, 2, 3, 0]);
        assert_eq!(&f[80..84], b"icon");
    }

    fn look_tokens(code: &str, mode: kabuk_theme::Mode) -> Tokens {
        kabuk_theme::resolve::resolve(code, mode, None).unwrap().tokens
    }

    /// D41: every style in every colour from every look's tokens; the body and the
    /// outline always differ, and the accent body takes the outline that reads better.
    #[test]
    fn every_style_and_colour_comes_from_the_tokens() {
        use config::{CursorColor as C, CursorStyle as S};
        for look in config::Look::ALL {
            for mode in [kabuk_theme::Mode::Dark, kabuk_theme::Mode::Light] {
                let t = look_tokens(look.code(), mode);
                for style in S::ALL {
                    for color in C::ALL {
                        let s = style_for(style, color, &t);
                        assert_ne!(s.fill, s.outline, "{look:?} {mode:?} {style:?} {color:?}");
                    }
                }
                let accent = style_for(S::Round, C::Accent, &t);
                assert_eq!(accent.fill, t.color("accent-s").unwrap());
            }
        }
        let t = look_tokens("a", kabuk_theme::Mode::Dark);
        let light = style_for(S::Round, C::Light, &t);
        let dark = style_for(S::Round, C::Dark, &t);
        assert_eq!((light.fill, light.outline), (dark.outline, dark.fill));
        assert!(style_for(S::Block, C::Light, &t).block_text);
        assert!(style_for(S::Circuit, C::Light, &t).via);
        let slim = style_for(S::Slim, C::Light, &t);
        assert!(slim.slim && !slim.round);
        assert!(style_for(S::Bold, C::Light, &t).outline_width > light.outline_width);
    }

    /// Settings' previews: one SVG per role on the grid, with the style's colours, and a
    /// block text cursor only in the block style.
    #[test]
    fn previews_are_svg_pictures_in_the_styles_colours() {
        use config::{CursorColor as C, CursorStyle as S};
        let t = look_tokens("a", kabuk_theme::Mode::Dark);
        let s = style_for(S::Round, C::Light, &t);
        let arrow = svg(&s.drawing(Role::Arrow, 0));
        assert!(arrow.starts_with("<svg") && arrow.ends_with("</svg>"));
        assert!(arrow.contains(r#"viewBox="0 0 32 32""#));
        assert!(
            arrow.contains(&hex(s.fill)) && arrow.contains(&hex(s.outline)),
            "{arrow}"
        );
        assert!(arrow.contains("M6 3 L6 25"), "{arrow}");
        let text = |st| svg(&style_for(st, C::Light, &t).drawing(Role::Text, 0));
        assert!(text(S::Block).contains("<rect"));
        assert!(!text(S::Round).contains("<rect"));
        let ring = svg(&s.drawing(Role::Busy, 3));
        assert!(ring.contains("<circle") && ring.contains(" A9 9 0 0 1 "), "{ring}");
    }

    /// Found by eye (D41): in the accent colour, Devre's via was the body's colour and
    /// could not be seen; it takes the outline's colour there.
    #[test]
    fn the_via_shows_on_an_accent_body() {
        use config::{CursorColor as C, CursorStyle as S};
        for code in ["a", "c"] {
            let t = look_tokens(code, kabuk_theme::Mode::Dark);
            for color in C::ALL {
                let s = style_for(S::Circuit, color, &t);
                let marks = s.drawing(Role::Arrow, 0).marks;
                let via = marks
                    .iter()
                    .find(|m| matches!(m.shape, Shape::Circle { .. }))
                    .expect("a via");
                assert_ne!(via.fill, Some(s.fill), "{code} {color:?}");
            }
        }
    }
}
