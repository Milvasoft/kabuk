//! Telling a hung shell from a busy one (DECISIONS D55). The watchdog asks the shell's
//! bar and window manager every [`PROBE_EVERY`] whether they answer; once none of their
//! answers came for [`HUNG_AFTER`], over at least [`MISSED_PROBES`] questions in a row,
//! the shell is hung: the watchdog ends it, restores the desktop and starts it again, as
//! after a crash (found by the user: the bar and the window manager froze for a quarter
//! of an hour and nothing noticed).

use std::time::Duration;

/// How often the watchdog asks.
pub const PROBE_EVERY: Duration = Duration::from_secs(5);
/// How long one question waits for its answer.
pub const ANSWER_WAIT: Duration = Duration::from_secs(2);
/// How long the shell may stay silent.
pub const HUNG_AFTER: Duration = Duration::from_secs(15);
/// Questions in a row that must go unanswered too, so that time jumping ahead (the PC
/// waking from sleep) never ends a shell by itself.
pub const MISSED_PROBES: u32 = 3;
/// The window classes of the bar and of the window manager.
pub const BAR_CLASS: &str = "Kabuk.Bar";
pub const WM_CLASS: &str = "Kabuk.Wm";
/// The classes of the shell's windows that are asked: the bar and the window manager,
/// whose threads run everything the user sees.
pub const PROBED_CLASSES: [&str; 2] = [BAR_CLASS, WM_CLASS];

/// What one question found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Every window asked answered.
    Answered,
    /// At least one did not.
    Silent,
    /// No window to ask (the shell is starting or runs without them).
    NoWindows,
}

/// Follows the answers of one shell.
#[derive(Clone, Debug, Default)]
pub struct HangWatch {
    /// When the silence began, and how many questions it lasted.
    silent_since: Option<Duration>,
    missed: u32,
}

impl HangWatch {
    /// Records the question asked at `now`; true once the shell counts as hung.
    pub fn probe(&mut self, now: Duration, probe: Probe) -> bool {
        match probe {
            Probe::Silent => {
                let since = *self.silent_since.get_or_insert(now);
                self.missed += 1;
                self.missed >= MISSED_PROBES && now.saturating_sub(since) >= HUNG_AFTER
            }
            Probe::Answered | Probe::NoWindows => {
                *self = HangWatch::default();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: answers keep it alive; silence just under and exactly at the limit;
    //! one answer in the middle starts the count again; a jump of time with too few
    //! questions; no windows to ask; the limits fit into the probing rhythm.
    use super::*;

    fn s(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn a_shell_that_answers_is_never_hung() {
        let mut w = HangWatch::default();
        for i in 0..100 {
            assert!(!w.probe(s(i * 5), Probe::Answered));
        }
    }

    #[test]
    fn silence_for_the_limit_is_a_hang() {
        let mut w = HangWatch::default();
        assert!(!w.probe(s(0), Probe::Silent));
        assert!(!w.probe(s(5), Probe::Silent));
        assert!(!w.probe(s(10), Probe::Silent));
        assert!(!w.probe(Duration::from_millis(14_999), Probe::Silent));
        assert!(w.probe(s(15), Probe::Silent));
    }

    #[test]
    fn one_answer_starts_the_count_again() {
        let mut w = HangWatch::default();
        for t in [0, 5, 10] {
            assert!(!w.probe(s(t), Probe::Silent));
        }
        assert!(!w.probe(s(12), Probe::Answered));
        for t in [15, 20, 25] {
            assert!(!w.probe(s(t), Probe::Silent));
        }
        assert!(w.probe(s(30), Probe::Silent));
    }

    /// The PC slept between two questions: the time passed, the questions did not.
    #[test]
    fn a_jump_of_time_alone_is_not_a_hang() {
        let mut w = HangWatch::default();
        assert!(!w.probe(s(0), Probe::Silent));
        assert!(!w.probe(s(3600), Probe::Silent));
        assert!(!w.probe(s(3601), Probe::Answered));
    }

    #[test]
    fn a_shell_without_windows_is_not_judged() {
        let mut w = HangWatch::default();
        assert!(!w.probe(s(0), Probe::Silent));
        assert!(!w.probe(s(5), Probe::Silent));
        assert!(!w.probe(s(10), Probe::NoWindows));
        assert!(!w.probe(s(15), Probe::Silent));
        assert!(!w.probe(s(20), Probe::NoWindows));
    }

    #[test]
    fn the_limits_fit_the_rhythm() {
        // A question waits less than the time between two of them, and the silence
        // lasts for more questions than the minimum, so both conditions can be met.
        assert!(ANSWER_WAIT < PROBE_EVERY);
        assert!(HUNG_AFTER >= PROBE_EVERY * (MISSED_PROBES - 1));
    }
}
