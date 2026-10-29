//! The command interface (ARCHITECTURE §12, SPEC §8.23): `kabuk msg <command>` and any
//! other program talk to the running shell over a named pipe, one JSON object per line.
//! Pure: the commands, their parsing and the wire format.
//!
//! A request carries the command's words (`{"args":["workspace","3"]}`), so the client
//! and the server share one parser; the answer is `{"ok":true,"result":…}` or
//! `{"ok":false,"error":"…"}`.

use serde::{Deserialize, Serialize};

use crate::wm::{Action, Direction};

/// The commands of ARCHITECTURE §12: every action of `[shortcuts]` and the bar's panels
/// (SPEC §8.23).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `workspace <n>`: switch the focused monitor to workspace n.
    Workspace(u32),
    /// `workspace next|prev`: switch the focused monitor this many workspaces on.
    StepWorkspace(i32),
    /// `move-to <n>`: move the focused window to workspace n.
    MoveTo(u32),
    /// `focus <left|down|up|right>`.
    Focus(Direction),
    /// `move <left|down|up|right>`: swap the focused window with its neighbour that way.
    Move(Direction),
    /// `float toggle`.
    FloatToggle,
    /// `float on|off`: float the focused window or tile it, whatever it was (PLAN M19 U4).
    Float(bool),
    /// `close`: close the focused window.
    Close,
    /// `scratchpad toggle`: show or hide the hidden workspace on the focused monitor.
    ScratchpadToggle,
    /// `scratchpad send`: send the focused window to the hidden workspace.
    ScratchpadSend,
    /// `group toggle`: make the focused window a tabbed group, or break its group up.
    GroupToggle,
    /// `group next`: show the next tab of the focused group.
    NextTab,
    /// `layout <dwindle|master|floating|tiling>` on the focused monitor's workspace.
    Layout(LayoutName),
    /// `layout cycle`: the focused monitor's workspace takes its next layout.
    CycleLayout,
    /// `dropdown`: drop the terminal down, or hide it (DECISIONS D62).
    Dropdown,
    /// `desktop`: put every window down, or bring them back (DECISIONS D88).
    ShowDesktop,
    /// `reload`: read `kabuk.toml` again.
    Reload,
    /// `query <state|windows|monitors|tray>`: JSON about the running shell.
    Query(QueryKind),
    /// `launcher`: open the launcher.
    Launcher,
    /// `overview`: open the overview of every workspace.
    Overview,
    /// `saver`: start the screen saver now.
    Saver,
    /// `apps`: open the grid of every app (DECISIONS D56).
    AppGrid,
    /// `keys`: show or hide the card of Kabuk's shortcuts (DECISIONS D61).
    KeysCard,
    /// `panel <quick|network|calendar|volume>`: open or close one of the bar's panels, as
    /// a click on its icon does.
    Panel(PanelName),
    /// A layout profile opens (DECISIONS D65).
    Profile(String),
    /// `restore`: emergency stop and restore, as `kabuk --restore`.
    Restore,
    /// `quiet on|off|toggle`: Kabuk's do not disturb (PLAN M19 U20); `None` toggles.
    Quiet(Option<bool>),
    /// `started-again`: a second `kabuk` that found this one running and gives way to it
    /// tells it so, and the user is told (PLAN M19 U20). Not for people: not in the usage.
    StartedAgain,
}

impl Request {
    /// The window manager's action this command does, the same as its shortcut; `None`
    /// for the commands the bar or the shell itself carries out.
    pub fn action(&self) -> Option<Action> {
        Some(match *self {
            Request::Workspace(n) => Action::Workspace(n),
            Request::StepWorkspace(by) => Action::StepWorkspace(by),
            Request::MoveTo(n) => Action::MoveToWorkspace(n),
            Request::Focus(d) => Action::Focus(d),
            Request::Move(d) => Action::Move(d),
            Request::FloatToggle => Action::ToggleFloat,
            Request::Float(on) => Action::SetFloat(on),
            Request::Close => Action::Close,
            Request::ScratchpadToggle => Action::ToggleScratchpad,
            Request::ScratchpadSend => Action::ToScratchpad,
            Request::GroupToggle => Action::ToggleGroup,
            Request::NextTab => Action::NextTab,
            Request::CycleLayout => Action::CycleLayout,
            Request::Dropdown => Action::Dropdown,
            Request::Layout(_)
            | Request::ShowDesktop
            | Request::Reload
            | Request::Query(_)
            | Request::Launcher
            | Request::Overview
            | Request::Saver
            | Request::AppGrid
            | Request::KeysCard
            | Request::Panel(_)
            | Request::Profile(_)
            | Request::Restore
            | Request::Quiet(_)
            | Request::StartedAgain => return None,
        })
    }

    /// The command opens something that takes the keyboard, so the client lets the
    /// shell come to the front.
    pub fn takes_keyboard(&self) -> bool {
        matches!(
            self,
            Request::Launcher | Request::Overview | Request::AppGrid | Request::KeysCard | Request::Panel(_)
        )
    }
}

/// The bar's panels `panel <name>` opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelName {
    /// The quick settings (Wi-Fi, Bluetooth, volume, brightness…).
    Quick,
    /// The network's list of Wi-Fi networks.
    Network,
    /// The calendar with the notifications, or Windows' notification centre when Kabuk's
    /// calendar is off, as a click on the clock.
    Calendar,
    /// The volume's level and mixer.
    Volume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutName {
    Dwindle,
    Master,
    Floating,
    Tiling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    State,
    Windows,
    Monitors,
    /// Other programs' tray icons the bar holds, with their programs.
    Tray,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IpcError {
    #[error("no command given")]
    Empty,
    #[error("unknown command `{0}`")]
    Unknown(String),
    #[error("`{command}` expects {expected}")]
    BadArgs { command: String, expected: &'static str },
    #[error("unreadable message: {0}")]
    Wire(String),
}

/// Every command's first word, as the usage text (`cli-usage`) lists them.
pub const COMMANDS: &[&str] = &[
    "workspace",
    "move-to",
    "focus",
    "move",
    "float",
    "close",
    "scratchpad",
    "group",
    "layout",
    "dropdown",
    "desktop",
    "launcher",
    "overview",
    "apps",
    "keys",
    "saver",
    "panel",
    "profile",
    "reload",
    "query",
    "quiet",
    "restore",
];

/// The word of [`Request::StartedAgain`].
pub const STARTED_AGAIN: &str = "started-again";

/// The workspace numbers commands accept (SPEC §2.2: 1-9).
pub const WORKSPACES: std::ops::RangeInclusive<u32> = 1..=9;

fn workspace(command: &str, arg: Option<&str>, expected: &'static str) -> Result<u32, IpcError> {
    arg.and_then(|a| a.parse().ok())
        .filter(|n| WORKSPACES.contains(n))
        .ok_or_else(|| IpcError::BadArgs {
            command: command.into(),
            expected,
        })
}

fn direction(arg: Option<&str>) -> Option<Direction> {
    match arg {
        Some("left") => Some(Direction::Left),
        Some("down") => Some(Direction::Down),
        Some("up") => Some(Direction::Up),
        Some("right") => Some(Direction::Right),
        _ => None,
    }
}

/// Parses a command's words (`["workspace", "3"]`).
pub fn parse(words: &[String]) -> Result<Request, IpcError> {
    let (command, rest) = words.split_first().ok_or(IpcError::Empty)?;
    let arg = rest.first().map(String::as_str);
    let one = |expected: &'static str| IpcError::BadArgs {
        command: command.clone(),
        expected,
    };
    // Exactly the words a command takes; anything after them is a mistake.
    let takes = |n: usize, expected: &'static str| if rest.len() == n { Ok(()) } else { Err(one(expected)) };
    let r = match command.as_str() {
        "workspace" => {
            const EXPECTED: &str = "a workspace number 1-9, next or prev";
            takes(1, EXPECTED)?;
            match arg {
                Some("next") => Request::StepWorkspace(1),
                Some("prev") => Request::StepWorkspace(-1),
                _ => Request::Workspace(workspace(command, arg, EXPECTED)?),
            }
        }
        "move-to" => {
            const EXPECTED: &str = "a workspace number 1-9";
            takes(1, EXPECTED)?;
            Request::MoveTo(workspace(command, arg, EXPECTED)?)
        }
        "focus" => {
            takes(1, "left, down, up or right")?;
            Request::Focus(direction(arg).ok_or_else(|| one("left, down, up or right"))?)
        }
        "move" => {
            takes(1, "left, down, up or right")?;
            Request::Move(direction(arg).ok_or_else(|| one("left, down, up or right"))?)
        }
        "float" => {
            const EXPECTED: &str = "toggle, on or off";
            takes(1, EXPECTED)?;
            match arg {
                Some("toggle") => Request::FloatToggle,
                Some("on") => Request::Float(true),
                Some("off") => Request::Float(false),
                _ => return Err(one(EXPECTED)),
            }
        }
        "close" => {
            takes(0, "no arguments")?;
            Request::Close
        }
        "scratchpad" => {
            takes(1, "toggle or send")?;
            match arg {
                Some("toggle") => Request::ScratchpadToggle,
                Some("send") => Request::ScratchpadSend,
                _ => return Err(one("toggle or send")),
            }
        }
        "group" => {
            takes(1, "toggle or next")?;
            match arg {
                Some("toggle") => Request::GroupToggle,
                Some("next") => Request::NextTab,
                _ => return Err(one("toggle or next")),
            }
        }
        "layout" => {
            const EXPECTED: &str = "dwindle, master, floating, tiling or cycle";
            takes(1, EXPECTED)?;
            match arg {
                Some("dwindle") => Request::Layout(LayoutName::Dwindle),
                Some("master") => Request::Layout(LayoutName::Master),
                Some("floating") => Request::Layout(LayoutName::Floating),
                Some("tiling") => Request::Layout(LayoutName::Tiling),
                Some("cycle") => Request::CycleLayout,
                _ => return Err(one(EXPECTED)),
            }
        }
        "dropdown" => {
            takes(0, "no arguments")?;
            Request::Dropdown
        }
        "desktop" => {
            takes(0, "no arguments")?;
            Request::ShowDesktop
        }
        "keys" => {
            takes(0, "no arguments")?;
            Request::KeysCard
        }
        "panel" => {
            const EXPECTED: &str = "quick, network, calendar or volume";
            takes(1, EXPECTED)?;
            Request::Panel(match arg {
                Some("quick") => PanelName::Quick,
                Some("network") => PanelName::Network,
                Some("calendar") => PanelName::Calendar,
                Some("volume") => PanelName::Volume,
                _ => return Err(one(EXPECTED)),
            })
        }
        "query" => {
            takes(1, "state, windows, monitors or tray")?;
            Request::Query(match arg {
                Some("state") => QueryKind::State,
                Some("windows") => QueryKind::Windows,
                Some("monitors") => QueryKind::Monitors,
                Some("tray") => QueryKind::Tray,
                _ => return Err(one("state, windows, monitors or tray")),
            })
        }
        "reload" => {
            takes(0, "no arguments")?;
            Request::Reload
        }
        "launcher" => {
            takes(0, "no arguments")?;
            Request::Launcher
        }
        "overview" => {
            takes(0, "no arguments")?;
            Request::Overview
        }
        "saver" => {
            takes(0, "no arguments")?;
            Request::Saver
        }
        "apps" => {
            takes(0, "no arguments")?;
            Request::AppGrid
        }
        "profile" => {
            // The name may have spaces; it is the rest of the words.
            let name = rest.join(" ");
            if name.trim().is_empty() {
                return Err(one("a layout profile's name"));
            }
            Request::Profile(name.trim().to_string())
        }
        "restore" => {
            takes(0, "no arguments")?;
            Request::Restore
        }
        "quiet" => {
            const EXPECTED: &str = "on, off or toggle";
            takes(1, EXPECTED)?;
            Request::Quiet(match arg {
                Some("on") => Some(true),
                Some("off") => Some(false),
                Some("toggle") => None,
                _ => return Err(one(EXPECTED)),
            })
        }
        STARTED_AGAIN => {
            takes(0, "no arguments")?;
            Request::StartedAgain
        }
        other => return Err(IpcError::Unknown(other.to_string())),
    };
    Ok(r)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Wire {
    args: Vec<String>,
}

/// The request line for a command's words (newline included).
pub fn request_line(words: &[String]) -> String {
    let mut s = serde_json::to_string(&Wire { args: words.to_vec() }).unwrap_or_default();
    s.push('\n');
    s
}

/// The words of a request line.
pub fn read_request(line: &str) -> Result<Vec<String>, IpcError> {
    serde_json::from_str::<Wire>(line.trim())
        .map(|w| w.args)
        .map_err(|e| IpcError::Wire(e.to_string()))
}

/// The server's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn done() -> Response {
        Response {
            ok: true,
            result: None,
            error: None,
        }
    }

    pub fn with(result: serde_json::Value) -> Response {
        Response {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Response {
        Response {
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }

    /// The answer line (newline included).
    pub fn line(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_else(|_| r#"{"ok":false}"#.into());
        s.push('\n');
        s
    }

    pub fn read(line: &str) -> Result<Response, IpcError> {
        serde_json::from_str(line.trim()).map_err(|e| IpcError::Wire(e.to_string()))
    }
}

/// The pipe of one user's instance: the user's SID keeps users apart, the instance id
/// (from the state directory) keeps a test's shell apart from the user's.
pub fn pipe_name(user_sid: &str, instance_id: &str) -> String {
    format!(r"\\.\pipe\kabuk-{user_sid}-{instance_id}")
}

#[cfg(test)]
mod tests {
    //! Edge cases: every command of ARCHITECTURE §12, workspace numbers at and past the
    //! ends, missing, extra and wrong arguments, an unknown and an empty command; every
    //! `[shortcuts]` entry against its command's action (and `drag`, no action); the wire
    //! format both ways with Turkish text, a broken line, a response without optional
    //! fields; the pipe name.
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn p(s: &str) -> Result<Request, IpcError> {
        parse(&words(s))
    }

    #[test]
    fn every_command_parses() {
        assert_eq!(p("workspace 3"), Ok(Request::Workspace(3)));
        assert_eq!(p("move-to 9"), Ok(Request::MoveTo(9)));
        assert_eq!(p("focus left"), Ok(Request::Focus(Direction::Left)));
        assert_eq!(p("focus right"), Ok(Request::Focus(Direction::Right)));
        assert_eq!(p("float toggle"), Ok(Request::FloatToggle));
        assert_eq!(p("scratchpad toggle"), Ok(Request::ScratchpadToggle));
        assert_eq!(p("scratchpad send"), Ok(Request::ScratchpadSend));
        assert_eq!(p("layout master"), Ok(Request::Layout(LayoutName::Master)));
        assert_eq!(p("layout floating"), Ok(Request::Layout(LayoutName::Floating)));
        assert_eq!(p("reload"), Ok(Request::Reload));
        assert_eq!(p("query state"), Ok(Request::Query(QueryKind::State)));
        assert_eq!(p("query windows"), Ok(Request::Query(QueryKind::Windows)));
        assert_eq!(p("query monitors"), Ok(Request::Query(QueryKind::Monitors)));
        assert_eq!(p("query tray"), Ok(Request::Query(QueryKind::Tray)));
        assert_eq!(p("launcher"), Ok(Request::Launcher));
        assert_eq!(p("overview"), Ok(Request::Overview));
        assert_eq!(p("saver"), Ok(Request::Saver));
        assert_eq!(p("apps"), Ok(Request::AppGrid));
        assert_eq!(p("profile Kodlama gece"), Ok(Request::Profile("Kodlama gece".into())));
        assert!(matches!(p("profile"), Err(IpcError::BadArgs { .. })));
        assert_eq!(p("restore"), Ok(Request::Restore));
        assert_eq!(p("quiet on"), Ok(Request::Quiet(Some(true))));
        assert_eq!(p("quiet off"), Ok(Request::Quiet(Some(false))));
        assert_eq!(p("quiet toggle"), Ok(Request::Quiet(None)));
        assert_eq!(p(STARTED_AGAIN), Ok(Request::StartedAgain));
        for bad in ["quiet", "quiet maybe", "quiet on now", "started-again now"] {
            assert!(matches!(p(bad), Err(IpcError::BadArgs { .. })), "{bad}");
        }
        assert!(!COMMANDS.contains(&STARTED_AGAIN), "not for people");
    }

    /// SPEC §8.23: every action has a command (PLAN M19 U4).
    #[test]
    fn every_action_has_a_command() {
        assert_eq!(p("workspace next"), Ok(Request::StepWorkspace(1)));
        assert_eq!(p("workspace prev"), Ok(Request::StepWorkspace(-1)));
        assert_eq!(p("move left"), Ok(Request::Move(Direction::Left)));
        assert_eq!(p("move down"), Ok(Request::Move(Direction::Down)));
        assert_eq!(p("move up"), Ok(Request::Move(Direction::Up)));
        assert_eq!(p("move right"), Ok(Request::Move(Direction::Right)));
        assert_eq!(p("focus down"), Ok(Request::Focus(Direction::Down)));
        assert_eq!(p("focus up"), Ok(Request::Focus(Direction::Up)));
        assert_eq!(p("close"), Ok(Request::Close));
        assert_eq!(p("layout cycle"), Ok(Request::CycleLayout));
        assert_eq!(p("layout dwindle"), Ok(Request::Layout(LayoutName::Dwindle)));
        assert_eq!(p("layout tiling"), Ok(Request::Layout(LayoutName::Tiling)));
        assert_eq!(p("group toggle"), Ok(Request::GroupToggle));
        assert_eq!(p("group next"), Ok(Request::NextTab));
        assert_eq!(p("dropdown"), Ok(Request::Dropdown));
        assert_eq!(p("desktop"), Ok(Request::ShowDesktop));
        assert_eq!(p("keys"), Ok(Request::KeysCard));
        assert_eq!(p("panel quick"), Ok(Request::Panel(PanelName::Quick)));
        assert_eq!(p("panel network"), Ok(Request::Panel(PanelName::Network)));
        assert_eq!(p("panel calendar"), Ok(Request::Panel(PanelName::Calendar)));
        assert_eq!(p("panel volume"), Ok(Request::Panel(PanelName::Volume)));
        // The shortcut toggles; the command line also sets it either way.
        assert_eq!(p("float on"), Ok(Request::Float(true)));
        assert_eq!(p("float off"), Ok(Request::Float(false)));
        assert_eq!(p("float on").unwrap().action(), Some(Action::SetFloat(true)));
        assert_eq!(p("float off").unwrap().action(), Some(Action::SetFloat(false)));
    }

    #[test]
    fn the_new_commands_bad_forms_are_explained() {
        for bad in [
            "workspace next 2",
            "workspace previous",
            "workspace Next",
            "move",
            "move north",
            "move left now",
            "close 1",
            "close all",
            "layout",
            "layout cycle now",
            "layout next",
            "group",
            "group make",
            "group toggle now",
            "dropdown x",
            "desktop show",
            "keys all",
            "panel",
            "panel clock",
            "panel quick now",
        ] {
            assert!(matches!(p(bad), Err(IpcError::BadArgs { .. })), "{bad}");
        }
        assert_eq!(
            p("move").unwrap_err().to_string(),
            "`move` expects left, down, up or right"
        );
        assert_eq!(
            p("panel tray").unwrap_err().to_string(),
            "`panel` expects quick, network, calendar or volume"
        );
        assert_eq!(p("tab"), Err(IpcError::Unknown("tab".into())));
    }

    /// The command for each `[shortcuts]` entry (the schema's keys), which must do what
    /// the shortcut does. `drag` is no action: it is the key held while dragging.
    const SHORTCUT_COMMANDS: &[(&str, &str)] = &[
        ("launcher", "launcher"),
        ("workspace", "workspace 1"),
        ("move_to_workspace", "move-to 1"),
        ("focus_left", "focus left"),
        ("focus_down", "focus down"),
        ("focus_up", "focus up"),
        ("focus_right", "focus right"),
        ("move_left", "move left"),
        ("move_down", "move down"),
        ("move_up", "move up"),
        ("move_right", "move right"),
        ("toggle_float", "float toggle"),
        ("close_window", "close"),
        ("cycle_layout", "layout cycle"),
        ("toggle_scratchpad", "scratchpad toggle"),
        ("to_scratchpad", "scratchpad send"),
        ("toggle_group", "group toggle"),
        ("next_tab", "group next"),
        ("overview", "overview"),
        ("start_saver", "saver"),
        ("app_grid", "apps"),
        ("keys_card", "keys"),
        ("dropdown", "dropdown"),
    ];
    const NOT_AN_ACTION: &[&str] = &["drag"];

    #[test]
    fn every_shortcut_has_a_command_doing_the_same() {
        use crate::config::shortcut::{Chord, Key, Mods};
        use crate::config::Shortcuts;

        let defaults = serde_json::to_value(Shortcuts::default()).unwrap();
        let fields = defaults.as_object().expect("a table of shortcuts");
        let mut listed: Vec<&str> = SHORTCUT_COMMANDS.iter().map(|(f, _)| *f).collect();
        listed.extend(NOT_AN_ACTION);
        listed.sort_unstable();
        let mut keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, listed, "a shortcut without a command, or a command for none");

        let bound = crate::wm::bindings(&Shortcuts::default(), *WORKSPACES.end());
        let mut seen = Vec::new();
        for (field, command) in SHORTCUT_COMMANDS {
            let request = p(command).unwrap_or_else(|e| panic!("{field}: `{command}`: {e}"));
            // The default chord of the field: its modifiers with 1 for the workspaces.
            let chord = match &fields[*field] {
                serde_json::Value::String(mods) => Chord {
                    mods: Mods::parse(mods).unwrap(),
                    key: Key::Digit(1),
                },
                serde_json::Value::Array(list) => Chord::parse(list[0].as_str().unwrap()).unwrap(),
                other => panic!("{field}: {other}"),
            };
            let action = bound.iter().find(|(c, _)| *c == chord).map(|(_, a)| *a);
            match action {
                // The window manager's: the command's action is the shortcut's.
                Some(a) => assert_eq!(request.action(), Some(a), "{field}: `{command}`"),
                // The bar's: one of its own commands, no window manager action.
                None => {
                    assert_eq!(request.action(), None, "{field}: `{command}`");
                    assert!(
                        matches!(
                            request,
                            Request::Launcher
                                | Request::Overview
                                | Request::Saver
                                | Request::AppGrid
                                | Request::KeysCard
                        ),
                        "{field}: `{command}` is {request:?}"
                    );
                }
            }
            assert!(!seen.contains(&request), "{field}: `{command}` used twice");
            seen.push(request);
        }
    }

    #[test]
    fn the_list_of_commands_is_the_parsers() {
        for name in COMMANDS {
            assert!(!matches!(p(name), Err(IpcError::Unknown(_))), "{name}");
        }
        for (field, command) in SHORTCUT_COMMANDS {
            let name = command.split_whitespace().next().unwrap();
            assert!(COMMANDS.contains(&name), "{field}: `{name}` not listed");
        }
    }

    #[test]
    fn commands_for_the_bar_and_the_shell_have_no_window_manager_action() {
        for command in [
            "layout master",
            "desktop",
            "reload",
            "query state",
            "launcher",
            "overview",
            "saver",
            "apps",
            "keys",
            "panel quick",
            "profile Kodlama",
            "restore",
            "quiet toggle",
            "started-again",
        ] {
            assert_eq!(p(command).unwrap().action(), None, "{command}");
        }
    }

    #[test]
    fn what_opens_with_the_keyboard_lets_the_shell_come_to_the_front() {
        for command in ["launcher", "overview", "apps", "keys", "panel network"] {
            assert!(p(command).unwrap().takes_keyboard(), "{command}");
        }
        for command in ["workspace 2", "close", "desktop", "saver", "dropdown", "reload"] {
            assert!(!p(command).unwrap().takes_keyboard(), "{command}");
        }
    }

    #[test]
    fn bad_commands_are_explained() {
        assert_eq!(p(""), Err(IpcError::Empty));
        assert_eq!(p("dance"), Err(IpcError::Unknown("dance".into())));
        for bad in [
            "workspace",
            "workspace 0",
            "workspace 10",
            "workspace x",
            "workspace 1 2",
            "move-to",
            "focus",
            "focus north",
            "float",
            "float maybe",
            "float on now",
            "scratchpad",
            "scratchpad show",
            "scratchpad toggle now",
            "layout grid",
            "query",
            "query all",
            "reload now",
            "launcher x",
            "overview now",
            "apps all",
            "restore 1",
        ] {
            assert!(matches!(p(bad), Err(IpcError::BadArgs { .. })), "{bad}");
        }
        let e = p("workspace 12").unwrap_err();
        assert_eq!(
            e.to_string(),
            "`workspace` expects a workspace number 1-9, next or prev"
        );
        assert_eq!(
            p("float up").unwrap_err().to_string(),
            "`float` expects toggle, on or off"
        );
        let e = p("move-to 12").unwrap_err();
        assert_eq!(e.to_string(), "`move-to` expects a workspace number 1-9");
    }

    #[test]
    fn the_wire_format_round_trips() {
        let w = vec!["query".to_string(), "windows".into(), "ışık İ".into()];
        let line = request_line(&w);
        assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
        assert_eq!(read_request(&line), Ok(w));
        assert!(matches!(read_request("{nope"), Err(IpcError::Wire(_))));

        let r = Response::with(serde_json::json!({"workspaces": [1, 2]}));
        assert_eq!(Response::read(&r.line()).unwrap(), r);
        assert_eq!(Response::done().line(), "{\"ok\":true}\n");
        let f = Response::read(&Response::failed("hata: ç").line()).unwrap();
        assert_eq!((f.ok, f.error.as_deref()), (false, Some("hata: ç")));
        assert!(Response::read("").is_err());
    }

    #[test]
    fn the_pipe_belongs_to_a_user_and_an_instance() {
        assert_eq!(pipe_name("S-1-5-21-1", "00ff"), r"\\.\pipe\kabuk-S-1-5-21-1-00ff");
    }
}
