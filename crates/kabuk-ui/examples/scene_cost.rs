//! Where a scene frame's CPU time goes (D36, PLAN M12): each scene's draw calls counted,
//! then the scene drawn at a few sizes, whole, with one kind of drawing left out and with
//! nothing at all, and the wall time per frame printed (the GPU part of a frame is under
//! a tenth of a millisecond, so this is nearly all CPU).
//!
//!   cargo run --release -p kabuk-ui --example scene_cost

use std::time::Instant;

use kabuk_scenes::constellation::Constellation;
use kabuk_scenes::milva::Milva;
use kabuk_scenes::{Canvas, Clock, Point, Rgba, Scene, Settings, TextStyle, Transform};
use kabuk_ui::render::{Gpu, SceneCache};

const FRAMES: usize = 90;
const STEP: f64 = 1.0 / 30.0;
/// Seconds played before timing: the eye open, the points spread.
const WARM_UP: f64 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Skip {
    Nothing,
    Lines,
    Circles,
    Text,
    Paths,
    /// Nothing drawn: what a frame costs anyway.
    Everything,
}

/// Passes every call on except the kind skipped.
struct Filter<'a> {
    inner: &'a mut dyn Canvas,
    skip: Skip,
}

impl Canvas for Filter<'_> {
    fn push(&mut self, t: Transform) {
        self.inner.push(t);
    }
    fn pop(&mut self) {
        self.inner.pop();
    }
    fn line(&mut self, a: Point, b: Point, width: f32, c: Rgba) {
        if self.skip != Skip::Lines {
            self.inner.line(a, b, width, c);
        }
    }
    fn flat_line(&mut self, a: Point, b: Point, width: f32, c: Rgba) {
        if self.skip != Skip::Lines {
            self.inner.flat_line(a, b, width, c);
        }
    }
    fn polyline(&mut self, points: &[Point], width: f32, c: Rgba) {
        if self.skip != Skip::Paths {
            self.inner.polyline(points, width, c);
        }
    }
    fn fill_circle(&mut self, centre: Point, radius: f32, c: Rgba) {
        if self.skip != Skip::Circles {
            self.inner.fill_circle(centre, radius, c);
        }
    }
    fn circle(&mut self, centre: Point, radius: f32, width: f32, c: Rgba) {
        if self.skip != Skip::Circles {
            self.inner.circle(centre, radius, width, c);
        }
    }
    fn fill_half_disc(&mut self, centre: Point, radius: f32, c: Rgba) {
        if self.skip != Skip::Paths {
            self.inner.fill_half_disc(centre, radius, c);
        }
    }
    fn fill_rect(&mut self, at: Point, size: (f32, f32), c: Rgba) {
        self.inner.fill_rect(at, size, c);
    }
    fn measure(&mut self, text: &str, style: TextStyle) -> (f32, f32) {
        self.inner.measure(text, style)
    }
    fn text(&mut self, text: &str, at: Point, style: TextStyle, c: Rgba) {
        if self.skip != Skip::Text {
            self.inner.text(text, at, style, c);
        }
    }
    fn wordmark_size(&mut self, em: f32) -> (f32, f32) {
        self.inner.wordmark_size(em)
    }
    fn wordmark(&mut self, at: Point, em: f32, letters: Rgba, dot: Rgba) {
        if self.skip != Skip::Text {
            self.inner.wordmark(at, em, letters, dot);
        }
    }
}

fn main() {
    let gpu = Gpu::new().expect("gpu");
    let clock = Clock {
        time: "20:31".into(),
        date: "12 Ekim Pazartesi".into(),
    };
    for name in ["milva", "constellation"] {
        for size in [(1920u32, 1080u32), (2560, 1440), (3840, 2160)] {
            {
                let mut scene: Box<dyn Scene> = if name == "milva" {
                    Box::new(Milva::new(Settings::default(), 0, 42))
                } else {
                    Box::new(Constellation::new(Settings::default(), 0, 42))
                };
                scene.resize(size.0, size.1);
                for _ in 0..(WARM_UP / STEP) as usize {
                    scene.update(STEP);
                }
                let mut r = kabuk_scenes::canvas::Recorder::default();
                scene.draw(&mut r, &clock);
                let segs: usize = r.polylines.iter().map(|p| p.0.len()).sum();
                println!(
                    "{name} {}x{}: {} lines ({} flat), {} polylines ({segs} points), {} dots, {} rings, {} halves, {} texts",
                    size.0,
                    size.1,
                    r.lines.len() + r.flat_lines.len(),
                    r.flat_lines.len(),
                    r.polylines.len(),
                    r.dots.len(),
                    r.rings.len(),
                    r.halves,
                    r.texts.len()
                );
            }
            for skip in [
                Skip::Everything,
                Skip::Nothing,
                Skip::Lines,
                Skip::Paths,
                Skip::Circles,
                Skip::Text,
            ] {
                let mut scene: Box<dyn Scene> = if name == "milva" {
                    Box::new(Milva::new(Settings::default(), 0, 42))
                } else {
                    Box::new(Constellation::new(Settings::default(), 0, 42))
                };
                scene.resize(size.0, size.1);
                let mut cache = SceneCache::default();
                let _ = gpu.render_scene(1, 1, &mut cache, |c| scene.measure(c, &clock));
                for _ in 0..(WARM_UP / STEP) as usize {
                    scene.update(STEP);
                }
                let scene = std::cell::RefCell::new(scene);
                let started = Instant::now();
                gpu.time_scene(size, FRAMES, &mut cache, || scene.borrow_mut().update(STEP), &|c| {
                    if skip != Skip::Everything {
                        scene.borrow().draw(&mut Filter { inner: c, skip }, &clock);
                    }
                })
                .expect("timed");
                let per = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
                println!(
                    "{name:<14} {:>4}x{:<4} skip {:<8} {per:6.2} ms/frame = {:5.1} % of a core at 30 fps",
                    size.0,
                    size.1,
                    format!("{skip:?}"),
                    per * 30.0 / 10.0
                );
            }
        }
    }
}
