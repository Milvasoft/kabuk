//! A message to a window after every frame the compositor shows, for that window's own
//! animation. Windows' timers wake every ~15.6 ms at best and out of step with the
//! screen, so an animation stepped by one moves in uneven jumps (found by the user: the
//! auto-hiding bar slid in "takıla takıla" beside windows moving at the screen's pace).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Graphics::Dwm::DwmFlush;

use crate::msgloop::{self, Msg};
use crate::window::Hwnd;

/// How long the ticker waits when the compositor cannot be waited on (it is off, as on
/// a locked screen), so it never spins.
const NO_COMPOSITOR_WAIT: Duration = Duration::from_millis(16);

/// Posts its message to its window after each frame the compositor shows, one at a time:
/// the next goes only once the window said it took the last ([`FrameTicker::taken`]), so
/// a busy window is never flooded. Stops when dropped: its thread ends after the frame it
/// waits for, without the dropping thread waiting on it (PLAN M19 P16: the bar's thread
/// waited up to a frame at the end of every slide and every dock hover).
pub struct FrameTicker {
    stop: Arc<AtomicBool>,
    pending: Arc<AtomicBool>,
}

impl FrameTicker {
    /// Starts posting `msg` (its `wparam` and `lparam` 0) to `window`.
    pub fn start(window: Hwnd, msg: u32) -> std::io::Result<FrameTicker> {
        let stop = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(false));
        let (theirs_stop, theirs_pending) = (stop.clone(), pending.clone());
        std::thread::Builder::new().name("frame-ticker".into()).spawn(move || {
            while !theirs_stop.load(Ordering::Acquire) {
                // SAFETY: plain call; waits for the compositor's next frame.
                if unsafe { DwmFlush() }.is_err() {
                    std::thread::sleep(NO_COMPOSITOR_WAIT);
                }
                if theirs_stop.load(Ordering::Acquire) {
                    break;
                }
                if theirs_pending.swap(true, Ordering::AcqRel) {
                    continue;
                }
                let posted = msgloop::post_message(
                    window,
                    Msg {
                        id: msg,
                        wparam: 0,
                        lparam: 0,
                    },
                );
                if posted.is_err() {
                    // The window is gone.
                    break;
                }
            }
        })?;
        Ok(FrameTicker { stop, pending })
    }

    /// The window took the last message: the next frame posts another.
    pub fn taken(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

impl Drop for FrameTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a window that takes every message gets one per frame, one that does
    //! not take them gets only one; dropping the ticker stops it.
    use super::*;
    use crate::msgloop::{self as ml, MessageWindow};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    const TEST_MSG: u32 = ml::msg::APP + 7;

    #[test]
    fn a_window_gets_a_tick_per_frame_and_none_while_busy() {
        let count = Rc::new(Cell::new(0u32));
        let take = Rc::new(Cell::new(false));
        let ticker: Rc<RefCell<Option<FrameTicker>>> = Rc::default();
        let (c, t, k) = (count.clone(), take.clone(), ticker.clone());
        let window = MessageWindow::new("Kabuk.Test.Ticker", "ticker", move |m| {
            if m.id != TEST_MSG {
                return None;
            }
            c.set(c.get() + 1);
            if t.get() {
                if let Some(k) = k.borrow().as_ref() {
                    k.taken();
                }
            }
            Some(0)
        })
        .expect("a message window");
        *ticker.borrow_mut() = Some(FrameTicker::start(window.hwnd(), TEST_MSG).expect("a ticker"));
        // Not taken: one only.
        ml::run_until(&[], Some(Duration::from_millis(200)));
        assert_eq!(count.get(), 1, "ticks while the window did not take them");
        // Taken: one per frame (the compositor runs at 30 Hz or more; a locked screen
        // falls back to the wait, about 60 a second).
        take.set(true);
        count.set(0);
        if let Some(k) = ticker.borrow().as_ref() {
            k.taken();
        }
        ml::run_until(&[], Some(Duration::from_millis(500)));
        assert!(count.get() >= 10, "{} ticks in 0.5 s", count.get());
        // Stopped at once, without waiting for a frame; none after (one may have been on
        // its way).
        let dropped = std::time::Instant::now();
        drop(ticker.borrow_mut().take());
        assert!(
            dropped.elapsed() < Duration::from_millis(5),
            "dropping waited {:?}",
            dropped.elapsed()
        );
        ml::run_until(&[], Some(Duration::from_millis(30)));
        count.set(0);
        ml::run_until(&[], Some(Duration::from_millis(200)));
        assert_eq!(count.get(), 0, "ticks after the ticker stopped");
    }
}
