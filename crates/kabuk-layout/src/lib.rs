//! Pure tiling layout engine: dwindle and master layouts with gaps (ARCHITECTURE §8).
//!
//! The input is a monitor's work area, the gaps and the workspace's windows in order;
//! the output is a target rectangle for every tiled window, in physical pixels. Floating
//! windows are passed through untouched. A window whose minimum size does not fit its
//! place is reported as unfit (the caller floats it) and the rest is laid out again.
//!
//! The layout is a function of the window order only, so adding a window and removing
//! it again gives the same layout back.

pub mod desk;
pub mod overview;

pub use desk::{spot_rect, Desk, Effect, MonitorInfo, NewWindow, Settings, Spot, TabStrip, Toward, WorkspaceInfo};

/// A window as the window manager knows it (e.g. its HWND).
pub type WindowId = u64;

/// A rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Shrunk by `d` on every side (may become empty or negative).
    pub fn inset(&self, d: i32) -> Rect {
        Rect {
            x: self.x + d,
            y: self.y + d,
            w: self.w - 2 * d,
            h: self.h - 2 * d,
        }
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

/// The tiling layout of a workspace (SPEC §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Each new window takes half of the previous one's space, turning each time.
    Dwindle,
    /// One main window and a stack of the others beside it.
    Master,
}

/// Where new windows go (`windows.new_window`): `Right` splits side by side first (master:
/// the stack to the right), `Below` top over bottom first (master: the stack below).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Right,
    Below,
}

/// Space between windows (`inner`) and between windows and the work area's edge (`outer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gaps {
    pub inner: i32,
    pub outer: i32,
}

/// The master window's share of the space: the design's `grid-template-columns: 1.35fr 1fr`.
pub const MASTER_RATIO: f32 = 1.35 / 2.35;
/// Dwindle halves.
const SPLIT_RATIO: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub area: Rect,
    pub gaps: Gaps,
    pub kind: Kind,
    pub direction: Direction,
    /// The master's share, 0 to 1.
    pub master_ratio: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub floating: bool,
    /// The smallest size the window accepts (`WM_GETMINMAXINFO`), if known.
    pub min: Option<Size>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Tiled windows in workspace order with their target rectangles.
    pub tiled: Vec<(WindowId, Rect)>,
    /// Windows that did not fit, in the order they were taken out (newest first).
    pub unfit: Vec<WindowId>,
}

/// Lays out the non-floating windows of `windows` in `p.area`.
pub fn arrange(p: &Params, windows: &[Window]) -> Outcome {
    arrange_split(p, windows, &[])
}

/// [`arrange`] with the shares the user set by dragging edges: `splits[i]` is split
/// `i`'s first part's share (dwindle: window `i` against the rest; master: the master
/// against the stack). Missing ones are the layout's own.
pub fn arrange_split(p: &Params, windows: &[Window], splits: &[f32]) -> Outcome {
    let mut tiling: Vec<&Window> = windows.iter().filter(|w| !w.floating).collect();
    let mut unfit = Vec::new();
    loop {
        let boxes = boxes(p, tiling.len(), splits);
        // The newest window that does not fit floats first; the others may fit then.
        let misfit = tiling.iter().zip(&boxes).rposition(|(w, b)| !fits(w, b));
        match misfit {
            Some(i) => unfit.push(tiling.remove(i).id),
            None => {
                return Outcome {
                    tiled: tiling.iter().map(|w| w.id).zip(boxes).collect(),
                    unfit,
                }
            }
        }
    }
}

fn fits(w: &Window, b: &Rect) -> bool {
    let min = w.min.unwrap_or(Size { w: 1, h: 1 });
    b.w >= min.w.max(1) && b.h >= min.h.max(1)
}

/// The shares a dragged split may take: no window disappears (DECISIONS D27).
pub const SPLIT_LIMITS: (f32, f32) = (0.1, 0.9);

/// Each split of `n` windows: the region it divides and whether it divides across.
fn splits_of(p: &Params, n: usize, splits: &[f32]) -> Vec<(Rect, bool)> {
    let usable = p.area.inset(p.gaps.outer);
    let side_by_side = p.direction == Direction::Right;
    match p.kind {
        _ if n < 2 => Vec::new(),
        Kind::Dwindle => {
            let mut out = Vec::with_capacity(n - 1);
            let mut rest = usable;
            let mut across = side_by_side;
            for i in 0..n - 1 {
                out.push((rest, across));
                let ratio = splits.get(i).copied().unwrap_or(SPLIT_RATIO);
                rest = split(rest, across, ratio, p.gaps.inner).1;
                across = !across;
            }
            out
        }
        Kind::Master => vec![(usable, side_by_side)],
    }
}

/// The shares after a window of the `n` tiled ones was resized from `old` (its place)
/// to `new` by one of its edges, or `None` when no moved edge is a split (it faces the
/// screen's edge).
pub fn resize_split(p: &Params, n: usize, splits: &[f32], old: Rect, new: Rect) -> Option<Vec<f32>> {
    let count = match p.kind {
        _ if n < 2 => 0,
        Kind::Dwindle => n - 1,
        Kind::Master => 1,
    };
    let mut out: Vec<f32> = (0..count)
        .map(|i| match p.kind {
            Kind::Dwindle => splits.get(i).copied().unwrap_or(SPLIT_RATIO),
            Kind::Master => splits.get(i).copied().unwrap_or(p.master_ratio),
        })
        .collect();
    let gap = p.gaps.inner;
    let mut changed = false;
    for (i, (region, across)) in splits_of(p, n, &out.clone()).into_iter().enumerate() {
        let (start, length) = if across {
            (region.x, region.w)
        } else {
            (region.y, region.h)
        };
        let avail = length - gap;
        if avail <= 0 {
            continue;
        }
        let (line_first_end, line_second_start) = {
            let (a, b) = split(region, across, out[i], gap);
            if across {
                (a.right(), b.x)
            } else {
                (a.bottom(), b.y)
            }
        };
        let (old_lo, old_hi, new_lo, new_hi) = if across {
            (old.x, old.right(), new.x, new.right())
        } else {
            (old.y, old.bottom(), new.y, new.bottom())
        };
        // The window ends at this split's line (it is the first part) or starts after it.
        let first = if old_hi == line_first_end && new_hi != old_hi {
            Some(new_hi - start)
        } else if old_lo == line_second_start && new_lo != old_lo {
            Some(new_lo - gap - start)
        } else {
            None
        };
        if let Some(first) = first {
            let share = (first as f32 / avail as f32).clamp(SPLIT_LIMITS.0, SPLIT_LIMITS.1);
            out[i] = share;
            changed = true;
        }
    }
    changed.then_some(out)
}

/// The rectangles for `n` windows.
fn boxes(p: &Params, n: usize, splits: &[f32]) -> Vec<Rect> {
    let usable = p.area.inset(p.gaps.outer);
    if n == 0 {
        return Vec::new();
    }
    let side_by_side = p.direction == Direction::Right;
    match p.kind {
        Kind::Dwindle => {
            let mut out = Vec::with_capacity(n);
            let mut rest = usable;
            let mut across = side_by_side;
            for i in 1..n {
                let ratio = splits.get(i - 1).copied().unwrap_or(SPLIT_RATIO);
                let (a, b) = split(rest, across, ratio, p.gaps.inner);
                out.push(a);
                rest = b;
                across = !across;
            }
            out.push(rest);
            out
        }
        Kind::Master => {
            if n == 1 {
                return vec![usable];
            }
            let ratio = splits.first().copied().unwrap_or(p.master_ratio);
            let (master, stack) = split(usable, side_by_side, ratio, p.gaps.inner);
            let mut out = vec![master];
            // The stack is divided the other way: a column beside the master, a row below it.
            out.extend(divide(stack, !side_by_side, n - 1, p.gaps.inner));
            out
        }
    }
}

/// Two parts of `r` with `gap` between: side by side when `across`, else top over bottom.
fn split(r: Rect, across: bool, ratio: f32, gap: i32) -> (Rect, Rect) {
    let length = if across { r.w } else { r.h };
    let avail = length - gap;
    let first = (avail as f32 * ratio).round() as i32;
    let second = avail - first;
    if across {
        (
            Rect { w: first, ..r },
            Rect {
                x: r.x + first + gap,
                w: second,
                ..r
            },
        )
    } else {
        (
            Rect { h: first, ..r },
            Rect {
                y: r.y + first + gap,
                h: second,
                ..r
            },
        )
    }
}

/// `n` equal parts of `r` with `gap` between; leftover pixels go to the first parts.
fn divide(r: Rect, across: bool, n: usize, gap: i32) -> Vec<Rect> {
    let count = i32::try_from(n).unwrap_or(i32::MAX);
    let length = if across { r.w } else { r.h };
    let avail = length - gap * (count - 1);
    let base = avail.div_euclid(count);
    let extra = avail.rem_euclid(count);
    let mut at = if across { r.x } else { r.y };
    (0..count)
        .map(|i| {
            let size = base + i32::from(i < extra);
            let part = if across {
                Rect { x: at, w: size, ..r }
            } else {
                Rect { y: at, h: size, ..r }
            };
            at += size + gap;
            part
        })
        .collect()
}

#[cfg(test)]
mod tests;
