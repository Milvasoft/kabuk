//! The window manager on the real desktop, with `kabuk-testapp` windows only (the
//! test-mode window manager ignores every other window and registers no shortcuts;
//! actions come from test messages). TESTING §4.
//!
//! Edge cases: one window, two windows, a window closing, an empty workspace, a window
//! moved to a hidden workspace, a forced kill with windows hidden and a coloured frame,
//! `--restore` with windows on several workspaces, a minimum size that does not fit, a
//! hung window (message loop blocked), an owned dialog and a splash screen, a gap change
//! in the file, the frame reset on a clean exit, the action time budget, swap / float /
//! close on the focused window, the bar showing the window manager's workspaces.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::time::{Duration, Instant};

use common::*;
use kabuk::wm::{test_action, MSG_TEST_ACTION, WM_CLASS};
use kabuk_core::state::Rect;
use kabuk_win::msgloop::{send_message, Msg};
use kabuk_win::process::Process;
use kabuk_win::window::Hwnd;

/// SPEC §8.1: everything is back within 2 seconds of a crash.
const RESTORE_LIMIT: Duration = Duration::from_secs(2);
/// TESTING §4: with a hung window present, shortcuts and moves work within 100 ms.
const HUNG_LIMIT: Duration = Duration::from_millis(100);
/// TESTING §4: a configuration change is applied within 500 ms.
const CONFIG_LIMIT: Duration = Duration::from_millis(500);
/// PLAN M3: shortcut to layout under 16 ms.
const LAYOUT_BUDGET_US: u64 = 16_000;
/// `windows.gap` in the tests' configuration.
const GAP: i32 = 10;
const SETTLE: Duration = Duration::from_secs(3);
/// Game mode off: a full-screen program on the real desktop would still animations.
const NO_GAME: &str = "[game]\ndetect = false\n";

struct Wm {
    _shell: Proc,
    window: Hwnd,
}

impl Wm {
    fn start(sandbox: &Sandbox, config: &str) -> Wm {
        std::fs::write(sandbox.config_file(), config).unwrap();
        let shell = sandbox.shell_with_wm(&[]);
        let window = wait_for_wm(sandbox);
        Wm { _shell: shell, window }
    }

    /// Triggers an action and returns how long the round trip took.
    fn act(&self, code: usize, arg: isize) -> Duration {
        let started = Instant::now();
        send_message(
            self.window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .expect("window manager answers");
        started.elapsed()
    }

    fn focus(&self, w: &TestWindow) {
        self.act(test_action::FOCUSED, w.hwnd.0 as isize);
    }
}

fn wait_for_wm(sandbox: &Sandbox) -> Hwnd {
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("window manager running") >= 1).is_some(),
        "window manager did not start"
    );
    let pid = sandbox.state().kabuk_pid.expect("shell pid");
    kabuk_win::window::windows_of_class(WM_CLASS)
        .into_iter()
        .find(|w| w.pid() == Some(pid))
        .expect("window manager window")
}

fn config(extra: &str) -> String {
    format!("schema = 1\n[windows]\ngap = {GAP}\n{extra}")
}

fn primary() -> kabuk_win::shell::Monitor {
    kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap()
}

/// The gap in physical pixels on `m`.
fn gap_on(m: &kabuk_win::shell::Monitor) -> i32 {
    (f64::from(GAP) * f64::from(m.dpi) / 96.0).round() as i32
}

fn inset(r: Rect, d: i32) -> Rect {
    Rect {
        left: r.left + d,
        top: r.top + d,
        right: r.right - d,
        bottom: r.bottom - d,
    }
}

fn frame(w: &TestWindow) -> Option<Rect> {
    w.hwnd.frame_rect()
}

/// The monitor holding the window's centre.
fn monitor_of(w: &TestWindow) -> kabuk_win::shell::Monitor {
    let f = frame(w).unwrap();
    let (cx, cy) = ((f.left + f.right) / 2, (f.top + f.bottom) / 2);
    kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.rect.left <= cx && cx < m.rect.right && m.rect.top <= cy && cy < m.rect.bottom)
        .unwrap_or_else(primary)
}

/// Waits until `w` fills its monitor's work area inside the gap.
fn fills(w: &TestWindow) -> bool {
    let m = monitor_of(w);
    frame(w) == Some(inset(m.work_area, gap_on(&m)))
}

fn open(name: &str, extra: &[&str]) -> TestWindow {
    let t = title(name);
    let mut args = vec!["--title", t.as_str()];
    args.extend_from_slice(extra);
    TestWindow::open(&args)
}

fn hidden_in_state(sandbox: &Sandbox, w: &TestWindow) -> bool {
    sandbox.state().hidden_windows.iter().any(|h| h.hwnd == w.hwnd.0)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

#[test]
fn windows_tile_when_they_open_and_reflow_when_they_close() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let a = open("tile-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some(), "a: {:?}", frame(&a));
    let b = open("tile-b", &[]);
    let m = monitor_of(&a);
    let g = gap_on(&m);
    let side_by_side = || {
        let (Some(fa), Some(fb)) = (frame(&a), frame(&b)) else {
            return false;
        };
        let area = inset(m.work_area, g);
        fa.left == area.left && fb.right == area.right && fa.right + g == fb.left && !overlaps(&fa, &fb)
    };
    assert!(
        wait_until(SETTLE, side_by_side).is_some(),
        "{:?} {:?}",
        frame(&a),
        frame(&b)
    );
    b.close();
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "a after b closed: {:?}",
        frame(&a)
    );
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn switching_workspaces_hides_and_shows_windows_in_place() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("ws-a", &[]);
    let b = open("ws-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let placed = || {
        let (Some(fa), Some(fb)) = (frame(&a), frame(&b)) else {
            return false;
        };
        !overlaps(&fa, &fb) && fa.right - fa.left < 2000
    };
    assert!(wait_until(SETTLE, placed).is_some());
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());

    wm.act(test_action::WORKSPACE, 2);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible() && !b.hwnd.is_visible()).is_some());
    assert!(hidden_in_state(&sandbox, &a) && hidden_in_state(&sandbox, &b));
    let c = open("ws-c", &[]);
    assert!(
        wait_until(SETTLE, || fills(&c)).is_some(),
        "c on workspace 2: {:?}",
        frame(&c)
    );

    wm.act(test_action::WORKSPACE, 1);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()
        && b.hwnd.is_visible()
        && !c.hwnd.is_visible())
    .is_some());
    assert!(wait_until(SETTLE, || frame(&a) == Some(ra) && frame(&b) == Some(rb)).is_some());
    assert!(hidden_in_state(&sandbox, &c) && !hidden_in_state(&sandbox, &a));

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || c.hwnd.is_visible()).is_some());
}

#[test]
fn a_crash_shows_every_hidden_window_within_two_seconds() {
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config("")).unwrap();
    let _watchdog = sandbox.watchdog_with_wm();
    let window = wait_for_wm(&sandbox);
    let act = |code, arg| {
        send_message(
            window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .unwrap()
    };
    let a = open("crash-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    act(test_action::FOCUSED, a.hwnd.0 as isize);
    assert!(wait_until(SETTLE, || sandbox
        .state()
        .bordered_windows
        .iter()
        .any(|w| w.hwnd == a.hwnd.0))
    .is_some());
    act(test_action::WORKSPACE, 3);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible() && hidden_in_state(&sandbox, &a)).is_some());

    let pid = sandbox.state().kabuk_pid.unwrap();
    Process::open(pid).unwrap().terminate(1).unwrap();
    let back = wait_until(RESTORE_LIMIT, || a.hwnd.is_visible());
    assert!(
        back.is_some(),
        "hidden window not shown within {RESTORE_LIMIT:?} of the crash"
    );
    // The watchdog's restore also took the coloured frame off.
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("borders_reset: 1") >= 1).is_some(),
        "{:?}",
        sandbox.last_log_line(&["restore finished"])
    );
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn restore_shows_windows_of_every_workspace_and_exits_zero() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("restore-a", &[]);
    let b = open("restore-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    wm.focus(&b);
    wm.act(test_action::MOVE_TO_WORKSPACE, 3);
    assert!(wait_until(SETTLE, || !b.hwnd.is_visible()).is_some());
    wm.act(test_action::WORKSPACE, 2);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some());
    let (code, _) = sandbox.restore();
    assert_eq!(code, 0);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible() && b.hwnd.is_visible()).is_some());
    assert!(sandbox.state().hidden_windows.is_empty());
}

#[test]
fn a_window_whose_minimum_size_does_not_fit_floats_and_the_rest_tiles() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let a = open("min-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    let m = monitor_of(&a);
    let wide = format!("{}x{}", (m.work_area.right - m.work_area.left) * 3 / 4, 300);
    let _b = open("min-b", &["--min-size", &wide]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window refused its size") == 1).is_some());
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "a after b floated: {:?}",
        frame(&a)
    );
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn a_hung_window_does_not_slow_actions_or_other_windows() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let hung = open("hung-a", &["--hang-after", "300"]);
    let b = open("hung-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let not_answering = || {
        send_message(
            hung.hwnd,
            Msg {
                id: 0,
                wparam: 0,
                lparam: 0,
            },
            Duration::from_millis(50),
        )
        .is_err()
    };
    assert!(wait_until(SETTLE, not_answering).is_some(), "test window did not hang");

    let round_trip = wm.act(test_action::WORKSPACE, 2);
    assert!(round_trip < HUNG_LIMIT, "action took {round_trip:?}");
    assert!(
        wait_until(HUNG_LIMIT, || !b.hwnd.is_visible()).is_some(),
        "other window not hidden in time"
    );
    let round_trip = wm.act(test_action::WORKSPACE, 1);
    assert!(round_trip < HUNG_LIMIT, "action took {round_trip:?}");
    assert!(
        wait_until(HUNG_LIMIT, || b.hwnd.is_visible()).is_some(),
        "other window not shown in time"
    );
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn owned_dialogs_float_and_splash_screens_are_ignored() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let a = open("dialog-a", &["--dialog"]);
    let dialog = a.dialog.unwrap();
    let dialog_before = dialog.frame_rect();
    let splash = open("splash", &["--splash"]);
    let splash_before = frame(&splash);
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "main window: {:?}",
        frame(&a)
    );
    // The dialog is managed as floating (it follows its owner's workspace); neither it
    // nor the splash screen was moved.
    assert!(wait_until(SETTLE, || sandbox.log_lines(&format!("hwnd={}", dialog.0)) >= 1).is_some());
    assert!(sandbox
        .last_log_line(&["managing window", &format!("hwnd={}", dialog.0)])
        .unwrap()
        .contains("floating=true"));
    assert_eq!(dialog.frame_rect(), dialog_before);
    assert_eq!(frame(&splash), splash_before);
    assert_eq!(sandbox.log_lines(&format!("hwnd={}", splash.hwnd.0)), 0);
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn a_gap_change_in_the_file_applies_within_half_a_second() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let a = open("gap-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    let m = monitor_of(&a);
    std::fs::write(sandbox.config_file(), "schema = 1\n[windows]\ngap = 0\n").unwrap();
    let took = wait_until(CONFIG_LIMIT, || frame(&a) == Some(m.work_area));
    assert!(
        took.is_some(),
        "gap 0 not applied within {CONFIG_LIMIT:?}: {:?}",
        frame(&a)
    );
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn a_clean_exit_takes_the_frame_off_and_shows_everything() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("frame-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    wm.focus(&a);
    assert!(wait_until(SETTLE, || sandbox
        .state()
        .bordered_windows
        .iter()
        .any(|w| w.hwnd == a.hwnd.0))
    .is_some());
    wm.act(test_action::WORKSPACE, 2);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window manager stopped") == 1).is_some());
    let state = sandbox.state();
    assert!(state.bordered_windows.is_empty() && state.hidden_windows.is_empty());
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some());
}

#[test]
fn an_action_lays_out_in_under_16_ms() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let windows: Vec<TestWindow> = (0..3).map(|i| open(&format!("budget-{i}"), &[])).collect();
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 3).is_some());
    for _ in 0..5 {
        wm.act(test_action::CYCLE_LAYOUT, 0);
    }
    let times = action_times(&sandbox);
    assert_eq!(times.len(), 5);
    let worst = *times.iter().max().unwrap();
    println!(
        "worst action time {worst} µs over {} layouts of {} windows",
        times.len(),
        windows.len()
    );
    assert!(worst < LAYOUT_BUDGET_US, "{worst} µs");
    assert_eq!(sandbox.restore().0, 0);
}

/// The `apply_us` of every "action applied" log line.
fn action_times(sandbox: &Sandbox) -> Vec<u64> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(sandbox.dir.path().join("logs")).unwrap().flatten() {
        for l in std::fs::read_to_string(e.path()).unwrap().lines() {
            if let Some(us) = l.split("apply_us=").nth(1).and_then(|r| r.split_whitespace().next()) {
                v.push(us.parse::<u64>().unwrap());
            }
        }
    }
    v
}

#[test]
fn swap_float_and_close_act_on_the_focused_window() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("act-a", &[]);
    let b = open("act-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let apart = || matches!((frame(&a), frame(&b)), (Some(fa), Some(fb)) if !overlaps(&fa, &fb) && fa.left != fb.left);
    assert!(wait_until(SETTLE, apart).is_some());
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());
    let (left, right) = if ra.left < rb.left { (&a, &b) } else { (&b, &a) };
    let (rl, rr) = (frame(left).unwrap(), frame(right).unwrap());

    // Swap: the left window moves right.
    wm.focus(left);
    wm.act(test_action::MOVE, 3);
    assert!(wait_until(SETTLE, || frame(left) == Some(rr) && frame(right) == Some(rl)).is_some());
    // Float it: the other one takes the whole area.
    wm.act(test_action::TOGGLE_FLOAT, 0);
    assert!(wait_until(SETTLE, || fills(right)).is_some());
    // Close it.
    wm.act(test_action::CLOSE, 0);
    assert!(wait_until(SETTLE, || !left.hwnd.exists()).is_some());
    assert_eq!(sandbox.restore().0, 0);
}

/// Found by the user: Win+Shift+arrow did nothing to a maximised window (its tile moved
/// under it, the other window behind it). It is restored and goes to its new place.
#[test]
fn moving_a_maximised_window_restores_it_to_its_new_place() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("maxmove-a", &[]);
    let b = open("maxmove-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let apart = || matches!((frame(&a), frame(&b)), (Some(fa), Some(fb)) if !overlaps(&fa, &fb) && fa.left != fb.left);
    assert!(wait_until(SETTLE, apart).is_some());
    let (left, right) = if frame(&a).unwrap().left < frame(&b).unwrap().left {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let (rl, rr) = (frame(left).unwrap(), frame(right).unwrap());
    assert!(left.hwnd.maximize_async());
    assert!(wait_until(SETTLE, || left.hwnd.is_maximized()).is_some());
    wm.focus(left);
    wm.act(test_action::MOVE, 3);
    let moved = || !left.hwnd.is_maximized() && frame(left) == Some(rr) && frame(right) == Some(rl);
    assert!(
        wait_until(SETTLE, moved).is_some(),
        "maximised {}, left at {:?}, right at {:?}",
        left.hwnd.is_maximized(),
        frame(left),
        frame(right)
    );
    assert_eq!(sandbox.log_lines("maximised window restored to be moved"), 1);
    assert_eq!(sandbox.restore().0, 0);
}

/// Reported by the user: Win and an arrow did not move the focus. Kabuk takes the chord
/// from its keyboard hook, so Windows does not count it as Kabuk's input and refused to
/// bring the next window to the front. Here the action comes as a message, which Windows
/// does not count either: the window to the right must still come to the front, and back.
#[test]
fn focus_toward_brings_the_neighbour_to_the_front() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("toward-a", &[]);
    let b = open("toward-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let apart = || matches!((frame(&a), frame(&b)), (Some(fa), Some(fb)) if !overlaps(&fa, &fb) && fa.left != fb.left);
    assert!(wait_until(SETTLE, apart).is_some());
    let (left, right) = if frame(&a).unwrap().left < frame(&b).unwrap().left {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let front = || kabuk_win::events::foreground();
    wm.focus(left);
    wm.act(test_action::FOCUS, 3);
    assert!(
        wait_until(SETTLE, || front() == Some(right.hwnd)).is_some(),
        "not in front: {:?}",
        front()
    );
    wm.act(test_action::FOCUS, 0);
    assert!(
        wait_until(SETTLE, || front() == Some(left.hwnd)).is_some(),
        "not back: {:?}",
        front()
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// The bar's workspace indicator shows the window manager's workspaces, including an
/// occupied one beyond the first four.
#[test]
fn the_bar_shows_the_window_managers_workspaces() {
    let before: Vec<Rect> = kabuk_win::shell::monitors().iter().map(|m| m.work_area).collect();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config(NO_GAME)).unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    let window = wait_for_wm(&sandbox);
    let a = open("bar-a", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 1).is_some());
    let act = |code, arg| {
        send_message(
            window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .unwrap()
    };
    act(test_action::FOCUSED, a.hwnd.0 as isize);
    act(test_action::MOVE_TO_WORKSPACE, 6);
    let device = monitor_of(&a).device;
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["bar drawn", &device])
            .is_some_and(|l| l.contains("\"6\"")))
        .is_some(),
        "{:?}",
        sandbox.last_log_line(&["bar drawn"])
    );
    // M10: switching to another workspace blends the bar into its new picture (still with
    // Windows' effects off; game mode is off in this test).
    act(test_action::WORKSPACE, 6);
    let animates = kabuk_win::system::animations_on();
    let blended = wait_until(SETTLE, || sandbox.log_lines("workspace blends in") >= 1).is_some();
    assert_eq!(blended, animates, "{:?}", sandbox.last_log_line(&["workspace"]));
    assert_eq!(sandbox.restore().0, 0);
    let restored = || {
        kabuk_win::shell::monitors()
            .iter()
            .map(|m| m.work_area)
            .collect::<Vec<_>>()
            == before
    };
    assert!(wait_until(Duration::from_secs(5), restored).is_some());
}

/// Shortcuts nobody uses, so registering them for a moment disturbs no one.
const RARE_SHORTCUTS: &str = r#"
[shortcuts]
workspace = "Ctrl+Shift"
move_to_workspace = "Ctrl+Shift+Win"
focus_left = ["Ctrl+Alt+Shift+F13"]
focus_down = ["Ctrl+Alt+Shift+F14"]
focus_up = ["Ctrl+Alt+Shift+F15"]
focus_right = ["Ctrl+Alt+Shift+F16"]
move_left = ["Ctrl+Alt+Shift+F17"]
move_down = ["Ctrl+Alt+Shift+F18"]
move_up = ["Ctrl+Alt+Shift+F19"]
move_right = ["Ctrl+Alt+Shift+F20"]
toggle_float = ["Ctrl+Alt+Shift+F21"]
close_window = ["Ctrl+Alt+Shift+F22"]
cycle_layout = ["Ctrl+Alt+Shift+F23"]
toggle_scratchpad = ["Ctrl+Alt+F13"]
to_scratchpad = ["Ctrl+Alt+F14"]
toggle_group = ["Ctrl+Alt+F15"]
next_tab = ["Ctrl+Alt+F16"]
"#;

/// The configured shortcuts are bound and a hotkey message runs its action. Reported by
/// the user: a chord another program had registered first (Alt+Space) never reached
/// Kabuk; with the keyboard hook (D40) it is Kabuk's all the same, so nothing is taken.
#[test]
fn shortcuts_are_bound_even_when_another_program_holds_one() {
    use kabuk_core::config::shortcut::Chord;
    use kabuk_win::msgloop::{post_message, HotKey, MessageWindow, MSG_HOTKEY};
    // Another program holds the cycle-layout chord.
    let other = MessageWindow::new("Kabuk.Test.Taken", "taken", |_| None).unwrap();
    let _taken = HotKey::register(&other, 1, Chord::parse("Ctrl+Alt+Shift+F23").unwrap()).unwrap();

    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        format!("{}{RARE_SHORTCUTS}", config("workspaces = 2\nshown_workspaces = 2\n")),
    )
    .unwrap();
    let _shell = sandbox.shell_with_wm(&["--debug-shortcuts"]);
    let window = wait_for_wm(&sandbox);
    assert!(wait_until(SETTLE, || sandbox.log_lines("shortcuts registered") == 1).is_some());
    assert_eq!(
        sandbox.log_lines("shortcut taken by another program"),
        0,
        "{:?}",
        sandbox.last_log_line(&["taken"])
    );
    // 2 + 2 workspace chords and 15 others, every one of them Kabuk's.
    let registered = sandbox.last_log_line(&["shortcuts registered"]).unwrap();
    let count: usize = registered
        .split("count=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(count, 19, "{registered}");

    let a = open("keys-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    // Hotkey ids follow `kabuk_core::wm::bindings`: 0 is workspace 1, 1 is workspace 2.
    let hotkey = |id| {
        post_message(
            window,
            Msg {
                id: MSG_HOTKEY,
                wparam: id,
                lparam: 0,
            },
        )
        .unwrap()
    };
    hotkey(1);
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(),
        "workspace 2 shortcut"
    );
    hotkey(0);
    assert!(
        wait_until(SETTLE, || a.hwnd.is_visible()).is_some(),
        "workspace 1 shortcut"
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// A window that ignores every move floats after being asked twice; the others tile.
#[test]
fn a_window_that_refuses_to_move_floats_and_the_rest_tiles() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("refuse-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    let stubborn = open("refuse-b", &["--refuse-move", "--size", "500x400"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window refused its size") == 1).is_some());
    assert!(sandbox
        .last_log_line(&["window refused its size"])
        .unwrap()
        .contains(&format!("hwnd={}", stubborn.hwnd.0)));
    assert!(wait_until(SETTLE, || fills(&a)).is_some(), "a: {:?}", frame(&a));
    // Kabuk still answers at once.
    assert!(wm.act(test_action::CYCLE_LAYOUT, 0) < HUNG_LIMIT);
    assert_eq!(sandbox.restore().0, 0);
}

/// A window that keeps resizing itself neither hangs Kabuk nor disturbs the others.
#[test]
fn a_window_that_resizes_itself_does_not_disturb_kabuk() {
    let sandbox = Sandbox::new();
    let wm = Wm::start(&sandbox, &config(""));
    let a = open("self-a", &[]);
    let _restless = open("self-b", &["--resize-self", "50"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    for _ in 0..10 {
        assert!(wm.act(test_action::CYCLE_LAYOUT, 0) < HUNG_LIMIT);
    }
    let a_frame = frame(&a);
    assert!(a_frame.is_some_and(|f| f.right - f.left > 100), "{a_frame:?}");
    assert_eq!(sandbox.restore().0, 0);
}

/// A minimised window leaves the layout; restored, it takes its old place again.
#[test]
fn a_minimised_window_comes_back_to_its_place() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let windows: Vec<TestWindow> = (0..3).map(|i| open(&format!("min-{i}"), &[])).collect();
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 3).is_some());
    let tiled = || {
        let f: Vec<Option<Rect>> = windows.iter().map(frame).collect();
        f.iter().all(|r| r.is_some_and(|r| r.right - r.left < 2000))
            && (0..3).all(|i| (i + 1..3).all(|j| !overlaps(&f[i].unwrap(), &f[j].unwrap())))
    };
    assert!(wait_until(SETTLE, tiled).is_some());
    let places: Vec<Rect> = windows.iter().map(|w| frame(w).unwrap()).collect();
    let middle = &windows[1];
    middle.hwnd.minimize_async();
    assert!(wait_until(SETTLE, || sandbox.log_lines("window minimized") == 1).is_some());
    // The other two now share the space.
    assert!(wait_until(SETTLE, || frame(&windows[0]) != Some(places[0])
        || frame(&windows[2]) != Some(places[2]))
    .is_some());
    middle.hwnd.restore_async();
    assert!(wait_until(SETTLE, || sandbox.log_lines("window restored") == 1).is_some());
    assert!(
        wait_until(SETTLE, || windows
            .iter()
            .zip(&places)
            .all(|(w, p)| frame(w) == Some(*p)))
        .is_some(),
        "{:?} vs {places:?}",
        windows.iter().map(frame).collect::<Vec<_>>()
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// A window that goes to another virtual desktop (cloaked, yet "visible" to Windows)
/// leaves the layout.
#[test]
fn a_cloaked_window_leaves_the_layout() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let a = open("cloak-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    let b = open("cloak-b", &["--cloak-after", "500"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    assert!(
        wait_until(SETTLE, || b.hwnd.is_cloaked()).is_some(),
        "test window did not cloak"
    );
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "a after b cloaked: {:?}",
        frame(&a)
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// A failure inside the window manager ends the shell (a panic in a window procedure
/// aborts the process), so the watchdog restores the desktop and starts it again instead
/// of leaving windows hidden; the log says why.
#[test]
fn a_window_manager_failure_hands_over_to_the_watchdog() {
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config("")).unwrap();
    let _watchdog = sandbox.watchdog_with_wm();
    let window = wait_for_wm(&sandbox);
    let act = |code, arg| {
        send_message(
            window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
    };
    let a = open("panic-a", &[]);
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    act(test_action::WORKSPACE, 2).unwrap();
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some());
    // The shell dies while handling it, so the send may report an error.
    let _ = act(test_action::PANIC, 0);
    assert!(
        wait_until(RESTORE_LIMIT, || a.hwnd.is_visible()).is_some(),
        "window not shown after the failure"
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("test-requested window manager failure")
            == 1)
        .is_some()
    );
    // The window manager's own failure handling ran (the panic did not abort the process).
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("window manager failed; ending the shell")
            == 1)
        .is_some()
    );
    assert!(wait_until(SETTLE, || sandbox.log_lines("shell crashed; desktop restored") >= 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
}

/// Regression: at start the window manager inspected Kabuk's own bar windows; reading
/// their title sends them a message, but their thread was waiting for the window manager
/// to start, so the shell hung. Kabuk's own windows are never looked at.
#[test]
fn the_window_manager_never_waits_on_kabuks_own_windows() {
    let before: Vec<Rect> = kabuk_win::shell::monitors().iter().map(|m| m.work_area).collect();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config("")).unwrap();
    // Point the test-mode window manager at the bar's own class.
    let _shell = sandbox.kabuk_with(&["--manage-class", "Kabuk.Bar"]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("window manager running") == 1).is_some(),
        "the shell hung starting the window manager"
    );
    assert_eq!(sandbox.log_lines("managing window"), 0);
    assert_eq!(sandbox.restore().0, 0);
    let restored = || {
        kabuk_win::shell::monitors()
            .iter()
            .map(|m| m.work_area)
            .collect::<Vec<_>>()
            == before
    };
    assert!(wait_until(Duration::from_secs(5), restored).is_some());
}

fn rule(title: &str, action: &str) -> String {
    format!("\n[[rules.rule]]\ntitle = \"{title}\"\n{action}\n")
}

/// Rules send a window to a workspace, float it against an edge, or leave it alone.
#[test]
fn rules_send_float_place_and_ignore_windows() {
    let to3 = title("rule-ws");
    let right = title("rule-right");
    let alone = title("rule-ignore");
    let cfg = format!(
        "{}{}{}{}",
        config(""),
        rule(&to3, "workspace = 3"),
        rule(&right, "float = true\nplacement = \"right\""),
        rule(&alone, "ignore = true")
    );
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &cfg);
    let a = TestWindow::open(&["--title", &to3]);
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible() && hidden_in_state(&sandbox, &a)).is_some(),
        "not sent to workspace 3"
    );

    let b = TestWindow::open(&["--title", &right, "--size", "500x400"]);
    let m = monitor_of(&b);
    let edge = m.work_area.right - gap_on(&m);
    assert!(
        wait_until(SETTLE, || frame(&b).is_some_and(|f| f.right == edge)).is_some(),
        "not placed at the right: {:?} (edge {edge})",
        frame(&b)
    );
    assert!(sandbox
        .last_log_line(&["managing window", &format!("hwnd={}", b.hwnd.0)])
        .unwrap()
        .contains("floating=true"));

    let c = TestWindow::open(&["--title", &alone]);
    let before = frame(&c);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window ignored by a rule") == 1).is_some());
    assert_eq!(sandbox.log_lines(&format!("managing window hwnd={}", c.hwnd.0)), 0);
    assert_eq!(frame(&c), before);
    assert_eq!(sandbox.restore().0, 0);
}

/// DECISIONS D23 (decided by the user): a rule's `no_grab` keeps the window's Alt +
/// clicks with it, and the window is still tiled.
#[test]
fn a_no_grab_rule_reaches_its_window() {
    let mine = title("rule-nograb");
    let other = title("rule-grab");
    let cfg = format!("{}{}", config(""), rule(&mine, "no_grab = true"));
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &cfg);
    let a = TestWindow::open(&["--title", &mine]);
    let b = TestWindow::open(&["--title", &other]);
    let line = |w: &TestWindow| sandbox.last_log_line(&["managing window", &format!("hwnd={}", w.hwnd.0)]);
    assert!(wait_until(SETTLE, || line(&a).is_some() && line(&b).is_some()).is_some());
    assert!(line(&a).unwrap().contains("no_grab=true"));
    assert!(line(&a).unwrap().contains("floating=false"), "still tiled");
    assert!(line(&b).unwrap().contains("no_grab=false"));
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M5: after a rule is added, the matching window follows it the next time it opens.
#[test]
fn a_rule_added_live_applies_to_the_next_window() {
    let t = title("rule-live");
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    let first = TestWindow::open(&["--title", &t]);
    assert!(
        wait_until(SETTLE, || fills(&first)).is_some(),
        "without the rule it tiles"
    );
    first.close();
    std::fs::write(
        sandbox.config_file(),
        format!("{}{}", config(""), rule(&t, "workspace = 2")),
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("window manager configuration applied")
            >= 1)
        .is_some()
    );
    let again = TestWindow::open(&["--title", &t]);
    assert!(
        wait_until(SETTLE, || !again.hwnd.is_visible() && hidden_in_state(&sandbox, &again)).is_some(),
        "rule not followed"
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// How long a window is watched to see that nothing happens to it.
const QUIET: Duration = Duration::from_secs(1);

fn side_by_side(sandbox: &Sandbox, name: &str, extra: &str) -> (Wm, TestWindow, TestWindow) {
    let wm = Wm::start(sandbox, &config(extra));
    let a = open(&format!("{name}-a"), &[]);
    let b = open(&format!("{name}-b"), &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    let apart = || matches!((frame(&a), frame(&b)), (Some(fa), Some(fb)) if !overlaps(&fa, &fb) && fa.left != fb.left);
    assert!(wait_until(SETTLE, apart).is_some(), "{:?} {:?}", frame(&a), frame(&b));
    (wm, a, b)
}

/// TESTING §4: a window moved by its title bar floats; a click without movement does not.
#[test]
fn a_window_dragged_by_its_title_floats_and_a_click_does_not() {
    let sandbox = Sandbox::new();
    let (_wm, a, b) = side_by_side(&sandbox, "drag", "");
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());

    b.fake_drag(0, 0);
    std::thread::sleep(QUIET);
    assert_eq!(
        (frame(&a), frame(&b)),
        (Some(ra), Some(rb)),
        "a click changed the layout"
    );
    assert_eq!(sandbox.log_lines("window floated by a drag"), 0);

    b.fake_drag(-200, 80);
    let dropped = Rect {
        left: rb.left - 200,
        top: rb.top + 80,
        right: rb.right - 200,
        bottom: rb.bottom + 80,
    };
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "the other window did not take the space: {:?}",
        frame(&a)
    );
    assert_eq!(sandbox.log_lines("window floated by a drag"), 1);
    std::thread::sleep(QUIET);
    assert_eq!(frame(&b), Some(dropped), "the dragged window was moved back");
    assert_eq!(sandbox.restore().0, 0);
}

/// With "Drag to float" off a dragged window goes back to its tile.
#[test]
fn with_drag_to_float_off_a_dragged_window_goes_back() {
    let sandbox = Sandbox::new();
    let (_wm, a, b) = side_by_side(&sandbox, "nodrag", "drag_to_float = false\n");
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());
    b.fake_drag(-200, 80);
    assert!(
        wait_until(SETTLE, || frame(&b) == Some(rb)).is_some(),
        "not put back: {:?}",
        frame(&b)
    );
    std::thread::sleep(QUIET);
    assert_eq!((frame(&a), frame(&b)), (Some(ra), Some(rb)));
    assert_eq!(sandbox.log_lines("window floated by a drag"), 0);
    assert_eq!(sandbox.restore().0, 0);
}

/// DECISIONS D27: dragging the edge between two tiled windows resizes both; an edge
/// facing the screen's edge is undone.
#[test]
fn dragging_the_edge_between_tiled_windows_resizes_both() {
    const WIDER: i32 = 300;
    let sandbox = Sandbox::new();
    let (_wm, a, b) = side_by_side(&sandbox, "split", "");
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());
    let (first, second) = if ra.left < rb.left { (&a, &b) } else { (&b, &a) };
    let (r1, r2) = (frame(first).unwrap(), frame(second).unwrap());
    let gap = r2.left - r1.right;

    first.fake_resize(0, WIDER);
    let moved = || {
        matches!((frame(first), frame(second)), (Some(f), Some(s))
            if (f.right - r1.right - WIDER).abs() <= 1 && s.left == f.right + gap && s.right == r2.right)
    };
    assert!(
        wait_until(SETTLE, moved).is_some(),
        "{:?} {:?}",
        frame(first),
        frame(second)
    );
    std::thread::sleep(QUIET);
    assert!(moved(), "{:?} {:?}", frame(first), frame(second));
    let (f, s) = (frame(first).unwrap(), frame(second).unwrap());
    assert_eq!(sandbox.log_lines("window floated by a drag"), 0);

    second.fake_resize(0, -WIDER);
    assert!(
        wait_until(SETTLE, || frame(second) == Some(s)).is_some(),
        "the outer edge was not undone: {:?}",
        frame(second)
    );
    assert_eq!(frame(first), Some(f));
    assert_eq!(sandbox.restore().0, 0);
}

/// SPEC §2.2: a workspace can run entirely floating; switching back tiles its windows.
#[test]
fn a_workspace_switched_to_floating_leaves_windows_and_tiles_them_again() {
    let sandbox = Sandbox::new();
    let (wm, a, b) = side_by_side(&sandbox, "wsfloat", "");
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());
    wm.act(test_action::TOGGLE_FLOATING_LAYOUT, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("workspace layout switched") == 1).is_some());
    assert!(sandbox
        .last_log_line(&["workspace layout switched"])
        .unwrap()
        .contains("floating=true"));
    let c = open("wsfloat-c", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 3).is_some());
    // It floats where it opened, so the others are not reflowed around it.
    std::thread::sleep(QUIET);
    assert_eq!(
        (frame(&a), frame(&b)),
        (Some(ra), Some(rb)),
        "floating windows were moved"
    );

    wm.act(test_action::TOGGLE_FLOATING_LAYOUT, 0);
    let tiled = || {
        let (Some(fa), Some(fb), Some(fc)) = (frame(&a), frame(&b), frame(&c)) else {
            return false;
        };
        !overlaps(&fa, &fb) && !overlaps(&fa, &fc) && !overlaps(&fb, &fc)
    };
    assert!(
        wait_until(SETTLE, tiled).is_some(),
        "{:?} {:?} {:?}",
        frame(&a),
        frame(&b),
        frame(&c)
    );
    assert!(sandbox
        .last_log_line(&["workspace layout switched"])
        .unwrap()
        .contains("floating=false"));
    assert_eq!(sandbox.restore().0, 0);
}

/// The mouse hook runs with the window manager and goes away with it.
#[test]
fn the_mouse_hook_runs_with_the_window_manager() {
    let sandbox = Sandbox::new();
    let _wm = Wm::start(&sandbox, &config(""));
    assert!(wait_until(SETTLE, || sandbox.log_lines("mouse hook installed") == 1).is_some());
    // `--restore` ends the shell cleanly; the hook goes before the window manager stops.
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window manager stopped") == 1).is_some());
    assert_eq!(sandbox.log_lines("mouse hook removed"), 1);
}

/// Alt+drag as the hook reports it: the window follows, floats where it is let go, and a
/// double click on its title tiles it again.
#[test]
fn a_grabbed_window_floats_where_it_is_let_go_and_a_double_click_tiles_it() {
    let sandbox = Sandbox::new();
    let (wm, a, b) = side_by_side(&sandbox, "grab", "drag_to_float = false\n");
    let rb = frame(&b).unwrap();
    wm.act(test_action::GRAB, b.hwnd.0 as isize);
    let dropped = Rect {
        left: rb.left + 120,
        top: rb.top + 80,
        right: rb.right + 120,
        bottom: rb.bottom + 80,
    };
    assert!(
        wait_until(SETTLE, || fills(&a)).is_some(),
        "the other window did not take the space"
    );
    std::thread::sleep(QUIET);
    assert_eq!(frame(&b), Some(dropped));
    assert_eq!(sandbox.log_lines("window let go after a grab"), 1);

    wm.act(test_action::RETILE, b.hwnd.0 as isize);
    assert!(
        wait_until(SETTLE, || frame(&b) == Some(rb) && !fills(&a)).is_some(),
        "not tiled again: {:?}",
        frame(&b)
    );
    assert_eq!(sandbox.log_lines("window tiled by a double click"), 1);
    // A tiled window is not tiled "again".
    wm.act(test_action::RETILE, b.hwnd.0 as isize);
    assert_eq!(sandbox.log_lines("window tiled by a double click"), 1);
    assert_eq!(sandbox.restore().0, 0);
}

/// The hook asks a window whether the second click of a double click is on its title,
/// also a window that is not DPI aware (Windows translates the physical point for it).
#[test]
fn the_title_bar_is_told_apart_from_the_rest_of_a_window() {
    for extra in [&[][..], &["--dpi-unaware"][..]] {
        let w = open("caption", extra);
        // The testapp prints its handle before it shows the window.
        assert!(wait_until(SETTLE, || w.hwnd.is_visible()).is_some());
        // Physical pixels, as Kabuk (DPI aware) sees the screen; this test process is not.
        kabuk_win::dpi::physical(|| {
            let f = frame(&w).unwrap();
            let x = (f.left + f.right) / 2;
            let timeout = Duration::from_secs(1);
            // Below the top resize border and above the client area (a standard title
            // bar, about 30 px at 125 %).
            let on_title = kabuk_core::grab::Point { x, y: f.top + 12 };
            let inside = kabuk_core::grab::Point {
                x,
                y: (f.top + f.bottom) / 2,
            };
            assert!(
                kabuk_win::input::on_caption(w.hwnd, on_title, timeout),
                "{extra:?} {f:?}"
            );
            assert!(!kabuk_win::input::on_caption(w.hwnd, inside, timeout), "{extra:?}");
        });
    }
    // `root_window_at` is not asserted on: another program's window may cover this one.
}

/// M11 (D34): a window sent to the hidden workspace hides into the state file and the
/// other one takes the area; shown, it tiles across the monitor above the workspace,
/// which stays; hidden again, it goes; `--restore` brings it back.
#[test]
fn the_hidden_workspace_takes_a_window_and_shows_it_over_the_workspace() {
    let sandbox = Sandbox::new();
    let (wm, a, b) = side_by_side(&sandbox, "scratch", "");
    wm.act(test_action::FOCUSED, a.hwnd.0 as isize);
    wm.act(test_action::TO_SCRATCHPAD, 0);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(), "not hidden");
    assert!(hidden_in_state(&sandbox, &a));
    assert!(wait_until(SETTLE, || fills(&b)).is_some(), "{:?}", frame(&b));
    wm.act(test_action::TOGGLE_SCRATCHPAD, 0);
    assert!(
        wait_until(SETTLE, || a.hwnd.is_visible() && fills(&a)).is_some(),
        "{:?}",
        frame(&a)
    );
    assert!(b.hwnd.is_visible(), "the workspace went from under it");
    assert!(!hidden_in_state(&sandbox, &a));
    wm.act(test_action::TOGGLE_SCRATCHPAD, 0);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(), "still shown");
    assert!(hidden_in_state(&sandbox, &a) && b.hwnd.is_visible());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some());
}

/// M11 (D34): a window that opens while a group has the focus joins it; one member shows
/// under a strip of tabs drawn by Kabuk, the other hides in its place (recorded); the next
/// tab and a click on a tab switch them; breaking the group up tiles both and the strip
/// goes.
#[test]
fn a_tabbed_group_shows_one_member_under_its_strip() {
    let before: Vec<Rect> = kabuk_win::shell::monitors().iter().map(|m| m.work_area).collect();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config("")).unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    let window = wait_for_wm(&sandbox);
    let act = |code, arg| {
        send_message(
            window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .unwrap()
    };
    let a = open("tabs-a", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 1).is_some());
    assert!(wait_until(SETTLE, || fills(&a)).is_some());
    act(test_action::FOCUSED, a.hwnd.0 as isize);
    act(test_action::TOGGLE_GROUP, 0);
    let pid = sandbox.state().kabuk_pid.unwrap();
    let strips = || {
        kabuk_win::window::windows_of_class("Kabuk.Tabs")
            .into_iter()
            .filter(|w| w.pid() == Some(pid) && w.is_visible())
            .collect::<Vec<_>>()
    };
    assert!(wait_until(SETTLE, || strips().len() == 1).is_some(), "no strip");
    // The member sits right under its strip, inside the tile.
    let under = || {
        let (Some(s), Some(f)) = (
            strips()
                .first()
                .and_then(|s| kabuk_win::dpi::physical(|| s.window_rect())),
            frame(&a),
        ) else {
            return false;
        };
        s.bottom == f.top && s.left == f.left && s.right == f.right
    };
    assert!(
        wait_until(SETTLE, under).is_some(),
        "{:?} {:?}",
        strips().first().and_then(|s| s.window_rect()),
        frame(&a)
    );
    let place = frame(&a);
    let b = open("tabs-b", &[]);
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible() && b.hwnd.is_visible()).is_some(),
        "b did not join"
    );
    assert!(hidden_in_state(&sandbox, &a));
    // In a's place, under the strip.
    assert!(
        wait_until(SETTLE, || frame(&b) == place).is_some(),
        "{:?} {:?}",
        frame(&b),
        place
    );
    act(test_action::NEXT_TAB, 0);
    assert!(
        wait_until(SETTLE, || a.hwnd.is_visible() && !b.hwnd.is_visible()).is_some(),
        "next tab"
    );
    // A click on the second tab shows b again.
    let strip = strips()[0];
    let r = kabuk_win::dpi::physical(|| strip.window_rect()).unwrap();
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    kabuk_win::dpi::physical(|| {
        kabuk_win::msgloop::post_message(
            strip,
            Msg {
                id: kabuk_win::msgloop::msg::LBUTTONUP,
                wparam: 0,
                lparam: (((h / 2) << 16) | (w * 3 / 4)) as isize,
            },
        )
        .unwrap()
    });
    assert!(
        wait_until(SETTLE, || b.hwnd.is_visible() && !a.hwnd.is_visible()).is_some(),
        "tab click"
    );
    act(test_action::FOCUSED, b.hwnd.0 as isize);
    act(test_action::TOGGLE_GROUP, 0);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible() && b.hwnd.is_visible()).is_some());
    assert!(wait_until(SETTLE, || strips().is_empty()).is_some(), "the strip stayed");
    assert!(!hidden_in_state(&sandbox, &a));
    assert_eq!(sandbox.restore().0, 0);
    let restored = || {
        kabuk_win::shell::monitors()
            .iter()
            .map(|m| m.work_area)
            .collect::<Vec<_>>()
            == before
    };
    assert!(wait_until(Duration::from_secs(5), restored).is_some());
}

/// M10 (D35): swapping two tiled windows plays the move on Kabuk's overlay (live
/// thumbnails over the work area) while the windows jump to their places at once; the
/// overlay goes after the move, nothing is dropped by the compositor, and the windows end
/// exactly swapped.
#[test]
fn a_layout_change_plays_the_move_and_the_overlay_goes() {
    let sandbox = Sandbox::new();
    let (wm, a, b) = side_by_side(&sandbox, "move", "");
    let (ra, rb) = (frame(&a).unwrap(), frame(&b).unwrap());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let overlays = || {
        kabuk_win::window::windows_of_class(kabuk_win::window_motion::OVERLAY_CLASS)
            .into_iter()
            .filter(|w| w.pid() == Some(pid) && w.is_visible())
            .count()
    };
    let animates = kabuk_win::system::animations_on();
    // The strip between the work area's top and the windows shows the wallpaper.
    let m = monitor_of(&a);
    let strip = kabuk_win::msgloop::PxRect {
        x: ra.left,
        y: m.work_area.top,
        w: rb.right - ra.left,
        h: (ra.top - m.work_area.top).max(1),
    };
    let look = || kabuk_win::dpi::physical(|| kabuk_win::screen::capture(strip)).expect("screen");
    let wallpaper = look();
    let frames = kabuk_win::screen::Frames::now();
    // Opening the two windows moved them already.
    let moves = sandbox.log_lines("windows move");
    wm.act(test_action::FOCUSED, a.hwnd.0 as isize);
    // Swap a with its right-hand neighbour.
    wm.act(test_action::MOVE, 3);
    if animates {
        // Over the overlay the desktop is still there (its live thumbnail), not black.
        assert!(wait_until(SETTLE, || overlays() > 0).is_some(), "no overlay");
        let during = look();
        let black = kabuk_win::screen::difference(&vec![0u8; wallpaper.len()], &wallpaper);
        let off = kabuk_win::screen::difference(&during, &wallpaper);
        assert!(
            off < black / 2.0 || black < 10.0,
            "the wallpaper went: {off} (black would be {black})"
        );
        assert!(wait_until(SETTLE, || sandbox.log_lines("windows move") > moves).is_some());
        let line = sandbox.last_log_line(&["windows move"]).unwrap();
        assert!(line.contains("moving=2"), "{line}");
    }
    assert!(
        wait_until(SETTLE, || frame(&a) == Some(rb) && frame(&b) == Some(ra)).is_some(),
        "{:?} {:?}",
        frame(&a),
        frame(&b)
    );
    // The overlay is gone soon after the move (200 ms).
    assert!(
        wait_until(Duration::from_secs(1), || overlays() == 0).is_some(),
        "the overlay stayed"
    );
    let f = frames.since(kabuk_win::screen::Frames::now());
    assert!(f.missed == 0 && f.dropped == 0, "{f:?}");
    assert_eq!(sandbox.restore().0, 0);
}

/// M11 (D34): the overview covers each monitor with a cell per workspace, the one on
/// screen with live thumbnails; a click on another cell goes to that workspace and closes
/// it.
#[test]
fn the_overview_shows_every_workspace_and_a_click_goes_there() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_OVERVIEW};
    let before: Vec<Rect> = kabuk_win::shell::monitors().iter().map(|m| m.work_area).collect();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), config("")).unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    let window = wait_for_wm(&sandbox);
    let act = |code, arg| {
        send_message(
            window,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .unwrap()
    };
    let a = open("overview-a", &[]);
    let b = open("overview-b", &[]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("managing window") == 2).is_some());
    act(test_action::FOCUSED, b.hwnd.0 as isize);
    act(test_action::MOVE_TO_WORKSPACE, 2);
    assert!(wait_until(SETTLE, || !b.hwnd.is_visible()).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |open: bool| {
        kabuk_win::msgloop::post_message(
            control,
            Msg {
                id: MSG_TEST_OVERVIEW,
                wparam: usize::from(open),
                lparam: 0,
            },
        )
        .unwrap()
    };
    let sheets = || {
        kabuk_win::window::windows_of_class("Kabuk.Overview")
            .into_iter()
            .filter(|w| w.pid() == Some(pid) && w.is_visible())
            .collect::<Vec<_>>()
    };
    post(true);
    let monitors = kabuk_win::shell::monitors();
    assert!(
        wait_until(SETTLE, || sheets().len() == monitors.len()).is_some(),
        "one sheet per monitor"
    );
    let m = monitor_of(&a);
    let line = sandbox
        .last_log_line(&["overview shown", &m.device])
        .expect("sheet logged");
    // The workspaces the bar shows: 4 by default, workspace 2 among them (D34).
    assert!(line.contains("cells=4") && line.contains("thumbnails=1"), "{line}");
    // Asked for by the user: with full motion the window on screen flies into its cell.
    if kabuk_win::system::animations_on() {
        assert_eq!(sandbox.log_lines("overview windows fly in"), 1);
    }
    // The cell of workspace 2 on a's monitor.
    let sheet = sheets()
        .into_iter()
        .find(|s| {
            kabuk_win::dpi::physical(|| s.window_rect())
                .is_some_and(|r| r.left == m.work_area.left && r.top == m.work_area.top)
        })
        .expect("the sheet of a's monitor");
    let area = kabuk_layout::Rect {
        x: 0,
        y: 0,
        w: m.work_area.right - m.work_area.left,
        h: m.work_area.bottom - m.work_area.top,
    };
    let gap = (kabuk::overview::view::STYLE.gap * m.dpi as f32 / 96.0).round() as i32;
    let cell = kabuk_layout::overview::cells(4, area, gap)[1];
    kabuk_win::msgloop::post_message(
        sheet,
        Msg {
            id: kabuk_win::msgloop::msg::LBUTTONUP,
            wparam: 0,
            lparam: (((cell.y + cell.h / 2) << 16) | (cell.x + cell.w / 2)) as isize,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || b.hwnd.is_visible() && !a.hwnd.is_visible()).is_some(),
        "not on workspace 2"
    );
    assert!(
        wait_until(SETTLE, || sheets().is_empty()).is_some(),
        "the overview stayed"
    );
    // Open again: a click on the workspace on screen (now 2) closes it after its window
    // flew back, and stays there.
    post(true);
    assert!(wait_until(SETTLE, || sheets().len() == monitors.len()).is_some());
    let sheet = sheets()
        .into_iter()
        .find(|s| {
            kabuk_win::dpi::physical(|| s.window_rect())
                .is_some_and(|r| r.left == m.work_area.left && r.top == m.work_area.top)
        })
        .expect("the sheet again");
    kabuk_win::msgloop::post_message(
        sheet,
        Msg {
            id: kabuk_win::msgloop::msg::LBUTTONUP,
            wparam: 0,
            lparam: (((cell.y + cell.h / 2) << 16) | (cell.x + cell.w / 2)) as isize,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sheets().is_empty()).is_some(),
        "the overview stayed after its windows flew back"
    );
    assert!(b.hwnd.is_visible() && !a.hwnd.is_visible(), "the workspace changed");
    assert_eq!(sandbox.restore().0, 0);
    let restored = || {
        kabuk_win::shell::monitors()
            .iter()
            .map(|m| m.work_area)
            .collect::<Vec<_>>()
            == before
    };
    assert!(wait_until(Duration::from_secs(5), restored).is_some());
}
