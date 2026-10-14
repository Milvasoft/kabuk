//! Time source. Code that waits or measures takes a `Clock` so tests can use
//! [`FakeClock`] instead of sleeping.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    /// Monotonic time since an arbitrary, fixed origin.
    fn now(&self) -> Duration;
}

/// The real monotonic clock.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> SystemClock {
        SystemClock { origin: Instant::now() }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that only moves when told to.
#[derive(Debug, Default)]
pub struct FakeClock {
    now: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> FakeClock {
        FakeClock::default()
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        *now += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Local calendar time as shown on the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallTime {
    pub year: u16,
    /// 1 … 12.
    pub month: u8,
    pub day: u8,
    /// 0 = Sunday … 6.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millisecond: u16,
}

impl WallTime {
    /// Time until the next minute starts, so the clock wakes exactly once per minute.
    pub fn until_next_minute(&self) -> Duration {
        let into_minute = u64::from(self.second.min(59)) * 1000 + u64::from(self.millisecond.min(999));
        Duration::from_millis(60_000 - into_minute)
    }

    /// Same minute (the clock text would not change).
    pub fn same_minute(&self, other: &WallTime) -> bool {
        (self.year, self.month, self.day, self.hour, self.minute)
            == (other.year, other.month, other.day, other.hour, other.minute)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: fake clock starts at zero, only moves on advance, accumulates;
    //! system clock never goes backwards; minute boundary at :00.000, :59.999 and
    //! out-of-range leap seconds; same-minute comparison across days.
    use super::*;

    fn wall(hour: u8, minute: u8, second: u8, millisecond: u16) -> WallTime {
        WallTime {
            year: 2026,
            month: 10,
            day: 12,
            weekday: 1,
            hour,
            minute,
            second,
            millisecond,
        }
    }

    #[test]
    fn next_minute() {
        assert_eq!(wall(20, 31, 0, 0).until_next_minute(), Duration::from_secs(60));
        assert_eq!(wall(20, 31, 59, 999).until_next_minute(), Duration::from_millis(1));
        assert_eq!(wall(20, 31, 30, 500).until_next_minute(), Duration::from_millis(29_500));
        // A leap second (60) is treated as the last second of the minute.
        assert_eq!(wall(23, 59, 60, 0).until_next_minute(), Duration::from_secs(1));
    }

    #[test]
    fn same_minute() {
        assert!(wall(8, 5, 1, 0).same_minute(&wall(8, 5, 59, 999)));
        assert!(!wall(8, 5, 1, 0).same_minute(&wall(8, 6, 0, 0)));
        let mut next_day = wall(8, 5, 1, 0);
        next_day.day = 13;
        assert!(!wall(8, 5, 1, 0).same_minute(&next_day));
    }

    #[test]
    fn fake_clock_moves_only_when_advanced() {
        let c = FakeClock::new();
        assert_eq!(c.now(), Duration::ZERO);
        assert_eq!(c.now(), Duration::ZERO);
        c.advance(Duration::from_millis(1500));
        c.advance(Duration::from_millis(500));
        assert_eq!(c.now(), Duration::from_secs(2));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::new();
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }

    #[test]
    fn clocks_work_behind_a_trait_object() {
        let c: Box<dyn Clock> = Box::new(FakeClock::new());
        assert_eq!(c.now(), Duration::ZERO);
    }
}
