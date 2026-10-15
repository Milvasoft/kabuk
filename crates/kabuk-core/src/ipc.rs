//! The command interface (ARCHITECTURE §12, SPEC §8.23): `kabuk msg <command>` and any
//! other program talk to the running shell over a named pipe, one JSON object per line.
//! Pure: the commands, their parsing and the wire format.
//!
//! A request carries the command's words (`{"args":["workspace","3"]}`), so the client
//! and the server share one parser; the answer is `{"ok":true,"result":…}` or
//! `{"ok":false,"error":"…"}`.

use serde::{Deserialize, Serialize};

use crate::wm::Direction;

/// The commands of ARCHITECTURE §12.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `workspace <n>`: switch the focused monitor to workspace n.
    Workspace(u32),
    /// `move-to <n>`: move the focused window to workspace n.
    MoveTo(u32),
    /// `focus <left|down|up|right>`.
    Focus(Direction),
    /// `float toggle`.
    FloatToggle,
    /// `scratchpad toggle`: show or hide the hidden workspace on the focused monitor.
    ScratchpadToggle,
    /// `scratchpad send`: send the focused window to the hidden workspace.
    ScratchpadSend,
    /// `layout <dwindle|master|floating|tiling>` on the focused monitor's workspace.
    Layout(LayoutName),
    /// `reload`: read `kabuk.toml` again.
    Reload,
    /// `query <state|windows|monitors>`: JSON about the running shell.
    Query(QueryKind),
    /// `launcher`: open the launcher.
    Launcher,
    /// `overview`: open the overview of every workspace.
    Overview,
    /// `saver`: start the screen saver now.
    Saver,
    /// `apps`: open the grid of every app (DECISIONS D56).
    AppGrid,
    /// A layout profile opens (DECISIONS D65).
    Profile(String),
    /// `restore`: emergency stop and restore, as `kabuk --restore`.
    Restore,
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

/// The workspace numbers commands accept (SPEC §2.2: 1-9).
pub const WORKSPACES: std::ops::RangeInclusive<u32> = 1..=9;

fn workspace(command: &str, arg: Option<&str>) -> Result<u32, IpcError> {
    arg.and_then(|a| a.parse().ok())
        .filter(|n| WORKSPACES.contains(n))
        .ok_or_else(|| IpcError::BadArgs {
            command: command.into(),
            expected: "a workspace number 1-9",
        })
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
            takes(1, "a workspace number 1-9")?;
            Request::Workspace(workspace(command, arg)?)
        }
        "move-to" => {
            takes(1, "a workspace number 1-9")?;
            Request::MoveTo(workspace(command, arg)?)
        }
        "focus" => {
            takes(1, "left, down, up or right")?;
            Request::Focus(match arg {
                Some("left") => Direction::Left,
                Some("down") => Direction::Down,
                Some("up") => Direction::Up,
                Some("right") => Direction::Right,
                _ => return Err(one("left, down, up or right")),
            })
        }
        "float" => {
            takes(1, "toggle")?;
            match arg {
                Some("toggle") => Request::FloatToggle,
                _ => return Err(one("toggle")),
            }
        }
        "scratchpad" => {
            takes(1, "toggle or send")?;
            match arg {
                Some("toggle") => Request::ScratchpadToggle,
                Some("send") => Request::ScratchpadSend,
                _ => return Err(one("toggle or send")),
            }
        }
        "layout" => {
            takes(1, "dwindle, master, floating or tiling")?;
            Request::Layout(match arg {
                Some("dwindle") => LayoutName::Dwindle,
                Some("master") => LayoutName::Master,
                Some("floating") => LayoutName::Floating,
                Some("tiling") => LayoutName::Tiling,
                _ => return Err(one("dwindle, master, floating or tiling")),
            })
        }
        "query" => {
            takes(1, "state, windows or monitors")?;
            Request::Query(match arg {
                Some("state") => QueryKind::State,
                Some("windows") => QueryKind::Windows,
                Some("monitors") => QueryKind::Monitors,
                _ => return Err(one("state, windows or monitors")),
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
    //! ends, missing, extra and wrong arguments, an unknown and an empty command; the wire
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
        assert_eq!(p("launcher"), Ok(Request::Launcher));
        assert_eq!(p("overview"), Ok(Request::Overview));
        assert_eq!(p("saver"), Ok(Request::Saver));
        assert_eq!(p("apps"), Ok(Request::AppGrid));
        assert_eq!(p("profile Kodlama gece"), Ok(Request::Profile("Kodlama gece".into())));
        assert!(matches!(p("profile"), Err(IpcError::BadArgs { .. })));
        assert_eq!(p("restore"), Ok(Request::Restore));
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
            "float on",
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
        assert_eq!(e.to_string(), "`workspace` expects a workspace number 1-9");
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
