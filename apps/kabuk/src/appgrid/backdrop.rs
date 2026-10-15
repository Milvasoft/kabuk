//! The app grid's background, pure: the wallpaper, read small, blurred and darkened, then
//! stretched over the monitor (a small blurred picture stretched smoothly looks like a
//! large one blurred, at a fraction of the work).

/// `bgr` (`w` × `h`, top-down rows of blue, green, red) blurred with a box `radius`
/// pixels each side `passes` times and darkened to `dim` of its light, as opaque
/// premultiplied BGRA. Pixels past the edges repeat the edge's.
pub fn blurred(bgr: &[u8], w: usize, h: usize, radius: usize, passes: usize, dim: f32) -> Vec<u8> {
    if w == 0 || h == 0 || bgr.len() < w * h * 3 {
        return Vec::new();
    }
    let mut planes: [Vec<f32>; 3] = [0, 1, 2].map(|c| (0..w * h).map(|i| f32::from(bgr[i * 3 + c])).collect());
    for plane in &mut planes {
        for _ in 0..passes {
            box_pass(plane, w, h, radius, true);
            box_pass(plane, w, h, radius, false);
        }
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        for plane in &planes {
            out.push((plane[i] * dim).round().clamp(0.0, 255.0) as u8);
        }
        out.push(u8::MAX);
    }
    out
}

/// One box blur along the rows (`across`) or the columns.
fn box_pass(plane: &mut [f32], w: usize, h: usize, radius: usize, across: bool) {
    let (lines, length) = if across { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| if across { line * w + i } else { i * w + line };
    let span = (2 * radius + 1) as f32;
    let mut line_values = vec![0.0f32; length];
    for line in 0..lines {
        for (i, v) in line_values.iter_mut().enumerate() {
            *v = plane[at(line, i)];
        }
        let value = |i: isize| line_values[i.clamp(0, length as isize - 1) as usize];
        let r = radius as isize;
        let mut sum: f32 = (-r..=r).map(value).sum();
        for i in 0..length {
            plane[at(line, i)] = sum / span;
            let i = i as isize;
            sum += value(i + r + 1) - value(i - r);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a plain picture stays plain (only darker); a bright point spreads
    //! evenly and keeps its light; the edges repeat; opaque output of the right size;
    //! nothing for no picture or too few bytes.
    use super::*;

    #[test]
    fn a_plain_picture_only_darkens() {
        let px = [10u8, 100, 200].repeat(6 * 4);
        let out = blurred(&px, 6, 4, 2, 3, 0.5);
        assert_eq!(out.len(), 6 * 4 * 4);
        for p in out.chunks(4) {
            assert_eq!(p, [5, 50, 100, 255]);
        }
    }

    #[test]
    fn a_bright_point_spreads_and_keeps_its_light() {
        let (w, h) = (9, 9);
        let mut px = vec![0u8; w * h * 3];
        let centre = (4 * w + 4) * 3;
        px[centre..centre + 3].copy_from_slice(&[255, 255, 255]);
        let out = blurred(&px, w, h, 1, 1, 1.0);
        let light = |x: usize, y: usize| out[(y * w + x) * 4];
        assert!(light(4, 4) > 0 && light(4, 4) < 255);
        assert_eq!(light(3, 4), light(5, 4), "evenly to both sides");
        assert_eq!(light(4, 3), light(3, 4), "and up and down alike");
        assert_eq!(light(0, 0), 0, "not as far as the corner");
        let total: u32 = out.chunks(4).map(|p| u32::from(p[0])).sum();
        assert!((250..=260).contains(&total), "{total}");
    }

    #[test]
    fn edges_repeat_and_nothing_comes_of_nothing() {
        // A bright left column: the edge stays as bright as the column next to it.
        let (w, h) = (4, 1);
        let mut px = vec![0u8; w * h * 3];
        px[..3].copy_from_slice(&[90, 90, 90]);
        let out = blurred(&px, w, h, 1, 1, 1.0);
        assert_eq!(out[0], 60, "two of three repeat the edge");
        assert_eq!(out[4], 30);
        assert!(blurred(&[], 0, 0, 1, 1, 1.0).is_empty());
        assert!(blurred(&[1, 2], 1, 1, 1, 1, 1.0).is_empty());
    }
}
