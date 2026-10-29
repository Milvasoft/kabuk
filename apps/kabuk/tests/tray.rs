//! The tray and the hidden taskbar on the real desktop (PLAN M4). These tests hide the
//! user's taskbar and take the tray icons for a few seconds (standing approval in
//! CONTRIBUTING.md); each one ends with `kabuk --restore` and checks that the taskbar, its
//! auto-hide setting and the work areas are back.
//!
//! Edge cases: a program's icon appearing on the bar, a right click reaching the program
//! with the version-4 message layout, the icon's program quitting, a forced kill with the
//! taskbar hidden (the watchdog shows it within 2 s and puts auto-hide back), a program
//! asking where its icon is (`Shell_NotifyIconGetRect`) and getting its place on the bar.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::time::Duration;

use common::*;
use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_TRAY_CLICK};
use kabuk_core::state::Rect;
use kabuk_win::msgloop::{post_message, Msg};
use kabuk_win::process::Process;

/// SPEC §8.1: everything is back within 2 seconds of a crash.
const RESTORE_LIMIT: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_secs(5);
/// The test icon's id in `kabuk-testapp`.
const TEST_ICON_ID: isize = 1;
const WM_CONTEXTMENU: isize = 0x007B;
const WM_RBUTTONUP: isize = 0x0205;
const NIN_SELECT: isize = 0x0400;

struct Desktop {
    work_areas: Vec<Rect>,
    autohide: bool,
}

impl Desktop {
    fn now() -> Desktop {
        Desktop {
            work_areas: kabuk_win::shell::monitors().iter().map(|m| m.work_area).collect(),
            autohide: kabuk_win::shell::taskbar_autohide(),
        }
    }

    fn is_back(&self) -> bool {
        let now = Desktop::now();
        now.work_areas == self.work_areas && now.autohide == self.autohide && taskbars_visible()
    }
}

/// The tray calls the shell saw from one program's window, for failures.
fn calls_of(sandbox: &Sandbox, hwnd: u64) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(sandbox.dir.path().join("logs")).unwrap().flatten() {
        for l in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
            if (l.contains("tray call") && l.contains(&format!("hwnd={hwnd}"))) || l.contains("behind") {
                out.push_str(l.get(11..).unwrap_or(l));
                out.push('\n');
            }
        }
    }
    out
}

/// The shell's log lines about the tray, the taskbar, Explorer and the bars, for failures.
fn story(sandbox: &Sandbox) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(sandbox.dir.path().join("logs")).unwrap().flatten() {
        for l in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
            let wanted = ["tray", "taskbar", "Explorer", "bars built", "TaskbarCreated"]
                .iter()
                .any(|n| l.contains(n));
            if (wanted || l.contains("bar click")) && !l.contains("tray call") {
                out.push_str(l.get(11..).unwrap_or(l));
                out.push('\n');
            }
        }
    }
    out
}

/// Shown, and opaque again (D77).
fn taskbars_visible() -> bool {
    let bars = kabuk_win::shell::taskbars();
    !bars.is_empty() && bars.iter().all(|b| b.is_visible() && !b.is_see_through())
}

/// Hidden, and see-through for the moments Explorer shows them anyway (found by the
/// user: they flashed at every work area change; D77).
fn taskbar_states() -> Vec<(u64, bool, bool)> {
    kabuk_win::shell::taskbars()
        .iter()
        .map(|b| (b.0, b.is_visible(), b.is_see_through()))
        .collect()
}

fn taskbars_hidden() -> bool {
    kabuk_win::shell::taskbars()
        .iter()
        .all(|b| !b.is_visible() && b.is_see_through())
}

#[test]
fn a_programs_tray_icon_appears_on_the_bar_and_clicks_reach_it() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    assert!(
        wait_until(SETTLE, taskbars_hidden).is_some(),
        "taskbar not hidden: {:?}",
        taskbar_states()
    );

    let log = sandbox.dir.path().join("tray-clicks.txt");
    let tip = title("tray-icon");
    let app = TestWindow::open(&["--title", &tip, "--tray-log", log.to_str().unwrap()]);
    assert!(
        wait_until(SETTLE, || sandbox.last_log_line(&["tray call", &tip]).is_some()).is_some(),
        "the bar did not receive the icon:
{}",
        story(&sandbox)
    );
    // Drawn on the primary bar (the debug line lists the pictures drawn).
    let primary = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .device;
    let primary_line = || sandbox.last_log_line(&["bar drawn", &primary]);
    assert!(wait_until(SETTLE, || primary_line().is_some_and(|l| !l.contains("images=[]"))).is_some());

    // Right-click our icon (picked by its window, so no other program's icon is clicked).
    let control = kabuk_win::window::find_message_window(CONTROL_CLASS, "").expect("bar control window");
    // Posted, as a real click is handled: never inside a nested call.
    post_message(
        control,
        Msg {
            id: MSG_TEST_TRAY_CLICK,
            wparam: app.hwnd.0 as usize,
            lparam: 1,
        },
    )
    .unwrap();
    let got = || std::fs::read_to_string(&log).unwrap_or_default();
    let menu = (TEST_ICON_ID << 16) | WM_CONTEXTMENU;
    assert!(
        wait_until(SETTLE, || got().lines().any(|l| l.ends_with(&format!(" {menu}")))).is_some(),
        "no WM_CONTEXTMENU: {:?}\n{}",
        got(),
        calls_of(&sandbox, app.hwnd.0)
    );
    assert!(got()
        .lines()
        .any(|l| l.ends_with(&format!(" {}", (TEST_ICON_ID << 16) | WM_RBUTTONUP))));

    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// PLAN M19 B2, found on the user's machine: Windows Security, whose icon Explorer held
/// before Kabuk took the tray, answers the call to register with a modify, which Kabuk
/// ignored for an icon it did not hold, so the icon never reached the bar.
#[test]
fn an_icon_explorer_held_before_and_only_modified_comes_to_the_bar() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let log = sandbox.dir.path().join("tray-clicks.txt");
    let tip = title("tray-modified");
    // Its icon is Explorer's before the shell starts.
    let app = TestWindow::open(&[
        "--title",
        &tip,
        "--tray-log",
        log.to_str().unwrap(),
        "--tray-modify-again",
    ]);
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["tray call", "op=Modify", &tip, "explorer_took=true"])
            .is_some())
        .is_some(),
        "no modify Explorer took:\n{}",
        story(&sandbox)
    );
    let held = || {
        let out = std::process::Command::new(kabuk_exe())
            .arg("--state-dir")
            .arg(sandbox.state_dir())
            .arg("--config")
            .arg(sandbox.config_file())
            .args(["msg", "query", "tray"])
            .output()
            .expect("run kabuk msg");
        let icons: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        icons
            .as_array()
            .and_then(|a| a.iter().find(|i| i["tip"] == tip.as_str()).cloned())
    };
    let mut ours = None;
    wait_until(SETTLE, || {
        ours = held();
        ours.is_some()
    });
    let ours = ours.unwrap_or_else(|| panic!("our icon not held:\n{}", story(&sandbox)));
    assert_eq!(
        (ours["hwnd"].as_u64(), ours["shown"].as_bool()),
        (Some(app.hwnd.0), Some(true)),
        "{ours}"
    );

    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

#[test]
fn a_forced_kill_brings_the_taskbar_back_within_two_seconds() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _watchdog = sandbox.watchdog_taking_explorer();
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("Windows taskbar hidden") == 1).is_some());
    assert!(
        wait_until(SETTLE, taskbars_hidden).is_some(),
        "taskbar not hidden: {:?}",
        taskbar_states()
    );
    assert!(sandbox.state().taskbar.is_some(), "taskbar not recorded");

    let pid = sandbox.state().kabuk_pid.unwrap();
    Process::open(pid).unwrap().terminate(1).unwrap();
    assert!(
        wait_until(RESTORE_LIMIT, taskbars_visible).is_some(),
        "taskbar not back within {RESTORE_LIMIT:?}\n{}",
        story(&sandbox)
    );
    // The watchdog restarts the shell, which hides it again; restore ends that too.
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// Starts the shell taking the tray and the taskbar, and tells it Explorer ended (posted,
/// as the Explorer watch posts it; the watch itself is tested in `kabuk-win` and a real
/// Explorer restart is tried by hand).
fn shell_after_explorer_ended(sandbox: &Sandbox) -> Proc {
    use kabuk::bar::manager::MSG_TEST_EXPLORER_GONE;
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("Windows taskbar hidden") == 1).is_some());
    // The tray comes a moment after the taskbar is hidden.
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let control = kabuk_win::window::find_message_window(CONTROL_CLASS, "").expect("bar control window");
    post_message(
        control,
        Msg {
            id: MSG_TEST_EXPLORER_GONE,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("Explorer ended") == 1).is_some());
    shell
}

fn taken_again(sandbox: &Sandbox, within: Duration) {
    assert!(
        wait_until(within, || sandbox.log_lines("Explorer back") == 1).is_some(),
        "{}",
        story(sandbox)
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray window up") == 2).is_some(),
        "{}",
        story(sandbox)
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("Windows taskbar hidden") == 2).is_some(),
        "{}",
        story(sandbox)
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("bars built") == 2).is_some(),
        "{}",
        story(sandbox)
    );
    // Hidden asynchronously, and Explorer may show it once more while it settles; the
    // guard hides it again.
    assert!(
        wait_until(SETTLE, taskbars_hidden).is_some(),
        "the taskbar stayed visible\n{}",
        story(sandbox)
    );
}

/// After Explorer ends, the shell gives its tray window up (a new Explorer that finds one
/// does not start properly) and waits until the new Explorer announces its taskbar; then
/// it takes the tray again, hides the taskbar and rebuilds the bars.
#[test]
fn the_shell_takes_the_tray_and_the_taskbar_again_once_explorer_is_ready() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    let _shell = shell_after_explorer_ended(&sandbox);
    // Its tray window is gone while it waits.
    assert!(wait_until(SETTLE, || sandbox.log_lines("tray window removed") == 1).is_some());
    assert_eq!(sandbox.log_lines("Explorer back"), 0);
    // The new Explorer announces its taskbar.
    let pid = sandbox.state().kabuk_pid.unwrap();
    let message = kabuk_win::system::register_message(kabuk_win::system::TASKBAR_CREATED);
    for bar in kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
    {
        post_message(
            bar,
            Msg {
                id: message,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
    }
    taken_again(&sandbox, SETTLE);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// If the new Explorer never announces its taskbar, the shell takes over after a while.
#[test]
fn without_an_announcement_the_shell_takes_over_after_the_fallback() {
    use kabuk::bar::manager::EXPLORER_FALLBACK;
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    let _shell = shell_after_explorer_ended(&sandbox);
    taken_again(&sandbox, EXPLORER_FALLBACK + SETTLE);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// Hovering a tray icon shows its tooltip after a moment and the mouse leaving the bar
/// hides it; a double click reaches the program as WM_LBUTTONDBLCLK. The double click is a
/// real mouse message at the icon's place on the bar (the bar's own hit test); the hover
/// starts from a test message, because the real cursor is not moved.
/// Found by the user: with a program's tray menu open (Chrome's, a submenu being chosen),
/// Kabuk found its tray behind Explorer's and asked every program to register again;
/// putting its icon back, Chrome closed the menu. While a menu is open the asking waits,
/// and comes once it has closed.
#[test]
fn an_open_tray_menu_is_not_closed_by_asking_programs_again() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TRAY_BEHIND};
    use kabuk_win::msgloop::{post_message, Msg};
    /// The test app's own messages: open and close its popup menu.
    const OPEN_MENU: u32 = 0x8000 + 4;
    const CLOSE_MENU: u32 = 0x8000 + 5;
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let app = TestWindow::open(&["--title", &title("tray-menu")]);
    let post = |to: kabuk_win::window::Hwnd, id: u32| {
        post_message(
            to,
            Msg {
                id,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
    };
    post(app.hwnd, OPEN_MENU);
    assert!(
        wait_until(SETTLE, kabuk_win::window::popup_menu_open).is_some(),
        "the menu did not open"
    );
    let behind = || sandbox.log_lines("tray was behind Explorer's");
    let waited = || sandbox.log_lines("a menu is open, programs asked later");
    let asked = sandbox.log_lines("programs asked to register again");
    post(control, MSG_TRAY_BEHIND);
    assert!(
        wait_until(SETTLE, || waited() >= 1).is_some(),
        "the asking did not wait (menu open: {}):\n{}",
        kabuk_win::window::popup_menu_open(),
        story(&sandbox)
    );
    assert_eq!(
        sandbox.log_lines("programs asked to register again"),
        asked,
        "asked while the menu was open"
    );
    assert!(kabuk_win::window::popup_menu_open(), "the menu was closed");
    // Closed: the asking comes (or is skipped as too soon after the last one).
    let seen = behind();
    post(app.hwnd, CLOSE_MENU);
    assert!(wait_until(SETTLE, || !kabuk_win::window::popup_menu_open()).is_some());
    assert!(
        wait_until(SETTLE, || behind() > seen
            && sandbox
                .last_log_line(&["tray was behind"])
                .is_some_and(|l| !l.contains("a menu is open")))
        .is_some(),
        "not asked once the menu closed"
    );
    drop(app);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

#[test]
fn a_tray_icon_shows_its_tooltip_and_takes_double_clicks() {
    use kabuk::bar::manager::{MSG_TEST_TRAY_HOVER, MSG_TEST_TRAY_WHERE};
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const WM_LBUTTONDBLCLK: u32 = 0x0203;
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    // The test's icon comes after the user's own ones; keep it on the bar (D26).
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\ntray_shown = 12\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let log = sandbox.dir.path().join("tray-clicks.txt");
    let tip = title("tooltip");
    let app = TestWindow::open(&["--title", &tip, "--tray-log", log.to_str().unwrap()]);
    let control = kabuk_win::window::find_message_window(CONTROL_CLASS, "").expect("bar control window");
    // Where our icon is now (icons move along when other programs re-add theirs).
    let where_now = || {
        let mut point = 0isize;
        wait_until(SETTLE, || {
            point = kabuk_win::msgloop::send_message(
                control,
                Msg {
                    id: MSG_TEST_TRAY_WHERE,
                    wparam: app.hwnd.0 as usize,
                    lparam: 0,
                },
                Duration::from_secs(1),
            )
            .unwrap_or(0);
            point != 0
        })
        .expect("our icon on the bar");
        point
    };
    where_now();
    let pid = sandbox.state().kabuk_pid.unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let bar = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .find(|w| w.window_rect().is_some_and(|r| primary.rect.contains(&r)))
        .expect("primary bar");
    // Physical pixels: from a DPI-unaware test process Windows would scale the point.
    let at = |id| {
        kabuk_win::dpi::physical(|| {
            post_message(
                bar,
                Msg {
                    id,
                    wparam: 0,
                    lparam: where_now(),
                },
            )
            .unwrap()
        })
    };

    post_message(
        control,
        Msg {
            id: MSG_TEST_TRAY_HOVER,
            wparam: app.hwnd.0 as usize,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.last_log_line(&["tooltip shown", &tip]).is_some()).is_some(),
        "{}",
        story(&sandbox)
    );
    post_message(
        bar,
        Msg {
            id: WM_MOUSELEAVE,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("tray tooltip hidden") == 1).is_some());

    at(WM_LBUTTONDBLCLK);
    let got = || std::fs::read_to_string(&log).unwrap_or_default();
    let dbl = (TEST_ICON_ID << 16) | WM_LBUTTONDBLCLK as isize;
    assert!(
        wait_until(SETTLE, || got().lines().any(|l| l.ends_with(&format!(" {dbl}")))).is_some(),
        "no WM_LBUTTONDBLCLK: {:?}\n{}",
        got(),
        story(&sandbox)
    );

    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// Found by the user: the Bluetooth icon's menu opened at the screen's left edge. A
/// program that asks where its icon is (`Shell_NotifyIconGetRect`, to open its menu by
/// it) must get the icon's place on Kabuk's bar, not one on Explorer's hidden taskbar.
#[test]
fn a_program_asking_where_its_icon_is_gets_its_place_on_the_bar() {
    use kabuk::bar::manager::MSG_TEST_TRAY_WHERE;
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    // The test's icon comes after the user's own ones; keep it on the bar (D26).
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\ntray_shown = 12\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let log = sandbox.dir.path().join("tray-clicks.txt");
    let tip = title("where");
    let app = TestWindow::open(&["--title", &tip, "--tray-log", log.to_str().unwrap()]);
    let control = kabuk_win::window::find_message_window(CONTROL_CLASS, "").expect("bar control window");
    let point = || {
        kabuk_win::msgloop::send_message(
            control,
            Msg {
                id: MSG_TEST_TRAY_WHERE,
                wparam: app.hwnd.0 as usize,
                lparam: 0,
            },
            Duration::from_secs(1),
        )
        .unwrap_or(0)
    };
    assert!(
        wait_until(SETTLE, || point() != 0).is_some(),
        "our icon is not on the bar:\n{}",
        story(&sandbox)
    );
    // The icon's centre on screen: its place on the primary bar plus the bar's corner.
    let pid = sandbox.state().kabuk_pid.unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let bar = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .find_map(|w| w.window_rect().filter(|r| primary.rect.contains(r)))
        .expect("primary bar");
    let centre = || {
        let p = point();
        (bar.left + (p & 0xFFFF) as i16 as i32, bar.top + (p >> 16) as i16 as i32)
    };

    // `query tray` tells the icon apart by its program (asked for when a second volume
    // icon showed up on the bar).
    let out = std::process::Command::new(kabuk_exe())
        .arg("--state-dir")
        .arg(sandbox.state_dir())
        .arg("--config")
        .arg(sandbox.config_file())
        .args(["msg", "query", "tray"])
        .output()
        .expect("run kabuk msg");
    let icons: serde_json::Value = serde_json::from_slice(&out.stdout).expect("query tray answers JSON");
    let ours = icons
        .as_array()
        .and_then(|a| a.iter().find(|i| i["tip"] == tip.as_str()))
        .unwrap_or_else(|| panic!("our icon not in `query tray`: {icons}"));
    assert_eq!(ours["exe"], "kabuk-testapp.exe", "{ours}");
    assert_eq!(
        (ours["hwnd"].as_u64(), ours["shown"].as_bool()),
        (Some(app.hwnd.0), Some(true)),
        "{ours}"
    );

    // A right click: the program asks where its icon is after the menu message.
    post_message(
        control,
        Msg {
            id: MSG_TEST_TRAY_CLICK,
            wparam: app.hwnd.0 as usize,
            lparam: 1,
        },
    )
    .unwrap();
    let got = || std::fs::read_to_string(&log).unwrap_or_default();
    let rect = || {
        got().lines().find_map(|l| {
            let v: Vec<i32> = l
                .strip_prefix("rect ")?
                .split(' ')
                .filter_map(|n| n.parse().ok())
                .collect();
            (v.len() == 4).then(|| (v[0], v[1], v[2], v[3]))
        })
    };
    assert!(
        wait_until(SETTLE, || rect().is_some()).is_some(),
        "the program got no place: {:?}\n{}",
        got(),
        story(&sandbox)
    );
    let (l, t, r, b) = rect().unwrap();
    // Measured after the click: the figures coming after the first frames move the tray
    // (a centre measured before was one icon off).
    let centre = centre();
    assert!(
        l <= centre.0 && centre.0 <= r && t <= centre.1 && centre.1 <= b,
        "place {:?} does not hold the icon's centre {centre:?} (bar at {bar:?})",
        (l, t, r, b)
    );
    assert!(r > l && b > t, "an empty place: {:?}", (l, t, r, b));
    // Found by the user: the second answer was read as the icon's size and added to the
    // first, so the place reached past the screen and the Bluetooth menu opened far away.
    // The place is the icon itself: on the bar, no larger than the bar is high.
    let high = bar.bottom - bar.top;
    assert!(
        bar.left <= l && r <= bar.right && bar.top <= t && b <= bar.bottom,
        "place {:?} is not on the bar {bar:?}",
        (l, t, r, b)
    );
    assert!(
        r - l <= high && b - t <= high,
        "place {:?} is larger than an icon on a bar {high} px high",
        (l, t, r, b)
    );

    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// Found by the user: the Bluetooth icon (Explorer's own) asks Explorer's hidden taskbar
/// where it is, gets (0, 0), and opened its menu at the screen's corner. A program whose
/// menu opens far from the click on its icon gets it moved there: under the bar, its
/// right edge at the click.
#[test]
fn a_tray_menu_opened_at_the_screens_corner_moves_beside_its_icon() {
    use kabuk::bar::manager::MSG_TEST_TRAY_WHERE;
    /// The test app's own message: close its popup menu.
    const CLOSE_MENU: u32 = 0x8000 + 5;
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\ntray_shown = 12\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let log = sandbox.dir.path().join("tray-clicks.txt");
    let app = TestWindow::open(&[
        "--title",
        &title("corner-menu"),
        "--tray-log",
        log.to_str().unwrap(),
        "--tray-menu-at-corner",
    ]);
    let control = kabuk_win::window::find_message_window(CONTROL_CLASS, "").expect("bar control window");
    let mut point = 0isize;
    assert!(
        wait_until(SETTLE, || {
            point = kabuk_win::msgloop::send_message(
                control,
                Msg {
                    id: MSG_TEST_TRAY_WHERE,
                    wparam: app.hwnd.0 as usize,
                    lparam: 0,
                },
                Duration::from_secs(1),
            )
            .unwrap_or(0);
            point != 0
        })
        .is_some(),
        "our icon is not on the bar:\n{}",
        story(&sandbox)
    );
    let pid = sandbox.state().kabuk_pid.unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let bar = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .find_map(|w| w.window_rect().filter(|r| primary.rect.contains(r)))
        .expect("primary bar");
    let centre_x = bar.left + (point & 0xFFFF) as i16 as i32;

    post_message(
        control,
        Msg {
            id: MSG_TEST_TRAY_CLICK,
            wparam: app.hwnd.0 as usize,
            lparam: 0,
        },
    )
    .unwrap();
    let app_pid = app.hwnd.pid();
    let menu = || {
        kabuk_win::window::windows_of_class("#32768")
            .into_iter()
            .filter(|m| m.is_visible() && m.pid() == app_pid)
            .find_map(kabuk_win::window::Hwnd::window_rect)
    };
    assert!(
        wait_until(SETTLE, || menu().is_some()).is_some(),
        "the program's menu did not open"
    );
    let work = primary.work_area;
    let moved = || menu().is_some_and(|r| r.top >= work.top);
    assert!(
        wait_until(SETTLE, moved).is_some(),
        "the menu stayed at {:?}, far from its icon at x {centre_x}:\n{}",
        menu(),
        story(&sandbox)
    );
    let r = menu().expect("the menu closed by itself");
    assert_eq!(
        (r.right, r.top),
        (centre_x, work.top),
        "not hanging from the click under the bar"
    );
    assert_eq!(sandbox.log_lines("tray menu moved beside its icon"), 1);

    post_message(
        app.hwnd,
        Msg {
            id: CLOSE_MENU,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || menu().is_none()).is_some(),
        "the menu did not close"
    );
    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// DECISIONS D26 (asked for by the user): with `tray_shown = 0` every icon is in the
/// overflow panel, a click there reaches the program, and raising `tray_shown` in the
/// file brings icons back to the bar.
#[test]
fn icons_beyond_tray_shown_are_clicked_from_the_overflow_panel() {
    use kabuk::bar::manager::{MSG_TEST_FOREGROUND, MSG_TEST_TRAY_PANEL, MSG_TEST_TRAY_PANEL_CLICK};
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\ntray_shown = 0\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let log = sandbox.dir.path().join("tray-clicks.txt");
    let tip = title("tray-panel");
    let app = TestWindow::open(&["--title", &tip, "--tray-log", log.to_str().unwrap()]);
    assert!(wait_until(SETTLE, || sandbox.last_log_line(&["tray call", &tip]).is_some()).is_some());
    let primary = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .device;
    let primary_line = || sandbox.last_log_line(&["bar drawn", &primary]).unwrap_or_default();
    // Tray pictures only: the apps module's have the top bit set (`bar::apps::image_key`).
    let tray_drawn = || {
        let line = primary_line();
        let list = line
            .split("images=[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .unwrap_or_default()
            .to_string();
        list.split(',')
            .filter_map(|k| k.trim().parse::<u64>().ok())
            .filter(|k| *k < 1 << 63)
            .count()
    };
    // No icon on the bar, only the overflow button with the count.
    assert!(
        wait_until(SETTLE, || tray_drawn() == 0 && primary_line().contains('+')).is_some(),
        "{}",
        primary_line()
    );

    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|w| w.pid() == sandbox.state().kabuk_pid)
        .expect("bar control window");
    let post = |id: u32, wparam: usize, lparam: isize| post_message(control, Msg { id, wparam, lparam }).unwrap();
    post(MSG_TEST_TRAY_PANEL, 1, 0);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray panel opened") >= 1).is_some(),
        "{}",
        story(&sandbox)
    );
    post(MSG_TEST_TRAY_PANEL_CLICK, app.hwnd.0 as usize, 1);
    let got = || std::fs::read_to_string(&log).unwrap_or_default();
    let menu = (TEST_ICON_ID << 16) | WM_CONTEXTMENU;
    assert!(
        wait_until(SETTLE, || got().lines().any(|l| l.ends_with(&format!(" {menu}")))).is_some(),
        "no WM_CONTEXTMENU from the panel: {:?}\n{}",
        got(),
        calls_of(&sandbox, app.hwnd.0)
    );
    // Asked for by the user: a right click keeps the panel while the program's menu is up,
    // the program itself in front included.
    post(MSG_TEST_FOREGROUND, app.hwnd.0 as usize, 0);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        sandbox.log_lines("tray panel closed"),
        0,
        "the panel closed for its menu"
    );
    // Another program in front (a click elsewhere): it closes.
    let other = TestWindow::open(&["--title", &title("tray-panel-other")]);
    post(MSG_TEST_FOREGROUND, other.hwnd.0 as usize, 0);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray panel closed") >= 1).is_some(),
        "the panel stayed with another program in front"
    );
    drop(other);
    // Asked for by the user: a left click keeps the panel as well, the program's window
    // in front included.
    post(MSG_TEST_TRAY_PANEL, 1, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("tray panel opened") >= 2).is_some());
    post(MSG_TEST_TRAY_PANEL_CLICK, app.hwnd.0 as usize, 0);
    let select = (TEST_ICON_ID << 16) | NIN_SELECT;
    assert!(
        wait_until(SETTLE, || got().lines().any(|l| l.ends_with(&format!(" {select}")))).is_some(),
        "no NIN_SELECT from the panel: {:?}",
        got()
    );
    post(MSG_TEST_FOREGROUND, app.hwnd.0 as usize, 0);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        sandbox.log_lines("tray panel closed"),
        1,
        "the panel closed after a left click"
    );
    // The desktop in front (a click on it): it closes, though Explorer may own the icon.
    let desktop = kabuk_win::window::windows_of_class("Progman")
        .into_iter()
        .next()
        .expect("Explorer's desktop window");
    post(MSG_TEST_FOREGROUND, desktop.0 as usize, 0);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray panel closed") >= 2).is_some(),
        "the panel stayed with the desktop in front"
    );
    // A click on the bar, away from its icons: it closes too.
    post(MSG_TEST_TRAY_PANEL, 1, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("tray panel opened") >= 3).is_some());
    post(MSG_TEST_TRAY_PANEL_CLICK, app.hwnd.0 as usize, 0);
    let pid = sandbox.state().kabuk_pid;
    let primary_rect = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .rect;
    let (bar, rect) = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == pid)
        .find_map(|w| w.window_rect().filter(|r| primary_rect.contains(r)).map(|r| (w, r)))
        .expect("primary bar");
    // Between the clock and the tray: nothing there.
    let (x, y) = ((rect.right - rect.left) * 3 / 4, (rect.bottom - rect.top) / 2);
    let at = (y << 16) | (x & 0xFFFF);
    post_message(
        bar,
        Msg {
            id: WM_RBUTTONUP as u32,
            wparam: 0,
            lparam: at as isize,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray panel closed") >= 3).is_some(),
        "the panel stayed after a click on the bar: {:?}",
        sandbox.last_log_line(&["bar click"])
    );

    // Live: up to 12 on the bar again.
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\ntray_shown = 12\n").unwrap();
    assert!(wait_until(SETTLE, || tray_drawn() > 0).is_some(), "{}", primary_line());
    app.close();
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}

/// PLAN M12: after a forced kill the watchdog gives the user's wallpaper back with the
/// rest of the desktop (Kabuk had set pure black).
#[test]
fn a_forced_kill_gives_the_users_wallpaper_back() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"black\"\n").unwrap();
    let _watchdog = sandbox.watchdog_taking_explorer();
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("wallpaper set") >= 1).is_some());
    assert!(sandbox.state().wallpaper.is_some(), "the user's wallpaper not recorded");
    let pid = sandbox.state().kabuk_pid.unwrap();
    Process::open(pid).unwrap().terminate(1).unwrap();
    assert!(
        wait_until(RESTORE_LIMIT, || sandbox.log_lines("wallpaper_restored: true") >= 1).is_some(),
        "{}",
        story(&sandbox)
    );
    // The watchdog starts the shell again, which sets black again; restore ends that.
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::wallpaper::current().as_ref(), Some(&theirs));
    assert!(wait_until(SETTLE, || before.is_back()).is_some(), "desktop not back");
}
