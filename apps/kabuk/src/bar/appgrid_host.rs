//! The app grid on the bar thread (DECISIONS D56): every app on pages over the blurred
//! wallpaper of the monitor the user works on, as macOS's Launchpad. Two windows: under,
//! the chrome (the wallpaper, the search field, the page dots), which fades in; over it,
//! a strip of three pages that zooms in as it fades in and slides from page to page on
//! the compositor (a drag follows the mouse, the wheel, the arrows and the dots turn the
//! pages). Typing searches; Enter or a click opens an app; Escape, a click between the
//! apps or another window closes it. A press held on an app, or a right click, edits the
//! grid (asked for by the user): an app dragged goes to another place (held at a side
//! edge, the page turns), its badge hides it, Done, Escape or a click between the apps
//! ends it; the arrangement is kept in the state folder (`kabuk_core::app_grid`). The apps' icons load off the thread, as the
//! launcher's do; the wallpaper is blurred off the thread once per picture and monitor.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kabuk_core::config::shortcut::Chord;
use kabuk_i18n::Lang;
use kabuk_search::Casing;
use kabuk_ui::geom::{Point, Scale, Size};
use kabuk_ui::motion::{Level, APP_GRID, APP_GRID_BACKDROP, PAGE_SLIDE};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};
use kabuk_win::shell::Monitor;
use kabuk_win::window::Hwnd;

use super::motion_host::Shown;
use super::{timer, Shared};
use crate::appgrid::model::{self, Dir, Found, Grid, Shape, Wheel};
use crate::appgrid::{backdrop, style, view};
use crate::bar::pictures::Pictures;
use crate::launcher::item::Target;
use kabuk_core::app_grid::Arrangement;

/// Classes of the pages' window and the chrome's (tests find them by class).
pub const GRID_CLASS: &str = "Kabuk.AppGrid";
pub const CHROME_CLASS: &str = "Kabuk.AppGridChrome";
/// `kabuk msg apps` and the bar's button: open or close the grid (posted to the control
/// window).
pub const MSG_OPEN_APP_GRID: u32 = msg::APP + 44;
/// Integration tests open (`wparam` 1) the grid on the primary monitor without taking
/// the keyboard, or close it (0).
pub const MSG_TEST_APP_GRID: u32 = msg::APP + 45;
/// The picture worker loaded some apps' icons.
pub(super) const MSG_APP_GRID_PICTURES: u32 = msg::APP + 46;
/// A wallpaper was blurred.
pub(super) const MSG_APP_GRID_BACKDROP: u32 = msg::APP + 47;
/// How long the screen taking is kept after the grid closes, as the genie's renderer is.
const CAPTURE_KEPT: Duration = Duration::from_secs(60);
/// The screen behind the open grid has been blurred off the bar thread.
pub(super) const MSG_APP_GRID_SCREEN: u32 = msg::APP + 94;
/// The grid's global shortcuts take these hotkey ids (the screen saver's start at 300).
const HOTKEY_FIRST: i32 = 400;
/// How far a drag past the first or last page goes, as a share of the mouse's way.
const EDGE_GIVE: f32 = 0.3;

/// A blurred wallpaper: its file, when it was last written, and the monitor's size.
type BackdropKey = (String, u64, u32, u32);
/// Blurred wallpapers the worker finished: whose, its size in pixels and its pixels.
type Blurred = Arc<Mutex<Vec<(BackdropKey, u32, u32, Vec<u8>)>>>;
/// The screen behind the grid, blurred: its picture's key, size and pixels.
type BlurredScreen = Arc<Mutex<Option<(u64, u32, u32, Vec<u8>)>>>;

/// A blurred wallpaper kept: its key in the renderer's store, its size and pixels.
struct Backdrop {
    image: u64,
    size: (u32, u32),
    pixels: Vec<u8>,
}

#[derive(Default)]
pub(super) struct AppGridState {
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    open: Option<Open>,
    pictures: Pictures,
    /// Blurred wallpapers: their picture's key in the renderer's store, and the picture
    /// itself (the store drops pictures not its tray's when the tray changes; found
    /// trying the grid: every opening blurred the wallpaper again and faded it in late).
    backdrops: HashMap<BackdropKey, Backdrop>,
    blurring: HashSet<BackdropKey>,
    blurred: Blurred,
    /// Takes the screen's picture as the grid opens (made at start, let go during a game; D87).
    capture: Option<kabuk_win::screen_picture::ScreenCapture>,
    /// The screen behind the grid, blurred off the bar thread: its key and picture.
    screen_blurred: BlurredScreen,
}

/// A press of the left button on the pages: where (pixels), how far it went across
/// (DIPs), whether it is a drag yet, and the app (its place) it began on.
struct Press {
    at: (i32, i32),
    dx: f32,
    dragging: bool,
    app: Option<usize>,
}

/// The grid on screen.
struct Open {
    /// The pages over the chrome: their window goes first.
    sheet: Shown,
    chrome: Shown,
    rect: PxRect,
    scale: Scale,
    /// The monitor in DIPs.
    size: Size,
    grid: Grid,
    /// The pages the strip holds, left to right; the middle one is on screen.
    slots: [Option<usize>; 3],
    /// For clicks: the page on screen and the chrome, laid out at the window's origin.
    page_layout: Layout,
    chrome_layout: Layout,
    backdrop: Option<u64>,
    /// A page sliding in, and when it is there.
    sliding: Option<Instant>,
    press: Option<Press>,
    wheel: Wheel,
    /// The first half of a character outside the BMP, waiting for the second.
    surrogate: Option<u16>,
    /// The user edits the grid, and drags an app.
    editing: bool,
    drag: Option<view::Drag>,
    /// The way a dragged app held at a side edge turns the page, once its time is up.
    edge: Option<isize>,
    /// Since when the icons wiggle (editing, with animations on).
    wiggle: Option<Instant>,
    /// Apps sliding to new places: where each was, and since when (editing, with
    /// animations on; the wiggle's frames draw it).
    reflow: Reflow,
}

impl Open {
    /// Where each app of the page on screen is drawn now, mid-slide too.
    fn places_now(&self) -> HashMap<String, kabuk_ui::geom::Rect> {
        let mut now = view::places(&self.grid, self.grid.page(), self.drag, self.size);
        if let Some((from, since)) = &self.reflow {
            let t = view::eased(since.elapsed().as_secs_f32());
            for (open, r) in now.iter_mut() {
                if let Some(was) = from.get(open) {
                    *r = view::between(*was, *r, t);
                }
            }
        }
        now
    }

    /// The apps slide from where they are drawn now to their next places (when the
    /// icons wiggle: their frames draw the slide).
    fn begin_reflow(&mut self) {
        if self.wiggle.is_some() {
            self.reflow = Some((self.places_now(), Instant::now()));
        }
    }
}

/// A reflow as a frame draws it: where each app was and how far along; none once over.
type Reflow = Option<(HashMap<String, kabuk_ui::geom::Rect>, Instant)>;

fn reflow_now(reflow: &Reflow) -> Option<(&HashMap<String, kabuk_ui::geom::Rect>, f32)> {
    let (from, since) = reflow.as_ref()?;
    let elapsed = since.elapsed().as_secs_f32();
    (elapsed < style::REFLOW_SECONDS).then(|| (from, view::eased(elapsed)))
}

impl Open {
    fn width(&self) -> f32 {
        self.rect.w as f32
    }
}

/// The key a blurred wallpaper is kept under: the file, when it was written (a picture
/// written again under the same name, as Kabuk's drift does, is blurred again) and the
/// monitor's size.
fn backdrop_key(file: &str, rect: PxRect) -> BackdropKey {
    let written = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (file.to_string(), written, rect.w.max(1) as u32, rect.h.max(1) as u32)
}

fn monitor_rect(m: &Monitor) -> PxRect {
    PxRect {
        x: m.rect.left,
        y: m.rect.top,
        w: m.rect.right - m.rect.left,
        h: m.rect.bottom - m.rect.top,
    }
}

impl Shared {
    /// Starts the icon worker and blurs the monitors' wallpapers ahead, so the first
    /// opening shows them at once.
    pub(super) fn create_app_grid(&mut self) {
        if let Some(control) = self.control.as_ref().map(Window::hwnd) {
            self.app_grid.pictures = Pictures::start("appgrid", control, MSG_APP_GRID_PICTURES);
        }
        for m in kabuk_win::shell::monitors() {
            self.app_grid_backdrop(&m);
        }
        // The screen taking (a GPU device, ~150 ms to make) is made with the icons ahead,
        // not before the first bar frame (M19 P3).
        // The first pages' icons, loaded ahead off the thread (found trying the grid:
        // opened soon after Kabuk started, it showed tiles until they came); a while after
        // the start, once the apps' list has been read.
        if let Some(c) = &self.control {
            c.set_timer(timer::APP_GRID_AHEAD, Duration::from_secs(style::AHEAD_AFTER_SECONDS));
        }
        self.bind_app_grid_keys();
    }

    /// Asks for the first pages' icons and makes the screen taking (the timer after the
    /// start).
    pub(super) fn app_grid_ahead(&mut self) {
        if let Some(c) = &self.control {
            c.kill_timer(timer::APP_GRID_AHEAD);
        }
        if self.game_devices.is_empty() {
            self.app_grid_capture(true);
            // Made ahead: let go a while later too if the grid is not opened.
            if let Some(c) = &self.control {
                c.set_timer(timer::APP_GRID_CAPTURE, CAPTURE_KEPT);
            }
        }
        if let Some(primary) = kabuk_win::shell::monitors().into_iter().find(|m| m.primary) {
            let scale = primary.dpi as f32 / kabuk_layout::desk::BASE_DPI as f32;
            let size = Size {
                w: (primary.rect.right - primary.rect.left) as f32 / scale,
                h: (primary.rect.bottom - primary.rect.top) as f32 / scale,
            };
            let ahead = Shape::fit(size).per_page() * style::PAGES_AHEAD;
            let side = (style::ICON * scale).ceil() as u32;
            let sources: Vec<String> = self
                .app_grid_entries()
                .into_iter()
                .take(ahead)
                .map(|e| e.open)
                .collect();
            self.app_grid.pictures.ask(sources, side);
        }
    }

    pub(super) fn bind_app_grid_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.app_grid.clone();
        let Some(control) = self.control.as_ref() else { return };
        self.app_grid.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "app grid shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => self.app_grid.hotkeys.push(h),
                Err(e) => tracing::warn!(chord = %text, error = %e, "app grid shortcut taken by another program"),
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the grid's.
    pub(super) fn app_grid_hotkey(&mut self, id: usize) -> bool {
        let first = HOTKEY_FIRST as usize;
        let ours = (first..first + self.app_grid.hotkeys.len()).contains(&id);
        if ours {
            self.toggle_app_grid(None, true);
        }
        ours
    }

    /// Opens the grid on `monitor` (the one the user works on when `None`), or closes it.
    pub(super) fn toggle_app_grid(&mut self, monitor: Option<Monitor>, activate: bool) {
        if self.app_grid.open.is_some() {
            self.close_app_grid();
        } else {
            self.open_app_grid(monitor, activate);
        }
    }

    /// The monitor's blurred wallpaper: its key when it is ready; otherwise the worker is
    /// asked for it (once) and `None` is the answer for now.
    fn app_grid_backdrop(&mut self, m: &Monitor) -> Option<u64> {
        let rect = monitor_rect(m);
        let file = kabuk_win::wallpaper::files()
            .into_iter()
            .find(|(r, _)| (r.left, r.top, r.right, r.bottom) == (m.rect.left, m.rect.top, m.rect.right, m.rect.bottom))
            .and_then(|(_, f)| f)?;
        let key = backdrop_key(&file, rect);
        if let Some(b) = self.app_grid.backdrops.get(&key) {
            if !self.gpu.images.contains(b.image) {
                self.gpu.images.set_sized(b.image, b.size.0, b.size.1, b.pixels.clone());
            }
            return Some(b.image);
        }
        let control = self.control.as_ref().map(Window::hwnd)?;
        if !self.app_grid.blurring.insert(key.clone()) {
            return None;
        }
        let out = self.app_grid.blurred.clone();
        let spawned = std::thread::Builder::new()
            .name("appgrid-backdrop".into())
            .spawn(move || blur_worker(key, control, out));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "app grid wallpaper not blurred");
        }
        None
    }

    /// Makes the screen taking (a GPU device) or lets it go: let go while a game is in
    /// front, so no device of Kabuk's lives beside it (D87).
    pub(super) fn app_grid_capture(&mut self, on: bool) {
        if !on {
            if self.app_grid.capture.take().is_some() {
                tracing::info!("app grid screen taking let go for the game");
            }
            return;
        }
        if self.app_grid.capture.is_some() {
            return;
        }
        self.app_grid.capture = kabuk_win::screen_picture::ScreenCapture::new();
        if self.app_grid.capture.is_some() {
            tracing::info!("app grid screen taking made");
        } else {
            tracing::warn!("the screen cannot be taken; the app grid shows the wallpaper under it");
        }
    }

    /// The grid stayed closed a while: the screen taking goes, made again at the next
    /// opening.
    pub(super) fn app_grid_capture_idle(&mut self) {
        if self.app_grid.open.is_none() && self.app_grid.capture.take().is_some() {
            tracing::info!("app grid screen taking let go while closed");
        }
    }

    /// The monitor's screen as it is now, small, blurred and darkened: the grid's
    /// background (found by the user: over a black wallpaper the grid's was black).
    fn app_grid_screen(&mut self, m: &Monitor) -> Option<u64> {
        let rect = monitor_rect(m);
        let (w, h) = (
            (rect.w.max(1) as u32 / style::BLUR_SHRINK).max(1),
            (rect.h.max(1) as u32 / style::BLUR_SHRINK).max(1),
        );
        // Opened before the timer made it: made now (never beside a game, D87).
        if self.game_devices.is_empty() {
            self.app_grid_capture(true);
        }
        let wait = Duration::from_millis(style::SCREEN_WAIT_MS);
        let started = Instant::now();
        let bgr = self.app_grid.capture.as_ref()?.picture(m.handle, w, h, wait);
        tracing::debug!(
            ms = started.elapsed().as_millis() as u64,
            taken = bgr.is_some(),
            "app grid screen taken"
        );
        let bgr = bgr?;
        // Blurred off the bar thread (PLAN M19 P14: it held the grid's opening): the grid
        // opens over the blurred wallpaper and the screen fades in once it is ready.
        let key = crate::bar::apps::image_key(&format!("appgrid-screen|{}", m.device));
        let control = self.control.as_ref().map(Window::hwnd)?;
        let out = self.app_grid.screen_blurred.clone();
        let spawned = std::thread::Builder::new().name("appgrid-screen".into()).spawn(move || {
            kabuk_win::system::background_thread();
            let started = Instant::now();
            let px = backdrop::blurred(
                &bgr,
                w as usize,
                h as usize,
                style::BLUR_RADIUS,
                style::BLUR_PASSES,
                style::SCREEN_DIM,
            );
            tracing::debug!(ms = started.elapsed().as_millis() as u64, "app grid screen blurred");
            if let Ok(mut o) = out.lock() {
                *o = Some((key, w, h, px));
            }
            let _ = msgloop::post_message(
                control,
                Msg {
                    id: MSG_APP_GRID_SCREEN,
                    wparam: 0,
                    lparam: 0,
                },
            );
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "app grid screen not blurred");
        }
        None
    }

    /// The screen behind the open grid came blurred: it fades in over the wallpaper.
    pub(super) fn app_grid_screen_arrived(&mut self) {
        let Some((key, w, h, px)) = self.app_grid.screen_blurred.lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        self.gpu.images.set_sized(key, w, h, px);
        let Some(o) = self.app_grid.open.as_mut() else { return };
        o.backdrop = Some(key);
        if let Some(s) = o.chrome.surface_mut() {
            if let Err(e) = s.crossfade(&self.gpu, kabuk_ui::motion::BLEND) {
                tracing::debug!(error = %e, "app grid screen not blended in");
            }
        }
        self.render_app_grid(true, false);
    }

    /// Blurred wallpapers arrived: into the store, and an open grid fades its in.
    pub(super) fn app_grid_backdrops_arrived(&mut self) {
        let done: Vec<_> = self
            .app_grid
            .blurred
            .lock()
            .map(|mut d| std::mem::take(&mut *d))
            .unwrap_or_default();
        for (key, w, h, px) in done {
            self.app_grid.blurring.remove(&key);
            let image =
                crate::bar::apps::image_key(&format!("appgrid-backdrop|{}|{}|{}x{}", key.0, key.1, key.2, key.3));
            self.gpu.images.set_sized(image, w, h, px.clone());
            let size = (key.2, key.3);
            self.app_grid.backdrops.insert(
                key,
                Backdrop {
                    image,
                    size: (w, h),
                    pixels: px,
                },
            );
            let Some(o) = self.app_grid.open.as_mut() else { continue };
            if o.backdrop.is_none() && (o.rect.w as u32, o.rect.h as u32) == size {
                o.backdrop = Some(image);
                if let Some(s) = o.chrome.surface_mut() {
                    if let Err(e) = s.crossfade(&self.gpu, kabuk_ui::motion::BLEND) {
                        tracing::debug!(error = %e, "app grid wallpaper not blended in");
                    }
                }
                self.render_app_grid(true, false);
            }
        }
    }

    fn app_grid_casing(&self) -> Casing {
        if self.i18n.lang() == Lang::Tr {
            Casing::Turkish
        } else {
            Casing::Default
        }
    }

    /// The user's arrangement of the grid; none (alphabetical, nothing hidden) without
    /// its file.
    fn app_grid_arrangement(&self) -> Arrangement {
        self.app_grid_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| Arrangement::read(&t))
            .unwrap_or_default()
    }

    fn app_grid_file(&self) -> Option<std::path::PathBuf> {
        Some(self.state.path().parent()?.join(kabuk_core::app_grid::FILE))
    }

    fn save_app_grid_arrangement(&self, a: &Arrangement) {
        let Some(file) = self.app_grid_file() else { return };
        if let Err(e) = kabuk_core::fsutil::write_atomic(&file, a.to_json().as_bytes()) {
            tracing::error!(error = %e, "app grid arrangement not written");
        }
    }

    /// Windows' apps the grid lists, in the user's order, the hidden ones left out.
    fn app_grid_entries(&self) -> Vec<model::Entry> {
        let found = kabuk_win::shellitems::installed_apps_cached()
            .iter()
            .map(|a| Found {
                name: a.name.clone(),
                id: a.id.clone(),
                path: a.target.clone(),
                open: a.target(),
            })
            .collect();
        self.app_grid_arrangement()
            .apply(model::entries(found, self.app_grid_casing()), |e| e.open.as_str())
    }

    /// The grid listed again after the arrangement changed, on the same page.
    fn app_grid_rebuild(&mut self) {
        let entries = self.app_grid_entries();
        let casing = self.app_grid_casing();
        let Some(o) = self.app_grid.open.as_mut() else { return };
        let page = o.grid.page();
        o.grid = Grid::new(entries, casing, o.grid.shape());
        o.grid.go(page);
        o.slots = model::slots(o.grid.page(), o.grid.pages());
    }

    /// Editing begins (a held press or a right click on an app): the search goes, each
    /// app shows its badge, Done stands where the search was.
    fn app_grid_edit(&mut self, on: bool) {
        let Some(o) = self.app_grid.open.as_mut() else { return };
        if o.editing == on {
            return;
        }
        o.editing = on;
        o.drag = None;
        o.edge = None;
        // The icons wiggle while editing, unless the user turned animations off.
        let wiggles = on && self.motion_level() == Level::Full;
        let Some(o) = self.app_grid.open.as_mut() else { return };
        o.wiggle = wiggles.then(Instant::now);
        if let Some(c) = &self.control {
            if wiggles {
                c.set_timer(timer::APP_GRID_WIGGLE, Duration::from_millis(style::WIGGLE_FRAME_MS));
            } else {
                c.kill_timer(timer::APP_GRID_WIGGLE);
            }
        }
        if on && o.grid.clear() {
            o.slots = model::slots(o.grid.page(), o.grid.pages());
        }
        tracing::info!(on, wiggles, "app grid editing");
        self.render_app_grid(true, true);
    }

    /// The next frame of the wiggle: only the page on screen is drawn (the pages beside it
    /// show only while sliding, and a slide's frames are its own).
    pub(super) fn app_grid_wiggle_frame(&mut self) {
        match self.app_grid.open.as_ref() {
            Some(o) if o.wiggle.is_some() => {
                if o.sliding.is_some() {
                    return;
                }
            }
            _ => {
                self.kill(timer::APP_GRID_WIGGLE);
                return;
            }
        }
        let Shared {
            app_grid,
            gpu,
            i18n,
            look,
            ..
        } = self;
        let AppGridState { open, pictures, .. } = app_grid;
        let Some(o) = open.as_mut() else { return };
        let frame = view::Frame {
            size: o.size,
            i18n,
            text_scale: look.style.text_scale,
            pictures: &pictures.loaded,
            backdrop: o.backdrop,
            editing: o.editing,
            drag: o.drag,
            wiggle: o.wiggle.map(|t| t.elapsed().as_secs_f32()),
            reflow: reflow_now(&o.reflow),
        };
        let layout = kabuk_ui::node::layout(
            &view::strip(&o.grid, [None, o.slots[1], None], &frame),
            Point::default(),
            &gpu.text,
        );
        if let Some(s) = o.sheet.surface_mut() {
            if let Err(e) = s.render(gpu, &layout, &look.palette, o.scale, (0.0, 0.0), None) {
                tracing::debug!(error = %e, "app grid wiggle not drawn");
            }
        }
        // Presses find the apps where this frame draws them (found by the user: after a
        // hide the places of the slide's first frame stayed, so a badge pressed hid
        // another app and a press on an app ended editing).
        o.page_layout = kabuk_ui::node::layout(&view::page(&o.grid, o.slots[1], &frame), Point::default(), &gpu.text);
        let _ = gpu.commit();
    }

    /// The held press's time is up: editing begins, the press now moving its app.
    pub(super) fn app_grid_held(&mut self) {
        if let Some(c) = &self.control {
            c.kill_timer(timer::APP_GRID_HOLD);
        }
        let held = self
            .app_grid
            .open
            .as_ref()
            .and_then(|o| o.press.as_ref())
            .filter(|p| !p.dragging)
            .and_then(|p| p.app);
        if held.is_some() {
            self.app_grid_edit(true);
        }
    }

    /// The dragged app went to the place under the mouse at `at` (DIPs); held at a side
    /// edge, the page turns once its time is up.
    fn app_grid_drag_to(&mut self, from: usize, at: Point) {
        let Some(o) = self.app_grid.open.as_mut() else { return };
        let to = o.grid.place_of(view::cell_at(&o.grid, at, o.size));
        let edge = if at.x < style::EDGE_TURN {
            Some(-1)
        } else if at.x > o.size.w - style::EDGE_TURN {
            Some(1)
        } else {
            None
        };
        if edge != o.edge {
            o.edge = edge;
            if let Some(c) = &self.control {
                match edge {
                    Some(_) => c.set_timer(timer::APP_GRID_EDGE, Duration::from_millis(style::EDGE_TURN_MS)),
                    None => c.kill_timer(timer::APP_GRID_EDGE),
                }
            }
        }
        // A new gap: the others slide to make room (and out of the old one).
        if o.drag.map(|d| d.to) != Some(to) {
            o.begin_reflow();
        }
        o.drag = Some(view::Drag { from, to, at });
        self.render_app_grid(false, true);
    }

    /// A dragged app stayed at a side edge: the page turns that way.
    pub(super) fn app_grid_edge_due(&mut self) {
        if let Some(c) = &self.control {
            c.kill_timer(timer::APP_GRID_EDGE);
        }
        let Some(o) = self.app_grid.open.as_mut() else { return };
        let (Some(way), Some(d)) = (o.edge.take(), o.drag) else {
            return;
        };
        let page = o.grid.page() as isize;
        self.app_grid_go(page + way, None);
        // The drop place follows the new page; still at the edge, it turns on.
        self.app_grid_drag_to(d.from, d.at);
    }

    /// The dragged app is let go: it takes its new place, kept for next time.
    fn app_grid_drop(&mut self) {
        if let Some(c) = &self.control {
            c.kill_timer(timer::APP_GRID_EDGE);
        }
        let Some(o) = self.app_grid.open.as_mut() else { return };
        o.edge = None;
        // The dropped app slides from the mouse into its place.
        o.begin_reflow();
        let Some(d) = o.drag.take() else { return };
        let list = o.grid.shown_opens();
        let mut a = self.app_grid_arrangement();
        if a.moved(&list, d.from, d.to) {
            tracing::info!(from = d.from, to = d.to, "app grid app moved");
            self.save_app_grid_arrangement(&a);
            self.app_grid_rebuild();
        }
        self.render_app_grid(false, true);
    }

    /// The app at place `at` leaves the grid until the settings show it again.
    fn app_grid_hide(&mut self, at: usize) {
        let Some(e) = self.app_grid.open.as_ref().and_then(|o| o.grid.shown_at(at)).cloned() else {
            return;
        };
        if let Some(o) = self.app_grid.open.as_mut() {
            // The others close up the hidden app's place.
            o.begin_reflow();
        }
        let mut a = self.app_grid_arrangement();
        if a.hide(&e.open, &e.name) {
            tracing::info!(app = %e.name, "app grid app hidden");
            self.save_app_grid_arrangement(&a);
            self.app_grid_rebuild();
            self.render_app_grid(true, true);
        }
    }

    fn open_app_grid(&mut self, monitor: Option<Monitor>, activate: bool) {
        let asked = Instant::now();
        let Some(mon) = monitor.or_else(Self::launcher_monitor) else {
            return;
        };
        self.close_launcher();
        let rect = monitor_rect(&mon);
        let scale = Scale(mon.dpi as f32 / kabuk_layout::desk::BASE_DPI as f32);
        let size = Size {
            w: rect.w as f32 / scale.0,
            h: rect.h as f32 / scale.0,
        };
        let grid = Grid::new(self.app_grid_entries(), self.app_grid_casing(), Shape::fit(size));
        let backdrop = self.app_grid_screen(&mon).or_else(|| self.app_grid_backdrop(&mon));
        let title = self.instance_title();
        let chrome = Window::shell_surface(CHROME_CLASS, &title, rect, |_| None);
        let weak = self.me.clone();
        let sheet = Window::input_surface(GRID_CLASS, &title, rect, move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_app_grid(m)
        });
        let (chrome, sheet) = match (chrome, sheet) {
            (Ok(c), Ok(s)) => (c, s),
            (Err(e), _) | (_, Err(e)) => {
                tracing::error!(error = %e, "app grid window unavailable");
                return;
            }
        };
        // The chrome first, so the pages' window, made topmost after it, is above it.
        chrome.set_rect(rect);
        sheet.set_rect(rect);
        let (w, h) = (rect.w.max(1) as u32, rect.h.max(1) as u32);
        let surfaces = (
            WindowSurface::new(&self.gpu, chrome.hwnd().0 as isize, w, h),
            WindowSurface::new(&self.gpu, sheet.hwnd().0 as isize, w * 3, h),
        );
        let (chrome_surface, sheet_surface) = match surfaces {
            (Ok(c), Ok(s)) => (c, s),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(error = %e, "app grid surface unavailable");
                return;
            }
        };
        let level = self.motion_level();
        let pages = grid.pages();
        let page = grid.page();
        self.app_grid.open = Some(Open {
            sheet: Shown::enter(&self.gpu, Some(sheet_surface), sheet, (APP_GRID, scale), level),
            chrome: Shown::enter(
                &self.gpu,
                Some(chrome_surface),
                chrome,
                (APP_GRID_BACKDROP, scale),
                level,
            ),
            rect,
            scale,
            size,
            grid,
            slots: model::slots(page, pages),
            page_layout: Layout::default(),
            chrome_layout: Layout::default(),
            backdrop,
            sliding: None,
            press: None,
            wheel: Wheel::default(),
            surrogate: None,
            editing: false,
            drag: None,
            edge: None,
            wiggle: None,
            reflow: None,
        });
        self.render_app_grid(true, true);
        if let Some(o) = self.app_grid.open.as_ref() {
            if let Some(s) = o.sheet_surface() {
                let _ = s.set_offset_x(-o.width());
            }
            let _ = self.gpu.commit();
            o.chrome.window().show();
            let focused = activate && o.sheet.window().activate();
            if !focused {
                o.sheet.window().show();
            }
            tracing::info!(
                monitor = %mon.device,
                apps = o.grid.count(),
                pages,
                columns = o.grid.shape().columns,
                rows = o.grid.shape().rows,
                wallpaper = o.backdrop.is_some(),
                focused,
                ms = asked.elapsed().as_millis() as u64,
                "app grid shown"
            );
        }
    }

    /// Closes the grid: both windows fade out (the pages zooming away) and go once they
    /// have.
    pub(super) fn close_app_grid(&mut self) {
        let Some(o) = self.app_grid.open.take() else { return };
        if o.press.is_some() {
            o.sheet.window().capture_mouse(false);
        }
        if let Some(c) = &self.control {
            for t in [
                timer::APP_GRID_SLIDE,
                timer::APP_GRID_HOLD,
                timer::APP_GRID_EDGE,
                timer::APP_GRID_WIGGLE,
            ] {
                c.kill_timer(t);
            }
        }
        tracing::info!("app grid closed");
        // Its screen taking (a GPU device) is kept a while for the next opening, then let
        // go (PLAN M19 P15: it was kept for as long as the shell ran).
        if let Some(c) = &self.control {
            c.set_timer(timer::APP_GRID_CAPTURE, CAPTURE_KEPT);
        }
        self.retire(Some(o.sheet));
        self.retire(Some(o.chrome));
    }

    /// Draws the chrome and/or the strip again (in full) and commits.
    fn render_app_grid(&mut self, chrome: bool, strip: bool) {
        let side = {
            let Some(o) = self.app_grid.open.as_ref() else { return };
            (style::ICON * o.scale.0).ceil() as u32
        };
        // The icons of the pages the strip holds are asked for first.
        let wanted: Vec<String> = self
            .app_grid
            .open
            .as_ref()
            .map(|o| {
                o.slots
                    .iter()
                    .flatten()
                    .flat_map(|p| o.grid.on_page(*p))
                    .filter_map(|(_, i)| o.grid.entry(i).map(|e| e.open.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.app_grid.pictures.forget_dropped(&self.gpu.images);
        self.app_grid.pictures.ask(wanted, side);
        let Shared {
            app_grid,
            gpu,
            i18n,
            look,
            ..
        } = self;
        let AppGridState { open, pictures, .. } = app_grid;
        let Some(o) = open.as_mut() else { return };
        if let (true, Some(key)) = (chrome, o.backdrop) {
            if !gpu.images.contains(key) {
                // Found by the user: editing drew the grid over the desktop, its picture
                // gone from the store.
                tracing::warn!(key, "app grid picture lost");
            }
        }
        let frame = view::Frame {
            size: o.size,
            i18n,
            text_scale: look.style.text_scale,
            pictures: &pictures.loaded,
            backdrop: o.backdrop,
            editing: o.editing,
            drag: o.drag,
            wiggle: o.wiggle.map(|t| t.elapsed().as_secs_f32()),
            reflow: reflow_now(&o.reflow),
        };
        if chrome {
            let layout = kabuk_ui::node::layout(&view::chrome(&o.grid, &frame), Point::default(), &gpu.text);
            if let Some(s) = o.chrome.surface_mut() {
                if let Err(e) = s.render(gpu, &layout, &look.palette, o.scale, (0.0, 0.0), None) {
                    tracing::warn!(error = %e, "app grid chrome not drawn");
                }
            }
            o.chrome_layout = layout;
        }
        if strip {
            let layout = kabuk_ui::node::layout(&view::strip(&o.grid, o.slots, &frame), Point::default(), &gpu.text);
            if let Some(s) = o.sheet.surface_mut() {
                if let Err(e) = s.render(gpu, &layout, &look.palette, o.scale, (0.0, 0.0), None) {
                    tracing::warn!(error = %e, "app grid pages not drawn");
                }
            }
            // Where presses are found: the page as drawn now (the wiggle's frames keep it
            // up to date while the apps slide).
            o.page_layout =
                kabuk_ui::node::layout(&view::page(&o.grid, o.slots[1], &frame), Point::default(), &gpu.text);
            let names: Vec<&str> = o
                .slots
                .get(1)
                .copied()
                .flatten()
                .map(|p| o.grid.on_page(p))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(_, i)| o.grid.entry(i).map(|e| e.name.as_str()))
                .collect();
            tracing::debug!(page = o.grid.page(), query = %o.grid.query(), apps = %names.join(" | "), "app grid page");
        }
        let _ = gpu.commit();
    }

    /// Icons the worker loaded: an open grid shows them.
    pub(super) fn app_grid_pictures_arrived(&mut self) {
        let came = self.app_grid.pictures.arrived(&self.gpu.images);
        tracing::debug!(
            came,
            waiting = self.app_grid.pictures.pending(),
            "app grid pictures arrived"
        );
        if came > 0
            && self
                .app_grid
                .open
                .as_ref()
                .is_some_and(|o| o.sliding.is_none() && o.press.is_none())
        {
            self.render_app_grid(false, true);
        }
    }

    /// Turns to page `to` (kept within the pages), sliding from `from_x` (the strip's
    /// offset now, in pixels).
    fn app_grid_go(&mut self, to: isize, from_x: Option<f32>) {
        let Some(o) = self.app_grid.open.as_mut() else { return };
        if o.sliding.is_some() {
            return;
        }
        let from = o.grid.page();
        let to = to.clamp(0, o.grid.pages() as isize - 1) as usize;
        if to == from || !o.grid.go(to) {
            // Nothing to turn to: a drag that fell short slides back.
            if let Some(x) = from_x {
                let level = self.motion_level();
                self.app_grid_slide(x, 1, level);
            }
            return;
        }
        self.app_grid_turned(from, from_x);
    }

    /// The grid shows another page than `from` now (the arrows moved it, or a turn):
    /// the strip puts it beside `from` and slides there.
    fn app_grid_turned(&mut self, from: usize, from_x: Option<f32>) {
        let level = self.motion_level();
        let Some(o) = self.app_grid.open.as_mut() else { return };
        let to = o.grid.page();
        let (slots, slot) = model::slots_towards(from, to, o.grid.pages());
        let x = from_x.unwrap_or(-o.width());
        let redraw = slots != o.slots;
        o.slots = slots;
        tracing::info!(page = to, pages = o.grid.pages(), "app grid page turned");
        if redraw {
            self.render_app_grid(false, true);
        }
        self.render_app_grid(true, false);
        self.app_grid_slide(x, slot, level);
        if let Some(o) = self.app_grid.open.as_mut() {
            let seconds = if level == Level::Full { PAGE_SLIDE } else { 0.0 };
            o.sliding = Some(Instant::now() + Duration::from_secs_f32(seconds));
            if let Some(c) = &self.control {
                c.set_timer(timer::APP_GRID_SLIDE, Duration::from_secs_f32(seconds));
            }
        }
    }

    /// Slides the strip from `from_x` (pixels) until its slot `slot` (0 the left page, 1
    /// the middle one, 2 the right one) is on screen.
    fn app_grid_slide(&mut self, from_x: f32, slot: usize, level: Level) {
        let Some(o) = self.app_grid.open.as_ref() else { return };
        let to = -(slot as f32) * o.width();
        let seconds = if level == Level::Full { PAGE_SLIDE } else { 0.0 };
        if let Some(s) = o.sheet_surface() {
            if let Err(e) = s.slide_x(&self.gpu, from_x, to, seconds) {
                tracing::debug!(error = %e, "app grid page not slid");
            }
        }
        let _ = self.gpu.commit();
    }

    /// The slide is over: the strip holds the new page's neighbours, with it in the
    /// middle, as before.
    pub(super) fn app_grid_slide_over(&mut self) {
        if let Some(c) = &self.control {
            c.kill_timer(timer::APP_GRID_SLIDE);
        }
        let Some(o) = self.app_grid.open.as_mut() else { return };
        o.sliding = None;
        o.slots = model::slots(o.grid.page(), o.grid.pages());
        self.render_app_grid(false, true);
        if let Some(o) = self.app_grid.open.as_ref() {
            if let Some(s) = o.sheet_surface() {
                let _ = s.set_offset_x(-o.width());
            }
            let _ = self.gpu.commit();
        }
    }

    /// Opens the app at `at` among those shown, and closes the grid.
    fn app_grid_open_app(&mut self, at: Option<usize>) {
        let Some(o) = self.app_grid.open.as_ref() else { return };
        let entry = match at {
            Some(at) => o.grid.shown_at(at),
            None => o.grid.chosen(),
        }
        .cloned();
        let Some(e) = entry else { return };
        tracing::info!(app = %e.name, "app grid opened an app");
        self.close_app_grid();
        self.run_launcher_target(Target::Shell(e.open));
    }

    /// The search changed: the first page, without a slide.
    fn app_grid_searched(&mut self) {
        if let Some(o) = self.app_grid.open.as_mut() {
            o.slots = model::slots(o.grid.page(), o.grid.pages());
        }
        self.render_app_grid(true, true);
    }

    /// The pages' window's messages.
    pub(super) fn on_app_grid(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::KEYDOWN => {
                self.app_grid_key(m.wparam);
                Some(0)
            }
            msg::CHAR if self.app_grid.open.as_ref().is_some_and(|o| o.editing) => Some(0),
            msg::CHAR => {
                let text = self.app_grid_char(m.wparam as u16)?;
                if let Some(o) = self.app_grid.open.as_mut() {
                    o.grid.type_text(&text);
                }
                self.app_grid_searched();
                Some(0)
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                let o = self.app_grid.open.as_mut()?;
                let delta = msg::wheel(m.wparam);
                // Tilted right goes on, as turned towards the user does.
                let delta = if m.id == msg::MOUSEHWHEEL { -delta } else { delta };
                let turn = o.wheel.turn(delta, msg::WHEEL_DELTA);
                if turn != 0 {
                    let to = o.grid.page() as isize + turn;
                    self.app_grid_go(to, None);
                }
                Some(0)
            }
            msg::LBUTTONDOWN => {
                let o = self.app_grid.open.as_mut()?;
                if o.sliding.is_none() {
                    let at = msg::point(m.lparam);
                    let p = Point {
                        x: o.scale.dips(at.0),
                        y: o.scale.dips(at.1),
                    };
                    let app = o
                        .page_layout
                        .hit_test(p)
                        .filter(|h| h.module == view::hit::APP)
                        .map(|h| h.value as usize);
                    o.press = Some(Press {
                        at,
                        dx: 0.0,
                        dragging: false,
                        app,
                    });
                    o.sheet.window().capture_mouse(true);
                    // Held on an app, the press begins editing.
                    if app.is_some() && !o.editing {
                        if let Some(c) = &self.control {
                            c.set_timer(timer::APP_GRID_HOLD, Duration::from_millis(style::HOLD_MS));
                        }
                    }
                }
                Some(0)
            }
            msg::RBUTTONUP => {
                let o = self.app_grid.open.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let p = Point {
                    x: o.scale.dips(x),
                    y: o.scale.dips(y),
                };
                let on_app = o.page_layout.hit_test(p).is_some_and(|h| h.module == view::hit::APP);
                if on_app {
                    self.app_grid_edit(true);
                }
                Some(0)
            }
            msg::MOUSEMOVE => {
                let (x, y) = msg::point(m.lparam);
                let held = m.wparam & msg::MK_LBUTTON != 0;
                let o = self.app_grid.open.as_mut()?;
                let p = o.press.as_mut()?;
                if !held {
                    // The button went up where the window did not see it.
                    o.press = None;
                    o.sheet.window().capture_mouse(false);
                    return Some(0);
                }
                let moved = x - p.at.0;
                let dx = o.scale.dips(moved);
                let dy = o.scale.dips(y - p.at.1);
                if !p.dragging && dx.abs().max(dy.abs()) >= style::DRAG_START {
                    p.dragging = true;
                    if let Some(c) = &self.control {
                        c.kill_timer(timer::APP_GRID_HOLD);
                    }
                }
                if !p.dragging {
                    return Some(0);
                }
                // Editing, a drag that began on an app moves it.
                if let Some(from) = p.app.filter(|_| o.editing) {
                    let at = Point {
                        x: o.scale.dips(x),
                        y: o.scale.dips(y),
                    };
                    self.app_grid_drag_to(from, at);
                    return Some(0);
                }
                p.dx = dx;
                // Past the first or last page it gives only a little.
                let page = o.grid.page();
                let beyond = (dx > 0.0 && page == 0) || (dx < 0.0 && page + 1 >= o.grid.pages());
                let give = if beyond { EDGE_GIVE } else { 1.0 };
                let x = -o.width() + moved as f32 * give;
                if let Some(s) = o.sheet_surface() {
                    let _ = s.set_offset_x(x);
                }
                let _ = self.gpu.commit();
                Some(0)
            }
            msg::LBUTTONUP => {
                if let Some(c) = &self.control {
                    c.kill_timer(timer::APP_GRID_HOLD);
                }
                let o = self.app_grid.open.as_mut()?;
                let press = o.press.take();
                o.sheet.window().capture_mouse(false);
                let (x, y) = msg::point(m.lparam);
                if o.drag.is_some() {
                    self.app_grid_drop();
                    return Some(0);
                }
                match press {
                    Some(p) if p.dragging => {
                        let page = o.grid.page() as isize;
                        let beyond = (p.dx > 0.0 && page == 0) || (p.dx < 0.0 && page + 1 >= o.grid.pages() as isize);
                        let give = if beyond { EDGE_GIVE } else { 1.0 };
                        let from_x = -o.width() + o.scale.0 * p.dx * give;
                        let turn = model::swipe(p.dx, o.size.w);
                        self.app_grid_go(page + turn, Some(from_x));
                    }
                    _ => {
                        let at = Point {
                            x: o.scale.dips(x),
                            y: o.scale.dips(y),
                        };
                        self.app_grid_click(at);
                    }
                }
                Some(0)
            }
            msg::SETCURSOR => {
                // Over the search field, the text cursor: a field one can type into.
                let o = self.app_grid.open.as_ref()?;
                let (x, y) = kabuk_win::input::cursor_pos()?;
                let at = Point {
                    x: o.scale.dips(x - o.rect.x),
                    y: o.scale.dips(y - o.rect.y),
                };
                let over = o
                    .chrome_layout
                    .hit_test(at)
                    .is_some_and(|h| h.module == view::hit::SEARCH);
                if !over {
                    return None;
                }
                msgloop::show_text_cursor();
                Some(1)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    // Another window took the keyboard.
                    self.close_app_grid();
                }
                None
            }
            _ => None,
        }
    }

    /// A click (not a drag) at `p` (DIPs on the monitor): a dot turns to its page, an app
    /// opens, the search field shows it takes the keys, anywhere else closes the grid.
    fn app_grid_click(&mut self, p: Point) {
        let Some(o) = self.app_grid.open.as_ref() else { return };
        let chrome = o.chrome_layout.hit_test(p).map(|h| (h.module, h.value));
        let app = o
            .page_layout
            .hit_test(p)
            .filter(|h| h.module == view::hit::APP)
            .map(|h| h.value as usize);
        let editing = o.editing;
        let hide = o
            .page_layout
            .hit_test(p)
            .filter(|h| h.module == view::hit::HIDE)
            .map(|h| h.value as usize);
        if editing {
            match (chrome, hide, app) {
                (Some((view::hit::DONE, _)), _, _) => self.app_grid_edit(false),
                (Some((view::hit::DOT, page)), _, _) => self.app_grid_go(page as isize, None),
                (_, Some(at), _) => self.app_grid_hide(at),
                (_, _, Some(_)) => {}
                _ => self.app_grid_edit(false),
            }
            return;
        }
        match (chrome, app) {
            (Some((view::hit::DOT, page)), _) => self.app_grid_go(page as isize, None),
            (Some((view::hit::SEARCH, _)), _) => {
                if self.app_grid.open.as_mut().is_some_and(|o| o.grid.focus_search()) {
                    tracing::info!("app grid search field clicked");
                    self.render_app_grid(true, false);
                }
            }
            (_, Some(at)) => self.app_grid_open_app(Some(at)),
            _ => self.close_app_grid(),
        }
    }

    fn app_grid_key(&mut self, key: usize) {
        let Some(o) = self.app_grid.open.as_mut() else { return };
        let page = o.grid.page();
        let dir = match key {
            vk::LEFT => Some(Dir::Left),
            vk::RIGHT => Some(Dir::Right),
            vk::UP => Some(Dir::Up),
            vk::DOWN => Some(Dir::Down),
            _ => None,
        };
        if let Some(d) = dir {
            if o.sliding.is_some() || !o.grid.step(d) {
                return;
            }
            if o.grid.page() != page {
                self.app_grid_turned(page, None);
            } else {
                self.render_app_grid(false, true);
            }
            return;
        }
        let pages = o.grid.pages() as isize;
        match key {
            vk::PRIOR => self.app_grid_go(page as isize - 1, None),
            vk::NEXT => self.app_grid_go(page as isize + 1, None),
            vk::HOME => self.app_grid_go(0, None),
            vk::END => self.app_grid_go(pages - 1, None),
            vk::RETURN => self.app_grid_open_app(None),
            vk::ESCAPE if o.editing => self.app_grid_edit(false),
            vk::ESCAPE => {
                if o.grid.clear() {
                    self.app_grid_searched();
                } else {
                    self.close_app_grid();
                }
            }
            vk::BACK => {
                if msgloop::ctrl_held() {
                    o.grid.delete_word();
                } else {
                    o.grid.backspace();
                }
                self.app_grid_searched();
            }
            vk::V if msgloop::ctrl_held() => {
                if let Some(text) = kabuk_win::clipboard::text() {
                    o.grid.type_text(&text);
                    self.app_grid_searched();
                }
            }
            _ => {}
        }
    }

    /// A `WM_CHAR`: text, or nothing for control characters (Enter, Backspace, Ctrl+…).
    fn app_grid_char(&mut self, unit: u16) -> Option<String> {
        let o = self.app_grid.open.as_mut()?;
        if (0xD800..0xDC00).contains(&unit) {
            o.surrogate = Some(unit);
            return None;
        }
        let units: Vec<u16> = match o.surrogate.take() {
            Some(high) if (0xDC00..0xE000).contains(&unit) => vec![high, unit],
            _ => vec![unit],
        };
        let s = String::from_utf16(&units).ok()?;
        (!s.chars().any(char::is_control)).then_some(s)
    }

    /// Integration tests: open on the primary monitor without the keyboard, or close.
    #[cfg(feature = "integration")]
    /// `what`: 0 closes the grid, 1 opens it on the primary monitor, 2 starts editing,
    /// 3 presses the badge of the app at place `place`, 4 presses that app: the mouse's
    /// own messages to the pages' window, at the middle of what is drawn there now (tests
    /// never move the real mouse).
    pub(super) fn test_app_grid(&mut self, what: usize, place: usize) {
        match what {
            0 => self.close_app_grid(),
            1 => {
                if self.app_grid.open.is_none() {
                    let primary = kabuk_win::shell::monitors().into_iter().find(|m| m.primary);
                    self.open_app_grid(primary, false);
                }
            }
            2 => self.app_grid_edit(true),
            _ => self.test_app_grid_press(if what == 3 { view::hit::HIDE } else { view::hit::APP }, place),
        }
    }

    /// A press and release in the middle of where `module` `place` is drawn on the page
    /// on screen now (its slide included), as the mouse would send them.
    #[cfg(feature = "integration")]
    fn test_app_grid_press(&mut self, module: &str, place: usize) {
        let Some(o) = self.app_grid.open.as_ref() else { return };
        let frame = view::Frame {
            size: o.size,
            i18n: &self.i18n,
            text_scale: self.look.style.text_scale,
            pictures: &self.app_grid.pictures.loaded,
            backdrop: o.backdrop,
            editing: o.editing,
            drag: o.drag,
            wiggle: None,
            reflow: reflow_now(&o.reflow),
        };
        let drawn = kabuk_ui::node::layout(
            &view::page(&o.grid, o.slots[1], &frame),
            Point::default(),
            &self.gpu.text,
        );
        let Some(r) = drawn
            .hits
            .iter()
            .find(|(_, h)| h.module == module && h.value as usize == place)
            .map(|(r, _)| *r)
        else {
            tracing::warn!(module, place, "app grid test press: nothing drawn there");
            return;
        };
        let (x, y) = (o.scale.px(r.x + r.w / 2.0), o.scale.px(r.y + r.h / 2.0));
        let at = ((y as u32 & 0xFFFF) << 16 | (x as u32 & 0xFFFF)) as isize;
        let window = o.sheet.window().hwnd();
        for (id, wparam) in [(msg::LBUTTONDOWN, msg::MK_LBUTTON), (msg::LBUTTONUP, 0)] {
            let _ = msgloop::post_message(window, Msg { id, wparam, lparam: at });
        }
        tracing::info!(module, place, x, y, "app grid test press");
    }
}

impl Open {
    fn sheet_surface(&self) -> Option<&WindowSurface> {
        self.sheet.surface()
    }
}

/// Reads the wallpaper `key` names small, blurs and darkens it, and posts it to `control`.
fn blur_worker(key: BackdropKey, control: Hwnd, out: Blurred) {
    kabuk_win::system::com_init();
    let started = Instant::now();
    let (w, h) = ((key.2 / style::BLUR_SHRINK).max(1), (key.3 / style::BLUR_SHRINK).max(1));
    let px = match kabuk_win::picture::cover(std::path::Path::new(&key.0), w, h) {
        Ok(bgr) => backdrop::blurred(
            &bgr,
            w as usize,
            h as usize,
            style::BLUR_RADIUS,
            style::BLUR_PASSES,
            style::BLUR_DIM,
        ),
        Err(e) => {
            tracing::warn!(file = %key.0, error = %e, "app grid wallpaper not read");
            return;
        }
    };
    tracing::info!(
        w,
        h,
        ms = started.elapsed().as_millis() as u64,
        "app grid wallpaper blurred"
    );
    if let Ok(mut o) = out.lock() {
        o.push((key, w, h, px));
    }
    let _ = msgloop::post_message(
        control,
        Msg {
            id: MSG_APP_GRID_BACKDROP,
            wparam: 0,
            lparam: 0,
        },
    );
}
