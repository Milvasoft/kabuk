//! A small SVG reader for the design's icons: `<path d>`, `<rect>` and `<circle>`,
//! with the full path command set (absolute and relative, implicit repeats, smooth
//! curves). Output is in the icon's own coordinates (24×24 view box). [`parse_painted`]
//! also reads each shape's paint as the system theme's icons give it: the colour tokens
//! in its `style` (`fill:var(--accent-s);stroke:var(--dim);stroke-width:1.4`).

use crate::geom::Point;

#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quad(Point, Point),
    Arc {
        rx: f32,
        ry: f32,
        rotation: f32,
        large: bool,
        sweep: bool,
        to: Point,
    },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Path(Vec<Seg>),
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
        ry: f32,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SvgError {
    #[error("missing attribute `{0}`")]
    Missing(&'static str),
    #[error("bad number near `{0}`")]
    Number(String),
    #[error("path must start with a move command")]
    NoMove,
    #[error("unknown path command `{0}`")]
    Command(char),
}

/// A shape with the colour tokens it is filled and stroked with (`None`: not at all).
#[derive(Clone, Debug, PartialEq)]
pub struct Painted {
    pub shape: Shape,
    pub fill: Option<String>,
    pub stroke: Option<(String, f32)>,
    pub opacity: f32,
}

/// The token of a `var(--name)` paint; `None` for `none`.
fn token(value: &str) -> Option<String> {
    let v = value.trim();
    v.strip_prefix("var(--")
        .and_then(|r| r.strip_suffix(')'))
        .map(str::to_string)
}

/// The `name:value` pairs of a `style` attribute.
fn style(attrs: &str) -> Vec<(&str, &str)> {
    attr(attrs, "style")
        .unwrap_or_default()
        .split(';')
        .filter_map(|kv| kv.split_once(':'))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

/// Every shape of a document with its paint, in order, and whether its joins are round
/// (the root's `stroke-linejoin`).
pub fn parse_painted(svg: &str) -> Result<(Vec<Painted>, bool), SvgError> {
    let mut out = Vec::new();
    let mut round = false;
    for (name, attrs) in elements(svg) {
        if name == "svg" {
            round = style(attrs)
                .iter()
                .any(|(k, v)| *k == "stroke-linejoin" && *v == "round");
            continue;
        }
        let Some(shape) = shape(name, attrs)? else { continue };
        let st = style(attrs);
        let get = |k: &str| st.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
        let width = get("stroke-width").map(number).transpose()?.unwrap_or(1.0);
        out.push(Painted {
            shape,
            fill: get("fill").and_then(token),
            stroke: get("stroke")
                .and_then(token)
                .filter(|_| width > 0.0)
                .map(|t| (t, width)),
            opacity: get("opacity").map(number).transpose()?.unwrap_or(1.0),
        });
    }
    Ok((out, round))
}

/// All shapes of an icon document, in order.
pub fn parse(svg: &str) -> Result<Vec<Shape>, SvgError> {
    let mut shapes = Vec::new();
    for (name, attrs) in elements(svg) {
        if let Some(s) = shape(name, attrs)? {
            shapes.push(s);
        }
    }
    Ok(shapes)
}

/// The shape an element draws, if it is one of the shapes read.
fn shape(name: &str, attrs: &str) -> Result<Option<Shape>, SvgError> {
    let mut shapes = Vec::new();
    {
        match name {
            "path" => shapes.push(Shape::Path(parse_path(
                attr(attrs, "d").ok_or(SvgError::Missing("d"))?,
            )?)),
            "rect" => {
                let num = |n: &'static str, default: Option<f32>| -> Result<f32, SvgError> {
                    match attr(attrs, n) {
                        Some(v) => number(v),
                        None => default.ok_or(SvgError::Missing(n)),
                    }
                };
                let rx_attr = attr(attrs, "rx").map(number).transpose()?;
                let ry_attr = attr(attrs, "ry").map(number).transpose()?;
                // SVG: a missing rx/ry takes the other one's value.
                let rx = rx_attr.or(ry_attr).unwrap_or(0.0);
                let ry = ry_attr.or(rx_attr).unwrap_or(0.0);
                shapes.push(Shape::Rect {
                    x: num("x", Some(0.0))?,
                    y: num("y", Some(0.0))?,
                    w: num("width", None)?,
                    h: num("height", None)?,
                    rx,
                    ry,
                });
            }
            "circle" => shapes.push(Shape::Circle {
                cx: number(attr(attrs, "cx").unwrap_or("0"))?,
                cy: number(attr(attrs, "cy").unwrap_or("0"))?,
                r: number(attr(attrs, "r").ok_or(SvgError::Missing("r"))?)?,
            }),
            _ => {}
        }
    }
    Ok(shapes.pop())
}

/// `(name, attribute text)` of every element tag, skipping comments and closing tags.
fn elements(svg: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        if rest.starts_with("!--") {
            rest = rest.find("-->").map_or("", |e| &rest[e + 3..]);
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let tag = rest[..end].trim_end_matches('/');
        rest = &rest[end + 1..];
        if tag.starts_with('/') || tag.starts_with('?') {
            continue;
        }
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        out.push((name, attrs));
    }
    out
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        let value_and_rest = &after[quote.len_utf8()..];
        let close = value_and_rest.find(quote)?;
        if key == name {
            return Some(&value_and_rest[..close]);
        }
        rest = &value_and_rest[close + 1..];
    }
    None
}

fn number(text: &str) -> Result<f32, SvgError> {
    text.trim().parse().map_err(|_| SvgError::Number(text.to_string()))
}

/// Tokenizer for path data: commands, numbers and arc flags.
struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Tokens<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn command(&mut self) -> Option<char> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() {
            self.i += 1;
            Some(c as char)
        } else {
            None
        }
    }

    fn at_number(&mut self) -> bool {
        self.skip();
        self.s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Result<f32, SvgError> {
        self.skip();
        let start = self.i;
        let mut seen_dot = false;
        let mut seen_exp = false;
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' => {}
                // A second dot starts the next number (`.5.5` is two numbers).
                b'.' if !seen_dot && !seen_exp => seen_dot = true,
                b'e' | b'E' if !seen_exp => {
                    seen_exp = true;
                    if matches!(self.s.get(self.i + 1), Some(b'-' | b'+')) {
                        self.i += 1;
                    }
                }
                _ => break,
            }
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
        text.parse().map_err(|_| SvgError::Number(text.to_string()))
    }

    /// Arc flags may be written without separators (`a1 1 0 01 1 1`).
    fn flag(&mut self) -> Result<bool, SvgError> {
        self.skip();
        match self.s.get(self.i) {
            Some(b'0') => {
                self.i += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.i += 1;
                Ok(true)
            }
            _ => Err(SvgError::Number(
                String::from_utf8_lossy(&self.s[self.i..]).into_owned(),
            )),
        }
    }

    fn point(&mut self) -> Result<Point, SvgError> {
        Ok(Point {
            x: self.number()?,
            y: self.number()?,
        })
    }
}

pub fn parse_path(d: &str) -> Result<Vec<Seg>, SvgError> {
    let mut t = Tokens { s: d.as_bytes(), i: 0 };
    let mut out = Vec::new();
    let mut cur = Point::default();
    let mut start = Point::default();
    // Reflection points for smooth curves.
    let mut last_cubic: Option<Point> = None;
    let mut last_quad: Option<Point> = None;
    let mut cmd = match t.command() {
        Some(c @ ('M' | 'm')) => c,
        Some(_) | None if d.trim().is_empty() => return Ok(out),
        _ => return Err(SvgError::NoMove),
    };
    loop {
        let rel = cmd.is_ascii_lowercase();
        let abs = |p: Point, cur: Point| {
            if rel {
                Point {
                    x: cur.x + p.x,
                    y: cur.y + p.y,
                }
            } else {
                p
            }
        };
        let mut cubic_ctrl = None;
        let mut quad_ctrl = None;
        match cmd.to_ascii_uppercase() {
            'M' => {
                cur = abs(t.point()?, cur);
                start = cur;
                out.push(Seg::Move(cur));
                // Further pairs after a move are lines.
                cmd = if rel { 'l' } else { 'L' };
                if !t.at_number() {
                    cmd = match t.command() {
                        Some(c) => c,
                        None => break,
                    };
                }
                last_cubic = None;
                last_quad = None;
                continue;
            }
            'L' => {
                cur = abs(t.point()?, cur);
                out.push(Seg::Line(cur));
            }
            'H' => {
                let x = t.number()?;
                cur = Point {
                    x: if rel { cur.x + x } else { x },
                    y: cur.y,
                };
                out.push(Seg::Line(cur));
            }
            'V' => {
                let y = t.number()?;
                cur = Point {
                    x: cur.x,
                    y: if rel { cur.y + y } else { y },
                };
                out.push(Seg::Line(cur));
            }
            'C' => {
                let c1 = abs(t.point()?, cur);
                let c2 = abs(t.point()?, cur);
                let to = abs(t.point()?, cur);
                out.push(Seg::Cubic(c1, c2, to));
                cubic_ctrl = Some(c2);
                cur = to;
            }
            'S' => {
                let c1 = last_cubic.map_or(cur, |c| Point {
                    x: 2.0 * cur.x - c.x,
                    y: 2.0 * cur.y - c.y,
                });
                let c2 = abs(t.point()?, cur);
                let to = abs(t.point()?, cur);
                out.push(Seg::Cubic(c1, c2, to));
                cubic_ctrl = Some(c2);
                cur = to;
            }
            'Q' => {
                let c = abs(t.point()?, cur);
                let to = abs(t.point()?, cur);
                out.push(Seg::Quad(c, to));
                quad_ctrl = Some(c);
                cur = to;
            }
            'T' => {
                let c = last_quad.map_or(cur, |q| Point {
                    x: 2.0 * cur.x - q.x,
                    y: 2.0 * cur.y - q.y,
                });
                let to = abs(t.point()?, cur);
                out.push(Seg::Quad(c, to));
                quad_ctrl = Some(c);
                cur = to;
            }
            'A' => {
                let rx = t.number()?;
                let ry = t.number()?;
                let rotation = t.number()?;
                let large = t.flag()?;
                let sweep = t.flag()?;
                let to = abs(t.point()?, cur);
                out.push(Seg::Arc {
                    rx,
                    ry,
                    rotation,
                    large,
                    sweep,
                    to,
                });
                cur = to;
            }
            'Z' => {
                out.push(Seg::Close);
                cur = start;
            }
            other => return Err(SvgError::Command(other)),
        }
        last_cubic = cubic_ctrl;
        last_quad = quad_ctrl;
        if !cmd.eq_ignore_ascii_case(&'Z') && t.at_number() {
            continue; // implicit repeat of the same command
        }
        cmd = match t.command() {
            Some(c) => c,
            None => break,
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Edge cases: implicit lines after a move, relative commands, H/V, smooth
    //! cubic/quadratic reflection (with and without a previous curve), arc flags
    //! written without spaces, numbers written as `-1.5-2`, `.5.5` and with exponents,
    //! close returning to the subpath start, rect rx/ry defaults, comments, unknown
    //! commands, every embedded icon parses.
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn moves_lines_and_close() {
        let segs = parse_path("M4 10v4h4l5 4V6L8 10H4z").unwrap();
        assert_eq!(
            segs,
            vec![
                Seg::Move(p(4.0, 10.0)),
                Seg::Line(p(4.0, 14.0)),
                Seg::Line(p(8.0, 14.0)),
                Seg::Line(p(13.0, 18.0)),
                Seg::Line(p(13.0, 6.0)),
                Seg::Line(p(8.0, 10.0)),
                Seg::Line(p(4.0, 10.0)),
                Seg::Close,
            ]
        );
    }

    #[test]
    fn implicit_lines_after_move_and_relative_moves() {
        let segs = parse_path("m1 1 2 0 0 2zm5 5h1").unwrap();
        assert_eq!(
            segs,
            vec![
                Seg::Move(p(1.0, 1.0)),
                Seg::Line(p(3.0, 1.0)),
                Seg::Line(p(3.0, 3.0)),
                Seg::Close,
                Seg::Move(p(6.0, 6.0)),
                Seg::Line(p(7.0, 6.0)),
            ]
        );
    }

    #[test]
    fn arcs_with_compact_flags() {
        let segs = parse_path("M10 14V5a2 2 0 1 1 4 0").unwrap();
        assert_eq!(
            segs[2],
            Seg::Arc {
                rx: 2.0,
                ry: 2.0,
                rotation: 0.0,
                large: true,
                sweep: true,
                to: p(14.0, 5.0)
            }
        );
        let compact = parse_path("M0 0a1 1 0 01 2 2").unwrap();
        assert_eq!(
            compact[1],
            Seg::Arc {
                rx: 1.0,
                ry: 1.0,
                rotation: 0.0,
                large: false,
                sweep: true,
                to: p(2.0, 2.0)
            }
        );
    }

    #[test]
    fn number_forms() {
        let segs = parse_path("M-1.5-2L.5.5l1e1 -1E-1").unwrap();
        assert_eq!(
            segs,
            vec![
                Seg::Move(p(-1.5, -2.0)),
                Seg::Line(p(0.5, 0.5)),
                Seg::Line(p(10.5, 0.4))
            ]
        );
    }

    #[test]
    fn smooth_curves_reflect_the_previous_control_point() {
        let segs = parse_path("M0 0C0 1 1 2 2 2S4 3 4 4").unwrap();
        assert_eq!(segs[2], Seg::Cubic(p(3.0, 2.0), p(4.0, 3.0), p(4.0, 4.0)));
        let no_prev = parse_path("M1 1S2 2 3 3").unwrap();
        assert_eq!(no_prev[1], Seg::Cubic(p(1.0, 1.0), p(2.0, 2.0), p(3.0, 3.0)));
        let q = parse_path("M0 0Q1 1 2 0T4 0").unwrap();
        assert_eq!(q[2], Seg::Quad(p(3.0, -1.0), p(4.0, 0.0)));
    }

    #[test]
    fn errors() {
        assert_eq!(parse_path("L1 1"), Err(SvgError::NoMove));
        assert_eq!(parse_path("M1 1X2"), Err(SvgError::Command('X')));
        assert!(matches!(parse_path("M1"), Err(SvgError::Number(_))));
        assert_eq!(parse_path(""), Ok(vec![]));
        assert_eq!(parse(r#"<rect x="1"/>"#), Err(SvgError::Missing("width")));
    }

    #[test]
    fn documents_with_rects_circles_and_comments() {
        let doc = r#"<!-- a <comment> --><svg viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2"/><circle cx="6.5" cy="18" r="2.5"/><path d='M1 1L2 2'/></svg>"#;
        let shapes = parse(doc).unwrap();
        assert_eq!(
            shapes[0],
            Shape::Rect {
                x: 6.0,
                y: 6.0,
                w: 12.0,
                h: 12.0,
                rx: 2.0,
                ry: 2.0
            }
        );
        assert_eq!(
            shapes[1],
            Shape::Circle {
                cx: 6.5,
                cy: 18.0,
                r: 2.5
            }
        );
        assert_eq!(
            shapes[2],
            Shape::Path(vec![Seg::Move(p(1.0, 1.0)), Seg::Line(p(2.0, 2.0))])
        );
    }

    #[test]
    fn every_embedded_icon_parses() {
        for (name, source) in crate::icons::ICONS {
            let shapes = parse(source).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!shapes.is_empty(), "{name}");
        }
    }

    /// The system theme's icons: each shape's tokens, width and opacity, and the root's
    /// joins.
    #[test]
    fn painted_shapes_carry_their_tokens() {
        let svg = r#"<svg viewBox="0 0 32 32" aria-hidden="true" style="stroke-linejoin:round;stroke-linecap:round"><rect x="3" y="6" width="11" height="6" rx="3" style="fill:var(--accent-s);stroke:none;stroke-width:0;opacity:.55"/><path d="M8 3h11l6 6v20H8z" style="fill:none;stroke:var(--dim);stroke-width:1.6"/><circle cx="13" cy="14" r="2" style="fill:var(--accent-s);stroke:none;stroke-width:1.6"/></svg>"#;
        let (shapes, round) = parse_painted(svg).unwrap();
        assert!(round);
        assert_eq!(shapes.len(), 3);
        assert_eq!(shapes[0].fill.as_deref(), Some("accent-s"));
        assert_eq!(shapes[0].stroke, None);
        assert!((shapes[0].opacity - 0.55).abs() < 1e-6);
        assert_eq!(shapes[1].fill, None);
        assert_eq!(shapes[1].stroke, Some(("dim".to_string(), 1.6)));
        assert!(matches!(shapes[2].shape, Shape::Circle { .. }));
        let (none, sharp) = parse_painted(r#"<svg style="stroke-linejoin:miter"></svg>"#).unwrap();
        assert!(none.is_empty() && !sharp);
    }
}
