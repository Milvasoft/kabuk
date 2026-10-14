//! The number of notices in Windows' notification centre (DECISIONS D32), counted on a
//! thread of its own and posted to a window.
//!
//! Counting costs tens of milliseconds of CPU (`GetNotificationsAsync`), so it is not done
//! on a timer: a second thread watches the folder of Windows' notification database and
//! tells the window when it changes (a notice arrived or went), and the window asks for a
//! count then. Idle, both threads sleep.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::NotificationKinds;

use crate::msgloop::{self, Msg};
use crate::watch::DirWatch;
use crate::window::Hwnd;

/// How long the watcher sleeps before it looks whether it should stop.
const STOP_CHECK: Duration = Duration::from_secs(1);
/// Writes to the database come in bursts: one change is told after this quiet time, and
/// never more often than [`MIN_GAP`].
const SETTLE: Duration = Duration::from_millis(300);
const MIN_GAP: Duration = Duration::from_secs(2);

/// The count now; `None` when Windows does not let Kabuk read it.
pub fn count() -> Option<u32> {
    let listener = UserNotificationListener::Current().ok()?;
    if listener.GetAccessStatus().ok()? != UserNotificationListenerAccessStatus::Allowed {
        return None;
    }
    listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .ok()?
        .join()
        .ok()?
        .Size()
        .ok()
}

/// The folder of Windows' notification database (`wpndatabase.db`).
pub fn database_folder() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join(r"Microsoft\Windows\Notifications"))
}

/// Counts when asked and posts `message` to `window` with the count in `wparam` and
/// `lparam` 1 (0 when it cannot be read); posts `changed` when the notification centre
/// changed. Stops when dropped.
pub struct Counter {
    ask: Option<mpsc::Sender<()>>,
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Counter {
    pub fn start(window: Hwnd, message: u32, changed: u32) -> std::io::Result<Counter> {
        let (tx, rx) = mpsc::channel::<()>();
        let counting = std::thread::Builder::new()
            .name("notification-count".into())
            .spawn(move || {
                crate::system::com_init_mta();
                while rx.recv().is_ok() {
                    // Several asks while one is counted need one answer.
                    while rx.try_recv().is_ok() {}
                    let n = count();
                    let _ = msgloop::post_message(
                        window,
                        Msg {
                            id: message,
                            wparam: n.map_or(0, |v| v as usize),
                            lparam: isize::from(n.is_some()),
                        },
                    );
                }
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = vec![counting];
        match database_folder().map(|d| DirWatch::new(&d)) {
            Some(Ok(watch)) => {
                let stopping = stop.clone();
                threads.push(
                    std::thread::Builder::new()
                        .name("notification-watch".into())
                        .spawn(move || watch_changes(&watch, &stopping, window, changed))?,
                );
            }
            Some(Err(e)) => tracing::warn!(error = %e, "notification changes not watched"),
            None => tracing::warn!("no notification database folder"),
        }
        Ok(Counter {
            ask: Some(tx),
            stop,
            threads,
        })
    }

    pub fn ask(&self) {
        if let Some(tx) = &self.ask {
            let _ = tx.send(());
        }
    }
}

fn watch_changes(watch: &DirWatch, stop: &AtomicBool, window: Hwnd, changed: u32) {
    use crate::handle::Waitable;
    let mut last: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        if !watch.wait_handle().wait(Some(STOP_CHECK)) {
            continue;
        }
        // Let the burst of writes finish, then tell once.
        let _ = watch.rearm();
        while watch.wait_handle().wait(Some(SETTLE)) {
            let _ = watch.rearm();
        }
        if let Some(t) = last {
            std::thread::sleep(MIN_GAP.saturating_sub(t.elapsed()));
        }
        last = Some(Instant::now());
        let _ = msgloop::post_message(
            window,
            Msg {
                id: changed,
                wparam: 0,
                lparam: 0,
            },
        );
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        drop(self.ask.take());
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the count answers (a number or "not allowed") without failing; the
    //! database folder is under the user's local data; a burst of writes is told once and
    //! the watcher stops when asked.
    use super::*;

    #[test]
    fn the_count_answers() {
        crate::system::com_init_mta();
        if let Some(n) = count() {
            assert!(n < 100_000);
        }
    }

    #[test]
    fn a_change_in_the_folder_is_told_once_and_the_watch_stops() {
        use crate::msgloop::{run_until, MessageWindow, Wake};
        use std::cell::Cell;
        use std::rc::Rc;
        const CHANGED: u32 = crate::msgloop::msg::APP + 77;
        let dir = tempfile::tempdir().unwrap();
        let seen = Rc::new(Cell::new(0));
        let s = seen.clone();
        let w = MessageWindow::new("Kabuk.Test.NotifWatch", "notif-watch", move |m| {
            if m.id == CHANGED {
                s.set(s.get() + 1);
                crate::msgloop::post_quit(0);
                return Some(0);
            }
            None
        })
        .unwrap();
        let watch = DirWatch::new(dir.path()).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let hwnd = w.hwnd();
        let t = std::thread::spawn(move || watch_changes(&watch, &stopping, hwnd, CHANGED));
        // A burst of writes is one change.
        for i in 0..5 {
            std::fs::write(dir.path().join("wpndatabase.db-wal"), vec![0u8; 100 * (i + 1)]).unwrap();
        }
        assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(0));
        assert_eq!(seen.get(), 1);
        stop.store(true, Ordering::Relaxed);
        let started = Instant::now();
        t.join().unwrap();
        assert!(started.elapsed() <= STOP_CHECK + SETTLE * 2, "the watch did not stop");
    }

    #[test]
    fn the_database_folder_is_the_users() {
        let d = database_folder().expect("LOCALAPPDATA");
        assert!(d.ends_with(r"Microsoft\Windows\Notifications"));
        assert!(d.starts_with(std::env::var_os("LOCALAPPDATA").unwrap()));
    }
}
