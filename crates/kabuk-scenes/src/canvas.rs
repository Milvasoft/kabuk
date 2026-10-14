//! What a scene draws with: lines, circles, arcs, a few filled shapes, text and the
//! "milva" wordmark, in straight colours over a black surface, in physical pixels and
//! through a stack of transforms. The wallpaper and the saver implement it with
//! Direct2D; tests record the calls.

/// A point in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn distance(self, o: Point) -> f32 {
        ((o.x - self.x).powi(2) + (o.y - self.y).powi(2)).sqrt()
    }

    pub fn lerp(self, o: Point, t: f32) -> Point {
        Point {
            x: self.x + (o.x - self.x) * t,
            y: self.y + (o.y - self.y) * t,
        }
    }
}

/// A straight (not premultiplied) colour, each channel 0-1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    /// `0xRRGGBB` at full opacity.
    pub fn hex(rgb: u32) -> Rgba {
        let c = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
        Rgba {
            r: c(16),
            g: c(8),
            b: c(0),
            a: 1.0,
        }
    }

    /// Dimmed to `value` (0-1) with opacity `alpha` (OledSaver's `Dim`).
    pub fn dim(self, value: f32, alpha: f32) -> Rgba {
        Rgba {
            r: (self.r * value).clamp(0.0, 1.0),
            g: (self.g * value).clamp(0.0, 1.0),
            b: (self.b * value).clamp(0.0, 1.0),
            a: alpha.clamp(0.0, 1.0),
        }
    }

    /// From hue (degrees), saturation and value (0-1), with opacity `alpha`.
    pub fn hsv(hue: f64, saturation: f64, value: f64, alpha: f32) -> Rgba {
        let h = hue.rem_euclid(360.0);
        let chroma = value * saturation;
        let x = chroma * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = value - chroma;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let c = |v: f64| (v + m).clamp(0.0, 1.0) as f32;
        Rgba {
            r: c(r),
            g: c(g),
            b: c(b),
            a: alpha.clamp(0.0, 1.0),
        }
    }
}

/// A text's face and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Pixel size of the em.
    pub size: f32,
    /// 300 light, 400 regular, 600 semibold.
    pub weight: u16,
}

/// A transform applied to what follows: move, then turn (degrees, clockwise on
/// screen), then scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Point,
    pub rotate: f32,
    pub scale: (f32, f32),
}

impl Transform {
    pub fn translate(x: f32, y: f32) -> Transform {
        Transform {
            translate: Point { x, y },
            rotate: 0.0,
            scale: (1.0, 1.0),
        }
    }
}

pub trait Canvas {
    /// Applies `t` (inside the transforms already pushed) until the matching pop.
    fn push(&mut self, t: Transform);
    fn pop(&mut self);
    /// A line with round caps.
    fn line(&mut self, a: Point, b: Point, width: f32, color: Rgba);
    /// A line cut square at its ends (OledSaver's plain pen); Direct2D draws it in about
    /// half the time of a round-capped one.
    fn flat_line(&mut self, a: Point, b: Point, width: f32, color: Rgba);
    /// Joined lines with round caps and joins.
    fn polyline(&mut self, points: &[Point], width: f32, color: Rgba);
    fn fill_circle(&mut self, centre: Point, radius: f32, color: Rgba);
    fn circle(&mut self, centre: Point, radius: f32, width: f32, color: Rgba);
    /// The lower half of a disc (OledSaver's `FillPie(…, 0, 180)`).
    fn fill_half_disc(&mut self, centre: Point, radius: f32, color: Rgba);
    fn fill_rect(&mut self, at: Point, size: (f32, f32), color: Rgba);
    /// The text's width and height at `style`.
    fn measure(&mut self, text: &str, style: TextStyle) -> (f32, f32);
    /// Text with its top-left corner at `at`.
    fn text(&mut self, text: &str, at: Point, style: TextStyle, color: Rgba);
    /// The lowercase "milva" wordmark's size with letters of `em` pixels.
    fn wordmark_size(&mut self, em: f32) -> (f32, f32);
    /// The wordmark with its top-left corner at `at`: letters in `letters`, the dot of
    /// the dotless i in `dot`.
    fn wordmark(&mut self, at: Point, em: f32, letters: Rgba, dot: Rgba);
}

/// A canvas that keeps what was drawn, for tests (in the coordinates given, without
/// applying transforms; `depth` tracks the stack).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorder {
    pub lines: Vec<(Point, Point, f32, Rgba)>,
    pub flat_lines: Vec<(Point, Point, f32, Rgba)>,
    pub polylines: Vec<(Vec<Point>, f32, Rgba)>,
    pub dots: Vec<(Point, f32, Rgba)>,
    pub rings: Vec<(Point, f32, Rgba)>,
    pub halves: usize,
    pub rects: usize,
    pub texts: Vec<(String, Point, TextStyle, Rgba)>,
    pub wordmarks: Vec<(Point, f32)>,
    pub depth: i32,
    pub deepest: i32,
}

impl Canvas for Recorder {
    fn push(&mut self, _t: Transform) {
        self.depth += 1;
        self.deepest = self.deepest.max(self.depth);
    }
    fn pop(&mut self) {
        self.depth -= 1;
    }
    fn line(&mut self, a: Point, b: Point, width: f32, color: Rgba) {
        self.lines.push((a, b, width, color));
    }
    fn flat_line(&mut self, a: Point, b: Point, width: f32, color: Rgba) {
        self.flat_lines.push((a, b, width, color));
    }
    fn polyline(&mut self, points: &[Point], width: f32, color: Rgba) {
        self.polylines.push((points.to_vec(), width, color));
    }
    fn fill_circle(&mut self, centre: Point, radius: f32, color: Rgba) {
        self.dots.push((centre, radius, color));
    }
    fn circle(&mut self, centre: Point, radius: f32, _width: f32, color: Rgba) {
        self.rings.push((centre, radius, color));
    }
    fn fill_half_disc(&mut self, _centre: Point, _radius: f32, _color: Rgba) {
        self.halves += 1;
    }
    fn fill_rect(&mut self, _at: Point, _size: (f32, f32), _color: Rgba) {
        self.rects += 1;
    }
    /// Text is 0.55 em wide per character and one em tall.
    fn measure(&mut self, text: &str, style: TextStyle) -> (f32, f32) {
        (text.chars().count() as f32 * style.size * 0.55, style.size)
    }
    fn text(&mut self, text: &str, at: Point, style: TextStyle, color: Rgba) {
        self.texts.push((text.to_string(), at, style, color));
    }
    /// Five letters 0.6 em wide, 1.1 em tall.
    fn wordmark_size(&mut self, em: f32) -> (f32, f32) {
        (em * 3.0, em * 1.1)
    }
    fn wordmark(&mut self, at: Point, em: f32, _letters: Rgba, _dot: Rgba) {
        self.wordmarks.push((at, em));
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: hex channels, dimming clamps, the hue wheel's corners and wrap-around,
    //! distances and halfway points.
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn colours_from_hex_dimming_and_the_hue_wheel() {
        let y = Rgba::hex(0xFBC419);
        assert!(close(y.r, 251.0 / 255.0) && close(y.g, 196.0 / 255.0) && close(y.b, 25.0 / 255.0) && y.a == 1.0);
        let d = y.dim(0.5, 2.0);
        assert!(close(d.r, 251.0 / 510.0) && d.a == 1.0);
        let red = Rgba::hsv(0.0, 1.0, 1.0, 1.0);
        assert!(close(red.r, 1.0) && close(red.g, 0.0) && close(red.b, 0.0));
        let green = Rgba::hsv(120.0, 1.0, 1.0, 1.0);
        assert!(close(green.g, 1.0) && close(green.r, 0.0));
        assert_eq!(Rgba::hsv(-240.0, 1.0, 1.0, 1.0), green, "wraps around");
        let grey = Rgba::hsv(200.0, 0.0, 0.5, 0.3);
        assert!(close(grey.r, 0.5) && close(grey.g, 0.5) && close(grey.b, 0.5) && close(grey.a, 0.3));
    }

    #[test]
    fn points_measure_and_interpolate() {
        let (a, b) = (Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 });
        assert!(close(a.distance(b), 5.0));
        assert_eq!(a.lerp(b, 0.5), Point { x: 1.5, y: 2.0 });
    }
}
