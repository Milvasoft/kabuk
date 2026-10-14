//! Milva's circuit eye (OledSaver's `MilvaScene`, the same geometry and constants): an eye
//! from the Milvasoft logo opens somewhere on the board, blinks and follows the signals;
//! circuit traces route out of its pads and across the screen on a 45° grid without
//! touching, signals run along them, and everything fades away again so no pixel keeps
//! the same job for long. The "milva" wordmark and the clock stand beside the eye.

use crate::canvas::{Canvas, Point, Rgba, TextStyle, Transform};
use crate::rng::Rng;
use crate::{Clock, Frame, Scene, Settings};

const YELLOW: u32 = 0xFBC419;
const ORANGE: u32 = 0xF38424;
const GREY: u32 = 0xC0BFBE;

/// Grid steps, clockwise from east (y down): E, SE, S, SW, W, NW, N, NE.
const DIRECTIONS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

// Eye geometry, in logo units around the eye's centre (traced from the Milvasoft logo).
const BAR_ANGLE: f32 = -8.1;
const BODY_RADIUS: f32 = 60.0;
const PUPIL_RADIUS: f32 = 17.0;
const PUPIL_REST_X: f32 = 23.0;
const PUPIL_TRAVEL: f32 = 34.0;
const LOGO_STROKE: f32 = 12.0;
const RING_STROKE: f32 = 8.0;
const EYE_EXTENT: f32 = 168.0;
const BAR: (Point, Point) = (Point { x: -109.0, y: 15.0 }, Point { x: 95.0, y: -14.0 });
const LEGS: [&[Point]; 4] = [
    &[Point { x: 13.0, y: -12.0 }, Point { x: 35.0, y: -69.0 }],
    &[
        Point { x: -48.0, y: 6.0 },
        Point { x: -46.0, y: -46.0 },
        Point { x: -130.0, y: -38.0 },
    ],
    &[Point { x: -25.0, y: 54.0 }, Point { x: -53.0, y: 122.0 }],
    &[
        Point { x: 50.0, y: 46.0 },
        Point { x: 117.0, y: 54.0 },
        Point { x: 132.0, y: 10.0 },
    ],
];

/// One of the eye's four legs: its pad, how it looks and which way a trace leaves it.
struct EyePad {
    centre: Point,
    radius: f32,
    ring: bool,
    direction: usize,
}

const EYE_PADS: [EyePad; 4] = [
    EyePad {
        centre: Point { x: 35.0, y: -69.0 },
        radius: 18.0,
        ring: false,
        direction: 6,
    },
    EyePad {
        centre: Point { x: -145.0, y: -39.0 },
        radius: 15.0,
        ring: true,
        direction: 4,
    },
    EyePad {
        centre: Point { x: -53.0, y: 122.0 },
        radius: 18.0,
        ring: false,
        direction: 2,
    },
    EyePad {
        centre: Point { x: 136.0, y: -6.0 },
        radius: 15.0,
        ring: true,
        direction: 0,
    },
];

// Tuning.
const EYE_SCALE: f32 = 1.25;
const CELL_SIZE: f32 = 32.0;
const MIN_CELL: f32 = 4.0;
const BOARD_TRACE_WIDTH: f32 = 7.0;
const EYE_TRACE_WIDTH: f32 = 9.5;
const TRACE_LEVEL: f32 = 0.32;
const EYE_TRACE_LEVEL: f32 = 0.55;
const EYE_LEVEL: f32 = 0.8;
const WORDMARK_LEVEL: f32 = 0.75;
const CLOCK_LEVEL: f32 = 0.55;
const MIN_RUN: u32 = 2;
const MIN_BOARD_CELLS: usize = 4;
const MIN_EYE_CELLS: usize = 3;
const BOARD_TRACES_PER_1080P: f64 = 12.0;
const DEFAULT_DENSITY: f64 = 110.0;
const GROW_CELLS_PER_SECOND: f32 = 16.0;
const PULSE_SPEED: f32 = 420.0;
const PULSE_TRAIL: f32 = 90.0;
const TRAIL_SAMPLES: usize = 8;
const GLOW_SECONDS: f32 = 1.1;
const TRACE_LIFE: (f64, f64) = (14.0, 32.0);
const TRACE_FADE_SECONDS: f64 = 2.5;
const EYE_LIFE: (f64, f64) = (40.0, 65.0);
const EYE_OPEN_SECONDS: f64 = 0.7;
const EYE_CLOSE_SECONDS: f64 = 0.7;
const EYE_GAP_SECONDS: f64 = 1.2;
const BLINK_SECONDS: f64 = 0.28;
const BRAND_GAP_FACTOR: f32 = 0.12;
/// Wordmark and clock sizes as shares of the height.
const WORDMARK_SIZE: f32 = 0.11;
const CLOCK_SIZE: f32 = 0.026;
const MIN_WORDMARK: f32 = 8.0;
const MIN_CLOCK: f32 = 5.0;
const SEMIBOLD: u16 = 600;
/// A date longer than today's still fits the brand block.
const CLOCK_ROOM: f32 = 1.25;
/// The clock sits this much of its size under the wordmark.
const CLOCK_GAP: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Growing,
    Live,
    Fading,
}

/// A circuit trace routed cell by cell over the board grid.
#[derive(Clone, Debug)]
struct Trace {
    id: i32,
    /// The eye pad it leaves from, if it is one of the eye's.
    eye_pad: Option<usize>,
    width: f32,
    start_pad: bool,
    start_ring: bool,
    end_ring: bool,
    target_cells: usize,
    direction: usize,
    run: u32,
    grow_after: f64,
    visible: f32,
    state: State,
    live_until: f64,
    fade_start: f64,
    fade_end: f64,
    next_pulse: f64,
    pulse: f32,
    reverse: bool,
    start_glow: f32,
    end_glow: f32,
    cells: Vec<(i32, i32)>,
    points: Vec<Point>,
    distances: Vec<f32>,
}

impl Trace {
    fn new(id: i32) -> Trace {
        Trace {
            id,
            eye_pad: None,
            width: 0.0,
            start_pad: false,
            start_ring: false,
            end_ring: false,
            target_cells: 0,
            direction: 0,
            run: 0,
            grow_after: 0.0,
            visible: 0.0,
            state: State::Growing,
            live_until: 0.0,
            fade_start: 0.0,
            fade_end: 0.0,
            next_pulse: 0.0,
            pulse: -1.0,
            reverse: false,
            start_glow: 0.0,
            end_glow: 0.0,
            cells: Vec::new(),
            points: Vec::new(),
            distances: Vec::new(),
        }
    }

    fn leaves_eye(&self) -> bool {
        self.eye_pad.is_some()
    }

    fn length(&self) -> f32 {
        self.distances.last().copied().unwrap_or(0.0)
    }

    fn add_point(&mut self, p: Point) {
        let d = match self.points.last() {
            Some(last) => self.length() + last.distance(p),
            None => 0.0,
        };
        self.distances.push(d);
        self.points.push(p);
    }

    /// Where the trace is `distance` pixels from its start.
    fn point_at(&self, distance: f32) -> Point {
        if distance <= 0.0 || self.points.len() == 1 {
            return self.points[0];
        }
        for i in 1..self.points.len() {
            if distance > self.distances[i] {
                continue;
            }
            let segment = self.distances[i] - self.distances[i - 1];
            let t = if segment <= 0.0 {
                1.0
            } else {
                (distance - self.distances[i - 1]) / segment
            };
            return self.points[i - 1].lerp(self.points[i], t);
        }
        self.points[self.points.len() - 1]
    }

    /// The part drawn so far (all of it once grown).
    fn visible_points(&self) -> Vec<Point> {
        if self.state != State::Growing || self.visible >= self.length() {
            return self.points.clone();
        }
        let mut out = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if self.distances[i] <= self.visible {
                out.push(*p);
                continue;
            }
            out.push(self.point_at(self.visible));
            break;
        }
        out
    }
}

/// A rectangle (the wordmark and clock block).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Area {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Area {
    fn inflate(self, by: f32) -> Area {
        Area {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2.0 * by,
            h: self.h + 2.0 * by,
        }
    }

    fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }
}

/// Milva's eye while it is open somewhere on the board.
#[derive(Clone, Debug)]
struct Eye {
    centre: Point,
    rotation: f32,
    scale: f32,
    open_at: f64,
    close_at: f64,
    closing: bool,
    traces_created: bool,
    next_blink: f64,
    blink_start: f64,
    pupil_x: f32,
    flash: f32,
    pad_glow: [f32; 4],
    /// The ids of its traces.
    traces: Vec<i32>,
    brand: Option<Area>,
}

impl Eye {
    fn radius(&self) -> f32 {
        EYE_EXTENT * self.scale
    }

    fn to_world(&self, local: Point) -> Point {
        let a = self.rotation.to_radians();
        let (c, s) = (a.cos(), a.sin());
        let (x, y) = (local.x * self.scale, local.y * self.scale);
        Point {
            x: self.centre.x + x * c - y * s,
            y: self.centre.y + x * s + y * c,
        }
    }
}

pub struct Milva {
    settings: Settings,
    rng: Rng,
    phase: f64,
    frame: Frame,
    traces: Vec<Trace>,
    owners: Vec<i32>,
    columns: i32,
    rows: i32,
    cell: f32,
    origin: Point,
    next_id: i32,
    eye: Option<Eye>,
    next_eye_at: f64,
    next_spawn_at: f64,
    /// The wordmark's em and the brand block's size (set on the first draw that can
    /// measure; `None` until then, and the eye then stands alone).
    brand_size: Option<(f32, f32)>,
}

impl Milva {
    pub fn new(settings: Settings, index: u32, seed: u64) -> Milva {
        let mut rng = Rng::new(seed ^ (u64::from(index) + 1).wrapping_mul(7919));
        let phase = f64::from(index) * 2.1 + rng.f64() * std::f64::consts::PI;
        Milva {
            settings,
            rng,
            phase,
            frame: Frame::default(),
            traces: Vec::new(),
            owners: Vec::new(),
            columns: 0,
            rows: 0,
            cell: 0.0,
            origin: Point::default(),
            next_id: 1,
            eye: None,
            next_eye_at: 0.0,
            next_spawn_at: 0.0,
            brand_size: None,
        }
    }

    /// The wordmark and clock block's size, measured on `canvas` (the date has room to
    /// grow), for the eye that opens next.
    pub fn measure_brand(&mut self, canvas: &mut dyn Canvas, clock: &Clock) {
        let h = self.frame.height;
        let em = (h * WORDMARK_SIZE).max(MIN_WORDMARK);
        let mark = canvas.wordmark_size(em);
        self.brand_size = Some(if self.settings.show_clock {
            let style = self.clock_style();
            let c = canvas.measure(&clock_text(clock), style);
            (mark.0.max(c.0 * CLOCK_ROOM), mark.1 + style.size * CLOCK_GAP + c.1)
        } else {
            mark
        });
    }

    fn clock_style(&self) -> TextStyle {
        TextStyle {
            size: (self.frame.height * CLOCK_SIZE).max(MIN_CLOCK),
            weight: SEMIBOLD,
        }
    }

    fn speed_factor(&self) -> f64 {
        0.5 + f64::from(self.settings.speed) * 0.1
    }

    /// Small, always moving accents may run brighter than the rest.
    fn accent(&self) -> f32 {
        (self.settings.brightness() * 1.4).min(1.0)
    }

    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        if hi <= lo {
            (lo + hi) / 2.0
        } else {
            lo + self.rng.f64() as f32 * (hi - lo)
        }
    }

    // ------------------------------------------------------------ the grid

    fn cell_centre(&self, (x, y): (i32, i32)) -> Point {
        Point {
            x: self.origin.x + x as f32 * self.cell,
            y: self.origin.y + y as f32 * self.cell,
        }
    }

    fn nearest_cell(&self, p: Point) -> (i32, i32) {
        (
            ((p.x - self.origin.x) / self.cell).round() as i32,
            ((p.y - self.origin.y) / self.cell).round() as i32,
        )
    }

    fn in_grid(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.columns && y < self.rows
    }

    /// The trace owning the cell, 0 when free, -1 outside the board.
    fn owner(&self, x: i32, y: i32) -> i32 {
        if self.in_grid(x, y) {
            self.owners[(y * self.columns + x) as usize]
        } else {
            -1
        }
    }

    fn occupy(&mut self, (x, y): (i32, i32), id: i32) {
        if self.in_grid(x, y) {
            self.owners[(y * self.columns + x) as usize] = id;
        }
    }

    fn release(&mut self, t: &Trace) {
        for &(x, y) in &t.cells {
            if self.owner(x, y) == t.id {
                self.owners[(y * self.columns + x) as usize] = 0;
            }
        }
        if let Some(e) = self.eye.as_mut() {
            e.traces.retain(|id| *id != t.id);
        }
    }

    /// Inside the eye's space or its brand block; traces route around both.
    fn inside_eye(&self, p: Point) -> bool {
        let Some(e) = &self.eye else { return false };
        if p.distance(e.centre) < e.radius() + self.cell * 0.5 {
            return true;
        }
        e.brand.is_some_and(|b| b.inflate(self.cell * 0.6).contains(p))
    }

    fn blocked(&self, x: i32, y: i32) -> bool {
        self.owner(x, y) != 0 || self.inside_eye(self.cell_centre((x, y)))
    }

    /// A free cell with nothing around it: a good place for a new trace to start.
    fn clear_area(&self, (x, y): (i32, i32)) -> bool {
        (-1..=1).all(|dy| (-1..=1).all(|dx| !self.blocked(x + dx, y + dy)))
    }

    /// The trace may step from `from` towards `direction`: the cell is free and no trace
    /// (itself included) sits right next to it, so traces keep a gap and never cross.
    fn can_enter(&self, from: (i32, i32), direction: usize) -> bool {
        let (sx, sy) = DIRECTIONS[direction];
        let (x, y) = (from.0 + sx, from.1 + sy);
        if self.blocked(x, y) {
            return false;
        }
        (0..8).step_by(2).all(|d| {
            let (nx, ny) = (x + DIRECTIONS[d].0, y + DIRECTIONS[d].1);
            (nx, ny) == from || self.owner(nx, ny) <= 0
        })
    }

    fn no_neighbours(&self, (x, y): (i32, i32)) -> bool {
        (0..8)
            .step_by(2)
            .all(|d| self.owner(x + DIRECTIONS[d].0, y + DIRECTIONS[d].1) <= 0)
    }

    // ------------------------------------------------------------ traces

    fn update_traces(&mut self, seconds: f64) {
        let grow = GROW_CELLS_PER_SECOND * self.cell * self.speed_factor() as f32 * seconds as f32;
        let pulse = PULSE_SPEED * self.frame.unit * self.speed_factor() as f32 * seconds as f32;
        let decay = seconds as f32 / GLOW_SECONDS;
        let time = self.frame.time;
        let mut i = self.traces.len();
        while i > 0 {
            i -= 1;
            let mut t = self.traces[i].clone();
            t.start_glow = (t.start_glow - decay).max(0.0);
            t.end_glow = (t.end_glow - decay).max(0.0);
            match t.state {
                State::Growing => {
                    if time >= t.grow_after {
                        self.grow(&mut t, grow);
                    }
                }
                State::Live => {
                    if !t.leaves_eye() && time >= t.live_until {
                        begin_fade(&mut t, time, TRACE_FADE_SECONDS);
                    } else {
                        self.advance_pulse(&mut t, pulse);
                    }
                }
                State::Fading => {
                    if time >= t.fade_end {
                        self.release(&t);
                        self.traces.remove(i);
                        continue;
                    }
                }
            }
            self.traces[i] = t;
        }
    }

    fn grow(&mut self, t: &mut Trace, distance: f32) {
        t.visible += distance;
        while t.visible >= t.length() {
            if t.cells.len() >= t.target_cells || !self.extend(t) {
                t.visible = t.length();
                self.finish_growth(t);
                return;
            }
        }
    }

    /// One cell further: mostly straight, 45° turns only after a short straight run.
    fn extend(&mut self, t: &mut Trace) -> bool {
        let head = t.cells[t.cells.len() - 1];
        let straight = t.direction;
        let left = (straight + 7) % 8;
        let right = (straight + 1) % 8;
        let can_turn = t.run >= MIN_RUN;
        let ws = if self.can_enter(head, straight) { 5.0 } else { 0.0 };
        let wl = if can_turn && self.can_enter(head, left) {
            1.0
        } else {
            0.0
        };
        let wr = if can_turn && self.can_enter(head, right) {
            1.0
        } else {
            0.0
        };
        let total = ws + wl + wr;
        if total <= 0.0 {
            return false;
        }
        let pick = self.rng.f64() * total;
        let direction = if pick < ws {
            straight
        } else if pick < ws + wl {
            left
        } else {
            right
        };
        let (sx, sy) = DIRECTIONS[direction];
        let next = (head.0 + sx, head.1 + sy);
        self.occupy(next, t.id);
        t.cells.push(next);
        t.add_point(self.cell_centre(next));
        t.run = if direction == t.direction { t.run + 1 } else { 1 };
        t.direction = direction;
        true
    }

    fn finish_growth(&mut self, t: &mut Trace) {
        let min = if t.leaves_eye() { MIN_EYE_CELLS } else { MIN_BOARD_CELLS };
        let time = self.frame.time;
        // A stub that could not get anywhere is not worth keeping.
        if t.cells.len() < min {
            begin_fade(t, time, 0.6);
            return;
        }
        t.state = State::Live;
        t.live_until = time + self.rng.range(TRACE_LIFE.0, TRACE_LIFE.1);
        t.next_pulse = time + 0.3 + self.rng.f64() * 1.2;
        t.reverse = !t.leaves_eye() && self.rng.below(2) == 0;
        // The end pad flashes as it is "soldered".
        t.end_glow = 1.0;
    }

    fn advance_pulse(&mut self, t: &mut Trace, distance: f32) {
        let time = self.frame.time;
        if t.pulse < 0.0 {
            if time < t.next_pulse {
                return;
            }
            t.pulse = 0.0;
            // A signal leaving the eye: it lights up as it sends.
            if let (Some(p), false, Some(e)) = (t.eye_pad, t.reverse, self.eye.as_mut()) {
                e.flash = 1.0;
                e.pad_glow[p] = 1.0;
            }
            return;
        }
        t.pulse += distance;
        if t.pulse < t.length() {
            return;
        }
        if t.reverse {
            t.start_glow = 1.0;
            // A signal arriving at the eye: it lights up as it receives.
            if let (Some(p), Some(e)) = (t.eye_pad, self.eye.as_mut()) {
                e.flash = 1.0;
                e.pad_glow[p] = 1.0;
            }
        } else {
            t.end_glow = 1.0;
        }
        t.pulse = -1.0;
        t.next_pulse = time + (1.2 + self.rng.f64() * 3.5) / self.speed_factor();
        t.reverse = self.rng.below(if t.leaves_eye() { 3 } else { 2 }) == 0;
    }

    fn spawn_board_trace(&mut self) {
        for _ in 0..25 {
            let start = (
                1 + self.rng.below((self.columns - 2).max(1) as usize) as i32,
                1 + self.rng.below((self.rows - 2).max(1) as usize) as i32,
            );
            if !self.clear_area(start) || !self.frame.is_visible(self.cell_centre(start), -self.cell) {
                continue;
            }
            let mut t = Trace::new(self.next_id);
            self.next_id += 1;
            t.width = (BOARD_TRACE_WIDTH * self.frame.unit).max(1.5);
            t.start_pad = true;
            t.start_ring = self.rng.below(2) == 0;
            t.end_ring = self.rng.below(2) == 0;
            t.target_cells = 6 + self.rng.below(22);
            t.direction = if self.rng.f64() < 0.7 {
                self.rng.below(4) * 2
            } else {
                self.rng.below(4) * 2 + 1
            };
            t.grow_after = self.frame.time;
            self.occupy(start, t.id);
            t.cells.push(start);
            t.add_point(self.cell_centre(start));
            if !self.can_enter(start, t.direction) {
                self.release(&t);
                continue;
            }
            self.traces.push(t);
            return;
        }
    }

    fn board_traces(&self) -> usize {
        self.traces
            .iter()
            .filter(|t| !t.leaves_eye() && t.state != State::Fading)
            .count()
    }

    fn target_board_traces(&self) -> usize {
        let area = f64::from(self.frame.width) * f64::from(self.frame.height) / (1920.0 * 1080.0);
        ((BOARD_TRACES_PER_1080P * area * f64::from(self.settings.density) / DEFAULT_DENSITY).round() as usize).max(2)
    }

    // ------------------------------------------------------------ the eye

    fn update_eye(&mut self, seconds: f64) {
        let time = self.frame.time;
        let Some(mut e) = self.eye.take() else {
            if time >= self.next_eye_at {
                self.open_eye();
            }
            return;
        };
        if !e.traces_created && time >= e.open_at + EYE_OPEN_SECONDS + 0.2 {
            self.eye = Some(e);
            self.create_eye_traces();
            let Some(back) = self.eye.take() else { return };
            e = back;
        }
        if !e.closing && time >= e.close_at {
            e.closing = true;
            for t in self.traces.iter_mut().filter(|t| e.traces.contains(&t.id)) {
                begin_fade(t, time, EYE_CLOSE_SECONDS + 1.5);
            }
        }
        if time >= e.close_at + EYE_CLOSE_SECONDS {
            self.next_eye_at = time + EYE_GAP_SECONDS;
            return;
        }
        if !e.closing && time >= e.next_blink {
            e.blink_start = time;
            e.next_blink = time + 3.5 + self.rng.f64() * 5.0;
        }
        let decay = seconds as f32 * 2.0;
        e.flash = (e.flash - decay).max(0.0);
        for g in &mut e.pad_glow {
            *g = (*g - decay * 0.6).max(0.0);
        }
        let target = self.look_target(&e);
        e.pupil_x += (target - e.pupil_x) * (seconds * 3.0).min(1.0) as f32;
        self.eye = Some(e);
    }

    /// Opens the eye with the wordmark beside it, where few traces are in the way;
    /// those fade out quickly.
    fn open_eye(&mut self) {
        let (w, h) = (self.frame.width, self.frame.height);
        let scale = EYE_SCALE * self.frame.unit;
        let radius = EYE_EXTENT * scale;
        let gap = radius * BRAND_GAP_FACTOR;
        let margin = self.cell;
        let brand = self.brand_size.filter(|b| b.0 > 0.0);
        let half = brand.map_or(radius, |b| radius.max(b.1 / 2.0));
        let has_brand =
            brand.is_some_and(|b| 2.0 * radius + gap + b.0 + 2.0 * margin <= w && 2.0 * half + 2.0 * margin <= h);
        let (bw, bh) = brand.unwrap_or_default();
        let mut best = (Point { x: w / 2.0, y: h / 2.0 }, None, usize::MAX);
        for _ in 0..16 {
            let (centre, area) = if has_brand {
                let left = self.rng.below(2) == 0;
                let (lo, hi) = if left {
                    (margin + bw + gap + radius, w - margin - radius)
                } else {
                    (margin + radius, w - margin - gap - bw - radius)
                };
                let c = Point {
                    x: self.between(lo, hi),
                    y: self.between(margin + half, h - margin - half),
                };
                let x = if left {
                    c.x - radius - gap - bw
                } else {
                    c.x + radius + gap
                };
                (
                    c,
                    Some(Area {
                        x,
                        y: c.y - bh / 2.0,
                        w: bw,
                        h: bh,
                    }),
                )
            } else {
                let mx = (w / 2.0).min(radius + margin);
                let my = (h / 2.0).min(radius + margin);
                (
                    Point {
                        x: self.between(mx, w - mx),
                        y: self.between(my, h - my),
                    },
                    None,
                )
            };
            let score = self
                .traces
                .iter()
                .filter(|t| self.touches_lockup(t, centre, radius, area))
                .count();
            if score < best.2 {
                best = (centre, area, score);
            }
        }
        let (centre, area, _) = best;
        let time = self.frame.time;
        let cell = self.cell;
        for t in &mut self.traces {
            if touches(t, centre, radius, area, cell) {
                begin_fade(t, time, 0.8);
            }
        }
        let rotation = (self.rng.f64() * 24.0 - 12.0) as f32;
        let close_at = time + self.rng.range(EYE_LIFE.0, EYE_LIFE.1);
        let next_blink = time + 2.5 + self.rng.f64() * 3.0;
        self.eye = Some(Eye {
            centre,
            rotation,
            scale,
            open_at: time,
            close_at,
            closing: false,
            traces_created: false,
            next_blink,
            blink_start: f64::MIN,
            pupil_x: PUPIL_REST_X,
            flash: 0.0,
            pad_glow: [0.0; 4],
            traces: Vec::new(),
            brand: area,
        });
    }

    fn touches_lockup(&self, t: &Trace, centre: Point, radius: f32, brand: Option<Area>) -> bool {
        touches(t, centre, radius, brand, self.cell)
    }

    /// One trace out of each of the eye's pads, carrying the circuit onto the board.
    fn create_eye_traces(&mut self) {
        let Some(e) = self.eye.clone() else { return };
        if let Some(eye) = self.eye.as_mut() {
            eye.traces_created = true;
        }
        for (p, pad) in EYE_PADS.iter().enumerate() {
            let centre = e.to_world(pad.centre);
            let preferred = (pad.direction as i32 + (e.rotation / 45.0).round() as i32).rem_euclid(8) + 8;
            let mut found = None;
            // Leave in the pad's own direction; bend up to 90° when something is there.
            for turn in [0, -1, 1, -2, 2] {
                let candidate = ((preferred + turn) % 8) as usize;
                let (sx, sy) = DIRECTIONS[candidate];
                let step = if sx != 0 && sy != 0 {
                    std::f32::consts::FRAC_1_SQRT_2
                } else {
                    1.0
                };
                for k in 1..=5 {
                    let probe = Point {
                        x: centre.x + sx as f32 * step * self.cell * k as f32,
                        y: centre.y + sy as f32 * step * self.cell * k as f32,
                    };
                    let cell = self.nearest_cell(probe);
                    if !self.blocked(cell.0, cell.1)
                        && self.no_neighbours(cell)
                        && !self.crosses_brand(&e, centre, self.cell_centre(cell))
                    {
                        found = Some((candidate, cell));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            let Some((direction, start)) = found else { continue };
            let mut t = Trace::new(self.next_id);
            self.next_id += 1;
            t.eye_pad = Some(p);
            t.width = (EYE_TRACE_WIDTH * self.frame.unit).max(2.0);
            t.end_ring = self.rng.below(2) == 0;
            t.target_cells = 10 + self.rng.below(16);
            t.direction = direction;
            t.grow_after = self.frame.time;
            t.add_point(centre);
            self.occupy(start, t.id);
            t.cells.push(start);
            t.add_point(self.cell_centre(start));
            if let Some(eye) = self.eye.as_mut() {
                eye.traces.push(t.id);
            }
            self.traces.push(t);
        }
    }

    /// The short lead from an eye pad to its first cell would cut through the brand.
    fn crosses_brand(&self, e: &Eye, from: Point, to: Point) -> bool {
        const SAMPLES: usize = 12;
        let Some(b) = e.brand else { return false };
        let area = b.inflate(self.cell * 0.6);
        (0..=SAMPLES).any(|i| area.contains(from.lerp(to, i as f32 / SAMPLES as f32)))
    }

    /// Where the pupil slides along the lid: towards the nearest signal in flight,
    /// otherwise a slow idle drift.
    fn look_target(&self, e: &Eye) -> f32 {
        let mut nearest = f32::MAX;
        let mut target = None;
        for t in self.traces.iter().filter(|t| t.pulse >= 0.0) {
            let head = t.point_at(if t.reverse { t.length() - t.pulse } else { t.pulse });
            let d = head.distance(e.centre) * if t.leaves_eye() { 0.5 } else { 1.0 };
            if d < nearest {
                nearest = d;
                target = Some(head);
            }
        }
        let Some(target) = target else {
            return (f64::from(PUPIL_REST_X) * 0.4
                + (self.frame.time * 0.4 + self.phase).sin() * f64::from(PUPIL_TRAVEL) * 0.6)
                as f32;
        };
        // The offset along the lid.
        let a = (e.rotation + BAR_ANGLE).to_radians();
        let along = (target.x - e.centre.x) * a.cos() + (target.y - e.centre.y) * a.sin();
        PUPIL_TRAVEL * (along / (e.radius() * 1.2)).clamp(-1.0, 1.0)
    }

    fn presence(&self, e: &Eye) -> f32 {
        let t = self.frame.time;
        let opening = (t - e.open_at) / EYE_OPEN_SECONDS;
        let closing = 1.0 - (t - e.close_at) / EYE_CLOSE_SECONDS;
        opening.min(closing).clamp(0.0, 1.0) as f32
    }

    /// 0 wide open, 1 shut: opening, closing and blinks together.
    fn lid(&self, e: &Eye) -> f32 {
        let t = self.frame.time;
        let opening = 1.0 - (t - e.open_at - 0.2) / (EYE_OPEN_SECONDS - 0.2);
        let closing = (t - e.close_at) / (EYE_CLOSE_SECONDS - 0.2);
        let blink_phase = (t - e.blink_start) / BLINK_SECONDS;
        let blink = if (0.0..=1.0).contains(&blink_phase) {
            1.0 - (2.0 * blink_phase - 1.0).abs()
        } else {
            0.0
        };
        opening.max(closing).max(blink).clamp(0.0, 1.0) as f32
    }

    // ------------------------------------------------------------ drawing

    fn draw_traces(&self, c: &mut dyn Canvas) {
        let fade = self.frame.fade();
        let bright = self.settings.brightness();
        let grey = Rgba::hex(GREY);
        for t in &self.traces {
            let alpha = trace_alpha(t, self.frame.time) * fade;
            if alpha <= 0.0 {
                continue;
            }
            let level = if t.leaves_eye() { EYE_TRACE_LEVEL } else { TRACE_LEVEL };
            let colour = grey.dim(bright * level, alpha);
            let points = t.visible_points();
            if points.len() >= 2 {
                c.polyline(&points, t.width, colour);
            }
            if t.start_pad {
                self.draw_pad(c, t.points[0], t.start_ring, t.width, colour, t.start_glow * alpha);
            }
            if t.state == State::Growing {
                // The router's bit: a small yellow tip where the trace is being laid.
                if let (true, Some(tip)) = (points.len() >= 2, points.last()) {
                    c.fill_circle(*tip, t.width * 0.6, Rgba::hex(YELLOW).dim(self.accent(), 0.9 * alpha));
                }
            } else {
                self.draw_pad(
                    c,
                    t.points[t.points.len() - 1],
                    t.end_ring,
                    t.width,
                    colour,
                    t.end_glow * alpha,
                );
            }
        }
    }

    /// A filled via or a hollow ring; glows yellow with an orange ripple as a signal lands.
    fn draw_pad(&self, c: &mut dyn Canvas, centre: Point, ring: bool, width: f32, colour: Rgba, glow: f32) {
        let outer = if ring {
            let stroke = width * 0.65;
            let radius = width * 1.1;
            c.fill_circle(centre, radius, Rgba::BLACK);
            c.circle(centre, radius, stroke, colour);
            radius + stroke / 2.0
        } else {
            let outer = width * 1.5;
            c.fill_circle(centre, outer, colour);
            outer
        };
        if glow <= 0.0 {
            return;
        }
        let fade = self.frame.fade();
        c.fill_circle(centre, outer * 0.85, Rgba::hex(YELLOW).dim(self.accent(), glow * fade));
        let ripple = outer * (1.2 + (1.0 - glow) * 2.2);
        c.circle(
            centre,
            ripple,
            (width * 0.35).max(1.0),
            Rgba::hex(ORANGE).dim(self.accent(), glow * 0.7 * fade),
        );
    }

    fn draw_pulses(&self, c: &mut dyn Canvas) {
        let fade = self.frame.fade();
        let accent = self.accent();
        for t in self.traces.iter().filter(|t| t.pulse >= 0.0) {
            let length = t.length();
            let head = if t.reverse { length - t.pulse } else { t.pulse };
            let trail = PULSE_TRAIL * self.frame.unit;
            let tail = if t.reverse {
                length.min(head + trail)
            } else {
                (head - trail).max(0.0)
            };
            let width = t.width * 0.7;
            let mut previous = t.point_at(tail);
            for s in 1..=TRAIL_SAMPLES {
                let share = s as f32 / TRAIL_SAMPLES as f32;
                let current = t.point_at(tail + (head - tail) * share);
                c.line(
                    previous,
                    current,
                    width,
                    Rgba::hex(ORANGE).dim(accent, share * 0.8 * fade),
                );
                previous = current;
            }
            let at = t.point_at(head);
            c.fill_circle(at, t.width * 1.6, Rgba::hex(YELLOW).dim(accent, 0.2 * fade));
            c.fill_circle(at, t.width * 0.75, Rgba::hex(YELLOW).dim(accent, fade));
        }
    }

    /// The eye as light on black: a half-disc hanging from its lid bar, a sliding pupil
    /// and four legs ending in two vias and two ring pads.
    fn draw_eye(&self, c: &mut dyn Canvas) {
        let Some(e) = &self.eye else { return };
        let presence = self.presence(e) * self.frame.fade();
        if presence <= 0.0 {
            return;
        }
        let lid = self.lid(e);
        let colour = Rgba::hex(GREY).dim(self.settings.brightness() * EYE_LEVEL, presence);
        let yellow = Rgba::hex(YELLOW);
        c.push(Transform {
            translate: e.centre,
            rotate: e.rotation,
            scale: (e.scale, e.scale),
        });
        for leg in LEGS {
            c.polyline(leg, LOGO_STROKE, colour);
        }
        // The eye and pupil live in the lid's frame; a blink squashes them into the bar.
        if lid < 0.98 {
            c.push(Transform {
                translate: Point::default(),
                rotate: BAR_ANGLE,
                scale: (1.0, 1.0 - lid),
            });
            c.fill_half_disc(Point::default(), BODY_RADIUS, colour);
            let pupil = Point { x: e.pupil_x, y: 0.0 };
            c.fill_half_disc(pupil, PUPIL_RADIUS, Rgba::BLACK);
            if e.flash > 0.0 {
                c.fill_half_disc(pupil, PUPIL_RADIUS, yellow.dim(self.accent(), e.flash * presence));
            }
            c.pop();
        }
        c.line(BAR.0, BAR.1, LOGO_STROKE, colour);
        for (p, pad) in EYE_PADS.iter().enumerate() {
            let glow = e.pad_glow[p] * presence;
            if pad.ring {
                let r = pad.radius - RING_STROKE / 2.0;
                c.fill_circle(pad.centre, r, Rgba::BLACK);
                c.circle(pad.centre, r, RING_STROKE, colour);
                if glow > 0.0 {
                    c.circle(pad.centre, r, RING_STROKE, yellow.dim(self.accent(), glow));
                }
            } else {
                c.fill_circle(pad.centre, pad.radius, colour);
                if glow > 0.0 {
                    c.fill_circle(pad.centre, pad.radius, yellow.dim(self.accent(), glow));
                }
            }
        }
        c.pop();
    }

    /// The wordmark and clock beside the eye, on a black plate; they come and go with it.
    fn draw_brand(&self, c: &mut dyn Canvas, clock: &Clock) {
        let Some(e) = &self.eye else { return };
        let Some(b) = e.brand else { return };
        let presence = self.presence(e) * self.frame.fade();
        if presence <= 0.0 {
            return;
        }
        let bright = self.settings.brightness();
        let plate = self.cell * 0.6;
        c.fill_rect(
            Point {
                x: b.x - plate,
                y: b.y - plate,
            },
            (b.w + plate * 2.0, b.h + plate * 2.0),
            Rgba::BLACK,
        );
        let em = (self.frame.height * WORDMARK_SIZE).max(MIN_WORDMARK);
        let mark = c.wordmark_size(em);
        c.wordmark(
            Point { x: b.x, y: b.y },
            em,
            Rgba::hex(GREY).dim(bright * WORDMARK_LEVEL, presence),
            Rgba::hex(YELLOW).dim(self.accent(), presence),
        );
        if self.settings.show_clock {
            let style = self.clock_style();
            c.text(
                &clock_text(clock),
                Point {
                    x: b.x,
                    y: b.y + mark.1 + style.size * CLOCK_GAP,
                },
                style,
                Rgba::hex(GREY).dim(bright * CLOCK_LEVEL, presence),
            );
        }
    }
}

/// The clock line beside the wordmark: "20:31  ·  12 Ekim Pazartesi".
fn clock_text(c: &Clock) -> String {
    format!("{}  ·  {}", c.time, c.date)
}

fn begin_fade(t: &mut Trace, time: f64, seconds: f64) {
    if t.state == State::Fading {
        return;
    }
    t.state = State::Fading;
    t.fade_start = time;
    t.fade_end = time + seconds;
    t.pulse = -1.0;
}

fn trace_alpha(t: &Trace, time: f64) -> f32 {
    if t.state == State::Fading {
        ((t.fade_end - time) / (t.fade_end - t.fade_start).max(0.001)).max(0.0) as f32
    } else {
        1.0
    }
}

fn touches(t: &Trace, centre: Point, radius: f32, brand: Option<Area>, cell: f32) -> bool {
    if t.points.iter().any(|p| p.distance(centre) < radius + cell) {
        return true;
    }
    brand.is_some_and(|b| {
        let area = b.inflate(cell);
        t.points.iter().any(|p| area.contains(*p))
    })
}

impl Scene for Milva {
    fn resize(&mut self, width: u32, height: u32) {
        if !self.frame.resize(width, height) {
            return;
        }
        let f = self.frame;
        self.cell = (CELL_SIZE * f.unit).max(MIN_CELL);
        self.columns = (f.width / self.cell) as i32 + 3;
        self.rows = (f.height / self.cell) as i32 + 3;
        self.owners = vec![0; (self.columns * self.rows) as usize];
        // A random sub-cell offset, so the lattice never sits on the same pixels twice.
        self.origin = Point {
            x: -self.cell * self.rng.f64() as f32,
            y: -self.cell * self.rng.f64() as f32,
        };
        self.traces.clear();
        self.eye = None;
        self.next_eye_at = f.time + 0.6;
        self.next_spawn_at = f.time + 1.2;
        self.brand_size = None;
    }

    fn update(&mut self, seconds: f64) {
        self.frame.time += seconds;
        if self.owners.is_empty() {
            return;
        }
        self.update_traces(seconds);
        self.update_eye(seconds);
        if self.frame.time >= self.next_spawn_at {
            if self.board_traces() < self.target_board_traces() {
                self.spawn_board_trace();
            }
            self.next_spawn_at = self.frame.time + (0.15 + self.rng.f64() * 0.45) / self.speed_factor();
        }
    }

    fn measure(&mut self, canvas: &mut dyn Canvas, clock: &Clock) {
        self.measure_brand(canvas, clock);
    }

    fn draw(&self, canvas: &mut dyn Canvas, clock: &Clock) {
        self.draw_traces(canvas);
        self.draw_pulses(canvas);
        self.draw_brand(canvas, clock);
        self.draw_eye(canvas);
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: traces grow, never share or touch cells, stay on the board, and fade
    //! away; the eye opens with its brand inside the screen, sends traces out of its pads,
    //! blinks, closes and opens again elsewhere; colours keep under the brightness;
    //! transforms are balanced; a tiny preview still runs.
    use super::*;
    use crate::canvas::Recorder;

    fn clock() -> Clock {
        Clock {
            time: "20:31".into(),
            date: "12 Ekim Pazartesi".into(),
        }
    }

    fn run(s: &mut Milva, seconds: f64) {
        let mut r = Recorder::default();
        s.measure_brand(&mut r, &clock());
        let steps = (seconds * 30.0) as usize;
        for _ in 0..steps {
            s.update(1.0 / 30.0);
        }
    }

    fn scene() -> Milva {
        let mut s = Milva::new(Settings::default(), 0, 42);
        s.resize(1920, 1080);
        s
    }

    /// No two traces own the same cell, and every trace's cells are its own in the grid.
    fn cells_are_owned_once(s: &Milva) {
        let mut seen = std::collections::HashMap::new();
        for t in &s.traces {
            for &c in &t.cells {
                if let Some(other) = seen.insert(c, t.id) {
                    assert_eq!(other, t.id, "cell {c:?} shared by {other} and {}", t.id);
                }
                if s.in_grid(c.0, c.1) {
                    assert_eq!(s.owner(c.0, c.1), t.id);
                }
            }
        }
    }

    #[test]
    fn traces_grow_without_sharing_cells_and_fade_away() {
        let mut s = scene();
        run(&mut s, 20.0);
        assert!(s.board_traces() >= 2, "{}", s.board_traces());
        assert!(s.traces.iter().any(|t| t.state == State::Live));
        cells_are_owned_once(&s);
        let first: Vec<i32> = s.traces.iter().map(|t| t.id).collect();
        run(&mut s, 120.0);
        cells_are_owned_once(&s);
        // Every trace from the start has lived its life and gone.
        assert!(s.traces.iter().all(|t| !first.contains(&t.id)));
        // Freed cells are free again: the grid owns only living traces' cells.
        let owned = s.owners.iter().filter(|o| **o > 0).count();
        let cells: usize = s
            .traces
            .iter()
            .map(|t| t.cells.iter().filter(|c| s.in_grid(c.0, c.1)).count())
            .sum();
        assert_eq!(owned, cells);
    }

    #[test]
    fn the_eye_opens_with_its_brand_on_screen_and_sends_traces() {
        let mut s = scene();
        run(&mut s, 3.0);
        let e = s.eye.clone().expect("the eye opened");
        let b = e.brand.expect("room for the wordmark");
        assert!(
            b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= 1920.0 && b.y + b.h <= 1080.0,
            "{b:?}"
        );
        assert!(
            e.centre.distance(Point {
                x: b.x + b.w / 2.0,
                y: b.y + b.h / 2.0
            }) > e.radius()
        );
        assert!(e.traces_created);
        assert!(!e.traces.is_empty(), "no trace left the pads");
        assert!(s
            .traces
            .iter()
            .filter(|t| t.leaves_eye())
            .all(|t| e.traces.contains(&t.id)));
    }

    #[test]
    fn the_eye_blinks_closes_and_opens_somewhere_else() {
        let mut s = scene();
        run(&mut s, 3.0);
        let first = s.eye.clone().unwrap();
        let mut blinked = false;
        let mut closed = false;
        for _ in 0..(80 * 30) {
            s.update(1.0 / 30.0);
            match &s.eye {
                Some(e) if e.open_at == first.open_at => blinked |= e.blink_start > f64::MIN,
                Some(_) => break,
                None => closed = true,
            }
        }
        assert!(blinked && closed);
        let second = s.eye.clone().expect("opened again");
        assert_ne!(second.centre, first.centre);
    }

    #[test]
    fn drawing_keeps_under_the_brightness_and_balances_transforms() {
        let dim = Settings {
            brightness: 20,
            ..Settings::default()
        };
        let mut s = Milva::new(dim, 1, 7);
        s.resize(1920, 1080);
        run(&mut s, 8.0);
        let mut r = Recorder::default();
        s.draw(&mut r, &clock());
        assert_eq!(r.depth, 0, "a transform was left on");
        assert!(r.deepest >= 1 && !r.polylines.is_empty() && !r.wordmarks.is_empty());
        let cap = 0.2 * 1.4 + 1e-4;
        for (_, _, c) in &r.polylines {
            assert!(c.r.max(c.g).max(c.b) <= cap);
        }
        for (_, _, c) in &r.dots {
            assert!(c.r.max(c.g).max(c.b) <= cap);
        }
        assert_eq!(r.texts.len(), 1);
        assert!(r.texts[0].0.contains("20:31") && r.texts[0].0.contains("Pazartesi"));
    }

    #[test]
    fn a_tiny_preview_still_runs() {
        let mut s = Milva::new(Settings::default(), 0, 3);
        s.resize(152, 112);
        run(&mut s, 30.0);
        let mut r = Recorder::default();
        s.draw(&mut r, &clock());
        assert_eq!(r.depth, 0);
    }
}
