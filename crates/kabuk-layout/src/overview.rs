//! The overview (DECISIONS D34): every workspace of a monitor in a grid over its work
//! area, each cell showing that workspace's windows scaled down in their places. Pure.

use crate::Rect;

/// The grid's columns and rows for `n` workspaces: the largest cells (the fewest along
/// the longer side), then the fewest empty ones, then more columns on a wide area and more
/// rows on a tall one (9 → 3 × 3, 4 → 2 × 2, 2 → 2 × 1 wide or 1 × 2 tall).
pub fn grid(n: usize, area: Rect) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let wide = area.w >= area.h;
    (1..=n)
        .map(|cols| (cols, n.div_ceil(cols)))
        .min_by_key(|&(c, r)| {
            let along = if wide { r > c } else { c > r };
            (c.max(r), c * r - n, along)
        })
        .unwrap_or((n, 1))
}

/// The cells of `n` workspaces in `area`, `gap` apart and from its edge, row by row; each
/// cell keeps the area's shape and the grid is centred.
pub fn cells(n: usize, area: Rect, gap: i32) -> Vec<Rect> {
    let (cols, rows) = grid(n, area);
    if cols == 0 {
        return Vec::new();
    }
    let (c, r) = (cols as i32, rows as i32);
    let free_w = area.w - gap * (c + 1);
    let free_h = area.h - gap * (r + 1);
    // The largest cell of the area's shape that fits both ways.
    let scale = (f64::from(free_w) / f64::from(c) / f64::from(area.w.max(1)))
        .min(f64::from(free_h) / f64::from(r) / f64::from(area.h.max(1)))
        .max(0.0);
    let w = (f64::from(area.w) * scale).floor() as i32;
    let h = (f64::from(area.h) * scale).floor() as i32;
    let x0 = area.x + (area.w - (c * w + (c - 1) * gap)) / 2;
    let y0 = area.y + (area.h - (r * h + (r - 1) * gap)) / 2;
    (0..n)
        .map(|i| {
            let (col, row) = ((i % cols) as i32, (i / cols) as i32);
            Rect {
                x: x0 + col * (w + gap),
                y: y0 + row * (h + gap),
                w,
                h,
            }
        })
        .collect()
}

/// `window` (in `area`) scaled into `cell`, which has the area's shape; a window reaching
/// past the area (a maximised one's frame) is cut to the cell, so it never covers what is
/// around the cell (the shown cell's frame).
pub fn place_in(window: Rect, area: Rect, cell: Rect) -> Rect {
    let sx = f64::from(cell.w) / f64::from(area.w.max(1));
    let sy = f64::from(cell.h) / f64::from(area.h.max(1));
    let x = cell.x + (f64::from(window.x - area.x) * sx).round() as i32;
    let y = cell.y + (f64::from(window.y - area.y) * sy).round() as i32;
    let right = x + (f64::from(window.w) * sx).round() as i32;
    let bottom = y + (f64::from(window.h) * sy).round() as i32;
    let (x, y) = (x.max(cell.x), y.max(cell.y));
    Rect {
        x,
        y,
        w: (right.min(cell.right()) - x).max(0),
        h: (bottom.min(cell.bottom()) - y).max(0),
    }
}

/// The cell under a point, if any.
pub fn cell_at(cells: &[Rect], x: i32, y: i32) -> Option<usize> {
    cells
        .iter()
        .position(|c| x >= c.x && x < c.right() && y >= c.y && y < c.bottom())
}

/// Where a window flying from `from` (its place) to `to` (its place in a cell) is when
/// `u` (0-1, eased by the caller) of the way is done.
pub fn flying(from: Rect, to: Rect, u: f32) -> Rect {
    let u = f64::from(u.clamp(0.0, 1.0));
    let at = |a: i32, b: i32| a + (f64::from(b - a) * u).round() as i32;
    Rect {
        x: at(from.x, to.x),
        y: at(from.y, to.y),
        w: at(from.w, to.w),
        h: at(from.h, to.h),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no workspaces, one, the common counts on wide and tall areas; cells
    //! never overlap, stay inside the area and keep its shape; a window scaled into a
    //! cell; a point between cells; a window flying into its cell, at the start, half way,
    //! at the end and past either end.
    use super::*;

    #[test]
    fn a_window_flies_from_its_place_into_its_cell() {
        let from = Rect {
            x: -100,
            y: 0,
            w: 1000,
            h: 800,
        };
        let to = Rect {
            x: 100,
            y: 50,
            w: 200,
            h: 160,
        };
        assert_eq!(flying(from, to, 0.0), from);
        assert_eq!(flying(from, to, 1.0), to);
        assert_eq!(
            flying(from, to, 0.5),
            Rect {
                x: 0,
                y: 25,
                w: 600,
                h: 480
            }
        );
        assert_eq!(flying(from, to, -1.0), from);
        assert_eq!(flying(from, to, 2.0), to);
    }

    const WIDE: Rect = Rect {
        x: 0,
        y: 65,
        w: 2560,
        h: 1315,
    };
    const TALL: Rect = Rect {
        x: -1080,
        y: 0,
        w: 1080,
        h: 1920,
    };

    /// Found by the user: a maximised window's frame reaches a few pixels past the work
    /// area, and its thumbnail, scaled with it, covered the top of the shown cell's frame.
    #[test]
    fn a_window_past_the_area_stays_inside_its_cell() {
        let cell = Rect {
            x: 100,
            y: 100,
            w: 1000,
            h: 514,
        };
        let past = Rect {
            x: WIDE.x - 8,
            y: WIDE.y - 7,
            w: WIDE.w + 16,
            h: WIDE.h + 15,
        };
        let placed = place_in(past, WIDE, cell);
        assert!(cell.contains(&placed), "{placed:?} outside {cell:?}");
        assert_eq!(place_in(WIDE, WIDE, cell), cell);
        // A window inside keeps its share of the cell.
        let half = Rect { w: WIDE.w / 2, ..WIDE };
        assert_eq!(place_in(half, WIDE, cell), Rect { w: 500, ..cell });
    }

    #[test]
    fn grids_are_as_square_as_the_area() {
        assert_eq!(grid(0, WIDE), (0, 0));
        assert_eq!(grid(1, WIDE), (1, 1));
        assert_eq!(grid(2, WIDE), (2, 1));
        assert_eq!(grid(4, WIDE), (2, 2));
        assert_eq!(grid(9, WIDE), (3, 3));
        assert_eq!(grid(2, TALL), (1, 2));
        assert_eq!(grid(6, WIDE), (3, 2));
    }

    #[test]
    fn cells_stay_inside_apart_and_keep_the_shape() {
        for area in [WIDE, TALL] {
            for n in 1..=9 {
                let cs = cells(n, area, 24);
                assert_eq!(cs.len(), n);
                for (i, a) in cs.iter().enumerate() {
                    assert!(area.contains(a), "{n}: {a:?} outside {area:?}");
                    let shape = f64::from(a.w) / f64::from(a.h);
                    let want = f64::from(area.w) / f64::from(area.h);
                    assert!((shape - want).abs() < 0.02, "{n}: shape {shape} vs {want}");
                    for b in &cs[i + 1..] {
                        assert!(!a.intersects(b), "{n}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
        assert!(cells(0, WIDE, 24).is_empty());
    }

    #[test]
    fn a_window_scales_into_its_cell_and_points_find_cells() {
        let cell = Rect {
            x: 100,
            y: 200,
            w: 640,
            h: 329,
        };
        let whole = place_in(WIDE, WIDE, cell);
        assert_eq!(whole, cell);
        let half = place_in(
            Rect {
                x: 1280,
                y: 65,
                w: 1280,
                h: 1315,
            },
            WIDE,
            cell,
        );
        assert_eq!((half.x, half.w, half.h), (420, 320, 329));
        let cs = cells(4, WIDE, 24);
        assert_eq!(cell_at(&cs, cs[3].x + 1, cs[3].y + 1), Some(3));
        assert_eq!(cell_at(&cs, cs[0].right() + 1, cs[0].y + 1), None, "in the gap");
    }
}
