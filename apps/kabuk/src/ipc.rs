//! The command pipe's server (ARCHITECTURE §12): `kabuk msg …` and other programs drive
//! the running shell. It runs on its own thread and only hands work to the window
//! manager's queue and the bars' control window, so a client never waits on either.

use kabuk_core::instance::Instance;
use kabuk_core::ipc::{self, QueryKind, Request, Response};
use kabuk_core::wm::Action;
use kabuk_layout::Kind;
use kabuk_win::msgloop::{self, Msg};
use kabuk_win::pipe::{PipeServer, Reply};
use kabuk_win::window::Hwnd;
use serde_json::{json, Value};

use crate::bar::manager::{MSG_OPEN_LAUNCHER, MSG_OPEN_OVERVIEW, MSG_RELOAD, MSG_START_SAVER};
use crate::wm::{self, Command, SharedState, WmLink};

/// Error keys the server answers with; the client shows their translation.
pub mod error {
    pub const NO_WM: &str = "ipc-error-no-wm";
    pub const NO_BAR: &str = "ipc-error-no-bar";
    pub const BAD_REQUEST: &str = "ipc-error-bad-request";
}

/// What the commands reach.
pub struct Targets {
    pub wm: Option<WmLink>,
    /// The bars' control window.
    pub bar: Option<Hwnd>,
    pub state: SharedState,
    pub instance: Instance,
}

/// Starts serving this instance's pipe.
pub fn start(targets: Targets) -> std::io::Result<PipeServer> {
    let name = ipc::pipe_name(&kabuk_win::pipe::user_sid()?, targets.instance.id());
    let server = PipeServer::start(&name, move |line| handle(line, &targets))?;
    tracing::info!("command pipe open");
    Ok(server)
}

fn post(to: Hwnd, id: u32) -> bool {
    msgloop::post_message(
        to,
        Msg {
            id,
            wparam: 0,
            lparam: 0,
        },
    )
    .is_ok()
}

fn done(r: Response) -> Reply {
    Reply {
        line: r.line(),
        after: None,
    }
}

fn handle(line: &str, t: &Targets) -> Reply {
    let request = ipc::read_request(line).and_then(|words| ipc::parse(&words));
    let request = match request {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "command not understood");
            return done(Response::failed(error::BAD_REQUEST));
        }
    };
    tracing::info!(command = ?request, "command received");
    let to_wm = |c: Command| match &t.wm {
        Some(link) => {
            link.send(c);
            Response::done()
        }
        None => Response::failed(error::NO_WM),
    };
    let to_bar = |id: u32| match t.bar {
        Some(b) if post(b, id) => Response::done(),
        _ => Response::failed(error::NO_BAR),
    };
    let r = match request {
        Request::Workspace(n) => to_wm(Command::Action(Action::Workspace(n))),
        Request::MoveTo(n) => to_wm(Command::Action(Action::MoveToWorkspace(n))),
        Request::Focus(d) => to_wm(Command::Action(Action::Focus(d))),
        Request::FloatToggle => to_wm(Command::Action(Action::ToggleFloat)),
        Request::ScratchpadToggle => to_wm(Command::Action(Action::ToggleScratchpad)),
        Request::ScratchpadSend => to_wm(Command::Action(Action::ToScratchpad)),
        Request::Layout(name) => to_wm(Command::SetLayout(name)),
        Request::Reload => {
            if let Some(link) = &t.wm {
                link.send(Command::ConfigChanged);
            }
            if let Some(b) = t.bar {
                post(b, MSG_RELOAD);
            }
            Response::done()
        }
        Request::Launcher => to_bar(MSG_OPEN_LAUNCHER),
        Request::Overview => to_bar(MSG_OPEN_OVERVIEW),
        Request::Saver => to_bar(MSG_START_SAVER),
        Request::Query(kind) => Response::with(query(kind, t)),
        Request::Restore => {
            let event = t.instance.emergency_event();
            return Reply {
                line: Response::done().line(),
                // After the answer: the stop ends this process.
                after: Some(Box::new(move || {
                    match kabuk_win::sync::NamedEvent::create_or_open(&event).and_then(|e| e.set()) {
                        Ok(()) => tracing::warn!("emergency stop requested over the command pipe"),
                        Err(e) => tracing::error!(error = %e, "could not signal the emergency stop"),
                    }
                })),
            };
        }
    };
    done(r)
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Dwindle => "dwindle",
        Kind::Master => "master",
    }
}

fn query(kind: QueryKind, t: &Targets) -> Value {
    let view = t.wm.as_ref().map(WmLink::view).unwrap_or_default();
    match kind {
        QueryKind::State => {
            let hidden = wm::lock(&t.state).state().hidden_windows.len();
            json!({
                "pid": std::process::id(),
                "instance": t.instance.id(),
                "window_manager": t.wm.is_some(),
                "bar": t.bar.is_some(),
                "focused": view.focused,
                "hidden_windows": hidden,
                "monitors": view.monitors.iter().map(|m| {
                    let active = m.workspaces.iter().find(|w| w.active);
                    json!({
                        "device": m.device,
                        "number": m.number,
                        "workspace": active.map(|w| w.number),
                        "layout": kind_name(m.layout),
                        "floating": active.is_some_and(|w| w.floating),
                        "game_mode": m.game,
                    })
                }).collect::<Vec<_>>(),
            })
        }
        QueryKind::Windows => Value::Array(
            view.windows
                .iter()
                .map(|w| {
                    let h = Hwnd(w.hwnd);
                    json!({
                        "hwnd": w.hwnd,
                        "title": h.title(),
                        "class": h.class_name(),
                        "pid": h.pid(),
                        "device": w.device,
                        "monitor": view.monitors.iter().find(|m| m.device == w.device).map(|m| m.number),
                        "workspace": w.workspace,
                        "floating": w.floating,
                        "minimized": w.minimized,
                        "focused": view.focused == Some(w.hwnd),
                    })
                })
                .collect(),
        ),
        QueryKind::Monitors => Value::Array(
            kabuk_win::shell::monitors()
                .into_iter()
                .map(|m| {
                    let active = view
                        .workspaces(&m.device)
                        .and_then(|ws| ws.iter().find(|w| w.active))
                        .map(|w| w.number);
                    json!({
                        "device": m.device,
                        "number": m.number,
                        "primary": m.primary,
                        "dpi": m.dpi,
                        "rect": m.rect,
                        "work_area": m.work_area,
                        "workspace": active,
                    })
                })
                .collect(),
        ),
    }
}
