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

/// A notice in Windows' notification centre, as the calendar panel lists it (DECISIONS
/// D88).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Windows' id for it, to remove it.
    pub id: u32,
    /// The app that sent it, as Windows names it.
    pub app: String,
    /// Its first line (the title) and the rest.
    pub title: String,
    pub body: String,
    /// When it came, local time.
    pub when: kabuk_core::clock::WallTime,
}

/// `DateTime.UniversalTime` (100 ns since 1601) as local time.
fn local(ticks: i64) -> Option<kabuk_core::clock::WallTime> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ticks = u64::try_from(ticks).ok()?;
    let ft = FILETIME {
        dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut t) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: valid in and out structures.
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut t).ok()?;
    }
    Some(kabuk_core::clock::WallTime {
        year: t.wYear,
        month: u8::try_from(t.wMonth).unwrap_or(1),
        day: u8::try_from(t.wDay).unwrap_or(1),
        weekday: u8::try_from(t.wDayOfWeek).unwrap_or(0),
        hour: u8::try_from(t.wHour).unwrap_or(0),
        minute: u8::try_from(t.wMinute).unwrap_or(0),
        second: u8::try_from(t.wSecond).unwrap_or(0),
        millisecond: t.wMilliseconds,
    })
}

fn listener() -> Option<UserNotificationListener> {
    let listener = UserNotificationListener::Current().ok()?;
    (listener.GetAccessStatus().ok()? == UserNotificationListenerAccessStatus::Allowed).then_some(listener)
}

/// The notices in Windows' notification centre, newest first; `None` when Windows does not
/// let Kabuk read them. Blocks for tens of milliseconds: call it off the bar's thread.
pub fn list() -> Option<Vec<Notice>> {
    use windows::UI::Notifications::KnownNotificationBindings;
    let all = listener()?
        .GetNotificationsAsync(NotificationKinds::Toast)
        .ok()?
        .join()
        .ok()?;
    let mut out = Vec::new();
    for n in all {
        let app = n
            .AppInfo()
            .and_then(|a| a.DisplayInfo())
            .and_then(|d| d.DisplayName())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let texts: Vec<String> = n
            .Notification()
            .and_then(|t| t.Visual())
            .and_then(|v| v.GetBinding(&KnownNotificationBindings::ToastGeneric()?))
            .and_then(|b| b.GetTextElements())
            .map(|els| {
                els.into_iter()
                    .filter_map(|e| e.Text().ok())
                    .map(|t| t.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let Some(when) = n.CreationTime().ok().and_then(|t| local(t.UniversalTime)) else {
            continue;
        };
        out.push(Notice {
            id: n.Id().unwrap_or(0),
            app,
            title: texts.first().cloned().unwrap_or_default(),
            body: texts.get(1..).map(|r| r.join(" ")).unwrap_or_default(),
            when,
        });
    }
    out.sort_by_key(|n| {
        std::cmp::Reverse((
            n.when.year,
            n.when.month,
            n.when.day,
            n.when.hour,
            n.when.minute,
            n.when.second,
        ))
    });
    Some(out)
}

/// Removes notice `id` from Windows' notification centre.
pub fn remove(id: u32) -> std::io::Result<()> {
    let l = listener().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    l.RemoveNotification(id).map_err(crate::sync::io_error)
}

/// Empties Windows' notification centre.
pub fn clear() -> std::io::Result<()> {
    let l = listener().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    l.ClearNotifications().map_err(crate::sync::io_error)
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
