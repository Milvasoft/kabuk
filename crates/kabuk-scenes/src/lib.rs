//! The animated scenes of Kabuk's wallpaper and screen saver (SPEC §2.7, §2.8), ported
//! from the user's OledSaver (`reference/OledSaver`, DECISIONS D12). Pure: a scene is a
//! simulation advanced by time that draws through a [`Canvas`]; the wallpaper and the
//! saver draw it with Direct2D. Every scene keeps the background black and never leaves
//! anything in one place for long (OLED care).

pub mod canvas;
pub mod constellation;
pub mod milva;
pub mod rng;

pub use canvas::{Canvas, Point, Rgba, TextStyle, Transform};

/// The user's settings of a scene (OledSaver's `SaverSettings`, same ranges).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Peak brightness, percent (10-100).
    pub brightness: u32,
    /// Points per 1920 × 1080 area (20-200).
    pub density: u32,
    /// Flow speed (1-10).
    pub speed: u32,
    /// The wandering clock and date.
    pub show_clock: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            brightness: 60,
            density: 110,
            speed: 5,
            show_clock: true,
        }
    }
}

impl Settings {
    /// Brightness as 0-1.
    pub fn brightness(&self) -> f32 {
        self.brightness as f32 / 100.0
    }
}

/// The clock's words for the frame being drawn (the host formats them in the user's
/// language: "20:31", "12 Ekim Pazartesi").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    pub time: String,
    pub date: String,
}

/// A scene of one monitor.
pub trait Scene {
    /// The drawing area changed (physical pixels).
    fn resize(&mut self, width: u32, height: u32);
    /// Advances the simulation by `seconds`.
    fn update(&mut self, seconds: f64);
    /// Draws the frame over a black surface.
    fn draw(&self, canvas: &mut dyn Canvas, clock: &Clock);
    /// Measures what the scene needs measured with the real fonts (after a resize, before
    /// drawing); nothing by default.
    fn measure(&mut self, _canvas: &mut dyn Canvas, _clock: &Clock) {}
}

/// Time keeping shared by the scenes (OledSaver's `Scene` base): the size, the scale
/// relative to a 1080-pixel screen, the time and the fade-in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    /// `min(width, height) / 1080`, at least 0.15.
    pub unit: f32,
    /// Seconds since the scene started.
    pub time: f64,
}

/// The scene fades in over this long instead of popping up.
pub const FADE_IN_SECONDS: f64 = 1.5;
/// The smallest scale a scene draws at (tiny preview windows).
pub const MIN_UNIT: f32 = 0.15;
/// The screen height a scene's sizes are given for.
pub const BASE_HEIGHT: f32 = 1080.0;

impl Frame {
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let (w, h) = (width as f32, height as f32);
        if width == 0 || height == 0 || (w == self.width && h == self.height) {
            return false;
        }
        self.width = w;
        self.height = h;
        self.unit = (w.min(h) / BASE_HEIGHT).max(MIN_UNIT);
        true
    }

    /// 0 → 1 over the first [`FADE_IN_SECONDS`].
    pub fn fade(&self) -> f32 {
        (self.time / FADE_IN_SECONDS).min(1.0) as f32
    }

    /// The top-left corner of a `block` wandering over the whole area on a Lissajous path
    /// with long, unrelated periods, so it never parks anywhere.
    pub fn wander(&self, period: (f64, f64), phase: f64, block: (f32, f32), margin: f32) -> Point {
        use std::f64::consts::TAU;
        let u = 0.5 + 0.5 * (self.time * TAU / period.0 + phase).sin();
        let v = 0.5 + 0.5 * (self.time * TAU / period.1 + phase * 1.7).sin();
        Point {
            x: margin + (u as f32) * (self.width - 2.0 * margin - block.0).max(0.0),
            y: margin + (v as f32) * (self.height - 2.0 * margin - block.1).max(0.0),
        }
    }

    /// On screen, give or take `slack` pixels.
    pub fn is_visible(&self, p: Point, slack: f32) -> bool {
        p.x >= -slack && p.x <= self.width + slack && p.y >= -slack && p.y <= self.height + slack
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a zero or unchanged size is ignored, the scale has a floor, the fade
    //! ends at one, the wandering block stays inside the area.
    use super::*;

    #[test]
    fn the_frame_scales_fades_and_wanders_inside() {
        let mut f = Frame::default();
        assert!(!f.resize(0, 1080));
        assert!(f.resize(2560, 1440));
        assert!(!f.resize(2560, 1440));
        assert!((f.unit - 1440.0 / 1080.0).abs() < 1e-6);
        let mut tiny = Frame::default();
        tiny.resize(100, 60);
        assert_eq!(tiny.unit, MIN_UNIT);
        assert_eq!(f.fade(), 0.0);
        f.time = FADE_IN_SECONDS * 2.0;
        assert_eq!(f.fade(), 1.0);
        for t in 0..500 {
            f.time = f64::from(t) * 7.3;
            let p = f.wander((173.0, 109.0), 0.4, (400.0, 200.0), 0.0);
            assert!(p.x >= 0.0 && p.x + 400.0 <= f.width + 0.01 && p.y >= 0.0 && p.y + 200.0 <= f.height + 0.01);
        }
        assert!(f.is_visible(Point { x: -5.0, y: 10.0 }, 10.0));
        assert!(!f.is_visible(Point { x: -50.0, y: 10.0 }, 10.0));
    }
}
