//! The OLED checks of a wallpaper (SPEC §9), pure, on frames already drawn: small
//! grids of brightness (0-1) sampled over a minute of the wallpaper's time. A wallpaper
//! passes when its background is black, its average brightness is at most
//! [`MAX_AVERAGE`], and no part of it stays bright and still for the whole minute.

/// Average brightness over every frame and pixel, at most (SPEC §9: 8 %).
pub const MAX_AVERAGE: f32 = 0.08;
/// The background is black when at least this share of the pixels is darker than
/// [`BLACK`] (on average over the frames).
pub const BLACK_SHARE: f32 = 0.5;
pub const BLACK: f32 = 0.02;
/// A pixel is bright from here, and still while it changes less than [`STILL`] over the
/// minute; a still bright region is refused once it covers [`STATIC_SHARE`] of the
/// picture (a single star blinking in place is not one).
pub const BRIGHT: f32 = 0.25;
pub const STILL: f32 = 0.05;
pub const STATIC_SHARE: f32 = 0.001;
/// The time the frames cover (SPEC §9: 60 s) and how many are sampled over it.
pub const SPAN_SECONDS: f32 = 60.0;
pub const SAMPLES: usize = 61;
/// The size of the sampled frames.
pub const GRID: (u32, u32) = (192, 108);

/// The brightness of an sRGB colour (Rec. 709 weights on the 0-1 values).
pub fn brightness(r: u8, g: u8, b: u8) -> f32 {
    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
}

/// A frame's brightness grid from its RGBA pixels.
pub fn grid(rgba: &[u8]) -> Vec<f32> {
    rgba.chunks_exact(4).map(|p| brightness(p[0], p[1], p[2])).collect()
}

/// What the checks measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oled {
    pub average: f32,
    pub black_share: f32,
    /// The share of the picture that stays bright and still.
    pub static_share: f32,
}

/// Why a wallpaper is refused for OLED.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OledFault {
    NotBlack(f32),
    TooBright(f32),
    StaticBright(f32),
}

impl Oled {
    /// Measures `frames` (equal-sized brightness grids, in time order).
    pub fn measure(frames: &[Vec<f32>]) -> Oled {
        let Some(first) = frames.first() else {
            return Oled {
                average: 0.0,
                black_share: 1.0,
                static_share: 0.0,
            };
        };
        let n = first.len().max(1);
        let count = frames.len() as f32;
        let mut sum = 0.0f64;
        let mut black = 0usize;
        let mut lo = vec![f32::MAX; n];
        let mut hi = vec![f32::MIN; n];
        for f in frames {
            for (i, v) in f.iter().take(n).enumerate() {
                sum += f64::from(*v);
                black += usize::from(*v < BLACK);
                lo[i] = lo[i].min(*v);
                hi[i] = hi[i].max(*v);
            }
        }
        let still_bright = lo
            .iter()
            .zip(&hi)
            .filter(|(l, h)| **l >= BRIGHT && **h - **l < STILL)
            .count();
        Oled {
            average: (sum / (n as f64 * f64::from(count))) as f32,
            black_share: black as f32 / (n as f32 * count),
            static_share: still_bright as f32 / n as f32,
        }
    }

    /// The first rule broken, if any.
    pub fn fault(&self) -> Option<OledFault> {
        if self.black_share < BLACK_SHARE {
            Some(OledFault::NotBlack(self.black_share))
        } else if self.average > MAX_AVERAGE {
            Some(OledFault::TooBright(self.average))
        } else if self.static_share >= STATIC_SHARE {
            Some(OledFault::StaticBright(self.static_share))
        } else {
            None
        }
    }
}

/// The wallpaper's times the frames are drawn at.
pub fn sample_times() -> Vec<f32> {
    (0..SAMPLES)
        .map(|i| SPAN_SECONDS * i as f32 / (SAMPLES - 1) as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: brightness weights; black, too bright, a still bright square, a bright
    //! square that moves, a single still star, and no frames at all.
    use super::*;

    const N: usize = 100 * 100;

    fn frames(f: impl Fn(usize, usize) -> f32) -> Vec<Vec<f32>> {
        (0..SAMPLES).map(|t| (0..N).map(|i| f(t, i)).collect()).collect()
    }

    #[test]
    fn brightness_follows_the_eye() {
        assert_eq!(brightness(0, 0, 0), 0.0);
        assert!((brightness(255, 255, 255) - 1.0).abs() < 1e-6);
        assert!(brightness(0, 255, 0) > brightness(255, 0, 0) && brightness(255, 0, 0) > brightness(0, 0, 255));
        assert_eq!(grid(&[255, 255, 255, 255, 0, 0, 0, 255]).len(), 2);
    }

    #[test]
    fn a_dark_moving_wallpaper_passes() {
        // Dim waves sliding across: mostly black, nothing still.
        let o = Oled::measure(&frames(|t, i| if (i + t * 37) % 10 == 0 { 0.3 } else { 0.0 }));
        assert_eq!(o.fault(), None, "{o:?}");
        assert!((o.average - 0.03).abs() < 0.005);
    }

    #[test]
    fn a_grey_background_is_not_black() {
        let o = Oled::measure(&frames(|_, _| 0.05));
        assert!(matches!(o.fault(), Some(OledFault::NotBlack(_))), "{o:?}");
    }

    #[test]
    fn too_much_light_is_refused() {
        // Black background, but a fifth of the pixels at 60 % brightness, moving.
        let o = Oled::measure(&frames(|t, i| if (i + t) % 5 == 0 { 0.6 } else { 0.0 }));
        assert!(
            matches!(o.fault(), Some(OledFault::TooBright(a)) if a > MAX_AVERAGE),
            "{o:?}"
        );
    }

    #[test]
    fn a_bright_square_that_stays_is_refused_and_one_that_moves_is_not() {
        let square = |x0: usize| move |i: usize| (x0..x0 + 10).contains(&(i % 100)) && (i / 100) < 10;
        let still = Oled::measure(&frames(|_, i| if square(0)(i) { 0.8 } else { 0.0 }));
        assert!(matches!(still.fault(), Some(OledFault::StaticBright(_))), "{still:?}");
        let moving = Oled::measure(&frames(|t, i| if square(t % 90)(i) { 0.8 } else { 0.0 }));
        assert_eq!(moving.fault(), None, "{moving:?}");
        // A bright square that only pulses a little is still in place.
        let pulse = Oled::measure(&frames(
            |t, i| if square(0)(i) { 0.8 + (t % 2) as f32 * 0.01 } else { 0.0 },
        ));
        assert!(matches!(pulse.fault(), Some(OledFault::StaticBright(_))));
        // One still star (a pixel in ten thousand) is allowed.
        let star = Oled::measure(&frames(|_, i| if i == 5050 { 0.9 } else { 0.0 }));
        assert_eq!(star.fault(), None, "{star:?}");
    }

    #[test]
    fn the_samples_span_a_minute() {
        let t = sample_times();
        assert_eq!((t.len(), t[0], *t.last().unwrap()), (SAMPLES, 0.0, SPAN_SECONDS));
        assert_eq!(Oled::measure(&[]).fault(), None);
    }
}
