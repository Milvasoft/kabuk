//! Plain geometry in device-independent pixels (DIPs) unless stated otherwise.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }

    /// Smallest rect containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    pub fn all(v: f32) -> Insets {
        Insets {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Insets {
        Insets {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// Device scale: physical pixels per DIP (DPI / 96).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale(pub f32);

impl Scale {
    pub const BASE_DPI: f32 = 96.0;

    pub fn from_dpi(dpi: u32) -> Scale {
        Scale(dpi as f32 / Scale::BASE_DPI)
    }

    /// DIPs to whole physical pixels, rounded.
    pub fn px(&self, dips: f32) -> i32 {
        (dips * self.0).round() as i32
    }

    pub fn dips(&self, px: i32) -> f32 {
        px as f32 / self.0
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: half-open containment, union of disjoint and nested rects, DPI
    //! rounding at 100/125/150/175 %, negative offsets.
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(Point { x: 10.0, y: 20.0 }));
        assert!(!r.contains(Point { x: 40.0, y: 20.0 }));
        assert!(!r.contains(Point { x: 10.0, y: 60.0 }));
        assert_eq!(r.offset(-10.0, -20.0), Rect::new(0.0, 0.0, 30.0, 40.0));
        assert_eq!(r.union(&Rect::new(0.0, 0.0, 5.0, 5.0)), Rect::new(0.0, 0.0, 40.0, 60.0));
        assert_eq!(r.union(&Rect::new(12.0, 22.0, 1.0, 1.0)), r);
        assert_eq!(Insets::symmetric(14.0, 0.0).horizontal(), 28.0);
        assert_eq!(Insets::all(2.0).vertical(), 4.0);
    }

    #[test]
    fn dpi_scaling() {
        assert_eq!(Scale::from_dpi(96).px(38.0), 38);
        assert_eq!(Scale::from_dpi(120).px(38.0), 48); // 47.5 rounds up
        assert_eq!(Scale::from_dpi(144).px(38.0), 57);
        assert_eq!(Scale::from_dpi(168).px(14.0), 25); // 24.5
        assert_eq!(Scale::from_dpi(120).dips(48), 38.4);
    }
}
