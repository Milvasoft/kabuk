//! The tray and the hidden taskbar on the real desktop (PLAN M4). These tests hide the
//! user's taskbar and take the tray icons for a few seconds (standing approval in
//! CONTRIBUTING.md); each one ends with `kabuk --restore` and checks that the taskbar, its
//! auto-hide setting and the work areas are back.
//!
//! Edge cases: a program's icon appearing on the bar, a right click reaching the program
//! with the version-4 message layout, the icon's program quitting, a forced kill with the
//! taskbar hidden (the watchdog shows it within 2 s and puts auto-hide back).
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

fn taskbars_visible() -> bool {
    let bars = kabuk_win::shell::taskbars();
    !bars.is_empty() && bars.iter().all(|b| b.is_visible())
}

fn taskbars_hidden() -> bool {
    kabuk_win::shell::taskbars().iter().all(|b| !b.is_visible())
}

#[test]
fn a_programs_tray_icon_appears_on_the_bar_and_clicks_reach_it() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    assert!(wait_until(SETTLE, taskbars_hidden).is_some(), "taskbar not hidden");

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

#[test]
fn a_forced_kill_brings_the_taskbar_back_within_two_seconds() {
    let before = Desktop::now();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _watchdog = sandbox.watchdog_taking_explorer();
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("Windows taskbar hidden") == 1).is_some());
    assert!(wait_until(SETTLE, taskbars_hidden).is_some());
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
        wait_until(SETTLE, || sandbox
            .last_log_line(&["tray tooltip shown", &tip])
            .is_some())
        .is_some(),
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

/// DECISIONS D26 (asked for by the user): with `tray_shown = 0` every icon is in the
/// overflow panel, a click there reaches the program, and raising `tray_shown` in the
/// file brings icons back to the bar.
#[test]
fn icons_beyond_tray_shown_are_clicked_from_the_overflow_panel() {
    use kabuk::bar::manager::{MSG_TEST_TRAY_PANEL, MSG_TEST_TRAY_PANEL_CLICK};
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
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("tray panel closed") >= 1).is_some(),
        "the panel stays after a click"
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
