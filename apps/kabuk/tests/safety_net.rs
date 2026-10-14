//! M0 safety net, end to end with real processes: watchdog crash recovery, restart
//! budget, `kabuk --restore` in every state, emergency hotkey, single instance,
//! leftover restore on start.
//!
//! Edge cases: forced kill (TerminateProcess), repeated crashes, missing / corrupt /
//! stale state files, a handle recorded with the wrong owner, a running shell stopped
//! by `--restore`, the hotkey delivered to the watchdog and to a lone shell, a second
//! shell for the same state directory, taskbar and work-area entries that must be
//! re-applied without changing anything.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::time::Duration;

use common::*;
use kabuk_core::instance::{exit, Instance};
use kabuk_core::state::{StateFile, TaskbarBackup, WorkAreaBackup};
use kabuk_win::msgloop::{post_message, Msg, MSG_HOTKEY};
use kabuk_win::process::Process;
use kabuk_win::window::{find_message_window, Hwnd};

/// SPEC §8.1: everything is back within 2 seconds of a crash.
const RESTORE_LIMIT: Duration = Duration::from_secs(2);
const EMERGENCY_HOTKEY_ID: usize = 1;

fn hidden_in_state(sandbox: &Sandbox, hwnd: Hwnd) -> bool {
    sandbox.state().hidden_windows.iter().any(|w| w.hwnd == hwnd.0)
}

fn wait_hidden_by_shell(sandbox: &Sandbox, window: &TestWindow) {
    assert!(
        wait_until(STARTUP_TIMEOUT, || !window.hwnd.is_visible()
            && hidden_in_state(sandbox, window.hwnd))
        .is_some(),
        "the shell never hid the test window"
    );
}

#[test]
fn watchdog_restores_hidden_window_within_two_seconds_after_a_kill() {
    let sandbox = Sandbox::new();
    let t = title("crash");
    let window = TestWindow::open(&["--title", &t]);
    let mut watchdog = sandbox.watchdog(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&sandbox, &window);

    let pid = sandbox.state().kabuk_pid.expect("shell pid recorded");
    Process::open(pid).unwrap().terminate(1).unwrap();
    let took = wait_until(Duration::from_secs(5), || window.hwnd.is_visible()).expect("window came back");
    assert!(took <= RESTORE_LIMIT, "restore took {took:?}");

    // The watchdog started a new shell, which does not hide the window again.
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.state().kabuk_pid.is_some_and(|p| p != pid)).is_some());
    assert!(window.hwnd.is_visible());
    assert!(!hidden_in_state(&sandbox, window.hwnd));
    assert_eq!(sandbox.log_lines("shell crashed; desktop restored"), 1);

    let (code, _) = sandbox.restore();
    assert_eq!(code, 0);
    let status = watchdog
        .wait_exit(Duration::from_secs(5))
        .expect("watchdog stops after --restore");
    assert_eq!(status.code(), Some(exit::OK));
}

#[test]
fn watchdog_gives_up_after_three_restarts_in_a_minute() {
    let sandbox = Sandbox::new();
    let mut watchdog = sandbox.watchdog(&["--debug-crash-after-ms", "50"]);
    let status = watchdog.wait_exit(Duration::from_secs(20)).expect("watchdog gave up");
    assert_eq!(status.code(), Some(exit::GAVE_UP));
    // First start plus three restarts.
    assert_eq!(sandbox.log_lines("shell started"), 4);
    assert_eq!(sandbox.log_lines("too many crashes; giving up"), 1);
}

#[test]
fn restore_shows_recorded_windows_and_exits_zero() {
    let sandbox = Sandbox::new();
    let a = TestWindow::open(&["--title", &title("restore-a"), "--hidden"]);
    let b = TestWindow::open(&["--title", &title("restore-b"), "--hidden"]);
    assert!(!a.hwnd.is_visible() && !b.hwnd.is_visible());
    sandbox.write_hidden(&[a.record(), b.record()]);

    let (code, out) = sandbox.restore();
    assert_eq!(code, 0);
    assert!(out.contains('2'), "{out}");
    assert!(wait_until(RESTORE_LIMIT, || a.hwnd.is_visible() && b.hwnd.is_visible()).is_some());
    assert!(!sandbox.state().has_changes());
}

#[test]
fn restore_exits_zero_with_missing_corrupt_and_stale_state() {
    // No state directory content at all.
    let sandbox = Sandbox::new();
    assert_eq!(sandbox.restore().0, 0);

    // Corrupt file: moved aside, still exit 0.
    std::fs::write(sandbox.state_file(), "{ not json").unwrap();
    assert_eq!(sandbox.restore().0, 0);
    assert!(sandbox.state_dir().join("state.json.corrupt").exists());

    // A window that no longer exists.
    let gone = TestWindow::open(&["--title", &title("gone"), "--hidden"]);
    let record = gone.record();
    drop(gone);
    assert!(wait_until(Duration::from_secs(5), || !Hwnd(record.hwnd).exists()).is_some());
    sandbox.write_hidden(&[record]);
    assert_eq!(sandbox.restore().0, 0);
    assert!(!sandbox.state().has_changes());
}

#[test]
fn restore_leaves_a_reused_handle_alone() {
    let sandbox = Sandbox::new();
    let w = TestWindow::open(&["--title", &title("reused"), "--hidden"]);
    let mut record = w.record();
    record.pid += 4; // as if the handle now belonged to another process
    sandbox.write_hidden(&[record]);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_millis(300), || w.hwnd.is_visible()).is_none());
    assert_eq!(sandbox.log_lines("handle now belongs to another window"), 1);
}

#[test]
fn restore_stops_a_running_shell_first() {
    let sandbox = Sandbox::new();
    let t = title("stop-shell");
    let window = TestWindow::open(&["--title", &t]);
    let mut shell = sandbox.shell(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&sandbox, &window);

    let (code, _) = sandbox.restore();
    assert_eq!(code, 0);
    let status = shell.wait_exit(Duration::from_secs(5)).expect("shell stopped");
    assert_eq!(status.code(), Some(exit::EMERGENCY));
    assert!(wait_until(RESTORE_LIMIT, || window.hwnd.is_visible()).is_some());
}

#[test]
fn emergency_hotkey_on_the_watchdog_stops_and_restores_everything() {
    let sandbox = Sandbox::new();
    let t = title("hotkey-watchdog");
    let window = TestWindow::open(&["--title", &t]);
    let mut watchdog = sandbox.watchdog(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&sandbox, &window);
    let shell_pid = sandbox.state().kabuk_pid.unwrap();
    let shell = Process::open(shell_pid).unwrap();

    let id = Instance::for_state_dir(sandbox.state_dir());
    let target = find_message_window("Kabuk.Watchdog", id.id()).expect("watchdog window");
    post_message(
        target,
        Msg {
            id: MSG_HOTKEY,
            wparam: EMERGENCY_HOTKEY_ID,
            lparam: 0,
        },
    )
    .unwrap();

    let took = wait_until(Duration::from_secs(5), || window.hwnd.is_visible()).expect("window came back");
    assert!(took <= RESTORE_LIMIT, "restore took {took:?}");
    assert_eq!(
        watchdog.wait_exit(Duration::from_secs(5)).and_then(|s| s.code()),
        Some(exit::OK)
    );
    assert!(shell.wait(Some(Duration::from_secs(2))));
    assert_eq!(shell.exit_code(), Some(exit::EMERGENCY as u32));
}

#[test]
fn emergency_hotkey_on_a_lone_shell_stops_and_restores() {
    let sandbox = Sandbox::new();
    let t = title("hotkey-shell");
    let window = TestWindow::open(&["--title", &t]);
    let mut shell = sandbox.shell(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&sandbox, &window);

    let id = Instance::for_state_dir(sandbox.state_dir());
    let target = find_message_window("Kabuk.Shell", id.id()).expect("shell window");
    post_message(
        target,
        Msg {
            id: MSG_HOTKEY,
            wparam: EMERGENCY_HOTKEY_ID,
            lparam: 0,
        },
    )
    .unwrap();

    assert_eq!(
        shell.wait_exit(Duration::from_secs(5)).and_then(|s| s.code()),
        Some(exit::EMERGENCY)
    );
    assert!(wait_until(RESTORE_LIMIT, || window.hwnd.is_visible()).is_some());
    assert!(!sandbox.state().has_changes());
}

#[test]
fn shell_restores_leftovers_from_a_previous_crash_on_start() {
    let sandbox = Sandbox::new();
    let window = TestWindow::open(&["--title", &title("leftover"), "--hidden"]);
    sandbox.write_hidden(&[window.record()]);
    let mut shell = sandbox.shell(&[]);
    assert!(wait_until(RESTORE_LIMIT, || window.hwnd.is_visible()).is_some());
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.state().kabuk_pid.is_some()).is_some());
    assert!(!sandbox.state().has_changes());
    assert!(shell.is_running());
}

/// Asked for by the user: never two shells on one desk, the one started later takes
/// over. The first gives way as `--restore` would stop it (the windows it hid come back),
/// then the second runs.
#[test]
fn a_second_shell_for_the_same_state_takes_over() {
    let sandbox = Sandbox::new();
    let t = title("second");
    let window = TestWindow::open(&["--title", &t]);
    let mut first = sandbox.shell(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&sandbox, &window);
    let first_pid = sandbox.state().kabuk_pid;

    let mut second = sandbox.shell(&[]);
    assert!(first.wait_exit(Duration::from_secs(5)).is_some(), "the first stayed");
    assert!(window.hwnd.is_visible(), "its window stayed hidden");
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.state().kabuk_pid.is_some()
            && sandbox.state().kabuk_pid != first_pid)
        .is_some(),
        "the second did not take over"
    );
    assert!(second.is_running());
    assert_eq!(sandbox.restore().0, 0);
}

/// Asked for by the user: a watchdog started again takes over, with a shell of its own.
#[test]
fn a_second_watchdog_for_the_same_state_takes_over() {
    let sandbox = Sandbox::new();
    let mut first = sandbox.watchdog(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.state().kabuk_pid.is_some()).is_some());
    let first_shell = sandbox.state().kabuk_pid;
    let mut second = sandbox.watchdog(&[]);
    assert!(
        first.wait_exit(Duration::from_secs(5)).is_some(),
        "the first watchdog stayed"
    );
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.state().kabuk_pid.is_some()
            && sandbox.state().kabuk_pid != first_shell)
        .is_some(),
        "no shell of the second watchdog"
    );
    assert!(second.is_running());
    assert_eq!(
        sandbox.log_lines("a watchdog already runs for this state directory; it gives way"),
        1
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// Asked for by the user: a shell of another state directory (another build, a test)
/// does not run beside the one on the desk; the one there gives way and its windows
/// come back.
#[test]
fn a_shell_of_another_state_directory_takes_the_desk() {
    let mine = Sandbox::new();
    let other = Sandbox::new();
    let t = title("desk");
    let window = TestWindow::open(&["--title", &t]);
    let mut first = mine.shell(&["--debug-hide-title", &t]);
    wait_hidden_by_shell(&mine, &window);
    let mut second = other.shell(&[]);
    assert!(first.wait_exit(Duration::from_secs(5)).is_some(), "both shells run");
    assert!(window.hwnd.is_visible(), "its window stayed hidden");
    assert!(!mine.state().has_changes(), "its changes stayed");
    assert!(wait_until(STARTUP_TIMEOUT, || other.state().kabuk_pid.is_some()).is_some());
    assert!(second.is_running());
    assert_eq!(other.log_lines("another shell has the desk; it gives way"), 1);
    assert_eq!(other.restore().0, 0);
}

#[test]
fn restore_reapplies_taskbar_and_work_area_entries() {
    // The recorded values are the current ones, so re-applying them changes nothing
    // on the user's desktop while exercising the same code path as a real restore.
    let sandbox = Sandbox::new();
    let autohide = kabuk_win::shell::taskbar_autohide();
    let primary = kabuk_win::shell::monitors()[0].clone();
    let mut file = StateFile::open(&sandbox.state_file()).unwrap();
    file.update(|s| {
        s.taskbar = Some(TaskbarBackup { autohide });
        s.work_areas.push(WorkAreaBackup {
            monitor: primary.rect,
            work_area: primary.work_area,
        });
    })
    .unwrap();
    drop(file);

    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::shell::taskbar_autohide(), autohide);
    assert_eq!(kabuk_win::shell::monitors()[0].work_area, primary.work_area);
    assert!(kabuk_win::shell::taskbars().iter().all(|t| t.exists()));
    assert!(!sandbox.state().has_changes());
    assert_eq!(sandbox.log_lines("taskbar_restored: true"), 1);
    assert_eq!(sandbox.log_lines("work_areas_restored: 1"), 1);
}

/// Regression: a work-area record that does not match a current monitor (here: as if
/// written in scaled coordinates) is skipped instead of shrinking the work area.
#[test]
fn restore_skips_work_area_records_for_other_monitor_layouts() {
    let sandbox = Sandbox::new();
    let primary = kabuk_win::shell::monitors()[0].clone();
    let scale = |r: kabuk_core::state::Rect| kabuk_core::state::Rect {
        left: r.left * 4 / 5,
        top: r.top * 4 / 5,
        right: r.right * 4 / 5,
        bottom: r.bottom * 4 / 5,
    };
    let mut file = StateFile::open(&sandbox.state_file()).unwrap();
    file.update(|s| {
        s.work_areas.push(WorkAreaBackup {
            monitor: scale(primary.rect),
            work_area: scale(primary.work_area),
        })
    })
    .unwrap();
    drop(file);

    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::shell::monitors()[0].work_area, primary.work_area);
    assert_eq!(
        sandbox.log_lines("work area record does not match the monitors; skipped"),
        1
    );
    assert!(!sandbox.state().has_changes());
}

#[test]
fn simulated_crash_switch_exits_with_the_crash_code() {
    let sandbox = Sandbox::new();
    let mut shell = sandbox.shell(&["--debug-crash-after-ms", "10"]);
    assert_eq!(shell.wait_exit(Duration::from_secs(5)).and_then(|s| s.code()), Some(99));
}
