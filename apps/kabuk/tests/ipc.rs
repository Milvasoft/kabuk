//! The command interface end to end (PLAN M7): every command of ARCHITECTURE §12 sent
//! with `kabuk msg` to a running shell (bar and test-mode window manager), and the pipe's
//! ACL.
//!
//! Edge cases: each command's effect, the three queries' JSON, a command the shell
//! cannot carry out, a typo (explained without a shell), no shell running, `restore`
//! answering before it stops the shell.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::process::Command;
use std::time::Duration;

use common::*;
use kabuk::wm::{test_action, MSG_TEST_ACTION, WM_CLASS};
use kabuk_core::instance::{exit, Instance};
use kabuk_win::msgloop::{send_message, Msg};
use serde_json::Value;

const SETTLE: Duration = Duration::from_secs(5);
const GAP: i32 = 10;
/// Game mode off: a full-screen program on the real desktop would hold notices back and
/// still animations, which these tests watch.
const NO_GAME: &str = "[game]\ndetect = false\n";
/// How far into a notice's exit its faded picture is read (of 220 ms).
const INTO_EXIT: Duration = Duration::from_millis(40);

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }
}

fn msg(sandbox: &Sandbox, words: &[&str]) -> Out {
    let out = Command::new(kabuk_exe())
        .arg("--state-dir")
        .arg(sandbox.state_dir())
        .arg("--config")
        .arg(sandbox.config_file())
        .arg("msg")
        .args(words)
        .output()
        .expect("run kabuk msg");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn ok(sandbox: &Sandbox, words: &[&str]) -> Out {
    let o = msg(sandbox, words);
    assert_eq!(o.code, 0, "{words:?}: {}", o.stderr);
    o
}

struct Shell {
    proc: Proc,
    pid: u32,
}

impl Shell {
    fn start(sandbox: &Sandbox) -> Shell {
        Shell::start_with(sandbox, "")
    }

    /// With `more` added to the configuration's end.
    fn start_with(sandbox: &Sandbox, more: &str) -> Shell {
        std::fs::write(
            sandbox.config_file(),
            format!("schema = 1\n[language]\nlanguage = \"en\"\n[windows]\ngap = {GAP}\n{more}"),
        )
        .unwrap();
        let proc = sandbox.shell_with_bar_and_wm();
        assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("command pipe open") == 1).is_some());
        let pid = sandbox.state().kabuk_pid.expect("pid");
        Shell { proc, pid }
    }

    fn focus(&self, w: &TestWindow) {
        let wm = kabuk_win::window::windows_of_class(WM_CLASS)
            .into_iter()
            .find(|h| h.pid() == Some(self.pid))
            .expect("window manager window");
        send_message(
            wm,
            Msg {
                id: MSG_TEST_ACTION,
                wparam: test_action::FOCUSED,
                lparam: w.hwnd.0 as isize,
            },
            Duration::from_secs(2),
        )
        .expect("window manager answers");
    }
}

fn managed(sandbox: &Sandbox, w: &TestWindow) {
    assert!(wait_until(SETTLE, || sandbox
        .log_lines(&format!("managing window hwnd={}", w.hwnd.0))
        == 1)
    .is_some());
}

fn window_entry(sandbox: &Sandbox, w: &TestWindow) -> Value {
    ok(sandbox, &["query", "windows"])
        .json()
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["hwnd"] == w.hwnd.0)
        .cloned()
        .unwrap_or(Value::Null)
}

fn primary_state(sandbox: &Sandbox) -> Value {
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    ok(sandbox, &["query", "state"]).json()["monitors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["device"] == primary.device.as_str())
        .cloned()
        .unwrap()
}

/// PLAN M19 U11: the tiling layout set in Settings reaches the workspaces already open
/// (it reached only those made after the change); one switched by hand keeps its own.
#[test]
fn a_tiling_layout_set_in_settings_reaches_the_open_workspaces() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start(&sandbox);
    assert_eq!(primary_state(&sandbox)["layout"], "dwindle");
    let set = |layout: &str, gap: i32| {
        std::fs::write(
            sandbox.config_file(),
            format!(
                "schema = 1\n[language]\nlanguage = \"en\"\n[windows]\ngap = {gap}\ntiling_layout = \"{layout}\"\n"
            ),
        )
        .unwrap();
    };
    set("master", GAP);
    assert!(
        wait_until(SETTLE, || primary_state(&sandbox)["layout"] == "master").is_some(),
        "{}",
        primary_state(&sandbox)
    );
    // Switched by hand: the setting no longer moves it.
    ok(&sandbox, &["layout", "dwindle"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["layout"] == "dwindle").is_some());
    let applied = sandbox.log_lines("window manager configuration applied");
    // Another setting changes, the layout setting stays.
    set("master", GAP + 1);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("window manager configuration applied")
            > applied)
        .is_some()
    );
    assert_eq!(primary_state(&sandbox)["layout"], "dwindle");
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn the_queries_describe_the_running_shell() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let state = ok(&sandbox, &["query", "state"]).json();
    assert_eq!(state["pid"], shell.pid);
    assert_eq!(state["instance"], Instance::for_state_dir(sandbox.state_dir()).id());
    assert_eq!(
        (state["window_manager"].as_bool(), state["bar"].as_bool()),
        (Some(true), Some(true))
    );
    let monitors = ok(&sandbox, &["query", "monitors"]).json();
    let real = kabuk_win::shell::monitors();
    assert_eq!(monitors.as_array().unwrap().len(), real.len());
    for m in &real {
        let j = monitors
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["device"] == m.device.as_str())
            .unwrap();
        assert_eq!(
            (j["number"].as_u64(), j["primary"].as_bool()),
            (Some(u64::from(m.number)), Some(m.primary))
        );
        assert_eq!(j["workspace"], 1);
        assert_eq!(j["rect"]["right"], m.rect.right);
    }
    let name = title("ipc-query");
    let w = TestWindow::open(&["--title", &name]);
    managed(&sandbox, &w);
    let e = window_entry(&sandbox, &w);
    assert_eq!(
        (e["title"].as_str(), e["workspace"].as_u64()),
        (Some(name.as_str()), Some(1)),
        "{e}"
    );
    assert_eq!(e["class"], TEST_WINDOW_CLASS);
    assert_eq!(e["floating"], false);
    drop(shell);
}

#[test]
fn workspace_and_move_to_switch_and_send_windows() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-ws")]);
    managed(&sandbox, &a);
    ok(&sandbox, &["workspace", "2"]);
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(),
        "workspace 2 not shown"
    );
    assert_eq!(primary_state(&sandbox)["workspace"], 2);
    ok(&sandbox, &["workspace", "1"]);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some());
    shell.focus(&a);
    ok(&sandbox, &["move-to", "3"]);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(), "not moved");
    assert_eq!(window_entry(&sandbox, &a)["workspace"], 3);
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn focus_float_and_layout_act_on_the_focused_monitor() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-a")]);
    let b = TestWindow::open(&["--title", &title("ipc-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    shell.focus(&a);
    ok(&sandbox, &["focus", "right"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("action applied action=Focus(Right)") == 1).is_some());

    shell.focus(&b);
    ok(&sandbox, &["float", "toggle"]);
    assert!(wait_until(SETTLE, || window_entry(&sandbox, &b)["floating"] == true).is_some());

    ok(&sandbox, &["layout", "master"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["layout"] == "master").is_some());
    ok(&sandbox, &["layout", "dwindle"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["layout"] == "dwindle").is_some());
    ok(&sandbox, &["layout", "floating"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["floating"] == true).is_some());
    assert_eq!(window_entry(&sandbox, &a)["floating"], true);
    ok(&sandbox, &["layout", "tiling"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["floating"] == false).is_some());
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M19 U4: `float on|off` float or tile the focused window whatever it was (the
/// shortcut, and `float toggle`, only flip it); one already that way stays.
#[test]
fn float_on_and_off_set_the_focused_window_either_way() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-float-a")]);
    let b = TestWindow::open(&["--title", &title("ipc-float-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    let floating = |w: &TestWindow| window_entry(&sandbox, w)["floating"].clone();
    shell.focus(&b);
    ok(&sandbox, &["float", "off"]);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("action applied action=SetFloat(false)")
            == 1)
        .is_some()
    );
    assert_eq!(floating(&b), false, "a tiled window stays tiled");
    ok(&sandbox, &["float", "on"]);
    assert!(wait_until(SETTLE, || floating(&b) == true).is_some(), "not floated");
    ok(&sandbox, &["float", "on"]);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("action applied action=SetFloat(true)")
            == 2)
        .is_some()
    );
    assert_eq!(floating(&b), true, "a floating window stays floating");
    ok(&sandbox, &["float", "off"]);
    assert!(wait_until(SETTLE, || floating(&b) == false).is_some(), "not tiled");
    assert_eq!(floating(&a), false, "the other window was touched");
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M19 U20: a `kabuk` that finds this one running and must give way to it asks it,
/// over the pipe, to tell the user Kabuk is already running.
#[test]
fn a_start_that_gives_way_is_told_by_the_running_shell() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start_with(&sandbox, NO_GAME);
    let told = "notice shown title=Kabuk is already running";
    assert_eq!(sandbox.log_lines(told), 0);
    ok(&sandbox, &[kabuk_core::ipc::STARTED_AGAIN]);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines(told) == 1).is_some(),
        "not told"
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M19 U20: do not disturb holds Kabuk's notices back, set from `kabuk msg quiet`
/// (written to `kabuk.toml`, as Settings writes it); turned off, one summary counts them.
#[test]
fn do_not_disturb_holds_notices_back_and_sums_them_up() {
    let sandbox = Sandbox::new();
    let shell = Shell::start_with(&sandbox, NO_GAME);
    let a = TestWindow::open(&["--title", &title("ipc-quiet-a")]);
    let b = TestWindow::open(&["--title", &title("ipc-quiet-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    let file = || std::fs::read_to_string(sandbox.config_file()).unwrap_or_default();
    ok(&sandbox, &["quiet", "on"]);
    assert!(
        wait_until(SETTLE, || file().contains("quiet = true")).is_some(),
        "not written"
    );
    assert!(wait_until(SETTLE, || sandbox.log_lines("configuration reloaded") >= 1).is_some());
    let shown = sandbox.log_lines("notice shown");
    shell.focus(&b);
    ok(&sandbox, &["float", "toggle"]);
    assert!(wait_until(SETTLE, || window_entry(&sandbox, &b)["floating"] == true).is_some());
    ok(&sandbox, &["float", "toggle"]);
    assert!(wait_until(SETTLE, || window_entry(&sandbox, &b)["floating"] == false).is_some());
    assert_eq!(sandbox.log_lines("notice shown"), shown, "a notice came through");
    ok(&sandbox, &["quiet", "toggle"]);
    assert!(
        wait_until(SETTLE, || file().contains("quiet = false")).is_some(),
        "not written"
    );
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown title=Do not disturb is off")
            == 1)
        .is_some(),
        "no summary"
    );
    let summary = sandbox
        .last_log_line(&["notice shown title=Do not disturb is off"])
        .unwrap_or_default();
    assert!(summary.contains("2 notices"), "{summary}");
    // Shown again.
    ok(&sandbox, &["float", "toggle"]);
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown title=Window is floating")
            == 1)
        .is_some(),
        "notices stayed held back"
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// DECISIONS D65: `query windows` tells each window's program and what opens it again;
/// a layout profile opened by name moves the program's open window to its workspace and
/// starts the program again for its second place, whose window goes there as it comes.
#[test]
fn a_layout_profile_moves_open_windows_and_starts_the_rest() {
    let sandbox = Sandbox::new();
    let exe = testapp_exe();
    let main = kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| m.primary)
        .unwrap()
        .number;
    let profile = format!(
        "[[profile]]\nname = \"Test layout\"\n\
         [[profile.window]]\nexe = \"kabuk-testapp.exe\"\nopen = '{0}'\nmonitor = {main}\nworkspace = 3\n\
         [[profile.window]]\nexe = \"kabuk-testapp.exe\"\nopen = '{0}'\nmonitor = {main}\nworkspace = 4\n",
        exe.display()
    );
    let _shell = Shell::start_with(&sandbox, &profile);
    // Kabuk starts the second window (the test app's own title), so the test ends it,
    // failed or not (found in the VM: a failed run left it, and the next tests managed it).
    struct EndsStarted;
    impl Drop for EndsStarted {
        fn drop(&mut self) {
            for w in kabuk_win::window::windows_of_class(TEST_WINDOW_CLASS) {
                if w.title() != TESTAPP_DEFAULT_TITLE {
                    continue;
                }
                if let Some(p) = w.pid().and_then(|pid| kabuk_win::process::Process::open(pid).ok()) {
                    let _ = p.terminate(1);
                    p.wait(Some(SETTLE));
                }
            }
        }
    }
    let _ends = EndsStarted;
    let a = TestWindow::open(&["--title", &title("profile-a")]);
    managed(&sandbox, &a);
    let entry = window_entry(&sandbox, &a);
    assert_eq!(entry["exe"], "kabuk-testapp.exe", "{entry}");
    assert!(
        entry["open"]
            .as_str()
            .is_some_and(|o| o.to_lowercase().ends_with("kabuk-testapp.exe")),
        "{entry}"
    );

    ok(&sandbox, &["profile", "test", "layout"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("layout profile opened") == 1).is_some());
    assert!(wait_until(SETTLE, || window_entry(&sandbox, &a)["workspace"] == 3).is_some());
    // The second place: the program started again, its window put there.
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox
        .log_lines("window placed by a layout profile")
        == 1)
    .is_some());
    // The query answers from the window manager's last published view, a moment behind
    // its log (found in the VM).
    let started = || {
        ok(&sandbox, &["query", "windows"])
            .json()
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["hwnd"] != a.hwnd.0 && e["exe"] == "kabuk-testapp.exe")
            .cloned()
    };
    assert!(
        wait_until(SETTLE, || started().is_some()).is_some(),
        "the started window"
    );
    let started = started().expect("the started window");
    assert_eq!(started["workspace"], 4, "{started}");
    if let Some(h) = started["hwnd"].as_u64() {
        kabuk_win::window::Hwnd(h).close_async();
    }
    let unknown = msg(&sandbox, &["profile", "nope"]);
    assert_eq!(unknown.code, 0, "a profile that is not there is said in the log");
    assert!(wait_until(SETTLE, || sandbox.log_lines("no such layout profile") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn reload_and_launcher_reach_the_bar_and_the_window_manager() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start(&sandbox);
    let (bar, wm) = (
        sandbox.log_lines("configuration reloaded"),
        sandbox.log_lines("window manager configuration applied"),
    );
    ok(&sandbox, &["reload"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("configuration reloaded") == bar + 1).is_some());
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("window manager configuration applied")
            == wm + 1)
        .is_some()
    );
    ok(&sandbox, &["launcher"]);
    assert!(wait_until(SETTLE, || sandbox.log_lines("launcher shown") == 1).is_some());
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M19 U4: the window manager's shortcut actions that had no command reach it from
/// `kabuk msg`: `workspace next|prev`, `move`, `layout cycle`, `group toggle|next`, `close`.
#[test]
fn every_window_action_of_the_shortcuts_has_a_command() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-act-a")]);
    let b = TestWindow::open(&["--title", &title("ipc-act-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    let applied = |action: &str| sandbox.log_lines(&format!("action applied action={action}"));

    ok(&sandbox, &["workspace", "next"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["workspace"] == 2).is_some());
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(),
        "workspace 1 still shown"
    );
    ok(&sandbox, &["workspace", "prev"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["workspace"] == 1).is_some());
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some());
    assert_eq!((applied("StepWorkspace(1)"), applied("StepWorkspace(-1)")), (1, 1));

    // The focused window swaps places with its neighbour.
    let left = |w: &TestWindow| w.hwnd.frame_rect().map(|r| r.left);
    assert!(wait_until(SETTLE, || left(&a).is_some() && left(&a) != left(&b)).is_some());
    shell.focus(&a);
    let a_was_left = left(&a) < left(&b);
    ok(&sandbox, &["move", if a_was_left { "right" } else { "left" }]);
    assert!(
        wait_until(SETTLE, || (left(&a) < left(&b)) != a_was_left).is_some(),
        "the window did not move"
    );

    let layout = primary_state(&sandbox)["layout"].clone();
    ok(&sandbox, &["layout", "cycle"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["layout"] != layout).is_some());
    ok(&sandbox, &["layout", "cycle"]);
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["layout"] == layout).is_some());

    shell.focus(&a);
    ok(&sandbox, &["group", "toggle"]);
    assert!(wait_until(SETTLE, || applied("ToggleGroup") == 1).is_some());
    ok(&sandbox, &["group", "next"]);
    assert!(wait_until(SETTLE, || applied("NextTab") == 1).is_some());
    ok(&sandbox, &["group", "toggle"]);
    assert!(wait_until(SETTLE, || applied("ToggleGroup") == 2).is_some());

    shell.focus(&b);
    ok(&sandbox, &["close"]);
    assert!(wait_until(SETTLE, || !b.hwnd.exists()).is_some(), "not closed");
    assert!(a.hwnd.exists(), "the other window closed");
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M19 U4: `desktop`, `keys` and `panel <name>` do what the show-desktop corner, the
/// shortcut card's chord and a click on the bar's icons do, and a second time undo it.
#[test]
fn the_bars_actions_and_panels_have_commands() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-desk")]);
    managed(&sandbox, &a);
    ok(&sandbox, &["desktop"]);
    assert!(wait_until(SETTLE, || a.hwnd.is_minimized()).is_some(), "not put down");
    assert_eq!(sandbox.log_lines("desktop shown windows=1"), 1);
    ok(&sandbox, &["desktop"]);
    assert!(wait_until(SETTLE, || !a.hwnd.is_minimized()).is_some(), "not back");

    let open = |class: &str| {
        kabuk_win::window::windows_of_class(class)
            .into_iter()
            .any(|h| h.pid() == Some(shell.pid) && h.is_visible())
    };
    ok(&sandbox, &["keys"]);
    assert!(wait_until(SETTLE, || open("Kabuk.KeysCard")).is_some(), "no card");
    assert_eq!(sandbox.log_lines("keys card shown welcome=false"), 1);
    ok(&sandbox, &["keys"]);
    assert!(wait_until(SETTLE, || !open("Kabuk.KeysCard")).is_some(), "card stayed");

    for (name, class, line) in [
        ("quick", "Kabuk.QuickPanel", "quick settings opened"),
        ("network", "Kabuk.NetworkPanel", "network panel opened"),
        ("calendar", "Kabuk.CalendarPanel", "calendar panel opened"),
        (
            "volume",
            "Kabuk.VolumePanel",
            "volume panel opened menu=Volume with_track=true",
        ),
    ] {
        ok(&sandbox, &["panel", name]);
        assert!(wait_until(SETTLE, || open(class)).is_some(), "{class} did not open");
        assert_eq!(sandbox.log_lines(line), 1, "{line}");
        ok(&sandbox, &["panel", name]);
        assert!(wait_until(SETTLE, || !open(class)).is_some(), "{class} did not close");
    }
    assert_eq!(sandbox.restore().0, 0);
}

#[test]
fn restore_answers_then_stops_the_shell_and_shows_every_window() {
    let sandbox = Sandbox::new();
    let mut shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-restore")]);
    managed(&sandbox, &a);
    ok(&sandbox, &["workspace", "2"]);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some());
    ok(&sandbox, &["restore"]);
    let status = shell.proc.wait_exit(SETTLE).expect("the shell stopped");
    assert_eq!(status.code(), Some(exit::EMERGENCY));
    assert!(a.hwnd.is_visible(), "a hidden window was left hidden");
    let after = msg(&sandbox, &["query", "state"]);
    assert_eq!(after.code, exit::NOT_RUNNING, "{}", after.stderr);
    assert!(after.stderr.contains("Kabuk is not running"), "{}", after.stderr);
}

#[test]
fn mistakes_are_explained_and_missing_parts_reported() {
    let sandbox = Sandbox::new();
    // Without a shell: a typo is still explained, a good command finds nobody.
    let typo = msg(&sandbox, &["dance"]);
    assert_eq!(typo.code, exit::USAGE);
    assert!(
        typo.stderr.contains("dance") && typo.stderr.contains("kabuk msg"),
        "{}",
        typo.stderr
    );
    assert_eq!(msg(&sandbox, &["workspace", "12"]).code, exit::USAGE);
    assert_eq!(msg(&sandbox, &["panel", "tray"]).code, exit::USAGE);
    assert_eq!(msg(&sandbox, &["group", "make"]).code, exit::USAGE);
    assert_eq!(msg(&sandbox, &["reload"]).code, exit::NOT_RUNNING);
    // A shell without the window manager says so for its commands.
    std::fs::write(sandbox.config_file(), "schema = 1\n[language]\nlanguage = \"en\"\n").unwrap();
    let _shell = sandbox.shell_with_bar(&[]);
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("command pipe open") == 1).is_some());
    let no_wm = msg(&sandbox, &["workspace", "2"]);
    assert_eq!(no_wm.code, exit::COMMAND_FAILED);
    assert!(no_wm.stderr.contains("The window manager is off"), "{}", no_wm.stderr);
    assert_eq!(sandbox.restore().0, 0);
}

/// PLAN M7: the pipe is open only to the current user.
#[test]
fn the_shells_pipe_is_open_only_to_the_current_user() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start(&sandbox);
    let sid = kabuk_win::pipe::user_sid().unwrap();
    let name = kabuk_core::ipc::pipe_name(&sid, Instance::for_state_dir(sandbox.state_dir()).id());
    assert_eq!(kabuk_win::pipe::allowed_sids(&name).unwrap(), vec![sid]);
    // Commands still work after the ACL check's connection.
    ok(&sandbox, &["query", "state"]);
    assert_eq!(sandbox.restore().0, 0);
}

/// SPEC §2.4 and TESTING §4: a full-screen window turns game mode on within a second
/// (seen in `query state`); the mouse hook goes, tiling pauses (a new window keeps its
/// own place, the others stay), and once the window is gone the layout is as it was. No
/// notice was held back, so none tells it ended (every full-screen video ended with one).
#[test]
fn a_full_screen_window_turns_game_mode_on_and_the_layout_comes_back() {
    use kabuk::bar::manager::MSG_APPBAR;
    use kabuk_win::msgloop::post_message;
    const ABN_FULLSCREENAPP: usize = 2;
    const GAME_MODE_LIMIT: Duration = Duration::from_secs(1);
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("game-a")]);
    let b = TestWindow::open(&["--title", &title("game-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    let frames = || (a.hwnd.frame_rect(), b.hwnd.frame_rect());
    let apart = |f: (Option<kabuk_core::state::Rect>, Option<kabuk_core::state::Rect>)| matches!(f, (Some(x), Some(y)) if x.left != y.left);
    assert!(wait_until(SETTLE, || apart(frames())).is_some());
    std::thread::sleep(Duration::from_millis(500));
    let before = frames();

    let game = TestWindow::open(&["--fullscreen", "--title", &title("game")]);
    assert!(wait_until(SETTLE, || game.hwnd.is_visible()).is_some());
    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary).unwrap();
    let bar = kabuk_win::window::windows_of_class("Kabuk.Bar")
        .into_iter()
        .filter(|w| w.pid() == Some(shell.pid))
        .find(|w| w.window_rect().is_some_and(|r| primary.rect.contains(&r)))
        .expect("primary bar");
    let notify = |entering: bool| {
        post_message(
            bar,
            Msg {
                id: MSG_APPBAR,
                wparam: ABN_FULLSCREENAPP,
                lparam: isize::from(entering),
            },
        )
        .unwrap();
    };
    notify(true);
    assert!(
        wait_until(GAME_MODE_LIMIT, || primary_state(&sandbox)["game_mode"] == true).is_some(),
        "game mode not on within {GAME_MODE_LIMIT:?}"
    );
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("mouse hook removed for a full-screen program")
        == 1)
    .is_some());
    // A window opened meanwhile keeps its place; the tiled ones stay.
    let c = TestWindow::open(&["--title", &title("game-c")]);
    managed(&sandbox, &c);
    assert_eq!(window_entry(&sandbox, &c)["floating"], true);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(frames(), before, "the layout moved during game mode");

    // Out of full screen (closing it could leave the user's own full-screen video in front).
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
    assert!(wait_until(SETTLE, || primary_state(&sandbox)["game_mode"] == false).is_some());
    assert!(wait_until(SETTLE, || sandbox
        .log_lines("mouse hook back after a full-screen program")
        == 1)
    .is_some());
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        sandbox.log_lines("notice shown"),
        0,
        "{}",
        sandbox.last_log_line(&["notice"]).unwrap_or_default()
    );
    assert_eq!(sandbox.restore().0, 0);
}

/// SPEC §2.6: Kabuk tells what it did in the look's notice; here a window dragged off
/// its tile ("Window is floating").
#[test]
fn a_window_floated_by_a_drag_is_told_in_a_notice() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start_with(&sandbox, NO_GAME);
    let a = TestWindow::open(&["--title", &title("notice-a")]);
    let b = TestWindow::open(&["--title", &title("notice-b")]);
    managed(&sandbox, &a);
    managed(&sandbox, &b);
    b.fake_drag(-200, 80);
    assert!(wait_until(SETTLE, || sandbox.log_lines("window floated by a drag") == 1).is_some());
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown title=Window is floating")
            == 1)
        .is_some(),
        "{}",
        sandbox.last_log_line(&["notice"]).unwrap_or_default()
    );
    let pid = sandbox.state().kabuk_pid.unwrap();
    let shown = kabuk_win::window::windows_of_class("Kabuk.Notice")
        .into_iter()
        .any(|w| w.pid() == Some(pid) && w.is_visible());
    assert!(shown, "no notice window");
    assert_eq!(sandbox.restore().0, 0);
}

/// M10: a notice fades in, and when its time is up fades out while the mouse goes
/// through it, then its window goes. With Windows' "Animation effects" off it comes and
/// goes at once.
#[test]
fn a_notice_fades_in_and_out_letting_the_mouse_through() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start_with(&sandbox, NO_GAME);
    let animates = kabuk_win::system::animations_on();
    let pid = sandbox.state().kabuk_pid.unwrap();
    let notices = || {
        kabuk_win::window::windows_of_class("Kabuk.Notice")
            .into_iter()
            .filter(|w| w.pid() == Some(pid) && w.is_visible())
            .collect::<Vec<_>>()
    };
    let frames = kabuk_win::screen::Frames::now();
    let _windows = notice_by_drag(&sandbox, "fade");
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown title=Window is floating")
            == 1)
        .is_some()
    );
    let enters = sandbox.last_log_line(&["popup enters"]).expect("entrance logged");
    if animates {
        assert!(enters.contains("level=Full") && enters.contains("ms=220"), "{enters}");
    } else {
        assert!(enters.contains("level=Off") && enters.contains("ms=0"), "{enters}");
    }
    let shown = notices();
    assert_eq!(shown.len(), 1);
    assert!(!shown[0].passes_input(), "a shown notice takes clicks");
    let r = kabuk_win::dpi::physical(|| shown[0].window_rect()).expect("notice rect");
    let area = kabuk_win::msgloop::PxRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    let look = || kabuk_win::dpi::physical(|| kabuk_win::screen::capture(area)).expect("screen");
    // Settled after its entrance.
    std::thread::sleep(Duration::from_millis(600));
    let full = look();
    // M10: the compositor kept up with the entrance (no frame missed or dropped).
    let entered = frames.since(kabuk_win::screen::Frames::now());
    assert!(
        entered.composed > 0 && entered.missed == 0 && entered.dropped == 0,
        "{entered:?}"
    );
    // `showToast`'s 3.6 s, then the exit.
    let left = wait_until(Duration::from_secs(6), || shown[0].passes_input() || !shown[0].exists());
    assert!(left.is_some(), "the notice did not leave");
    // The window lets the mouse through as soon as the exit is asked for, before the
    // compositor shows its first faded frame: look a little into the 220 ms exit.
    std::thread::sleep(INTO_EXIT);
    let leaving = look();
    let frames = kabuk_win::screen::Frames::now();
    if animates {
        assert_eq!(sandbox.log_lines("popup leaves"), 1);
        assert!(
            shown[0].exists() && shown[0].passes_input(),
            "the notice went without its exit"
        );
    }
    let gone = wait_until(Duration::from_secs(2), || !shown[0].exists());
    assert!(gone.is_some(), "the notice stayed after its exit");
    let left = frames.since(kabuk_win::screen::Frames::now());
    assert!(left.missed == 0 && left.dropped == 0, "{left:?}");
    assert!(notices().is_empty());
    let after = look();
    // The notice was drawn, and while it fades it still shows (letting the mouse through
    // must not blank its window).
    let drawn = kabuk_win::screen::difference(&full, &after);
    assert!(drawn > 2.0, "the notice did not show: {drawn}");
    if animates {
        let fading = kabuk_win::screen::difference(&leaving, &after);
        assert!(fading > drawn * 0.2, "blank while fading: {fading} of {drawn}");
        // Already changing from the full picture (the test windows behind are not
        // painted, so the background itself may differ between reads).
        let moved = kabuk_win::screen::difference(&leaving, &full);
        assert!(moved > 1.0, "not fading: {moved} from the full picture");
    }
    assert_eq!(sandbox.restore().0, 0);
}

/// M10: `appearance.animations = "off"` shows and removes a notice at once.
#[test]
fn with_animations_off_a_notice_comes_and_goes_at_once() {
    let sandbox = Sandbox::new();
    let _shell = Shell::start_with(&sandbox, &format!("{NO_GAME}[appearance]\nanimations = \"off\"\n"));
    let _windows = notice_by_drag(&sandbox, "fade-off");
    assert!(
        wait_until(SETTLE, || sandbox.log_lines("notice shown title=Window is floating")
            == 1)
        .is_some()
    );
    let enters = sandbox.last_log_line(&["popup enters"]).expect("entrance logged");
    assert!(enters.contains("level=Off") && enters.contains("ms=0"), "{enters}");
    let pid = sandbox.state().kabuk_pid.unwrap();
    let shown: Vec<_> = kabuk_win::window::windows_of_class("Kabuk.Notice")
        .into_iter()
        .filter(|w| w.pid() == Some(pid))
        .collect();
    assert_eq!(shown.len(), 1);
    assert!(wait_until(Duration::from_secs(6), || !shown[0].exists()).is_some());
    assert_eq!(sandbox.log_lines("popup leaves"), 0);
    assert_eq!(sandbox.restore().0, 0);
}

/// Two tiled test windows, one dragged off its tile, which Kabuk tells in a notice.
fn notice_by_drag(sandbox: &Sandbox, name: &str) -> (TestWindow, TestWindow) {
    let a = TestWindow::open(&["--title", &title(&format!("{name}-a"))]);
    let b = TestWindow::open(&["--title", &title(&format!("{name}-b"))]);
    managed(sandbox, &a);
    managed(sandbox, &b);
    b.fake_drag(-200, 80);
    (a, b)
}

/// D34: `scratchpad send` hides the focused window on the hidden workspace and
/// `scratchpad toggle` shows it there, then hides it again.
#[test]
fn scratchpad_send_and_toggle_hide_and_show_the_window() {
    let sandbox = Sandbox::new();
    let shell = Shell::start(&sandbox);
    let a = TestWindow::open(&["--title", &title("ipc-scratch")]);
    managed(&sandbox, &a);
    shell.focus(&a);
    ok(&sandbox, &["scratchpad", "send"]);
    assert!(wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(), "not sent");
    let e = window_entry(&sandbox, &a);
    assert_eq!(e["workspace"].as_u64(), Some(0), "{e}");
    ok(&sandbox, &["scratchpad", "toggle"]);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some(), "not shown");
    ok(&sandbox, &["scratchpad", "toggle"]);
    assert!(
        wait_until(SETTLE, || !a.hwnd.is_visible()).is_some(),
        "not hidden again"
    );
    assert_eq!(sandbox.restore().0, 0);
    assert!(wait_until(SETTLE, || a.hwnd.is_visible()).is_some());
}
