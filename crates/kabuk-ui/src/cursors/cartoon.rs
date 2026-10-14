//! The cartoon sets (asked for by the user on 2026-10-14 with a picture): fat white
//! bodies in a thick dark outline, hands for moving and links, and coloured badges on the
//! arrow; `Round` with soft corners, `Sharp` with pointed ones. The body and the outline
//! follow the chosen colour; the badges keep their own colours, as in the picture.
//!
//! Each body is drawn twice: every part's outline first, then every part's fill, so the
//! parts of one body (a hand's palm and fingers) join without lines between them.

use kabuk_theme::Color;

use super::{p, polygon, turned, Cartoon, CursorStyle, Drawing, Mark, Role, FRAMES, UNITS};
use crate::svg::{Seg, Shape};

/// The outline's width outside the body, in grid units.
const OUTLINE: f32 = 1.7;
/// How wide a finger, a bar or a bracket is.
const FINGER: f32 = 3.2;
const LITTLE_FINGER: f32 = 2.9;
const BAR: f32 = 2.8;
/// The badge on the arrow: its centre and radius, and the width of its signs.
const BADGE: (f32, f32, f32) = (23.5, 23.5, 6.0);
const SIGN: f32 = 2.0;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 1.0 }
}

/// The badges' colours, the style's own like its shapes.
pub struct Badges {
    pub help: Color,
    pub working: Color,
    pub unavailable: Color,
    pub info: Color,
    pub person: Color,
    pub nwse: Color,
    pub nesw: Color,
    /// The signs drawn on a badge.
    pub sign: Color,
}

pub const BADGES: Badges = Badges {
    help: rgb(0xff, 0xc2, 0x1a),
    working: rgb(0x22, 0xb0, 0x4b),
    unavailable: rgb(0xe0, 0x30, 0x3d),
    info: rgb(0x7a, 0x3f, 0xf0),
    person: rgb(0x1b, 0x9b, 0xe0),
    nwse: rgb(0x2a, 0xa3, 0xe8),
    nesw: rgb(0xf2, 0x64, 0x3a),
    sign: rgb(0xff, 0xff, 0xff),
};

/// One part of a body: a filled shape, or a line of some width with round ends.
enum Part {
    Solid(Shape),
    Line(Shape, f32),
}

fn line(from: (f32, f32), to: (f32, f32)) -> Shape {
    Shape::Path(vec![Seg::Move(p(from.0, from.1)), Seg::Line(p(to.0, to.1))])
}

fn circle(cx: f32, cy: f32, r: f32) -> Shape {
    Shape::Circle { cx, cy, r }
}

/// A body of `parts` in `fill`, outlined in the style's outline colour.
fn body(s: &CursorStyle, parts: &[Part], fill: Color, round: bool) -> Vec<Mark> {
    let outline = parts.iter().map(|part| match part {
        Part::Solid(shape) => Mark {
            shape: shape.clone(),
            fill: Some(s.outline),
            stroke: Some((s.outline, 2.0 * OUTLINE)),
            round,
        },
        Part::Line(shape, w) => Mark {
            shape: shape.clone(),
            fill: None,
            stroke: Some((s.outline, w + 2.0 * OUTLINE)),
            round: true,
        },
    });
    let inside = parts.iter().map(|part| match part {
        Part::Solid(shape) => Mark {
            shape: shape.clone(),
            fill: Some(fill),
            stroke: None,
            round,
        },
        Part::Line(shape, w) => Mark {
            shape: shape.clone(),
            fill: None,
            stroke: Some((fill, *w)),
            round: true,
        },
    });
    outline.chain(inside).collect()
}

/// A sign on a badge: lines and dots in the badge's sign colour.
fn sign(parts: Vec<Part>) -> Vec<Mark> {
    parts
        .into_iter()
        .map(|part| match part {
            Part::Solid(shape) => Mark {
                shape,
                fill: Some(BADGES.sign),
                stroke: None,
                round: true,
            },
            Part::Line(shape, w) => Mark {
                shape,
                fill: None,
                stroke: Some((BADGES.sign, w)),
                round: true,
            },
        })
        .collect()
}

/// Three dots turning with the frame around (`cx`, `cy`).
fn dots(cx: f32, cy: f32, r: f32, dot: f32, frame: u32) -> Vec<Part> {
    (0..3)
        .map(|k| {
            let deg = 360.0 * frame as f32 / FRAMES as f32 + 120.0 * k as f32 - 90.0;
            let (sn, cs) = deg.to_radians().sin_cos();
            Part::Solid(circle(cx + r * cs, cy + r * sn, dot))
        })
        .collect()
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> [(f32, f32); 4] {
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

pub(super) fn drawing(s: &CursorStyle, shape: Cartoon, role: Role, frame: u32) -> Drawing {
    let round = shape == Cartoon::Round;
    let tip = p(7.0, 5.0);
    let centre = p(UNITS / 2.0, UNITS / 2.0);
    // Round: a plump triangle; Sharp: the classic arrow head.
    let arrow_points: Vec<(f32, f32)> = if round {
        vec![(7.0, 5.0), (8.5, 25.0), (21.0, 17.5)]
    } else {
        vec![(7.0, 5.0), (7.0, 25.5), (11.5, 20.5), (20.5, 20.5)]
    };
    let arrow = || body(s, &[Part::Solid(polygon(&arrow_points))], s.fill, round);
    let (bx, by, br) = BADGE;
    let badge = |colour: Color, signs: Vec<Part>| {
        let mut m = body(s, &[Part::Solid(circle(bx, by, br))], colour, true);
        m.extend(sign(signs));
        m
    };
    let with_badge = |colour: Color, signs: Vec<Part>| {
        let mut m = arrow();
        m.extend(badge(colour, signs));
        (m, tip)
    };
    let (marks, hotspot) = match role {
        Role::Arrow => (arrow(), tip),
        Role::Help => with_badge(
            BADGES.help,
            vec![
                Part::Line(
                    Shape::Path(vec![
                        Seg::Move(p(21.4, 21.8)),
                        Seg::Arc {
                            rx: 2.2,
                            ry: 2.2,
                            rotation: 0.0,
                            large: true,
                            sweep: true,
                            to: p(24.4, 23.6),
                        },
                        Seg::Line(p(23.5, 24.6)),
                    ]),
                    SIGN,
                ),
                Part::Solid(circle(23.5, 27.4, 1.0)),
            ],
        ),
        Role::Working => with_badge(BADGES.working, dots(bx, by, 2.7, 1.2, frame)),
        Role::Busy => {
            let mut m = body(s, &[Part::Solid(circle(16.0, 16.0, 10.0))], BADGES.working, true);
            m.extend(sign(dots(16.0, 16.0, 5.0, 2.0, frame)));
            (m, centre)
        }
        Role::Unavailable => with_badge(
            BADGES.unavailable,
            vec![Part::Line(line((20.6, 26.4), (26.4, 20.6)), SIGN)],
        ),
        Role::Alternate => with_badge(
            BADGES.info,
            vec![
                Part::Solid(circle(bx, by - 2.9, 1.1)),
                Part::Line(line((bx, by - 0.4), (bx, by + 3.2)), SIGN),
            ],
        ),
        Role::Pin => with_badge(
            BADGES.info,
            vec![Part::Line(circle(bx, by, 2.8), 1.4), Part::Solid(circle(bx, by, 1.0))],
        ),
        Role::Person => with_badge(
            BADGES.person,
            vec![
                Part::Solid(circle(bx, by - 2.0, 1.7)),
                Part::Solid(Shape::Path(vec![
                    Seg::Move(p(bx - 3.4, by + 3.6)),
                    Seg::Arc {
                        rx: 3.4,
                        ry: 3.0,
                        rotation: 0.0,
                        large: false,
                        sweep: true,
                        to: p(bx + 3.4, by + 3.6),
                    },
                    Seg::Close,
                ])),
            ],
        ),
        Role::Precision => {
            let parts = [
                Part::Line(line((16.0, 4.0), (16.0, 12.0)), BAR),
                Part::Line(line((16.0, 20.0), (16.0, 28.0)), BAR),
                Part::Line(line((4.0, 16.0), (12.0, 16.0)), BAR),
                Part::Line(line((20.0, 16.0), (28.0, 16.0)), BAR),
            ];
            let mut m = body(s, &parts, s.fill, round);
            m.extend(body(
                s,
                &[Part::Solid(circle(16.0, 16.0, 1.6))],
                BADGES.unavailable,
                true,
            ));
            (m, centre)
        }
        Role::Text => {
            let parts = [
                Part::Line(line((12.0, 5.5), (20.0, 5.5)), BAR),
                Part::Line(line((16.0, 5.5), (16.0, 26.5)), BAR),
                Part::Line(line((12.0, 26.5), (20.0, 26.5)), BAR),
            ];
            (body(s, &parts, s.fill, round), centre)
        }
        Role::Handwriting => {
            let pen = polygon(&[(6.0, 26.0), (8.5, 19.5), (21.5, 6.5), (25.5, 10.5), (12.5, 23.5)]);
            let mut m = body(s, &[Part::Solid(pen)], s.fill, round);
            m.extend(body(
                s,
                &[Part::Solid(polygon(&[
                    (21.5, 6.5),
                    (25.5, 10.5),
                    (23.6, 12.4),
                    (19.6, 8.4),
                ]))],
                BADGES.help,
                round,
            ));
            (m, p(6.0, 26.0))
        }
        Role::Link => {
            // A hand pointing up: the index finger out, the others folded.
            let parts = [
                Part::Line(line((13.0, 4.8), (13.0, 17.0)), FINGER),
                Part::Line(line((16.8, 13.5), (16.8, 17.0)), FINGER),
                Part::Line(line((20.4, 14.4), (20.4, 17.5)), FINGER),
                Part::Line(line((23.6, 16.0), (23.6, 18.5)), LITTLE_FINGER),
                Part::Line(line((8.6, 17.4), (11.4, 21.2)), FINGER),
                Part::Solid(Shape::Rect {
                    x: 11.2,
                    y: 15.0,
                    w: 14.2,
                    h: 12.5,
                    rx: 5.0,
                    ry: 5.0,
                }),
            ];
            (body(s, &parts, s.fill, true), p(13.0, 3.2))
        }
        Role::Move => {
            // An open hand.
            let parts = [
                Part::Line(line((10.8, 7.6), (10.8, 17.0)), FINGER),
                Part::Line(line((14.7, 5.8), (14.7, 16.0)), FINGER),
                Part::Line(line((18.6, 6.8), (18.6, 16.0)), FINGER),
                Part::Line(line((22.3, 9.6), (22.3, 17.0)), LITTLE_FINGER),
                Part::Line(line((6.6, 15.4), (10.0, 20.2)), FINGER),
                Part::Solid(Shape::Rect {
                    x: 9.0,
                    y: 14.0,
                    w: 15.2,
                    h: 13.2,
                    rx: 5.0,
                    ry: 5.0,
                }),
            ];
            (body(s, &parts, s.fill, true), centre)
        }
        Role::SizeNS | Role::SizeWE => {
            // An hourglass between two bars.
            let pts: [[(f32, f32); 4]; 2] = [rect(8.0, 3.5, 16.0, BAR), rect(8.0, 25.7, 16.0, BAR)];
            let tops: [[(f32, f32); 3]; 2] = [
                [(9.5, 9.0), (22.5, 9.0), (16.0, 15.6)],
                [(9.5, 23.0), (22.5, 23.0), (16.0, 16.4)],
            ];
            let deg = if role == Role::SizeWE { 90.0 } else { 0.0 };
            let parts: Vec<Part> = pts
                .iter()
                .map(|r| Part::Solid(polygon(&turned(r, deg))))
                .chain(tops.iter().map(|t| Part::Solid(polygon(&turned(t, deg)))))
                .collect();
            (body(s, &parts, s.fill, round), centre)
        }
        Role::SizeNWSE | Role::SizeNESW => {
            // A coloured fan from the corner being moved, the corner's bracket on it.
            let nwse = role == Role::SizeNWSE;
            let (cx, sx) = if nwse { (7.5, 1.0) } else { (24.5, -1.0) };
            let (cy, r) = (7.5, 16.0);
            let fan = Shape::Path(vec![
                Seg::Move(p(cx, cy)),
                Seg::Line(p(cx + sx * r, cy)),
                Seg::Arc {
                    rx: r,
                    ry: r,
                    rotation: 0.0,
                    large: false,
                    sweep: nwse,
                    to: p(cx, cy + r),
                },
                Seg::Close,
            ]);
            let colour = if nwse { BADGES.nwse } else { BADGES.nesw };
            let mut m = body(s, &[Part::Solid(fan)], colour, round);
            let bracket = Shape::Path(vec![
                Seg::Move(p(cx, cy + 7.0)),
                Seg::Line(p(cx, cy)),
                Seg::Line(p(cx + sx * 7.0, cy)),
            ]);
            m.extend(body(s, &[Part::Line(bracket, BAR)], s.fill, round));
            (m, centre)
        }
    };
    Drawing { marks, hotspot }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every role of both sets draws inside the grid with its hot spot there;
    //! the badges carry their own colours whatever the body's colour; a body is outlined
    //! before it is filled (its parts join); the busy dots turn; Round and Sharp differ.
    use super::super::{style_for, svg};
    use super::*;
    use kabuk_core::config::{CursorColor, CursorStyle as S};

    fn tokens() -> kabuk_theme::Tokens {
        kabuk_theme::resolve::resolve("a", kabuk_theme::Mode::Dark, None)
            .unwrap()
            .tokens
    }

    #[test]
    fn every_role_of_both_sets_draws_inside_the_grid() {
        for style in [S::Cartoon, S::Comic] {
            for color in CursorColor::ALL {
                let st = style_for(style, color, &tokens());
                assert!(st.cartoon.is_some());
                for role in Role::ALL {
                    let d = st.drawing(role, 0);
                    assert!(d.marks.len() >= 2, "{style:?} {role:?}");
                    let (x, y) = (d.hotspot.x, d.hotspot.y);
                    assert!(
                        (0.0..UNITS).contains(&x) && (0.0..UNITS).contains(&y),
                        "{role:?} {x} {y}"
                    );
                }
            }
        }
    }

    #[test]
    fn badges_keep_their_colours_and_bodies_are_outlined_first() {
        let st = style_for(S::Cartoon, CursorColor::Accent, &tokens());
        let has = |role: Role, c: Color| st.drawing(role, 0).marks.iter().any(|m| m.fill == Some(c));
        assert!(has(Role::Help, BADGES.help));
        assert!(has(Role::Unavailable, BADGES.unavailable));
        assert!(has(Role::Working, BADGES.working));
        assert!(has(Role::Person, BADGES.person));
        assert!(has(Role::SizeNWSE, BADGES.nwse) && has(Role::SizeNESW, BADGES.nesw));
        // A hand: all its outlines come before any of its fills.
        let hand = st.drawing(Role::Move, 0).marks;
        let first_fill = hand
            .iter()
            .position(|m| m.fill == Some(st.fill) || m.stroke.is_some_and(|(c, _)| c == st.fill))
            .unwrap();
        assert!(hand[..first_fill]
            .iter()
            .all(|m| m.fill.is_none_or(|c| c == st.outline)));
        assert!(hand[first_fill..].iter().all(|m| m.fill != Some(st.outline)));
    }

    #[test]
    fn the_busy_dots_turn_and_the_two_sets_differ() {
        let st = style_for(S::Cartoon, CursorColor::Light, &tokens());
        assert_ne!(st.drawing(Role::Busy, 0), st.drawing(Role::Busy, FRAMES / 3 + 1));
        let sharp = style_for(S::Comic, CursorColor::Light, &tokens());
        assert_ne!(st.drawing(Role::Arrow, 0), sharp.drawing(Role::Arrow, 0));
        // Settings previews them like the others.
        assert!(svg(&st.drawing(Role::Link, 0)).starts_with("<svg"));
    }
}
