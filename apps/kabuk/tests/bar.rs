//! The bar on the real desktop. These tests reserve screen space for a few seconds
//! (as the running shell does) and always give it back: each ends with
//! `kabuk --restore`, and the work areas are compared with the values from before.
//!
//! Edge cases: bars must settle (no rebuild loop when one bar's reservation notifies
//! the others); the reserved strip matches the look's size at the monitor's DPI;
//! a forced kill with bars up gets the work areas back through the watchdog; a module
//! switched off in `kabuk.toml` is applied live; bars that meet the window manager only
//! after it published the open windows still show them.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::time::Duration;

use common::*;
use kabuk_core::state::Rect;
use kabuk_win::process::Process;

const SETTLE: Duration = Duration::from_secs(5);

fn work_areas() -> Vec<(String, Rect)> {
    kabuk_win::shell::monitors()
        .into_iter()
        .map(|m| (m.device, m.work_area))
        .collect()
}

/// Ada reserves `--edge` + `--bar-h` (14 + 38 DIPs) at the top of every monitor.
fn expected_top(dpi: u32) -> i32 {
    ((14.0 + 38.0) * dpi as f32 / 96.0).round() as i32
}

fn reserved(before: &[(String, Rect)]) -> bool {
    let now = kabuk_win::shell::monitors();
    before.iter().all(|(device, area)| {
        now.iter()
            .find(|m| &m.device == device)
            .is_some_and(|m| m.work_area.top == area.top.max(m.rect.top + expected_top(m.dpi)))
    })
}

fn restored(before: &[(String, Rect)]) -> bool {
    work_areas() == before
}

#[test]
fn bars_settle_once_and_reserve_their_strip() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let mut shell = sandbox.shell_with_bar(&[]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some(),
        "space not reserved: {:?}",
        work_areas()
    );
    // Regression: one bar's reservation notified the others, which rebuilt every bar,
    // forever. Give it time to misbehave, then count the builds.
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(sandbox.log_lines("bars built"), 1);
    assert!(shell.is_running());

    assert_eq!(sandbox.restore().0, 0);
    assert!(
        wait_until(Duration::from_secs(5), || restored(&before)).is_some(),
        "{:?} vs {before:?}",
        work_areas()
    );
}

/// The window of class `class` of Kabuk `pid` on Monitor 1 (main).
fn primary_window_of(class: &str, pid: u32) -> kabuk_win::window::Hwnd {
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    kabuk_win::window::windows_of_class(class)
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .find(|w| {
            w.window_rect()
                .is_some_and(|r| r.left >= primary.rect.left && r.right <= primary.rect.right)
        })
        .unwrap_or_else(|| panic!("no {class} on Monitor 1 (main)"))
}

/// The work area of Monitor 1 (main) now.
fn primary_work_area() -> Rect {
    kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .work_area
}

/// SPEC §2.1, found by the user: `bar.autohide` did nothing. An auto-hiding bar reserves
/// no space and waits off its edge with a sliver left on screen; the mouse resting on it
/// brings it in, and some time after the mouse leaves it slides off again. DECISIONS
/// D70, found by the user: it came over the windows whenever the mouse passed the edge
/// (on its way to a window's top), and slid off while its own panel was open. D73, asked
/// for by the user: shown, it pushes the windows down (reserves its strip), and gives the
/// room back when it slides off. Run in the test machine: the mouse is moved.
#[test]
fn an_auto_hiding_bar_makes_room_while_shown_and_slides_in_when_the_mouse_rests() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_VOLUME_PANEL};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\nautohide = true\n").unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let away = (
        (primary.rect.left + primary.rect.right) / 2,
        (primary.rect.top + primary.rect.bottom) / 2,
    );
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(work_areas(), before, "space reserved");
    let pid = sandbox.state().kabuk_pid.unwrap();
    let bar = primary_window_of("Kabuk.Bar", pid);
    let top = || bar.window_rect().map(|r| r.top).unwrap();
    // Hidden: all but a sliver above the monitor.
    assert!(
        wait_until(SETTLE, || top() < primary.rect.top).is_some(),
        "not hidden: {}",
        top()
    );
    let hidden = top();
    let sliver = bar.window_rect().unwrap().bottom - 1;
    assert!(sliver >= primary.rect.top, "nothing left to reach");
    let edge = (away.0, sliver);
    let move_to = |(x, y): (i32, i32)| {
        assert!(kabuk_win::input::set_cursor_pos(x, y));
        kabuk_win::msgloop::post_message(
            bar,
            kabuk_win::msgloop::Msg {
                id: kabuk_win::msgloop::msg::MOUSEMOVE,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
    };
    // Passing the edge, as on the way to a window's top: it stays away.
    move_to(edge);
    std::thread::sleep(Duration::from_millis(60));
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1 / 2));
    std::thread::sleep(Duration::from_millis(900));
    assert_eq!(top(), hidden, "it came in for a mouse passing the edge");
    assert_eq!(sandbox.log_lines("mouse rested at the edge"), 0);
    // Resting there: it comes in.
    let told = || sandbox.log_lines("appbar told of a move");
    let told_before = told();
    move_to(edge);
    assert!(
        wait_until(SETTLE, || top() == primary.rect.top).is_some(),
        "not shown for a resting mouse: {}\n{}",
        top(),
        sandbox.last_log_line(&["auto-hiding"]).unwrap_or_default()
    );
    assert_eq!(sandbox.log_lines("mouse rested at the edge"), 1);
    // Found by a review of the code (M19 P2): Explorer was told of every frame of the
    // slide, each call waiting on it. Once the slide ends (and once for its last frame).
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        told() - told_before <= 2,
        "Explorer told of the slide {} times",
        told() - told_before
    );
    // Shown, it makes room: the windows' area starts under it.
    assert!(
        wait_until(SETTLE, || primary_work_area().top >= bar.window_rect().unwrap().bottom).is_some(),
        "the shown bar did not push the windows: {:?} under {:?}",
        primary_work_area(),
        bar.window_rect()
    );
    // Its panel open, the mouse gone from it: it stays while the panel is open.
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|w| w.pid() == Some(pid))
        .expect("bar control window");
    let post = |wparam: usize| {
        kabuk_win::msgloop::post_message(
            control,
            kabuk_win::msgloop::Msg {
                id: MSG_TEST_VOLUME_PANEL,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    post(1);
    assert!(wait_until(SETTLE, || sandbox.log_lines("volume panel opened") >= 1).is_some());
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(top(), primary.rect.top, "it slid off with its panel open");
    // The panel closed: it slides off and gives the room back.
    post(0);
    assert!(
        wait_until(SETTLE, || top() == hidden).is_some(),
        "not hidden again: {}",
        top()
    );
    assert!(
        wait_until(SETTLE, || work_areas() == before).is_some(),
        "the room was not given back: {:?}",
        work_areas()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(work_areas(), before);
}

/// DECISIONS D75, asked for by the user: a shown auto-hiding bar pushed a maximised window
/// at once ("tak diye"). The window manager hears the work area coming first: the scene is
/// up over the window before Windows moves it, so it slides with the bar, as it slides
/// back when the bar goes. Run in the test machine: the mouse is moved.
#[test]
fn an_auto_hiding_bar_slides_a_maximised_window_out_of_its_way() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\nautohide = true\n").unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let away = (
        (primary.rect.left + primary.rect.right) / 2,
        (primary.rect.top + primary.rect.bottom) / 2,
    );
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let w = TestWindow::open(&["--title", &title("pushed")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    assert!(w.hwnd.maximize_async());
    assert!(wait_until(SETTLE, || w.hwnd.is_maximized()).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let bar = primary_window_of("Kabuk.Bar", pid);
    let top = || bar.window_rect().map(|r| r.top).unwrap();
    assert!(wait_until(SETTLE, || top() < primary.rect.top).is_some(), "not hidden");
    let hidden = top();
    std::thread::sleep(Duration::from_millis(500));
    if !kabuk_win::system::animations_on() {
        // Windows' animation effects are off: no scenes to look for.
        assert_eq!(sandbox.restore().0, 0);
        return;
    }
    let overlay_up = || {
        kabuk_win::window::windows_of_class(kabuk_win::window_motion::OVERLAY_CLASS)
            .into_iter()
            .any(|o| o.pid() == Some(pid) && o.is_visible())
    };
    // Until the window has moved: whether the scene was up over it before it did.
    let watch = |was: Rect| {
        let mut covered = false;
        let moved = wait_until(SETTLE, || {
            covered |= overlay_up();
            w.hwnd.window_rect() != Some(was)
        });
        (moved.is_some(), covered)
    };
    let edge = (away.0, bar.window_rect().unwrap().bottom - 1);
    let was = w.hwnd.window_rect().unwrap();
    assert!(kabuk_win::input::set_cursor_pos(edge.0, edge.1));
    kabuk_win::msgloop::post_message(
        bar,
        kabuk_win::msgloop::Msg {
            id: kabuk_win::msgloop::msg::MOUSEMOVE,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    let (moved, covered) = watch(was);
    assert!(moved, "the maximised window was not pushed");
    assert!(
        covered,
        "the maximised window jumped: no scene over it before it moved\n{:?}",
        sandbox.last_log_line(&["work area coming"])
    );
    assert!(
        sandbox
            .last_log_line(&["work area coming"])
            .is_some_and(|l| l.contains("maximised=1")),
        "{:?}",
        sandbox.last_log_line(&["work area coming"])
    );
    assert!(wait_until(SETTLE, || top() == primary.rect.top).is_some(), "not shown");
    assert!(
        wait_until(SETTLE, || w
            .hwnd
            .frame_rect()
            .is_some_and(|f| f.top >= bar.window_rect().unwrap().bottom))
        .is_some(),
        "the window is still under the bar: {:?} under {:?}",
        w.hwnd.frame_rect(),
        bar.window_rect()
    );
    // The bar slides off: the window slides back up.
    std::thread::sleep(Duration::from_millis(300));
    let was = w.hwnd.window_rect().unwrap();
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    let (moved, covered) = watch(was);
    assert!(moved, "the room was not given back");
    assert!(
        covered,
        "the maximised window jumped back: no scene over it before it moved"
    );
    assert!(wait_until(SETTLE, || top() == hidden).is_some(), "not hidden again");
    assert!(wait_until(SETTLE, || work_areas() == before).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(work_areas(), before);
}

/// DECISIONS D70, found by the user: with auto-hide, Rıhtım's dock did not hide. It
/// reserves nothing, waits off its edge with a sliver at the monitor's edge, comes in
/// when the mouse rests there and slides off once it leaves. D90 (asked for by the user
/// with a video of macOS's dock): shown, it comes over the windows as macOS's does,
/// pushing nothing (D73's push stays the top bar's). Run in the test machine: the mouse
/// is moved.
#[test]
fn rihtims_dock_auto_hides_with_the_bar() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\nlook = \"rihtim\"\n[bar]\nautohide = true\n",
    )
    .unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let away = (
        (primary.rect.left + primary.rect.right) / 2,
        (primary.rect.top + primary.rect.bottom) / 2,
    );
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("dock placed") == 1).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(work_areas(), before, "space reserved");
    let pid = sandbox.state().kabuk_pid.unwrap();
    let dock = primary_window_of("Kabuk.Dock", pid);
    let rect = || dock.window_rect().unwrap();
    let bottom = primary.rect.bottom;
    // Hidden: only a sliver of it inside the monitor's bottom edge.
    assert!(
        wait_until(SETTLE, || rect().top < bottom && rect().bottom > bottom).is_some(),
        "not hidden: {:?}\n{}",
        rect(),
        sandbox.last_log_line(&["auto-hiding"]).unwrap_or_default()
    );
    let hidden = rect();
    let at = ((hidden.left + hidden.right) / 2, bottom - 1);
    assert!(kabuk_win::input::set_cursor_pos(at.0, at.1));
    kabuk_win::msgloop::post_message(
        dock,
        kabuk_win::msgloop::Msg {
            id: kabuk_win::msgloop::msg::MOUSEMOVE,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || rect().bottom < bottom).is_some(),
        "not shown: {:?}\n{}",
        rect(),
        sandbox.last_log_line(&["auto-hiding"]).unwrap_or_default()
    );
    // Over the windows: the work area stays as it was all the while.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(work_areas(), before, "the shown dock pushed the windows");
    assert_eq!(sandbox.log_lines("auto-hiding dock makes room"), 0);
    // The mouse resting at the edge, under the floating dock, keeps it (found by the
    // user: it came and went again and again).
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        rect().bottom < bottom,
        "it slid off under a resting mouse: {:?}",
        rect()
    );
    assert_eq!(sandbox.log_lines("auto-hiding dock gives its room back"), 0);
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    assert!(
        wait_until(SETTLE, || rect() == hidden).is_some(),
        "not hidden again: {:?}",
        rect()
    );
    assert_eq!(work_areas(), before);
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(work_areas(), before);
}

/// DECISIONS D90 (asked for by the user with a video of macOS's dock): Rıhtım's
/// auto-hiding dock comes in over the windows and pushes nothing, while the auto-hiding
/// top bar still makes room as it comes in (D73). Shown by test messages, so the mouse
/// is not moved.
#[test]
fn rihtims_dock_comes_over_the_windows_and_the_bar_pushes_them() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_PEEK};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\nlook = \"rihtim\"\n[bar]\nautohide = true\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("dock placed") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let main_bar = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|h| h.pid() == Some(pid))
        .position(|h| {
            h.window_rect().is_some_and(|r| {
                let cx = (r.left + r.right) / 2;
                primary.rect.left <= cx && cx < primary.rect.right
            })
        });
    let peek = |which: usize, show: bool| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_PEEK,
                wparam: which,
                lparam: isize::from(show),
            },
        )
        .unwrap();
    };
    let dock = primary_window_of("Kabuk.Dock", pid);
    let bottom = primary.rect.bottom;
    assert!(wait_until(SETTLE, || dock.window_rect().unwrap().bottom > bottom).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(work_areas(), before, "hidden, something reserved");

    peek(0, true);
    assert!(
        wait_until(SETTLE, || dock.window_rect().unwrap().bottom <= bottom).is_some(),
        "the dock did not come in: {:?}",
        dock.window_rect()
    );
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(work_areas(), before, "the shown dock pushed the windows");
    assert_eq!(sandbox.log_lines("auto-hiding dock makes room"), 0);
    peek(0, false);

    // The bar (its place among the bars Windows lists, as the shell keeps them) still
    // makes room as it comes in.
    let full = before.iter().find(|(d, _)| *d == primary.device).unwrap().1;
    for i in 0..kabuk_win::shell::monitors().len() {
        peek(i + 1, true);
    }
    assert!(
        wait_until(SETTLE, || primary_work_area().top > full.top).is_some(),
        "the shown bar did not push the windows: {:?} (bar {main_bar:?})",
        primary_work_area()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

#[test]
fn a_killed_shell_gets_its_space_back_through_the_watchdog() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let mut watchdog = {
        // The watchdog helper turns the bar off for other tests; start it by hand here.
        let mut cmd = std::process::Command::new(watchdog_exe());
        cmd.arg("--state-dir")
            .arg(sandbox.state_dir())
            .arg("--config")
            .arg(sandbox.config_file())
            .args(["--quiet", "--no-hotkey", "--kabuk"])
            .arg(kabuk_exe())
            // The window manager would manage the user's windows.
            .args(["--", "--no-wm", "--leave-explorer"]);
        Proc(cmd.spawn().unwrap())
    };
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    Process::open(pid).unwrap().terminate(1).unwrap();
    assert!(wait_until(Duration::from_secs(5), || sandbox
        .log_lines("shell crashed; desktop restored")
        == 1)
    .is_some());
    // The watchdog put every recorded work area back before restarting the shell.
    let report = sandbox.log_lines(&format!("work_areas_restored: {}", before.len()));
    assert!(report >= 1, "watchdog restore did not re-apply the work areas");
    // PLAN M19 U12: the shell started again says so.
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox
            .last_log_line(&["notice shown"])
            .is_some_and(|l| l.contains("started again") || l.contains("yeniden başladı")))
        .is_some(),
        "the restarted shell did not say so"
    );

    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        watchdog.wait_exit(Duration::from_secs(5)).and_then(|s| s.code()),
        Some(0)
    );
    assert!(
        wait_until(Duration::from_secs(5), || restored(&before)).is_some(),
        "{:?} vs {before:?}",
        work_areas()
    );
}

/// Physical pixels a bar on `edge` reserves at `dpi` (Ada: edge distance plus bar or rail).
fn thickness(edge: &str, dpi: u32) -> i32 {
    let dips = if matches!(edge, "left" | "right") {
        14.0 + 54.0
    } else {
        14.0 + 38.0
    };
    (dips * dpi as f32 / 96.0).round() as i32
}

fn reserved_on(edge: &str, before: &[(String, Rect)]) -> bool {
    let now = kabuk_win::shell::monitors();
    before.iter().all(|(device, area)| {
        let Some(m) = now.iter().find(|m| &m.device == device) else {
            return false;
        };
        // AppBars stack: the bar sits next to whatever already reserves that side (the
        // taskbar stays visible until M4), so the work area shrinks by the bar's thickness.
        let t = thickness(edge, m.dpi);
        m.work_area == bar_free(edge, area, t)
    })
}

/// The work area left once a bar of `t` px takes `edge` of `area`.
fn bar_free(edge: &str, area: &Rect, t: i32) -> Rect {
    match edge {
        "top" => Rect {
            top: area.top + t,
            ..*area
        },
        "bottom" => Rect {
            bottom: area.bottom - t,
            ..*area
        },
        "left" => Rect {
            left: area.left + t,
            ..*area
        },
        _ => Rect {
            right: area.right - t,
            ..*area
        },
    }
}

/// The strip a bar of `t` px takes on `edge` of `area`.
fn bar_strip(edge: &str, area: &Rect, t: i32) -> Rect {
    match edge {
        "top" => Rect {
            bottom: area.top + t,
            ..*area
        },
        "bottom" => Rect {
            top: area.bottom - t,
            ..*area
        },
        "left" => Rect {
            right: area.left + t,
            ..*area
        },
        _ => Rect {
            left: area.right - t,
            ..*area
        },
    }
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

#[test]
fn every_edge_reserves_its_side_and_maximised_windows_stop_at_the_bar() {
    let before = work_areas();
    for edge in ["bottom", "left", "right", "top"] {
        let sandbox = Sandbox::new();
        std::fs::write(
            sandbox.config_file(),
            format!("schema = 1\n[bar]\nposition = \"{edge}\"\n"),
        )
        .unwrap();
        let _shell = sandbox.shell_with_bar(&[]);
        assert!(
            wait_until(STARTUP_TIMEOUT, || reserved_on(edge, &before)).is_some(),
            "{edge}: {:?}",
            work_areas()
        );
        assert!(
            wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some(),
            "{edge}"
        );

        let window = TestWindow::open(&["--title", &title(&format!("max-{edge}")), "--maximized"]);
        let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
        let t = thickness(edge, primary.dpi);
        let original = &before.iter().find(|(d, _)| *d == primary.device).unwrap().1;
        let strip = bar_strip(edge, original, t);
        let mut frame = None;
        assert!(
            wait_until(Duration::from_secs(5), || {
                frame = window.hwnd.frame_rect();
                frame.is_some_and(|f| primary.work_area.contains(&f) && f.right - f.left > 100)
            })
            .is_some(),
            "{edge}: maximised frame {frame:?} not inside {:?}",
            primary.work_area
        );
        assert!(!overlaps(&frame.unwrap(), &strip), "{edge}: window under the bar");
        drop(window);

        assert_eq!(sandbox.restore().0, 0);
        assert!(
            wait_until(Duration::from_secs(5), || restored(&before)).is_some(),
            "{edge}: {:?}",
            work_areas()
        );
    }
}

fn primary_device() -> String {
    kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .device
}

#[test]
fn language_change_switches_date_and_decimal_separator_live() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[language]\nlanguage = \"tr\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let device = primary_device();
    let tr_days = ["\"Paz ", "\"Pzt ", "\"Sal ", "\"Çar ", "\"Per ", "\"Cum ", "\"Cmt "];
    let en_days = ["\"Sun ", "\"Mon ", "\"Tue ", "\"Wed ", "\"Thu ", "\"Fri ", "\"Sat "];
    let drawn = || sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default();
    // The RAM figure is the drawn text ending in "G", e.g. "18,6G".
    let ram = |line: String| {
        line.split('"')
            .find(|t| t.ends_with('G') && t.len() > 1)
            .unwrap_or_default()
            .to_string()
    };
    // The figures come a moment after the first bars (read off the bar thread, M19 P3).
    assert!(
        wait_until(STARTUP_TIMEOUT, || tr_days.iter().any(|d| drawn().contains(d))
            && !ram(drawn()).is_empty())
        .is_some(),
        "{}",
        drawn()
    );
    let tr_ram = ram(drawn());
    assert!(
        tr_ram.contains(',') && !tr_ram.contains('.'),
        "Turkish RAM uses a comma: {tr_ram}"
    );

    std::fs::write(sandbox.config_file(), "schema = 1\n[language]\nlanguage = \"en\"\n").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || en_days.iter().any(|d| drawn().contains(d))).is_some(),
        "{}",
        drawn()
    );
    let en_ram = ram(drawn());
    assert!(
        en_ram.contains('.') && !en_ram.contains(','),
        "English RAM uses a point: {en_ram}"
    );

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D28 (asked for by the user): the strip behind the bar is solid by default,
/// and the blur and no background apply live.
#[test]
fn the_bar_background_follows_its_setting_live() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let device = primary_device();
    let drawn = || sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default();
    for (value, expect) in [
        (None, "strip=true background=Solid"),
        (Some("blur"), "strip=false background=Blur"),
        (Some("off"), "strip=false background=Off"),
        (Some("solid"), "strip=true background=Solid"),
    ] {
        if let Some(v) = value {
            std::fs::write(
                sandbox.config_file(),
                format!("schema = 1\n[bar]\nbackground = \"{v}\"\n"),
            )
            .unwrap();
        }
        assert!(
            wait_until(Duration::from_secs(5), || drawn().contains(expect)).is_some(),
            "{value:?}: {}",
            drawn()
        );
        // The bar made again for the setting has the blur only with it (found by the
        // user: a new bar had none, so the blur looked like no background).
        let blur = format!("blur={}", value == Some("blur"));
        let backdrop = || sandbox.last_log_line(&["bar backdrop set"]).unwrap_or_default();
        assert!(
            wait_until(Duration::from_secs(5), || backdrop().contains(&blur)).is_some(),
            "{value:?}: {}",
            backdrop()
        );
    }
    assert_eq!(sandbox.log_lines("blur behind the bar unavailable"), 0);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D29 (asked for by the user): the bar's height is set in the file and the
/// reserved strip follows it live.
#[test]
fn the_bar_height_applies_live() {
    const TALLER: i32 = 48;
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    std::fs::write(sandbox.config_file(), format!("schema = 1\n[bar]\nheight = {TALLER}\n")).unwrap();
    let taller = || {
        let now = kabuk_win::shell::monitors();
        before.iter().all(|(device, area)| {
            now.iter().find(|m| &m.device == device).is_some_and(|m| {
                let t = ((14 + TALLER) as f32 * m.dpi as f32 / 96.0).round() as i32;
                m.work_area.top == area.top.max(m.rect.top + t)
            })
        })
    };
    assert!(
        wait_until(Duration::from_secs(5), taller).is_some(),
        "{:?}",
        work_areas()
    );
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || reserved(&before)).is_some(),
        "{:?}",
        work_areas()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

#[test]
fn turning_a_module_off_in_the_file_applies_live() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar.modules]\ncpu = false\n").unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || sandbox.log_lines("configuration reloaded")
            >= 1)
        .is_some()
    );
    // A rejected edit keeps the last valid configuration.
    std::fs::write(sandbox.config_file(), "schema = 1\n[oled]\ndim_after_secs = 1\n").unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || sandbox.log_lines("configuration rejected")
            >= 1)
        .is_some()
    );

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Explorer broadcasts `TaskbarCreated` after it restarts; every AppBar registration
/// is lost then. The bars must rebuild and reserve their space again. (A real Explorer
/// restart is tried by hand; this sends the same message without restarting it.)
#[test]
fn taskbar_created_rebuilds_the_bars() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let bars: Vec<_> = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .collect();
    assert_eq!(bars.len(), kabuk_win::shell::monitors().len());
    let message = kabuk_win::system::register_message(kabuk_win::system::TASKBAR_CREATED);
    for b in &bars {
        kabuk_win::msgloop::post_message(
            *b,
            kabuk_win::msgloop::Msg {
                id: message,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
    }
    assert!(wait_until(Duration::from_secs(5), || sandbox.log_lines("bars built") == 2).is_some());
    assert!(
        wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some(),
        "{:?}",
        work_areas()
    );

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

fn shell_windows(class: &str, pid: u32) -> Vec<kabuk_win::window::Hwnd> {
    kabuk_win::window::windows_of_class(class)
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .collect()
}

/// Rıhtım's tokens: the bar's top strip and the dock's bottom strip, in px at `dpi`.
fn rihtim_strips(dpi: u32) -> (i32, i32) {
    let t = kabuk_theme::theme("f").unwrap();
    let t = t.tokens(kabuk_theme::Mode::Dark);
    let px = |dips: f32| (dips * dpi as f32 / 96.0).round() as i32;
    (
        px(t.px("edge").unwrap() + t.px("bar-h").unwrap()),
        px(t.px("dock").unwrap()),
    )
}

/// Rıhtım puts a dock on the primary monitor's bottom edge and reserves its strip there
/// only; switching to another look live removes it and gives the space back.
#[test]
fn rihtim_docks_on_the_primary_monitor_and_leaves_with_the_look() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\nlook = \"rihtim\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    let primary = primary_device();
    let expected = |dock: bool| {
        let now = kabuk_win::shell::monitors();
        before.iter().all(|(device, area)| {
            let Some(m) = now.iter().find(|m| &m.device == device) else {
                return false;
            };
            let (bar, dock_px) = if dock {
                rihtim_strips(m.dpi)
            } else {
                (thickness("top", m.dpi), 0)
            };
            let bottom = if *device == primary {
                area.bottom - dock_px
            } else {
                area.bottom
            };
            m.work_area
                == Rect {
                    top: area.top + bar,
                    bottom,
                    ..*area
                }
        })
    };
    assert!(
        wait_until(STARTUP_TIMEOUT, || expected(true)).is_some(),
        "{:?} vs {before:?}",
        work_areas()
    );
    // The log is written after the reservation; wait for it.
    assert!(wait_until(Duration::from_secs(2), || sandbox.log_lines("dock placed") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let docks = shell_windows("Kabuk.Dock", pid);
    assert_eq!(docks.len(), 1);
    // The dock window sits inside its strip, on the primary monitor.
    let m = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let frame = docks[0].frame_rect().unwrap();
    assert!(m.rect.contains(&frame), "{frame:?} not on {:?}", m.rect);
    assert!(
        frame.top >= m.work_area.bottom,
        "{frame:?} above the strip {:?}",
        m.work_area
    );

    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\nlook = \"ada\"\n").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || expected(false)).is_some(),
        "{:?} vs {before:?}",
        work_areas()
    );
    assert!(shell_windows("Kabuk.Dock", pid).is_empty());

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// "Follow Windows" changes the bar as soon as Windows announces a new app mode, and the
/// mode, accent and text size settings apply live. Windows' mode is read from a test file
/// here, so the user's theme is never touched.
#[test]
fn follow_windows_and_appearance_settings_apply_live() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let mode_file = sandbox.dir.path().join("app-mode");
    std::fs::write(&mode_file, "dark").unwrap();
    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\nmode = \"system\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&["--debug-app-mode-file", mode_file.to_str().unwrap()]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let last_look = || sandbox.last_log_line(&["look resolved"]).unwrap_or_default();
    assert!(last_look().contains("mode=Dark"), "{}", last_look());

    std::fs::write(&mode_file, "light").unwrap();
    let pid = sandbox.state().kabuk_pid.unwrap();
    let sent = std::time::Instant::now();
    for bar in shell_windows("Kabuk.Bar", pid) {
        kabuk_win::system::send_setting_change(bar, kabuk_win::system::IMMERSIVE_COLOR_SET, Duration::from_secs(2))
            .unwrap();
    }
    // The bar handles the message before the send returns; allow only for the log write.
    assert!(
        wait_until(Duration::from_millis(500), || last_look().contains("mode=Light")).is_some(),
        "{}",
        last_look()
    );
    assert!(sent.elapsed() < Duration::from_secs(1));
    assert_eq!(sandbox.log_lines("Windows app mode changed"), 1);
    assert_eq!(sandbox.log_lines("mode changed mode=Light"), 1);

    // Other areas of WM_SETTINGCHANGE change nothing.
    for bar in shell_windows("Kabuk.Bar", pid) {
        kabuk_win::system::send_setting_change(bar, "Environment", Duration::from_secs(2)).unwrap();
    }
    assert_eq!(sandbox.log_lines("Windows app mode changed"), 1);

    let accent = |line: &str| {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix("accent="))
            .unwrap_or_default()
            .to_string()
    };
    let default_accent = accent(&last_look());
    assert!(default_accent.starts_with('#'), "{}", last_look());
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\nmode = \"dark\"\naccent = \"#e91e63\"\ntext_size = \"large\"\n",
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || {
            let l = last_look();
            l.contains("mode=Dark") && l.contains("text_scale=1.15") && accent(&l) != default_accent
        })
        .is_some(),
        "{}",
        last_look()
    );

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// M10: a change of mode blends the bar's old picture into the new one instead of
/// switching it in one frame; with Windows' animation effects off it switches at once.
#[test]
fn a_mode_change_blends_the_bar_into_its_new_look() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let mode_file = sandbox.dir.path().join("app-mode");
    std::fs::write(&mode_file, "dark").unwrap();
    // Game mode off: a full-screen program on the real desktop would still animations.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\nmode = \"system\"\n[game]\ndetect = false\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&["--debug-app-mode-file", mode_file.to_str().unwrap()]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let bars = shell_windows("Kabuk.Bar", pid);
    let r = kabuk_win::dpi::physical(|| bars[0].window_rect()).expect("bar rect");
    let area = kabuk_win::msgloop::PxRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    let look = || kabuk_win::dpi::physical(|| kabuk_win::screen::capture(area)).expect("screen");
    std::thread::sleep(Duration::from_millis(500));
    let dark = look();
    let frames = kabuk_win::screen::Frames::now();
    std::fs::write(&mode_file, "light").unwrap();
    for bar in &bars {
        kabuk_win::system::send_setting_change(*bar, kabuk_win::system::IMMERSIVE_COLOR_SET, Duration::from_secs(2))
            .unwrap();
    }
    // Right after the change, while it blends.
    let blending = look();
    std::thread::sleep(Duration::from_millis(600));
    let light = look();
    // M10: the compositor kept up with the blend (no frame missed or dropped).
    let blended = frames.since(kabuk_win::screen::Frames::now());
    assert!(
        blended.composed > 0 && blended.missed == 0 && blended.dropped == 0,
        "{blended:?}"
    );
    let change = kabuk_win::screen::difference(&dark, &light);
    assert!(change > 2.0, "the mode did not change the bar: {change}");
    let animates = kabuk_win::system::animations_on();
    assert_eq!(sandbox.log_lines("look blends in"), usize::from(animates));
    if animates {
        // Some way from the dark picture and not yet at the light one.
        let left = kabuk_win::screen::difference(&blending, &light);
        assert!(left > change * 0.2, "switched in one frame: {left} of {change}");
    }
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Reported by the user: the bar stayed over a full-screen game and video. A program
/// covering a monitor sends its bar away (the other monitor's stays), and the bar comes
/// back when the program leaves. The shell's ABN_FULLSCREENAPP is posted by the test, as
/// Explorer would send it.
#[test]
fn a_full_screen_program_sends_its_monitors_bar_away() {
    use kabuk::bar::manager::MSG_APPBAR;
    use kabuk_win::msgloop::{post_message, Msg};
    const ABN_FULLSCREENAPP: usize = 2;
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    // This shell's own bars (the user's running shell may reserve space already).
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") >= 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let on_primary = |w: &kabuk_win::window::Hwnd| {
        w.window_rect().is_some_and(|r| {
            let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            primary.rect.left <= cx && cx < primary.rect.right && primary.rect.top <= cy && cy < primary.rect.bottom
        })
    };
    let bars = shell_windows("Kabuk.Bar", pid);
    let main_bar = *bars.iter().find(|b| on_primary(b)).expect("primary bar");
    let others: Vec<_> = bars.iter().copied().filter(|b| *b != main_bar).collect();
    let notify = |entering: bool| {
        post_message(
            main_bar,
            Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };

    let game = TestWindow::open(&["--fullscreen", "--title", &title("fullscreen")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the full-screen window"
    );
    // The log is written a moment after the window changes.
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("bar steps aside for a full-screen program")
        == 1)
    .is_some());
    assert!(others.iter().all(|b| b.is_visible()), "another monitor's bar went away");

    // Out of full screen, still in front (closing it would leave whatever the user has
    // behind it in front, perhaps a full-screen video of their own).
    kabuk_win::dpi::physical(|| {
        let r = kabuk_core::state::Rect {
            left: primary.work_area.left + 100,
            top: primary.work_area.top + 100,
            right: primary.work_area.left + 900,
            bottom: primary.work_area.top + 700,
        };
        assert!(game.hwnd.place_frame_async(r));
    });
    assert!(wait_until(SETTLE, || game
        .hwnd
        .window_rect()
        .is_some_and(|r| r.right - r.left < 1000))
    .is_some());
    notify(false);
    assert!(
        wait_until(SETTLE, || main_bar.is_visible()).is_some(),
        "the bar did not come back"
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("bar back after a full-screen program")
            == 1)
        .is_some()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// D85, found by the user: games stuttered. While a full-screen program is in front the
/// keyboard hook is off (the game's keys never wait for Kabuk) and Kabuk runs below normal
/// priority; both come back once the program leaves full screen.
#[test]
fn kabuk_steps_back_for_a_full_screen_program() {
    use kabuk::bar::manager::MSG_APPBAR;
    use kabuk_win::msgloop::{post_message, Msg};
    const ABN_FULLSCREENAPP: usize = 2;
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.kabuk_with(&["--manage-class", TEST_WINDOW_CLASS, "--debug-shortcuts"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") >= 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let main_bar = shell_windows("Kabuk.Bar", pid)
        .into_iter()
        .find(|b| {
            b.window_rect().is_some_and(|r| {
                let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                primary.rect.left <= cx && cx < primary.rect.right && primary.rect.top <= cy && cy < primary.rect.bottom
            })
        })
        .expect("primary bar");
    let notify = |entering: bool| {
        post_message(
            main_bar,
            Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };

    let game = TestWindow::open(&["--fullscreen", "--title", &title("stepback")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    // The game in front, as a game is: only then do its keys stop going through Kabuk
    // (a window left in front by another test on another monitor keeps the hook on).
    assert!(
        wait_until(SETTLE, || game.hwnd.focus()).is_some(),
        "the game could not come to the front"
    );
    notify(true);
    // Every line of the shell's log about game mode, for a failure's message.
    let game_lines = || -> String {
        std::fs::read_dir(sandbox.state_dir().join("logs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("kabuk."))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .flat_map(|t| t.lines().map(str::to_string).collect::<Vec<_>>())
            .filter(|l| {
                [
                    "game mode",
                    "kabuk::wm",
                    "keyboard",
                    "full screen",
                    "steps aside",
                    "front",
                ]
                .iter()
                .any(|k| l.contains(k))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("keyboard hook off") == 1).is_some(),
        "the keyboard hook stayed on over the full-screen window:\n{}",
        game_lines()
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("below_normal=true") == 1).is_some(),
        "Kabuk's priority stayed normal: {:?}",
        sandbox.last_log_line(&["priority"])
    );

    kabuk_win::dpi::physical(|| {
        let r = kabuk_core::state::Rect {
            left: primary.work_area.left + 100,
            top: primary.work_area.top + 100,
            right: primary.work_area.left + 900,
            bottom: primary.work_area.top + 700,
        };
        assert!(game.hwnd.place_frame_async(r));
    });
    assert!(wait_until(SETTLE, || game
        .hwnd
        .window_rect()
        .is_some_and(|r| r.right - r.left < 1000))
    .is_some());
    notify(false);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("keyboard hook back") == 1).is_some(),
        "the keyboard hook did not come back:\n{}",
        game_lines()
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("below_normal=false") == 1).is_some(),
        "Kabuk's priority did not come back"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// D85, found by a test: a game already in full screen when Kabuk starts is found by the
/// bar before the window manager is there, and the window manager never paused for it.
/// It learns of it as it comes, and its keyboard hook comes off.
#[test]
fn a_game_running_before_kabuk_starts_is_stepped_back_for() {
    let before = work_areas();
    let game = TestWindow::open(&["--fullscreen", "--title", &title("early-game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    assert!(
        wait_until(SETTLE, || game.hwnd.focus()).is_some(),
        "the game could not come to the front"
    );
    let sandbox = Sandbox::new();
    let _shell = sandbox.kabuk_with(&["--manage-class", TEST_WINDOW_CLASS, "--debug-shortcuts"]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox
            .log_lines("tiling paused for full-screen programs")
            == 1)
        .is_some(),
        "the window manager never heard of the game: {:?}",
        sandbox.last_log_line(&["game mode"])
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("keyboard hook off") == 1).is_some(),
        "the keyboard hook stayed on over the game"
    );
    // D87, found in the user's log: Kabuk started under a game made the genie's GPU
    // renderer as game mode came, and kept it a minute beside the game. None is left.
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("genie renderer made")
            == sandbox.log_lines("genie renderer let go"))
        .is_some(),
        "the genie's renderer stayed beside the game: made {}, let go {}",
        sandbox.log_lines("genie renderer made"),
        sandbox.log_lines("genie renderer let go")
    );
    drop(game);
    // Seen still on screen as it closes, the game keeps game mode as long as a window in
    // front of it may (D87, 3 s), then lets it go (found by a full run: a slow machine
    // took a moment more than SETTLE).
    let hold = Duration::from_secs(3);
    assert!(
        wait_until(SETTLE + hold, || sandbox.log_lines("keyboard hook back") == 1).is_some(),
        "the keyboard hook did not come back after the game: {:?}",
        sandbox.last_log_line(&["kabuk::bar::manager: game"])
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user with a screenshot of a game: Rıhtım's dock stayed over a full-screen
/// game while the bar went away. The dock steps aside with its monitor's bar, and comes
/// back with it.
#[test]
fn a_full_screen_program_sends_the_dock_away() {
    use kabuk::bar::manager::MSG_APPBAR;
    use kabuk_win::msgloop::{post_message, Msg};
    const ABN_FULLSCREENAPP: usize = 2;
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\nlook = \"rihtim\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("dock placed") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let dock = primary_window_of("Kabuk.Dock", pid);
    let main_bar = primary_window_of("Kabuk.Bar", pid);
    assert!(wait_until(SETTLE, || dock.is_visible()).is_some(), "no dock");
    let notify = |entering: bool| {
        post_message(
            main_bar,
            Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };
    let game = TestWindow::open(&["--fullscreen", "--title", &title("dock-game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the game"
    );
    assert!(
        wait_until(SETTLE, || !dock.is_visible()).is_some(),
        "the dock stayed over the game"
    );
    drop(game);
    notify(false);
    assert!(
        wait_until(SETTLE, || dock.is_visible() && main_bar.is_visible()).is_some(),
        "the dock did not come back"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user: a program being developed (Fenpo, redeployed by Visual Studio)
/// showed a blank icon on the bar for as long as the shell ran, the picture loaded while
/// its package was being put back. A program started again gets its picture anew.
#[test]
fn a_program_started_again_gets_its_icon_anew() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    // The bar's apps' pictures (loaded on a worker since M19 P3).
    let loaded = || sandbox.log_lines("owner=\"apps\" icon=");
    // By its file name: the build folder can be reached by another path (a junction),
    // which the process's own path shows.
    let exe = testapp_exe().file_name().unwrap().to_string_lossy().into_owned();
    let of_testapp = || {
        sandbox
            .last_log_line(&["owner=\"apps\"", "picture loaded"])
            .is_some_and(|l| l.contains(&exe))
    };
    // Pictures load one at a time off the bar thread, a second each on a busy machine:
    // the pins' are asked for at the start, ahead of it.
    const ICONS: Duration = Duration::from_secs(30);
    let first = TestWindow::open(&["--title", &title("icon-again-1")]);
    assert!(wait_until(ICONS, &of_testapp).is_some(), "its icon loaded");
    let after_first = loaded();
    drop(first);
    let second = TestWindow::open(&["--title", &title("icon-again-2")]);
    let managed = format!("managing window hwnd={}", second.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    assert!(
        wait_until(ICONS, || loaded() > after_first && of_testapp()).is_some(),
        "the new process's icon was not loaded again"
    );
    drop(second);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D25: a click on an app's icon shows its window; a click while that window
/// is in front minimizes it, as on Windows' taskbar (asked for by the user).
#[test]
fn an_apps_icon_shows_its_window_and_minimizes_it_when_in_front() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_CLICK};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("apps-click")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let click = |in_front: bool| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_CLICK,
                wparam: w.hwnd.0 as usize,
                lparam: isize::from(in_front),
            },
        )
        .expect("post to the bar");
    };

    click(true);
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some(), "not minimized");
    assert_eq!(sandbox.log_lines("app minimized from the bar"), 1);
    click(false);
    assert!(
        wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some(),
        "not shown again"
    );
    assert_eq!(sandbox.log_lines("app shown from the bar"), 1);

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D86 (asked for by the user: a picture dragged over WhatsApp on Windows'
/// taskbar brings WhatsApp forward): a drag resting over an app's icon brings its window
/// forward; a drag that moves on before the delay brings nothing.
#[test]
fn a_drag_resting_over_an_apps_icon_brings_its_window_forward() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_DRAG_APP};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("apps-drag")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    // Over the app (its window), or over nothing (0).
    let drag = |over: u64| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_DRAG_APP,
                wparam: over as usize,
                lparam: 0,
            },
        )
        .expect("post to the bar");
    };
    w.hwnd.minimize_async();
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some(), "not minimized");

    // Moved on at once: nothing comes forward.
    drag(w.hwnd.0);
    drag(0);
    std::thread::sleep(Duration::from_secs(2));
    assert!(w.hwnd.is_minimized(), "a passing drag brought the window forward");
    assert_eq!(sandbox.log_lines("app shown for a drag"), 0);

    // Resting: the window comes forward.
    drag(w.hwnd.0);
    assert!(
        wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some(),
        "a resting drag did not bring the window forward"
    );
    assert_eq!(sandbox.log_lines("app shown for a drag"), 1);

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D45 (asked for by the user: Windows' taskbar on a right click): an app's menu opens
/// and closes; pin keeps the program in Kabuk's own pins, dragged to the front it stays
/// first, unpin takes it off again, and "close" closes its window. Its own rule (D63):
/// always floating is written to the configuration file, and taken out again.
#[test]
fn an_apps_menu_pins_moves_unpins_and_closes() {
    use kabuk::bar::manager::{
        APP_MENU_CLOSE, APP_MENU_CLOSE_WINDOWS, APP_MENU_FLOAT_RULE, APP_MENU_OPEN, APP_MENU_PIN, APP_MENU_TO_FRONT,
        CONTROL_CLASS, MSG_TEST_APP_MENU,
    };
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("apps-menu")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |what: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_MENU,
                wparam: what,
                lparam: w.hwnd.0 as isize,
            },
        )
        .expect("post to the bar")
    };
    let menu = || {
        kabuk_win::window::windows_of_class("Kabuk.AppMenu")
            .into_iter()
            .find(|h| h.pid() == Some(pid) && h.is_visible())
    };
    let pins_file = sandbox.state_file().parent().unwrap().join("pins.json");
    let pins = || -> Vec<String> {
        std::fs::read_to_string(&pins_file)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["pins"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|p| p["link"].as_str().map(str::to_lowercase))
            .collect()
    };
    // The path Windows gives for its process: the real one (the build folder may be a
    // link to another drive).
    let exe = std::fs::canonicalize(testapp_exe())
        .unwrap()
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_lowercase();

    post(APP_MENU_OPEN);
    assert!(
        wait_until(SETTLE, || menu().is_some()).is_some(),
        "the menu did not open"
    );
    // Not pinned, one window on workspace 1: a new window, pin, always floating, always
    // on workspace 1, close.
    assert!(wait_until(SETTLE, || sandbox.log_lines("app menu opened") == 1).is_some());
    assert_eq!(sandbox.log_lines("entries=5"), 1);
    post(APP_MENU_CLOSE);
    assert!(wait_until(SETTLE, || menu().is_none()).is_some(), "the menu stayed");

    post(APP_MENU_PIN);
    assert!(
        wait_until(SETTLE, || pins().contains(&exe)).is_some(),
        "not pinned: {:?}",
        pins()
    );
    post(APP_MENU_TO_FRONT);
    assert!(
        wait_until(SETTLE, || pins().first() == Some(&exe)).is_some(),
        "not first: {:?}",
        pins()
    );
    post(APP_MENU_PIN);
    assert!(
        wait_until(SETTLE, || !pins().contains(&exe)).is_some(),
        "not unpinned: {:?}",
        pins()
    );
    // Always floating: the program's own rule in the file, then gone again.
    let floats = || {
        let text = std::fs::read_to_string(sandbox.config_file()).unwrap_or_default();
        text.contains("[[rules.rule]]") && text.contains("kabuk-testapp.exe") && text.contains("float = true")
    };
    post(APP_MENU_FLOAT_RULE);
    assert!(
        wait_until(SETTLE, floats).is_some(),
        "{}",
        std::fs::read_to_string(sandbox.config_file()).unwrap_or_default()
    );
    post(APP_MENU_FLOAT_RULE);
    assert!(wait_until(SETTLE, || !floats()).is_some(), "the rule stayed");
    assert_eq!(sandbox.log_lines("app rule saved"), 2);
    post(APP_MENU_CLOSE_WINDOWS);
    assert!(wait_until(SETTLE, || !w.hwnd.exists()).is_some(), "the window stayed");

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Asked for by the user (Windows' taskbar on hover): an app's previews show a live
/// picture of each of its windows, a click on one brings that window back, and they go.
#[test]
fn an_apps_previews_show_its_windows_and_bring_one_back() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_PREVIEW};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1
",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("apps-preview")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |what: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_PREVIEW,
                wparam: what,
                lparam: w.hwnd.0 as isize,
            },
        )
        .expect("post to the bar")
    };
    let preview = || {
        kabuk_win::window::windows_of_class("Kabuk.AppPreview")
            .into_iter()
            .find(|h| h.pid() == Some(pid) && h.is_visible())
    };

    post(1);
    assert!(wait_until(SETTLE, || preview().is_some()).is_some(), "no previews");
    assert!(wait_until(SETTLE, || sandbox.log_lines("windows=1 pictures=1") == 1).is_some());
    post(0);
    assert!(
        wait_until(SETTLE, || preview().is_none()).is_some(),
        "the previews stayed"
    );

    // A minimised window's card brings it back.
    w.hwnd.minimize_async();
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some());
    post(1);
    assert!(wait_until(SETTLE, || preview().is_some()).is_some(), "no previews");
    post(2);
    assert!(
        wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some(),
        "not brought back"
    );
    assert_eq!(sandbox.log_lines("window shown from its preview"), 1);
    assert!(
        wait_until(SETTLE, || preview().is_none()).is_some(),
        "the previews stayed"
    );

    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user: Chrome, minimised when Kabuk started, never played the genie while
/// windows opened later did. A window first met minimised gets the genie too.
#[test]
fn a_window_met_minimised_gets_the_genie() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let w = TestWindow::open(&["--title", &title("met-minimised"), "--minimized"]);
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some());
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let recorded = || sandbox.state().genie_windows.iter().any(|g| g.hwnd == w.hwnd.0);
    if kabuk_win::system::animations_on() {
        assert!(
            wait_until(SETTLE, recorded).is_some(),
            "no genie for the minimised window"
        );
    } else {
        // Windows' animation effects are off: no genie, nothing turned off.
        std::thread::sleep(SETTLE);
        assert!(!recorded());
    }
    assert_eq!(sandbox.restore().0, 0);
}

/// Asked for by the user (macOS's genie): a managed window's own transitions are turned
/// off (recorded first); minimising it plays the genie into its icon on the bar, and
/// bringing it back from the bar plays it out of the icon (brought back by Windows, it
/// shows at once: D82); Windows' own effect chosen in the file gives its transitions back,
/// and so does `--restore`.
#[test]
fn minimising_plays_the_genie_into_the_apps_icon() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1
",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("genie")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let recorded = || sandbox.state().genie_windows.iter().any(|g| g.hwnd == w.hwnd.0);
    if !kabuk_win::system::animations_on() {
        // Windows' animation effects are off: no genie, nothing turned off.
        std::thread::sleep(SETTLE);
        assert!(!recorded());
        assert_eq!(sandbox.restore().0, 0);
        return;
    }
    assert!(wait_until(SETTLE, &recorded).is_some(), "not recorded");
    // The bar told the window manager where the window's icon is.
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["app icons known"])
            .is_some_and(|l| !l.contains("icons=0")))
        .is_some(),
        "no icons: {:?} | {:?} | {:?} | {:?} | {:?} | {:?}",
        sandbox.last_log_line(&["app icons known"]),
        sandbox.last_log_line(&["game mode"]),
        sandbox.last_log_line(&["full-screen check"]),
        sandbox.last_log_line(&["apps"]),
        sandbox.last_log_line(&["bar drawn"]),
        sandbox.last_log_line(&["bars built"])
    );
    let pid = sandbox.state().kabuk_pid.unwrap();
    let overlays = || {
        kabuk_win::window::windows_of_class(kabuk_win::window_motion::OVERLAY_CLASS)
            .into_iter()
            .filter(|o| o.pid() == Some(pid) && o.is_visible())
            .count()
    };
    let genies = |minimize: bool| {
        let needle = format!("hwnd={} minimize={minimize}", w.hwnd.0);
        sandbox.log_lines(&needle)
    };
    // Found by the user: the first genie had the strips' stairs, the later ones were
    // smooth; the renderer was made only by a genie. While the genie is on it is made
    // ahead and kept (DECISIONS D83).
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("genie renderer made") == 1).is_some(),
        "the genie's renderer not made ahead"
    );
    // How each genie played: drawn by the GPU with the window's picture for so many
    // frames, or `None` with the strips.
    let genie_scenes = || -> Vec<Option<u32>> {
        std::fs::read_dir(sandbox.state_dir().join("logs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("kabuk."))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .flat_map(|text| {
                text.lines()
                    .filter(|l| l.contains("window motion played") && l.contains("genie=true"))
                    .map(|l| {
                        l.split("gpu=")
                            .nth(1)
                            .and_then(|v| v.split_whitespace().next())
                            .and_then(|v| v.parse::<u32>().ok())
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let frames = kabuk_win::screen::Frames::now();
    let was = w.hwnd.window_rect().expect("its place");
    w.hwnd.minimize_async();
    assert!(
        wait_until(SETTLE, || genies(true) == 1).is_some(),
        "no genie into the icon"
    );
    // It pours in from where it was on screen (found by the user: from where the window
    // manager had last put it, on the other monitor).
    let line = sandbox.last_log_line(&["genie", "minimize=true"]).unwrap();
    let nums: Vec<i32> = line
        .split("frame=Rect {")
        .nth(1)
        .and_then(|r| r.split('}').next())
        .map(|r| {
            r.split(',')
                .filter_map(|kv| kv.split(':').nth(1)?.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(nums.len(), 4, "{line}");
    let near = |a: i32, b: i32| (a - b).abs() <= 16;
    assert!(
        near(nums[0], was.left) && near(nums[1], was.top) && near(nums[2], was.right) && near(nums[3], was.bottom),
        "{line} vs {was:?}"
    );
    assert!(w.hwnd.is_minimized());
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");
    // Brought back by Windows (Alt+Tab, another program), it shows at once: a genie over
    // the real desktop would pour out over it (DECISIONS D82). From the bar, below, it
    // comes out of its icon.
    w.hwnd.restore_async();
    assert!(wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some());
    std::thread::sleep(Duration::from_secs_f32(kabuk_ui::motion::WINDOW_GENIE));
    assert_eq!(genies(false), 0, "a genie over the window Windows brought back");
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");
    // The compositor kept up with the strips.
    let f = frames.since(kabuk_win::screen::Frames::now());
    assert!(f.missed == 0 && f.dropped == 0, "{f:?}");

    // From the bar (found by the user: the window flashed before its genie): Kabuk puts
    // the genie on screen first, then minimises the window, and brings it back the same
    // way; the event that follows goes on with that genie.
    let control = kabuk_win::window::message_windows_of_class(kabuk::bar::manager::CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let click = |in_front: bool| {
        kabuk_win::msgloop::post_message(
            control,
            kabuk_win::msgloop::Msg {
                id: kabuk::bar::manager::MSG_TEST_APP_CLICK,
                wparam: w.hwnd.0 as usize,
                lparam: isize::from(in_front),
            },
        )
        .expect("post to the bar")
    };
    let leads = || sandbox.log_lines(&format!("genie leads hwnd={}", w.hwnd.0));
    click(true);
    assert!(
        wait_until(SETTLE, || w.hwnd.is_minimized()).is_some(),
        "not minimised from the bar"
    );
    assert_eq!(leads(), 1);
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");
    // Found by the user: a window of Monitor 2 came back on Monitor 1 for a moment
    // (where Windows last had it), then jumped to its tile. Windows' idea of its place is
    // spoiled on purpose: it must never show there.
    let tile = w.hwnd.restored_place().expect("its place").0;
    let stale = kabuk_core::state::Rect {
        left: tile.left + 200,
        top: tile.top + 150,
        right: tile.left + 600,
        bottom: tile.top + 450,
    };
    assert!(w.hwnd.set_restored_place(stale));
    assert!(wait_until(SETTLE, || w.hwnd.restored_place().is_some_and(|(r, _)| r == stale)).is_some());
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let stale_on_screen = kabuk_core::genie::workspace_to_screen(stale, primary.rect, primary.work_area);
    click(false);
    let mut seen_stale = None;
    let watching = std::time::Instant::now();
    while watching.elapsed() < Duration::from_millis(1500) {
        if let Some(r) = w.hwnd.window_rect().filter(|_| !w.hwnd.is_minimized()) {
            if (r.right - r.left, r.bottom - r.top) == (stale.right - stale.left, stale.bottom - stale.top)
                && r.left == stale_on_screen.left
            {
                seen_stale = Some(r);
            }
        }
    }
    assert_eq!(seen_stale, None, "it came back where Windows last had it");
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");
    click(true);
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some());
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");
    let leads = || sandbox.log_lines(&format!("genie leads hwnd={}", w.hwnd.0)) - 2;
    click(false);
    // Found by the user (a video): the window came back at once, its genie playing
    // under it. Until its genie has played, the window stays below the overlay; then it
    // comes back under the genie's last picture, the same as itself.
    let overlay_above = || {
        let all = kabuk_win::window::top_level_windows();
        let overlay = all.iter().position(|o| {
            o.pid() == Some(pid) && o.is_visible() && o.class_name() == kabuk_win::window_motion::OVERLAY_CLASS
        });
        let window = all.iter().position(|o| *o == w.hwnd);
        // Found by the user: an ordinary overlay stayed under the window in front, so a
        // window went behind it as its genie started. It is topmost, over every ordinary
        // window, and under the bar.
        if let Some(o) = overlay {
            assert!(all[o].is_topmost(), "the overlay is not topmost");
        }
        match (overlay, window) {
            (Some(o), Some(win)) => Some(o < win || w.hwnd.is_minimized()),
            _ => None,
        }
    };
    let clicked = std::time::Instant::now();
    let mut seen = 0;
    while clicked.elapsed() < SETTLE {
        match overlay_above() {
            Some(true) => seen += 1,
            Some(false) if clicked.elapsed() < Duration::from_secs_f32(kabuk_ui::motion::WINDOW_GENIE) => {
                panic!("the window came back above its genie after {:?}", clicked.elapsed())
            }
            Some(false) => seen += 1,
            None if seen > 0 => break,
            // Found by the user: the window came back, black and unpainted, before its
            // genie was on screen (the scene came up only once the GPU was ready).
            None if !w.hwnd.is_minimized() => {
                panic!("the window came back before its genie was on screen")
            }
            None => {}
        }
    }
    assert!(seen > 0, "no genie out of the icon from the bar");
    assert!(
        wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some(),
        "not brought back from the bar"
    );
    assert_eq!(leads(), 2);
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "the overlay stayed");

    // Found by the user: the strips' 22 px stairs on the funnel's edges. Every genie from
    // the bar is drawn by the GPU with the window's picture, smooth: the first into the
    // icon too, and the ones out of it with the picture kept when the window went in (D82,
    // D83). The first of all is Windows' own minimise: the window is gone before Kabuk
    // hears of it, no picture can be taken of it, and it plays with the strips.
    let scenes = genie_scenes();
    assert!(
        scenes.len() == 5 && scenes[1..].iter().all(|g| g.is_some_and(|n| n > 0)),
        "a genie from the bar not drawn smooth: {scenes:?}"
    );
    assert_eq!(sandbox.log_lines("genie renderer made"), 1);
    // Windows' own effect: the record goes, the transitions come back.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[windows]\nminimize_effect = \"windows\"\n",
    )
    .unwrap();
    assert!(wait_until(SETTLE, || !recorded()).is_some(), "still recorded");
    // No genie: its renderer goes.
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("genie renderer let go") == 1).is_some(),
        "the genie's renderer kept with Windows' own effect"
    );
    let played = genies(true);
    w.hwnd.minimize_async();
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(genies(true), played, "a genie with Windows' own effect");
    w.hwnd.restore_async();
    assert!(wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some());
    std::fs::write(
        sandbox.config_file(),
        "schema = 1
",
    )
    .unwrap();
    assert!(wait_until(SETTLE, recorded).is_some(), "not recorded again");
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("genie renderer made") == 2).is_some(),
        "the genie's renderer not made again"
    );
    let (code, out) = sandbox.restore();
    assert_eq!(code, 0, "{out}");
    assert!(
        sandbox.log_lines("transitions_reset: 1") >= 1 || out.contains("transitions_reset: 1"),
        "{out}"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D25 (PLAN M7a): the apps module shows the managed windows' programs, and
/// turning it off in the file takes them off the bar live.
#[test]
fn the_apps_module_follows_its_setting_live() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("apps-live")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let device = primary_device();
    // App pictures have the top bit set (`bar::apps::image_key`); tray ones never do.
    let apps_drawn = || {
        let line = sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default();
        let list = line
            .split("images=[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .unwrap_or_default();
        list.split(',')
            .filter_map(|k| k.trim().parse::<u64>().ok())
            .filter(|k| *k >= 1 << 63)
            .count()
    };
    assert!(
        wait_until(SETTLE, || apps_drawn() >= 1).is_some(),
        "the test window's program is not on the bar: {}",
        sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default()
    );
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar.modules]\ntaskbar = false\n").unwrap();
    assert!(wait_until(SETTLE, || apps_drawn() == 0).is_some(), "apps still drawn");
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    assert!(wait_until(SETTLE, || apps_drawn() >= 1).is_some(), "apps not back");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D88 (asked for by the user: the bar had no network icon): Windows' network
/// indicator shows the link on the primary bar and follows its setting live, its watch
/// with it; a click opens Kabuk's network panel, which closes again.
#[test]
fn the_network_indicator_follows_its_setting_and_opens_its_panel() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_NETWORK_PANEL, NETWORK_PANEL_CLOSE, NETWORK_PANEL_OPEN};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    sandbox.wait_started();
    let device = primary_device();
    let link_icons = ["\"wifi\"", "\"wifi-2\"", "\"wifi-1\"", "\"wifi-off\"", "\"eth\""];
    let drawn = || {
        let line = sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default();
        let icons = line
            .split("icons=[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .unwrap_or_default()
            .to_string();
        link_icons.iter().any(|i| icons.contains(i))
    };
    assert!(
        wait_until(SETTLE, drawn).is_some(),
        "no network indicator: {:?} {}",
        sandbox.last_log_line(&["network"]),
        sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default()
    );
    assert_eq!(sandbox.log_lines("network watched"), 1);

    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |what: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_NETWORK_PANEL,
                wparam: what,
                lparam: 0,
            },
        )
        .unwrap();
    };
    let panel = || {
        kabuk_win::window::windows_of_class("Kabuk.NetworkPanel")
            .into_iter()
            .any(|h| h.pid() == Some(pid) && h.is_visible())
    };
    post(NETWORK_PANEL_OPEN);
    assert!(wait_until(SETTLE, panel).is_some(), "the network panel did not open");
    assert_eq!(sandbox.log_lines("network panel opened"), 1);
    post(NETWORK_PANEL_CLOSE);
    assert!(
        wait_until(SETTLE, || !panel()).is_some(),
        "the network panel did not close"
    );

    std::fs::write(sandbox.config_file(), "schema = 1\n[bar.modules]\nnetwork = false\n").unwrap();
    assert!(
        wait_until(SETTLE, || !drawn()).is_some(),
        "the network indicator stayed"
    );
    assert_eq!(sandbox.log_lines("network no longer watched"), 1);
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    assert!(
        wait_until(SETTLE, drawn).is_some(),
        "the network indicator did not come back"
    );
    assert_eq!(sandbox.log_lines("network watched"), 2);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D89, the Store's refusal of `unvirtualizedResources`: the Store's Kabuk
/// writes nothing of the look's cursors, screen saver or colours to Windows (its package
/// would keep the writes to itself), whatever the configuration asks, and says so.
#[test]
fn the_store_version_leaves_windows_settings_alone() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\ncursors = \"kabuk\"\n[system_theme]\ncolors = true\n[screensaver]\nkind = \"circuit\"\n",
    )
    .unwrap();
    // SAFETY of the environment: the tests run one at a time; it is taken away below.
    std::env::set_var(kabuk::store::PRETEND_ENV, "1");
    let shell = sandbox.shell_with_bar(&[]);
    std::env::remove_var(kabuk::store::PRETEND_ENV);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    for line in [
        "cursors not in the Store version",
        "screen saver not in the Store version",
        "Windows' colours not in the Store version",
    ] {
        assert!(
            wait_until(SETTLE, || sandbox.log_lines(line) >= 1).is_some(),
            "{line} not said"
        );
    }
    for line in [
        "cursor set applied",
        "screen saver set",
        "Windows' colours follow the look",
    ] {
        assert_eq!(sandbox.log_lines(line), 0, "{line}");
    }
    drop(shell);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D88: the system indicators' panels (calendar and notifications, quick
/// settings, privacy, keyboard language) each open under the bar and close again, and one
/// opening closes the one open, as the volume panel and the power menu do.
#[test]
fn the_system_panels_open_and_close_each_other() {
    use kabuk::bar::manager::{
        CONTROL_CLASS, MSG_TEST_CALENDAR_PANEL, MSG_TEST_KEYBOARD_PANEL, MSG_TEST_PRIVACY_PANEL, MSG_TEST_QUICK_PANEL,
    };
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |id: u32, what: usize| {
        post_message(
            control,
            Msg {
                id,
                wparam: what,
                lparam: 0,
            },
        )
        .unwrap();
    };
    let open = |class: &str| {
        kabuk_win::window::windows_of_class(class)
            .into_iter()
            .any(|h| h.pid() == Some(pid) && h.is_visible())
    };
    let panels = [
        (MSG_TEST_CALENDAR_PANEL, "Kabuk.CalendarPanel", "calendar panel opened"),
        (MSG_TEST_QUICK_PANEL, "Kabuk.QuickPanel", "quick settings opened"),
        (MSG_TEST_PRIVACY_PANEL, "Kabuk.PrivacyPanel", "privacy panel opened"),
        (MSG_TEST_KEYBOARD_PANEL, "Kabuk.KeyboardPanel", "keyboard panel opened"),
    ];
    for (i, (id, class, line)) in panels.iter().enumerate() {
        post(*id, 1);
        assert!(wait_until(SETTLE, || open(class)).is_some(), "{class} did not open");
        assert_eq!(sandbox.log_lines(line), 1, "{line}");
        // The one open before gave way.
        if i > 0 {
            let (_, prev, _) = panels[i - 1];
            assert!(
                wait_until(SETTLE, || !open(prev)).is_some(),
                "{prev} stayed open under {class}"
            );
        }
    }
    let (id, class, _) = panels[panels.len() - 1];
    post(id, 0);
    assert!(wait_until(SETTLE, || !open(class)).is_some(), "{class} did not close");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by a user with a video: dragging a level in the quick settings made the panel
/// flicker, every drawing making a new surface that entered again. Drawn again (here as
/// Windows' answers come in), the open panel keeps its surface and enters only once.
#[test]
fn an_open_panel_is_drawn_again_in_place() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_QUICK_PANEL};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    // The quick settings' own entrances (a fresh shell also greets with the shortcut
    // card, another popup).
    let entrances = || sandbox.log_lines("quick_host.rs:");
    let entered = entrances();
    post_message(
        control,
        Msg {
            id: MSG_TEST_QUICK_PANEL,
            wparam: 1,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("quick settings read") == 1).is_some(),
        "Windows' answers did not come"
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("popup drawn again") == 1).is_some(),
        "the panel was not drawn again in place"
    );
    // What came and went, for a failure's message.
    let popups = || -> String {
        std::fs::read_dir(sandbox.state_dir().join("logs"))
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .flat_map(|t| t.lines().map(str::to_string).collect::<Vec<_>>())
            .filter(|l| {
                ["popup", "panel", "card", "greet", "quick"]
                    .iter()
                    .any(|k| l.contains(k))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(entrances(), entered + 1, "the panel entered again:\n{}", popups());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D88: the show-desktop corner puts every window down without a genie, and its
/// next click brings them back; the quick settings button follows its setting live.
#[test]
fn the_show_desktop_corner_puts_windows_down_and_back() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_SHOW_DESKTOP};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let a = TestWindow::open(&["--title", &title("desk-a")]);
    let b = TestWindow::open(&["--title", &title("desk-b")]);
    for w in [&a, &b] {
        let managed = format!("managing window hwnd={}", w.hwnd.0);
        assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    }
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let click = || {
        post_message(
            control,
            Msg {
                id: MSG_TEST_SHOW_DESKTOP,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
    };
    let genies = sandbox.log_lines("genie hwnd=");
    click();
    assert!(
        wait_until(SETTLE, || a.hwnd.is_minimized() && b.hwnd.is_minimized()).is_some(),
        "the windows stayed: {:?}",
        sandbox.last_log_line(&["desktop"])
    );
    assert_eq!(sandbox.log_lines("desktop shown windows=2"), 1);
    assert_eq!(
        sandbox.log_lines("genie hwnd="),
        genies,
        "a genie played for the desktop"
    );
    click();
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_minimized() && !b.hwnd.is_minimized()).is_some(),
        "the windows did not come back"
    );
    assert_eq!(sandbox.log_lines("desktop windows back windows=2"), 1);

    // The quick settings button goes with its setting and comes back.
    let device = primary_device();
    let drawn = || {
        sandbox
            .last_log_line(&["bar drawn", &device])
            .unwrap_or_default()
            .split("icons=[")
            .nth(1)
            .and_then(|r| r.split(']').next())
            .unwrap_or_default()
            .contains("\"qs\"")
    };
    assert!(wait_until(SETTLE, drawn).is_some(), "no quick settings button");
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[bar.modules]\nquick_settings = false\n",
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || !drawn()).is_some(),
        "the quick settings button stayed"
    );
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    assert!(
        wait_until(SETTLE, drawn).is_some(),
        "the quick settings button did not come back"
    );
    drop(a);
    drop(b);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by a flaky run of the genie test: the window manager took over the open windows
/// and published them before the bars listened, nothing changed afterwards, and the bar
/// never showed their programs nor told the window manager where their icons are (a
/// genie would have poured into the wrong place). Here the bars meet it a second late.
/// (Pinned programs are drawn either way, so the bar's pictures cannot tell.)
#[test]
fn bars_that_meet_the_window_manager_late_still_show_its_windows() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let w = TestWindow::open(&["--title", &title("late-bars")]);
    let _shell = sandbox.kabuk_with(&[
        "--manage-class",
        TEST_WINDOW_CLASS,
        "--debug-attach-wm-after-ms",
        "1000",
    ]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    // Its program's icon on the bar, which the window manager hears of (the only window
    // it manages).
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["app icons known"])
            .is_some_and(|l| l.contains("icons=1")))
        .is_some(),
        "the window manager does not know the icons: {:?}",
        sandbox.last_log_line(&["app icons known"])
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D17 (decided by the user): Rıhtım's dock holds the open apps, grows with
/// them, and its last tile opens the launcher; Rıhtım's bar shows no apps.
#[test]
fn rihtims_dock_holds_the_apps_and_opens_the_launcher() {
    use kabuk_win::msgloop::{msg, post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\nlook = \"rihtim\"\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("dock placed") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let dock = shell_windows("Kabuk.Dock", pid)[0];
    // In physical pixels, as the dock's own clicks are.
    let rect = || kabuk_win::dpi::physical(|| dock.window_rect().unwrap());
    let apps_in_dock = || {
        sandbox.last_log_line(&["dock drawn"]).and_then(|l| {
            l.split("apps=")
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse::<usize>()
                .ok()
        })
    };
    // The user's own pins are there from the start.
    assert!(wait_until(SETTLE, || apps_in_dock().is_some()).is_some());
    let pinned = apps_in_dock().unwrap();
    let empty = rect();
    let w = TestWindow::open(&["--title", &title("dock-app")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    assert!(
        wait_until(SETTLE, || apps_in_dock() == Some(pinned + 1)).is_some(),
        "the app is not in the dock: {:?}",
        apps_in_dock()
    );
    let wider = rect();
    assert!(
        wider.right - wider.left > empty.right - empty.left,
        "{wider:?} vs {empty:?}"
    );
    // Centred along its edge either way.
    assert!(((wider.left + wider.right) - (empty.left + empty.right)).abs() <= 2);
    // The bar has no app pictures in Rıhtım (they have the top bit set).
    let device = primary_device();
    let line = sandbox.last_log_line(&["bar drawn", &device]).unwrap_or_default();
    let list = line
        .split("images=[")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .unwrap_or_default();
    assert!(
        !list
            .split(',')
            .filter_map(|k| k.trim().parse::<u64>().ok())
            .any(|k| k >= 1 << 63),
        "{line}"
    );
    // The last tile, at the far end of the row, opens the launcher.
    let scale = f64::from(
        kabuk_win::shell::monitors()
            .into_iter()
            .find(|m| m.primary)
            .unwrap()
            .dpi,
    ) / 96.0;
    // Once it has settled (it redraws as the app list settles).
    let mut last = rect();
    assert!(wait_until(SETTLE, || {
        std::thread::sleep(Duration::from_millis(300));
        let now = rect();
        std::mem::replace(&mut last, now) == now
    })
    .is_some());
    let r = rect();
    let inner = (f64::from(kabuk::bar::dock::STYLE.border + kabuk::bar::dock::STYLE.padding)
        + f64::from(kabuk::bar::dock::STYLE.tile) / 2.0)
        * scale;
    let x = (r.right - r.left) - kabuk_core::oled::MAX_SHIFT - inner.round() as i32;
    let y = (r.bottom - r.top) / 2;
    // Posted in physical pixels: Windows scales a DPI-unaware sender's mouse coordinates.
    kabuk_win::dpi::physical(|| {
        post_message(
            dock,
            Msg {
                id: msg::LBUTTONUP,
                wparam: 0,
                lparam: ((y as isize) << 16) | (x as isize & 0xFFFF),
            },
        )
        .unwrap()
    });
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("launcher shown") == 1).is_some(),
        "{} / {} / {r:?} scale {scale}",
        sandbox.last_log_line(&["dock click"]).unwrap_or_default(),
        sandbox.last_log_line(&["dock drawn"]).unwrap_or_default()
    );
    // Found by the user: a right click on an app in the dock did nothing. It opens the
    // app's menu, as on the bar. The test app's tile is left of the launcher's (past the
    // separator): right clicks walk there until the menu opens.
    let menus = || sandbox.log_lines("app menu opened");
    let step = (f64::from(kabuk::bar::dock::STYLE.tile) / 4.0 * scale).round() as i32;
    let mut at = x;
    while menus() == 0 && at > 0 {
        at -= step;
        kabuk_win::dpi::physical(|| {
            post_message(
                dock,
                Msg {
                    id: msg::RBUTTONUP,
                    wparam: 0,
                    lparam: ((y as isize) << 16) | (at as isize & 0xFFFF),
                },
            )
            .unwrap()
        });
        let _ = wait_until(Duration::from_millis(150), || menus() > 0);
    }
    assert_eq!(menus(), 1, "no app menu from the dock");
    assert!(
        sandbox
            .last_log_line(&["app menu opened"])
            .is_some_and(|l| l.contains("kabuk-testapp")),
        "{:?}",
        sandbox.last_log_line(&["app menu opened"])
    );
    let right_click = |x: i32| {
        kabuk_win::dpi::physical(|| {
            post_message(
                dock,
                Msg {
                    id: msg::RBUTTONUP,
                    wparam: 0,
                    lparam: ((y as isize) << 16) | (x as isize & 0xFFFF),
                },
            )
            .unwrap()
        })
    };
    let menu_open = || shell_windows("Kabuk.AppMenu", pid).iter().any(|w| w.is_visible());
    // Found by the user: with a menu open, a right click on another app only closed it.
    // One right click opens that app's menu; on the same app again it closes. (The test
    // cannot give the menu the keyboard, so Windows may close it by itself: the second
    // part runs while it is still open.)
    if pinned > 0 {
        let pitch = (f64::from(kabuk::bar::dock::STYLE.tile + kabuk::bar::dock::STYLE.gap) * scale).round() as i32;
        right_click(at - pitch);
        assert!(
            wait_until(SETTLE, || menus() == 2).is_some(),
            "another app's menu did not open"
        );
        assert!(
            sandbox
                .last_log_line(&["app menu opened"])
                .is_some_and(|l| !l.contains("kabuk-testapp")),
            "{:?}",
            sandbox.last_log_line(&["app menu opened"])
        );
        if menu_open() {
            right_click(at - pitch);
            assert!(
                wait_until(SETTLE, || !menu_open()).is_some(),
                "the same app's menu stayed"
            );
            assert_eq!(menus(), 2, "opened again");
        }
    }
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D20 (decided by the user): with `appearance.cursors = "kabuk"` the look's
/// set becomes the user's cursors, with their own recorded first; `--restore` gives
/// theirs back exactly.
#[test]
fn the_looks_cursor_set_is_used_and_the_users_come_back() {
    const NAMES: [&str; 2] = ["Arrow", "Wait"];
    let (before, before_source) = kabuk_win::cursors::current(&NAMES);
    let areas = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[appearance]\ncursors = \"kabuk\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("cursor set applied") == 1).is_some(),
        "{}",
        sandbox.last_log_line(&["cursor"]).unwrap_or_default()
    );
    let state_dir = sandbox.state_dir().display().to_string();
    let (now, _) = kabuk_win::cursors::current(&NAMES);
    for (name, value) in now.iter().skip(1) {
        let v = value.clone().unwrap_or_default();
        assert!(v.starts_with(&state_dir), "{name} = {v}");
        assert!(std::path::Path::new(&v).exists(), "{v}");
    }
    assert_eq!(now[0].1.as_deref(), Some("Kabuk"), "the scheme's name");
    let recorded = sandbox.state().cursors.expect("the user's cursors recorded");
    for value in &before {
        assert!(recorded.values.contains(value), "{value:?} not recorded");
    }
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::cursors::current(&NAMES), (before, before_source));
    assert!(wait_until(Duration::from_secs(5), || restored(&areas)).is_some());
}

/// DECISIONS D41 (decided by the user): a style, colour and size picked apart from the
/// look are used, at Windows' pointer size for that size, follow the file live, and the
/// user's cursors and pointer size come back exactly.
#[test]
fn a_picked_cursor_style_and_size_are_used_and_the_users_come_back() {
    const NAMES: [&str; 2] = ["Arrow", "IBeam"];
    let (before, before_source) = kabuk_win::cursors::current(&NAMES);
    let before_size = kabuk_win::cursors::base_size();
    let before_in_use = kabuk_win::cursors::base_size_in_use();
    let areas = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\ncursors = \"custom\"\ncursor_style = \"block\"\ncursor_color = \"accent\"\ncursor_size = \"large\"\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("cursor set applied") == 1).is_some(),
        "{}",
        sandbox.last_log_line(&["cursor"]).unwrap_or_default()
    );
    assert_eq!(kabuk_win::cursors::base_size(), Some(48));
    assert_eq!(
        kabuk_win::cursors::base_size_in_use(),
        Some(48),
        "Windows loads them at that size"
    );
    assert_eq!(sandbox.log_lines("size=48"), 1);
    let (now, _) = kabuk_win::cursors::current(&NAMES);
    let state_dir = sandbox.state_dir().display().to_string();
    for (name, value) in now.iter().skip(1) {
        let v = value.clone().unwrap_or_default();
        assert!(v.starts_with(&state_dir), "{name} = {v}");
    }
    let recorded = sandbox.state().cursors.expect("the user's cursors recorded");
    assert!(recorded.base_size_recorded);
    assert_eq!(recorded.base_size, before_size);
    // Live: another size from the file, the user's size still the one recorded.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[appearance]\ncursors = \"custom\"\ncursor_style = \"slim\"\ncursor_size = \"huge\"\n",
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || kabuk_win::cursors::base_size_in_use() == Some(64)).is_some(),
        "the new size not applied"
    );
    assert_eq!(sandbox.state().cursors.expect("still recorded").base_size, before_size);
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::cursors::current(&NAMES), (before, before_source));
    assert_eq!(kabuk_win::cursors::base_size(), before_size);
    assert_eq!(kabuk_win::cursors::base_size_in_use(), before_in_use);
    assert!(wait_until(Duration::from_secs(5), || restored(&areas)).is_some());
}

/// DECISIONS D42 (decided by the user): with the colour layer, Windows' mode follows
/// Kabuk's and its accent colour the look's accent as the look shows it; Windows' own
/// values are recorded first, come back when the switch goes off (live) and with
/// `--restore`, exactly, absent ones included.
#[test]
fn windows_colours_follow_the_look_and_come_back() {
    const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
    let before = kabuk_win::system_colors::current();
    let areas = work_areas();
    let sandbox = Sandbox::new();
    let on = "schema = 1\n[appearance]\nlook = \"ada\"\nmode = \"light\"\naccent = \"#ff7eb6\"\n[system_theme]\ncolors = true\n";
    std::fs::write(sandbox.config_file(), on).unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    let applied = || sandbox.log_lines("Windows' colours follow the look");
    assert!(
        wait_until(STARTUP_TIMEOUT, || applied() == 1).is_some(),
        "{}",
        sandbox.last_log_line(&["colours"]).unwrap_or_default()
    );
    let expected = kabuk_theme::resolve::resolve("a", kabuk_theme::Mode::Light, Some("#ff7eb6"))
        .unwrap()
        .tokens
        .color("accent-s")
        .unwrap();
    let dword = |key: &str, name: &str| kabuk_win::registry::read_dword(key, name);
    assert_eq!(dword(PERSONALIZE, "AppsUseLightTheme"), Some(1));
    assert_eq!(dword(PERSONALIZE, "SystemUsesLightTheme"), Some(1));
    assert_eq!(
        dword(ACCENT, "AccentColorMenu"),
        Some(kabuk_theme::windows_accent::abgr(kabuk_theme::Color {
            a: 1.0,
            ..expected
        }))
    );
    assert_eq!(sandbox.state().system_colors, Some(before.clone()), "recorded first");
    // Off from the file: Windows' own come back at once.
    std::fs::write(sandbox.config_file(), on.replace("colors = true", "colors = false")).unwrap();
    let differ = || -> Vec<String> {
        kabuk_win::system_colors::current()
            .into_iter()
            .zip(&before)
            .filter(|(now, was)| now != *was)
            .map(|(now, was)| format!("{}: {:?} (was {:?})", now.name, now.data, was.data))
            .collect()
    };
    assert!(
        // A restore watches the DWM's words for a while before the record goes.
        wait_until(SETTLE + Duration::from_secs(3), || differ().is_empty()
            && sandbox.state().system_colors.is_none())
        .is_some(),
        "not given back live: {:?}",
        differ()
    );
    // On again, then `--restore`.
    std::fs::write(sandbox.config_file(), on).unwrap();
    assert!(wait_until(SETTLE, || applied() == 2).is_some(), "not set again");
    assert_eq!(sandbox.restore().0, 0);
    assert!(
        wait_until(SETTLE, || differ().is_empty()).is_some(),
        "not given back by --restore: {:?}",
        differ()
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&areas)).is_some());
}

/// DECISIONS D30 (asked for by the user): a managed window's title bar takes the look's
/// colours and corners, recorded first; turning the setting off and `--restore` give
/// Windows' own back. DWM reads back only the corners; the colours are checked through
/// the calls DWM accepted (logged) and the state file's record.
#[test]
fn other_programs_frames_follow_the_look_and_come_back() {
    // DWMWCP_DEFAULT and DWMWCP_ROUND.
    const DEFAULT_CORNERS: u32 = 0;
    const ROUND: u32 = 2;
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--title", &title("frame-look")]);
    let managed = format!("managing window hwnd={}", w.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    let hwnd = format!("hwnd={}", w.hwnd.0);
    assert!(
        wait_until(SETTLE, || w.hwnd.corner_preference() == Some(ROUND)).is_some(),
        "{:?}",
        w.hwnd.corner_preference()
    );
    let applied = sandbox
        .last_log_line(&["frame follows the look", &hwnd])
        .expect("frame applied");
    let tokens = kabuk_theme::resolve::resolve("a", kabuk_theme::Mode::Dark, None)
        .unwrap()
        .tokens;
    let c = tokens.color("win-bg").unwrap();
    assert!(
        applied.contains(&format!("caption: ({}, {}, {})", c.r, c.g, c.b)),
        "{applied}"
    );
    assert!(
        sandbox.state().styled_windows.iter().any(|s| s.hwnd == w.hwnd.0),
        "not recorded"
    );

    std::fs::write(sandbox.config_file(), "schema = 1\n[windows]\nfollow_look = false\n").unwrap();
    assert!(wait_until(SETTLE, || w.hwnd.corner_preference() == Some(DEFAULT_CORNERS)).is_some());
    assert!(sandbox
        .last_log_line(&["frame back to Windows'", &hwnd])
        .is_some_and(|l| l.contains("reset=true")));
    // The record goes once the frame is back (the log line comes first).
    assert!(wait_until(SETTLE, || sandbox.state().styled_windows.is_empty()).is_some());

    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    assert!(wait_until(SETTLE, || w.hwnd.corner_preference() == Some(ROUND)).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        w.hwnd.corner_preference(),
        Some(DEFAULT_CORNERS),
        "--restore left the frame styled"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// SPEC §5: dimmed, every monitor and its bar get a click-through overlay; the next
/// input lifts it (posted here as the `WM_INPUT` raw input sends; no input is made).
/// When to dim is checked with a fake clock in `kabuk_core::oled`.
#[test]
fn dimming_covers_every_monitor_and_input_lifts_it() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_INPUT, MSG_TEST_DIM};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |id: u32| {
        post_message(
            control,
            Msg {
                id,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap()
    };
    let overlays = || {
        kabuk_win::window::windows_of_class("Kabuk.Dim")
            .into_iter()
            .filter(|w| w.pid() == Some(pid) && w.is_visible())
            .count()
    };
    let bars = shell_windows("Kabuk.Bar", pid).len();
    post(MSG_TEST_DIM);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("dimmed after no input") == 1).is_some(),
        "{}",
        sandbox.last_log_line(&["dim"]).unwrap_or_default()
    );
    assert_eq!(overlays(), 2 * bars, "a monitor and a bar overlay per bar");
    post(MSG_INPUT);
    assert!(wait_until(SETTLE, || overlays() == 0).is_some(), "still dimmed");
    assert!(wait_until(SETTLE, || sandbox.log_lines("dimming lifted") == 1).is_some());
    // Regression: the wait restarted from Windows' idle time, which posted input does not
    // move, so on a machine left alone for the dim delay it dimmed again at once.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(overlays(), 0, "dimmed again right after the input");
    assert_eq!(sandbox.log_lines("dimmed after no input"), 1);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// SPEC §2.7 and PLAN M9, M12: the chosen screen saver is set in Windows' settings
/// (Kabuk's own saver installed in the state folder, and the wait), the user's own
/// recorded first; `--restore` gives them back exactly.
#[test]
fn the_screen_saver_choice_is_set_in_windows_and_comes_back() {
    let before = kabuk_win::screensaver::current();
    let areas = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[screensaver]\nkind = \"constellation\"\nwait_minutes = 3\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("screen saver set") == 1).is_some(),
        "{}",
        sandbox.last_log_line(&["screen saver"]).unwrap_or_default()
    );
    let (now, theme) = kabuk_win::screensaver::current();
    let value = |name: &str| now.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.clone());
    let saver = value("SCRNSAVE.EXE").expect("a saver set");
    assert!(std::path::Path::new(&saver).exists(), "{saver}");
    assert_eq!(
        std::fs::canonicalize(&saver).unwrap(),
        std::fs::canonicalize(sandbox.state_dir().join("kabuk-saver.scr")).unwrap()
    );
    // The program itself, copied (it reads its scene from kabuk.toml).
    let built = std::fs::metadata(kabuk_exe().with_file_name("kabuk-saver.exe"))
        .unwrap()
        .len();
    assert_eq!(std::fs::metadata(&saver).unwrap().len(), built);
    assert_eq!(value("ScreenSaveTimeOut").as_deref(), Some("180"));
    assert_eq!(value("ScreenSaveActive").as_deref(), Some("1"));
    assert_eq!(theme, before.1, "OledSaver's own theme left alone");
    assert!(sandbox.state().saver.is_some(), "the user's settings recorded");
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(kabuk_win::screensaver::current(), before);
    assert!(wait_until(Duration::from_secs(5), || restored(&areas)).is_some());
}

/// DECISIONS D66 (suggested and asked for by the user): the media card opens under the
/// bar and closes again, whether or not something plays on the machine.
#[test]
fn the_media_card_opens_and_closes() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_MEDIA_CARD};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let card = || {
        kabuk_win::window::windows_of_class("Kabuk.MediaCard")
            .into_iter()
            .find(|w| w.pid() == Some(pid) && w.is_visible())
    };
    let post = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_MEDIA_CARD,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    post(1);
    assert!(wait_until(SETTLE, || card().is_some()).is_some(), "no card");
    assert_eq!(sandbox.log_lines("media card opened"), 1);
    let r = card().unwrap().window_rect().unwrap();
    let m = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    assert!(m.rect.contains(&r), "{r:?} off {:?}", m.rect);
    post(0);
    assert!(wait_until(SETTLE, || card().is_none()).is_some(), "the card stayed");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D61 (suggested and asked for by the user): the shortcut card opens in the
/// middle of the monitor, as the card or as the greeting, and closes again.
#[test]
fn the_shortcut_card_opens_in_the_middle_and_closes() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_KEYS_CARD};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let card = || {
        kabuk_win::window::windows_of_class("Kabuk.KeysCard")
            .into_iter()
            .find(|w| w.pid() == Some(pid) && w.is_visible())
    };
    let post = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_KEYS_CARD,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    // No greeting in a test: the shell runs without its shortcuts.
    assert_eq!(sandbox.log_lines("keys card shown"), 0);
    post(1);
    assert!(wait_until(SETTLE, || card().is_some()).is_some(), "no card");
    assert_eq!(sandbox.log_lines("keys card shown welcome=false"), 1);
    let r = card().unwrap().window_rect().unwrap();
    let m = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.work_area.left <= r.left && r.right <= m.work_area.right)
        .expect("on a monitor");
    let (cx, mx) = ((r.left + r.right) / 2, (m.work_area.left + m.work_area.right) / 2);
    let (cy, my) = ((r.top + r.bottom) / 2, (m.work_area.top + m.work_area.bottom) / 2);
    assert!(
        (cx - mx).abs() <= 1 && (cy - my).abs() <= 1,
        "{r:?} in {:?}",
        m.work_area
    );
    post(0);
    assert!(wait_until(SETTLE, || card().is_none()).is_some(), "the card stayed");
    post(2);
    assert!(wait_until(SETTLE, || sandbox.log_lines("keys card shown welcome=true") == 1).is_some());
    post(0);
    assert!(wait_until(SETTLE, || card().is_none()).is_some(), "the greeting stayed");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Asked for by the user (off by default): a hot corner reached does what was chosen
/// for it. The mouse hook tells the bar's control window; here the test does, as tests
/// never move the mouse: the grid of every app opens and closes, and no action or an
/// unknown one does nothing.
#[test]
fn a_hot_corner_reached_does_what_was_chosen() {
    use kabuk::bar::manager::CONTROL_CLASS;
    use kabuk::grab::MSG_HOT_CORNER;
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[hot_corners]\ntop_left = \"app_grid\"\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    sandbox.wait_started();
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_HOT_CORNER,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    let none = kabuk_core::config::HotCornerAction::ALL
        .iter()
        .position(|a| *a == kabuk_core::config::HotCornerAction::None)
        .unwrap();
    let grid = kabuk_core::config::HotCornerAction::ALL
        .iter()
        .position(|a| *a == kabuk_core::config::HotCornerAction::AppGrid)
        .unwrap();
    post(none);
    post(99);
    assert!(wait_until(SETTLE, || sandbox.log_lines("hot corner reached") == 1).is_some());
    post(grid);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    post(grid);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == 1).is_some());
    assert_eq!(sandbox.log_lines("app grid shown"), 1);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user: editing the app grid, after one app was hidden the next badge
/// pressed hid another app, and a press on an app ended editing (presses were found at
/// the places of the slide's first frame). Pressed where they are drawn, badges hide the
/// app drawn there, one after another, and a press on an app keeps editing.
#[test]
fn the_app_grids_badges_hide_the_app_drawn_under_them() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_GRID};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let grid = |what: usize, place: isize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_GRID,
                wparam: what,
                lparam: place,
            },
        )
        .unwrap()
    };
    // The apps of the page on screen, in order, as the grid says them.
    let shown = || -> Vec<String> {
        sandbox
            .last_log_line(&["app grid page"])
            .and_then(|l| {
                l.split("apps=")
                    .nth(1)
                    .map(|a| a.split(" | ").map(|n| n.trim().to_string()).collect())
            })
            .unwrap_or_default()
    };
    grid(1, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    // Every icon is in first: an icon coming later draws the grid again, which would
    // hide a press map left behind.
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["app grid pictures arrived"])
            .is_some_and(|l| l.contains("waiting=0")))
        .is_some(),
        "the icons"
    );
    grid(2, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid editing on=true") == 1).is_some());
    // The bug lived in the apps' slide, which only plays while they wiggle.
    assert!(
        sandbox.log_lines("app grid editing on=true wiggles=true") == 1,
        "no wiggle here (Windows' animation effects off?)"
    );
    let apps = shown();
    assert!(apps.len() >= 4, "{apps:?}");
    // The first badge, then the first badge again: the app now drawn first.
    for (n, name) in apps.iter().take(2).enumerate() {
        grid(3, 0);
        assert!(
            wait_until(SETTLE, || sandbox.log_lines("app grid app hidden") == n + 1).is_some(),
            "badge {n}"
        );
        let hidden = sandbox.last_log_line(&["app grid app hidden"]).unwrap();
        assert!(hidden.ends_with(&format!("app={name}")), "{hidden} (wanted {name})");
        // The others slide into the gap; the next press waits until they are there.
        std::thread::sleep(Duration::from_millis(400));
    }
    // A press on an app while editing keeps editing.
    grid(4, 1);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid test press") == 3).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(sandbox.log_lines("app grid editing on=false"), 0, "editing ended");
    grid(0, 0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user in a video: editing the app grid drew its apps over the desktop, the
/// grid's blurred picture gone, and the icons blank for a moment. A tray icon's change
/// cleared every picture but the tray's and the bar's apps' from the store. The grid
/// keeps its pictures while the tray changes.
#[test]
fn the_app_grid_keeps_its_pictures_while_the_tray_changes() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_GRID};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    // The tray is taken from Explorer for a few seconds, as the tray tests do.
    let _shell = sandbox.kabuk_with(&[TAKE_EXPLORER, "--no-wm"]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("tray window up") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let grid = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_GRID,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    grid(1);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    assert!(
        sandbox
            .last_log_line(&["app grid shown"])
            .is_some_and(|l| l.contains("wallpaper=true")),
        "the grid has its picture"
    );
    // A program's tray icon comes while the grid is open.
    let log = sandbox.dir.path().join("tray-calls.txt");
    let tip = title("tray-keeps");
    let _app = TestWindow::open(&["--title", &tip, "--tray-log", log.to_str().unwrap()]);
    assert!(wait_until(SETTLE, || sandbox.last_log_line(&["tray call", &tip]).is_some()).is_some());
    grid(2);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid editing on=true") == 1).is_some());
    assert_eq!(sandbox.log_lines("app grid picture lost"), 0);
    grid(0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// PLAN M19 P14: every opening of the app grid listed, sorted and indexed Windows' apps
/// again. They are listed once and kept: a second opening lists nothing, and a change of
/// the arrangement (Settings writes its file) lists them again.
#[test]
fn a_second_opening_of_the_app_grid_lists_no_apps() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_GRID};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    sandbox.wait_started();
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let grid = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_GRID,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    let open_and_close = |n: usize| {
        grid(1);
        assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == n).is_some());
        grid(0);
        assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == n).is_some());
    };
    open_and_close(1);
    open_and_close(2);
    assert_eq!(
        sandbox.log_lines("app grid apps listed"),
        1,
        "listed again: {:?}",
        sandbox.last_log_line(&["app grid apps listed"])
    );
    // Settings hides an app: the next opening lists them again, once.
    let mut hidden = kabuk_core::app_grid::Arrangement::default();
    hidden.hide("shell:AppsFolder\\kabuk-test-none", "None");
    std::fs::write(sandbox.state_dir().join(kabuk_core::app_grid::FILE), hidden.to_json()).unwrap();
    open_and_close(3);
    open_and_close(4);
    assert_eq!(sandbox.log_lines("app grid apps listed"), 2);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D68 (asked for by the user, who will try it on a touchpad): with swipes on,
/// the bar's control window watches the touchpad, and a swipe does what was chosen for
/// its way; turned off live, the watch goes. This machine has no touchpad, so the test
/// sends the swipe the fingers would make.
#[test]
fn a_touchpad_swipe_does_what_was_chosen() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_SWIPE};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    let on = "schema = 1\n[gestures]\nenabled = true\nswipe_up = \"app_grid\"\nswipe_down = \"none\"\n";
    std::fs::write(sandbox.config_file(), on).unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    sandbox.wait_started();
    assert!(wait_until(SETTLE, || sandbox.log_lines("touchpad watched on=true") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let swipe = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_SWIPE,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    // Down: nothing chosen. Up: the grid of every app, and again to close it.
    swipe(3);
    assert!(wait_until(SETTLE, || sandbox.log_lines("touchpad swipe") == 1).is_some());
    swipe(2);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    swipe(2);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == 1).is_some());
    // Off, live: the watch goes.
    std::fs::write(sandbox.config_file(), "schema = 1\n[gestures]\nenabled = false\n").unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("touchpad watched on=false") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Reported by the user: a click on the volume icon showed no slider. The left click's
/// panel (the level's track, the speaker, Windows' mixer and sound settings) and the right
/// click's menu (the entries only) open under the volume and close again. No test sets
/// the user's volume, so nothing on them is clicked.
#[test]
fn the_volume_panel_and_menu_open_and_close() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_VOLUME_PANEL};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let panel = || {
        kabuk_win::window::windows_of_class("Kabuk.VolumePanel")
            .into_iter()
            .find(|w| w.pid() == Some(pid) && w.is_visible())
    };
    let post = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_VOLUME_PANEL,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    post(1);
    assert!(
        wait_until(SETTLE, || panel().is_some()).is_some(),
        "the panel did not open"
    );
    let height = |w: kabuk_win::window::Hwnd| w.window_rect().map(|r| r.bottom - r.top).unwrap_or(0);
    let with_track = height(panel().unwrap());
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("volume panel opened menu=Volume with_track=true")
        == 1)
    .is_some());
    post(0);
    assert!(wait_until(SETTLE, || panel().is_none()).is_some(), "the panel stayed");
    post(2);
    assert!(
        wait_until(SETTLE, || panel().is_some()).is_some(),
        "the menu did not open"
    );
    let menu = height(panel().unwrap());
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("volume panel opened menu=Volume with_track=false")
        == 1)
    .is_some());
    // The menu has no track, so it is shorter than the panel.
    assert!(0 < menu && menu < with_track, "{menu} vs {with_track}");
    post(0);
    assert!(wait_until(SETTLE, || panel().is_none()).is_some(), "the menu stayed");
    // The clock's menu (a right click on it, as on Windows' taskbar).
    post(3);
    assert!(
        wait_until(SETTLE, || panel().is_some()).is_some(),
        "the clock's menu did not open"
    );
    assert!(wait_until(SETTLE, || sandbox.log_lines("volume panel opened menu=Clock") == 1).is_some());
    post(0);
    assert!(
        wait_until(SETTLE, || panel().is_none()).is_some(),
        "the clock's menu stayed"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// The power button's menu opens beside the bar and closes again (its entries act on the
/// session, so no test clicks them).
#[test]
fn the_power_menu_opens_and_closes() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_POWER_MENU};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let menu = || {
        kabuk_win::window::windows_of_class("Kabuk.PowerMenu")
            .into_iter()
            .any(|w| w.pid() == Some(pid) && w.is_visible())
    };
    let post = |open: bool| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_POWER_MENU,
                wparam: usize::from(open),
                lparam: 0,
            },
        )
        .unwrap()
    };
    post(true);
    assert!(wait_until(SETTLE, menu).is_some(), "the menu did not open");
    assert_eq!(sandbox.log_lines("power menu opened"), 1);
    // M10: it enters quickly and, as it holds the keyboard, leaves at once (D33).
    let enters = sandbox.last_log_line(&["popup enters"]).expect("entrance logged");
    let expected = if kabuk_win::system::animations_on() {
        "ms=180"
    } else {
        "ms=0"
    };
    assert!(enters.contains(expected), "{enters}");
    post(false);
    assert!(wait_until(SETTLE, || !menu()).is_some(), "the menu stayed");
    assert_eq!(sandbox.log_lines("popup leaves"), 0);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// M12 (SPEC §2.8): a still wallpaper is set on every monitor while Kabuk runs, the
/// user's own recorded first; choosing "Windows" again gives it back at once, and so does
/// `--restore`. The user's wallpaper changes for a few seconds (approved on 2026-10-13).
#[test]
fn a_still_wallpaper_is_set_and_the_users_own_comes_back() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    assert!(
        !kabuk_win::wallpaper::slideshow_on(),
        "a slideshow runs: Kabuk would leave it alone"
    );
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"black\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let monitors = kabuk_win::shell::monitors().len();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("wallpaper set") == monitors).is_some(),
        "{:?}",
        sandbox.last_log_line(&["wallpaper"])
    );
    // Recorded before it was changed, exactly as it was.
    assert_eq!(sandbox.state().wallpaper.as_ref(), Some(&theirs));
    let now = kabuk_win::wallpaper::current().unwrap();
    assert!(
        now.monitors
            .iter()
            .all(|(_, p)| p.as_deref().is_some_and(|p| p.ends_with("wallpaper-black.bmp"))),
        "{now:?}"
    );
    // "Windows" again: the user's own is back while Kabuk runs, and nothing is recorded.
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"windows\"\n").unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("the user's wallpaper is back") == 1).is_some());
    assert_eq!(kabuk_win::wallpaper::current().as_ref(), Some(&theirs));
    assert!(wait_until(SETTLE, || sandbox.state().wallpaper.is_none()).is_some());
    // Set again, then given back by --restore.
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"black\"\n").unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("wallpaper set") == 2 * monitors).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D47 (decided by the user): the user's own still picture drifts: each monitor
/// shows one of its four places, made once, and the next step shows the next place
/// without the user's own wallpaper between; `--restore` gives theirs back.
#[test]
fn a_still_picture_drifts_a_step_at_a_time() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_DRIFT};
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    assert!(!kabuk_win::wallpaper::slideshow_on(), "a slideshow runs");
    let before = work_areas();
    let sandbox = Sandbox::new();
    // A picture of the test's own: stripes, so each place differs.
    let picture = sandbox.dir.path().join("mine.jpg");
    let (w, h) = (64u32, 36u32);
    let px: Vec<u8> = (0..h)
        .flat_map(|_| (0..w).flat_map(|x| [(x * 4) as u8, 80, 200]))
        .collect();
    kabuk_win::picture::save_jpeg(&picture, w, h, &px).unwrap();
    let config = format!(
        "schema = 1\n[wallpaper]\nkind = \"image\"\nimage = '{}'\n",
        picture.display()
    );
    std::fs::write(sandbox.config_file(), config).unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    let monitors = kabuk_win::shell::monitors().len();
    let set = || sandbox.log_lines("wallpaper set");
    assert!(
        wait_until(STARTUP_TIMEOUT, || set() == monitors).is_some(),
        "{:?}",
        sandbox.last_log_line(&["wallpaper", "drift"])
    );
    let places = || -> Vec<String> {
        kabuk_win::wallpaper::current()
            .unwrap()
            .monitors
            .iter()
            .map(|(_, p)| p.clone().unwrap_or_default())
            .collect()
    };
    let first = places();
    assert!(
        first
            .iter()
            .all(|p| p.contains("wallpaper-drift-") && p.ends_with(".jpg")),
        "{first:?}"
    );
    let pid = sandbox.state().kabuk_pid.unwrap();
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|c| c.pid() == Some(pid))
        .expect("bar control window");
    kabuk_win::msgloop::post_message(
        control,
        kabuk_win::msgloop::Msg {
            id: MSG_TEST_DRIFT,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(wait_until(SETTLE, || set() == 2 * monitors).is_some(), "no step");
    let second = places();
    for (a, b) in first.iter().zip(&second) {
        assert_ne!(a, b, "the same place after a step");
        assert!(b.contains("wallpaper-drift-"), "{b}");
    }
    // The user's own did not show between the steps.
    assert_eq!(sandbox.log_lines("the user's wallpaper is back"), 0);
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// PLAN M19 U11: the drift's step set in Settings. A changed step with the same picture
/// left the timer waiting for the old one.
#[test]
fn a_changed_drift_step_is_waited_for_at_once() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    assert!(!kabuk_win::wallpaper::slideshow_on(), "a slideshow runs");
    let before = work_areas();
    let sandbox = Sandbox::new();
    let picture = sandbox.dir.path().join("mine.jpg");
    let (w, h) = (64u32, 36u32);
    let px: Vec<u8> = (0..h)
        .flat_map(|_| (0..w).flat_map(|x| [(x * 4) as u8, 80, 200]))
        .collect();
    kabuk_win::picture::save_jpeg(&picture, w, h, &px).unwrap();
    let config = |minutes: u32| {
        format!(
            "schema = 1\n[wallpaper]\nkind = \"image\"\nimage = '{}'\ndrift_minutes = {minutes}\n",
            picture.display()
        )
    };
    std::fs::write(sandbox.config_file(), config(120)).unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    let scheduled = || sandbox.log_lines("wallpaper drift scheduled");
    assert!(
        wait_until(STARTUP_TIMEOUT, || scheduled() > 0).is_some(),
        "no drift scheduled"
    );
    let was = scheduled();
    std::fs::write(sandbox.config_file(), config(1)).unwrap();
    assert!(
        wait_until(SETTLE, || scheduled() > was).is_some(),
        "the new step was not waited for"
    );
    let line = sandbox.last_log_line(&["wallpaper drift scheduled"]).unwrap();
    let secs: u64 = line
        .split("in_secs=")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{line}"));
    assert!(secs <= 60, "{line}");
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// PLAN M13: a created wallpaper chosen in `[wallpaper]` starts the scene player behind
/// the icons, which compiles it and draws it on every monitor; one that is missing from
/// the state folder opens no window, so Windows' wallpaper under it shows its black.
#[test]
fn a_created_wallpaper_plays_behind_the_icons() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    let before = work_areas();
    let sandbox = Sandbox::new();
    let wall = sandbox.state_dir().join("walls").join("gen-1");
    std::fs::create_dir_all(&wall).unwrap();
    std::fs::write(wall.join("wall.toml"), kabuk_wallgen::prompt::SAMPLE_TOML).unwrap();
    std::fs::write(wall.join("wall.hlsl"), kabuk_wallgen::prompt::SAMPLE_HLSL).unwrap();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"generated\"\ngenerated = \"gen-1\"\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let monitors = kabuk_win::shell::monitors().len();
    let played = |targets: usize, created: usize| {
        sandbox
            .last_log_line(&["scenes play", "Wallpaper"])
            .is_some_and(|l| l.contains(&format!("targets={targets} ")) && l.contains(&format!("created={created}")))
    };
    assert!(
        wait_until(SETTLE, || played(monitors, monitors)).is_some(),
        "{:?}",
        sandbox.last_log_line(&["scenes play"])
    );
    // Another id that is not there: the player starts again and draws nothing.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"generated\"\ngenerated = \"gen-2\"\n",
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("animated wallpaper started") == 2).is_some());
    assert!(
        wait_until(SETTLE, || played(0, 0)).is_some(),
        "{:?}",
        sandbox.last_log_line(&["scenes play"])
    );
    assert!(sandbox.log_lines("created wallpaper not loaded") >= 1);
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// DECISIONS D58: a live design of the gallery plays behind the icons on every monitor;
/// a still one becomes Windows' picture, drawn once at each monitor's size into the
/// state folder (and the player stops); the user's own comes back after.
#[test]
fn the_gallery_plays_live_designs_and_draws_still_ones() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"gallery\"\ngallery = \"wall-live/aurora-shore\"\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let monitors = kabuk_win::shell::monitors();
    let played = |targets: usize, created: usize| {
        sandbox
            .last_log_line(&["scenes play", "Wallpaper"])
            .is_some_and(|l| l.contains(&format!("targets={targets} ")) && l.contains(&format!("created={created}")))
    };
    assert!(
        wait_until(SETTLE, || played(monitors.len(), monitors.len())).is_some(),
        "{:?}",
        sandbox.last_log_line(&["scenes play"])
    );
    // A still design: one picture per monitor size, set as Windows' wallpaper.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"gallery\"\ngallery = \"wall-still/agate-slice\"\n",
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("animated wallpaper stopped") >= 1).is_some(),
        "the player went on"
    );
    let pictures = || -> Vec<String> {
        std::fs::read_dir(sandbox.state_dir())
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with(kabuk_core::wallpaper::GALLERY_PREFIX))
                    .collect()
            })
            .unwrap_or_default()
    };
    // Each monitor's picture is drawn in turn; every one is set once all are.
    let all_set = || {
        kabuk_win::wallpaper::current().is_some_and(|w| {
            w.monitors.iter().all(|(_, p)| {
                p.as_deref().is_some_and(|p| {
                    p.contains("wallpaper-gallery-wall-still-agate-slice-") && kabuk_core::wallpaper::is_kabuks(p)
                })
            })
        })
    };
    assert!(
        wait_until(SETTLE, all_set).is_some(),
        "{:?}",
        kabuk_win::wallpaper::current()
    );
    let made = pictures();
    for m in &monitors {
        let size = format!("-{}x{}-", m.rect.right - m.rect.left, m.rect.bottom - m.rect.top);
        assert!(made.iter().any(|n| n.contains(&size)), "{size} not in {made:?}");
    }
    // An unknown design: the user's own stays, nothing breaks.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"gallery\"\ngallery = \"wall-still/nope\"\n",
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("no such gallery design") >= 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// SPEC §2.8: the animated wallpaper pauses in game mode. The player stops while a
/// full-screen program covers a monitor (the shell's ABN_FULLSCREENAPP is posted by the
/// test, as Explorer would send it) and starts again when the program leaves full screen.
#[test]
fn the_animated_wallpaper_stops_in_game_mode() {
    use kabuk::bar::manager::MSG_APPBAR;
    use kabuk_win::msgloop::{post_message, Msg};
    const ABN_FULLSCREENAPP: usize = 2;
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"constellation\"\n",
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") >= 1).is_some());
    let started = || sandbox.log_lines("animated wallpaper started");
    assert!(wait_until(SETTLE, || started() == 1).is_some(), "not started");
    let pid = sandbox
        .last_log_line(&["animated wallpaper started"])
        .and_then(|l| {
            l.split_whitespace()
                .find_map(|w| w.strip_prefix("pid="))
                .and_then(|p| p.parse::<u32>().ok())
        })
        .expect("the player's pid");
    let player = kabuk_win::process::Process::open(pid).expect("the player runs");
    let kabuk = sandbox.state().kabuk_pid.expect("pid");
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let main_bar = shell_windows("Kabuk.Bar", kabuk)
        .into_iter()
        .find(|w| {
            w.window_rect().is_some_and(|r| {
                let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                primary.rect.left <= cx && cx < primary.rect.right && primary.rect.top <= cy && cy < primary.rect.bottom
            })
        })
        .expect("primary bar");
    let notify = |entering: bool| {
        post_message(
            main_bar,
            Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };

    let game = TestWindow::open(&["--fullscreen", "--title", &title("wallpaper-game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    notify(true);
    assert!(
        player.wait(Some(SETTLE)),
        "the player kept running in game mode: {:?}",
        sandbox.last_log_line(&["full-screen check"])
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("animated wallpaper stopped game=true")
            == 1)
        .is_some()
    );

    kabuk_win::dpi::physical(|| {
        let r = kabuk_core::state::Rect {
            left: primary.work_area.left + 100,
            top: primary.work_area.top + 100,
            right: primary.work_area.left + 900,
            bottom: primary.work_area.top + 700,
        };
        assert!(game.hwnd.place_frame_async(r));
    });
    assert!(wait_until(SETTLE, || game
        .hwnd
        .window_rect()
        .is_some_and(|r| r.right - r.left < 1000))
    .is_some());
    notify(false);
    assert!(
        wait_until(SETTLE, || started() == 2).is_some(),
        "the player did not come back after game mode"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// M12 (D36): choosing an animated wallpaper starts Kabuk's scene player behind the
/// icons (Windows' own wallpaper turns pure black under it); choosing "Windows" again
/// stops it and gives the user's wallpaper back; and the player ends with Kabuk.
#[test]
fn an_animated_wallpaper_runs_while_chosen_and_ends_with_kabuk() {
    kabuk_win::system::com_init();
    let theirs = kabuk_win::wallpaper::backup().expect("Windows' wallpaper readable");
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"circuit\"\n").unwrap();
    let shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = || {
        sandbox.last_log_line(&["animated wallpaper started"]).and_then(|l| {
            l.split_whitespace()
                .find_map(|w| w.strip_prefix("pid="))
                .and_then(|p| p.parse::<u32>().ok())
        })
    };
    assert!(wait_until(SETTLE, || pid().is_some()).is_some(), "not started");
    let player = kabuk_win::process::Process::open(pid().unwrap()).expect("the player runs");
    assert!(
        !player.wait(Some(Duration::from_millis(500))),
        "the player ended by itself"
    );
    // "Windows" again: the player stops and the user's wallpaper is back.
    std::fs::write(sandbox.config_file(), "schema = 1\n[wallpaper]\nkind = \"windows\"\n").unwrap();
    assert!(player.wait(Some(SETTLE)), "the player kept running");
    assert!(wait_until(SETTLE, || sandbox.log_lines("animated wallpaper stopped") == 1).is_some());
    assert_eq!(kabuk_win::wallpaper::current().as_ref(), Some(&theirs));
    // Chosen again, then Kabuk is killed: the player sees it and ends within its look.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[wallpaper]\nkind = \"constellation\"\n",
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("animated wallpaper started") == 2).is_some());
    let second = kabuk_win::process::Process::open(pid().unwrap()).expect("the player runs");
    drop(shell);
    assert!(second.wait(Some(Duration::from_secs(5))), "the player outlived Kabuk");
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(
        kabuk_win::wallpaper::current().as_ref(),
        Some(&theirs),
        "the user's wallpaper did not come back"
    );
    assert!(wait_until(Duration::from_secs(5), || restored(&before)).is_some());
}

/// Found by the user: in Devre a black band lay behind the bar's groups (DECISIONS D71).
/// With the background off, the looks whose groups float (Ada, Devre) leave the desktop
/// as it was around their groups; with a solid one Ada fills its strip, while Devre fills
/// only its groups, so far less of the desktop goes dark.
#[test]
fn floating_groups_leave_the_desktop_behind_them() {
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let rows = kabuk_win::msgloop::PxRect {
        x: primary.rect.left,
        y: primary.rect.top,
        w: primary.rect.right - primary.rect.left,
        h: 3,
    };
    let look_at = || kabuk_win::dpi::physical(|| kabuk_win::screen::capture(rows)).expect("screen");
    let mut change = std::collections::HashMap::new();
    for look in ["ada", "devre"] {
        for background in ["off", "solid"] {
            let before = work_areas();
            let desktop = look_at();
            let sandbox = Sandbox::new();
            std::fs::write(
                sandbox.config_file(),
                format!("schema = 1\n[appearance]\nlook = \"{look}\"\n[bar]\nposition = \"top\"\nbackground = \"{background}\"\n"),
            )
            .unwrap();
            let _shell = sandbox.shell_with_bar(&[]);
            assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bar drawn") >= 1).is_some());
            std::thread::sleep(Duration::from_millis(800));
            change.insert((look, background), kabuk_win::screen::difference(&desktop, &look_at()));
            assert_eq!(sandbox.restore().0, 0);
            assert!(wait_until(SETTLE, || restored(&before)).is_some());
        }
    }
    let c = |look, background| change[&(look, background)];
    assert!(c("ada", "off") < 1.0 && c("devre", "off") < 1.0, "{change:?}");
    assert!(
        c("devre", "solid") < c("ada", "solid") * 0.75,
        "Devre filled its strip: {change:?}"
    );
}

/// The bar of Monitor 1 (main) of shell `pid`, and a sender of the shell's
/// ABN_FULLSCREENAPP to it, as Explorer sends it (`true`: a program went full screen).
fn main_bar_and_notify(pid: u32) -> (kabuk_win::window::Hwnd, impl Fn(bool)) {
    use kabuk::bar::manager::MSG_APPBAR;
    const ABN_FULLSCREENAPP: usize = 2;
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let main_bar = shell_windows("Kabuk.Bar", pid)
        .into_iter()
        .find(|w| {
            w.window_rect().is_some_and(|r| {
                let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                primary.rect.left <= cx && cx < primary.rect.right && primary.rect.top <= cy && cy < primary.rect.bottom
            })
        })
        .expect("primary bar");
    let notify = move |entering: bool| {
        kabuk_win::msgloop::post_message(
            main_bar,
            kabuk_win::msgloop::Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };
    (main_bar, notify)
}

/// DECISIONS D72, found on another computer: game mode failed there. A game in exclusive
/// full screen changes the resolution, and Windows tells every window the display
/// changed: the bars are not made again while the full-screen program is in front (new
/// bar windows over it could take it out of its full screen); they are once it is gone.
/// The display change is announced to the bars as Windows does (the test machine has one
/// resolution only).
#[test]
fn a_display_change_under_a_full_screen_game_waits_for_it() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let (main_bar, notify) = main_bar_and_notify(pid);
    let game = TestWindow::open(&["--fullscreen", "--title", &title("exclusive")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the game"
    );
    for bar in shell_windows("Kabuk.Bar", pid) {
        kabuk_win::msgloop::post_message(
            bar,
            kabuk_win::msgloop::Msg {
                id: kabuk_win::msgloop::msg::DISPLAYCHANGE,
                wparam: 32,
                lparam: 0,
            },
        )
        .unwrap();
    }
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("display changed under a full-screen program")
        == 1)
    .is_some());
    std::thread::sleep(kabuk::bar::manager::DISPLAY_DEBOUNCE * 2);
    assert_eq!(
        sandbox.log_lines("bars built"),
        1,
        "the bars were made again under the game"
    );
    assert!(!main_bar.is_visible(), "a bar came up over the game");
    // The game ends: the bars are made again for the change.
    game.close();
    notify(false);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("bars built") >= 2).is_some(),
        "the bars were not made again after the game"
    );
    assert_eq!(
        sandbox.log_lines("full-screen program gone; bars made again for the display change"),
        1
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// M19's perf run: the idle shell was over its 40 MB. The app grid's screen taking (a GPU
/// device), made ahead a while after the start and let go a minute later, left about
/// 10 MB with the GPU's driver for good. It is made when the grid opens; the first pages'
/// icons still come ahead.
#[test]
fn the_app_grid_takes_the_screen_only_when_it_opens() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_GRID};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    sandbox.wait_started();
    assert!(
        wait_until(SETTLE + SETTLE, || sandbox.log_lines("app grid pictures arrived") > 0).is_some(),
        "no icons ahead"
    );
    assert_eq!(sandbox.log_lines("app grid screen taking made"), 0);
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    kabuk_win::msgloop::post_message(
        control,
        kabuk_win::msgloop::Msg {
            id: MSG_TEST_APP_GRID,
            wparam: 1,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    assert_eq!(sandbox.log_lines("app grid screen taking made"), 1);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// DECISIONS D87, found by the user: a game still stuttered now and then for about a
/// second. A moment of another window in front of it (a launcher, a notice) ended game
/// mode and began it again, and Kabuk brought everything back and took it away under the
/// game. Game mode belongs to the game's window: a window in front of it for a moment
/// keeps it; one staying in front (the user went to it) ends it.
#[test]
fn a_window_in_front_of_a_game_for_a_moment_keeps_game_mode() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let (main_bar, notify) = main_bar_and_notify(pid);
    let game = TestWindow::open(&["--fullscreen", "--title", &title("held-game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    assert!(
        wait_until(SETTLE, || game.hwnd.focus()).is_some(),
        "the game could not come to the front"
    );
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the game"
    );
    // Nothing of Kabuk's busies the GPU or every program beside the game: the app grid's
    // screen taking (a GPU device) goes, or was never made (it is made a while after the
    // start, M19 P3), and programs are not asked to put their tray icons back until the
    // game is over.
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("app grid screen taking made")
            == sandbox.log_lines("app grid screen taking let go for the game"))
        .is_some(),
        "the app grid kept its GPU device beside the game"
    );
    {
        use kabuk::bar::manager::{CONTROL_CLASS, MSG_TRAY_BEHIND};
        let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
            .into_iter()
            .find(|h| h.pid() == Some(pid))
            .expect("bar control window");
        kabuk_win::msgloop::post_message(
            control,
            kabuk_win::msgloop::Msg {
                id: MSG_TRAY_BEHIND,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
        assert!(
            wait_until(SETTLE, || sandbox.log_lines("a game is in front, programs asked later")
                >= 1)
            .is_some(),
            "programs were asked for their tray icons under the game"
        );
        assert_eq!(sandbox.log_lines("programs asked to register again"), 0);
    }
    let came_back = || sandbox.log_lines("bar back after a full-screen program");
    // The shell's lines about game mode, for a failure's message.
    let game_lines = || -> String {
        std::fs::read_dir(sandbox.state_dir().join("logs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("kabuk."))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .flat_map(|t| t.lines().map(str::to_string).collect::<Vec<_>>())
            .filter(|l| {
                ["game", "full screen", "full-screen", "bar back", "steps aside"]
                    .iter()
                    .any(|k| l.contains(k))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // A window in front of the game for a moment.
    let other = TestWindow::open(&["--topmost", "--title", &title("a-moment")]);
    assert!(wait_until(SETTLE, || other.hwnd.is_visible()).is_some());
    assert!(wait_until(SETTLE, || other.hwnd.focus()).is_some());
    notify(true);
    assert!(
        wait_until(SETTLE, || sandbox
            .log_lines("game mode kept: another window in front of the game")
            == 1)
        .is_some(),
        "game mode was not kept for the game: {:?}",
        game_lines()
    );
    assert!(
        !main_bar.is_visible() && came_back() == 0,
        "the bar came back over the game"
    );
    other.close();
    assert!(wait_until(SETTLE, || game.hwnd.focus()).is_some());
    notify(true);
    // Back in front: game mode stays, past the time a window in front may stay.
    std::thread::sleep(Duration::from_secs(4));
    assert!(
        !main_bar.is_visible() && came_back() == 0 && sandbox.log_lines("game let go") == 0,
        "game mode ended with the game back in front: {:?}",
        game_lines()
    );

    // A window staying in front: the user went to it, and the bar comes back.
    let stays = TestWindow::open(&["--topmost", "--title", &title("stays")]);
    assert!(wait_until(SETTLE, || stays.hwnd.is_visible()).is_some());
    assert!(wait_until(SETTLE, || stays.hwnd.focus()).is_some());
    notify(true);
    assert!(
        wait_until(Duration::from_secs(10), || main_bar.is_visible()).is_some(),
        "the bar did not come back with another window staying in front of the game"
    );
    assert_eq!(sandbox.log_lines("away=true"), 1);
    drop(stays);
    drop(game);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// DECISIONS D87, found by a full run: a game seen still on screen as it closed kept game
/// mode, and no event came after it. The game holding game mode is watched: hidden (or
/// closed, or minimised) it lets game mode go at once, though nothing else comes in front.
#[test]
fn a_game_going_away_ends_game_mode_with_nothing_else_in_front() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let (main_bar, notify) = main_bar_and_notify(pid);
    let game = TestWindow::open(&["--fullscreen", "--title", &title("hidden-game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    assert!(
        wait_until(SETTLE, || game.hwnd.focus()).is_some(),
        "the game could not come to the front"
    );
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the game"
    );
    // Hidden, it stays the window in front: no foreground event comes.
    assert!(game.hwnd.hide_async());
    assert!(
        wait_until(Duration::from_secs(2), || main_bar.is_visible()).is_some(),
        "game mode stayed after the game went: {:?}",
        sandbox.last_log_line(&["kabuk::bar::manager: game"])
    );
    assert_eq!(sandbox.log_lines("game let go"), 1);
    drop(game);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// DECISIONS D72: an overlay over a full-screen game (a graphics driver's, Game Bar's, a
/// voice chat's), a window clicks pass through, is not the program in front: the game
/// still sends the bar away.
#[test]
fn an_overlay_over_a_full_screen_game_does_not_keep_the_bar() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let (main_bar, notify) = main_bar_and_notify(pid);
    let game = TestWindow::open(&["--fullscreen", "--title", &title("game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    notify(true);
    assert!(
        wait_until(SETTLE, || !main_bar.is_visible()).is_some(),
        "the bar stayed over the game"
    );
    // An overlay comes up over the game (found: the next check took it for the program
    // in front and brought the bar back over the game).
    let checks = sandbox.log_lines("full-screen check");
    let overlay = TestWindow::open(&["--overlay", "--title", &title("overlay")]);
    assert!(wait_until(SETTLE, || overlay.hwnd.is_visible()).is_some());
    notify(true);
    assert!(wait_until(SETTLE, || sandbox.log_lines("full-screen check") > checks).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !main_bar.is_visible() && sandbox.log_lines("bar back after a full-screen program") == 0,
        "the bar came back over the game for an overlay: {:?}",
        sandbox.last_log_line(&["full-screen check"])
    );
    overlay.close();
    game.close();
    notify(false);
    assert!(
        wait_until(SETTLE, || main_bar.is_visible()).is_some(),
        "the bar did not come back"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// Found by a review of the code (M19 P1): every title change of the window in front ran
/// the full-screen check, the bars' care, the apps and the keyboard, each enumerating
/// every window. A rename only draws the title; a change of focus does the rest.
#[test]
fn a_renaming_window_in_front_costs_no_foreground_work() {
    use kabuk_win::msgloop::{post_message, Msg};
    const RETITLE: u32 = 0x8000 + 6;
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let busy = TestWindow::open(&["--title", &title("renames")]);
    assert!(wait_until(SETTLE, || busy.hwnd.is_visible()).is_some());
    assert!(wait_until(SETTLE, || busy.hwnd.focus()).is_some());
    let work = || sandbox.log_lines("foreground work");
    assert!(
        wait_until(SETTLE, || work() >= 1).is_some(),
        "focus moving did no foreground work: {:?}",
        sandbox.last_log_line(&["focus moved"])
    );
    std::thread::sleep(Duration::from_millis(500));
    let settled = work();
    post_message(
        busy.hwnd,
        Msg {
            id: RETITLE,
            wparam: 30,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(wait_until(SETTLE, || busy.hwnd.title().ends_with(" 29")).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        work(),
        settled,
        "a rename did foreground work: {:?}",
        sandbox.last_log_line(&["focus moved"])
    );

    // A change of focus still does it.
    let other = TestWindow::open(&["--title", &title("other")]);
    assert!(wait_until(SETTLE, || other.hwnd.focus()).is_some());
    assert!(
        wait_until(SETTLE, || work() > settled).is_some(),
        "a change of focus did no foreground work"
    );
    drop(other);
    drop(busy);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// Found by a review of the code (M19 P4): the figures' timers ran while nobody could see
/// them, each tick laying every bar out again. They stop while the bars are away (here an
/// auto-hiding bar slid off) and while no figure is on the bar at all.
#[test]
fn the_figures_stop_while_no_bar_shows_them() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\nautohide = true\n").unwrap();
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    assert!(kabuk_win::input::set_cursor_pos(
        (primary.rect.left + primary.rect.right) / 2,
        (primary.rect.top + primary.rect.bottom) / 2,
    ));
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.unwrap();
    let bar = primary_window_of("Kabuk.Bar", pid);
    assert!(wait_until(SETTLE, || bar.window_rect().unwrap().top < primary.rect.top).is_some());
    // Started before it slid off and stopped since, or never started.
    std::thread::sleep(Duration::from_millis(500));
    let last = sandbox.last_log_line(&["figures scheduled"]).unwrap_or_default();
    assert!(
        !last.contains("=true"),
        "the figures still run with the bar away: {last}"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());

    // No figure on the bar: their timers never start.
    let quiet = Sandbox::new();
    std::fs::write(
        quiet.config_file(),
        "schema = 1\n[bar.modules]\ncpu = false\nram = false\nnet = false\ngpu = false\n",
    )
    .unwrap();
    let _quiet_shell = quiet.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(quiet.log_lines("stats=true"), 0);
    assert_eq!(quiet.log_lines("gpu=true"), 0);
    assert_eq!(quiet.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// Asked for by the user on 2026-10-29: a click on an app with several windows went to
/// them in turn. As on Windows' taskbar, it shows their previews to pick one from, and a
/// second click takes them away.
#[test]
fn a_click_on_an_app_with_several_windows_shows_their_previews() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_CLICK};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let a = TestWindow::open(&["--title", &title("several-a")]);
    let b = TestWindow::open(&["--title", &title("several-b")]);
    for w in [&a, &b] {
        let managed = format!("managing window hwnd={}", w.hwnd.0);
        assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    }
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let click = || {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_CLICK,
                wparam: b.hwnd.0 as usize,
                lparam: 1,
            },
        )
        .expect("post to the bar");
    };
    click();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("app previews shown by a click") == 1).is_some(),
        "no previews for a click"
    );
    assert!(wait_until(SETTLE, || sandbox.log_lines("windows=2") >= 1).is_some());
    assert_eq!(
        sandbox.log_lines("app shown from the bar"),
        0,
        "a window was shown instead"
    );
    assert_eq!(sandbox.log_lines("app minimized from the bar"), 0);
    click();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("app previews hidden") >= 1).is_some(),
        "a second click left the previews"
    );
    drop(b);
    drop(a);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// Asked for by the user on 2026-10-29: apps were missing from the bar (DECISIONS D93).
/// A window the window manager leaves alone (here by a rule) is still on Windows'
/// taskbar; so it is on the bar, and a click on it brings it forward.
#[test]
fn a_window_the_window_manager_leaves_alone_is_on_the_bar() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_APP_CLICK};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let alone = title("left-alone");
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        format!("schema = 1\n[[rules.rule]]\ntitle = \"{alone}\"\nignore = true\n"),
    )
    .unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let w = TestWindow::open(&["--minimized", "--title", &alone]);
    assert!(wait_until(SETTLE, || w.hwnd.is_minimized()).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        sandbox.log_lines(&format!("managing window hwnd={}", w.hwnd.0)),
        0,
        "the rule did not leave it alone"
    );
    post_message(
        control,
        Msg {
            id: MSG_TEST_APP_CLICK,
            wparam: w.hwnd.0 as usize,
            lparam: 0,
        },
    )
    .expect("post to the bar");
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("app shown from the bar") == 1).is_some(),
        "the window left alone is not on the bar"
    );
    assert!(
        wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some(),
        "not brought back"
    );
    drop(w);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// Asked for by the user on 2026-10-29: a right click on an empty part of the bar opens
/// what Windows' taskbar menu holds (Task Manager, the bar's settings), where it came.
#[test]
fn a_right_click_on_an_empty_part_of_the_bar_opens_the_taskbar_menu() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_VOLUME_PANEL};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |what: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_VOLUME_PANEL,
                wparam: what,
                lparam: 0,
            },
        )
        .unwrap();
    };
    post(4);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("menu=Bar") == 1).is_some(),
        "no menu for an empty part of the bar: {:?}",
        sandbox.last_log_line(&["bar click"])
    );
    let menu = kabuk_win::window::windows_of_class("Kabuk.VolumePanel")
        .into_iter()
        .find(|h| h.pid() == Some(pid) && h.is_visible())
        .expect("the menu's window");
    // Where the click came: over the bar's start, not its middle or end.
    let spot = sandbox
        .last_log_line(&["bar click", "Right"])
        .and_then(|l| l.split(" x=").nth(1)?.split_whitespace().next()?.parse::<i32>().ok())
        .expect("the click's place");
    let bar = primary_window_of("Kabuk.Bar", pid).window_rect().unwrap();
    let r = menu.window_rect().unwrap();
    assert!(
        r.left <= bar.left + spot && bar.left + spot <= r.right + 1,
        "the menu {r:?} is not at the click {spot}"
    );
    // The same again closes it.
    post(4);
    assert!(
        wait_until(SETTLE, || !menu.is_visible()).is_some(),
        "a second click left it"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U1: a kabuk.toml with a mistake was only logged; the user saw the defaults
/// and did not know why. Now a notice says so and where, at the start and on a change.
#[test]
fn a_mistake_in_the_settings_file_is_told() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar]\nheight = 9999\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let told = || sandbox.last_log_line(&["notice shown"]).unwrap_or_default();
    assert!(
        wait_until(STARTUP_TIMEOUT, || told().contains("kabuk.toml")).is_some(),
        "no notice for the refused file: {}",
        told()
    );
    assert!(told().contains("efault") || told().contains("arsayılan"), "{}", told());
    // Fixed, then broken again while it runs: the last working settings stay.
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let shown = sandbox.log_lines("notice shown");
    std::fs::write(sandbox.config_file(), "schema = 1\n[bar\n").unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown") > shown).is_some(),
        "no notice for the refused change"
    );
    assert!(
        told().contains("ast working") || told().contains("Son çalışan"),
        "{}",
        told()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// DECISIONS D95: notices came on Monitor 1 (main) even for a change made on Monitor 2's
/// bar. A change made on a bar is told on that bar's monitor.
#[test]
fn a_notice_comes_on_the_monitor_of_the_bar_it_was_made_on() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_NOTICE_FROM_BAR};
    use kabuk_win::msgloop::{post_message, Msg};
    let monitors = kabuk_win::shell::monitors();
    if monitors.len() < 2 {
        eprintln!("one monitor: nothing to tell apart");
        return;
    }
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    // The bars are made in Windows' monitor order: a bar of a monitor that is not the main one.
    let (index, other) = monitors
        .iter()
        .enumerate()
        .find(|(_, m)| !m.primary)
        .map(|(i, m)| (i, m.rect))
        .unwrap();
    post_message(
        control,
        Msg {
            id: MSG_TEST_NOTICE_FROM_BAR,
            wparam: index,
            lparam: 0,
        },
    )
    .unwrap();
    let notice = || {
        kabuk_win::window::windows_of_class("Kabuk.Notice")
            .into_iter()
            .find(|h| h.pid() == Some(pid) && h.is_visible())
            .and_then(|h| h.window_rect())
    };
    let r = wait_until(SETTLE, || notice().is_some()).and_then(|_| notice());
    let r = r.expect("no notice");
    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    assert!(
        other.left <= cx && cx < other.right && other.top <= cy && cy < other.bottom,
        "the notice {r:?} is not on the bar's monitor {other:?}"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U6: something asked for that did not open was only logged. The user is told.
#[test]
fn something_that_does_not_open_is_told() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_OPEN_MISSING};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let shown = sandbox.log_lines("notice shown");
    post_message(
        control,
        Msg {
            id: MSG_TEST_OPEN_MISSING,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown") > shown).is_some(),
        "nothing told"
    );
    let told = sandbox.last_log_line(&["notice shown"]).unwrap_or_default();
    assert!(told.contains("did not open") || told.contains("Açılamadı"), "{told}");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U20: Kabuk started while it runs took over without a word (the user's choice
/// of 2026-10-14 keeps the takeover): the new shell says Kabuk was already running.
#[test]
fn kabuk_started_while_it_runs_says_so() {
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[language]\nlanguage = \"en\"\n[game]\ndetect = false\n",
    )
    .unwrap();
    let mut first = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("late start done") == 1).is_some());
    let told = "notice shown title=Kabuk was already running";
    assert_eq!(sandbox.log_lines(told), 0, "the first start said it");
    let _second = sandbox.shell_with_bar(&[]);
    assert!(first.wait_exit(SETTLE).is_some(), "the first stayed");
    assert!(
        wait_until(STARTUP_TIMEOUT, || sandbox.log_lines(told) == 1).is_some(),
        "the second start said nothing"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U18: a brightness Kabuk set (a wheel notch over the battery) showed nothing;
/// the volume's OSD shows it, with the sun, on that bar's monitor. A monitor with no
/// brightness to set (no DDC/CI, no laptop panel) is told instead, and no OSD comes: each
/// machine checks the path it has.
#[test]
fn a_wheel_step_on_the_brightness_shows_its_osd() {
    use kabuk::bar::manager::{BRIGHTNESS_STEP, CONTROL_CLASS, MSG_TEST_WHEEL_BRIGHTNESS};
    use kabuk_win::brightness::Panel;
    use kabuk_win::msgloop::{post_message, Msg};

    /// Gives the monitor its brightness back, even when the test fails.
    struct Back(Option<(Panel, u8)>);
    impl Drop for Back {
        fn drop(&mut self) {
            if let Some((panel, level)) = self.0 {
                let _ = kabuk_win::brightness::set(panel, level);
            }
        }
    }

    kabuk_win::system::com_init_mta();
    let primary = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .expect("a main monitor");
    let back = Back(kabuk_win::brightness::read(primary.handle, &primary.device));
    eprintln!("the main monitor's brightness: {:?}", back.0);
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[game]\ndetect = false\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    // Down, unless that is the bottom already.
    let notches: i32 = match back.0 {
        Some((_, now)) if i32::from(now) >= BRIGHTNESS_STEP => -1,
        _ => 1,
    };
    let shown = sandbox.log_lines("notice shown");
    post_message(
        control,
        Msg {
            id: MSG_TEST_WHEEL_BRIGHTNESS,
            wparam: 0,
            lparam: notches as isize,
        },
    )
    .unwrap();
    let osd = || {
        kabuk_win::window::windows_of_class("Kabuk.Osd")
            .into_iter()
            .find(|h| h.pid() == Some(pid) && h.is_visible())
            .and_then(|h| h.window_rect())
    };
    match back.0 {
        Some((_, now)) => {
            assert!(
                wait_until(SETTLE, || sandbox.log_lines("level shown level=Brightness") == 1).is_some(),
                "no brightness OSD"
            );
            let r = osd().expect("the OSD's window");
            let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            let m = primary.rect;
            assert!(
                m.left <= cx && cx < m.right && m.top <= cy && cy < m.bottom,
                "the OSD {r:?} is not on the main monitor {m:?}"
            );
            let to = (i32::from(now) + notches * BRIGHTNESS_STEP).clamp(0, 100);
            let line = sandbox
                .last_log_line(&["level shown level=Brightness"])
                .unwrap_or_default();
            assert!(line.contains(&format!("percent={to}")), "{line}");
            let read = kabuk_win::brightness::read(primary.handle, &primary.device).map(|(_, p)| i32::from(p));
            assert_eq!(read, Some(to), "the monitor's brightness");
        }
        None => {
            assert!(
                wait_until(SETTLE, || sandbox.log_lines("notice shown") > shown).is_some(),
                "nothing told"
            );
            assert_eq!(sandbox.log_lines("brightness not set"), 1);
            let told = sandbox.last_log_line(&["notice shown"]).unwrap_or_default();
            assert!(told.contains("brightness") || told.contains("Parlaklık"), "{told}");
            assert_eq!(sandbox.log_lines("level shown"), 0, "an OSD for nothing set");
            assert!(osd().is_none());
        }
    }
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U14: a window flashing for attention (a chat's new message) showed nothing on
/// the bar; Windows' taskbar marks its button. The bar hears it from the shell.
#[test]
fn a_window_asking_for_attention_is_seen() {
    use kabuk_win::msgloop::{post_message, Msg};
    const FLASH: u32 = 0x8000 + 7;
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar_and_wm();
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let chat = TestWindow::open(&["--title", &title("attention")]);
    let other = TestWindow::open(&["--title", &title("in-front")]);
    // The other one in front if Windows lets it (a screen saver or a lock keeps the
    // focus): the flash is heard either way.
    let _ = wait_until(SETTLE, || other.hwnd.focus());
    post_message(
        chat.hwnd,
        Msg {
            id: FLASH,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    let wants = format!("app wants attention hwnd={}", chat.hwnd.0);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines(&wants) == 1).is_some(),
        "the flashing window was not seen"
    );
    drop(other);
    drop(chat);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U15: only tray icons had a tooltip. The clock's says the whole date with the
/// year (a side bar hides the date).
#[test]
fn the_clock_has_a_tooltip_with_the_whole_date() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_CLOCK_TIP};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    post_message(
        control,
        Msg {
            id: MSG_TEST_CLOCK_TIP,
            wparam: 0,
            lparam: 0,
        },
    )
    .unwrap();
    let year = kabuk_win::system::local_time().year.to_string();
    assert!(
        wait_until(SETTLE, || sandbox
            .last_log_line(&["tooltip shown"])
            .is_some_and(|l| l.contains(&year)))
        .is_some(),
        "no tooltip with the date: {:?}",
        sandbox.last_log_line(&["tooltip"])
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U20: workspaces named in `kabuk.toml` are named in their tooltip (and so to a
/// screen reader), a change of the names is taken at once, and one without a name keeps
/// its number.
#[test]
fn a_workspaces_tooltip_has_its_name() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_WORKSPACE_TIP};
    use kabuk_win::msgloop::{post_message, Msg};
    let before = work_areas();
    let sandbox = Sandbox::new();
    let config = |names: &str| {
        std::fs::write(
            sandbox.config_file(),
            format!("schema = 1\n[language]\nlanguage = \"en\"\n[windows]\nworkspace_names = {names}\n"),
        )
        .unwrap();
    };
    config(r#"["Web", "Kod"]"#);
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let tip_of = |n: usize, expected: &str| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_WORKSPACE_TIP,
                wparam: n,
                lparam: 0,
            },
        )
        .unwrap();
        assert!(
            wait_until(SETTLE, || sandbox
                .last_log_line(&["tooltip shown"])
                .is_some_and(|l| l.contains(&format!("tip={expected} "))))
            .is_some(),
            "workspace {n}: {:?}",
            sandbox.last_log_line(&["tooltip shown"])
        );
    };
    tip_of(2, "Workspace 2 · Kod");
    tip_of(3, "Workspace 3");
    config(r#"["Web", "Code"]"#);
    assert!(wait_until(SETTLE, || sandbox.log_lines("configuration reloaded") >= 1).is_some());
    tip_of(2, "Workspace 2 · Code");
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// DECISIONS D97: Narrator saw nothing of the bar. A screen reader finds its modules as
/// buttons named in the user's language, where they are drawn, and invoking the clock
/// does what a click on it does (its calendar opens).
#[test]
fn a_screen_reader_finds_the_bars_modules_and_clicks_the_clock() {
    use kabuk_win::uia::{self, Role};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let bar = primary_window_of(kabuk_core::hang::BAR_CLASS, pid);
    let i18n = kabuk_win::system::i18n(None);
    let clock = i18n.tr("mod-clock");
    let workspace = i18n.tr_args("tip-workspace", &[("value", "1".into())]);
    let has_clock = || uia::read(bar).is_ok_and(|s| s.iter().any(|p| p.name.starts_with(&clock)));
    assert!(
        wait_until(SETTLE, has_clock).is_some(),
        "no clock among {:?}",
        uia::read(bar)
    );
    let seen = uia::read(bar).unwrap();
    let r = bar.window_rect().unwrap();
    let inside = |p: &uia::Seen| {
        p.rect.left >= r.left && p.rect.right <= r.right && p.rect.top >= r.top && p.rect.bottom <= r.bottom
    };
    let found = seen.iter().find(|p| p.name.starts_with(&clock)).unwrap();
    assert_eq!(found.role, Some(Role::Button), "{found:?}");
    assert!(
        inside(found) && found.rect.right > found.rect.left,
        "{found:?} outside {r:?}"
    );
    assert!(
        seen.iter().any(|p| p.name == workspace && inside(p)),
        "no {workspace:?} in {seen:?}"
    );
    assert!(seen.iter().all(|p| !p.name.trim().is_empty()), "{seen:?}");
    // Invoked, the clock is clicked: its calendar opens.
    uia::invoke(bar, &found.name).unwrap();
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("calendar panel opened") > 0).is_some(),
        "the clock's calendar did not open"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U17: a level was not offered to screen readers (an Invoke would have set it to
/// half). The volume panel's level is a slider holding the volume as drawn, from none to
/// full a wheel notch's step at a time, which is not invoked; set by a screen reader, the
/// panel sets it as a drag would. No test changes the user's volume: it is set to what it
/// is, and only read while muted (drawn 0).
#[test]
fn a_screen_reader_reads_and_sets_the_volume_as_a_slider() {
    use kabuk::bar::manager::{CONTROL_CLASS, MSG_TEST_VOLUME_PANEL, VOLUME_STEP};
    use kabuk_win::msgloop::{post_message, Msg};
    use kabuk_win::uia::{self, Role};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let post = |wparam: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_VOLUME_PANEL,
                wparam,
                lparam: 0,
            },
        )
        .unwrap()
    };
    post(1);
    let panel = || {
        kabuk_win::window::windows_of_class("Kabuk.VolumePanel")
            .into_iter()
            .find(|h| h.pid() == Some(pid) && h.is_visible())
    };
    assert!(wait_until(SETTLE, || panel().is_some()).is_some(), "no volume panel");
    let panel = panel().unwrap();
    let i18n = kabuk_win::system::i18n(None);
    let volume = i18n.tr("mod-vol");
    assert!(
        wait_until(SETTLE, || uia::read(panel)
            .is_ok_and(|s| s.iter().any(|p| p.name == volume)))
        .is_some(),
        "no level among {:?}",
        uia::read(panel)
    );
    let seen = uia::read(panel).unwrap();
    let level = seen.iter().find(|p| p.name == volume).unwrap();
    assert_eq!(level.role, Some(Role::Slider), "{level:?}");
    let r = uia::range(panel, &volume).unwrap();
    assert_eq!((r.min, r.max, r.step), (0, 100, VOLUME_STEP), "{r:?}");
    assert!((0..=100).contains(&r.value), "{r:?}");
    assert!(uia::invoke(panel, &volume).is_err(), "a level was invoked");
    // The speaker says what pressing it does.
    let muted = seen.iter().any(|p| p.name == i18n.tr("acc-unmute"));
    assert!(muted || seen.iter().any(|p| p.name == i18n.tr("acc-mute")), "{seen:?}");
    if !muted {
        uia::set_range(panel, &volume, f64::from(r.value)).unwrap();
        let line = format!("percent={}", r.value);
        assert!(
            wait_until(SETTLE, || sandbox
                .last_log_line(&["volume panel level set by a screen reader"])
                .is_some_and(|l| l.contains(&line)))
            .is_some(),
            "the level was not set: {:?}",
            sandbox.last_log_line(&["screen reader"])
        );
        assert_eq!(uia::range(panel, &volume).unwrap().value, r.value);
    }
    // Past its ends: refused.
    assert!(uia::set_range(panel, &volume, 101.0).is_err());
    post(0);
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}

/// PLAN M19 U17: the app grid was drawn only. A screen reader finds the apps of its page
/// as list items named as the grid names them; while it is edited, each app's badge is a
/// button that hides it, and invoked, it hides the app drawn there (twice: the apps slid
/// into the first one's place are found where they are drawn now).
#[test]
fn a_screen_reader_finds_the_app_grids_apps_and_hides_one() {
    use kabuk::bar::manager::{APP_GRID_CLASS, CONTROL_CLASS, MSG_TEST_APP_GRID};
    use kabuk_win::msgloop::{post_message, Msg};
    use kabuk_win::uia::{self, Role};
    let before = work_areas();
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || reserved(&before)).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let control = kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("bar control window");
    let grid = |what: usize| {
        post_message(
            control,
            Msg {
                id: MSG_TEST_APP_GRID,
                wparam: what,
                lparam: 0,
            },
        )
        .unwrap()
    };
    // The apps of the page on screen, in order, as the grid says them.
    let shown = || -> Vec<String> {
        sandbox
            .last_log_line(&["app grid page"])
            .and_then(|l| {
                l.split("apps=")
                    .nth(1)
                    .map(|a| a.split(" | ").map(|n| n.trim().to_string()).collect())
            })
            .unwrap_or_default()
    };
    grid(1);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid shown") == 1).is_some());
    let sheet = kabuk_win::window::windows_of_class(APP_GRID_CLASS)
        .into_iter()
        .find(|h| h.pid() == Some(pid))
        .expect("the grid's pages");
    let items = || -> Vec<String> {
        uia::read(sheet)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.role == Some(Role::ListItem))
            .map(|p| p.name)
            .collect()
    };
    let apps = shown();
    assert!(apps.len() >= 3, "{apps:?}");
    assert!(
        wait_until(SETTLE, || items() == shown()).is_some(),
        "{:?} read for {:?}",
        items(),
        shown()
    );
    let r = sheet.window_rect().unwrap();
    assert!(
        uia::read(sheet)
            .unwrap()
            .iter()
            .all(|p| p.rect.left >= r.left && p.rect.right <= r.right && p.rect.bottom > p.rect.top),
        "drawn outside the grid"
    );
    // Edited: a badge for each app.
    grid(2);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid editing on=true") == 1).is_some());
    let i18n = kabuk_win::system::i18n(None);
    let hide = |name: &str| i18n.tr_args("acc-grid-hide", &[("value", name.to_string().into())]);
    for (n, name) in apps.iter().take(2).enumerate() {
        let badge = hide(name);
        assert!(
            wait_until(SETTLE, || uia::read(sheet).is_ok_and(|s| s
                .iter()
                .any(|p| p.name == badge && p.role == Some(Role::Button))))
            .is_some(),
            "no {badge:?} among {:?}",
            uia::read(sheet)
        );
        uia::invoke(sheet, &badge).unwrap();
        assert!(
            wait_until(SETTLE, || sandbox.log_lines("app grid app hidden") == n + 1).is_some(),
            "{badge:?} hid nothing"
        );
        let hidden = sandbox.last_log_line(&["app grid app hidden"]).unwrap();
        assert!(hidden.ends_with(&format!("app={name}")), "{hidden} (wanted {name})");
    }
    grid(0);
    assert!(wait_until(SETTLE, || sandbox.log_lines("app grid closed") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || restored(&before)).is_some());
}
