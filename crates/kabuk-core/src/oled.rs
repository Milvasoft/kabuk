//! OLED care (SPEC §5): the bar's content moves by a pixel every few minutes so the same
//! pixels are not lit forever, and the bar and windows dim after a while without input.

use std::time::Duration;

/// How often the bar content moves.
pub const SHIFT_INTERVAL: Duration = Duration::from_secs(180);

/// Offsets in physical pixels, visited in order: a 1-2 px square walk that returns home.
pub const SHIFT_PATTERN: [(i32, i32); 8] = [(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (0, 1)];

/// The largest offset on either axis; a surface leaves this much room around its content.
pub const MAX_SHIFT: i32 = 2;

/// The offset for the `step`-th shift (wraps around the pattern).
pub fn shift_at(step: u64) -> (i32, i32) {
    SHIFT_PATTERN[(step % SHIFT_PATTERN.len() as u64) as usize]
}

/// The design's dimmed opacities (`.screen.idle .win { opacity: .55 }`,
/// `.screen.idle .bar { opacity: .35 }`).
pub const DIMMED_WINDOWS: f32 = 0.55;
pub const DIMMED_BAR: f32 = 0.35;
/// In light mode dimming starts after this share of the delay (SPEC §5: earlier, as a
/// light screen lights far more of the panel; DECISIONS D31).
pub const LIGHT_DELAY_SHARE: f32 = 0.5;
/// Never sooner than this, whatever the share gives.
pub const MIN_DIM_DELAY: Duration = Duration::from_secs(10);

/// What idle dimming does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleStep {
    /// Dim now.
    Dim,
    /// Look again after this long (the time left, or the whole delay while something
    /// keeps the screen in use).
    CheckIn(Duration),
}

/// The delay for the mode in use.
pub fn dim_delay(delay: Duration, light: bool) -> Duration {
    if light {
        delay.mul_f32(LIGHT_DELAY_SHARE).max(MIN_DIM_DELAY).min(delay)
    } else {
        delay
    }
}

/// After `idle` without input, with dimming after `delay`: dim, or when to look again.
/// `busy` (media playing, a full-screen program, a program keeping the display on)
/// holds it off for another whole delay.
pub fn idle_step(idle: Duration, delay: Duration, busy: bool) -> IdleStep {
    if busy {
        return IdleStep::CheckIn(delay);
    }
    match delay.checked_sub(idle) {
        Some(left) if !left.is_zero() => IdleStep::CheckIn(left),
        _ => IdleStep::Dim,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: first step is home, wrap-around, every move is at most one pixel per
    //! axis, offsets stay within 0..=2 px.
    use super::*;

    #[test]
    fn pattern_walks_gently_and_wraps() {
        assert_eq!(shift_at(0), (0, 0));
        assert_eq!(shift_at(8), (0, 0));
        assert_eq!(shift_at(9), shift_at(1));
        for i in 0..SHIFT_PATTERN.len() as u64 {
            let (a, b) = (shift_at(i), shift_at(i + 1));
            assert!((a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1, "{a:?} -> {b:?}");
            assert!((0..=MAX_SHIFT).contains(&a.0) && (0..=MAX_SHIFT).contains(&a.1));
        }
        assert!(SHIFT_INTERVAL >= Duration::from_secs(60));
    }

    /// SPEC §5, with a fake clock: dims once the delay passed without input, waits for
    /// what is left otherwise, never while something keeps the screen in use; light mode
    /// sooner but not below 10 s.
    #[test]
    fn idle_dimming_waits_for_the_time_left() {
        let s = Duration::from_secs;
        assert_eq!(idle_step(s(0), s(60), false), IdleStep::CheckIn(s(60)));
        assert_eq!(idle_step(s(45), s(60), false), IdleStep::CheckIn(s(15)));
        assert_eq!(idle_step(s(60), s(60), false), IdleStep::Dim);
        assert_eq!(idle_step(s(300), s(60), false), IdleStep::Dim);
        assert_eq!(idle_step(s(300), s(60), true), IdleStep::CheckIn(s(60)));
        assert_eq!(dim_delay(s(60), false), s(60));
        assert_eq!(dim_delay(s(60), true), s(30));
        assert_eq!(dim_delay(s(12), true), s(10));
        assert_eq!(dim_delay(s(10), true), s(10));
    }
}
