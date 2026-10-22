//! The battery as the bar's battery indicator shows it (DECISIONS D88). Pure.

/// What Windows reports of the battery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// Charge left, 0 to 100.
    pub percent: u8,
    pub charging: bool,
    /// On the charger (charging or full).
    pub plugged: bool,
    /// Time left on the battery, when Windows has worked it out (not while plugged).
    pub seconds_left: Option<u32>,
}

/// At or under this much charge, off the charger, the level is drawn in the warning colour,
/// as Windows' own battery icon turns at its low level.
pub const LOW: u8 = 15;

impl Battery {
    /// Low and not charging: the level is drawn as a warning.
    pub fn low(&self) -> bool {
        self.percent <= LOW && !self.plugged
    }

    /// How much of the icon's body the level fills, 0 to 1; never quite empty, so an
    /// almost flat battery still shows a sliver.
    pub fn fill(&self) -> f32 {
        f32::from(self.percent.clamp(4, 100)) / 100.0
    }

    /// The time left as hours and minutes, when known and on the battery.
    pub fn left(&self) -> Option<(u32, u32)> {
        let s = self.seconds_left.filter(|_| !self.plugged)?;
        let minutes = s / 60;
        Some((minutes / 60, minutes % 60))
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: low on and off the charger, at the line and over it; the fill's floor
    //! and ceiling; time left unknown, while plugged, under an hour.
    use super::*;

    fn b(percent: u8, plugged: bool, seconds_left: Option<u32>) -> Battery {
        Battery {
            percent,
            charging: plugged && percent < 100,
            plugged,
            seconds_left,
        }
    }

    #[test]
    fn low_only_off_the_charger_at_the_line() {
        assert!(b(15, false, None).low());
        assert!(!b(16, false, None).low());
        assert!(!b(5, true, None).low());
    }

    #[test]
    fn the_fill_never_vanishes() {
        assert_eq!(b(0, false, None).fill(), 0.04);
        assert_eq!(b(82, false, None).fill(), 0.82);
        assert_eq!(b(100, true, None).fill(), 1.0);
    }

    #[test]
    fn time_left_in_hours_and_minutes() {
        assert_eq!(b(82, false, Some(3 * 3600 + 12 * 60 + 30)).left(), Some((3, 12)));
        assert_eq!(b(20, false, Some(40 * 60)).left(), Some((0, 40)));
        assert_eq!(b(82, false, None).left(), None);
        assert_eq!(b(82, true, Some(3600)).left(), None);
    }
}
