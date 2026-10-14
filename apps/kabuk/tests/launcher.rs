//! The launcher in a running shell (PLAN M6). It is opened with a test message instead of
//! the shortcut, so it shows without taking the keyboard from the user's window, and
//! keys are posted to its own window (no input is sent to the system). No entry that
//! acts on the session (lock, sleep, restart) is ever chosen.
//!
//! Edge cases: the first frame's time, Turkish letters ("EKRANI"), Escape, the
//! calculator copying its result, an open window on another workspace, the slow sources
//! arriving while it is open.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::time::Duration;

use common::*;
use kabuk::bar::manager::{CONTROL_CLASS, LAUNCHER_CLASS, MSG_TEST_LAUNCHER};
use kabuk::wm::{test_action, MSG_TEST_ACTION, WM_CLASS};
use kabuk_win::msgloop::{msg, post_message, send_message, vk, Msg};
use kabuk_win::window::Hwnd;

const SETTLE: Duration = Duration::from_secs(5);
/// PLAN M6: shortcut to first frame.
const FIRST_FRAME_LIMIT_US: u64 = 100_000;

/// Only the fast sources, so the results do not depend on the user's files.
fn config(extra: &str) -> String {
    format!(
        "schema = 1\n[language]\nlanguage = \"tr\"\n[launcher.sources]\napps = false\nrecent = false\nfiles = false\nbookmarks = false\n{extra}"
    )
}

struct Shell {
    _proc: Proc,
    pid: u32,
}

impl Shell {
    fn start(sandbox: &Sandbox, cfg: &str) -> Shell {
        std::fs::write(sandbox.config_file(), cfg).unwrap();
        let proc = sandbox.shell_with_bar_and_wm();
        assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("window manager running") >= 1).is_some());
        assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") >= 1).is_some());
        let pid = sandbox.state().kabuk_pid.expect("shell pid");
        Shell { _proc: proc, pid }
    }

    fn control(&self) -> Hwnd {
        kabuk_win::window::message_windows_of_class(CONTROL_CLASS)
            .into_iter()
            .find(|w| w.pid() == Some(self.pid))
            .expect("bar control window")
    }

    fn launcher(&self) -> Hwnd {
        kabuk_win::window::windows_of_class(LAUNCHER_CLASS)
            .into_iter()
            .find(|w| w.pid() == Some(self.pid))
            .expect("launcher window")
    }

    fn wm(&self) -> Hwnd {
        kabuk_win::window::windows_of_class(WM_CLASS)
            .into_iter()
            .find(|w| w.pid() == Some(self.pid))
            .expect("window manager window")
    }

    /// Posted, not sent: a sent message can arrive nested while the bar thread is busy,
    /// and the bar drops those.
    fn open(&self, sandbox: &Sandbox) {
        let shown = sandbox.log_lines("launcher shown");
        post(self.control(), MSG_TEST_LAUNCHER, 1);
        assert!(
            wait_until(SETTLE, || sandbox.log_lines("launcher shown") > shown).is_some(),
            "not opened:\n{}",
            story(sandbox)
        );
    }

    fn type_text(&self, text: &str) {
        let l = self.launcher();
        for unit in text.encode_utf16() {
            post(l, msg::CHAR, usize::from(unit));
        }
    }

    fn key(&self, vk: usize) {
        post(self.launcher(), msg::KEYDOWN, vk);
    }

    fn act(&self, code: usize, arg: isize) {
        send_message(
            self.wm(),
            Msg {
                id: MSG_TEST_ACTION,
                wparam: code,
                lparam: arg,
            },
            Duration::from_secs(2),
        )
        .expect("window manager answers");
    }
}

fn post(to: Hwnd, id: u32, wparam: usize) {
    post_message(to, Msg { id, wparam, lparam: 0 }).expect("post to the launcher");
}

/// The shell's launcher log lines, for failures.
fn story(sandbox: &Sandbox) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(sandbox.state_dir().join("logs")).unwrap().flatten() {
        for l in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
            if l.contains("launcher") || l.contains("ERROR") || l.contains("WARN") {
                out.push_str(l.get(11..).unwrap_or(l));
                out.push('\n');
            }
        }
    }
    out
}

/// The last rows line the launcher logged for `query`.
fn rows_for(sandbox: &Sandbox, query: &str) -> Option<String> {
    sandbox.last_log_line(&["launcher rows", &format!("query={query} ")])
}

/// PLAN M6: "EKRANI" finds "Ekranı kilitle" in the real launcher, which opens in time;
/// Escape closes it.
#[test]
fn ekrani_finds_ekrani_kilitle_and_escape_closes() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox, &config(""));
    shell.open(&sandbox);
    assert!(
        wait_until(SETTLE, || shell.launcher().is_visible()).is_some(),
        "not shown:\n{}",
        story(&sandbox)
    );
    let shown = sandbox.last_log_line(&["launcher shown"]).expect("shown line");
    let us: u64 = shown
        .split("first_frame_us=")
        .nth(1)
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .expect("first frame time");
    assert!(us < FIRST_FRAME_LIMIT_US, "first frame took {us} µs");

    shell.type_text("EKRANI");
    assert!(wait_until(SETTLE, || rows_for(&sandbox, "EKRANI").is_some()).is_some());
    let rows = rows_for(&sandbox, "EKRANI").unwrap();
    let first = rows.split("rows=").nth(1).unwrap_or_default();
    assert!(first.starts_with("Ekranı kilitle"), "{rows}");

    shell.key(vk::ESCAPE);
    assert!(
        wait_until(SETTLE, || !shell.launcher().is_visible()).is_some(),
        "Escape did not close it"
    );
    assert_eq!(sandbox.log_lines("launcher ran"), 0, "nothing may run");
    assert_eq!(sandbox.restore().0, 0);
}

/// SPEC §2.5: Enter copies the calculator's result.
#[test]
fn the_calculators_result_is_copied() {
    if kabuk_win::clipboard::holds_more_than_text() {
        eprintln!("clipboard holds more than text; left alone");
        return;
    }
    let before = kabuk_win::clipboard::text();
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox, &config(""));
    shell.open(&sandbox);
    shell.type_text("9,8+1");
    assert!(wait_until(SETTLE, || rows_for(&sandbox, "9,8+1")
        .is_some_and(|r| r.contains("rows== 10,8")))
    .is_some());
    shell.key(vk::RETURN);
    assert!(wait_until(SETTLE, || kabuk_win::clipboard::text().as_deref() == Some("10,8")).is_some());
    assert!(!shell.launcher().is_visible());
    if let Some(b) = before {
        kabuk_win::clipboard::set_text(&b).unwrap();
    }
    assert_eq!(sandbox.restore().0, 0);
}

/// SPEC §2.5: choosing an open window shows its workspace (it stands in for Alt+Tab).
#[test]
fn an_open_window_on_another_workspace_is_shown() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox, &config(""));
    let name = title("launcher-target");
    let w = TestWindow::open(&["--title", &name]);
    assert!(wait_until(SETTLE, || sandbox
        .log_lines(&format!("managing window hwnd={}", w.hwnd.0))
        == 1)
    .is_some());
    shell.act(test_action::FOCUSED, w.hwnd.0 as isize);
    shell.act(test_action::MOVE_TO_WORKSPACE, 2);
    assert!(wait_until(SETTLE, || !w.hwnd.is_visible()).is_some(), "not moved away");

    shell.open(&sandbox);
    shell.type_text(&name);
    assert!(wait_until(SETTLE, || rows_for(&sandbox, &name)
        .is_some_and(|r| r.contains(&format!("rows={name}"))))
    .is_some());
    shell.key(vk::RETURN);
    assert!(
        wait_until(SETTLE, || w.hwnd.is_visible()).is_some(),
        "its workspace was not shown"
    );
    assert_eq!(sandbox.log_lines("window revealed from the launcher"), 1);
    assert_eq!(sandbox.restore().0, 0);
}

/// The slow sources arrive while the launcher is open and show up in it.
#[test]
fn installed_apps_arrive_while_it_is_open() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox, &config("").replace("apps = false", "apps = true"));
    shell.open(&sandbox);
    assert!(wait_until(Duration::from_secs(20), || sandbox
        .log_lines("launcher entries gathered")
        == 1)
    .is_some());
    let gathered = sandbox.last_log_line(&["launcher entries gathered"]).unwrap();
    assert!(!gathered.contains("entries=0 "), "{gathered}");
    // Apps come before the commands, so once they are in, the empty query's first row is
    // an app (its name is in Windows' own language, so the test does not name one).
    let first_is_app = || {
        rows_for(&sandbox, "").is_some_and(|r| {
            let first = r.split("rows=").nth(1).unwrap_or_default();
            !first.is_empty() && !first.starts_with("Ekranı kilitle")
        })
    };
    assert!(wait_until(SETTLE, first_is_app).is_some(), "{}", story(&sandbox));
    shell.key(vk::ESCAPE);
    assert_eq!(sandbox.restore().0, 0);
}

/// The launcher's "Kabuk ayarları" opens the settings app on this instance's files, its
/// page runs, and a second start brings the same window back instead of a new one.
#[test]
fn kabuk_ayarlari_opens_one_settings_window() {
    let sandbox = Sandbox::new();
    // This instance's settings window; one the user has open is another class.
    let settings_class =
        kabuk_win::window::settings_class(kabuk_core::instance::Instance::for_state_dir(sandbox.state_dir()).id());
    let shell = Shell::start(&sandbox, &config(""));
    let windows = || kabuk_win::window::windows_of_class(&settings_class);
    assert!(windows().is_empty(), "a settings window is already open");
    for round in 0..2 {
        shell.open(&sandbox);
        shell.type_text("kabuk ayar");
        assert!(wait_until(SETTLE, || rows_for(&sandbox, "kabuk ayar")
            .is_some_and(|r| r.contains("rows=Kabuk ayarları")))
        .is_some());
        shell.key(vk::RETURN);
        assert!(
            wait_until(SETTLE, || sandbox.log_lines("settings opened") == round + 1).is_some(),
            "{}",
            story(&sandbox)
        );
        assert!(
            wait_until(SETTLE, || windows().len() == 1).is_some(),
            "round {round}: {:?}",
            windows()
        );
    }
    // Its page ran, on this instance's files (its log is in the sandbox's state folder).
    assert!(wait_until(SETTLE, || sandbox.log_lines("settings page loaded") >= 1).is_some());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(windows().len(), 1, "a second window opened");
    // PLAN M8: closed, it leaves nothing running, its WebView2 helpers included (they
    // carry the sandbox's profile folder on their command line).
    let w = windows()[0];
    let app = kabuk_win::process::Process::open(w.pid().expect("pid")).expect("settings process");
    w.close_async();
    assert!(app.wait(Some(SETTLE)), "the settings app did not end");
    let profile = sandbox.state_dir().join("webview").display().to_string();
    let helpers = || {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process -Filter \"Name='msedgewebview2.exe'\" | ForEach-Object { $_.CommandLine }",
            ])
            .output()
            .expect("list processes");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains(&profile))
            .count()
    };
    assert!(
        wait_until(SETTLE, || helpers() == 0).is_some(),
        "{} WebView2 helper(s) left",
        helpers()
    );
    assert_eq!(sandbox.restore().0, 0);
}
