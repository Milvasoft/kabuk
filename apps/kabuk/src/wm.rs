//! The window manager thread (ARCHITECTURE §4 and §8).
//!
//! It owns the workspace model ([`kabuk_layout::Desk`]), learns about windows from
//! out-of-context WinEvents, and applies the model's effects to real windows without ever
//! waiting on them. Every window is written to the state file before it is hidden
//! (CONTRIBUTING.md rule 3), and so is every window whose border colour it changes.
//!
//! Other threads talk to it through [`WmHandle`]: commands go into a queue and a posted
//! message wakes the thread; the bar reads the workspace summary from [`WmView`] and is
//! told when it changes.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kabuk_core::config::{self, Config, LiveConfig, Reload};
use kabuk_core::grab::Order;
use kabuk_core::rules;
use kabuk_core::state::{self, HiddenWindow, StateFile};
use kabuk_core::wm::{self as policy, Action, Direction, Verdict};
use kabuk_layout::{
    Desk, Effect, Gaps, Kind, MonitorInfo, NewWindow, Rect, Settings, Toward, WorkspaceInfo, MASTER_RATIO,
};
use kabuk_win::msgloop::{self, msg, Msg, Window, MSG_HOTKEY};
use kabuk_win::window::Hwnd;
use kabuk_win::wmevents::{WindowEvent, WindowEvents};

use crate::grab::{InputThread, SharedSnapshot, Snapshot};

/// The state file, shared by the bar (work areas) and the window manager (windows).
pub type SharedState = Arc<Mutex<StateFile>>;

/// Locks the state file; a panic elsewhere must not stop the safety net.
pub fn lock(state: &SharedState) -> MutexGuard<'_, StateFile> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Class of the window manager's hidden window; its title is the instance id, so tests
/// can find it.
pub const WM_CLASS: &str = kabuk_core::hang::WM_CLASS;

/// Commands queued for the window manager wake it with this message.
const MSG_COMMAND: u32 = msg::APP + 10;
/// Ends the window manager's message loop.
const MSG_STOP: u32 = msg::APP + 11;
/// Integration tests trigger shortcut actions with this message: `wparam` is a
/// [`test_action`] code and `lparam` its argument. Handled before `SendMessage` returns.
pub const MSG_TEST_ACTION: u32 = msg::APP + 12;

/// Codes of [`MSG_TEST_ACTION`].
pub mod test_action {
    pub const WORKSPACE: usize = 1;
    pub const MOVE_TO_WORKSPACE: usize = 2;
    /// `lparam`: 0 left, 1 down, 2 up, 3 right.
    pub const FOCUS: usize = 3;
    pub const MOVE: usize = 4;
    pub const TOGGLE_FLOAT: usize = 5;
    pub const CLOSE: usize = 6;
    pub const CYCLE_LAYOUT: usize = 7;
    /// Tiling <-> floating for the focused monitor's workspace (the bar's layout indicator).
    pub const TOGGLE_FLOATING_LAYOUT: usize = 9;
    /// Treat window `lparam` as focused (tests cannot always take the foreground).
    pub const FOCUSED: usize = 8;
    /// What the mouse hook sends for an Alt+drag of window `lparam` by (120, 80): its
    /// held places, then the drop.
    pub const GRAB: usize = 10;
    /// What the mouse hook sends for a double click on window `lparam`'s title.
    pub const RETILE: usize = 11;
    pub const TOGGLE_SCRATCHPAD: usize = 12;
    pub const TO_SCRATCHPAD: usize = 13;
    pub const TOGGLE_GROUP: usize = 14;
    pub const NEXT_TAB: usize = 15;
    /// Drops the terminal down, or hides it (DECISIONS D62).
    pub const DROPDOWN: usize = 16;
    /// Fail inside the window manager, to test the hand-over to the watchdog.
    pub const PANIC: usize = 99;
}

mod timer {
    /// Reads back where placed windows ended up.
    pub const CHECK: usize = 1;
    /// Re-reads the monitors after a display or work-area change.
    pub const MONITORS: usize = 2;
    /// Reads `kabuk.toml` once a burst of change notifications is over.
    pub const CONFIG: usize = 3;
    /// Minimises or restores a window once its genie is on screen.
    pub const GENIE: usize = 4;
}

/// How long a genie Kabuk starts itself (a click on the bar) is on screen before the
/// window minimises or comes back, so the window never shows without it: the overlay's
/// set-up and a frame. The overlay is above every ordinary window, so a window coming
/// back stays under its genie, and has painted itself by the time the genie is large.
const GENIE_LEAD: Duration = Duration::from_millis(24);
/// How long a window being minimised from the bar may wait for its genie's picture (the
/// GPU draws the genie smooth from it; taken after the window is gone, it cannot be),
/// and how often it is looked for meanwhile.
const GENIE_PICTURE_WAIT: Duration = Duration::from_millis(120);
const GENIE_PICTURE_STEP: Duration = Duration::from_millis(4);
/// How long after Kabuk minimised or restored a window its event still belongs to the
/// genie Kabuk started.
const GENIE_OWN: Duration = Duration::from_secs(1);

/// [`test_action::GRAB`]: how far and in how many held places.
const GRAB_TEST_BY: (i32, i32) = (120, 80);
const GRAB_TEST_STEPS: i32 = 4;

/// How long after placing windows their size is read back (they move asynchronously).
const CHECK_DELAY: Duration = Duration::from_millis(250);
/// Monitor and work-area changes come in bursts (each bar's reservation is one).
const MONITOR_DEBOUNCE: Duration = Duration::from_millis(150);
/// A window that ended up more than this many pixels larger than asked, or away from
/// where it was put, did not accept its place.
const REFUSE_TOLERANCE: i32 = 2;
/// How many times a window is placed before refusing counts.
const PLACE_ATTEMPTS: u8 = 2;
/// `WM_SETTINGCHANGE` for a work-area change (`SPI_SETWORKAREA`).
const SPI_SETWORKAREA: usize = 0x2F;
/// `WM_POWERBROADCAST` after waking from sleep (`PBT_APMRESUMEAUTOMATIC`).
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
/// The gap when neither `windows.gap` nor the look gives one (Ada's `--gap`).
const FALLBACK_GAP: i32 = 14;

/// The workspace summary of every monitor, for the bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WmView {
    pub monitors: Vec<MonitorView>,
    /// Every managed window (minimised ones too), for the launcher.
    pub windows: Vec<WindowView>,
    /// The focused managed window.
    pub focused: Option<u64>,
    /// The latest notices for the user, numbered in order (the bar shows the ones it has
    /// not seen yet; SPEC §2.6).
    pub notices: Vec<(u64, crate::notify::Notice)>,
    /// The strips of the tabbed groups on screen (D34).
    pub tab_strips: Vec<StripView>,
}

/// A tabbed group's strip: where it goes (physical pixels) and its tabs in order: the
/// window, its title and whether it is the shown one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripView {
    pub group: u64,
    pub rect: Rect,
    pub tabs: Vec<(u64, String, bool)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowView {
    pub hwnd: u64,
    /// The monitor's device, as in [`MonitorView::device`].
    pub device: String,
    /// 1-based.
    pub workspace: u32,
    pub floating: bool,
    pub minimized: bool,
    /// Where it is, or goes when its workspace comes back (physical pixels).
    pub rect: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorView {
    /// `\\.\DISPLAYn`, as the bar knows its monitor.
    pub device: String,
    /// Windows' display number ("Monitor n").
    pub number: u32,
    /// The tiling layout of the active workspace.
    pub layout: Kind,
    pub workspaces: Vec<WorkspaceInfo>,
    /// A full-screen program runs here and tiling is paused (SPEC §2.4).
    pub game: bool,
}

impl WmView {
    pub fn workspaces(&self, device: &str) -> Option<&[WorkspaceInfo]> {
        self.monitors
            .iter()
            .find(|m| m.device == device)
            .map(|m| m.workspaces.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A click on the bar's workspace indicator.
    SwitchOn {
        device: String,
        number: u32,
    },
    Action(Action),
    /// The launcher chose an open window: show its workspace and focus it.
    Reveal(u64),
    /// A click on a tab of a tabbed group's strip: show that member.
    SelectTab(u64),
    /// `kabuk msg layout <name>` on the focused monitor.
    SetLayout(kabuk_core::ipc::LayoutName),
    /// A click on the bar's layout indicator: the monitor's active workspace switches
    /// between tiling and floating.
    ToggleFloatingLayout {
        device: String,
    },
    /// The colour of the active window's frame (the look's accent), or none.
    SetAccent(Option<(u8, u8, u8)>),
    /// The look's frame for other programs' windows (DECISIONS D30).
    SetFrame(kabuk_win::window::FrameStyle),
    /// The monitors a full-screen program covers (SPEC §2.4); empty when none does.
    GameMode(Vec<String>),
    /// `kabuk.toml` changed on disk.
    ConfigChanged,
    /// `--debug-hang-after-ms`: the thread stops answering for good, as a hung shell.
    #[cfg(feature = "integration")]
    DebugHang,
    /// From the mouse hook: a window held with Alt, let go, or double-clicked.
    Grab(Order),
    /// A click on the bar minimises the app's front window.
    Minimize(u64),
    /// `kabuk msg profile <name>`, the launcher or the settings: a layout profile opens
    /// (DECISIONS D65).
    OpenProfile(String),
    /// Where each window's app icon is on the bar (screen pixels), for the genie.
    AppIcons(Vec<(u64, state::Rect)>),
    /// The mouse is on an app's icon: a genie may come soon (its renderer is made ahead).
    WarmGenie,
}

pub struct Options {
    pub config: Config,
    pub config_file: PathBuf,
    pub state: SharedState,
    /// Test mode: manage only windows of this class, and take actions from
    /// [`MSG_TEST_ACTION`] instead of global shortcuts.
    pub manage_class: Option<String>,
    /// The instance id, the hidden window's title.
    pub instance: String,
    /// Register the configured shortcuts (always outside test mode).
    pub shortcuts: bool,
}

/// Tells another thread's window that the workspace summary changed.
type Listener = Arc<Mutex<Option<(Hwnd, u32)>>>;

/// Runs the window manager on its own thread; dropping it stops the thread.
pub struct WmHandle {
    /// The mouse hook; stopped before the window manager.
    input: Option<InputThread>,
    snapshot: SharedSnapshot,
    thread: Option<std::thread::JoinHandle<()>>,
    window: Hwnd,
    queue: Arc<Mutex<VecDeque<Command>>>,
    view: Arc<Mutex<WmView>>,
    listener: Listener,
}

/// What the bar keeps to talk to the window manager.
#[derive(Clone)]
pub struct WmLink {
    window: Hwnd,
    queue: Arc<Mutex<VecDeque<Command>>>,
    view: Arc<Mutex<WmView>>,
    listener: Listener,
}

impl WmLink {
    pub fn send(&self, c: Command) {
        send(&self.queue, self.window, c);
    }

    pub fn view(&self) -> WmView {
        self.view.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// `window` gets message `id` whenever the workspace summary changes.
    pub fn listen(&self, window: Hwnd, id: u32) {
        if let Ok(mut l) = self.listener.lock() {
            *l = Some((window, id));
        }
    }
}

fn send(queue: &Mutex<VecDeque<Command>>, window: Hwnd, c: Command) {
    if let Ok(mut q) = queue.lock() {
        // A held window only needs its latest place.
        if let Command::Grab(Order::Place { window: w, .. }) = c {
            q.retain(|old| !matches!(old, Command::Grab(Order::Place { window, .. }) if *window == w));
        }
        q.push_back(c);
    }
    let _ = msgloop::post_message(
        window,
        Msg {
            id: MSG_COMMAND,
            wparam: 0,
            lparam: 0,
        },
    );
}

impl WmHandle {
    /// Where the mouse hook tells of a hot corner reached: the bar's control window.
    pub fn set_bar_window(&self, bar: Option<Hwnd>) {
        if let Ok(mut s) = self.snapshot.lock() {
            s.bar = bar;
        }
    }

    /// Starts the thread and waits until it manages the existing windows.
    pub fn start(options: Options) -> std::io::Result<WmHandle> {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let view = Arc::new(Mutex::new(WmView::default()));
        let listener: Listener = Arc::new(Mutex::new(None));
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(Snapshot::default()));
        if let Ok(mut s) = snapshot.lock() {
            s.set_config(&options.config);
            s.monitors = kabuk_win::shell::monitors().iter().map(|m| m.rect).collect();
        }
        let (ready_tx, ready_rx) = mpsc::channel();
        let shared = (queue.clone(), view.clone(), listener.clone(), snapshot.clone());
        let thread = std::thread::Builder::new().name("wm".into()).spawn(move || {
            let (queue, view, listener, snapshot) = shared;
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(options, queue, view, listener, snapshot, ready_tx);
            }));
            if ran.is_err() {
                // Windows may be hidden on other workspaces; the watchdog shows them and
                // restarts the shell.
                tracing::error!("window manager failed; ending the shell so the watchdog restores the desktop");
                std::process::exit(kabuk_core::instance::exit::INTERNAL_FAILURE);
            }
        })?;
        match ready_rx.recv() {
            Ok(Ok(window)) => {
                let orders = queue.clone();
                // Without the hook only Alt+drag and double clicks are missing.
                let input = InputThread::start(snapshot.clone(), move |o| send(&orders, window, Command::Grab(o)))
                    .map_err(|e| tracing::error!(error = %e, "mouse hook unavailable"))
                    .ok();
                Ok(WmHandle {
                    input,
                    snapshot,
                    thread: Some(thread),
                    window,
                    queue,
                    view,
                    listener,
                })
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(std::io::Error::other("window manager thread ended during start"))
            }
        }
    }

    pub fn link(&self) -> WmLink {
        WmLink {
            window: self.window,
            queue: self.queue.clone(),
            view: self.view.clone(),
            listener: self.listener.clone(),
        }
    }

    pub fn send(&self, c: Command) {
        send(&self.queue, self.window, c);
    }
}

impl Drop for WmHandle {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = msgloop::post_message(
            self.window,
            Msg {
                id: MSG_STOP,
                wparam: 0,
                lparam: 0,
            },
        );
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------- the thread

struct Wm {
    desk: Desk,
    config: LiveConfig,
    /// `[rules]`, ready to match new windows.
    rules: rules::Rules,
    /// Windows whose rules keep Kabuk from grabbing them with Alt (DECISIONS D23).
    no_grab: std::collections::HashSet<u64>,
    config_file: PathBuf,
    state: SharedState,
    manage_class: Option<String>,
    shortcuts: bool,
    window: Option<Window>,
    /// Registered shortcuts: hotkey id, registration and action.
    hotkeys: Vec<(i32, kabuk_win::keyhook::Bound, Action)>,
    /// Placements to read back: window, the frame it was given and how many times it was
    /// asked already.
    checks: Vec<(u64, Rect, u8)>,
    accent: Option<(u8, u8, u8)>,
    /// The window wearing the accent frame.
    bordered: Option<u64>,
    /// The look's frame, and the windows wearing it (DECISIONS D30).
    frame: Option<kabuk_win::window::FrameStyle>,
    styled: std::collections::HashSet<u64>,
    /// Monitors where tiling is paused for a full-screen program (SPEC §2.4).
    game: std::collections::HashSet<String>,
    /// Plays window moves over the work area (DECISIONS D35).
    mover: Option<kabuk_win::window_motion::Mover>,
    /// The windows whose own transitions are off for the genie, where their icons are,
    /// and the genie the next moves play with.
    genie: std::collections::HashSet<u64>,
    icons: std::collections::HashMap<u64, state::Rect>,
    pending_genie: Option<kabuk_win::window_motion::Genie>,
    /// A genie Kabuk started before minimising or restoring the window itself, and when;
    /// the event that follows goes on with it instead of starting another.
    own_genie: Option<(kabuk_win::window_motion::Genie, std::time::Instant)>,
    /// The latest notices and the number of the last one.
    notices: VecDeque<(u64, crate::notify::Notice)>,
    notice_seq: u64,
    queue: Arc<Mutex<VecDeque<Command>>>,
    view: Arc<Mutex<WmView>>,
    listener: Listener,
    /// What the mouse hook knows about the managed windows.
    snapshot: SharedSnapshot,
    /// The window held with Alt, with its outer rectangle and frame from before it moved.
    held: Option<(u64, state::Rect, state::Rect)>,
    /// A maximised window a move restores: placed though Windows still has it maximised.
    unmaximizing: Option<u64>,
    /// The drop-down terminal's window (DECISIONS D62), kept out of the layout, and when
    /// its program was started while its window has not come yet.
    dropdown: Option<u64>,
    dropdown_wait: Option<Instant>,
    /// Programs a layout profile started, waiting for their windows: the program, where
    /// its window goes, and since when (D65).
    profile_wait: Vec<(kabuk_core::config::ProfileWindow, Instant)>,
}

fn run(
    options: Options,
    queue: Arc<Mutex<VecDeque<Command>>>,
    view: Arc<Mutex<WmView>>,
    listener: Listener,
    snapshot: SharedSnapshot,
    ready: mpsc::Sender<std::io::Result<Hwnd>>,
) {
    let monitors = monitors();
    let accent = accent_of(&options.config);
    let wm = Rc::new(RefCell::new(Wm {
        desk: Desk::new(settings(&options.config), monitors),
        config: LiveConfig::new(options.config.clone()),
        rules: rules::Rules::new(&options.config.rules),
        no_grab: std::collections::HashSet::new(),
        config_file: options.config_file,
        state: options.state,
        manage_class: options.manage_class,
        shortcuts: options.shortcuts,
        window: None,
        hotkeys: Vec::new(),
        checks: Vec::new(),
        accent,
        bordered: None,
        frame: None,
        styled: std::collections::HashSet::new(),
        genie: std::collections::HashSet::new(),
        icons: std::collections::HashMap::new(),
        pending_genie: None,
        own_genie: None,
        notices: VecDeque::new(),
        notice_seq: 0,
        game: std::collections::HashSet::new(),
        mover: kabuk_win::window_motion::Mover::start(ease)
            .map_err(|e| tracing::warn!(error = %e, "window moves will not be animated"))
            .ok(),
        queue,
        view,
        listener,
        snapshot,
        held: None,
        unmaximizing: None,
        dropdown: None,
        dropdown_wait: None,
        profile_wait: Vec::new(),
    }));
    let weak = Rc::downgrade(&wm);
    let own = Rc::new(std::cell::Cell::new(None));
    let window = match Window::shell_surface(
        WM_CLASS,
        &options.instance,
        msgloop::PxRect::default(),
        proc(weak.clone(), own.clone()),
    ) {
        Ok(w) => w,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let hwnd = window.hwnd();
    own.set(Some(hwnd));
    let events = {
        let weak = weak.clone();
        WindowEvents::start(move |e| {
            if let Some(rc) = weak.upgrade() {
                if let Ok(mut wm) = rc.try_borrow_mut() {
                    wm.on_event(e);
                }
            }
        })
    };
    let events = match events {
        Ok(e) => e,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    {
        let mut w = wm.borrow_mut();
        w.window = Some(window);
        if w.shortcuts {
            w.bind_shortcuts();
        }
    }
    // Ready before taking over the open windows, so the starting thread never waits on it.
    let _ = ready.send(Ok(hwnd));
    wm.borrow_mut().manage_existing();
    tracing::info!("window manager running");
    let _ = msgloop::run_until(&[], None);
    drop(events);
    wm.borrow_mut().stop();
    tracing::info!("window manager stopped");
}

/// The window manager's window procedure. `own` is its window, once made.
fn proc(weak: Weak<RefCell<Wm>>, own: Rc<std::cell::Cell<Option<Hwnd>>>) -> impl Fn(Msg) -> Option<isize> + 'static {
    move |m| {
        if m.id == MSG_STOP {
            msgloop::post_quit(0);
            return Some(0);
        }
        let rc = weak.upgrade()?;
        let Ok(mut wm) = rc.try_borrow_mut() else {
            // Sent while the window manager is in the middle of its work (a call of its
            // own handed it on): an action is done right after instead of being lost
            // (found by a test: a focus change dropped the next two).
            if m.id == MSG_TEST_ACTION {
                if let Some(h) = own.get() {
                    let _ = msgloop::post_message(h, m);
                    return Some(0);
                }
            }
            return None;
        };
        wm.on_message(m)
    }
}

/// The monitors as the model sees them: work areas in physical pixels.
fn monitors() -> Vec<MonitorInfo> {
    kabuk_win::shell::monitors()
        .into_iter()
        .map(|m| MonitorInfo {
            key: m.device,
            area: rect_of(m.work_area),
            primary: m.primary,
            dpi: m.dpi,
        })
        .collect()
}

fn rect_of(r: state::Rect) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

/// The easing of window moves: the same cubic ease-out as Kabuk's own surfaces.
fn ease(u: f32) -> f32 {
    kabuk_ui::motion::ease_out(0.0, 1.0, 1.0).at(u)
}

/// `frame` grown by the invisible borders `outer` has around `visible`.
fn with_borders(frame: state::Rect, outer: state::Rect, visible: state::Rect) -> state::Rect {
    state::Rect {
        left: frame.left - (visible.left - outer.left),
        top: frame.top - (visible.top - outer.top),
        right: frame.right + (outer.right - visible.right),
        bottom: frame.bottom + (outer.bottom - visible.bottom),
    }
}

/// Where the genie goes without an icon: a small square in the middle of the monitor's
/// edge outside its work area (where the bar is), else of its bottom edge.
fn bar_edge_spot(monitor: state::Rect, work: state::Rect) -> state::Rect {
    /// The square's side as a share of the monitor's height.
    const SHARE: i32 = 24;
    let side = ((monitor.bottom - monitor.top) / SHARE).max(1);
    let cx = (monitor.left + monitor.right) / 2;
    let (top, bottom) = if work.top > monitor.top {
        (monitor.top, monitor.top + side)
    } else {
        (monitor.bottom - side, monitor.bottom)
    };
    state::Rect {
        left: cx - side / 2,
        top,
        right: cx + side / 2,
        bottom,
    }
}

fn state_rect(r: Rect) -> state::Rect {
    state::Rect {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

/// The layout settings from `[windows]` and `[rules]`; the gap falls back to the look's.
fn settings(cfg: &Config) -> Settings {
    let look_gap = kabuk_theme::theme(cfg.appearance.look.code())
        .and_then(|t| t.tokens(kabuk_theme::Mode::Dark).px("gap"))
        .map_or(FALLBACK_GAP, |g| g.round() as i32);
    let gap = cfg
        .windows
        .gap
        .map_or(look_gap, |g| i32::try_from(g).unwrap_or(look_gap));
    Settings {
        workspaces: cfg.windows.workspaces,
        gaps: Gaps { inner: gap, outer: gap },
        direction: match cfg.windows.new_window {
            config::NewWindow::Below => kabuk_layout::Direction::Below,
            config::NewWindow::Right | config::NewWindow::Floating => kabuk_layout::Direction::Right,
        },
        kind: match cfg.windows.tiling_layout {
            config::TilingLayout::Dwindle => Kind::Dwindle,
            config::TilingLayout::Master => Kind::Master,
        },
        floating_layout: cfg.windows.default_layout == config::WorkspaceLayout::Floating
            || cfg.windows.new_window == config::NewWindow::Floating,
        float_unfit: cfg.rules.float_unfit,
        drag_to_float: cfg.windows.drag_to_float,
        master_ratio: MASTER_RATIO,
        tab_strip: crate::tabs::style::TABS.height.round() as i32,
    }
}

/// The accent frame colour before the bar reports the resolved one (dark mode).
fn accent_of(cfg: &Config) -> Option<(u8, u8, u8)> {
    if !cfg.windows.active_border {
        return None;
    }
    let accent = (cfg.appearance.accent != config::THEME_ACCENT).then_some(cfg.appearance.accent.as_str());
    let r = kabuk_theme::resolve::resolve(cfg.appearance.look.code(), kabuk_theme::Mode::Dark, accent)?;
    r.tokens.color("accent-s").map(|c| (c.r, c.g, c.b))
}

/// Whether a window that was given `want` and shows `got` refused it: larger (a minimum
/// size) or elsewhere (it ignores moves). Smaller is fine: some windows snap their size
/// to a grid (terminal character cells).
fn refused(want: Rect, got: Rect) -> bool {
    let larger = got.w > want.w + REFUSE_TOLERANCE || got.h > want.h + REFUSE_TOLERANCE;
    let moved_away = (got.x - want.x).abs() > REFUSE_TOLERANCE || (got.y - want.y).abs() > REFUSE_TOLERANCE;
    larger || moved_away
}

/// `px` at 96 DPI on a monitor of `dpi`, rounded.
fn scale_px(px: i32, dpi: u32) -> i32 {
    ((f64::from(px) * f64::from(dpi)) / f64::from(kabuk_layout::desk::BASE_DPI)).round() as i32
}

fn contains_centre(monitor: state::Rect, r: Rect) -> bool {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    monitor.left <= cx && cx < monitor.right && monitor.top <= cy && cy < monitor.bottom
}

fn spot(p: config::Placement) -> kabuk_layout::Spot {
    match p {
        config::Placement::Left => kabuk_layout::Spot::Left,
        config::Placement::Right => kabuk_layout::Spot::Right,
        config::Placement::Top => kabuk_layout::Spot::Top,
        config::Placement::Bottom => kabuk_layout::Spot::Bottom,
        config::Placement::Center => kabuk_layout::Spot::Center,
    }
}

fn toward(d: Direction) -> Toward {
    match d {
        Direction::Left => Toward::Left,
        Direction::Down => Toward::Down,
        Direction::Up => Toward::Up,
        Direction::Right => Toward::Right,
    }
}

impl Wm {
    fn window(&self) -> Option<&Window> {
        self.window.as_ref()
    }

    /// The configured gap (96-DPI pixels), also the margin of placed floating windows.
    fn desk_gap(&self) -> i32 {
        settings(self.config.current()).gaps.inner
    }

    fn bind_shortcuts(&mut self) {
        self.hotkeys.clear();
        let Some(window) = self.window.as_ref() else { return };
        let cfg = self.config.current();
        for (i, (chord, action)) in policy::bindings(&cfg.shortcuts, cfg.windows.workspaces)
            .into_iter()
            .enumerate()
        {
            let id = i32::try_from(i).unwrap_or(i32::MAX);
            match kabuk_win::keyhook::bind(window, id, chord) {
                Ok(h) => self.hotkeys.push((id, h, action)),
                Err(e) => tracing::warn!(%chord, error = %e, "shortcut taken by another program"),
            }
        }
        tracing::info!(count = self.hotkeys.len(), "shortcuts registered");
    }

    /// Manages the windows that are already open, top of the z-order first.
    fn manage_existing(&mut self) {
        for hwnd in kabuk_win::window::top_level_windows() {
            self.consider(hwnd);
        }
        if let Some(fg) = kabuk_win::events::foreground() {
            self.desk.focused(fg.0);
            self.update_border();
        }
        self.publish();
    }

    fn stop(&mut self) {
        self.hotkeys.clear();
        if let Some(id) = self.bordered.take() {
            Hwnd(id).set_border_color(None);
            let result = lock(&self.state).remove_bordered(id);
            if let Err(e) = result {
                tracing::error!(error = %e, "border record not cleared");
            }
        }
    }

    fn on_message(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            MSG_COMMAND => {
                loop {
                    let next = self.queue.lock().ok().and_then(|mut q| q.pop_front());
                    let Some(c) = next else { break };
                    self.command(c);
                }
                Some(0)
            }
            MSG_HOTKEY => {
                let id = i32::try_from(m.wparam).ok();
                let action = self.hotkeys.iter().find(|(i, _, _)| Some(*i) == id).map(|(_, _, a)| *a);
                if let Some(a) = action {
                    self.act_on_front(a);
                }
                Some(0)
            }
            MSG_TEST_ACTION if self.manage_class.is_some() => {
                self.test_action(m.wparam, m.lparam);
                Some(0)
            }
            msg::TIMER => {
                match m.wparam {
                    timer::CHECK => {
                        self.kill(timer::CHECK);
                        self.check_placements();
                    }
                    timer::GENIE => {
                        self.kill(timer::GENIE);
                        self.genie_lead_over();
                    }
                    timer::CONFIG => {
                        self.kill(timer::CONFIG);
                        self.reload_config();
                    }
                    timer::MONITORS => {
                        self.kill(timer::MONITORS);
                        if let Ok(mut s) = self.snapshot.lock() {
                            s.monitors = kabuk_win::shell::monitors().iter().map(|m| m.rect).collect();
                        }
                        let effects = self.desk.set_monitors(monitors());
                        self.apply(effects);
                    }
                    _ => return None,
                }
                Some(0)
            }
            msg::DISPLAYCHANGE => {
                self.schedule(timer::MONITORS, MONITOR_DEBOUNCE);
                None
            }
            msg::SETTINGCHANGE if m.wparam == SPI_SETWORKAREA => {
                self.schedule(timer::MONITORS, MONITOR_DEBOUNCE);
                None
            }
            msg::POWERBROADCAST if m.wparam == PBT_APMRESUMEAUTOMATIC => {
                self.schedule(timer::MONITORS, MONITOR_DEBOUNCE);
                None
            }
            _ => None,
        }
    }

    fn schedule(&self, id: usize, after: Duration) {
        if let Some(w) = self.window() {
            w.set_timer(id, after);
        }
    }

    fn kill(&self, id: usize) {
        if let Some(w) = self.window() {
            w.kill_timer(id);
        }
    }

    fn command(&mut self, c: Command) {
        match c {
            Command::SwitchOn { device, number } => {
                let effects = self.desk.switch(&device, number);
                self.apply(effects);
            }
            Command::Action(a) => self.act_on_front(a),
            Command::ToggleFloatingLayout { device } => self.toggle_floating_layout(&device),
            Command::Reveal(hwnd) => self.reveal(hwnd),
            Command::SelectTab(hwnd) => {
                let effects = self.desk.select_tab(hwnd);
                self.apply(effects);
            }
            Command::SetLayout(name) => self.set_layout(name),
            Command::GameMode(devices) => self.set_game_mode(devices),
            Command::SetFrame(frame) => {
                if self.frame != Some(frame) {
                    self.frame = Some(frame);
                    self.restyle_all();
                }
            }
            Command::SetAccent(rgb) => {
                let rgb = if self.config.current().windows.active_border {
                    rgb
                } else {
                    None
                };
                if rgb != self.accent {
                    self.accent = rgb;
                    if let Some(id) = self.bordered {
                        Hwnd(id).set_border_color(self.accent);
                    }
                    self.update_border();
                }
            }
            // A save comes as several notifications (a temporary file, a rename); read
            // once they stop, as the bars do.
            Command::ConfigChanged => self.schedule(timer::CONFIG, crate::bar::manager::CONFIG_DEBOUNCE),
            #[cfg(feature = "integration")]
            Command::DebugHang => {
                tracing::warn!("simulated hang");
                loop {
                    std::thread::park();
                }
            }
            Command::Grab(order) => self.grab(order),
            Command::Minimize(hwnd) => self.minimize(hwnd),
            Command::OpenProfile(name) => self.open_profile(&name),
            Command::AppIcons(icons) => {
                tracing::debug!(icons = icons.len(), "app icons known");
                self.icons = icons.into_iter().collect();
            }
            Command::WarmGenie => {
                if self.genie_on() {
                    if let Some(m) = &self.mover {
                        m.warm();
                    }
                }
            }
        }
    }

    fn grab(&mut self, order: Order) {
        match order {
            Order::Place { window, rect } => {
                if !self.desk.contains(window) || self.desk.is_parked(window) {
                    return;
                }
                // Held by the user: a pending read-back would fight the move.
                self.checks.retain(|(w, _, _)| *w != window);
                let h = Hwnd(window);
                let borders = match self.held {
                    Some((w, outer, visible)) if w == window => Some((outer, visible)),
                    _ => h.outer_and_frame(),
                };
                if let Some((outer, visible)) = borders {
                    self.held = Some((window, outer, visible));
                    h.place_frame_like_async(rect, outer, visible);
                }
            }
            Order::Drop { window, rect } => {
                self.held = None;
                tracing::info!(hwnd = window, "window let go after a grab");
                let effects = self.desk.dropped(window, rect_of(rect));
                self.apply(effects);
            }
            Order::Retile { window } => {
                if self.desk.is_floating(window) {
                    tracing::info!(hwnd = window, "window tiled by a double click");
                    self.notice(crate::notify::Notice::Tiled);
                    let effects = self.desk.toggle_floating(window);
                    self.apply(effects);
                }
            }
            // Handled by the input thread itself.
            Order::MaskAlt => {}
        }
    }

    fn test_action(&mut self, code: usize, arg: isize) {
        let dir = match arg {
            0 => Direction::Left,
            1 => Direction::Down,
            2 => Direction::Up,
            _ => Direction::Right,
        };
        let n = u32::try_from(arg).unwrap_or(0);
        let action = match code {
            test_action::WORKSPACE => Action::Workspace(n),
            test_action::MOVE_TO_WORKSPACE => Action::MoveToWorkspace(n),
            test_action::FOCUS => Action::Focus(dir),
            test_action::MOVE => Action::Move(dir),
            test_action::TOGGLE_FLOAT => Action::ToggleFloat,
            test_action::CLOSE => Action::Close,
            test_action::CYCLE_LAYOUT => Action::CycleLayout,
            test_action::TOGGLE_SCRATCHPAD => Action::ToggleScratchpad,
            test_action::TO_SCRATCHPAD => Action::ToScratchpad,
            test_action::TOGGLE_GROUP => Action::ToggleGroup,
            test_action::NEXT_TAB => Action::NextTab,
            test_action::DROPDOWN => Action::Dropdown,
            test_action::FOCUSED => {
                self.desk.focused(u64::try_from(arg).unwrap_or(0));
                self.update_border();
                return;
            }
            test_action::TOGGLE_FLOATING_LAYOUT => {
                if let Some(m) = self.action_monitor() {
                    self.toggle_floating_layout(&m);
                }
                return;
            }
            test_action::GRAB => {
                let window = u64::try_from(arg).unwrap_or(0);
                let Some(from) = Hwnd(window).frame_rect() else { return };
                for step in 1..=GRAB_TEST_STEPS {
                    let rect = state::Rect {
                        left: from.left + GRAB_TEST_BY.0 * step / GRAB_TEST_STEPS,
                        top: from.top + GRAB_TEST_BY.1 * step / GRAB_TEST_STEPS,
                        right: from.right + GRAB_TEST_BY.0 * step / GRAB_TEST_STEPS,
                        bottom: from.bottom + GRAB_TEST_BY.1 * step / GRAB_TEST_STEPS,
                    };
                    self.grab(Order::Place { window, rect });
                    if step == GRAB_TEST_STEPS {
                        self.grab(Order::Drop { window, rect });
                    }
                }
                return;
            }
            test_action::RETILE => {
                self.grab(Order::Retile {
                    window: u64::try_from(arg).unwrap_or(0),
                });
                return;
            }
            test_action::PANIC => panic!("test-requested window manager failure"),
            _ => return,
        };
        self.act(action);
    }

    /// The user's action, for the window in front: Windows tells Kabuk which window is in
    /// front with events that can come before the window is back from the bar, so the
    /// focus Kabuk had could be another window (found by the user: Win+arrow did nothing,
    /// or moved the window before it).
    fn act_on_front(&mut self, a: Action) {
        if let Some(front) = kabuk_win::events::foreground() {
            if self.desk.contains(front.0) && !self.desk.is_parked(front.0) {
                self.desk.focused(front.0);
            }
        }
        self.act(a);
    }

    /// A shortcut action. Logs how long applying it took (SPEC §8: under 16 ms).
    fn act(&mut self, a: Action) {
        let started = Instant::now();
        let monitor = self.action_monitor();
        let focused = self.desk.focus();
        let effects = match a {
            Action::Workspace(n) => monitor.map(|m| self.desk.switch(&m, n)).unwrap_or_default(),
            Action::MoveToWorkspace(n) => focused.map(|f| self.desk.move_to(f, n)).unwrap_or_default(),
            Action::Focus(d) => self.desk.focus_toward(toward(d)).into_iter().collect(),
            Action::Move(d) => self.desk.swap_toward(toward(d)),
            Action::ToggleFloat => focused.map(|f| self.desk.toggle_floating(f)).unwrap_or_default(),
            Action::Close => {
                if let Some(f) = focused {
                    Hwnd(f).close_async();
                }
                Vec::new()
            }
            Action::CycleLayout => monitor.map(|m| self.desk.cycle_layout(&m)).unwrap_or_default(),
            Action::ToggleScratchpad => monitor.map(|m| self.desk.toggle_scratchpad(&m)).unwrap_or_default(),
            Action::ToScratchpad => focused.map(|f| self.desk.to_scratchpad(f)).unwrap_or_default(),
            Action::ToggleGroup => focused.map(|f| self.desk.toggle_group(f)).unwrap_or_default(),
            Action::NextTab => focused
                .and_then(|f| self.desk.next_tab(f, true))
                .map(|t| self.desk.select_tab(t))
                .unwrap_or_default(),
            Action::Dropdown => {
                self.toggle_dropdown();
                Vec::new()
            }
        };
        if let (Action::Move(_), Some(f)) = (a, focused) {
            self.unmaximize_moved(f, &effects);
        }
        self.apply(effects);
        self.unmaximizing = None;
        tracing::info!(action = ?a, apply_us = started.elapsed().as_micros() as u64, "action applied");
    }

    /// A maximised window covers its tile, so moving it showed nothing (found by the user:
    /// Win+Shift+arrow did nothing to it): when the move places it, Windows restores it
    /// first and it goes to its new place.
    fn unmaximize_moved(&mut self, id: u64, effects: &[Effect]) {
        let h = Hwnd(id);
        let placed = effects.iter().any(|e| matches!(e, Effect::Place(p, _) if *p == id));
        if placed && h.is_maximized() && h.restore_async() {
            tracing::info!(hwnd = id, "maximised window restored to be moved");
            self.unmaximizing = Some(id);
        }
    }

    /// The drop-down terminal (DECISIONS D62): shown, it hides; hidden, it drops down on
    /// the monitor the user works on; with none, its program starts and its window, once
    /// it comes, drops down.
    fn toggle_dropdown(&mut self) {
        let alive = self.dropdown.filter(|id| Hwnd(*id).exists());
        self.dropdown = alive;
        match alive {
            Some(id) if Hwnd(id).is_visible() && !Hwnd(id).is_minimized() => {
                self.hide_all(&[Hwnd(id)]);
                tracing::info!(hwnd = id, "dropdown hidden");
            }
            Some(_) => self.show_dropdown(),
            None if self
                .dropdown_wait
                .is_some_and(|t| t.elapsed() < kabuk_core::dropdown::WAIT) => {}
            None => {
                let d = self.config.current().dropdown.clone();
                let (program, args) =
                    kabuk_core::dropdown::command(&d, kabuk_win::shellitems::windows_terminal_installed());
                match kabuk_win::jumplist::run(&program, &args, "") {
                    Ok(()) => {
                        tracing::info!(%program, "dropdown started");
                        self.dropdown_wait = Some(Instant::now());
                    }
                    Err(e) => tracing::warn!(%program, error = %e, "dropdown program not started"),
                }
            }
        }
    }

    /// The drop-down terminal along the top of the monitor the user works on, in front.
    fn show_dropdown(&mut self) {
        let Some(id) = self.dropdown else { return };
        let h = Hwnd(id);
        let monitors = kabuk_win::shell::monitors();
        let device = self.action_monitor();
        let Some(m) = monitors
            .iter()
            .find(|m| Some(&m.device) == device.as_ref())
            .or(monitors.first())
        else {
            return;
        };
        let height = self.config.current().dropdown.height;
        let at = kabuk_core::dropdown::place(m.work_area, height);
        if h.is_minimized() {
            h.restore_async();
        }
        h.show_async();
        let result = lock(&self.state).remove_hidden(id);
        if let Err(e) = result {
            tracing::error!(error = %e, "hidden record not cleared");
        }
        h.place_frame_async(at);
        if !h.focus() {
            tracing::debug!(hwnd = id, "focus refused by Windows");
        }
        tracing::info!(hwnd = id, monitor = %m.device, ?at, "dropdown shown");
    }

    /// Opens layout profile `name` (DECISIONS D65): each program open goes to its place,
    /// the rest start and their windows go there as they come.
    fn open_profile(&mut self, name: &str) {
        use kabuk_core::profile::{plan, Seen, Step};
        let Some(p) = self
            .config
            .current()
            .profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            tracing::warn!(name, "no such layout profile");
            return;
        };
        let open: Vec<Seen> = self
            .desk
            .all_windows()
            .into_iter()
            .filter_map(|id| {
                let exe = Hwnd(id)
                    .pid()
                    .and_then(|pid| kabuk_win::process::Process::open(pid).ok())
                    .and_then(|p| p.image_name())?;
                Some(Seen {
                    hwnd: id,
                    exe,
                    open: String::new(),
                    monitor: 0,
                    workspace: 0,
                    floating: false,
                })
            })
            .collect();
        let monitors = kabuk_win::shell::monitors();
        let (mut moved, mut started) = (0, 0);
        for step in plan(&p, &open) {
            match step {
                Step::Move {
                    hwnd,
                    monitor,
                    workspace,
                } => {
                    let Some(m) = monitors
                        .iter()
                        .find(|m| m.number == monitor)
                        .or_else(|| monitors.iter().find(|m| m.primary))
                    else {
                        continue;
                    };
                    let effects = self.desk.move_to_place(hwnd, &m.device, workspace);
                    self.apply(effects);
                    moved += 1;
                }
                Step::Launch(w) => {
                    let target = w.open.clone();
                    std::thread::spawn(move || {
                        kabuk_win::system::com_init();
                        if let Err(e) = kabuk_win::shellitems::open(&target) {
                            tracing::warn!(%target, error = %e, "layout profile program not started");
                        }
                    });
                    self.profile_wait.push((w, Instant::now()));
                    started += 1;
                }
            }
        }
        tracing::info!(name = %p.name, moved, started, "layout profile opened");
    }

    /// Where workspace actions apply: the focused window's monitor, else the primary one.
    fn action_monitor(&self) -> Option<String> {
        self.desk.focused_monitor().or_else(|| {
            kabuk_win::shell::monitors()
                .into_iter()
                .find(|m| m.primary)
                .map(|m| m.device)
        })
    }

    fn set_layout(&mut self, name: kabuk_core::ipc::LayoutName) {
        use kabuk_core::ipc::LayoutName;
        let Some(m) = self.action_monitor() else { return };
        let effects = match name {
            LayoutName::Dwindle => self.desk.set_kind(&m, Kind::Dwindle),
            LayoutName::Master => self.desk.set_kind(&m, Kind::Master),
            LayoutName::Floating => self.desk.set_workspace_floating(&m, true),
            LayoutName::Tiling => self.desk.set_workspace_floating(&m, false),
        };
        tracing::info!(layout = ?name, monitor = %m, "layout set");
        self.apply(effects);
    }

    fn reveal(&mut self, hwnd: u64) {
        let h = Hwnd(hwnd);
        // Minimised though not parked yet: Windows' event comes after (found in the test
        // VM, slower than the user's machine: the window stayed minimised).
        if self.desk.is_parked(hwnd) || h.is_minimized() {
            // Windows restores it; MinimizeEnd puts it back in its place. With the genie,
            // its genie is on screen first.
            if self.lead_genie(h, false) {
                return;
            }
            self.put_back_where_tiled(h);
            h.restore_async();
            if !h.focus() {
                tracing::debug!(hwnd, "focus refused by Windows");
            }
            return;
        }
        let effects = self.desk.reveal(hwnd);
        tracing::info!(hwnd, "window revealed from the launcher");
        self.apply(effects);
    }

    /// A click on the bar minimises `hwnd`: with the genie, once its genie is on screen.
    fn minimize(&mut self, hwnd: u64) {
        let h = Hwnd(hwnd);
        if !self.lead_genie(h, true) {
            h.minimize_async();
        }
    }

    /// Starts `h`'s genie before Kabuk minimises or restores it itself; false when there
    /// is no genie to play. The window minimises or comes back once its genie is on screen
    /// ([`GENIE_LEAD`]). A window coming back waited for its whole genie once, and showed
    /// blank for a frame as it came back unpainted at full size (found by the user); now it
    /// paints while its genie is still small.
    fn lead_genie(&mut self, h: Hwnd, minimize: bool) -> bool {
        let genie = if minimize {
            self.genie_of_shown(h)
        } else {
            self.genie_of(h, false, true)
        };
        let Some(g) = genie else { return false };
        self.own_genie = Some((g, std::time::Instant::now()));
        self.pending_genie = Some(g);
        self.animate_moves(&[]);
        self.schedule(timer::GENIE, GENIE_LEAD);
        tracing::debug!(hwnd = h.0, minimize, "genie leads");
        true
    }

    /// The genie has its window's place on screen: the window minimises under it, or
    /// comes back and to the front.
    fn genie_lead_over(&mut self) {
        let Some((g, since)) = self.own_genie else {
            return;
        };
        let h = g.window;
        if g.minimize {
            let pictured = self.mover.as_ref().is_none_or(|m| m.pictured(h));
            if !pictured && since.elapsed() < GENIE_PICTURE_WAIT {
                self.schedule(timer::GENIE, GENIE_PICTURE_STEP);
                return;
            }
            h.minimize_async();
        } else {
            self.put_back_where_tiled(h);
            h.restore_async();
            if !h.focus() {
                tracing::debug!(hwnd = h.0, "focus refused by Windows");
            }
        }
    }

    /// The genie Kabuk already started for `h`, going on from where it is, for the event
    /// its own minimise or restore brings; `None` for another window or after a while.
    /// It keeps its frame and icon (the icon moved between them once).
    fn own_genie_for(&mut self, h: Hwnd, minimize: bool) -> Option<Option<kabuk_win::window_motion::Genie>> {
        let (g, since) = self.own_genie?;
        let late = GENIE_OWN;
        if g.window != h || g.minimize != minimize || since.elapsed() > late {
            return None;
        }
        self.own_genie = None;
        // The event comes once the window is where the genie left it: no hold after.
        Some(Some(kabuk_win::window_motion::Genie {
            elapsed: since.elapsed().as_secs_f32(),
            hold: 0.0,
            ..g
        }))
    }

    fn toggle_floating_layout(&mut self, device: &str) {
        let effects = self.desk.toggle_workspace_floating(device);
        let floating = self.desk.workspaces(device).iter().any(|w| w.active && w.floating);
        tracing::info!(device, floating, "workspace layout switched");
        self.notice(if floating {
            crate::notify::Notice::FloatingLayout
        } else {
            crate::notify::Notice::TilingLayout
        });
        self.apply(effects);
    }

    fn reload_config(&mut self) {
        let text = match std::fs::read_to_string(&self.config_file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                tracing::error!(error = %e, "configuration unreadable; window manager keeps the current one");
                return;
            }
        };
        let before = self.config.current().clone();
        if let Reload::Rejected { .. } = self.config.reload(&text) {
            // The bar logs the errors; the last valid configuration stays.
            return;
        }
        let after = self.config.current().clone();
        let rebind = after.shortcuts != before.shortcuts || after.windows.workspaces != before.windows.workspaces;
        if rebind && self.shortcuts {
            self.bind_shortcuts();
        }
        if after.windows.follow_look != before.windows.follow_look {
            self.restyle_all();
        }
        if after.windows.minimize_effect != before.windows.minimize_effect
            || after.appearance.animations != before.appearance.animations
        {
            self.regenie_all();
        }
        if after.windows.active_border != before.windows.active_border || after.appearance != before.appearance {
            let accent = accent_of(&after);
            self.command(Command::SetAccent(accent));
        }
        // New rules apply to windows that open from now on.
        self.rules = rules::Rules::new(&after.rules);
        if let Ok(mut s) = self.snapshot.lock() {
            s.set_config(&after);
        }
        let effects = self.desk.set_settings(settings(&after));
        self.apply(effects);
        tracing::info!("window manager configuration applied");
    }

    /// A window appeared (or came back): manage it if the filter says so.
    fn consider(&mut self, hwnd: Hwnd) {
        // Kabuk's own windows are never managed, and never asked anything: reading the
        // title of a window of this process sends it a message, and its thread may be
        // waiting for this one.
        if hwnd.pid() == Some(std::process::id()) {
            return;
        }
        if self.dropdown == Some(hwnd.0) {
            return;
        }
        if self.desk.contains(hwnd.0) {
            // Known, but on a hidden workspace and showing itself: bring it to the
            // active workspace instead of leaving it on screen out of place.
            if !self.desk.visible_windows().contains(&hwnd.0) && hwnd.is_visible() {
                if let Some((device, _)) = self.desk.workspace_of(hwnd.0) {
                    if let Some(n) = self.desk.active_workspace(&device) {
                        tracing::info!(hwnd = hwnd.0, "window on a hidden workspace showed itself");
                        let effects = self.desk.move_to(hwnd.0, n);
                        self.apply(effects);
                    }
                }
            }
            return;
        }
        if let Some(class) = &self.manage_class {
            if hwnd.class_name() != *class {
                return;
            }
        }
        let facts = hwnd.facts();
        let floating = match policy::classify(&facts) {
            Verdict::Ignore => return,
            Verdict::Float => true,
            Verdict::Tile => false,
        };
        // The drop-down terminal's window, come after its program started.
        let waiting = self
            .dropdown_wait
            .is_some_and(|t| t.elapsed() < kabuk_core::dropdown::WAIT);
        if waiting && kabuk_core::dropdown::is_it(&self.config.current().dropdown, &facts.class) {
            self.dropdown_wait = None;
            self.dropdown = Some(hwnd.0);
            tracing::info!(hwnd = hwnd.0, class = %facts.class, "dropdown captured");
            self.show_dropdown();
            return;
        }
        let Some(opened_at) = hwnd.frame_rect().map(rect_of) else {
            return;
        };
        let exe = hwnd
            .pid()
            .and_then(|pid| kabuk_win::process::Process::open(pid).ok())
            .and_then(|p| p.image_name())
            .unwrap_or_default();
        let title = hwnd.title();
        let mut rule = self.rules.apply(&rules::Subject {
            exe: &exe,
            class: &facts.class,
            title: &title,
        });
        if rule.ignore {
            tracing::info!(hwnd = hwnd.0, exe = %exe, rules = ?rule.matched, "window ignored by a rule");
            return;
        }
        // A window a layout profile started: its place there (D65).
        self.profile_wait
            .retain(|(_, since)| since.elapsed() < kabuk_core::profile::WAIT);
        if let Some(i) = self
            .profile_wait
            .iter()
            .position(|(w, _)| w.exe.eq_ignore_ascii_case(&exe))
        {
            let (w, _) = self.profile_wait.remove(i);
            rule.monitor = Some(w.monitor);
            rule.workspace = Some(w.workspace);
            rule.float |= w.floating;
            tracing::info!(hwnd = hwnd.0, exe = %exe, "window placed by a layout profile");
        }
        let floating = floating || rule.float;
        if rule.no_grab {
            self.no_grab.insert(hwnd.0);
        }
        let monitors = kabuk_win::shell::monitors();
        // The rule's monitor ("Monitor n"), else the one the window opened on.
        let target = rule
            .monitor
            .and_then(|n| monitors.iter().find(|m| m.number == n))
            .or_else(|| monitors.iter().find(|m| contains_centre(m.rect, opened_at)))
            .or_else(|| monitors.iter().find(|m| m.primary));
        // Tiling is paused there: a new window keeps its own place (SPEC §2.4).
        let paused = target.is_some_and(|m| self.game.contains(&m.device));
        let floating = floating || paused;
        let dpi = target.map_or(kabuk_layout::desk::BASE_DPI, |m| m.dpi);
        let min = rule.min_width.map(|w| kabuk_layout::Size {
            w: scale_px(i32::try_from(w).unwrap_or(i32::MAX), dpi),
            h: 1,
        });
        let placed_at = match (rule.placement, target) {
            (Some(p), Some(m)) if floating => Some(kabuk_layout::spot_rect(
                rect_of(m.work_area),
                kabuk_layout::Size {
                    w: opened_at.w,
                    h: opened_at.h,
                },
                spot(p),
                scale_px(self.desk_gap(), dpi),
            )),
            _ => None,
        };
        tracing::info!(
            hwnd = hwnd.0,
            class = %facts.class,
            exe = %exe,
            floating,
            no_grab = rule.no_grab,
            rules = ?rule.matched,
            "managing window"
        );
        let id = hwnd.0;
        let effects = self.desk.add(NewWindow {
            id: hwnd.0,
            at: placed_at.unwrap_or(opened_at),
            monitor: rule.monitor.and(target).map(|m| m.device.as_str()),
            workspace: rule.workspace,
            floating,
            min,
            place: placed_at.is_some(),
        });
        self.apply(effects);
        self.style_window(id);
        self.genie_window(id);
    }

    fn forget(&mut self, hwnd: Hwnd, why: &str) {
        self.no_grab.remove(&hwnd.0);
        self.icons.remove(&hwnd.0);
        if self.genie.remove(&hwnd.0) {
            // Still there (now left alone): Windows' own transitions back.
            hwnd.set_transitions(true);
            let result = lock(&self.state).remove_genie(hwnd.0);
            if let Err(e) = result {
                tracing::error!(error = %e, "genie record not cleared");
            }
        }
        if self.styled.remove(&hwnd.0) {
            // Still there (now left alone): Windows' own frame back.
            hwnd.set_frame_style(None);
            let result = lock(&self.state).remove_styled(hwnd.0);
            if let Err(e) = result {
                tracing::error!(error = %e, "frame record not cleared");
            }
        }
        if !self.desk.contains(hwnd.0) {
            return;
        }
        tracing::info!(hwnd = hwnd.0, why, "window left");
        let effects = self.desk.remove(hwnd.0);
        if self.bordered == Some(hwnd.0) {
            self.bordered = None;
            let result = lock(&self.state).remove_bordered(hwnd.0);
            if let Err(e) = result {
                tracing::error!(error = %e, "border record not cleared");
            }
        }
        // It may have been hidden by Kabuk; it is gone or left on its own now.
        let result = lock(&self.state).remove_hidden(hwnd.0);
        if let Err(e) = result {
            tracing::error!(error = %e, "hidden record not cleared");
        }
        self.apply(effects);
    }

    fn on_event(&mut self, e: WindowEvent) {
        match e {
            WindowEvent::Shown(h) | WindowEvent::Uncloaked(h) => {
                if !self.desk.is_parked(h.0) {
                    self.consider(h);
                }
            }
            WindowEvent::MinimizeEnd(h) => {
                if self.desk.is_parked(h.0) {
                    tracing::info!(hwnd = h.0, "window restored");
                    let effects = self.desk.unpark(h.0);
                    self.pending_genie = match self.own_genie_for(h, false) {
                        Some(own) => own,
                        None => self.genie_of(h, false, false),
                    };
                    self.apply(effects);
                } else {
                    self.consider(h);
                }
            }
            WindowEvent::Hidden(h) => {
                // Hide events of windows Kabuk hid (on inactive workspaces) are expected.
                if self.desk.visible_windows().contains(&h.0) && !h.is_visible() {
                    self.forget(h, "hidden");
                }
            }
            WindowEvent::Cloaked(h) => {
                // Cloaked windows (another virtual desktop) still count as visible.
                if self.desk.visible_windows().contains(&h.0) && h.is_cloaked() {
                    self.forget(h, "cloaked");
                }
            }
            WindowEvent::MinimizeStart(h) => {
                if self.desk.contains(h.0) && !self.desk.is_parked(h.0) {
                    tracing::info!(hwnd = h.0, "window minimized");
                    // Where it was, before parking forgets it.
                    self.pending_genie = match self.own_genie_for(h, true) {
                        Some(own) => own,
                        None => self.genie_of(h, true, false),
                    };
                    let effects = self.desk.park(h.0);
                    self.apply(effects);
                }
            }
            WindowEvent::Destroyed(h) => {
                if self.dropdown == Some(h.0) {
                    self.dropdown = None;
                    let result = lock(&self.state).remove_hidden(h.0);
                    if let Err(e) = result {
                        tracing::error!(error = %e, "hidden record not cleared");
                    }
                    tracing::info!(hwnd = h.0, "dropdown closed");
                }
                self.forget(h, "destroyed")
            }
            WindowEvent::Foreground(h) => {
                if self.desk.contains(h.0) {
                    self.desk.focused(h.0);
                    self.update_border();
                    self.publish();
                }
            }
            WindowEvent::MoveSizeEnd(h) => {
                if let Some(now) = h.frame_rect().map(rect_of) {
                    let was_floating = self.desk.is_floating(h.0);
                    let effects = self.desk.moved(h.0, now);
                    if self.desk.is_floating(h.0) && !was_floating {
                        tracing::info!(hwnd = h.0, "window floated by a drag");
                        self.notice(crate::notify::Notice::Floated);
                    }
                    self.apply(effects);
                }
            }
            WindowEvent::MoveSizeStart(h) => {
                // The user holds it now: a pending placement check would fight the drag.
                self.checks.retain(|(w, _, _)| *w != h.0);
            }
        }
    }

    /// Applies effects in order. A window is recorded before it is hidden; if recording
    /// fails it stays visible.
    fn apply(&mut self, effects: Vec<Effect>) {
        self.animate_moves(&effects);
        // The windows to hide are on disk together before any of them hides, and those
        // shown again leave the record together, after.
        let hiding: Vec<Hwnd> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Hide(id) => Some(Hwnd(*id)),
                _ => None,
            })
            .collect();
        self.hide_all(&hiding);
        let mut shown = Vec::new();
        let mut placed = false;
        for e in effects {
            match e {
                Effect::Hide(_) => {}
                Effect::Show(id) => {
                    Hwnd(id).show_async();
                    shown.push(id);
                }
                Effect::Place(id, r) => {
                    // Windows stay where they are while a full-screen program runs there.
                    if self.desk.workspace_of(id).is_some_and(|(d, _)| self.game.contains(&d)) {
                        continue;
                    }
                    let h = Hwnd(id);
                    // A window the user maximised or minimised keeps that state.
                    if (h.is_maximized() && self.unmaximizing != Some(id)) || h.is_minimized() {
                        continue;
                    }
                    if h.place_frame_async(state_rect(r)) {
                        self.checks.retain(|(w, _, _)| *w != id);
                        self.checks.push((id, r, 0));
                        placed = true;
                    }
                }
                Effect::Focus(id) => {
                    if !Hwnd(id).focus() {
                        tracing::debug!(hwnd = id, "focus refused by Windows");
                    }
                    self.update_border();
                }
            }
        }
        if !shown.is_empty() {
            let result = lock(&self.state).remove_hidden_all(&shown);
            if let Err(e) = result {
                tracing::error!(error = %e, "hidden record not cleared");
            }
        }
        if placed {
            self.schedule(timer::CHECK, CHECK_DELAY);
        }
        self.publish();
    }

    /// Whether Kabuk plays the genie: chosen, with animations in full, outside game mode.
    fn genie_on(&self) -> bool {
        let c = self.config.current();
        c.windows.minimize_effect == config::MinimizeEffect::Genie
            && c.appearance.animations == config::Animations::Full
            && self.game.is_empty()
            && kabuk_win::system::animations_on()
    }

    /// Turns window `id`'s own transitions off for the genie (recorded first), or back on.
    fn genie_window(&mut self, id: u64) {
        let h = Hwnd(id);
        if self.genie_on() && self.desk.contains(id) {
            if self.genie.contains(&id) {
                return;
            }
            let Some(pid) = h.pid() else { return };
            let record = HiddenWindow {
                hwnd: id,
                pid,
                class: h.class_name(),
                title: h.title(),
            };
            let result = lock(&self.state).add_genie(record);
            if let Err(e) = result {
                tracing::error!(error = %e, "genie not recorded; Windows' transitions kept");
                return;
            }
            if h.set_transitions(false) {
                self.genie.insert(id);
                tracing::debug!(hwnd = id, "the genie minimises it");
            }
        } else if self.genie.remove(&id) {
            let back = h.set_transitions(true);
            tracing::debug!(hwnd = id, back, "Windows' transitions back");
            let result = lock(&self.state).remove_genie(id);
            if let Err(e) = result {
                tracing::error!(error = %e, "genie record not cleared");
            }
        }
    }

    /// The genie on every managed window, or Windows' transitions back on all of them.
    fn regenie_all(&mut self) {
        let ids: Vec<u64> = self
            .desk
            .all_windows()
            .into_iter()
            .chain(self.genie.iter().copied())
            .collect();
        for id in ids {
            self.genie_window(id);
        }
    }

    /// The genie for window `h` minimising (or restoring): from (or to) its frame, into
    /// (or out of) its icon on the bar, or the middle of its monitor's bar edge.
    fn genie_of(&self, h: Hwnd, minimize: bool, before_restore: bool) -> Option<kabuk_win::window_motion::Genie> {
        if !self.genie.contains(&h.0) || !self.genie_on() {
            return None;
        }
        // Where the window is (or comes back) as Windows has it: the window manager's own
        // place can be stale for a maximised window or one moved to another monitor.
        let monitors = kabuk_win::shell::monitors();
        let primary = monitors.iter().find(|m| m.primary)?;
        let (normal, maximized) = h.restored_place()?;
        let place = if before_restore {
            // Where the window manager puts it back (Windows is told the same first).
            match self.desk.parked_place(h.0) {
                Some((_, r)) => state_rect(r),
                None => kabuk_core::genie::workspace_to_screen(normal, primary.rect, primary.work_area),
            }
        } else if minimize {
            kabuk_core::genie::workspace_to_screen(normal, primary.rect, primary.work_area)
        } else if maximized {
            h.frame_rect()?
        } else {
            // Restoring: where the window manager puts it back now.
            self.desk.rect_of(h.0).map(state_rect).or_else(|| h.frame_rect())?
        };
        let centre = ((place.left + place.right) / 2, (place.top + place.bottom) / 2);
        let monitor = monitors.iter().find(|m| {
            let r = m.rect;
            centre.0 >= r.left && centre.0 < r.right && centre.1 >= r.top && centre.1 < r.bottom
        })?;
        let frame = if maximized { monitor.work_area } else { place };
        let icon = self
            .icons
            .get(&h.0)
            .copied()
            .unwrap_or_else(|| bar_edge_spot(monitor.rect, monitor.work_area));
        Some(kabuk_win::window_motion::Genie {
            window: h,
            frame,
            icon,
            minimize,
            seconds: kabuk_ui::motion::WINDOW_GENIE,
            elapsed: 0.0,
            hold: 0.0,
        })
    }

    /// Before Kabuk restores a minimised window: Windows is told to put it back where the
    /// window manager will, so it never shows first where Windows last had it (found by
    /// the user: a window of Monitor 2 came back on Monitor 1 for a moment, then jumped).
    fn put_back_where_tiled(&self, h: Hwnd) {
        let Some((_, place)) = self.desk.parked_place(h.0) else {
            return;
        };
        let monitors = kabuk_win::shell::monitors();
        let Some(primary) = monitors.iter().find(|m| m.primary) else {
            return;
        };
        let normal = kabuk_core::genie::screen_to_workspace(state_rect(place), primary.rect, primary.work_area);
        if !h.set_restored_place(normal) {
            tracing::debug!(hwnd = h.0, "restored place not set");
        }
    }

    /// The genie for a window still shown (Kabuk is about to minimise it): from where it
    /// is on screen.
    fn genie_of_shown(&self, h: Hwnd) -> Option<kabuk_win::window_motion::Genie> {
        if !self.genie.contains(&h.0) || !self.genie_on() || h.is_minimized() {
            return None;
        }
        let frame = h.frame_rect()?;
        let monitors = kabuk_win::shell::monitors();
        let centre = ((frame.left + frame.right) / 2, (frame.top + frame.bottom) / 2);
        let monitor = monitors.iter().find(|m| {
            let r = m.rect;
            centre.0 >= r.left && centre.0 < r.right && centre.1 >= r.top && centre.1 < r.bottom
        })?;
        let icon = self
            .icons
            .get(&h.0)
            .copied()
            .unwrap_or_else(|| bar_edge_spot(monitor.rect, monitor.work_area));
        Some(kabuk_win::window_motion::Genie {
            window: h,
            frame,
            icon,
            minimize: true,
            seconds: kabuk_ui::motion::WINDOW_GENIE,
            elapsed: 0.0,
            hold: 0.0,
        })
    }

    /// Plays the moves among `effects` over each work area they happen on, before the
    /// windows jump there, and a pending genie over its whole monitor (only with full
    /// animations, and never in game mode).
    fn animate_moves(&mut self, effects: &[Effect]) {
        let genie = self.pending_genie.take();
        let Some(mover) = &self.mover else { return };
        let full = self.config.current().appearance.animations == config::Animations::Full;
        if !full || !self.game.is_empty() || !kabuk_win::system::animations_on() {
            return;
        }
        let mut moving = std::collections::HashMap::new();
        for e in effects {
            let Effect::Place(id, r) = *e else { continue };
            let h = Hwnd(id);
            if !h.is_visible() || h.is_maximized() || h.is_minimized() {
                continue;
            }
            let Some((outer, visible)) = h.outer_and_frame() else {
                continue;
            };
            let to = with_borders(state_rect(r), outer, visible);
            if to != outer {
                moving.insert(id, to);
            }
        }
        if moving.is_empty() && genie.is_none() {
            return;
        }
        let seconds = kabuk_ui::motion::WINDOW_MOVE;
        for m in kabuk_win::shell::monitors() {
            // The genie's monitor plays over all of it: the icon is on its bar.
            let here_genie = genie.filter(|g| {
                let (cx, cy) = ((g.frame.left + g.frame.right) / 2, (g.frame.top + g.frame.bottom) / 2);
                cx >= m.rect.left && cx < m.rect.right && cy >= m.rect.top && cy < m.rect.bottom
            });
            let a = if here_genie.is_some() { m.rect } else { m.work_area };
            let here: std::collections::HashMap<u64, state::Rect> = moving
                .iter()
                .filter(|(_, to)| {
                    let (cx, cy) = ((to.left + to.right) / 2, (to.top + to.bottom) / 2);
                    cx >= a.left && cx < a.right && cy >= a.top && cy < a.bottom
                })
                .map(|(id, to)| (*id, *to))
                .collect();
            if here.is_empty() && here_genie.is_none() {
                continue;
            }
            let layers = kabuk_win::window_motion::layers_over(a, &here, here_genie.map(|g| g.window));
            tracing::info!(device = %m.device, moving = here.len(), layers = layers.len(), "windows move");
            if let Some(g) = &here_genie {
                tracing::info!(hwnd = g.window.0, minimize = g.minimize, frame = ?g.frame, icon = ?g.icon, "genie");
            }
            mover.play(kabuk_win::window_motion::Scene {
                area: a,
                layers,
                seconds,
                genie: here_genie,
            });
        }
    }

    /// Records `windows` in the state file, in one write, then hides them (a window
    /// gone meanwhile is left out).
    fn hide_all(&mut self, windows: &[Hwnd]) {
        let records: Vec<HiddenWindow> = windows
            .iter()
            .filter_map(|h| {
                Some(HiddenWindow {
                    hwnd: h.0,
                    pid: h.pid()?,
                    class: h.class_name(),
                    title: h.title(),
                })
            })
            .collect();
        if records.is_empty() {
            return;
        }
        let result = lock(&self.state).add_hidden_all(records.clone());
        match result {
            Ok(()) => {
                for r in &records {
                    Hwnd(r.hwnd).hide_async();
                }
            }
            Err(e) => tracing::error!(windows = records.len(), error = %e, "windows not recorded; left visible"),
        }
    }

    /// A window not where it was put (it refuses to move) or larger (its minimum size) is
    /// asked once more; refusing twice floats it, so a slow window is not floated for
    /// answering late.
    fn check_placements(&mut self) {
        let checks = std::mem::take(&mut self.checks);
        let mut effects = Vec::new();
        let mut again = false;
        for (id, want, asked) in checks {
            let h = Hwnd(id);
            // Minimised or hidden since it was placed: not a refusal (found in the log: a
            // window minimised at once was floated for "refusing" -32000).
            if h.is_minimized() || !h.is_visible() {
                continue;
            }
            let Some(got) = h.frame_rect().map(rect_of) else {
                continue;
            };
            if !refused(want, got) {
                continue;
            }
            if asked < PLACE_ATTEMPTS - 1 {
                if h.place_frame_async(state_rect(want)) {
                    self.checks.push((id, want, asked + 1));
                    again = true;
                }
                continue;
            }
            tracing::info!(hwnd = id, ?want, ?got, "window refused its size");
            effects.extend(self.desk.refused(id, kabuk_layout::Size { w: got.w, h: got.h }));
        }
        if again {
            self.schedule(timer::CHECK, CHECK_DELAY);
        }
        if !effects.is_empty() {
            self.apply(effects);
        }
    }

    /// The look's frame for every managed window, or Windows' own back when the setting
    /// is off (DECISIONS D30).
    fn restyle_all(&mut self) {
        let ids: Vec<u64> = self.desk.all_windows();
        for id in ids {
            self.style_window(id);
        }
    }

    /// Gives window `id` the look's frame (recorded first) or takes it off.
    fn style_window(&mut self, id: u64) {
        let want = self.frame.filter(|_| self.config.current().windows.follow_look);
        let h = Hwnd(id);
        match want {
            Some(frame) => {
                let Some(pid) = h.pid() else { return };
                let record = HiddenWindow {
                    hwnd: id,
                    pid,
                    class: h.class_name(),
                    title: h.title(),
                };
                let result = lock(&self.state).add_styled(record);
                if let Err(e) = result {
                    tracing::error!(error = %e, "frame not recorded; left as it is");
                    return;
                }
                if h.set_frame_style(Some(frame)) {
                    self.styled.insert(id);
                    tracing::debug!(hwnd = id, ?frame, "frame follows the look");
                }
            }
            None if self.styled.remove(&id) => {
                let reset = h.set_frame_style(None);
                tracing::debug!(hwnd = id, reset, "frame back to Windows'");
                let result = lock(&self.state).remove_styled(id);
                if let Err(e) = result {
                    tracing::error!(error = %e, "frame record not cleared");
                }
            }
            None => {}
        }
    }

    /// Puts the accent frame on the focused managed window, and takes it off the last one.
    fn update_border(&mut self) {
        let want = self.accent.and(self.desk.focus()).filter(|id| self.desk.contains(*id));
        if want == self.bordered {
            return;
        }
        if let Some(old) = self.bordered.take() {
            Hwnd(old).set_border_color(None);
            let result = lock(&self.state).remove_bordered(old);
            if let Err(e) = result {
                tracing::error!(error = %e, "border record not cleared");
            }
        }
        let (Some(id), Some(rgb)) = (want, self.accent) else {
            return;
        };
        let h = Hwnd(id);
        let Some(pid) = h.pid() else { return };
        let record = HiddenWindow {
            hwnd: id,
            pid,
            class: h.class_name(),
            title: h.title(),
        };
        let result = lock(&self.state).add_bordered(record);
        match result {
            Ok(()) => {
                if h.set_border_color(Some(rgb)) {
                    self.bordered = Some(id);
                } else {
                    // Refused (an elevated window): the record goes again. Under the lock
                    // taken above, this froze the window manager and the bar (found by
                    // the user, showing Hyper-V Manager from the bar).
                    let result = lock(&self.state).remove_bordered(id);
                    if let Err(e) = result {
                        tracing::error!(error = %e, "border record not cleared");
                    }
                }
            }
            Err(e) => tracing::error!(error = %e, "border not recorded; frame left alone"),
        }
    }

    /// A full-screen program covers `devices` (SPEC §2.4): the mouse hook and the
    /// keyboard hook let everything pass while any does (D40), tiling pauses there when
    /// the setting says so, and the layout comes back as it was once they are free again.
    fn set_game_mode(&mut self, devices: Vec<String>) {
        crate::grab::pause(&self.snapshot, !devices.is_empty());
        let covered = kabuk_win::shell::monitors()
            .into_iter()
            .filter(|m| devices.contains(&m.device))
            .map(|m| m.handle)
            .collect();
        kabuk_win::keyhook::set_paused_on(covered);
        let paused: std::collections::HashSet<String> = if self.config.current().game.pause_tiling {
            devices.into_iter().collect()
        } else {
            std::collections::HashSet::new()
        };
        if paused == self.game {
            return;
        }
        let freed = self.game.difference(&paused).count() > 0;
        self.game = paused;
        // Windows' own transitions in game mode, the genie again after it.
        self.regenie_all();
        tracing::info!(devices = ?self.game, "tiling paused for full-screen programs");
        if freed {
            // Whatever moved meanwhile goes back to its tile.
            let effects = self.desk.set_settings(settings(self.config.current()));
            self.apply(effects);
        }
        self.publish();
    }

    /// Something to tell the user; the bar shows it with the next summary.
    fn notice(&mut self, n: crate::notify::Notice) {
        /// Notices kept for a bar that is slow to read them.
        const KEPT: usize = 8;
        self.notice_seq += 1;
        self.notices.push_back((self.notice_seq, n));
        while self.notices.len() > KEPT {
            self.notices.pop_front();
        }
        self.publish();
    }

    /// Updates the bar's summary and tells the bar when it changed.
    fn publish(&mut self) {
        if let Ok(mut s) = self.snapshot.lock() {
            // The input hook grabs only these; rule-exempted windows keep their Alt + clicks.
            s.managed = self
                .desk
                .visible_windows()
                .into_iter()
                .filter(|id| !self.no_grab.contains(id))
                .map(|id| (id, self.desk.is_floating(id)))
                .collect();
        }
        let next = WmView {
            monitors: kabuk_win::shell::monitors()
                .into_iter()
                .map(|m| MonitorView {
                    workspaces: self.desk.workspaces(&m.device),
                    number: m.number,
                    layout: self
                        .desk
                        .active_workspace(&m.device)
                        .and_then(|n| self.desk.layout(&m.device, n))
                        .unwrap_or(Kind::Dwindle),
                    game: self.game.contains(&m.device),
                    device: m.device,
                })
                .collect(),
            windows: self
                .desk
                .placements()
                .into_iter()
                .map(|(hwnd, device, workspace)| WindowView {
                    hwnd,
                    device,
                    workspace,
                    floating: self.desk.is_floating(hwnd),
                    minimized: self.desk.is_parked(hwnd),
                    rect: self.desk.rect_of(hwnd),
                })
                .collect(),
            focused: self.desk.focus(),
            notices: self.notices.iter().copied().collect(),
            tab_strips: self
                .desk
                .tab_strips()
                .into_iter()
                .map(|t| StripView {
                    group: t.group,
                    rect: t.rect,
                    tabs: t
                        .tabs
                        .into_iter()
                        .map(|(id, shown)| (id, Hwnd(id).title(), shown))
                        .collect(),
                })
                .collect(),
        };
        let changed = match self.view.lock() {
            Ok(mut v) if *v != next => {
                *v = next;
                true
            }
            _ => false,
        };
        if changed {
            if let Some((window, id)) = self.listener.lock().ok().and_then(|l| *l) {
                let _ = msgloop::post_message(
                    window,
                    Msg {
                        id,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: an exact placement, rounding within the tolerance, a larger window, a
    //! window left elsewhere, a smaller (grid-snapped) window; settings from defaults, a
    //! floating default and a gap override.
    use super::*;

    const WANT: Rect = Rect {
        x: 13,
        y: 78,
        w: 1260,
        h: 1289,
    };

    #[test]
    fn refusal_is_larger_or_elsewhere() {
        assert!(!refused(WANT, WANT));
        assert!(!refused(
            WANT,
            Rect {
                x: 14,
                y: 77,
                w: 1262,
                h: 1287,
            }
        ));
        assert!(refused(WANT, Rect { w: 1900, ..WANT }));
        assert!(refused(WANT, Rect { x: 400, ..WANT }));
        assert!(!refused(
            WANT,
            Rect {
                w: 1240,
                h: 1270,
                ..WANT
            }
        ));
    }

    #[test]
    fn a_frame_takes_the_invisible_borders_of_its_window() {
        let outer = state::Rect {
            left: 93,
            top: 70,
            right: 1027,
            bottom: 787,
        };
        let visible = state::Rect {
            left: 100,
            top: 70,
            right: 1020,
            bottom: 780,
        };
        let frame = state::Rect {
            left: 10,
            top: 20,
            right: 510,
            bottom: 420,
        };
        assert_eq!(
            with_borders(frame, outer, visible),
            state::Rect {
                left: 3,
                top: 20,
                right: 517,
                bottom: 427
            }
        );
        assert_eq!(with_borders(visible, outer, visible), outer);
        // The easing starts and ends where the move does.
        assert!(ease(0.0).abs() < 1e-5 && (ease(1.0) - 1.0).abs() < 1e-5 && ease(0.5) > 0.5);
    }

    #[test]
    fn settings_follow_the_configuration() {
        let mut cfg = Config::default();
        cfg.appearance.look = config::Look::Ada;
        let s = settings(&cfg);
        assert_eq!(s.workspaces, 9);
        assert_eq!(s.kind, Kind::Dwindle);
        assert!(!s.floating_layout && s.float_unfit);
        // Ada's `--gap` when `windows.gap` is absent.
        assert_eq!(s.gaps, Gaps { inner: 14, outer: 14 });
        let mut c = cfg.clone();
        c.windows.gap = Some(0);
        c.windows.new_window = config::NewWindow::Floating;
        c.windows.tiling_layout = config::TilingLayout::Master;
        let s = settings(&c);
        assert_eq!(s.gaps, Gaps { inner: 0, outer: 0 });
        assert!(s.floating_layout);
        assert_eq!(s.kind, Kind::Master);
        // The frame colour is the look's accent, unless turned off.
        assert!(accent_of(&Config::default()).is_some());
        let mut off = Config::default();
        off.windows.active_border = false;
        assert_eq!(accent_of(&off), None);
    }
}
