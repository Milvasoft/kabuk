//! Runs the bars: one window per monitor, its AppBar reservation, the data sources
//! (clock, stats, audio, media, focused window) and redraws when something changes.
//!
//! Everything lives on the UI thread. Window handlers only reach the shared state
//! through `try_borrow_mut`, and anything that destroys windows (rebuilding the bars)
//! is deferred to the control window's timer so no window is destroyed inside its own
//! window procedure. Notifications that must not be lost (app mode, Explorer restart,
//! display changes) are noted in [`Pending`] before borrowing, so one that arrives while
//! the state is busy is handled from a timer instead of dropped. Everything Kabuk sends to
//! its own control window is posted, never sent: a sent message can arrive inside a
//! nested call (an AppBar call waiting on Explorer) while the state is borrowed.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kabuk_core::clock::WallTime;
use kabuk_core::config::{
    self, BarBackground, BarMonitors, ClockFormat, Config, Edge, LiveConfig, Reload, WorkspaceLayout,
};
use kabuk_core::state::{TaskbarBackup, WorkAreaBackup};
use kabuk_core::tray::{click_messages, Click, Icon, IconCall, IconKey, Tray};
use kabuk_i18n::{ClockStyle, I18n};
use kabuk_theme::resolve::{minutes_to_next_flip, mode_now, resolve, ModeChoice, TextSize};
use kabuk_theme::{Mode, Tokens};
use kabuk_ui::geom::{Point, Rect, Scale, Size};
use kabuk_ui::node::{Damage, Draw, Layout};
use kabuk_ui::render::{Gpu, Palette, PixelRect, Raster, RenderError, WindowSurface};
use kabuk_win::appbar::{self, AppBar};
use kabuk_win::audio::{self, Audio, Notify};
use kabuk_win::events::{self, ForegroundWatch};
use kabuk_win::media::{Media, MediaInfo};
use kabuk_win::msgloop::{self, msg, Msg, PxRect, Window};
use kabuk_win::process::ExitWatch;
use kabuk_win::shell::{self, Monitor};
use kabuk_win::stats::{Sample, Sampler, Wants};
use kabuk_win::taskbar_guard::TaskbarGuard;
use kabuk_win::tray::{TrayEvent, TrayHost};
use kabuk_win::window::Hwnd;

use super::dock;
use super::geometry::{self, BarSize};
use super::style::{Style, BLUR_TINT_ALPHA};
use super::theme;
use super::tooltip;
use super::view::{self, BarData, Title, Volume, Workspace};
use super::volume_panel;
use crate::wm::{self, SharedState, WmLink};

#[path = "launcher_host.rs"]
mod launcher_host;
pub use launcher_host::{LAUNCHER_CLASS, MSG_TEST_LAUNCHER};
#[path = "appgrid_host.rs"]
mod appgrid_host;
#[path = "apps_host.rs"]
mod apps_host;
#[path = "cursors_host.rs"]
pub mod cursors_host;
#[path = "gesture_host.rs"]
mod gesture_host;
#[path = "idle_host.rs"]
mod idle_host;
#[path = "motion_host.rs"]
mod motion_host;
#[path = "notify_host.rs"]
mod notify_host;
#[path = "overview_host.rs"]
mod overview_host;
pub use appgrid_host::{
    CHROME_CLASS as APP_GRID_CHROME_CLASS, GRID_CLASS as APP_GRID_CLASS, MSG_OPEN_APP_GRID, MSG_TEST_APP_GRID,
};
#[path = "system_colors_host.rs"]
mod system_colors_host;
pub use overview_host::{MSG_OPEN_OVERVIEW, MSG_TEST_OVERVIEW};
pub use saver_host::MSG_START_SAVER;
#[path = "app_menu_host.rs"]
mod app_menu_host;
#[path = "app_preview_host.rs"]
mod app_preview_host;
pub use app_preview_host::MSG_TEST_APP_PREVIEW;
pub use wallpaper_host::MSG_TEST_DRIFT;
#[path = "keys_card_host.rs"]
mod keys_card_host;
#[path = "media_card_host.rs"]
mod media_card_host;
#[path = "power_menu_host.rs"]
mod power_menu_host;
#[path = "saver_host.rs"]
mod saver_host;
#[path = "tabs_host.rs"]
mod tabs_host;
#[path = "volume_panel_host.rs"]
mod volume_panel_host;
#[path = "wallpaper_host.rs"]
mod wallpaper_host;
pub use app_menu_host::{
    MSG_TEST_APP_MENU, TEST_CLOSE as APP_MENU_CLOSE, TEST_CLOSE_WINDOWS as APP_MENU_CLOSE_WINDOWS,
    TEST_FLOAT_RULE as APP_MENU_FLOAT_RULE, TEST_OPEN as APP_MENU_OPEN, TEST_PIN as APP_MENU_PIN,
    TEST_TO_FRONT as APP_MENU_TO_FRONT,
};
pub use keys_card_host::MSG_TEST_KEYS_CARD;
pub use media_card_host::MSG_TEST_MEDIA_CARD;
pub use power_menu_host::MSG_TEST_POWER_MENU;
pub use volume_panel_host::MSG_TEST_VOLUME_PANEL;
/// Windows' notification centre.
const NOTIFICATION_CENTRE: &str = "ms-actioncenter:";
/// The notification count, counted on its thread (DECISIONS D32).
const MSG_NOTIF_COUNT: u32 = msg::APP + 31;
/// Windows' notification centre changed: count again.
const MSG_NOTIF_CHANGED: u32 = msg::APP + 34;
pub use apps_host::MSG_TEST_APP_CLICK;
pub use idle_host::{MSG_INPUT, MSG_TEST_DIM};
/// Integration tests: a touchpad swipe (`wparam`: 0 left, 1 right, 2 up, 3 down), as the
/// touchpad's fingers would make it (DECISIONS D68; this machine has no touchpad).
pub const MSG_TEST_SWIPE: u32 = msg::APP + 51;
#[path = "tray_panel_host.rs"]
mod tray_panel_host;
pub use tray_panel_host::{MSG_TEST_FOREGROUND, MSG_TEST_TRAY_PANEL, MSG_TEST_TRAY_PANEL_CLICK};

const BAR_CLASS: &str = kabuk_core::hang::BAR_CLASS;
const DOCK_CLASS: &str = "Kabuk.Dock";
const TOOLTIP_CLASS: &str = "Kabuk.Tooltip";
/// Class of the bars' message-only control window (tests find it by class).
pub const CONTROL_CLASS: &str = "Kabuk.BarControl";
/// Integration tests click the tray icon owned by window `wparam` with this message
/// (`lparam` 0 left, 1 right); by owner, so a test can only click its own icon.
pub const MSG_TEST_TRAY_CLICK: u32 = msg::APP + 20;

/// Control-window timers.
mod timer {
    pub const CLOCK: usize = 1;
    pub const STATS: usize = 2;
    pub const GPU: usize = 3;
    pub const SHIFT: usize = 4;
    pub const CONFIG: usize = 5;
    pub const REBUILD: usize = 6;
    pub const MODE: usize = 7;
    pub const PENDING: usize = 8;
    /// While Explorer restarts: the fallback if it never announces its taskbar.
    pub const EXPLORER: usize = 9;
    /// Asks programs to register their tray icons again.
    pub const TRAY_AGAIN: usize = 10;
    /// Acts once on a `TaskbarCreated` broadcast that reached several windows.
    pub const ANNOUNCED: usize = 11;
    /// Takes the tray once Explorer has settled.
    pub const TRAY_START: usize = 12;
    /// Shows the tooltip of the tray icon under the mouse.
    pub const TOOLTIP: usize = 13;
    /// Hides the notice and the volume OSD.
    pub const TOAST: usize = 14;
    pub const OSD: usize = 15;
    /// Idle dimming: looks again when the time left runs out.
    pub const IDLE: usize = 16;
    /// Drops the closed popups whose exit has played.
    pub const LEAVING: usize = 17;
    /// Drops the bars' old picture once a change of look has blended in.
    pub const CROSSFADE: usize = 18;
    /// Steps an app's growth under the mouse.
    pub const APP_GROW: usize = 19;
    /// Shows the previews of the app the mouse rests on, and hides them once it left.
    pub const APP_PREVIEW: usize = 20;
    pub const APP_PREVIEW_HIDE: usize = 21;
    /// The still picture's next drift step (DECISIONS D47).
    pub const DRIFT: usize = 22;
    /// The mouse has been away from an auto-hiding bar long enough: it slides off.
    pub const AUTOHIDE: usize = 24;
    /// Steps the auto-hiding bars' slides.
    pub const BAR_SLIDE: usize = 25;
    /// Waits for the screen saver shortcut's keys to be let go.
    pub const SAVER_KEYS: usize = 26;
    /// The app grid's page has slid into place.
    pub const APP_GRID_SLIDE: usize = 27;
    /// The app grid's first icons are loaded ahead.
    pub const APP_GRID_AHEAD: usize = 28;
    /// A press on an app of the grid was held: editing begins.
    pub const APP_GRID_HOLD: usize = 29;
    /// An app dragged to a side edge of the grid stayed there: the page turns.
    pub const APP_GRID_EDGE: usize = 30;
    /// Kabuk's first start: the shortcut card greets the user.
    pub const GREETING: usize = 31;
    /// The edited app grid's icons wiggle: the next frame.
    pub const APP_GRID_WIGGLE: usize = 32;
    /// A tray click's program had its time to open its menu: the menu watch ends.
    pub const TRAY_MENU: usize = 33;
    /// The mouse rested at an auto-hiding bar's (or the dock's) edge: it comes in.
    pub const REVEAL: usize = 34;
    /// The window manager had its moment to put its scene up: the auto-hiding bars
    /// reserve their strips or give them back.
    pub const RESERVE: usize = 35;
}

/// An auto-hiding bar (SPEC §2.1): how much of it stays on screen for the mouse to reach
/// (DIPs), how long the mouse may be away before it slides off, and how often its slide
/// is stepped.
const AUTOHIDE_PEEK: f32 = 2.0;
/// A volume key the keyboard hook took (`wparam`: its [`kabuk_win::keyhook::VolumeKey`]).
const MSG_VOLUME_KEY: u32 = msgloop::msg::APP + 42;
/// A frame of the screen while an auto-hiding bar slides ([`kabuk_win::ticker`]).
const MSG_SLIDE_FRAME: u32 = msgloop::msg::APP + 53;
/// A frame of the screen while the dock's tiles grow or settle ([`kabuk_win::ticker`]).
const MSG_DOCK_FRAME: u32 = msgloop::msg::APP + 54;
const AUTOHIDE_DELAY: Duration = Duration::from_millis(600);
const BAR_SLIDE_STEP: Duration = Duration::from_millis(16);
/// How long the mouse rests at an auto-hiding bar's edge before it comes in (found by
/// the user: it came over the windows whenever the mouse passed the edge, on its way to
/// a window's top; DECISIONS D70).
const REVEAL_DELAY: Duration = Duration::from_millis(250);

/// What auto-hides: a bar (by its index) or Rıhtım's dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Peeker {
    Bar(usize),
    Dock,
}
/// How often the mouse on an app's icon tells the window manager a genie may come.
const GENIE_WARM_EVERY: Duration = Duration::from_secs(10);

/// How long an app's icon takes to grow under the mouse or shrink back, and how often
/// it is drawn meanwhile.
const APP_GROW_SECONDS: f32 = 0.12;
const APP_GROW_STEP_MS: u32 = 16;
/// How far the mouse moves on a held app before it is a drag (DIPs).
const APP_DRAG_SLOP: f32 = 4.0;
/// How long an app's picture takes to slide into its new place (macOS's dock is about
/// as quick).
const APP_SLIDE_SECONDS: f32 = 0.16;

/// An app held with the left button: where, and whether it is being dragged.
#[derive(Clone, Copy, Debug)]
struct AppDrag {
    bar: usize,
    app: usize,
    at: (i32, i32),
    dragging: bool,
    /// Where the dragged picture is drawn from its place, following the mouse (DIPs).
    follow: (f32, f32),
}

/// An app's picture sliding into its place: from `from` (DIPs off its place) since `since`.
#[derive(Clone, Debug)]
struct AppSlide {
    /// The app's [`crate::bar::apps::slide_ids`] name.
    id: String,
    from: (f32, f32),
    since: std::time::Instant,
}

/// The app under the mouse and its growth, from `from` towards `to` since `since`.
#[derive(Clone, Copy, Debug)]
struct AppHover {
    app: Option<usize>,
    /// The app shrinking back (the one hovered before).
    leaving: Option<usize>,
    since: std::time::Instant,
}

/// The numbers of the active workspaces.
fn active(w: &[Workspace]) -> Vec<u32> {
    w.iter().filter(|w| w.active).map(|w| w.number).collect()
}

/// Sampling and debounce intervals (ARCHITECTURE §5 and §8).
pub const STATS_INTERVAL: Duration = Duration::from_secs(2);
pub const GPU_INTERVAL: Duration = Duration::from_secs(5);
pub const CONFIG_DEBOUNCE: Duration = Duration::from_millis(100);
pub const DISPLAY_DEBOUNCE: Duration = Duration::from_millis(500);
/// The bar is small and redraws only on change: the CPU rasteriser is cheap and saves
/// the ~48 MB a hardware device costs (DECISIONS D14).
const RASTER: Raster = Raster::Software;
/// Volume change per wheel notch, in percent points.
pub const VOLUME_STEP: i32 = 2;

/// How soon a notification that arrived while the bars were busy is handled again.
const PENDING_RETRY: Duration = Duration::from_millis(15);

/// Notifications that must not be lost. Window procedures can run while the shared state
/// is borrowed (an AppBar or COM call dispatches messages sent from other processes while
/// it waits), so they are noted here first, without borrowing, and handled once free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pending {
    /// Windows' light/dark app mode may have changed (`ImmersiveColorSet`).
    app_mode: bool,
    /// Monitors or scaling changed.
    shell: bool,
    /// `TaskbarCreated`: Explorer restarted, or Kabuk's tray asked programs to register.
    taskbar_created: bool,
}

/// Notes `m` in `pending` when it is one of the notifications that must not be lost.
fn note_pending(pending: &Cell<Pending>, m: &Msg, taskbar_created: u32) -> bool {
    let mut p = pending.get();
    if m.id == msg::SETTINGCHANGE {
        let area = kabuk_win::system::setting_change_area(m.lparam);
        if area.as_deref() != Some(kabuk_win::system::IMMERSIVE_COLOR_SET) {
            return false;
        }
        p.app_mode = true;
    } else if m.id == taskbar_created {
        p.taskbar_created = true;
    } else if m.id == msg::DISPLAYCHANGE || m.id == msg::DPICHANGED {
        p.shell = true;
    } else {
        return false;
    }
    pending.set(p);
    true
}

/// A window procedure of the bars and the dock: notes what must not be lost, then hands
/// the message over, or retries from the control window's timer when the state is busy.
fn surface_proc(
    weak: Weak<RefCell<Shared>>,
    pending: Rc<Cell<Pending>>,
    control: Option<kabuk_win::window::Hwnd>,
    taskbar_created: u32,
    on: impl Fn(&mut Shared, Msg) -> Option<isize> + 'static,
) -> impl Fn(Msg) -> Option<isize> + 'static {
    move |m| {
        if m.id == msg::MOUSEACTIVATE {
            return Some(msg::MA_NOACTIVATE);
        }
        if m.id == msg::QUERYENDSESSION {
            return Some(1);
        }
        let noted = note_pending(&pending, &m, taskbar_created);
        let rc = weak.upgrade()?;
        let Ok(mut s) = rc.try_borrow_mut() else {
            if let (true, Some(c)) = (noted, control) {
                tracing::debug!(id = m.id, "busy; notification kept for later");
                kabuk_win::msgloop::set_timer(c, timer::PENDING, PENDING_RETRY);
            }
            return None;
        };
        on(&mut s, m)
    }
}

/// Application messages.
const MSG_AUDIO: u32 = msg::APP + 1;
const MSG_MEDIA: u32 = msg::APP + 2;
/// `kabuk msg launcher`: open the launcher (posted to the control window).
pub const MSG_OPEN_LAUNCHER: u32 = msg::APP + 9;
/// `kabuk msg reload`: read `kabuk.toml` again (posted to the control window).
pub const MSG_RELOAD: u32 = msg::APP + 25;
/// The bars' AppBar callback message (tests post the shell's notifications with it).
pub const MSG_APPBAR: u32 = msg::APP + 3;
/// The window manager's workspace summary changed.
const MSG_WM: u32 = msg::APP + 4;
/// Tray calls are waiting in the queue.
const MSG_TRAY: u32 = msg::APP + 5;
/// Kabuk's tray window had fallen behind Explorer's (the taskbar was shown again); tests
/// post it too.
pub const MSG_TRAY_BEHIND: u32 = msg::APP + 7;
/// Programs are asked to register again this long after the tray fell behind, once for a
/// burst of shows.
const TRAY_ASK_AGAIN: Duration = Duration::from_millis(500);
/// How long after a tray click Kabuk watches for the program's menu, to move it beside
/// the icon when it opened far away (`kabuk_core::tray::menu_place`). A program opens it
/// while handling the click; kept short, so a menu the user opens later in the program's
/// own window is never moved.
const TRAY_MENU_WAIT: Duration = Duration::from_secs(1);
/// After asking programs to register their tray icons, Kabuk does not ask again for this
/// long: their registrations make Explorer show its taskbar, which would put Kabuk's tray
/// behind and ask again, every few seconds.
const ASK_AGAIN_QUIET: Duration = Duration::from_secs(30);

/// Whether to ask programs to register again, given when Kabuk last asked.
fn ask_again(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= ASK_AGAIN_QUIET)
}

/// Explorer's process ended (it crashed or was restarted).
const MSG_EXPLORER_GONE: u32 = msg::APP + 6;
/// Integration tests: the primary bar's client point (x in the low word, y in the high
/// word) at the centre of the tray icon owned by window `wparam`; 0 if there is none.
pub const MSG_TEST_TRAY_WHERE: u32 = msg::APP + 22;
/// Integration tests: the mouse is over the tray icon owned by window `wparam` on the
/// primary bar (real mouse moves would need the real cursor there).
pub const MSG_TEST_TRAY_HOVER: u32 = msg::APP + 23;
/// Integration tests: behave as if Explorer had ended.
pub const MSG_TEST_EXPLORER_GONE: u32 = msg::APP + 21;
/// After Explorer ended, Kabuk waits for the new Explorer to announce its taskbar
/// (`TaskbarCreated`), which it does once ready, and only while no other `Shell_TrayWnd`
/// exists. If it never does, Kabuk takes over again this long after, and as often.
pub const EXPLORER_FALLBACK: Duration = Duration::from_secs(20);
/// How long Explorer keeps showing its taskbar after auto-hide was turned on.
pub const TRAY_START_DELAY: Duration = Duration::from_secs(1);
/// How long the copies of one `TaskbarCreated` broadcast take to reach every window.
const ANNOUNCE_DEBOUNCE: Duration = Duration::from_millis(100);

struct Bar {
    window: Window,
    surface: Option<WindowSurface>,
    appbar: Option<AppBar>,
    monitor: Monitor,
    primary: bool,
    edge: Edge,
    scale: Scale,
    rect: PxRect,
    /// Reserved thickness in physical pixels.
    thickness: i32,
    /// Last drawn layout and where it was drawn (DIPs inside the window), for hit tests.
    layout: Layout,
    origin: Point,
    last: Option<BarData>,
    /// Hidden while a full-screen program (a game, a video) covers its monitor.
    aside: bool,
    /// With `bar.autohide`: how much of the bar is on screen and where it is sliding;
    /// `None` for a bar that keeps its place (and reserves it).
    peek: Option<Peek>,
}

/// An auto-hiding bar's slide: from `from` of it on screen to `to` (0 hidden, 1 shown)
/// since `since`.
#[derive(Clone, Copy, Debug)]
struct Peek {
    from: f32,
    to: f32,
    since: std::time::Instant,
}

impl Peek {
    fn hidden() -> Peek {
        Peek {
            from: 0.0,
            to: 0.0,
            since: std::time::Instant::now(),
        }
    }

    /// How much of the bar is on screen now.
    fn now(&self) -> f32 {
        let eased = crate::bar::apps::growth(self.since.elapsed().as_secs_f32(), kabuk_ui::motion::BAR_SLIDE, true);
        self.from + (self.to - self.from) * eased
    }

    fn done(&self) -> bool {
        self.since.elapsed().as_secs_f32() >= kabuk_ui::motion::BAR_SLIDE
    }
}

/// Rıhtım's dock on the primary monitor, opposite the bar.
struct Dock {
    window: Window,
    surface: Option<WindowSurface>,
    appbar: Option<AppBar>,
    monitor: Monitor,
    edge: Edge,
    scale: Scale,
    /// Reserved thickness and the strip the shell gave, in physical pixels.
    thickness: i32,
    strip: PxRect,
    rect: PxRect,
    /// The OLED shift it was last drawn with; `None` until drawn.
    drawn: Option<(i32, i32)>,
    /// What it shows and where, for clicks (DECISIONS D17).
    apps: Vec<dock::DockApp>,
    layout: Layout,
    origin: Point,
    /// With `bar.autohide` the dock hides too (D70): how much of it is on screen.
    peek: Option<Peek>,
    /// Where the mouse is along the dock, in DIPs from its middle (the last place while
    /// it leaves), and how far the tiles have grown under it (D79).
    pointer: f32,
    grow: Peek,
    /// The growth and mouse place it was last drawn with, and whether in its grown room.
    drawn_grow: (f32, f32, bool),
    /// Hidden while a full-screen program covers its monitor, as the bar is.
    aside: bool,
}

/// The tooltip window of a tray icon, kept alive while it shows (the surface goes first).
#[derive(Default)]
struct Live {
    stats: Sample,
    gpu_c: Option<u32>,
    volume: Option<audio::Volume>,
    media: MediaInfo,
    title: Option<Title>,
    time: Option<WallTime>,
    /// Workspaces are fake until the window manager (M3).
    active_ws: u32,
}

struct Shared {
    gpu: Gpu,
    /// The look as drawn now, refreshed with the configuration, Windows' app mode and the hour.
    look: Look,
    windows_light: bool,
    pending: Rc<Cell<Pending>>,
    /// Other programs' tray icons, and the calls not applied yet.
    tray: Tray,
    /// The tray's and the bar apps' pictures last kept in the store: the ones of them no
    /// longer shown are forgotten, never another part's (the app grid's, the launcher's).
    tray_images: Vec<u64>,
    tray_calls: Rc<RefCell<VecDeque<IconCall>>>,
    /// Where each drawn tray icon is on screen, for programs that ask
    /// (`Shell_NotifyIconGetRect`); written after every draw.
    tray_places: Rc<RefCell<Vec<(Icon, PxRect)>>>,
    /// Every tray icon held (hidden ones too), for `kabuk msg query tray` on the
    /// command pipe's thread; written whenever the tray changes.
    tray_list: Arc<Mutex<Vec<Icon>>>,
    /// Kabuk hides the Windows taskbar while it runs (not with `--leave-explorer`).
    hides_taskbar: bool,
    /// Tests: Windows' app mode comes from this file instead of the registry.
    app_mode_file: Option<PathBuf>,
    config: LiveConfig,
    config_file: PathBuf,
    i18n: I18n,
    state: SharedState,
    /// The window manager, for real workspaces; fake ones without it.
    wm: Option<WmLink>,
    bars: Vec<Bar>,
    dock: Option<Dock>,
    live: Live,
    sampler: Sampler,
    audio: Option<Audio>,
    /// The volume keys, taken so Windows' flyout does not show beside Kabuk's OSD.
    volume_keys: Option<kabuk_win::keyhook::VolumeKeys>,
    media: Option<Media>,
    shift_step: u64,
    taskbar_created: u32,
    /// The launcher; before the control window, which holds its shortcuts.
    launcher: Option<launcher_host::LauncherState>,
    /// The tray's overflow panel while it is open.
    tray_panel: Option<tray_panel_host::TrayPanel>,
    /// The apps module's pins, windows and order.
    apps: apps_host::AppsState,
    /// The cursor set in use (DECISIONS D20); `None`: the user's own.
    cursors_applied: Option<(PathBuf, u32)>,
    /// What the system theme's colour layer last wrote (D42); `None`: Windows' own.
    colors_applied: Option<(Option<bool>, kabuk_theme::Color)>,
    /// When the mouse on an app's icon last made the genie's renderer ready.
    genie_warmed: Option<std::time::Instant>,
    /// Counts the colour layer's turns on, so a restore still running knows it was
    /// turned on again.
    colors_turn: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Kabuk's notices and the volume OSD.
    notices: notify_host::NotifyState,
    /// The monitors in game mode (SPEC §2.4).
    game_devices: Vec<String>,
    /// Idle dimming's overlays while dimmed.
    idle: idle_host::IdleState,
    /// Touchpad swipes (DECISIONS D68).
    gestures: gesture_host::GestureState,
    /// The screen saver Kabuk set.
    saver: saver_host::SaverState,
    /// The power menu while it is open.
    power_menu: Option<power_menu_host::PowerMenu>,
    /// The shortcut card's shortcuts, and the card while it shows (DECISIONS D61).
    keys_card: keys_card_host::KeysCardState,
    /// The media card while it is open (D66).
    media_card: Option<media_card_host::MediaCard>,
    volume_panel: Option<volume_panel_host::VolumePanel>,
    app_menu: Option<app_menu_host::AppMenu>,
    app_drag: Option<AppDrag>,
    app_slides: Vec<AppSlide>,
    app_hover: AppHover,
    /// Where the window manager was last told the apps' icons are.
    icons_sent: Vec<(u64, kabuk_core::state::Rect)>,
    app_preview: Option<app_preview_host::AppPreview>,
    /// The bar whose app the previews wait for.
    preview_bar: usize,
    /// The app clicked last: its previews wait until the mouse leaves it (as Windows'
    /// taskbar; found in the user's video: a preview popped up over the window just
    /// brought back).
    preview_held: Option<usize>,
    /// Closed popups playing their exit.
    leaving: Vec<motion_host::Leaving>,
    /// The tabbed groups' strips on screen.
    tabs: tabs_host::TabsState,
    /// The overview while it is open, and its shortcuts.
    overview: overview_host::OverviewState,
    /// The grid of every app while it is open, its shortcuts, icons and wallpapers.
    app_grid: appgrid_host::AppGridState,
    /// The wallpaper Kabuk set.
    wallpaper: wallpaper_host::WallpaperState,
    /// Windows' notification count and its counter (DECISIONS D32).
    notif_count: Option<u32>,
    notif_counter: Option<kabuk_win::notifications::Counter>,
    control: Option<Window>,
    me: Weak<RefCell<Shared>>,
    /// Register global shortcuts (not in test mode).
    shortcuts: bool,
    /// The instance id: the title of windows tests look for.
    instance: String,
    /// Keeps the hidden taskbar hidden.
    taskbar_guard: Option<TaskbarGuard>,
    /// Tells when Explorer's process ends, so the tray and the taskbar can be taken again.
    explorer_watch: Option<ExitWatch>,
    /// Explorer ended and its new taskbar is not there yet.
    explorer_recovering: bool,
    /// When programs were last asked to register their tray icons.
    tray_asked: Option<std::time::Instant>,
    /// The tray icon under the mouse (bar index, icon), and its tooltip once shown. By the
    /// icon's key: programs re-adding icons move the others along the bar.
    hover: Option<(usize, IconKey)>,
    tooltip: Option<motion_host::Shown>,
    /// The auto-hiding bar (or dock) the mouse rests at, until [`REVEAL_DELAY`] passes.
    reveal: Option<Peeker>,
    /// Steps the auto-hiding bars' slides at every frame the screen shows while they
    /// slide (a timer stepped them in uneven jumps; found by the user; DECISIONS D75).
    slide_ticker: Option<kabuk_win::ticker::FrameTicker>,
    /// Steps the dock's tiles growing or settling under the mouse (D79).
    dock_ticker: Option<kabuk_win::ticker::FrameTicker>,
    /// A display change came during a full-screen program: the bars are made again once
    /// it is gone (D72).
    rebuild_after_game: bool,
    /// Watches the clicked tray icon's program for its menu, for `TRAY_MENU_WAIT`.
    tray_menu: Option<kabuk_win::events::MenuWatch>,
    /// Last, so it goes after the bars: their AppBar messages pass through it.
    tray_host: Option<TrayHost>,
}

#[derive(Debug, thiserror::Error)]
pub enum BarError {
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("window: {0}")]
    Window(std::io::Error),
}

/// Owns the bars. Dropping it removes them and gives their screen space back.
pub struct BarManager {
    shared: Rc<RefCell<Shared>>,
    _foreground: Option<ForegroundWatch>,
}

/// The look, mode, accent and text size in effect (SPEC §2.9 Appearance, §4).
struct Look {
    style: Style,
    mode: Mode,
    tokens: Tokens,
    palette: Palette,
    /// The bar's sizes: the look's, with the thickness the user set (D29).
    size: BarSize,
}

fn mode_choice(m: config::Mode) -> ModeChoice {
    match m {
        config::Mode::Dark => ModeChoice::Dark,
        config::Mode::Light => ModeChoice::Light,
        config::Mode::System => ModeChoice::System,
        config::Mode::Time => ModeChoice::Time,
    }
}

fn text_size(t: config::TextSize) -> TextSize {
    match t {
        config::TextSize::Small => TextSize::Small,
        config::TextSize::Normal => TextSize::Normal,
        config::TextSize::Large => TextSize::Large,
    }
}

/// `bar.color` as red, green and blue; `None` for the look's own.
fn bar_color(cfg: &Config) -> Option<[u8; 3]> {
    kabuk_core::config::parse_hex_color(&cfg.bar.color)
}

fn resolve_look(cfg: &Config, windows_light: bool, hour: u8) -> Look {
    let mode = mode_now(mode_choice(cfg.appearance.mode), windows_light, hour);
    let accent = (cfg.appearance.accent != config::THEME_ACCENT).then_some(cfg.appearance.accent.as_str());
    let code = cfg.appearance.look.code();
    let resolved = |m| {
        resolve(code, m, accent)
            .or_else(|| resolve("a", m, accent))
            .expect("the Ada theme is embedded")
    };
    // The colour the user chose for the bar strip (D57), painted when it is solid; over
    // the blur the tinted backdrop takes it instead.
    let bar_color = bar_color(cfg).filter(|_| cfg.bar.background == BarBackground::Solid);
    let style = Style::of(cfg.appearance.look)
        .with_text_scale(text_size(cfg.appearance.text_size).factor())
        // Over the blur the tinted backdrop stands in for the desk colour (D28).
        .with_wallpaper(wallpaper_behind_bars(cfg) && cfg.bar.background != BarBackground::Blur)
        .with_bar_color(bar_color)
        .with_solid(cfg.bar.background == BarBackground::Solid);
    let r = resolved(mode);
    if let Some(shade) = &r.accent_adapted {
        tracing::info!(shade = %shade, "accent adapted for readability");
    }
    tracing::info!(
        look = code,
        ?mode,
        accent = %r.tokens.raw("accent-s").unwrap_or_default(),
        text_scale = %style.text_scale,
        "look resolved"
    );
    // Etkinlik's bar stays black, so its text keeps the dark tokens in light mode.
    let fill = bar_color.map(|[r, g, b]| kabuk_theme::Color { r, g, b, a: 1.0 });
    let palette = if style.bar_dark {
        theme::palette(&resolved(Mode::Dark).tokens, fill)
    } else {
        theme::palette(&r.tokens, fill)
    };
    Look {
        style,
        mode,
        size: bar_size(&r.tokens, cfg.bar.height),
        tokens: r.tokens,
        palette,
    }
}

/// The frame other programs' windows get in `look` (DECISIONS D30): `--win-bg` and
/// `--fg-s`, the corners as the design draws the look's windows (Çizgi's, Konsol's and
/// Izgara's square, Defter's barely rounded).
pub fn frame_style(look: config::Look, t: &Tokens) -> kabuk_win::window::FrameStyle {
    use kabuk_win::window::{Corners, FrameStyle};
    let rgb = |name: &str, fallback: kabuk_theme::Color| {
        let c = t.color(name).unwrap_or(fallback);
        (c.r, c.g, c.b)
    };
    FrameStyle {
        caption: rgb("win-bg", kabuk_theme::Color::BLACK),
        text: rgb("fg-s", kabuk_theme::Color::WHITE),
        corners: match look {
            config::Look::Cizgi | config::Look::Konsol | config::Look::Izgara => Corners::Square,
            config::Look::Defter => Corners::RoundSmall,
            _ => Corners::Round,
        },
    }
}

/// Whether Windows' apps use the light theme; a test file says `light` or `dark` instead.
fn windows_light(test_file: Option<&std::path::Path>) -> bool {
    match test_file {
        Some(f) => std::fs::read_to_string(f).is_ok_and(|s| s.trim() == "light"),
        None => kabuk_win::system::apps_use_light_theme(),
    }
}

/// Whether a wallpaper shows behind the bars: always (SPEC §2.8). Windows' own, a picture,
/// the gradient or a scene can be anything; over Kabuk's black wallpaper the desk strip
/// of a dark look is the same black, and a light look's keeps its dark text off black.
fn wallpaper_behind_bars(_cfg: &Config) -> bool {
    true
}

fn bar_size(t: &Tokens, thickness: Option<u32>) -> BarSize {
    let set = thickness.map(|h| h as f32);
    BarSize {
        distance: t.px("edge").unwrap_or(14.0),
        height: set.or_else(|| t.px("bar-h")).unwrap_or(38.0),
        rail: set.or_else(|| t.px("rail")).unwrap_or(54.0),
    }
}

fn px_rect(r: kabuk_core::state::Rect) -> PxRect {
    PxRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

impl BarManager {
    pub fn new(
        config: Config,
        config_file: PathBuf,
        state: SharedState,
        app_mode_file: Option<PathBuf>,
        leave_explorer: bool,
        shortcuts: bool,
        instance: String,
    ) -> Result<BarManager, BarError> {
        kabuk_win::system::com_init();
        let gpu = Gpu::with(RASTER)?;
        let windows_light = windows_light(app_mode_file.as_deref());
        let look = resolve_look(&config, windows_light, kabuk_win::system::local_time().hour);
        gpu.text.set_families(theme::families(&look.tokens));
        let i18n = kabuk_win::system::i18n(Some(&config));
        let shared = Rc::new_cyclic(|me| {
            RefCell::new(Shared {
                gpu,
                look,
                windows_light,
                pending: Rc::default(),
                tray: Tray::default(),
                tray_images: Vec::new(),
                tray_calls: Rc::default(),
                tray_places: Rc::default(),
                tray_list: Arc::default(),
                hides_taskbar: !leave_explorer,
                app_mode_file,
                config: LiveConfig::new(config),
                config_file,
                i18n,
                state,
                wm: None,
                bars: Vec::new(),
                dock: None,
                live: Live {
                    active_ws: 1,
                    ..Live::default()
                },
                sampler: Sampler::default(),
                audio: None,
                volume_keys: None,
                media: None,
                shift_step: 0,
                taskbar_created: kabuk_win::system::register_message(kabuk_win::system::TASKBAR_CREATED),
                launcher: None,
                tray_panel: None,
                apps: apps_host::AppsState::default(),
                cursors_applied: None,
                colors_applied: None,
                genie_warmed: None,
                colors_turn: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                notices: notify_host::NotifyState::default(),
                game_devices: Vec::new(),
                idle: idle_host::IdleState::default(),
                gestures: gesture_host::GestureState::default(),
                saver: saver_host::SaverState::default(),
                power_menu: None,
                keys_card: Default::default(),
                media_card: None,
                volume_panel: None,
                app_menu: None,
                app_drag: None,
                app_slides: Vec::new(),
                icons_sent: Vec::new(),
                app_preview: None,
                preview_bar: 0,
                preview_held: None,
                app_hover: AppHover {
                    app: None,
                    leaving: None,
                    since: std::time::Instant::now(),
                },
                leaving: Vec::new(),
                tabs: tabs_host::TabsState::default(),
                overview: overview_host::OverviewState::default(),
                app_grid: appgrid_host::AppGridState::default(),
                wallpaper: wallpaper_host::WallpaperState::default(),
                notif_count: None,
                notif_counter: None,
                control: None,
                me: me.clone(),
                shortcuts,
                instance,
                taskbar_guard: None,
                explorer_watch: None,
                explorer_recovering: false,
                tray_asked: None,
                hover: None,
                tooltip: None,
                reveal: None,
                slide_ticker: None,
                dock_ticker: None,
                rebuild_after_game: false,
                tray_menu: None,
                tray_host: None,
            })
        });

        let weak = Rc::downgrade(&shared);
        let control = Window::new(CONTROL_CLASS, "", move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_control(m)
        })
        .map_err(BarError::Window)?;
        let notify_audio = Notify {
            window: control.hwnd(),
            message: MSG_AUDIO,
        };
        let notify_media = Notify {
            window: control.hwnd(),
            message: MSG_MEDIA,
        };
        {
            let mut s = shared.borrow_mut();
            s.audio = Audio::new(notify_audio)
                .map_err(|e| tracing::warn!(error = %e, "volume unavailable"))
                .ok();
            s.media = Some(Media::start(notify_media));
            if s.shortcuts && s.audio.is_some() {
                s.volume_keys = kabuk_win::keyhook::VolumeKeys::take(&control, MSG_VOLUME_KEY)
                    .map_err(|e| tracing::warn!(error = %e, "volume keys left to Windows"))
                    .ok();
            }
            s.live.volume = s.audio.as_ref().and_then(Audio::volume);
            let v = s.live.volume;
            s.volume_seen(v);
            s.live.title = events::foreground().and_then(|h| Title::split(&h.title()));
            s.live.stats = s.sample();
            s.live.gpu_c = kabuk_win::stats::gpu_temperature_c();
            s.live.time = Some(kabuk_win::system::local_time());
            s.notif_counter =
                kabuk_win::notifications::Counter::start(control.hwnd(), MSG_NOTIF_COUNT, MSG_NOTIF_CHANGED)
                    .map_err(|e| tracing::warn!(error = %e, "notification count unavailable"))
                    .ok();
            s.control = Some(control);
            s.count_notifications();
            s.create_launcher();
            // The app menus' names and ids, read ahead (it takes a few hundred ms).
            kabuk_win::shellitems::refresh_installed_apps();
            s.bind_overview_keys();
            s.bind_saver_keys();
            s.create_app_grid();
            s.bind_keys_card_keys();
            s.schedule_greeting();
            s.load_pins();
            s.apply_cursors();
            s.apply_system_colors();
            s.apply_saver();
            s.apply_wallpaper();
            s.schedule_idle();
            s.apply_gestures();
            s.rebuild();
            s.schedule_clock();
            s.schedule_mode_flip();
            if s.hides_taskbar {
                s.hide_taskbar();
                s.start_tray_soon();
            }
            if let Some(c) = &s.control {
                c.set_timer(timer::STATS, STATS_INTERVAL);
                c.set_timer(timer::GPU, GPU_INTERVAL);
                c.set_timer(timer::SHIFT, kabuk_core::oled::SHIFT_INTERVAL);
            }
        }

        let weak = Rc::downgrade(&shared);
        let foreground = ForegroundWatch::start(move |fg| {
            if let Some(rc) = weak.upgrade() {
                if let Ok(mut s) = rc.try_borrow_mut() {
                    tracing::trace!("foreground event");
                    // Explorer's taskbar came forward: keep programs finding Kabuk's tray.
                    if let Some(t) = &s.tray_host {
                        if !t.in_front() {
                            t.raise();
                        }
                    }
                    s.live.title = fg.and_then(|h| Title::split(&h.title()));
                    s.tray_panel_foreground(fg);
                    s.step_aside_for_fullscreen();
                    s.keep_bars_on_top();
                    s.refresh_apps();
                    s.render_all(false);
                }
            }
        })
        .map_err(|e| tracing::warn!(error = %e, "window title unavailable"))
        .ok();
        Ok(BarManager {
            shared,
            _foreground: foreground,
        })
    }

    /// The tray icons held, kept current, for `kabuk msg query tray`.
    pub fn tray_list(&self) -> Option<Arc<Mutex<Vec<Icon>>>> {
        Some(self.shared.try_borrow().ok()?.tray_list.clone())
    }

    /// The control window, where other threads post [`MSG_OPEN_LAUNCHER`], [`MSG_RELOAD`]
    /// and the hot corners reached (`crate::grab::MSG_HOT_CORNER`).
    pub fn control_window(&self) -> Option<Hwnd> {
        self.shared.try_borrow().ok()?.control.as_ref().map(Window::hwnd)
    }

    /// The configuration file changed on disk; it is re-read after a short debounce.
    pub fn config_file_changed(&self) {
        if let Ok(s) = self.shared.try_borrow() {
            if let Some(c) = &s.control {
                c.set_timer(timer::CONFIG, CONFIG_DEBOUNCE);
            }
        }
    }

    /// Shows the window manager's workspaces and sends workspace clicks to it.
    pub fn attach_wm(&self, link: WmLink) {
        if let Ok(mut s) = self.shared.try_borrow_mut() {
            if let Some(c) = &s.control {
                link.listen(c.hwnd(), MSG_WM);
            }
            link.send(wm::Command::SetAccent(s.accent()));
            link.send(wm::Command::SetFrame(s.frame_style()));
            s.wm = Some(link);
            s.render_all(false);
        }
    }

    /// The configuration in use (the last valid one).
    pub fn config(&self) -> Config {
        self.shared.borrow().config.current().clone()
    }

    /// Removes every bar now (session end, emergency).
    pub fn shutdown(&self) {
        if let Ok(mut s) = self.shared.try_borrow_mut() {
            s.dock = None;
            s.bars.clear();
        }
    }
}

impl Shared {
    fn config(&self) -> &Config {
        self.config.current()
    }

    fn schedule_clock(&self) {
        let now = kabuk_win::system::local_time();
        if let Some(c) = &self.control {
            c.set_timer(timer::CLOCK, now.until_next_minute());
        }
    }

    /// In the timed mode, wakes at the next 07:00 or 19:00.
    fn schedule_mode_flip(&self) {
        if let Some(c) = &self.control {
            if self.config().appearance.mode == config::Mode::Time {
                let now = kabuk_win::system::local_time();
                let minutes = minutes_to_next_flip(now.hour, now.minute);
                let into = Duration::from_secs(u64::from(now.second));
                c.set_timer(
                    timer::MODE,
                    Duration::from_secs(u64::from(minutes) * 60).saturating_sub(into),
                );
            } else {
                c.kill_timer(timer::MODE);
            }
        }
    }

    /// Re-resolves the look (mode, accent, text size) and redraws; rebuilds when the size changed.
    fn refresh_look(&mut self) {
        let next = resolve_look(self.config(), self.windows_light, kabuk_win::system::local_time().hour);
        let resized = next.size != self.look.size;
        let mode_changed = next.mode != self.look.mode;
        let repainted = next.style != self.look.style || next.tokens != self.look.tokens;
        self.gpu.text.set_families(theme::families(&next.tokens));
        self.look = next;
        if let Some(link) = &self.wm {
            link.send(wm::Command::SetAccent(self.accent()));
            link.send(wm::Command::SetFrame(self.frame_style()));
        }
        // The set follows the look, the mode and the accent; so does the night gradient.
        self.apply_cursors();
        self.apply_system_colors();
        self.apply_wallpaper();
        if mode_changed || repainted {
            if mode_changed {
                tracing::info!(mode = ?self.look.mode, "mode changed");
            }
            let bars: Vec<Hwnd> = self.bars.iter().map(|b| b.window.hwnd()).collect();
            for hwnd in bars {
                self.set_backdrop(hwnd);
            }
        }
        if resized {
            self.rebuild();
        } else {
            if repainted {
                self.begin_crossfade();
            }
            // App pictures follow the look's sizes (Rıhtım puts them in its dock).
            self.refresh_apps();
            self.render_all(true);
            self.redraw_tab_strips();
        }
    }

    /// The bars and the dock keep their picture above the next drawing and fade it out.
    fn begin_crossfade(&mut self) {
        if self.motion_level() == kabuk_ui::motion::Level::Off {
            return;
        }
        let seconds = kabuk_ui::motion::BLEND;
        let dock = self.dock.as_mut().and_then(|d| d.surface.as_mut());
        let surfaces = self.bars.iter_mut().filter_map(|b| b.surface.as_mut()).chain(dock);
        let faded = motion_host::crossfade(&self.gpu, surfaces, seconds);
        tracing::info!(surfaces = faded, ms = motion_host::ms(seconds), "look blends in");
        self.schedule_crossfades();
    }

    /// Drops the old pictures that have faded and waits for the rest.
    fn sweep_crossfades(&mut self) {
        self.kill(timer::CROSSFADE);
        let now = std::time::Instant::now();
        let dock = self.dock.as_mut().and_then(|d| d.surface.as_mut());
        let mut ended = false;
        for s in self.bars.iter_mut().filter_map(|b| b.surface.as_mut()).chain(dock) {
            match s.settle_crossfade(now) {
                Ok(None) => ended = true,
                Ok(Some(_)) => {}
                Err(e) => tracing::warn!(error = %e, "crossfade not ended"),
            }
        }
        if ended {
            let _ = self.gpu.commit();
        }
        self.schedule_crossfades();
    }

    fn schedule_crossfades(&mut self) {
        let now = std::time::Instant::now();
        let dock = self.dock.as_mut().and_then(|d| d.surface.as_mut());
        let next = self
            .bars
            .iter_mut()
            .filter_map(|b| b.surface.as_mut())
            .chain(dock)
            .filter_map(|s| s.settle_crossfade(now).ok().flatten())
            .min();
        if let (Some(next), Some(c)) = (next, &self.control) {
            c.set_timer(timer::CROSSFADE, next.saturating_duration_since(now));
        }
    }

    /// The active window frame's colour: the look's accent, unless turned off.
    /// Other programs' frames in the look (DECISIONS D30): the window colour and the text
    /// colour of the look's own windows, square corners where the look draws them square.
    fn frame_style(&self) -> kabuk_win::window::FrameStyle {
        frame_style(self.look.style.look, &self.look.tokens)
    }

    fn accent(&self) -> Option<(u8, u8, u8)> {
        if !self.config().windows.active_border {
            return None;
        }
        self.look.tokens.color("accent-s").map(|c| (c.r, c.g, c.b))
    }

    fn schedule_rebuild(&self) {
        if let Some(c) = &self.control {
            c.set_timer(timer::REBUILD, DISPLAY_DEBOUNCE);
        }
    }

    fn on_control(&mut self, m: Msg) -> Option<isize> {
        tracing::trace!(id = m.id, wparam = m.wparam, "control message");
        match m.id {
            MSG_NOTIF_CHANGED => {
                self.count_notifications();
                Some(0)
            }
            overview_host::MSG_OVERVIEW_FLOWN => {
                self.overview_flown();
                Some(0)
            }
            MSG_NOTIF_COUNT => {
                let count = (m.lparam == 1).then_some(u32::try_from(m.wparam).unwrap_or(u32::MAX));
                if count != self.notif_count {
                    tracing::debug!(?count, "notification count");
                    self.notif_count = count;
                    self.render_all(false);
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_OVERVIEW => {
                self.test_overview(m.wparam == 1);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_APP_GRID => {
                self.test_app_grid(m.wparam, m.lparam as usize);
                Some(0)
            }
            #[cfg(feature = "integration")]
            wallpaper_host::MSG_TEST_DRIFT => {
                self.drift_step(true);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_APP_PREVIEW => {
                self.test_app_preview(m.wparam, m.lparam as u64);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_APP_MENU => {
                self.test_app_menu(m.wparam, m.lparam as u64);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_VOLUME_PANEL => {
                if m.wparam == 0 {
                    self.close_volume_panel();
                } else {
                    let b = self.bars.iter().position(|b| b.primary)?;
                    if m.wparam == 3 {
                        self.open_system_menu(b, volume_panel::Menu::Clock, false, (view::hit::CLOCK, 0), false);
                    } else {
                        let menu = volume_panel::Menu::Volume;
                        self.open_system_menu(b, menu, m.wparam == 1, (view::hit::VOLUME, 0), false);
                    }
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_MEDIA_CARD => {
                if m.wparam == 1 {
                    let b = self.bars.iter().position(|b| b.primary)?;
                    self.open_media_card(b, false, true);
                } else {
                    self.close_media_card();
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_KEYS_CARD => {
                match m.wparam {
                    0 => self.close_keys_card(),
                    w => self.open_keys_card(w == 2, false),
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_POWER_MENU => {
                if m.wparam == 1 {
                    let b = self.bars.iter().position(|b| b.primary)?;
                    self.open_power_menu(b, false);
                } else {
                    self.close_power_menu();
                }
                Some(0)
            }
            // Raw input: keys and the mouse while dimmed, the touchpad while swipes are
            // on; Windows' own handling follows.
            MSG_INPUT => {
                self.input_seen();
                self.touchpad_input(m.lparam);
                None
            }
            #[cfg(feature = "integration")]
            MSG_TEST_SWIPE => {
                use kabuk_core::gesture::Swipe;
                let ways = [Swipe::Left, Swipe::Right, Swipe::Up, Swipe::Down];
                self.swipe(*ways.get(m.wparam)?);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_DIM => {
                self.dim();
                Some(0)
            }
            msg::TIMER => {
                match m.wparam {
                    timer::CLOCK => {
                        self.leave_if_desk_taken();
                        let now = kabuk_win::system::local_time();
                        let changed = self.live.time.is_none_or(|t| !t.same_minute(&now));
                        self.live.time = Some(now);
                        if changed {
                            self.render_all(false);
                            self.retry_saver();
                        }
                        self.schedule_clock();
                    }
                    // Not read during a game: reading the GPU's figures from its driver can
                    // make a game stutter on some machines (found on another computer:
                    // a game stuttered with Kabuk running), and the game's bar is away.
                    timer::STATS if !self.game_devices.is_empty() => {}
                    timer::GPU if !self.game_devices.is_empty() => {}
                    timer::STATS => {
                        self.live.stats = self.sample();
                        self.drop_dead_tray_icons();
                        self.render_all(false);
                    }
                    timer::GPU => {
                        if self.config().bar.modules.gpu {
                            self.live.gpu_c = kabuk_win::stats::gpu_temperature_c();
                            self.render_all(false);
                        }
                    }
                    timer::SHIFT => {
                        self.shift_step += 1;
                        self.render_all(true);
                    }
                    timer::CONFIG => {
                        self.kill(timer::CONFIG);
                        self.reload_config();
                    }
                    timer::REBUILD => self.rebuild_when_free(),
                    timer::MODE => {
                        self.refresh_look();
                        self.schedule_mode_flip();
                    }
                    timer::EXPLORER => self.explorer_back(),
                    timer::ANNOUNCED => self.explorer_announced(),
                    timer::TRAY_START => self.start_tray(),
                    timer::TOOLTIP => {
                        self.kill(timer::TOOLTIP);
                        self.show_tooltip();
                    }
                    timer::TRAY_MENU => {
                        self.kill(timer::TRAY_MENU);
                        self.tray_menu = None;
                    }
                    timer::TOAST => self.hide_toast(),
                    timer::IDLE => self.schedule_idle(),
                    timer::OSD => self.hide_osd(),
                    timer::LEAVING => self.sweep_leaving(),
                    timer::CROSSFADE => self.sweep_crossfades(),
                    timer::APP_GROW => self.step_app_growth(),
                    timer::APP_PREVIEW => self.preview_due(),
                    timer::APP_PREVIEW_HIDE => self.hide_app_preview(),
                    timer::DRIFT => self.drift_step(false),
                    timer::AUTOHIDE => self.autohide_due(),
                    timer::REVEAL => self.reveal_due(),
                    timer::RESERVE => self.reserve_due(),
                    timer::BAR_SLIDE => self.step_bar_slides(),
                    timer::SAVER_KEYS => self.saver_keys_due(),
                    timer::APP_GRID_SLIDE => self.app_grid_slide_over(),
                    timer::APP_GRID_AHEAD => self.app_grid_ahead(),
                    timer::APP_GRID_HOLD => self.app_grid_held(),
                    timer::APP_GRID_EDGE => self.app_grid_edge_due(),
                    timer::GREETING => self.greeting_due(),
                    timer::APP_GRID_WIGGLE => self.app_grid_wiggle_frame(),
                    timer::TRAY_AGAIN => {
                        self.kill(timer::TRAY_AGAIN);
                        let now = std::time::Instant::now();
                        if kabuk_win::window::popup_menu_open() {
                            // Found by the user: asked to register again, programs put their
                            // icons back and closed the tray menu open on screen (Chrome's,
                            // while a submenu was being chosen). Asked once it has closed.
                            tracing::info!("tray was behind Explorer's; a menu is open, programs asked later");
                            if let Some(c) = &self.control {
                                c.set_timer(timer::TRAY_AGAIN, TRAY_ASK_AGAIN);
                            }
                        } else if !ask_again(self.tray_asked, now) {
                            tracing::debug!("tray was behind Explorer's right after asking; not asking again");
                        } else if let Some(t) = &self.tray_host {
                            tracing::info!("tray was behind Explorer's; programs asked to register again");
                            t.explorer_ready();
                            self.tray_asked = Some(now);
                        }
                        self.forget_own_taskbar_created();
                    }
                    timer::PENDING => {
                        self.kill(timer::PENDING);
                        self.apply_pending();
                    }
                    _ => return None,
                }
                Some(0)
            }
            MSG_AUDIO => {
                if m.wparam == audio::SESSIONS_CHANGED {
                    // An app's level or session: the mixer only, no OSD.
                    self.mixer_changed();
                    return Some(0);
                }
                if m.wparam == audio::DEVICE_CHANGED {
                    if let Some(a) = self.audio.as_mut() {
                        a.reconnect();
                    }
                }
                self.live.volume = self.audio.as_ref().and_then(Audio::volume);
                let v = self.live.volume;
                self.volume_seen(v);
                self.render_all(false);
                Some(0)
            }
            MSG_WM => {
                let notices = self.wm.as_ref().map(|l| l.view().notices).unwrap_or_default();
                self.wm_notices(&notices);
                self.sync_tab_strips();
                self.refresh_apps();
                self.render_all(false);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_APP_CLICK => {
                self.test_app_click(m.wparam as u64, m.lparam == 1);
                Some(0)
            }
            msgloop::MSG_HOTKEY => (self.launcher_hotkey(m.wparam)
                || self.overview_hotkey(m.wparam)
                || self.saver_hotkey(m.wparam)
                || self.app_grid_hotkey(m.wparam)
                || self.keys_card_hotkey(m.wparam))
            .then_some(0),
            MSG_VOLUME_KEY => {
                self.volume_key(m.wparam);
                Some(0)
            }
            MSG_SLIDE_FRAME => {
                if let Some(t) = &self.slide_ticker {
                    t.taken();
                }
                self.step_bar_slides();
                Some(0)
            }
            MSG_DOCK_FRAME => {
                if let Some(t) = &self.dock_ticker {
                    t.taken();
                }
                self.render_all(false);
                self.tick_dock();
                Some(0)
            }
            MSG_START_SAVER => {
                self.start_saver_soon();
                Some(0)
            }
            crate::grab::MSG_HOT_CORNER => {
                self.hot_corner(m.wparam);
                Some(0)
            }
            launcher_host::MSG_LAUNCHER_PICTURES => {
                self.launcher_pictures_arrived();
                Some(0)
            }
            appgrid_host::MSG_APP_GRID_PICTURES => {
                self.app_grid_pictures_arrived();
                Some(0)
            }
            appgrid_host::MSG_APP_GRID_BACKDROP => {
                self.app_grid_backdrops_arrived();
                Some(0)
            }
            MSG_OPEN_APP_GRID => {
                let activate = self.shortcuts;
                self.toggle_app_grid(None, activate);
                Some(0)
            }
            launcher_host::MSG_LAUNCHER_ITEMS => {
                self.launcher_items_arrived();
                Some(0)
            }
            MSG_OPEN_LAUNCHER => {
                // With the keyboard for the user; tests never take it from the user's window.
                let activate = self.shortcuts;
                self.open_launcher(activate);
                Some(0)
            }
            MSG_OPEN_OVERVIEW => {
                let activate = self.shortcuts;
                self.toggle_overview(activate);
                Some(0)
            }
            MSG_RELOAD => {
                if let Some(c) = &self.control {
                    c.set_timer(timer::CONFIG, CONFIG_DEBOUNCE);
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_TRAY_PANEL => {
                if m.wparam == 1 {
                    let b = self.bars.iter().position(|b| b.primary)?;
                    self.open_tray_panel(b, false);
                } else {
                    self.close_tray_panel();
                }
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_TRAY_PANEL_CLICK => {
                let click = if m.lparam == 0 { Click::Left } else { Click::Right };
                self.test_panel_click(m.wparam as u64, click);
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_FOREGROUND => {
                self.tray_panel_foreground(Some(kabuk_win::window::Hwnd(m.wparam as u64)));
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_LAUNCHER => {
                if m.wparam == 1 {
                    self.open_launcher(false);
                } else {
                    self.close_launcher();
                }
                Some(0)
            }
            MSG_TRAY => {
                self.take_tray_calls();
                Some(0)
            }
            MSG_TRAY_BEHIND => {
                tracing::debug!("tray found behind Explorer's; programs asked soon");
                if let Some(c) = &self.control {
                    c.set_timer(timer::TRAY_AGAIN, TRAY_ASK_AGAIN);
                }
                Some(0)
            }
            MSG_EXPLORER_GONE => {
                self.explorer_gone();
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_TRAY_WHERE => {
                let owner = m.wparam as u64;
                let i = self.tray.visible().position(|icon| icon.hwnd == owner)?;
                let b = self.bars.iter().position(|b| b.primary)?;
                let r = self.hit_rect(b, view::hit::TRAY, u32::try_from(i).ok()?)?;
                let bar = &self.bars[b].rect;
                let (x, y) = (r.x + r.w / 2 - bar.x, r.y + r.h / 2 - bar.y);
                Some(((y as isize) << 16) | (x as isize & 0xFFFF))
            }
            #[cfg(feature = "integration")]
            MSG_TEST_TRAY_HOVER => {
                let owner = m.wparam as u64;
                let i = self.tray.visible().position(|icon| icon.hwnd == owner)?;
                let b = self.bars.iter().position(|b| b.primary)?;
                self.hovering(b, Some((view::hit::TRAY, u32::try_from(i).ok()?)));
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_EXPLORER_GONE => {
                self.explorer_gone();
                Some(0)
            }
            #[cfg(feature = "integration")]
            MSG_TEST_TRAY_CLICK => {
                let click = if m.lparam == 0 { Click::Left } else { Click::Right };
                let owner = m.wparam as u64;
                let found = self.tray.visible().position(|icon| icon.hwnd == owner);
                if let Some(i) = found {
                    // At the icon's centre on the primary bar, where a real click lands.
                    let at = self.bars.iter().position(|b| b.primary).and_then(|b| {
                        let r = self.hit_rect(b, view::hit::TRAY, u32::try_from(i).ok()?)?;
                        Some((r.x + r.w / 2, r.y + r.h / 2))
                    });
                    let (x, y) = at.unwrap_or_default();
                    self.tray_click(i, click, x, y);
                }
                Some(0)
            }
            MSG_MEDIA => {
                self.live.media = self.media.as_ref().map(Media::info).unwrap_or_default();
                self.render_all(false);
                self.media_card_changed();
                Some(0)
            }
            _ => None,
        }
    }

    /// Game mode (SPEC §2.4): a monitor a full-screen program covers (or every monitor,
    /// unless the others stay normal) hides its bar, the window manager pauses tiling
    /// there, and notices wait for one summary; each part as `[game]` says.
    /// Puts the shown bars and the dock back over the ordinary windows when one is above
    /// them: topmost as they are, they were found under ordinary windows on the user's
    /// machine, and the window motion's overlay with them, so a window coming back showed
    /// over its genie (DECISIONS D78). Looked at as the window in front changes.
    fn keep_bars_on_top(&self) {
        let windows = self
            .bars
            .iter()
            .filter(|b| !b.aside)
            .map(|b| &b.window)
            .chain(self.dock.as_ref().map(|d| &d.window));
        for w in windows {
            let h = w.hwnd();
            if h.is_visible() && h.under_ordinary_window() {
                w.raise_topmost();
                tracing::info!(hwnd = h.0, "bar raised back over the windows");
            }
        }
    }

    fn step_aside_for_fullscreen(&mut self) {
        /// How many of the front windows the check logs.
        const FRONT_LOGGED: usize = 3;
        let g = self.config().game.clone();
        let me = std::process::id();
        let windows = kabuk_win::window::front_windows(me);
        let front: Vec<(kabuk_core::state::Rect, bool)> = windows.iter().map(|(_, r, m)| (*r, *m)).collect();
        // Each monitor's rectangle now: a game in exclusive full screen may have changed
        // its resolution, and the bars keep the one they were made with until they are
        // made again (found on another computer: the game was not seen as full screen;
        // DECISIONS D72).
        let live = shell::monitors();
        let now_rect = |b: &Bar| {
            kabuk_core::fullscreen::rect_now(
                &b.monitor.device,
                live.iter().map(|m| (m.device.as_str(), m.rect)),
                b.monitor.rect,
            )
        };
        let full: Vec<bool> = self
            .bars
            .iter()
            .map(|b| g.detect && kabuk_core::fullscreen::is_fullscreen_front(front.iter().copied(), now_rect(b)))
            .collect();
        // Which window turned a monitor's game mode on, for the log (found by the user:
        // the shortcuts stopped, and nothing said which window looked full screen).
        for (b, is_full) in self.bars.iter().zip(&full) {
            if *is_full && !self.game_devices.contains(&b.monitor.device) {
                let w = kabuk_core::fullscreen::front_on(front.iter().map(|(r, _)| *r), now_rect(b))
                    .and_then(|i| windows.get(i));
                if let Some((h, r, _)) = w {
                    let exe = h
                        .pid()
                        .and_then(|p| kabuk_win::process::Process::query(p).ok())
                        .and_then(|p| p.image_name())
                        .unwrap_or_default();
                    tracing::info!(device = %b.monitor.device, hwnd = h.0, class = %h.class_name(), exe, rect = ?r, "full screen by window");
                }
            }
        }
        tracing::debug!(?full, front = ?front.iter().take(FRONT_LOGGED).collect::<Vec<_>>(), "full-screen check");
        let any = full.iter().any(|f| *f);
        let mut devices = Vec::new();
        for (bar, full) in self.bars.iter_mut().zip(full) {
            let game = full || (any && !g.other_monitors_normal);
            if game {
                devices.push(bar.monitor.device.clone());
            }
            let hide = game && g.hide_bar;
            if hide == bar.aside {
                continue;
            }
            bar.aside = hide;
            if hide {
                bar.window.hide();
                tracing::info!(device = %bar.monitor.device, "bar steps aside for a full-screen program");
            } else {
                bar.window.show();
                tracing::info!(device = %bar.monitor.device, "bar back after a full-screen program");
            }
        }
        // Rıhtım's dock steps aside with its monitor's bar (found by the user with a
        // screenshot: it stayed over a game).
        let dock_aside = g.hide_bar && self.dock.as_ref().is_some_and(|d| devices.contains(&d.monitor.device));
        if let Some(d) = self.dock.as_mut().filter(|d| d.aside != dock_aside) {
            d.aside = dock_aside;
            if dock_aside {
                d.window.hide();
                tracing::info!("dock steps aside for a full-screen program");
            } else {
                d.window.show();
                tracing::info!("dock back after a full-screen program");
            }
        }
        if devices != self.game_devices {
            let (was, now) = (!self.game_devices.is_empty(), !devices.is_empty());
            tracing::info!(?devices, "game mode");
            self.game_devices = devices.clone();
            if let Some(link) = &self.wm {
                link.send(wm::Command::GameMode(devices));
            }
            if was != now {
                self.notices_game_mode(now);
                self.apply_animated();
            }
            if !now && std::mem::take(&mut self.rebuild_after_game) {
                tracing::info!("full-screen program gone; bars made again for the display change");
                self.schedule_rebuild();
            }
        }
    }

    /// A display change settled: the bars are made again, unless a full-screen program
    /// is in front (a game that changed the resolution): new bar windows coming up over it
    /// could take it out of its exclusive full screen, so they are made once it is gone
    /// (DECISIONS D72).
    fn rebuild_when_free(&mut self) {
        self.kill(timer::REBUILD);
        self.step_aside_for_fullscreen();
        if self.game_devices.is_empty() {
            self.rebuild_after_game = false;
            self.rebuild();
        } else if !std::mem::replace(&mut self.rebuild_after_game, true) {
            tracing::info!("display changed under a full-screen program; bars made again after it");
        }
    }

    /// Reads only the figures whose modules are on.
    fn sample(&mut self) -> Sample {
        let m = &self.config.current().bar.modules;
        let wants = Wants {
            cpu: m.cpu,
            ram: m.ram,
            net: m.net,
            gpu: false,
        };
        self.sampler.sample(wants)
    }

    /// The mouse reached a hot corner (`wparam`: the action's place in
    /// [`config::HotCornerAction::ALL`]): what the user chose for it, on the monitor the
    /// mouse is on.
    fn hot_corner(&mut self, wparam: usize) {
        use config::HotCornerAction;
        let Some(action) = HotCornerAction::ALL.get(wparam).copied() else {
            return;
        };
        tracing::info!(?action, "hot corner reached");
        let activate = self.shortcuts;
        match action {
            HotCornerAction::None => {}
            HotCornerAction::AppGrid => self.toggle_app_grid(None, activate),
            HotCornerAction::Overview => self.toggle_overview(activate),
            HotCornerAction::Launcher => self.open_launcher(activate),
            HotCornerAction::ScreenSaver => self.start_saver_soon(),
        }
    }

    /// A volume key the hook took: the volume steps as Windows' would, and Kabuk's OSD
    /// shows when the change is announced.
    fn volume_key(&mut self, wparam: usize) {
        use kabuk_win::keyhook::VolumeKey;
        let (Some(audio), Some(key)) = (self.audio.as_ref(), VolumeKey::from_wparam(wparam)) else {
            return;
        };
        match key {
            VolumeKey::Up => audio.step(VOLUME_STEP),
            VolumeKey::Down => audio.step(-VOLUME_STEP),
            VolumeKey::Mute => audio.toggle_mute(),
        }
    }

    fn kill(&self, id: usize) {
        if let Some(c) = &self.control {
            c.kill_timer(id);
        }
    }

    fn reload_config(&mut self) {
        let text = match std::fs::read_to_string(&self.config_file) {
            Ok(t) => t,
            // Deleted: back to defaults.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => {
                tracing::error!(error = %e, "configuration unreadable; keeping the current one");
                return;
            }
        };
        let before = self.config().clone();
        match self.config.reload(&text) {
            Reload::Applied { warnings } => {
                for w in warnings {
                    tracing::warn!("{w}");
                }
                tracing::info!("configuration reloaded");
            }
            Reload::Rejected { errors } => {
                for e in errors {
                    tracing::error!("{e}");
                }
                tracing::error!("configuration rejected; keeping the last valid one");
                return;
            }
        }
        let after = self.config().clone();
        if after.language != before.language {
            self.i18n = kabuk_win::system::i18n(Some(&after));
        }
        self.launcher_config_changed(&before, &after);
        if before.shortcuts.overview != after.shortcuts.overview {
            self.bind_overview_keys();
        }
        if before.shortcuts.start_saver != after.shortcuts.start_saver {
            self.bind_saver_keys();
        }
        if before.shortcuts.app_grid != after.shortcuts.app_grid {
            self.bind_app_grid_keys();
        }
        if before.shortcuts.keys_card != after.shortcuts.keys_card {
            self.bind_keys_card_keys();
        }
        if before.bar.modules.taskbar != after.bar.modules.taskbar {
            self.refresh_apps();
        }
        self.schedule_mode_flip();
        if self.hides_taskbar && after.bar.modules.tray != before.bar.modules.tray {
            self.start_tray();
        }
        let layout_changed = after.bar.position != before.bar.position
            || after.bar.monitors != before.bar.monitors
            || after.bar.background != before.bar.background
            || after.bar.autohide != before.bar.autohide
            || after.appearance.look != before.appearance.look;
        if layout_changed {
            self.look = resolve_look(&after, self.windows_light, kabuk_win::system::local_time().hour);
            self.gpu.text.set_families(theme::families(&self.look.tokens));
            self.rebuild();
        } else {
            self.refresh_look();
        }
        // The cursor set follows the setting and the look (a no-op when nothing changed).
        self.apply_cursors();
        self.apply_system_colors();
        self.apply_saver();
        self.apply_wallpaper();
        self.schedule_idle();
        self.apply_gestures();
    }

    /// Recreates every bar (and Rıhtım's dock) from the current monitors and configuration.
    fn rebuild(&mut self) {
        self.dock = None;
        self.bars.clear();
        // A monitor came or went: Explorer may have made new taskbars meanwhile.
        if let Some(g) = &self.taskbar_guard {
            g.hide_all();
        }
        let cfg = self.config().clone();
        let size = self.look.size;
        let edge = cfg.bar.position.edge(cfg.appearance.look);
        let monitors = shell::monitors();
        for monitor in monitors.iter().cloned() {
            if cfg.bar.monitors == BarMonitors::Main && !monitor.primary {
                continue;
            }
            match self.create_bar(monitor, edge, &size) {
                Ok(bar) => self.bars.push(bar),
                Err(e) => tracing::error!(error = %e, "bar not created"),
            }
        }
        tracing::info!(bars = self.bars.len(), ?edge, "bars built");
        self.refresh_apps();
        if dock::shown(cfg.appearance.look) {
            if let Some(primary) = monitors.into_iter().find(|m| m.primary) {
                match self.create_dock(primary, dock::edge(edge)) {
                    Ok(d) => self.dock = Some(d),
                    Err(e) => tracing::error!(error = %e, "dock not created"),
                }
            }
        }
        self.render_all(true);
        // New bars start shown; a game already full screen keeps them away.
        self.step_aside_for_fullscreen();
    }

    /// Records the monitor's work area before a reservation changes it (CONTRIBUTING.md rule 3).
    fn record_work_area(&self, monitor: &Monitor) -> bool {
        let backup = WorkAreaBackup {
            monitor: monitor.rect,
            work_area: monitor.work_area,
        };
        let recorded = wm::lock(&self.state).update(|s| {
            if !s.work_areas.iter().any(|w| w.monitor == backup.monitor) {
                s.work_areas.push(backup);
            }
        });
        match recorded {
            Ok(()) => true,
            Err(e) => {
                // Without a backup the work area must not change.
                tracing::error!(error = %e, "work area not recorded; floating without reserving space");
                false
            }
        }
    }

    /// The dock's size in physical pixels, with room for the OLED shift on every side.
    fn dock_window_size(&self, edge: Edge, scale: Scale) -> (i32, i32) {
        let l = kabuk_ui::node::layout(
            &dock::view(edge, &dock::STYLE, &self.dock_apps()),
            Point::default(),
            &self.gpu.text,
        );
        let room = 2 * kabuk_core::oled::MAX_SHIFT;
        (
            (l.size.w * scale.0).ceil() as i32 + room,
            (l.size.h * scale.0).ceil() as i32 + room,
        )
    }

    fn create_dock(&mut self, monitor: Monitor, edge: Edge) -> std::io::Result<Dock> {
        let scale = Scale::from_dpi(monitor.dpi);
        let token = self.look.tokens.px("dock").unwrap_or(0.0);
        let thickness = dock::reserved_px(token, scale);
        let mon = px_rect(monitor.rect);
        let proc = surface_proc(
            self.me.clone(),
            self.pending.clone(),
            self.control.as_ref().map(Window::hwnd),
            self.taskbar_created,
            Shared::on_dock,
        );
        let window = Window::shell_surface(DOCK_CLASS, "Kabuk", appbar::strip(edge, mon, thickness), proc)?;
        // Auto-hiding (D70), it reserves nothing and waits off its edge as the bar does.
        let autohide = self.config().bar.autohide;
        let (appbar, strip) = if autohide {
            self.record_work_area(&monitor);
            (None, appbar::strip(edge, mon, thickness))
        } else if self.record_work_area(&monitor) {
            match AppBar::register(&window, MSG_APPBAR) {
                Ok(a) => {
                    let strip = a.place(edge, mon, thickness);
                    (Some(a), strip)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "dock space not reserved");
                    (None, appbar::strip(edge, mon, thickness))
                }
            }
        } else {
            (None, appbar::strip(edge, mon, thickness))
        };
        let distance = scale.px(dock::STYLE.distance) - kabuk_core::oled::MAX_SHIFT;
        let rect = dock::window_rect(edge, mon, strip, self.dock_window_size(edge, scale), distance);
        let peek = autohide.then(Peek::hidden);
        window.set_rect(match peek {
            Some(_) => appbar::slid_to_edge(edge, rect, mon, peek_px(scale), 0.0),
            None => rect,
        });
        window.show();
        let surface = match WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!(error = %e, "dock surface failed");
                None
            }
        };
        self.set_backdrop(window.hwnd());
        tracing::info!(device = %monitor.device, ?edge, ?strip, ?rect, "dock placed");
        Ok(Dock {
            window,
            surface,
            appbar,
            monitor,
            edge,
            scale,
            thickness,
            strip,
            rect,
            drawn: None,
            apps: Vec::new(),
            layout: Layout::default(),
            origin: Point::default(),
            peek,
            pointer: 0.0,
            grow: Peek::hidden(),
            drawn_grow: (0.0, 0.0, false),
            aside: false,
        })
    }

    /// Settings, display and shell notifications (already noted in `pending`), for any
    /// bar or the dock: `Some(result)` when `m` was one of them.
    fn on_notification(&mut self, m: &Msg) -> Option<Option<isize>> {
        let shell = m.id == self.taskbar_created || m.id == msg::DISPLAYCHANGE || m.id == msg::DPICHANGED;
        if m.id != msg::SETTINGCHANGE && !shell {
            return None;
        }
        if m.id == self.taskbar_created {
            // Two shells pass the tray back and forth with this: the older one leaves.
            self.leave_if_desk_taken();
        }
        self.apply_pending();
        Some(if shell { Some(0) } else { None })
    }

    /// Leaves (the emergency stop of this instance: its desktop comes back and its
    /// watchdog goes too) when the desk names another shell ([`kabuk_core::instance::lost_desk`]).
    fn leave_if_desk_taken(&self) {
        let Some(own) = wm::lock(&self.state).path().parent().map(std::path::Path::to_path_buf) else {
            return;
        };
        let owner = kabuk_win::desk_owner::current();
        if !kabuk_core::instance::lost_desk(&own, owner.as_deref()) {
            return;
        }
        tracing::warn!(owner = ?owner, "another shell has the desk; this one leaves");
        let instance = kabuk_core::instance::Instance::for_state_dir(&own);
        match kabuk_win::sync::NamedEvent::create_or_open(&instance.emergency_event()) {
            Ok(e) => {
                if let Err(err) = e.set() {
                    tracing::error!(error = %err, "could not stop for the other shell");
                }
            }
            Err(err) => tracing::error!(error = %err, "could not stop for the other shell"),
        }
    }

    /// Handles the notifications noted while busy, or just now.
    fn apply_pending(&mut self) {
        let p = self.pending.take();
        if p.app_mode {
            let light = windows_light(self.app_mode_file.as_deref());
            if light != self.windows_light {
                tracing::info!(light, "Windows app mode changed");
                self.windows_light = light;
                self.refresh_look();
            }
        }
        if p.taskbar_created {
            // One broadcast reaches every bar and the dock: act once for all of them.
            if let Some(c) = &self.control {
                c.set_timer(timer::ANNOUNCED, ANNOUNCE_DEBOUNCE);
            }
        }
        if p.shell {
            // Monitors or scaling changed: rebuild after things settle.
            tracing::info!("display or shell change");
            self.schedule_rebuild();
        }
    }

    /// Explorer announced its taskbar (`TaskbarCreated`, once per broadcast).
    fn explorer_announced(&mut self) {
        self.kill(timer::ANNOUNCED);
        if self.explorer_recovering {
            // The new Explorer is ready now.
            self.explorer_back();
        } else {
            // Explorer's tray is (newly) ready: AppBar registrations are gone, its taskbar
            // may show and its tray window is in front of ours.
            tracing::info!("Explorer announced its taskbar");
            if let Some(t) = &self.tray_host {
                t.explorer_ready();
                self.tray_asked = Some(std::time::Instant::now());
            }
            self.forget_own_taskbar_created();
            if self.hides_taskbar {
                self.hide_taskbar();
            }
            self.schedule_rebuild();
        }
    }

    /// Hides the Windows taskbar, recording it first (CONTRIBUTING.md rules 3 and 5): auto-hide
    /// is turned on so the hidden taskbar gives its space back, and both go back on exit.
    fn hide_taskbar(&mut self) {
        let recorded = wm::lock(&self.state).update(|s| {
            if s.taskbar.is_none() {
                s.taskbar = Some(TaskbarBackup {
                    autohide: kabuk_win::shell::taskbar_autohide(),
                });
            }
        });
        if let Err(e) = recorded {
            tracing::error!(error = %e, "taskbar not recorded; left visible");
            return;
        }
        kabuk_win::shell::set_taskbar_autohide(true);
        // Dropping the old guard first: after an Explorer restart the taskbars are new.
        self.taskbar_guard = None;
        let behind = self.control.as_ref().map(|c| (c.hwnd(), MSG_TRAY_BEHIND));
        match TaskbarGuard::start(self.tray_host.as_ref().and_then(TrayHost::hwnd), behind) {
            Ok(g) => {
                self.taskbar_guard = Some(g);
                tracing::info!("Windows taskbar hidden");
            }
            Err(e) => tracing::error!(error = %e, "taskbar not hidden"),
        }
        self.watch_explorer();
    }

    /// Starts watching the process that owns the taskbar.
    fn watch_explorer(&mut self) {
        self.explorer_watch = None;
        let (Some(pid), Some(control)) = (
            kabuk_win::shell::primary_taskbar().and_then(Hwnd::pid),
            self.control.as_ref().map(Window::hwnd),
        ) else {
            return;
        };
        match ExitWatch::start(pid, control, MSG_EXPLORER_GONE) {
            Ok(w) => self.explorer_watch = Some(w),
            Err(e) => tracing::warn!(error = %e, "Explorer not watched; its restart is noticed by TaskbarCreated only"),
        }
    }

    /// Explorer ended: give the tray window up at once (a new Explorer that finds a
    /// `Shell_TrayWnd` does not start its own properly) and wait for it to announce its
    /// taskbar.
    fn explorer_gone(&mut self) {
        tracing::warn!("Explorer ended; waiting for it to come back");
        self.explorer_recovering = true;
        self.explorer_watch = None;
        self.taskbar_guard = None;
        self.set_tray(false);
        if let Some(c) = &self.control {
            c.set_timer(timer::EXPLORER, EXPLORER_FALLBACK);
        }
    }

    /// The new Explorer is ready: take the tray, hide the taskbar, rebuild the bars. On the
    /// fallback timer without a taskbar yet, it waits for the next tick.
    fn explorer_back(&mut self) {
        if kabuk_win::shell::primary_taskbar().is_none() {
            return;
        }
        self.kill(timer::EXPLORER);
        self.explorer_recovering = false;
        tracing::info!("Explorer back");
        self.hide_taskbar();
        // The animated wallpaper's place went with Explorer's old desktop.
        self.restart_animated_wallpaper();
        self.schedule_rebuild();
        self.start_tray_soon();
    }

    /// Takes the tray once Explorer has settled after the taskbar was hidden: turning
    /// auto-hide on makes Explorer show its taskbar a few times, which puts its tray window
    /// in front of Kabuk's, and programs answering the request to register would then
    /// register with Explorer only.
    fn start_tray_soon(&self) {
        if let Some(c) = &self.control {
            c.set_timer(timer::TRAY_START, TRAY_START_DELAY);
        }
    }

    fn start_tray(&mut self) {
        self.kill(timer::TRAY_START);
        let on = self.config().bar.modules.tray;
        self.set_tray(on);
        if let Some(g) = &self.taskbar_guard {
            g.keep_in_front(self.tray_host.as_ref().and_then(TrayHost::hwnd));
        }
    }

    /// Kabuk's own `TaskbarCreated` broadcast reaches its own bars at once (same thread);
    /// that note is not Explorer's and is dropped.
    fn forget_own_taskbar_created(&self) {
        let mut p = self.pending.get();
        p.taskbar_created = false;
        self.pending.set(p);
    }

    /// Takes the tray icons (`on`) or gives them back to Explorer.
    fn set_tray(&mut self, on: bool) {
        self.set_tray_inner(on);
        self.forget_own_taskbar_created();
    }

    fn set_tray_inner(&mut self, on: bool) {
        if !on {
            if self.tray_host.take().is_some() {
                self.tray = Tray::default();
                self.publish_tray_list();
                let tray = self.tray.visible().map(|i| i.icon).collect::<Vec<_>>();
                self.forget_tray_images(tray);
                self.render_all(false);
            }
            return;
        }
        if self.tray_host.is_some() {
            return;
        }
        let Some(control) = self.control.as_ref().map(Window::hwnd) else {
            return;
        };
        let calls = self.tray_calls.clone();
        let places = self.tray_places.clone();
        let host = TrayHost::start(
            move |e| {
                let TrayEvent::Call(c) = e;
                calls.borrow_mut().push_back(c);
                let _ = kabuk_win::msgloop::post_message(
                    control,
                    Msg {
                        id: MSG_TRAY,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            },
            move |q| {
                places
                    .borrow()
                    .iter()
                    .find(|(icon, _)| q.is_about(icon))
                    .map(|(_, r)| *r)
            },
        );
        match host {
            Ok(h) => {
                // Starting asks every program to register.
                self.tray_host = Some(h);
                self.tray_asked = Some(std::time::Instant::now());
            }
            Err(e) => tracing::error!(error = %e, "tray unavailable"),
        }
    }

    /// Applies the tray calls that arrived, refreshes the icons' pictures and redraws.
    fn take_tray_calls(&mut self) {
        let calls: Vec<IconCall> = std::mem::take(&mut *self.tray_calls.borrow_mut()).into();
        let mut changed = false;
        for c in &calls {
            tracing::debug!(op = ?c.op, hwnd = c.hwnd, uid = c.uid, version = ?c.version, tip = ?c.tip, "tray call");
            changed |= self.tray.apply(c);
        }
        if changed {
            self.refresh_tray_images();
            self.render_all(false);
            self.refresh_tray_panel();
        }
        self.publish_tray_list();
    }

    /// Hands the tray icons held to the command pipe (`query tray`).
    fn publish_tray_list(&self) {
        if let Ok(mut l) = self.tray_list.lock() {
            *l = self.tray.icons().to_vec();
        }
    }

    /// Pictures for the shown icons at the primary bar's DPI; forgets the others.
    fn refresh_tray_images(&mut self) {
        let scale = self.bars.iter().find(|b| b.primary).map_or(Scale(1.0), |b| b.scale);
        let side = u32::try_from(scale.px(self.look.style.tray_icon)).unwrap_or(1).max(1);
        let keys: Vec<u64> = self.tray.visible().map(|i| i.icon).collect();
        for key in &keys {
            if !self.gpu.images.contains(*key) {
                match kabuk_win::icon::icon_pixels(*key, side) {
                    Some(px) => self.gpu.images.set(*key, side, px),
                    None => tracing::debug!(icon = key, "tray icon picture unavailable"),
                }
            }
        }
        self.forget_tray_images(keys);
    }

    /// Forgets the tray's and the bar apps' pictures no longer shown, `tray` the tray's
    /// shown now. Found by the user: this cleared the whole store, so the app grid being
    /// edited lost its blurred picture and its icons.
    fn forget_tray_images(&mut self, tray: Vec<u64>) {
        let mut kept = tray;
        kept.extend(self.app_image_keys());
        let gone: Vec<u64> = self.tray_images.iter().copied().filter(|k| !kept.contains(k)).collect();
        self.gpu.images.forget(gone);
        self.tray_images = kept;
    }

    /// Icons whose program went away without removing them.
    fn drop_dead_tray_icons(&mut self) {
        let dead: Vec<u64> = self
            .tray
            .visible()
            .map(|i| i.hwnd)
            .filter(|h| *h != 0 && !Hwnd(*h).exists())
            .collect();
        let mut changed = false;
        for h in dead {
            changed |= self.tray.remove_owner(h);
        }
        if changed {
            self.refresh_tray_images();
            self.publish_tray_list();
        }
    }

    /// The mouse is over `hit` on bar `index` (or left it): a tray icon's tooltip shows
    /// after a moment, and goes when the mouse moves elsewhere.
    /// The app under the mouse grows; the one it left shrinks back.
    /// The app under the mouse now.
    pub(super) fn app_hover_index(&self) -> Option<usize> {
        self.app_hover.app
    }

    fn hover_app(&mut self, bar: usize, app: Option<usize>) {
        if app == self.app_hover.app {
            return;
        }
        if app.is_some() {
            self.warm_genie();
        }
        self.preview_hover(bar, app);
        let now = std::time::Instant::now();
        let shown = self.app_hover.app.map(|a| self.app_growth(a));
        self.app_hover = AppHover {
            app,
            leaving: self.app_hover.app,
            // Shrinking starts from where it had grown to.
            since: now
                - std::time::Duration::from_secs_f32(APP_GROW_SECONDS * (1.0 - shown.unwrap_or(0.0)).clamp(0.0, 1.0)),
        };
        if self.motion_level() == kabuk_ui::motion::Level::Full {
            self.step_app_growth();
        }
    }

    /// How far app `i` has grown under the mouse now (nothing grows unless motion is full).
    pub(super) fn app_growth(&self, i: usize) -> f32 {
        if self.app_hover.app != Some(i) && self.app_hover.leaving != Some(i) {
            return 0.0;
        }
        if self.motion_level() != kabuk_ui::motion::Level::Full {
            return 0.0;
        }
        let elapsed = self.app_hover.since.elapsed().as_secs_f32();
        if self.app_hover.app == Some(i) {
            crate::bar::apps::growth(elapsed, APP_GROW_SECONDS, true)
        } else if self.app_hover.leaving == Some(i) {
            crate::bar::apps::growth(elapsed, APP_GROW_SECONDS, false)
        } else {
            0.0
        }
    }

    /// How far app `i` (named `id`) has its picture drawn from its place now: under the
    /// mouse while it is dragged, else where its slide has got to.
    pub(super) fn app_shift(&self, i: usize, id: &str) -> (f32, f32) {
        if let Some(d) = self.app_drag.filter(|d| d.dragging && d.app == i) {
            return d.follow;
        }
        self.app_shift_of(id)
    }

    /// Starts app `id`'s picture sliding from `from` into its place (at once without full
    /// motion); a slide it was in carries on from where it had got to.
    pub(super) fn start_slide(&mut self, id: String, from: (f32, f32)) {
        let now = self.app_shift_of(&id);
        self.app_slides.retain(|s| s.id != id);
        if self.motion_level() != kabuk_ui::motion::Level::Full || (from.0 == 0.0 && from.1 == 0.0) {
            return;
        }
        self.app_slides.push(AppSlide {
            id,
            from: (from.0 + now.0, from.1 + now.1),
            since: std::time::Instant::now(),
        });
        self.step_app_growth();
    }

    /// Where a running slide of app `id` has got to.
    fn app_shift_of(&self, id: &str) -> (f32, f32) {
        self.app_slides.iter().find(|s| s.id == id).map_or((0.0, 0.0), |s| {
            let left = crate::bar::apps::growth(s.since.elapsed().as_secs_f32(), APP_SLIDE_SECONDS, false);
            (s.from.0 * left, s.from.1 * left)
        })
    }

    /// After a drag swapped places, the apps other than `moved` slide from their old
    /// places (`before`, by [`crate::bar::apps::slide_ids`]) to their new ones (`after`).
    pub(super) fn slide_apps(&mut self, before: &[String], after: &[String], moved: &str) {
        let Some(d) = self.app_drag else { return };
        let Some((pitch, vertical)) = self.app_pitch(d.bar) else {
            return;
        };
        for (id, steps) in crate::bar::apps::slides(before, after, moved) {
            let from = if vertical {
                (0.0, steps * pitch)
            } else {
                (steps * pitch, 0.0)
            };
            self.start_slide(id, from);
        }
    }

    /// The distance between two apps' places on bar `bar` (DIPs) and whether they stand in
    /// a column (a bar on a side edge).
    fn app_pitch(&self, bar: usize) -> Option<(f32, bool)> {
        let a = self.hit_rect(bar, view::hit::APP, 0)?;
        let b = self.hit_rect(bar, view::hit::APP, 1)?;
        let scale = self.bars.get(bar)?.scale;
        let vertical = matches!(
            self.bars[bar].edge,
            kabuk_core::config::Edge::Left | kabuk_core::config::Edge::Right
        );
        let px = if vertical { b.y - a.y } else { b.x - a.x };
        Some((scale.dips(px), vertical))
    }

    /// Draws the growing or shrinking icon and the sliding pictures, again until they are
    /// done.
    fn step_app_growth(&mut self) {
        self.render_all(false);
        self.app_slides
            .retain(|s| s.since.elapsed().as_secs_f32() < APP_SLIDE_SECONDS);
        let done = self.app_hover.since.elapsed().as_secs_f32() >= APP_GROW_SECONDS && self.app_slides.is_empty();
        if done {
            self.kill(timer::APP_GROW);
            self.app_hover.leaving = None;
        } else if let Some(c) = &self.control {
            c.set_timer(
                timer::APP_GROW,
                std::time::Duration::from_millis(u64::from(APP_GROW_STEP_MS)),
            );
        }
    }

    /// The mouse moved on bar `index` while an app is held: past the slop it is dragged,
    /// and each app it passes over swaps places with it. True while dragging.
    fn app_drag_moved(&mut self, index: usize, x: i32, y: i32) -> bool {
        let Some(mut d) = self.app_drag else { return false };
        if d.bar != index {
            return false;
        }
        if !d.dragging {
            let slop = self.bars[index].scale.px(APP_DRAG_SLOP);
            if (x - d.at.0).abs() < slop && (y - d.at.1).abs() < slop {
                return false;
            }
            d.dragging = true;
            self.hide_tooltip();
            tracing::debug!(app = d.app, "app drag started");
        }
        // Off the bar, the place along it still counts.
        let (mid_x, mid_y) = (self.bars[index].rect.w / 2, self.bars[index].rect.h / 2);
        let over = [(x, y), (x, mid_y), (mid_x, y)]
            .into_iter()
            .find_map(|(px, py)| match self.hit(index, px, py) {
                Some((view::hit::APP, i)) => usize::try_from(i).ok(),
                _ => None,
            });
        if let Some(to) = over.filter(|to| *to != d.app) {
            self.app_drag = Some(d);
            self.app_dragged(d.app, to);
            d.app = to;
            self.app_hover.app = Some(to);
            self.app_hover.leaving = None;
        }
        // The picture follows the mouse along the bar, from the middle of its place.
        let bar = &self.bars[index];
        let vertical = matches!(
            bar.edge,
            kabuk_core::config::Edge::Left | kabuk_core::config::Edge::Right
        );
        if let Some(slot) = self.hit_rect(index, view::hit::APP, u32::try_from(d.app).unwrap_or(u32::MAX)) {
            let (mx, my) = (bar.rect.x + x, bar.rect.y + y);
            let (cx, cy) = (slot.x + slot.w / 2, slot.y + slot.h / 2);
            d.follow = if vertical {
                (0.0, bar.scale.dips(my - cy))
            } else {
                (bar.scale.dips(mx - cx), 0.0)
            };
        }
        self.app_drag = Some(d);
        self.render_all(false);
        true
    }

    fn hovering(&mut self, index: usize, hit: Option<(&'static str, u32)>) {
        let now = match hit {
            Some((view::hit::TRAY, i)) => usize::try_from(i)
                .ok()
                .and_then(|i| self.tray.visible().nth(i))
                .map(|icon| (index, icon.key)),
            _ => None,
        };
        if now != self.hover {
            self.hide_tooltip();
            self.hover = now;
            if now.is_some() {
                if let Some(c) = &self.control {
                    c.set_timer(timer::TOOLTIP, tooltip::TIP.delay);
                }
            }
        }
        let app = match hit {
            Some((view::hit::APP, i)) if self.app_drag.is_none() => usize::try_from(i).ok(),
            _ => None,
        };
        self.hover_app(index, app);
    }

    /// A genie may come soon (the mouse is on an app): its renderer is made ahead, at
    /// most once every [`GENIE_WARM_EVERY`].
    fn warm_genie(&mut self) {
        if self.genie_warmed.is_some_and(|t| t.elapsed() < GENIE_WARM_EVERY) {
            return;
        }
        self.genie_warmed = Some(std::time::Instant::now());
        if let Some(link) = &self.wm {
            link.send(wm::Command::WarmGenie);
        }
    }

    fn hide_tooltip(&mut self) {
        self.kill(timer::TOOLTIP);
        self.hide_app_preview();
        if let Some(t) = self.tooltip.take() {
            self.retire(Some(t));
            tracing::debug!("tray tooltip hidden");
        }
    }

    /// Where each drawn tray icon is: its place on the primary bar, or, for an icon in the
    /// overflow panel, the button that opens the panel (the panel closes before the click
    /// reaches the program, so its menu opens by that button).
    fn publish_tray_places(&self) {
        let Some(b) = self.bars.iter().position(|b| b.primary) else {
            self.tray_places.borrow_mut().clear();
            return;
        };
        let places = self
            .tray
            .visible()
            .enumerate()
            .map(|(i, icon)| {
                let rect = self
                    .hit_rect(b, view::hit::TRAY, u32::try_from(i).unwrap_or(u32::MAX))
                    .or_else(|| self.hit_rect(b, view::hit::TRAY_MORE, 0))
                    .unwrap_or(self.bars[b].rect);
                (icon.clone(), rect)
            })
            .collect();
        *self.tray_places.borrow_mut() = places;
    }

    /// Tells the window manager where each window's app icon is (the genie pours it
    /// there), when that changed: on the main monitor's bar, or Rıhtım's dock.
    fn publish_app_icons(&mut self) {
        let Some(link) = &self.wm else { return };
        let primary = self.bars.iter().position(|b| b.primary);
        // Each window's genie goes to its app's icon on the bar of its own monitor (D53).
        let devices: std::collections::HashMap<u64, String> =
            link.view().windows.into_iter().map(|w| (w.hwnd, w.device)).collect();
        let bar_of = |h: u64| {
            devices
                .get(&h)
                .and_then(|d| self.bars.iter().position(|b| &b.monitor.device == d))
                .or(primary)
        };
        let mut icons = Vec::new();
        for (i, app) in self.apps.list.iter().enumerate() {
            let v = u32::try_from(i).unwrap_or(u32::MAX);
            for h in &app.windows {
                let rect = bar_of(*h)
                    .and_then(|b| self.hit_rect(b, view::hit::APP, v))
                    .or_else(|| primary.and_then(|b| self.hit_rect(b, view::hit::APP, v)))
                    .or_else(|| self.dock_hit_rect(v));
                if let Some(r) = rect {
                    icons.push((
                        *h,
                        kabuk_core::state::Rect {
                            left: r.x,
                            top: r.y,
                            right: r.x + r.w,
                            bottom: r.y + r.h,
                        },
                    ));
                }
            }
        }
        if icons != self.icons_sent {
            link.send(wm::Command::AppIcons(icons.clone()));
            self.icons_sent = icons;
        }
    }

    /// The screen rectangle of app `value` on Rıhtım's dock.
    fn dock_hit_rect(&self, value: u32) -> Option<PxRect> {
        let d = self.dock.as_ref()?;
        let (r, _) = d
            .layout
            .hits
            .iter()
            .find(|(_, h)| h.module == dock::hit::APP && h.value == value)?;
        let s = d.scale;
        Some(PxRect {
            x: d.rect.x + s.px(r.x + d.origin.x),
            y: d.rect.y + s.px(r.y + d.origin.y),
            w: s.px(r.w),
            h: s.px(r.h),
        })
    }

    /// The screen rectangle of a hit region of bar `index` (`module`, `value`).
    fn hit_rect(&self, index: usize, module: &str, value: u32) -> Option<PxRect> {
        let bar = self.bars.get(index)?;
        let (r, _) = bar
            .layout
            .hits
            .iter()
            .find(|(_, h)| h.module == module && h.value == value)?;
        let s = bar.scale;
        Some(PxRect {
            x: bar.rect.x + s.px(r.x + bar.origin.x),
            y: bar.rect.y + s.px(r.y + bar.origin.y),
            w: s.px(r.w),
            h: s.px(r.h),
        })
    }

    fn show_tooltip(&mut self) {
        let Some((index, key)) = self.hover else { return };
        let found = self.tray.visible().enumerate().find(|(_, icon)| icon.key == key);
        let Some((i, tip)) = found.map(|(i, icon)| (i, icon.tip.clone())) else {
            return;
        };
        let Some(node) = tooltip::view(&tip, self.look.style.text_scale, &tooltip::TIP) else {
            return;
        };
        let (Some(bar), Some(icon)) = (self.bars.get(index), self.hit_rect(index, view::hit::TRAY, i as u32)) else {
            return;
        };
        let scale = bar.scale;
        let l = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = ((l.size.w * scale.0).ceil() as i32, (l.size.h * scale.0).ceil() as i32);
        let rect = tooltip::place(
            bar.edge,
            bar.rect,
            icon,
            size,
            scale.px(tooltip::TIP.gap),
            px_rect(bar.monitor.rect),
        );
        let window = match Window::shell_surface(TOOLTIP_CLASS, "", rect, |m| {
            (m.id == msg::MOUSEACTIVATE).then_some(msg::MA_NOACTIVATE)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "tooltip window failed");
                return;
            }
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "tooltip surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &l, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "tooltip render failed");
            }
        }
        let shown = motion_host::Shown::enter(
            &self.gpu,
            surface,
            window,
            (tooltip::TIP.entrance, scale),
            self.motion_level(),
        );
        let _ = self.gpu.commit();
        shown.window().show();
        tracing::debug!(tip = %tip, ?rect, "tray tooltip shown");
        self.tooltip = Some(shown);
    }

    /// Forwards a click on tray icon `index` to its program, at screen point (x, y).
    fn tray_click(&mut self, index: usize, click: Click, x: i32, y: i32) {
        let Some(icon) = self.tray.visible().nth(index).cloned() else {
            return;
        };
        let owner = Hwnd(icon.hwnd);
        if !owner.exists() {
            self.drop_dead_tray_icons();
            self.render_all(false);
            return;
        }
        if let Some(pid) = owner.pid() {
            kabuk_win::process::allow_foreground(pid);
            if click != Click::Double {
                self.watch_tray_menu(pid, x, y);
            }
        }
        for (wparam, lparam) in click_messages(&icon, click, x, y) {
            let m = Msg {
                id: icon.callback,
                wparam,
                lparam: lparam as isize,
            };
            if let Err(e) = kabuk_win::msgloop::post_message(owner, m) {
                tracing::debug!(error = %e, "tray click not delivered");
            }
        }
    }

    /// Found by the user: the Bluetooth icon (Explorer's own) asks Explorer's hidden
    /// taskbar where it is and opened its menu at the screen's corner. For a moment after
    /// a click, the program's first menu is moved beside the click (screen point x, y)
    /// when it opened far from it; submenus then open beside it by themselves.
    fn watch_tray_menu(&mut self, pid: u32, x: i32, y: i32) {
        // One watch per thread: the last one ends before the next starts.
        self.tray_menu = None;
        let Some(work) = kabuk_win::shell::monitors()
            .into_iter()
            .find(|m| m.rect.left <= x && x < m.rect.right && m.rect.top <= y && y < m.rect.bottom)
            .map(|m| m.work_area)
        else {
            return;
        };
        let mut first = true;
        let watch = kabuk_win::events::MenuWatch::start(pid, move |menu| {
            if !std::mem::take(&mut first) {
                return;
            }
            let Some(r) = menu.window_rect() else { return };
            if let Some((to_x, to_y)) = kabuk_core::tray::menu_place(r, (x, y), work) {
                let moved = menu.move_to_async(to_x, to_y);
                tracing::info!(pid, ?r, to_x, to_y, moved, "tray menu moved beside its icon");
            }
        });
        match watch {
            Ok(w) => {
                self.tray_menu = Some(w);
                if let Some(c) = &self.control {
                    c.set_timer(timer::TRAY_MENU, TRAY_MENU_WAIT);
                }
            }
            Err(e) => tracing::debug!(error = %e, "tray menu not watched"),
        }
    }

    fn on_dock(&mut self, m: Msg) -> Option<isize> {
        tracing::trace!(id = m.id, wparam = m.wparam, "dock message");
        if let Some(r) = self.on_notification(&m) {
            return r;
        }
        let d = self.dock.as_ref()?;
        match m.id {
            MSG_APPBAR => {
                if m.wparam == appbar::notify::POS_CHANGED {
                    self.reposition_dock();
                }
                Some(0)
            }
            msg::WINDOWPOSCHANGED => {
                if let Some(a) = &d.appbar {
                    a.window_pos_changed();
                }
                None
            }
            msg::ACTIVATE => {
                if let Some(a) = &d.appbar {
                    a.activated();
                }
                None
            }
            msg::MOUSEMOVE => {
                kabuk_win::msgloop::track_mouse_leave(d.window.hwnd());
                let (x, y) = msg::point(m.lparam);
                // Along the dock, from the middle of what is drawn (the dock's own).
                let pointer = match d.edge {
                    Edge::Left | Edge::Right => d.scale.dips(y) - (d.origin.y + d.layout.size.h / 2.0),
                    Edge::Top | Edge::Bottom => d.scale.dips(x) - (d.origin.x + d.layout.size.w / 2.0),
                };
                self.edge_reached(Peeker::Dock);
                self.dock_hover(Some(pointer));
                None
            }
            msg::MOUSELEAVE => {
                self.edge_left(Peeker::Dock);
                self.dock_hover(None);
                None
            }
            msg::LBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                let p = Point {
                    x: d.scale.dips(x) - d.origin.x,
                    y: d.scale.dips(y) - d.origin.y,
                };
                let hit = d.layout.hit_test(p).map(|h| (h.module, h.value));
                tracing::debug!(x, y, ?hit, "dock click");
                match hit {
                    Some((dock::hit::APP, i)) => self.app_click(
                        usize::try_from(i).unwrap_or(usize::MAX),
                        kabuk_win::events::foreground().map(|h| h.0),
                        msgloop::shift_held(),
                    ),
                    // With the keyboard for the user; tests never take it from the user's window.
                    Some((dock::hit::LAUNCHER, _)) => self.open_launcher(self.shortcuts),
                    _ => {}
                }
                Some(0)
            }
            // An app's menu, as on the bar (found by the user: a right click on the dock
            // did nothing).
            msg::RBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                let p = Point {
                    x: d.scale.dips(x) - d.origin.x,
                    y: d.scale.dips(y) - d.origin.y,
                };
                if let Some(h) = d.layout.hit_test(p).filter(|h| h.module == dock::hit::APP) {
                    let i = usize::try_from(h.value).unwrap_or(usize::MAX);
                    self.toggle_app_menu(app_menu_host::MenuHost::Dock, i);
                }
                Some(0)
            }
            // Another window of the app, as Windows' taskbar does on a middle click.
            msg::MBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                let p = Point {
                    x: d.scale.dips(x) - d.origin.x,
                    y: d.scale.dips(y) - d.origin.y,
                };
                if let Some(h) = d.layout.hit_test(p).filter(|h| h.module == dock::hit::APP) {
                    self.app_click(usize::try_from(h.value).unwrap_or(usize::MAX), None, true);
                }
                Some(0)
            }
            _ => None,
        }
    }

    fn reposition_dock(&mut self) {
        let size = match &self.dock {
            Some(d) => self.dock_window_size(d.edge, d.scale),
            None => return,
        };
        let Some(d) = self.dock.as_mut() else { return };
        let Some(appbar) = &d.appbar else { return };
        let mon = px_rect(d.monitor.rect);
        let strip = appbar.place(d.edge, mon, d.thickness);
        if strip == d.strip {
            return;
        }
        let distance = d.scale.px(dock::STYLE.distance) - kabuk_core::oled::MAX_SHIFT;
        let rect = dock::window_rect(d.edge, mon, strip, size, distance);
        tracing::info!(?strip, ?rect, "dock moved by the shell");
        d.strip = strip;
        d.rect = rect;
        d.window.set_rect(rect);
    }

    /// The mouse moved on the dock (`pointer`: along it, from its middle, DIPs) or left it
    /// (`None`): the tiles grow under it as macOS's dock's do, and settle once it leaves
    /// (asked for by the user; DECISIONS D79). Not while the dock waits off its edge.
    fn dock_hover(&mut self, pointer: Option<f32>) {
        let full = self.motion_level() == kabuk_ui::motion::Level::Full;
        let Some(d) = self.dock.as_mut() else { return };
        let waiting = d.peek.is_some_and(|p| p.to == 0.0);
        let to = if pointer.is_some() && full && !waiting {
            1.0
        } else {
            0.0
        };
        if let Some(p) = pointer {
            d.pointer = p;
        }
        if d.grow.to != to {
            d.grow = Peek {
                from: d.grow.now(),
                to,
                since: std::time::Instant::now(),
            };
        }
        self.render_all(false);
        self.tick_dock();
    }

    /// Keeps the frames coming while the dock's tiles grow or settle; once they are done
    /// the dock is drawn at rest (at its own size when they settled) and the frames stop.
    fn tick_dock(&mut self) {
        let Some(d) = self.dock.as_ref() else {
            self.dock_ticker = None;
            return;
        };
        if d.grow.done() {
            if self.dock_ticker.take().is_some() {
                self.render_all(false);
            }
            return;
        }
        if self.dock_ticker.is_none() {
            self.dock_ticker = self.control.as_ref().and_then(|c| {
                kabuk_win::ticker::FrameTicker::start(c.hwnd(), MSG_DOCK_FRAME)
                    .map_err(|e| tracing::warn!(error = %e, "the dock's tiles do not grow"))
                    .ok()
            });
        }
    }

    /// Draws the dock when it is new, the look changed (`force`), the OLED shift moved or
    /// its apps changed; it grows and shrinks with them, centred on its edge.
    fn render_dock(&mut self, force: bool, shift: (i32, i32)) -> Result<bool, RenderError> {
        let apps = self.dock_apps();
        let full = self.motion_level() == kabuk_ui::motion::Level::Full;
        let Some(d) = self.dock.as_mut() else { return Ok(false) };
        let shift = geometry::shift_toward_edge(d.edge, shift);
        // The tiles grow under the mouse with full motion only (D79); while they grow or
        // settle the dock is drawn in its grown room, else at its own size.
        let amount = if full { d.grow.now() } else { 0.0 };
        let grown = (amount, d.pointer, amount > 0.0 || (full && d.grow.to > 0.0));
        if !force && d.drawn == Some(shift) && d.apps == apps && d.drawn_grow == grown {
            return Ok(false);
        }
        let node = if grown.2 {
            let k = dock::magnifications(
                &dock::centers(&dock::STYLE, apps.len()),
                d.pointer,
                &dock::STYLE,
                amount,
            );
            let room = dock::grown_room(d.edge, &dock::STYLE, apps.len());
            dock::view_grown(d.edge, &dock::STYLE, &apps, &k, Some(room))
        } else {
            dock::view(d.edge, &dock::STYLE, &apps)
        };
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let room = kabuk_core::oled::MAX_SHIFT;
        let size = (
            (layout.size.w * d.scale.0).ceil() as i32 + 2 * room,
            (layout.size.h * d.scale.0).ceil() as i32 + 2 * room,
        );
        if size != (d.rect.w, d.rect.h) {
            let distance = d.scale.px(dock::STYLE.distance) - room;
            let rect = dock::window_rect(d.edge, px_rect(d.monitor.rect), d.strip, size, distance);
            d.rect = rect;
            d.window.set_rect(match d.peek {
                Some(p) => appbar::slid_to_edge(d.edge, rect, px_rect(d.monitor.rect), peek_px(d.scale), p.now()),
                None => rect,
            });
            if let Some(s) = d.surface.as_mut() {
                s.resize(
                    &self.gpu,
                    u32::try_from(size.0).unwrap_or(1),
                    u32::try_from(size.1).unwrap_or(1),
                )?;
            }
        }
        let Some(surface) = d.surface.as_mut() else {
            return Ok(false);
        };
        let origin = Point {
            x: d.scale.dips(room + shift.0),
            y: d.scale.dips(room + shift.1),
        };
        surface.render(
            &self.gpu,
            &layout,
            &self.look.palette,
            d.scale,
            (origin.x, origin.y),
            None,
        )?;
        tracing::debug!(apps = apps.len(), rect = ?d.rect, grown = ?grown, "dock drawn");
        d.drawn = Some(shift);
        d.drawn_grow = grown;
        d.apps = apps;
        d.layout = layout;
        d.origin = origin;
        Ok(true)
    }

    /// The blur behind a bar window when `bar.background = "blur"`, tinted with the
    /// desk colour of the mode in use (D28).
    fn set_backdrop(&self, hwnd: Hwnd) {
        let tint = (self.config().bar.background == BarBackground::Blur).then(|| {
            let alpha = (BLUR_TINT_ALPHA * f32::from(u8::MAX)).round() as u8;
            // The colour the user chose (D57), else the look's desk colour; black is the
            // desk of the dark modes.
            let (r, g, b) = match bar_color(self.config()) {
                Some([r, g, b]) => (r, g, b),
                None => self.look.tokens.color("desk").map_or((0, 0, 0), |d| (d.r, d.g, d.b)),
            };
            [r, g, b, alpha]
        });
        if hwnd.set_blur_behind(tint) {
            tracing::info!(hwnd = hwnd.0, blur = tint.is_some(), "bar backdrop set");
        } else if tint.is_some() {
            tracing::warn!("blur behind the bar unavailable");
        }
    }

    fn create_bar(&mut self, monitor: Monitor, edge: Edge, size: &BarSize) -> std::io::Result<Bar> {
        let scale = Scale::from_dpi(monitor.dpi);
        let thickness = size.reserved_px(edge, scale);
        let mon = px_rect(monitor.rect);
        let device = monitor.device.clone();
        let proc = surface_proc(
            self.me.clone(),
            self.pending.clone(),
            self.control.as_ref().map(Window::hwnd),
            self.taskbar_created,
            move |s, m| s.on_bar(&device, m),
        );
        // An auto-hiding bar reserves nothing: windows keep the whole monitor and the bar
        // slides over them when the mouse reaches its edge. It starts off its edge.
        let autohide = self.config().bar.autohide;
        let first = if autohide {
            appbar::slid(edge, appbar::strip(edge, mon, thickness), peek_px(scale), 0.0)
        } else {
            appbar::strip(edge, mon, thickness)
        };
        let window = Window::shell_surface(BAR_CLASS, "Kabuk", first, proc)?;

        let (appbar, rect) = if autohide {
            // It reserves its strip while shown (D73): the work area is recorded first.
            self.record_work_area(&monitor);
            (None, appbar::strip(edge, mon, thickness))
        } else if self.record_work_area(&monitor) {
            match AppBar::register(&window, MSG_APPBAR) {
                Ok(a) => {
                    let rect = a.place(edge, mon, thickness);
                    (Some(a), rect)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "space not reserved");
                    (None, appbar::strip(edge, mon, thickness))
                }
            }
        } else {
            (None, appbar::strip(edge, mon, thickness))
        };
        let peek = autohide.then(Peek::hidden);
        window.set_rect(if autohide {
            appbar::slid(edge, rect, peek_px(scale), 0.0)
        } else {
            rect
        });
        window.show();
        let surface = match WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!(error = %e, "bar surface failed");
                None
            }
        };
        // Every bar made gets its blur (found by the user: a bar made again for the new
        // setting had none, so blur looked as none).
        self.set_backdrop(window.hwnd());
        tracing::info!(device = %monitor.device, dpi = monitor.dpi, ?rect, "bar placed");
        Ok(Bar {
            window,
            surface,
            appbar,
            primary: monitor.primary,
            monitor,
            edge,
            scale,
            rect,
            thickness,
            layout: Layout::default(),
            origin: Point::default(),
            last: None,
            aside: false,
            peek,
        })
    }

    /// How much of auto-hiding `who` is on screen and where it slides; `None` when it
    /// keeps its place.
    fn peek_of(&self, who: Peeker) -> Option<Peek> {
        match who {
            Peeker::Bar(i) => self.bars.get(i).and_then(|b| b.peek),
            Peeker::Dock => self.dock.as_ref().and_then(|d| d.peek),
        }
    }

    /// Slides auto-hiding `who` onto the screen (`show`) or off it; nothing for one that
    /// keeps its place or already goes there.
    fn peek(&mut self, who: Peeker, show: bool) {
        if show {
            self.kill(timer::AUTOHIDE);
        }
        let full = self.motion_level() == kabuk_ui::motion::Level::Full;
        let slot = match who {
            Peeker::Bar(i) => self.bars.get_mut(i).map(|b| &mut b.peek),
            Peeker::Dock => self.dock.as_mut().map(|d| &mut d.peek),
        };
        let Some(slot) = slot else { return };
        let Some(p) = *slot else { return };
        let to = if show { 1.0 } else { 0.0 };
        if p.to == to {
            return;
        }
        *slot = Some(Peek {
            // Without full motion it is there at once.
            from: if full { p.now() } else { to },
            to,
            since: std::time::Instant::now(),
        });
        tracing::debug!(?who, show, "auto-hiding bar slides");
        self.announce(who, show);
        self.step_bar_slides();
    }

    /// Auto-hiding `who` is about to reserve its strip (`on`) or give it back: the window
    /// manager hears the work area coming first, so the windows slide with the bar
    /// instead of jumping (asked for by the user: they were pushed at once; D75), and
    /// the change follows as the slide ends: the maximised windows Windows then resizes
    /// do so under the window manager's scene, not halfway through it (found by the user:
    /// they slid "takıla takıla"). Without a window manager, or without full motion, it
    /// is at once.
    fn announce(&mut self, who: Peeker, on: bool) {
        let Some(link) = self.wm.clone() else {
            self.reserve(who, on);
            return;
        };
        let place = match who {
            Peeker::Bar(i) => self.bars.get(i).map(|b| (b.monitor.clone(), b.edge, b.thickness)),
            Peeker::Dock => self.dock.as_ref().map(|d| (d.monitor.clone(), d.edge, d.thickness)),
        };
        let Some((monitor, edge, thickness)) = place else {
            return;
        };
        if on && !self.record_work_area(&monitor) {
            return;
        }
        let work = kabuk_win::shell::monitors()
            .into_iter()
            .find(|m| m.device == monitor.device)
            .map_or(monitor.work_area, |m| m.work_area);
        let to = appbar::work_area_with(edge, px_rect(monitor.rect), px_rect(work), thickness, on);
        link.send(wm::Command::WorkAreaComing {
            device: monitor.device.clone(),
            to: kabuk_core::state::Rect {
                left: to.x,
                top: to.y,
                right: to.x + to.w,
                bottom: to.y + to.h,
            },
        });
        let lead = if self.motion_level() == kabuk_ui::motion::Level::Full {
            Duration::from_secs_f32(kabuk_ui::motion::BAR_SLIDE)
        } else {
            Duration::ZERO
        };
        if let Some(c) = &self.control {
            c.set_timer(timer::RESERVE, lead);
        }
    }

    /// The window manager's scene is up: every auto-hiding bar and the dock take or give
    /// back their strips as they slide now.
    fn reserve_due(&mut self) {
        self.kill(timer::RESERVE);
        let peekers = (0..self.bars.len())
            .map(Peeker::Bar)
            .chain(std::iter::once(Peeker::Dock));
        let due: Vec<(Peeker, bool)> = peekers
            .filter_map(|who| self.peek_of(who).map(|p| (who, p.to > 0.0)))
            .collect();
        for (who, on) in due {
            self.reserve(who, on);
        }
    }

    /// While auto-hiding `who` is shown it reserves its strip, as a bar that keeps its
    /// place does, so the windows make room instead of lying under it (asked for by the
    /// user: it came over the windows' tops; DECISIONS D73); sliding off it gives the room
    /// back. Nothing is reserved when the work area could not be recorded first.
    fn reserve(&mut self, who: Peeker, on: bool) {
        let monitor = match who {
            Peeker::Bar(i) => self.bars.get(i).map(|b| b.monitor.clone()),
            Peeker::Dock => self.dock.as_ref().map(|d| d.monitor.clone()),
        };
        let Some(monitor) = monitor else { return };
        if on && !self.record_work_area(&monitor) {
            return;
        }
        let mon = px_rect(monitor.rect);
        match who {
            Peeker::Bar(i) => {
                let Some(bar) = self.bars.get_mut(i) else { return };
                if !on {
                    if bar.appbar.take().is_some() {
                        tracing::info!(device = %bar.monitor.device, "auto-hiding bar gives its room back");
                    }
                    return;
                }
                if bar.appbar.is_some() {
                    return;
                }
                match AppBar::register(&bar.window, MSG_APPBAR) {
                    Ok(a) => {
                        bar.rect = a.place(bar.edge, mon, bar.thickness);
                        bar.appbar = Some(a);
                        tracing::info!(device = %bar.monitor.device, rect = ?bar.rect, "auto-hiding bar makes room");
                    }
                    Err(e) => tracing::warn!(error = %e, "auto-hiding bar made no room"),
                }
            }
            Peeker::Dock => {
                let Some(d) = self.dock.as_mut() else { return };
                if !on {
                    if d.appbar.take().is_some() {
                        tracing::info!("auto-hiding dock gives its room back");
                    }
                    return;
                }
                if d.appbar.is_some() {
                    return;
                }
                match AppBar::register(&d.window, MSG_APPBAR) {
                    Ok(a) => {
                        d.strip = a.place(d.edge, mon, d.thickness);
                        d.appbar = Some(a);
                        tracing::info!(strip = ?d.strip, "auto-hiding dock makes room");
                    }
                    Err(e) => tracing::warn!(error = %e, "auto-hiding dock made no room"),
                }
            }
        }
    }

    /// The mouse is on auto-hiding `who`: shown (or coming in), it stays; hidden, it
    /// comes in once the mouse has rested at its edge for [`REVEAL_DELAY`] (D70).
    fn edge_reached(&mut self, who: Peeker) {
        let Some(p) = self.peek_of(who) else { return };
        if p.to > 0.0 {
            self.peek(who, true);
        } else if self.reveal != Some(who) {
            self.reveal = Some(who);
            if let Some(c) = &self.control {
                c.set_timer(timer::REVEAL, REVEAL_DELAY);
            }
        }
    }

    /// The mouse left auto-hiding `who`: one about to come in stays away, a shown one
    /// slides off a while later.
    fn edge_left(&mut self, who: Peeker) {
        if self.reveal == Some(who) {
            self.reveal = None;
            self.kill(timer::REVEAL);
        }
        if self.peek_of(who).is_some() {
            if let Some(c) = &self.control {
                c.set_timer(timer::AUTOHIDE, AUTOHIDE_DELAY);
            }
        }
    }

    /// The mouse rested at an edge long enough: what it rests at comes in.
    fn reveal_due(&mut self) {
        self.kill(timer::REVEAL);
        if let Some(who) = self.reveal.take() {
            tracing::debug!(?who, "mouse rested at the edge");
            self.peek(who, true);
        }
    }

    /// One of the bar's or the dock's popups is open: an auto-hiding bar stays while it
    /// is (found by the user: it slid off as the mouse went into the panel it opened).
    fn bar_popup_open(&self) -> bool {
        self.volume_panel.is_some()
            || self.tray_panel.is_some()
            || self.power_menu.is_some()
            || self.app_menu.is_some()
            || self.media_card.is_some()
            || self.app_preview.is_some()
    }

    /// Moves the auto-hiding bars and dock to where their slides have got, again until
    /// they are done.
    fn step_bar_slides(&mut self) {
        let mut moving = false;
        for bar in &mut self.bars {
            let Some(p) = bar.peek else { continue };
            let px = peek_px(bar.scale);
            bar.window.set_rect(appbar::slid(bar.edge, bar.rect, px, p.now()));
            moving |= !p.done();
        }
        if let Some(d) = self.dock.as_mut() {
            if let Some(p) = d.peek {
                let mon = px_rect(d.monitor.rect);
                d.window
                    .set_rect(appbar::slid_to_edge(d.edge, d.rect, mon, peek_px(d.scale), p.now()));
                moving |= !p.done();
            }
        }
        if !moving {
            self.slide_ticker = None;
            self.kill(timer::BAR_SLIDE);
            return;
        }
        // At the screen's own pace; a timer only when no ticker can be had.
        if self.slide_ticker.is_none() {
            self.slide_ticker = self.control.as_ref().and_then(|c| {
                kabuk_win::ticker::FrameTicker::start(c.hwnd(), MSG_SLIDE_FRAME)
                    .map_err(|e| tracing::warn!(error = %e, "bar slides stepped by a timer"))
                    .ok()
            });
        }
        match (&self.control, &self.slide_ticker) {
            (Some(c), None) => c.set_timer(timer::BAR_SLIDE, BAR_SLIDE_STEP),
            _ => self.kill(timer::BAR_SLIDE),
        }
    }

    /// The mouse left an auto-hiding bar or the dock a while ago: those it is not over
    /// slide off, unless one of their popups is open.
    fn autohide_due(&mut self) {
        self.kill(timer::AUTOHIDE);
        let again = |s: &Shared| {
            if let Some(c) = &s.control {
                c.set_timer(timer::AUTOHIDE, AUTOHIDE_DELAY);
            }
        };
        if self.bar_popup_open() {
            tracing::trace!("auto-hiding bar kept for its popup");
            again(self);
            return;
        }
        let at = kabuk_win::input::cursor_pos();
        let over = |r: PxRect| at.is_some_and(|(x, y)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h);
        let mut still = false;
        let reach = |edge, rect, monitor: &Monitor| appbar::reach_to_edge(edge, rect, px_rect(monitor.rect));
        for i in 0..self.bars.len() {
            let b = &self.bars[i];
            if over(reach(b.edge, b.rect, &b.monitor)) {
                // Still on it (the window under the mouse changed): look again later.
                still = true;
            } else {
                self.peek(Peeker::Bar(i), false);
            }
        }
        if let Some(r) = self
            .dock
            .as_ref()
            .filter(|d| d.peek.is_some())
            .map(|d| reach(d.edge, d.rect, &d.monitor))
        {
            if over(r) {
                still = true;
            } else {
                self.peek(Peeker::Dock, false);
            }
        }
        if still {
            again(self);
        }
    }

    fn on_bar(&mut self, device: &str, m: Msg) -> Option<isize> {
        tracing::trace!(id = m.id, wparam = m.wparam, "bar message");
        if let Some(r) = self.on_notification(&m) {
            return r;
        }
        let index = self.bars.iter().position(|b| b.monitor.device == device)?;
        match m.id {
            MSG_APPBAR => {
                // Another AppBar (often a sibling bar) moved: ask for our place again. Only
                // a real change moves the window, so the bars settle instead of notifying
                // each other forever.
                if m.wparam == appbar::notify::POS_CHANGED {
                    self.reposition(index);
                }
                // A program went full screen or left it (the shell says which monitor
                // only loosely, so every monitor is looked at).
                if m.wparam == appbar::notify::FULLSCREEN_APP {
                    self.step_aside_for_fullscreen();
                }
                Some(0)
            }
            msg::WINDOWPOSCHANGED => {
                if let Some(a) = &self.bars[index].appbar {
                    a.window_pos_changed();
                }
                None
            }
            msg::ACTIVATE => {
                if let Some(a) = &self.bars[index].appbar {
                    a.activated();
                }
                None
            }
            msg::LBUTTONDOWN => {
                let (x, y) = msg::point(m.lparam);
                if let Some((view::hit::APP, i)) = self.hit(index, x, y) {
                    self.app_drag = Some(AppDrag {
                        bar: index,
                        app: usize::try_from(i).unwrap_or(usize::MAX),
                        at: (x, y),
                        dragging: false,
                        follow: (0.0, 0.0),
                    });
                    self.bars[index].window.capture_mouse(true);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                if let Some(d) = self.app_drag.take() {
                    self.bars[index].window.capture_mouse(false);
                    if d.dragging {
                        // Dropped: the order was kept as it changed; the picture slides
                        // from under the mouse into its place.
                        if let Some(id) = crate::bar::apps::slide_ids(&self.apps.list).get(d.app).cloned() {
                            self.start_slide(id, d.follow);
                        }
                        self.render_all(false);
                        return Some(0);
                    }
                }
                self.click(index, x, y, Click::Left);
                Some(0)
            }
            msg::RBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                self.click(index, x, y, Click::Right);
                Some(0)
            }
            // Another window of the app, as Windows' taskbar does on a middle click.
            msg::MBUTTONUP => {
                let (x, y) = msg::point(m.lparam);
                if let Some((view::hit::APP, i)) = self.hit(index, x, y) {
                    self.app_click(usize::try_from(i).unwrap_or(usize::MAX), None, true);
                }
                Some(0)
            }
            msg::LBUTTONDBLCLK => {
                let (x, y) = msg::point(m.lparam);
                self.click(index, x, y, Click::Double);
                Some(0)
            }
            msg::MOUSEMOVE => {
                kabuk_win::msgloop::track_mouse_leave(self.bars[index].window.hwnd());
                self.edge_reached(Peeker::Bar(index));
                // Away off its edge, its sliver shows nothing to hover or drag.
                if self.peek_of(Peeker::Bar(index)).is_some_and(|p| p.to == 0.0) {
                    return Some(0);
                }
                let (x, y) = msg::point(m.lparam);
                if self.app_drag_moved(index, x, y) {
                    return Some(0);
                }
                self.hovering(index, self.hit(index, x, y));
                Some(0)
            }
            msg::MOUSELEAVE => {
                self.hovering(index, None);
                self.edge_left(Peeker::Bar(index));
                Some(0)
            }
            msg::MOUSEWHEEL => {
                let (sx, sy) = msg::point(m.lparam);
                let bar = &self.bars[index];
                let (x, y) = (sx - bar.rect.x, sy - bar.rect.y);
                if self.hit(index, x, y).is_some_and(|h| h.0 == view::hit::VOLUME) {
                    let notches = msg::wheel(m.wparam) / msg::WHEEL_DELTA;
                    if let Some(a) = &self.audio {
                        a.step(notches * VOLUME_STEP);
                    }
                }
                Some(0)
            }
            msg::ENDSESSION => {
                if m.wparam != 0 {
                    tracing::info!("session ending; removing bars");
                    self.dock = None;
                    self.bars.clear();
                }
                Some(0)
            }
            _ => None,
        }
    }

    fn reposition(&mut self, index: usize) {
        let bar = &mut self.bars[index];
        let Some(appbar) = &bar.appbar else { return };
        let rect = appbar.place(bar.edge, px_rect(bar.monitor.rect), bar.thickness);
        if rect == bar.rect {
            return;
        }
        tracing::info!(device = %bar.monitor.device, ?rect, "bar moved by the shell");
        bar.window.set_rect(rect);
        bar.rect = rect;
        if let Some(surface) = bar.surface.as_mut() {
            if let Err(e) = surface.resize(&self.gpu, rect.w as u32, rect.h as u32) {
                tracing::error!(error = %e, "bar resize failed");
            }
        }
        bar.last = None;
        self.render_all(false);
    }

    fn hit(&self, index: usize, x: i32, y: i32) -> Option<(&'static str, u32)> {
        let bar = &self.bars[index];
        let p = Point {
            x: bar.scale.dips(x) - bar.origin.x,
            y: bar.scale.dips(y) - bar.origin.y,
        };
        bar.layout.hit_test(p).map(|h| (h.module, h.value))
    }

    fn click(&mut self, index: usize, x: i32, y: i32, button: Click) {
        self.hide_tooltip();
        let hit = self.hit(index, x, y);
        tracing::debug!(?button, x, y, ?hit, "bar click");
        // A click on the bar is a click elsewhere for the tray panel (its own button
        // toggles it below).
        if !matches!(hit, Some((view::hit::TRAY_MORE, _))) {
            self.close_tray_panel();
        }
        if let Some((view::hit::TRAY, i)) = hit {
            let (sx, sy) = (self.bars[index].rect.x + x, self.bars[index].rect.y + y);
            self.tray_click(usize::try_from(i).unwrap_or(usize::MAX), button, sx, sy);
            return;
        }
        if let Some((view::hit::VOLUME, _)) = hit {
            // As Windows' taskbar: the level on a left click, the menu on a right one.
            self.toggle_volume_panel(index, button == Click::Left);
            return;
        }
        if let (Click::Right, Some((view::hit::MEDIA, _))) = (button, hit) {
            self.toggle_media_card(index);
            return;
        }
        if let (Click::Right, Some((view::hit::APP, i))) = (button, hit) {
            self.toggle_app_menu(
                app_menu_host::MenuHost::Bar(index),
                usize::try_from(i).unwrap_or(usize::MAX),
            );
            return;
        }
        if button == Click::Right {
            // As Windows' taskbar: each system icon's own menu.
            let menu = match hit {
                Some((view::hit::CLOCK, _)) => Some(volume_panel::Menu::Clock),
                Some((view::hit::NOTIF, _)) => Some(volume_panel::Menu::Notifications),
                Some((view::hit::STATS, view::hit::STAT_NET)) => Some(volume_panel::Menu::Network),
                Some((view::hit::STATS, _)) => Some(volume_panel::Menu::Stats),
                _ => None,
            };
            if let (Some(menu), Some((module, value))) = (menu, hit) {
                self.toggle_system_menu(index, menu, false, (module, value));
            }
            return;
        }
        if button != Click::Left {
            return;
        }
        match hit {
            Some((view::hit::WORKSPACE, n)) => match &self.wm {
                Some(link) => link.send(wm::Command::SwitchOn {
                    device: self.bars[index].monitor.device.clone(),
                    number: n,
                }),
                None => {
                    self.live.active_ws = n;
                    self.render_all(false);
                }
            },
            Some((view::hit::TRAY_MORE, _)) => self.toggle_tray_panel(index),
            Some((view::hit::POWER, _)) => self.toggle_power_menu(index),
            Some((view::hit::APP_GRID | view::hit::APP_MORE, _)) => {
                let monitor = self.bars.get(index).map(|b| b.monitor.clone());
                let activate = self.shortcuts;
                self.toggle_app_grid(monitor, activate);
            }
            Some((view::hit::STATS, n)) => {
                // Task Manager for the figures, the network's settings for its rate.
                let target = if n == view::hit::STAT_NET {
                    "ms-settings:network-status"
                } else {
                    "taskmgr.exe"
                };
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(target) {
                        tracing::warn!(error = %e, target, "not opened");
                    }
                });
            }
            Some((view::hit::NOTIF, _)) | Some((view::hit::CLOCK, _)) => {
                // Windows' notification centre; the count is read again once it closes.
                std::thread::spawn(|| {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(NOTIFICATION_CENTRE) {
                        tracing::warn!(error = %e, "notification centre not opened");
                    }
                });
                self.count_notifications();
            }
            Some((view::hit::APP, i)) => self.app_click(
                usize::try_from(i).unwrap_or(usize::MAX),
                kabuk_win::events::foreground().map(|h| h.0),
                msgloop::shift_held(),
            ),
            Some((view::hit::MEDIA, _)) => {
                if let Some(media) = &self.media {
                    media.toggle();
                }
            }
            Some((view::hit::LAYOUT, _)) => {
                if let Some(link) = &self.wm {
                    link.send(wm::Command::ToggleFloatingLayout {
                        device: self.bars[index].monitor.device.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    fn data(&self, device: &str, primary: bool, edge: Edge) -> BarData {
        let cfg = self.config();
        let shown = cfg.windows.shown_workspaces.min(cfg.windows.workspaces).max(1);
        let shown_tray = usize::try_from(cfg.bar.tray_shown).unwrap_or(usize::MAX);
        let time = self.live.time.unwrap_or_else(kabuk_win::system::local_time);
        let style = match cfg.language.clock {
            ClockFormat::H24 => ClockStyle::H24,
            ClockFormat::H12 => ClockStyle::H12,
        };
        let wm_view = self.wm.as_ref().map(WmLink::view);
        let wm_workspaces = wm_view.as_ref().and_then(|v| v.workspaces(device));
        BarData {
            edge,
            primary,
            modules: cfg.bar.modules.clone(),
            workspaces: match wm_workspaces {
                Some(all) => view::visible_workspaces(all, shown),
                // Without the window manager (`--no-wm`) the indicator only shows clicks.
                None => (1..=shown)
                    .map(|n| Workspace {
                        number: n,
                        active: n == self.live.active_ws,
                        busy: false,
                    })
                    .collect(),
            },
            // The active workspace's layout, else (without the window manager) the default.
            floating_layout: wm_workspaces
                .and_then(|all| all.iter().find(|w| w.active))
                .map_or(cfg.windows.default_layout == WorkspaceLayout::Floating, |w| w.floating),
            title: self.live.title.clone(),
            time: self.i18n.time(time.hour, time.minute, style),
            date: self.i18n.date_short(time.weekday, time.day, time.month),
            media: self.live.media.title.clone(),
            media_playing: self.live.media.playing,
            cpu: self.live.stats.cpu,
            ram_gb: self.live.stats.ram_gib,
            gpu_c: self.live.gpu_c,
            net_mbs: self.live.stats.net_mbs,
            volume: self.live.volume.map(|v| Volume {
                percent: v.percent,
                muted: v.muted,
            }),
            tray: if primary {
                self.tray
                    .visible()
                    .take(shown_tray)
                    .map(|i| view::TrayIcon { image: i.icon })
                    .collect()
            } else {
                Vec::new()
            },
            tray_more: if primary {
                self.tray.visible().count().saturating_sub(shown_tray)
            } else {
                0
            },
            apps: self.app_icons(),
            notif_count: if primary { self.notif_count } else { None },
        }
    }

    /// Asks the counter for Windows' notification count when the bell is shown.
    fn count_notifications(&self) {
        if self.config().bar.modules.notif {
            if let Some(c) = &self.notif_counter {
                c.ask();
            }
        }
    }

    /// Redraws bars whose data changed (or all, when `force`).
    fn render_all(&mut self, force: bool) {
        let palette = self.look.palette.clone();
        let style = self.look.style;
        let size = self.look.size;
        let shift = kabuk_core::oled::shift_at(if self.config().oled.bar_shift {
            self.shift_step
        } else {
            0
        });
        let background = self.config().bar.background;
        // Devre's groups carry the solid fill themselves (D71).
        let strip = style.strip() && wallpaper_behind_bars(self.config());
        let mut lost = false;
        let mut drew = false;
        let blends = self.motion_level() != kabuk_ui::motion::Level::Off;
        let mut blending = false;
        for i in 0..self.bars.len() {
            let data = self.data(&self.bars[i].monitor.device, self.bars[i].primary, self.bars[i].edge);
            if !force && self.bars[i].last.as_ref() == Some(&data) {
                continue;
            }
            let started = std::time::Instant::now();
            let bar = &self.bars[i];
            let scale = bar.scale;
            let window = Size {
                w: scale.dips(bar.rect.w),
                h: scale.dips(bar.rect.h),
            };
            let content = if background == BarBackground::Off {
                geometry::content(bar.edge, window, &size)
            } else {
                geometry::centred(bar.edge, window, &size)
            };
            let thickness = size.thickness(bar.edge);
            let regions = view::regions(&data, &style, &self.i18n, thickness);
            let mut layout = view::compose(
                &regions,
                Size {
                    w: content.w,
                    h: content.h,
                },
                &style,
                bar.edge,
                &self.gpu.text,
            );
            let (dx, dy) = geometry::shift_toward_edge(bar.edge, shift);
            let origin = Point {
                x: content.x + scale.dips(dx),
                y: content.y + scale.dips(dy),
            };
            if strip {
                view::backdrop(&mut layout, Rect::new(-origin.x, -origin.y, window.w, window.h));
            }
            let layout_us = started.elapsed().as_micros() as u64;
            let bar = &mut self.bars[i];
            // Another workspace came forward: the old picture blends into the new one.
            let switched = blends
                && bar
                    .last
                    .as_ref()
                    .is_some_and(|l| active(&l.workspaces) != active(&data.workspaces));
            if switched {
                if let Some(s) = bar.surface.as_mut() {
                    blending |= motion_host::crossfade(&self.gpu, std::iter::once(s), kabuk_ui::motion::BLEND) > 0;
                }
            }
            // Redraw only what changed, unless the bar moved or a full redraw is asked for.
            let damage = if force || switched || bar.origin != origin || bar.last.is_none() {
                Damage::Full
            } else {
                kabuk_ui::node::damage(&bar.layout, &layout)
            };
            let dirty = match damage {
                Damage::None => None,
                Damage::Full => Some(None),
                Damage::Area(r) => Some(Some(PixelRect {
                    left: ((r.x + origin.x) * scale.0).floor() as i32,
                    top: ((r.y + origin.y) * scale.0).floor() as i32,
                    right: ((r.right() + origin.x) * scale.0).ceil() as i32,
                    bottom: ((r.bottom() + origin.y) * scale.0).ceil() as i32,
                })),
            };
            if let (Some(surface), Some(dirty)) = (bar.surface.as_mut(), dirty) {
                drew = true;
                match surface.render(&self.gpu, &layout, &palette, scale, (origin.x, origin.y), dirty) {
                    Ok(()) => {}
                    Err(RenderError::DeviceLost) => lost = true,
                    Err(e) => tracing::error!(error = %e, "bar render failed"),
                }
            }
            bar.layout = layout;
            bar.origin = origin;
            if tracing::enabled!(tracing::Level::DEBUG) {
                let (texts, icons, images) = drawn(&bar.layout);
                let total_us = started.elapsed().as_micros() as u64;
                tracing::debug!(device = %bar.monitor.device, ?texts, ?icons, ?images, strip, ?background, layout_us, total_us, "bar drawn");
            }
            bar.last = Some(data);
        }
        if blending {
            tracing::debug!("workspace blends in");
            self.schedule_crossfades();
        }
        match self.render_dock(force, shift) {
            Ok(d) => drew |= d,
            Err(RenderError::DeviceLost) => lost = true,
            Err(e) => tracing::error!(error = %e, "dock render failed"),
        }
        self.publish_app_icons();
        self.publish_tray_places();
        if drew {
            match self.gpu.commit() {
                Ok(()) => {}
                Err(RenderError::DeviceLost) => lost = true,
                Err(e) => tracing::error!(error = %e, "commit failed"),
            }
        }
        if lost {
            self.recover_device();
        }
    }

    fn recover_device(&mut self) {
        tracing::warn!("GPU device lost; recreating");
        match Gpu::with(RASTER) {
            Ok(gpu) => {
                gpu.text.set_families(theme::families(&self.look.tokens));
                self.gpu = gpu;
                for bar in &mut self.bars {
                    bar.surface = WindowSurface::new(
                        &self.gpu,
                        bar.window.hwnd().0 as isize,
                        bar.rect.w as u32,
                        bar.rect.h as u32,
                    )
                    .map_err(|e| tracing::error!(error = %e, "surface recreate failed"))
                    .ok();
                    bar.last = None;
                }
                if let Some(d) = self.dock.as_mut() {
                    d.surface =
                        WindowSurface::new(&self.gpu, d.window.hwnd().0 as isize, d.rect.w as u32, d.rect.h as u32)
                            .map_err(|e| tracing::error!(error = %e, "dock surface recreate failed"))
                            .ok();
                    d.drawn = None;
                }
                self.render_all(true);
            }
            Err(e) => tracing::error!(error = %e, "GPU unavailable"),
        }
    }
}

/// Texts and icon names of a layout, for the debug log (tests read it).
fn drawn(layout: &Layout) -> (Vec<String>, Vec<&'static str>, Vec<u64>) {
    let mut texts = Vec::new();
    let mut icons = Vec::new();
    let mut images = Vec::new();
    for d in &layout.draws {
        match d {
            Draw::Text { spec, .. } => texts.push(spec.plain()),
            Draw::Icon { spec, .. } => icons.push(spec.name),
            Draw::Image { spec, .. } => images.push(spec.key),
            Draw::Panel { .. } => {}
        }
    }
    (texts, icons, images)
}

/// How much of an auto-hiding bar stays on screen, in physical pixels (at least one).
fn peek_px(scale: Scale) -> i32 {
    ((AUTOHIDE_PEEK * scale.0).round() as i32).max(1)
}

#[cfg(test)]
mod tests {
    //! Edge cases: the app-mode area and other areas of `WM_SETTINGCHANGE`, a null area,
    //! each shell message, notes adding up until taken.
    use super::*;

    const TASKBAR_CREATED: u32 = 0xC123;

    fn setting(area: Option<&str>) -> Msg {
        // Leaked so the pointer stays valid for the test, as Windows guarantees while handling.
        let lparam = area.map_or(0, |a| {
            let w: &'static [u16] = Box::leak(a.encode_utf16().chain([0]).collect::<Vec<u16>>().into_boxed_slice());
            w.as_ptr() as isize
        });
        Msg {
            id: msg::SETTINGCHANGE,
            wparam: 0,
            lparam,
        }
    }

    /// DECISIONS D30: frames take the look's window and text colours; Çizgi and Konsol
    /// square, the others round; light mode gives a light frame.
    #[test]
    fn frames_follow_the_look() {
        use kabuk_win::window::Corners;
        let tokens = |code: &str, mode| kabuk_theme::resolve::resolve(code, mode, None).unwrap().tokens;
        let ada = frame_style(config::Look::Ada, &tokens("a", Mode::Dark));
        let c = tokens("a", Mode::Dark).color("win-bg").unwrap();
        assert_eq!((ada.caption, ada.corners), ((c.r, c.g, c.b), Corners::Round));
        assert_eq!(
            frame_style(config::Look::Cizgi, &tokens("b", Mode::Dark)).corners,
            Corners::Square
        );
        assert_eq!(
            frame_style(config::Look::Konsol, &tokens("e", Mode::Dark)).corners,
            Corners::Square
        );
        // `--radius: 0px` in Izgara, `3px` in Defter, `8px` in Pano.
        assert_eq!(
            frame_style(config::Look::Izgara, &tokens("i", Mode::Dark)).corners,
            Corners::Square
        );
        assert_eq!(
            frame_style(config::Look::Defter, &tokens("j", Mode::Dark)).corners,
            Corners::RoundSmall
        );
        assert_eq!(
            frame_style(config::Look::Pano, &tokens("h", Mode::Dark)).corners,
            Corners::Round
        );
        let light = frame_style(config::Look::Ada, &tokens("a", Mode::Light));
        let lum = |(r, g, b): (u8, u8, u8)| u32::from(r) + u32::from(g) + u32::from(b);
        assert!(lum(light.caption) > lum(ada.caption) && lum(light.text) < lum(ada.text));
    }

    /// DECISIONS D29: a set height replaces the look's thickness on every edge and keeps
    /// its distance from the screen edge.
    #[test]
    fn a_set_height_replaces_the_looks_thickness() {
        let ada = kabuk_theme::resolve::resolve("a", Mode::Dark, None).unwrap().tokens;
        let own = bar_size(&ada, None);
        assert_eq!((own.distance, own.height, own.rail), (14.0, 38.0, 54.0));
        let set = bar_size(&ada, Some(48));
        assert_eq!((set.distance, set.height, set.rail), (14.0, 48.0, 48.0));
    }

    #[test]
    fn programs_are_asked_again_only_after_a_quiet_time() {
        let t0 = std::time::Instant::now();
        assert!(ask_again(None, t0));
        assert!(!ask_again(Some(t0), t0));
        assert!(!ask_again(Some(t0), t0 + ASK_AGAIN_QUIET - Duration::from_millis(1)));
        assert!(ask_again(Some(t0), t0 + ASK_AGAIN_QUIET));
        // A clock that went backwards never asks too early.
        assert!(!ask_again(Some(t0 + Duration::from_secs(5)), t0));
    }

    #[test]
    fn only_notifications_that_matter_are_kept() {
        let p = Cell::new(Pending::default());
        assert!(!note_pending(&p, &setting(Some("Environment")), TASKBAR_CREATED));
        assert!(!note_pending(&p, &setting(None), TASKBAR_CREATED));
        assert!(!note_pending(
            &p,
            &Msg {
                id: msg::TIMER,
                wparam: 0,
                lparam: 0
            },
            TASKBAR_CREATED
        ));
        assert_eq!(p.get(), Pending::default());

        assert!(note_pending(
            &p,
            &setting(Some(kabuk_win::system::IMMERSIVE_COLOR_SET)),
            TASKBAR_CREATED
        ));
        assert_eq!(
            p.get(),
            Pending {
                app_mode: true,
                shell: false,
                taskbar_created: false,
            }
        );
        for id in [TASKBAR_CREATED, msg::DISPLAYCHANGE, msg::DPICHANGED] {
            assert!(note_pending(
                &p,
                &Msg {
                    id,
                    wparam: 0,
                    lparam: 0
                },
                TASKBAR_CREATED
            ));
        }
        assert_eq!(
            p.take(),
            Pending {
                app_mode: true,
                shell: true,
                taskbar_created: true,
            }
        );
        assert_eq!(p.get(), Pending::default());
    }
}
