//! The watchdog's restart budget: at most `max` restarts within `window`.

use std::collections::VecDeque;
use std::time::Duration;

/// ARCHITECTURE §2: at most 3 restarts in 60 s.
pub const MAX_RESTARTS: usize = 3;
pub const RESTART_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Restart,
    GiveUp,
}

#[derive(Clone, Debug)]
pub struct RestartLimiter {
    max: usize,
    window: Duration,
    restarts: VecDeque<Duration>,
}

impl RestartLimiter {
    pub fn new(max: usize, window: Duration) -> RestartLimiter {
        RestartLimiter {
            max,
            window,
            restarts: VecDeque::new(),
        }
    }

    /// Called when the shell crashed at `now`; records the restart if allowed.
    pub fn on_crash(&mut self, now: Duration) -> Decision {
        while let Some(&first) = self.restarts.front() {
            if now.saturating_sub(first) >= self.window {
                self.restarts.pop_front();
            } else {
                break;
            }
        }
        if self.restarts.len() >= self.max {
            Decision::GiveUp
        } else {
            self.restarts.push_back(now);
            Decision::Restart
        }
    }

    /// Restarts counted in the current window.
    pub fn recent(&self) -> usize {
        self.restarts.len()
    }
}

impl Default for RestartLimiter {
    fn default() -> Self {
        RestartLimiter::new(MAX_RESTARTS, RESTART_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: first crash, exactly `max` crashes, the crash after that, crashes
    //! spread just inside / exactly at the window boundary, recovery after the
    //! window passes, a zero budget.
    use super::*;
    use crate::clock::{Clock, FakeClock};

    #[test]
    fn allows_three_restarts_then_gives_up() {
        let clock = FakeClock::new();
        let mut l = RestartLimiter::default();
        for _ in 0..3 {
            assert_eq!(l.on_crash(clock.now()), Decision::Restart);
            clock.advance(Duration::from_secs(1));
        }
        assert_eq!(l.on_crash(clock.now()), Decision::GiveUp);
        assert_eq!(l.recent(), 3);
    }

    #[test]
    fn crashes_spread_over_more_than_the_window_keep_restarting() {
        let clock = FakeClock::new();
        let mut l = RestartLimiter::default();
        for _ in 0..20 {
            assert_eq!(l.on_crash(clock.now()), Decision::Restart);
            clock.advance(Duration::from_secs(21));
        }
    }

    #[test]
    fn window_boundary() {
        let clock = FakeClock::new();
        let mut l = RestartLimiter::default();
        for _ in 0..3 {
            assert_eq!(l.on_crash(clock.now()), Decision::Restart);
        }
        clock.advance(Duration::from_millis(59_999));
        assert_eq!(l.on_crash(clock.now()), Decision::GiveUp);
        clock.advance(Duration::from_millis(1));
        // The first three are now exactly 60 s old and no longer count.
        assert_eq!(l.on_crash(clock.now()), Decision::Restart);
        assert_eq!(l.recent(), 1);
    }

    #[test]
    fn zero_budget_never_restarts() {
        let mut l = RestartLimiter::new(0, RESTART_WINDOW);
        assert_eq!(l.on_crash(Duration::ZERO), Decision::GiveUp);
    }
}
