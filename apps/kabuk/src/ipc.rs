//! The command pipe's server (ARCHITECTURE §12): `kabuk msg …` and other programs drive
//! the running shell. It runs on its own thread and only hands work to the window
//! manager's queue and the bars' control window, so a client never waits on either.

use kabuk_core::instance::Instance;
use kabuk_core::ipc::{self, PanelName, QueryKind, Request, Response};
use kabuk_layout::Kind;
use kabuk_win::msgloop::{self, Msg};
use kabuk_win::pipe::{PipeServer, Reply};
use kabuk_win::window::Hwnd;
use serde_json::{json, Value};

use crate::bar::manager::{
    MSG_FOCUS_BAR, MSG_OPEN_APP_GRID, MSG_OPEN_CALENDAR_PANEL, MSG_OPEN_KEYS_CARD, MSG_OPEN_LAUNCHER,
    MSG_OPEN_NETWORK_PANEL, MSG_OPEN_OVERVIEW, MSG_OPEN_QUICK_PANEL, MSG_OPEN_VOLUME_PANEL, MSG_RELOAD, MSG_SET_QUIET,
    MSG_STARTED_AGAIN, MSG_START_SAVER,
};
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
    /// The tray icons the bars hold (`query tray`).
    pub tray: Option<std::sync::Arc<std::sync::Mutex<Vec<kabuk_core::tray::Icon>>>>,
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
    post_with(to, id, 0)
}

fn post_with(to: Hwnd, id: u32, wparam: usize) -> bool {
    msgloop::post_message(to, Msg { id, wparam, lparam: 0 }).is_ok()
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
    let to_bar_with = |id: u32, wparam: usize| match t.bar {
        Some(b) if post_with(b, id, wparam) => Response::done(),
        _ => Response::failed(error::NO_BAR),
    };
    let to_bar = |id: u32| to_bar_with(id, 0);
    let r = match request {
        Request::Layout(name) => to_wm(Command::SetLayout(name)),
        Request::ShowDesktop => to_wm(Command::ToggleDesktop),
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
        Request::AppGrid => to_bar(MSG_OPEN_APP_GRID),
        Request::KeysCard => to_bar(MSG_OPEN_KEYS_CARD),
        Request::FocusBar => to_bar(MSG_FOCUS_BAR),
        Request::Panel(panel) => to_bar(match panel {
            PanelName::Quick => MSG_OPEN_QUICK_PANEL,
            PanelName::Network => MSG_OPEN_NETWORK_PANEL,
            PanelName::Calendar => MSG_OPEN_CALENDAR_PANEL,
            PanelName::Volume => MSG_OPEN_VOLUME_PANEL,
        }),
        Request::Profile(name) => to_wm(Command::OpenProfile(name)),
        Request::Quiet(on) => to_bar_with(MSG_SET_QUIET, crate::bar::manager::quiet_wparam(on)),
        Request::StartedAgain => to_bar(MSG_STARTED_AGAIN),
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
        // A shortcut's action: the window manager does it as it does for the shortcut.
        shortcut => match shortcut.action() {
            Some(a) => to_wm(Command::Action(a)),
            None => Response::failed(error::BAD_REQUEST),
        },
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
        QueryKind::Tray => {
            let icons = t
                .tray
                .as_ref()
                .and_then(|l| l.lock().ok().map(|l| l.clone()))
                .unwrap_or_default();
            Value::Array(
                icons
                    .iter()
                    .map(|i| {
                        let owner = Hwnd(i.hwnd);
                        let path = owner
                            .pid()
                            .and_then(|pid| kabuk_win::process::Process::query(pid).ok())
                            .and_then(|p| p.image_path());
                        let guid = match i.key {
                            kabuk_core::tray::IconKey::Guid(g) => {
                                Some(g.iter().map(|b| format!("{b:02x}")).collect::<String>())
                            }
                            kabuk_core::tray::IconKey::Window { .. } => None,
                        };
                        json!({
                            "tip": i.tip,
                            "hwnd": i.hwnd,
                            "uid": i.uid,
                            "guid": guid,
                            "version": i.version,
                            "shown": i.drawn(),
                            "pid": owner.pid(),
                            "exe": path.as_deref().and_then(|p| p.rsplit(['\\', '/']).next()),
                            "window_class": owner.class_name(),
                        })
                    })
                    .collect(),
            )
        }
        QueryKind::Windows => Value::Array(
            view.windows
                .iter()
                .map(|w| {
                    let h = Hwnd(w.hwnd);
                    let path = h
                        .pid()
                        .and_then(|pid| kabuk_win::process::Process::query(pid).ok())
                        .and_then(|p| p.image_path());
                    json!({
                        "hwnd": w.hwnd,
                        "title": h.title(),
                        "class": h.class_name(),
                        "pid": h.pid(),
                        "exe": path.as_deref().and_then(|p| p.rsplit(['\\', '/']).next()),
                        // What opens the program again: a packaged app by its id, as its
                        // file under WindowsApps cannot be started (layout profiles, D65).
                        "open": open_target(path.as_deref(), h),
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

/// What opens a window's program again: its file, or for a packaged app (its file under
/// `WindowsApps`) `shell:AppsFolder\<its app id>`.
fn open_target(path: Option<&str>, h: Hwnd) -> Option<String> {
    let path = path?;
    if path.to_ascii_lowercase().contains("\\windowsapps\\") {
        return kabuk_win::shellitems::window_app_id(h).map(|id| format!("shell:AppsFolder\\{id}"));
    }
    Some(path.to_string())
}
