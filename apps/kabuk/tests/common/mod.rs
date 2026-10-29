//! Helpers for the Windows integration tests. Every helper cleans up in `Drop`, so a
//! failing test still leaves its windows visible and its processes stopped
//! (TESTING §1.8). Tests only touch windows created by `kabuk-testapp`.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use kabuk_core::state::{HiddenWindow, StateFile};
use kabuk_win::window::Hwnd;

/// The window class of `kabuk-testapp`, the only class the test-mode window manager manages.
pub const TEST_WINDOW_CLASS: &str = "KabukTestWindow";
/// The test app's title when it is started without `--title` (as a layout profile or
/// the drop-down starts it).
pub const TESTAPP_DEFAULT_TITLE: &str = "Kabuk test window";

/// Passed to [`Sandbox::kabuk_with`]: the shell hides the taskbar and takes the tray icons
/// as it does for the user (only tests about those do this).
pub const TAKE_EXPLORER: &str = "<take explorer>";

pub const POLL: Duration = Duration::from_millis(10);
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Polls `cond` until it holds or `timeout` passes. Returns the time it took.
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> Option<Duration> {
    let start = Instant::now();
    loop {
        if cond() {
            return Some(start.elapsed());
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

fn bin_dir() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_kabuk")).parent().unwrap().to_path_buf()
}

pub fn kabuk_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kabuk"))
}

fn sibling(name: &str) -> PathBuf {
    let p = bin_dir().join(name);
    assert!(
        p.exists(),
        "{} missing: build the workspace first (`cargo build --workspace`)",
        p.display()
    );
    p
}

pub fn watchdog_exe() -> PathBuf {
    sibling("kabuk-watchdog.exe")
}

pub fn testapp_exe() -> PathBuf {
    sibling("kabuk-testapp.exe")
}

/// A unique title per test and run.
pub fn title(test: &str) -> String {
    format!("kabuk-it-{test}-{}", std::process::id())
}

/// A test window. Shown again and closed on drop.
pub struct TestWindow {
    child: Child,
    pub hwnd: Hwnd,
    pub pid: u32,
    /// The owned dialog of `--dialog`.
    pub dialog: Option<Hwnd>,
}

impl TestWindow {
    pub fn open(args: &[&str]) -> TestWindow {
        let mut child = Command::new(testapp_exe())
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .expect("start kabuk-testapp");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut value = |key: &str| {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            line.trim()
                .strip_prefix(key)
                .unwrap_or_else(|| panic!("testapp prints {key}"))
                .parse::<u64>()
                .unwrap()
        };
        let hwnd = Hwnd(value("hwnd="));
        let dialog = args.contains(&"--dialog").then(|| Hwnd(value("dialog=")));
        let pid = child.id();
        TestWindow {
            child,
            hwnd,
            pid,
            dialog,
        }
    }

    pub fn record(&self) -> HiddenWindow {
        HiddenWindow {
            hwnd: self.hwnd.0,
            pid: self.pid,
            class: self.hwnd.class_name(),
            title: self.hwnd.title(),
        }
    }
}

/// The testapp's `FAKE_DRAG` and `FAKE_RESIZE` messages (`apps/kabuk-testapp/src/main.rs`).
const TESTAPP_FAKE_DRAG: u32 = kabuk_win::msgloop::msg::APP + 2;
const TESTAPP_FAKE_RESIZE: u32 = kabuk_win::msgloop::msg::APP + 3;

impl TestWindow {
    /// Makes the window report a title-bar drag by (dx, dy) (a click when both are 0),
    /// without touching the mouse.
    pub fn fake_drag(&self, dx: i32, dy: i32) {
        kabuk_win::msgloop::post_message(
            self.hwnd,
            kabuk_win::msgloop::Msg {
                id: TESTAPP_FAKE_DRAG,
                wparam: dx as isize as usize,
                lparam: dy as isize,
            },
        )
        .expect("post to the test window");
    }

    /// Makes the window report a resize that moves its left and right edges by these
    /// amounts, without touching the mouse.
    pub fn fake_resize(&self, left: i32, right: i32) {
        kabuk_win::msgloop::post_message(
            self.hwnd,
            kabuk_win::msgloop::Msg {
                id: TESTAPP_FAKE_RESIZE,
                wparam: left as isize as usize,
                lparam: right as isize,
            },
        )
        .expect("post to the test window");
    }

    /// Ends the window's process (the window closes as if the program quit).
    pub fn close(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        self.hwnd.show_async();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A child process killed on drop.
pub struct Proc(pub Child);

impl Proc {
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let mut status = None;
        wait_until(timeout, || {
            status = self.0.try_wait().ok().flatten();
            status.is_some()
        });
        status
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A private state directory. On drop: emergency restore of this instance, so nothing
/// a test started keeps running or stays hidden.
pub struct Sandbox {
    pub dir: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        // The tests were written against Ada (its top bar and sizes); the shells they
        // start take it as their default look (debug builds only).
        std::env::set_var(kabuk_core::config::TEST_DEFAULT_LOOK, "ada");
        Sandbox {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn state_dir(&self) -> &Path {
        self.dir.path()
    }

    pub fn state_file(&self) -> PathBuf {
        StateFile::path_in(self.dir.path())
    }

    pub fn state(&self) -> kabuk_core::state::State {
        StateFile::read(&self.state_file()).unwrap_or_default()
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.path().join("kabuk.toml")
    }

    /// `kabuk.exe` without the watchdog and without the bar.
    pub fn shell(&self, extra: &[&str]) -> Proc {
        let mut args = vec!["--no-bar"];
        args.extend_from_slice(extra);
        self.shell_with_bar(&args)
    }

    /// `kabuk.exe` with its bar: reserves screen space until the test ends. Logs at
    /// debug level so tests can read what each bar drew.
    pub fn shell_with_bar(&self, extra: &[&str]) -> Proc {
        self.shell_with_bar_logging(extra, "debug")
    }

    /// As [`Sandbox::shell_with_bar`] with a chosen log level (`info` for measurements).
    /// The window manager stays off: it would manage the user's windows.
    pub fn shell_with_bar_logging(&self, extra: &[&str], level: &str) -> Proc {
        let mut args = vec!["--no-wm"];
        args.extend_from_slice(extra);
        self.kabuk(&args, level)
    }

    /// `kabuk.exe` with the window manager in test mode: it manages only
    /// `kabuk-testapp` windows and takes actions from test messages, not shortcuts.
    /// No bar, so the user's work area stays as it is.
    pub fn shell_with_wm(&self, extra: &[&str]) -> Proc {
        let mut args = vec!["--no-bar", "--manage-class", TEST_WINDOW_CLASS];
        args.extend_from_slice(extra);
        self.kabuk(&args, "debug")
    }

    /// `kabuk.exe` with exactly these arguments (bar and window manager on unless they
    /// say otherwise), logging at debug level.
    pub fn kabuk_with(&self, args: &[&str]) -> Proc {
        self.kabuk(args, "debug")
    }

    fn kabuk(&self, args: &[&str], level: &str) -> Proc {
        let mut cmd = Command::new(kabuk_exe());
        cmd.env("KABUK_LOG", level);
        cmd.arg("--state-dir")
            .arg(self.state_dir())
            .arg("--config")
            .arg(self.config_file())
            .arg("--no-hotkey");
        // The user's taskbar and tray icons stay as they are, unless a test asks.
        if !args.contains(&TAKE_EXPLORER) {
            cmd.arg("--leave-explorer");
        }
        let args: Vec<&str> = args.iter().copied().filter(|a| *a != TAKE_EXPLORER).collect();
        cmd.args(&args);
        Proc(cmd.spawn().expect("start kabuk"))
    }

    /// The watchdog, which starts `kabuk.exe` with `shell_args` after `--`.
    pub fn watchdog(&self, shell_args: &[&str]) -> Proc {
        let mut cmd = Command::new(watchdog_exe());
        cmd.arg("--state-dir")
            .arg(self.state_dir())
            .arg("--config")
            .arg(self.config_file())
            .args(["--quiet", "--no-hotkey", "--kabuk"])
            .arg(kabuk_exe())
            .arg("--")
            // The bar would reserve screen space on the user's desktop during the test,
            // and the window manager would manage the user's windows.
            .arg("--no-bar")
            .arg("--no-wm")
            .arg("--leave-explorer")
            .args(shell_args);
        Proc(cmd.spawn().expect("start kabuk-watchdog"))
    }

    /// The watchdog with a test-mode window manager in its shell (see [`Sandbox::shell_with_wm`]).
    pub fn watchdog_with_wm(&self) -> Proc {
        self.watchdog_with_wm_and(&[])
    }

    /// As [`Sandbox::watchdog_with_wm`], with more arguments for the shell.
    pub fn watchdog_with_wm_and(&self, shell_args: &[&str]) -> Proc {
        let mut cmd = Command::new(watchdog_exe());
        cmd.env("KABUK_LOG", "debug")
            .arg("--state-dir")
            .arg(self.state_dir())
            .arg("--config")
            .arg(self.config_file())
            .args(["--quiet", "--no-hotkey", "--kabuk"])
            .arg(kabuk_exe())
            .args([
                "--",
                "--no-bar",
                "--leave-explorer",
                "--manage-class",
                TEST_WINDOW_CLASS,
            ])
            .args(shell_args);
        Proc(cmd.spawn().expect("start kabuk-watchdog"))
    }

    /// The watchdog with a shell that has its bars, hides the taskbar and takes the tray,
    /// as for the user (no window manager).
    pub fn watchdog_taking_explorer(&self) -> Proc {
        let mut cmd = Command::new(watchdog_exe());
        cmd.env("KABUK_LOG", "debug")
            .arg("--state-dir")
            .arg(self.state_dir())
            .arg("--config")
            .arg(self.config_file())
            .args(["--quiet", "--no-hotkey", "--kabuk"])
            .arg(kabuk_exe())
            .args(["--", "--no-wm"]);
        Proc(cmd.spawn().expect("start kabuk-watchdog"))
    }

    /// The bar and the test-mode window manager together (reserves screen space).
    pub fn shell_with_bar_and_wm(&self) -> Proc {
        self.shell_with_bar_and_wm_logging("debug")
    }

    /// As [`Sandbox::shell_with_bar_and_wm`] with a chosen log level.
    pub fn shell_with_bar_and_wm_logging(&self, level: &str) -> Proc {
        self.kabuk(&["--manage-class", TEST_WINDOW_CLASS], level)
    }

    /// Everything as for the user (bar, tray, hidden taskbar) with a test-mode window manager.
    pub fn shell_as_for_the_user(&self, level: &str) -> Proc {
        self.kabuk(&[TAKE_EXPLORER, "--manage-class", TEST_WINDOW_CLASS], level)
    }

    /// Waits until the shell's start is over: the launcher, the app grid and the system
    /// indicators come a moment after the first bars (M19 P3).
    pub fn wait_started(&self) {
        assert!(
            wait_until(STARTUP_TIMEOUT, || self.log_lines("late start done") == 1).is_some(),
            "the shell's start did not end"
        );
    }

    /// Runs `kabuk --restore` for this instance and returns its exit code and output.
    pub fn restore(&self) -> (i32, String) {
        let out = Command::new(kabuk_exe())
            .arg("--state-dir")
            .arg(self.state_dir())
            .arg("--config")
            .arg(self.config_file())
            .arg("--restore")
            .output()
            .expect("run kabuk --restore");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }

    pub fn write_hidden(&self, windows: &[HiddenWindow]) {
        let mut file = StateFile::open(&self.state_file()).unwrap();
        for w in windows {
            file.add_hidden(w.clone()).unwrap();
        }
    }

    /// The last log line containing every needle.
    pub fn last_log_line(&self, needles: &[&str]) -> Option<String> {
        let dir = std::fs::read_dir(self.dir.path().join("logs")).ok()?;
        let mut last = None;
        for e in dir.flatten() {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                for l in text.lines() {
                    if needles.iter().all(|n| l.contains(n)) {
                        last = Some(l.to_string());
                    }
                }
            }
        }
        last
    }

    /// Lines of this instance's log files that contain `needle`.
    pub fn log_lines(&self, needle: &str) -> usize {
        let Ok(dir) = std::fs::read_dir(self.dir.path().join("logs")) else {
            return 0;
        };
        dir.flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .map(|text| text.lines().filter(|l| l.contains(needle)).count())
            .sum()
    }
}

/// How many of a failed test's last log lines are printed.
const FAILED_LOG_LINES: usize = 120;

impl Drop for Sandbox {
    fn drop(&mut self) {
        // A failed test shows what the shell said last (its folder goes with it; found
        // running the tests in the VM, where nothing else was left to read).
        if std::thread::panicking() {
            for e in std::fs::read_dir(self.dir.path().join("logs"))
                .into_iter()
                .flatten()
                .flatten()
            {
                let text = std::fs::read_to_string(e.path()).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                eprintln!("--- {} (last lines)", e.file_name().to_string_lossy());
                for l in &lines[lines.len().saturating_sub(FAILED_LOG_LINES)..] {
                    eprintln!("{l}");
                }
            }
        }
        let _ = self.restore();
    }
}
