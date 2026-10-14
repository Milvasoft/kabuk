//! "Takımyıldız": softly linked, slowly colour-shifting points drifting on a flow field,
//! and a wandering clock (OledSaver's `ConstellationScene`, the same constants).

use crate::canvas::{Canvas, Point, Rgba, TextStyle};
use crate::rng::Rng;
use crate::{Clock, Frame, Scene, Settings};

const HUE_CYCLE_SECONDS: f64 = 240.0;
const HUE_SPREAD: f64 = 70.0;
const CLOCK_PERIOD: (f64, f64) = (173.0, 109.0);
const HUE_BANDS: usize = 12;
const ALPHA_LEVELS: usize = 16;
const MAX_LINK_ALPHA: f32 = 0.7;
const NODE_ALPHA: f32 = 0.85;
const CLOCK_ALPHA: f32 = 0.55;
const BASE_LINK_DISTANCE: f32 = 170.0;
const MIN_NODES: usize = 12;
const MAX_NODES: usize = 900;
/// The area the density is given for.
const BASE_AREA: f64 = 1920.0 * 1080.0;
/// Clock text sizes as shares of the height, and the date's place under the time.
const TIME_SIZE: f32 = 0.12;
const DATE_SIZE: f32 = 0.028;
const DATE_OFFSET: f32 = 0.92;
const MIN_TIME_SIZE: f32 = 6.0;
const MIN_DATE_SIZE: f32 = 5.0;
const LIGHT: u16 = 300;
const REGULAR: u16 = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Node {
    x: f32,
    y: f32,
    angle: f32,
    speed: f32,
    size: f32,
}

pub struct Constellation {
    settings: Settings,
    rng: Rng,
    phase: f64,
    clock_phase: f64,
    frame: Frame,
    nodes: Vec<Node>,
    link_distance: f32,
    padding: f32,
}

impl Constellation {
    /// `index` makes each monitor's scene differ (OledSaver seeds with the monitor index
    /// and the clock; `seed` stands for the clock here).
    pub fn new(settings: Settings, index: u32, seed: u64) -> Constellation {
        let mut rng = Rng::new(seed ^ (u64::from(index) + 1).wrapping_mul(7919));
        let phase = f64::from(index) * 2.1 + rng.f64() * std::f64::consts::PI;
        let clock_phase = rng.f64() * std::f64::consts::TAU;
        Constellation {
            settings,
            rng,
            phase,
            clock_phase,
            frame: Frame::default(),
            nodes: Vec::new(),
            link_distance: 0.0,
            padding: 0.0,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn hue_band(&self, x: f32) -> usize {
        let position = (x + self.padding) / (self.frame.width + 2.0 * self.padding);
        ((position * HUE_BANDS as f32) as isize).clamp(0, HUE_BANDS as isize - 1) as usize
    }

    fn band_hue(base: f64, band: usize) -> f64 {
        base + band as f64 * HUE_SPREAD / (HUE_BANDS - 1) as f64
    }
}

impl Scene for Constellation {
    fn resize(&mut self, width: u32, height: u32) {
        if !self.frame.resize(width, height) {
            return;
        }
        let f = self.frame;
        self.link_distance = BASE_LINK_DISTANCE * f.unit;
        self.padding = self.link_distance;
        let area = f64::from(f.width) * f64::from(f.height) / BASE_AREA;
        let count = ((f64::from(self.settings.density) * area).round() as usize).clamp(MIN_NODES, MAX_NODES);
        let pad = f64::from(self.padding);
        self.nodes = (0..count)
            .map(|_| Node {
                x: (self.rng.f64() * (f64::from(f.width) + 2.0 * pad) - pad) as f32,
                y: (self.rng.f64() * (f64::from(f.height) + 2.0 * pad) - pad) as f32,
                angle: (self.rng.f64() * std::f64::consts::TAU) as f32,
                speed: self.rng.range(0.6, 1.4) as f32,
                size: self.rng.range(2.2, 4.6) as f32,
            })
            .collect();
    }

    fn update(&mut self, seconds: f64) {
        self.frame.time += seconds;
        let f = self.frame;
        let speed = f.unit * (6.0 + self.settings.speed as f32 * 5.0);
        let (min_x, max_x) = (-self.padding, f.width + self.padding);
        let (min_y, max_y) = (-self.padding, f.height + self.padding);
        let (span_x, span_y) = (max_x - min_x, max_y - min_y);
        let unit = f64::from(f.unit);
        for n in &mut self.nodes {
            // A slowly changing flow field bends every path so the motion never repeats.
            let field = (f64::from(n.x) / (unit * 260.0) + f.time * 0.07 + self.phase).sin()
                + (f64::from(n.y) / (unit * 210.0) - f.time * 0.05).cos();
            n.angle += (field * 0.35 * seconds) as f32;
            n.x += n.angle.cos() * n.speed * speed * seconds as f32;
            n.y += n.angle.sin() * n.speed * speed * seconds as f32;
            if n.x < min_x {
                n.x += span_x;
            } else if n.x > max_x {
                n.x -= span_x;
            }
            if n.y < min_y {
                n.y += span_y;
            } else if n.y > max_y {
                n.y -= span_y;
            }
        }
    }

    fn draw(&self, canvas: &mut dyn Canvas, clock: &Clock) {
        if self.nodes.is_empty() {
            return;
        }
        let f = self.frame;
        let fade = f.fade();
        let bright = f64::from(self.settings.brightness());
        let base = (f.time * 360.0 / HUE_CYCLE_SECONDS + self.phase * 40.0) % 360.0;
        // Links: closer points are linked brighter, in a few hue bands across the screen.
        let width = (1.2 * f.unit).max(1.0);
        let max_d2 = self.link_distance * self.link_distance;
        for (i, a) in self.nodes.iter().enumerate() {
            for b in &self.nodes[i + 1..] {
                let (dx, dy) = (a.x - b.x, a.y - b.y);
                let d2 = dx * dx + dy * dy;
                if d2 >= max_d2 {
                    continue;
                }
                let closeness = 1.0 - d2.sqrt() / self.link_distance;
                let level = (closeness * (ALPHA_LEVELS - 1) as f32 + 0.5) as usize;
                if level == 0 {
                    continue;
                }
                let band = self.hue_band((a.x + b.x) * 0.5);
                let alpha = level as f32 / (ALPHA_LEVELS - 1) as f32 * MAX_LINK_ALPHA * fade;
                let colour = Rgba::hsv(Self::band_hue(base, band), 0.75, bright, alpha);
                canvas.flat_line(Point { x: a.x, y: a.y }, Point { x: b.x, y: b.y }, width, colour);
            }
        }
        for n in &self.nodes {
            let at = Point { x: n.x, y: n.y };
            if !f.is_visible(at, 10.0) {
                continue;
            }
            let colour = Rgba::hsv(
                Self::band_hue(base, self.hue_band(n.x)),
                0.55,
                bright,
                NODE_ALPHA * fade,
            );
            canvas.fill_circle(at, (n.size * f.unit).max(1.0) / 2.0, colour);
        }
        if self.settings.show_clock {
            let time_style = TextStyle {
                size: (f.height * TIME_SIZE).max(MIN_TIME_SIZE),
                weight: LIGHT,
            };
            let date_style = TextStyle {
                size: (f.height * DATE_SIZE).max(MIN_DATE_SIZE),
                weight: REGULAR,
            };
            let t = canvas.measure(&clock.time, time_style);
            let d = canvas.measure(&clock.date, date_style);
            let block = t.0.max(d.0);
            let offset = t.1 * DATE_OFFSET;
            let at = f.wander(CLOCK_PERIOD, self.clock_phase, (block, offset + d.1), 0.0);
            let colour = Rgba::hsv(base + 150.0, 0.3, bright, CLOCK_ALPHA * fade);
            canvas.text(
                &clock.time,
                Point {
                    x: at.x + (block - t.0) / 2.0,
                    y: at.y,
                },
                time_style,
                colour,
            );
            canvas.text(
                &clock.date,
                Point {
                    x: at.x + (block - d.0) / 2.0,
                    y: at.y + offset,
                },
                date_style,
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the number of points follows the density and the area within its
    //! bounds; points wrap around and never stop; links only join close points; the
    //! brightness caps the colours; the scene fades in; the clock stays on screen and is
    //! left out when turned off; two monitors differ.
    use super::*;
    use crate::canvas::Recorder;

    fn clock() -> Clock {
        Clock {
            time: "20:31".into(),
            date: "12 Ekim Pazartesi".into(),
        }
    }

    fn scene(w: u32, h: u32, settings: Settings) -> Constellation {
        let mut s = Constellation::new(settings, 0, 42);
        s.resize(w, h);
        s
    }

    #[test]
    fn the_point_count_follows_density_and_area() {
        assert_eq!(scene(1920, 1080, Settings::default()).node_count(), 110);
        assert_eq!(scene(3840, 2160, Settings::default()).node_count(), 440);
        let sparse = Settings {
            density: 20,
            ..Settings::default()
        };
        assert_eq!(scene(320, 180, sparse).node_count(), MIN_NODES);
        let dense = Settings {
            density: 200,
            ..Settings::default()
        };
        assert_eq!(scene(7680, 4320, dense).node_count(), MAX_NODES);
    }

    #[test]
    fn points_keep_moving_and_wrap_inside_the_padded_area() {
        let mut s = scene(1920, 1080, Settings::default());
        let start: Vec<Node> = s.nodes.clone();
        for _ in 0..30 * 120 {
            s.update(1.0 / 30.0);
        }
        let (pad, w, h) = (s.padding, s.frame.width, s.frame.height);
        assert!(s
            .nodes
            .iter()
            .all(|n| n.x >= -pad && n.x <= w + pad && n.y >= -pad && n.y <= h + pad));
        let moved = s
            .nodes
            .iter()
            .zip(&start)
            .filter(|(a, b)| a.x != b.x || a.y != b.y)
            .count();
        assert_eq!(moved, s.nodes.len(), "a point stood still");
    }

    #[test]
    fn links_join_only_close_points_and_colours_stay_under_the_brightness() {
        let dim = Settings {
            brightness: 30,
            ..Settings::default()
        };
        let mut s = scene(1920, 1080, dim);
        s.update(5.0);
        let mut r = Recorder::default();
        s.draw(&mut r, &clock());
        // OledSaver links with its plain pen: square ends.
        assert!(r.lines.is_empty() && !r.flat_lines.is_empty() && !r.dots.is_empty());
        for (a, b, _, c) in &r.flat_lines {
            let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            assert!(d < s.link_distance, "{d}");
            assert!(c.r.max(c.g).max(c.b) <= 0.3 + 1e-4 && c.a <= MAX_LINK_ALPHA + 1e-4);
        }
        for (_, _, c) in &r.dots {
            assert!(c.r.max(c.g).max(c.b) <= 0.3 + 1e-4);
        }
    }

    #[test]
    fn the_scene_fades_in_and_the_clock_wanders_on_screen() {
        let mut s = scene(1920, 1080, Settings::default());
        let mut first = Recorder::default();
        s.draw(&mut first, &clock());
        assert!(
            first.dots.iter().all(|(_, _, c)| c.a == 0.0),
            "not faded in at the start"
        );
        let mut later = Recorder::default();
        s.update(10.0);
        s.draw(&mut later, &clock());
        assert!(later.dots.iter().all(|(_, _, c)| (c.a - NODE_ALPHA).abs() < 1e-4));
        assert_eq!(later.texts.len(), 2);
        for (text, at, style, _) in &later.texts {
            let w = text.chars().count() as f32 * style.size * 0.55;
            assert!(
                at.x >= 0.0 && at.x + w <= 1920.0 + 0.5 && at.y >= 0.0 && at.y <= 1080.0,
                "{text} at {at:?}"
            );
        }
        let quiet = Settings {
            show_clock: false,
            ..Settings::default()
        };
        let mut q = scene(1920, 1080, quiet);
        q.update(10.0);
        let mut none = Recorder::default();
        q.draw(&mut none, &clock());
        assert!(none.texts.is_empty());
    }

    #[test]
    fn two_monitors_differ() {
        let mut a = Constellation::new(Settings::default(), 0, 42);
        let mut b = Constellation::new(Settings::default(), 1, 42);
        a.resize(1920, 1080);
        b.resize(1920, 1080);
        assert_ne!(a.nodes, b.nodes);
    }
}
