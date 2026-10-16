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

/// SPEC §2.1, found by the user: `bar.autohide` did nothing. An auto-hiding bar reserves
/// no space and waits off its edge with a sliver left on screen; the mouse resting on it
/// brings it in, and some time after the mouse leaves it slides off again. DECISIONS
/// D70, found by the user: it came over the windows whenever the mouse passed the edge
/// (on its way to a window's top), and slid off while its own panel was open. Run in the
/// test machine: the mouse is moved.
#[test]
fn an_auto_hiding_bar_reserves_nothing_and_slides_in_when_the_mouse_rests() {
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
    move_to(edge);
    assert!(
        wait_until(SETTLE, || top() == primary.rect.top).is_some(),
        "not shown for a resting mouse: {}\n{}",
        top(),
        sandbox.last_log_line(&["auto-hiding"]).unwrap_or_default()
    );
    assert_eq!(sandbox.log_lines("mouse rested at the edge"), 1);
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
    // The panel closed: it slides off.
    post(0);
    assert!(
        wait_until(SETTLE, || top() == hidden).is_some(),
        "not hidden again: {}",
        top()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(work_areas(), before);
}

/// DECISIONS D70, found by the user: with auto-hide, Rıhtım's dock did not hide. It
/// reserves nothing, waits off its edge with a sliver at the monitor's edge, comes in
/// when the mouse rests there and slides off once it leaves. Run in the test machine:
/// the mouse is moved.
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
    assert!(kabuk_win::input::set_cursor_pos(away.0, away.1));
    assert!(
        wait_until(SETTLE, || rect() == hidden).is_some(),
        "not hidden again: {:?}",
        rect()
    );
    assert_eq!(sandbox.restore().0, 0);
    assert_eq!(work_areas(), before);
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
    assert!(
        wait_until(STARTUP_TIMEOUT, || tr_days.iter().any(|d| drawn().contains(d))).is_some(),
        "{}",
        drawn()
    );
    // The RAM figure is the drawn text ending in "G", e.g. "18,6G".
    let ram = |line: String| {
        line.split('"')
            .find(|t| t.ends_with('G') && t.len() > 1)
            .unwrap_or_default()
            .to_string()
    };
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
    let loaded = || sandbox.log_lines("app icon loaded icon=");
    let exe = testapp_exe().display().to_string();
    let of_testapp = || {
        sandbox
            .last_log_line(&["app icon loaded"])
            .is_some_and(|l| l.contains(&exe))
    };
    let first = TestWindow::open(&["--title", &title("icon-again-1")]);
    assert!(wait_until(SETTLE, &of_testapp).is_some(), "its icon loaded");
    let after_first = loaded();
    drop(first);
    let second = TestWindow::open(&["--title", &title("icon-again-2")]);
    let managed = format!("managing window hwnd={}", second.hwnd.0);
    assert!(wait_until(SETTLE, || sandbox.log_lines(&managed) == 1).is_some());
    assert!(
        wait_until(SETTLE, || loaded() > after_first && of_testapp()).is_some(),
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

/// Asked for by the user (macOS's genie): a managed window's own transitions are turned
/// off (recorded first); minimising and restoring it play the genie into and out of its
/// icon on the bar; Windows' own effect chosen in the file gives its transitions back,
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
    w.hwnd.restore_async();
    assert!(
        wait_until(SETTLE, || genies(false) == 1).is_some(),
        "no genie out of the icon"
    );
    assert!(wait_until(SETTLE, || !w.hwnd.is_minimized()).is_some());
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

    // Found by the user: the strips' 22 px stairs on the funnel's edges. Once its renderer
    // is ready (made after the first genie), a genie is drawn by the GPU with the window's
    // picture, smooth.
    let smooth = sandbox.log_lines("genie renderer made") == 1
        && std::fs::read_dir(sandbox.state_dir().join("logs"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("kabuk."))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .any(|text| {
                text.lines().filter(|l| l.contains("window motion played")).any(|l| {
                    l.split("gpu=")
                        .nth(1)
                        .and_then(|v| v.split_whitespace().next())
                        .and_then(|v| v.parse::<u32>().ok())
                        .is_some_and(|n| n > 0)
                })
            });
    assert!(
        smooth,
        "no genie drawn smooth: {:?}",
        sandbox.last_log_line(&["window motion played"])
    );
    // Windows' own effect: the record goes, the transitions come back.
    std::fs::write(
        sandbox.config_file(),
        "schema = 1\n[windows]\nminimize_effect = \"windows\"\n",
    )
    .unwrap();
    assert!(wait_until(SETTLE, || !recorded()).is_some(), "still recorded");
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
