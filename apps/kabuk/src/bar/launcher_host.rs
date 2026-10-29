//! The launcher on the bar thread (SPEC §2.5): the shortcut that opens it, its window,
//! its entries and what choosing one does. A child of the bar manager, so it shares the
//! GPU, the look, the language and the configuration with the bars.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kabuk_core::config::{shortcut::Chord, Config};
use kabuk_i18n::{I18n, Lang};
use kabuk_search::Casing;
use kabuk_ui::geom::{Point, Scale, Size};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};
use kabuk_win::window::Hwnd;

use super::Shared;
use crate::bar::pictures::Pictures;
use crate::launcher::item::{Item, SystemCommand, Target};
use crate::launcher::model::{Key, Model, Options, Outcome};
use crate::launcher::place::{self, BarSpace};
use crate::launcher::sources::{self, WindowEntry};
use crate::launcher::style::LauncherStyle;
use crate::launcher::view::{self, Frame, HIT_ROW};
use crate::wm;

/// Class of the launcher window (tests find it by class).
pub const LAUNCHER_CLASS: &str = "Kabuk.Launcher";
/// The worker finished gathering entries.
pub(super) const MSG_LAUNCHER_ITEMS: u32 = msg::APP + 8;
/// The picture worker loaded some of the entries' pictures.
pub(super) const MSG_LAUNCHER_PICTURES: u32 = msg::APP + 43;
/// Integration tests open (`wparam` 1) or close (0) the launcher without the shortcut;
/// opened this way it shows without taking the keyboard from the user's window.
pub const MSG_TEST_LAUNCHER: u32 = msg::APP + 24;
/// Hotkey ids of the launcher's chords on the control window.
const HOTKEY_FIRST: i32 = 100;
/// The entries run lately, in the state folder.
const RAN_FILE: &str = "launcher-recent.json";
/// Gathered entries older than this are gathered again when the launcher opens.
const REFRESH_AFTER: Duration = Duration::from_secs(300);

pub(super) struct LauncherState {
    window: Window,
    /// Dropped before the window.
    surface: Option<WindowSurface>,
    model: Model,
    /// The slow sources: filled by the worker, then kept until they are stale.
    arrived: Arc<Mutex<Option<Vec<Item>>>>,
    slow: Vec<Item>,
    gathered_at: Option<Instant>,
    gathering: bool,
    open: bool,
    /// Where it was drawn: the window in physical pixels, its scale, the layout for clicks.
    rect: PxRect,
    scale: Scale,
    layout: Layout,
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    /// The first half of a character outside the BMP, waiting for the second.
    surrogate: Option<u16>,
    /// When the shortcut was pressed, for the first-frame time (PLAN M6).
    asked: Option<Instant>,
    /// The entries' own pictures (an app's icon takes 10-60 ms to load: a worker loads
    /// those of the rows shown, and they are drawn as they come).
    pictures: Pictures,
    /// The entries run lately, newest first (an empty launcher lists them first).
    ran: Vec<String>,
}

fn options(cfg: &Config, i18n: &I18n, bottom_up: bool) -> Options {
    Options {
        casing: if i18n.lang() == Lang::Tr {
            Casing::Turkish
        } else {
            Casing::Default
        },
        decimal: i18n.tr("decimal-separator").chars().next().unwrap_or('.'),
        calculator: cfg.launcher.sources.calculator,
        bottom_up,
    }
}

impl Shared {
    pub(super) fn create_launcher(&mut self) {
        let weak = self.me.clone();
        let window = Window::input_surface(LAUNCHER_CLASS, &self.instance_title(), PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_launcher(m)
        });
        let window = match window {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "launcher window unavailable");
                return;
            }
        };
        let cfg = self.config().clone();
        self.launcher = Some(LauncherState {
            window,
            surface: None,
            model: Model::new(Vec::new(), options(&cfg, &self.i18n, false)),
            arrived: Arc::default(),
            slow: Vec::new(),
            gathered_at: None,
            gathering: false,
            open: false,
            rect: PxRect::default(),
            scale: Scale(1.0),
            layout: Layout::default(),
            hotkeys: Vec::new(),
            surrogate: None,
            asked: None,
            pictures: Pictures::default(),
            ran: self
                .ran_file()
                .and_then(|f| std::fs::read_to_string(f).ok())
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
        });
        if let (Some(control), Some(l)) = (self.control.as_ref().map(Window::hwnd), self.launcher.as_mut()) {
            l.pictures = Pictures::start("launcher", control, MSG_LAUNCHER_PICTURES);
        }
        self.bind_launcher_keys();
    }

    /// The instance id is the launcher window's title, so tests find their own.
    pub(super) fn instance_title(&self) -> String {
        self.instance.clone()
    }

    /// Registers `shortcuts.launcher` (outside test mode).
    pub(super) fn bind_launcher_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.launcher.clone();
        let (Some(l), Some(control)) = (self.launcher.as_mut(), self.control.as_ref()) else {
            return;
        };
        l.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "launcher shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => l.hotkeys.push(h),
                Err(e) => tracing::warn!(chord = %text, error = %e, "launcher shortcut taken by another program"),
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the launcher's.
    pub(super) fn launcher_hotkey(&mut self, id: usize) -> bool {
        let Some(l) = self.launcher.as_ref() else { return false };
        let ours = (HOTKEY_FIRST as usize..HOTKEY_FIRST as usize + l.hotkeys.len()).contains(&id);
        if ours {
            if l.open {
                self.close_launcher();
            } else {
                self.open_launcher(true);
            }
        }
        ours
    }

    /// The configuration changed: new chords, and sources gathered again next time.
    pub(super) fn launcher_config_changed(&mut self, before: &Config, after: &Config) {
        if before.shortcuts.launcher != after.shortcuts.launcher {
            self.bind_launcher_keys();
        }
        if before.launcher != after.launcher || before.language != after.language {
            if let Some(l) = self.launcher.as_mut() {
                l.gathered_at = None;
            }
        }
    }

    pub(super) fn open_launcher(&mut self, activate: bool) {
        let asked = Instant::now();
        self.gather_launcher_items(false);
        let items = self.launcher_items();
        let cfg = self.config().clone();
        let i18n = &self.i18n;
        let Some(l) = self.launcher.as_mut() else { return };
        l.model = Model::new(items, options(&cfg, i18n, false));
        l.open = true;
        l.asked = Some(asked);
        l.surrogate = None;
        self.render_launcher();
        let level = self.motion_level();
        let Some(l) = self.launcher.as_mut() else { return };
        // It enters; it leaves at once, so the keyboard goes back where it was (D33).
        if let Some(s) = &l.surface {
            super::motion_host::play_entrance(&self.gpu, s, (kabuk_ui::motion::MENU, l.scale), level);
            let _ = self.gpu.commit();
        }
        if activate {
            if !l.window.activate() {
                tracing::debug!("launcher shown without the keyboard: Windows refused the foreground");
            }
        } else {
            l.window.show();
        }
        let took = asked.elapsed();
        tracing::info!(first_frame_us = took.as_micros() as u64, "launcher shown");
    }

    pub(super) fn close_launcher(&mut self) {
        if let Some(l) = self.launcher.as_mut() {
            if l.open {
                l.open = false;
                l.window.hide();
                tracing::debug!("launcher closed");
            }
        }
    }

    /// Open windows (front first) and the gathered sources.
    fn launcher_items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        if self.config().launcher.sources.windows {
            if let Some(view) = self.wm.as_ref().map(wm::WmLink::view) {
                let z: std::collections::HashMap<u64, usize> = kabuk_win::window::top_level_windows()
                    .into_iter()
                    .enumerate()
                    .map(|(i, h)| (h.0, i))
                    .collect();
                let mut entries: Vec<WindowEntry> = view
                    .windows
                    .iter()
                    .map(|w| WindowEntry {
                        picture: self.apps.framed.get(&w.hwnd).cloned().or_else(|| {
                            Hwnd(w.hwnd)
                                .pid()
                                .and_then(|pid| self.apps.exes.get(&pid))
                                .map(|(_, icon)| icon.clone())
                        }),
                        hwnd: w.hwnd,
                        title: Hwnd(w.hwnd).title(),
                        workspace: w.workspace,
                        monitor: view
                            .monitors
                            .iter()
                            .find(|m| m.device == w.device)
                            .map_or(0, |m| m.number),
                    })
                    .collect();
                entries.sort_by_key(|e| z.get(&e.hwnd).copied().unwrap_or(usize::MAX));
                items.extend(sources::window_items(&entries, &self.i18n));
            }
        }
        if let Some(l) = self.launcher.as_ref() {
            if l.slow.is_empty() && self.config().launcher.sources.commands {
                // Until the worker is done, the commands are there at least.
                items.extend(sources::commands(&self.i18n));
                let profiles: Vec<String> = self.config().profiles.iter().map(|p| p.name.clone()).collect();
                items.extend(sources::profiles(&self.i18n, &profiles));
            }
            items.extend(l.slow.iter().cloned());
            return crate::launcher::item::recent_first(items, &l.ran, &self.pinned_targets());
        }
        items
    }

    /// The bar's pinned apps as the launcher opens them (`shell:AppsFolder\<app id>`).
    fn pinned_targets(&self) -> Vec<String> {
        self.apps
            .pins
            .iter()
            .filter_map(|p| {
                p.app_id.clone().or_else(|| {
                    let exe = p.exe.as_deref()?;
                    kabuk_win::shellitems::program_entry(exe).map(|a| a.id)
                })
            })
            .map(|id| format!("shell:AppsFolder\\{id}"))
            .collect()
    }

    fn ran_file(&self) -> Option<std::path::PathBuf> {
        Some(self.state.path().parent()?.join(RAN_FILE))
    }

    /// Remembers `target` as run now, in the state folder too.
    fn remember_run(&mut self, target: &Target) {
        let file = self.ran_file();
        let Some(l) = self.launcher.as_mut() else { return };
        crate::launcher::item::remember(&mut l.ran, target);
        let text = serde_json::to_string(&l.ran).unwrap_or_default();
        if let Some(f) = file {
            if let Err(e) = kabuk_core::fsutil::write_atomic(&f, text.as_bytes()) {
                tracing::debug!(error = %e, "launcher's recent runs not saved");
            }
        }
    }

    /// Starts the worker unless fresh entries are there (or `force`).
    fn gather_launcher_items(&mut self, force: bool) {
        let cfg = self.config().launcher.clone();
        let lang = self.i18n.lang();
        let Some(control) = self.control.as_ref().map(Window::hwnd) else {
            return;
        };
        let Some(l) = self.launcher.as_mut() else { return };
        let fresh = l.gathered_at.is_some_and(|t| t.elapsed() < REFRESH_AFTER);
        if l.gathering || (fresh && !force) {
            return;
        }
        l.gathering = true;
        let out = l.arrived.clone();
        let started = std::thread::Builder::new()
            .name("launcher-sources".into())
            .spawn(move || {
                kabuk_win::system::com_init();
                let t = Instant::now();
                let items = sources::gather(&cfg, &I18n::new(lang));
                tracing::info!(
                    entries = items.len(),
                    took_ms = t.elapsed().as_millis() as u64,
                    "launcher entries gathered"
                );
                if let Ok(mut o) = out.lock() {
                    *o = Some(items);
                }
                let _ = msgloop::post_message(
                    control,
                    Msg {
                        id: MSG_LAUNCHER_ITEMS,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            });
        if let Err(e) = started {
            tracing::warn!(error = %e, "launcher sources not gathered");
            l.gathering = false;
        }
    }

    /// The worker's entries arrived: an open launcher shows them at once.
    pub(super) fn launcher_items_arrived(&mut self) {
        let Some(l) = self.launcher.as_mut() else { return };
        let items = l.arrived.lock().ok().and_then(|mut o| o.take());
        l.gathering = false;
        if let Some(items) = items {
            l.slow = items;
            l.gathered_at = Some(Instant::now());
        }
        if l.open {
            let all = self.launcher_items();
            if let Some(l) = self.launcher.as_mut() {
                l.model.set_items(all);
            }
            self.render_launcher();
        }
    }

    /// Pictures the worker loaded: into the renderer's store, and an open launcher shows
    /// them.
    pub(super) fn launcher_pictures_arrived(&mut self) {
        let Some(l) = self.launcher.as_mut() else { return };
        l.pictures.arrived(&self.gpu.images);
        if l.open {
            self.render_launcher();
        }
    }

    /// The monitor the user works on: the foreground window's, else the primary one.
    pub(super) fn launcher_monitor() -> Option<kabuk_win::shell::Monitor> {
        let monitors = kabuk_win::shell::monitors();
        let fg = kabuk_win::events::foreground().and_then(Hwnd::frame_rect);
        let on = |m: &&kabuk_win::shell::Monitor| {
            fg.is_some_and(|r| {
                let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                m.rect.left <= cx && cx < m.rect.right && m.rect.top <= cy && cy < m.rect.bottom
            })
        };
        monitors
            .iter()
            .find(on)
            .or_else(|| monitors.iter().find(|m| m.primary))
            .cloned()
    }

    fn render_launcher(&mut self) {
        let Some(mon) = Self::launcher_monitor() else { return };
        let scale = Scale(mon.dpi as f32 / kabuk_layout::desk::BASE_DPI as f32);
        let monitor = Size {
            w: (mon.rect.right - mon.rect.left) as f32 / scale.0,
            h: (mon.rect.bottom - mon.rect.top) as f32 / scale.0,
        };
        let bar = self
            .bars
            .iter()
            .find(|b| b.monitor.device == mon.device)
            .map(|b| BarSpace {
                edge: b.edge,
                distance: self.look.tokens.px("edge").unwrap_or(0.0),
                reserved: b.thickness as f32 / b.scale.0,
            });
        let style = LauncherStyle::of(self.look.style.look);
        let shape = place::shape(&style, monitor, bar);
        let cfg = self.config().clone();
        let text_scale = self.look.style.text_scale;
        let Some(l) = self.launcher.as_mut() else { return };
        if shape.bottom_up != l.model_bottom_up() {
            // Konsol below its input: rebuilt with the order it needs.
            let items: Vec<Item> = (0..).map_while(|i| l.model.item(i).cloned()).collect();
            let query = l.model.query().to_string();
            l.model = Model::new(items, options(&cfg, &self.i18n, shape.bottom_up));
            l.model.key(Key::Text(query));
        }
        let side = (style.icon * scale.0).ceil() as u32;
        l.pictures.forget_dropped(&self.gpu.images);
        let wanted: Vec<String> = l
            .model
            .rows()
            .iter()
            .filter_map(|r| match r {
                crate::launcher::model::Row::Item { index, .. } => l.model.item(*index)?.picture.clone(),
                _ => None,
            })
            .collect();
        l.pictures.ask(wanted, side);
        let frame = Frame {
            style: &style,
            i18n: &self.i18n,
            pictures: &l.pictures.loaded,
            text_scale,
            width: shape.width,
            height: shape.height,
            columns: shape.columns,
            bottom_up: shape.bottom_up,
            gathering: l.gathering,
        };
        let node = view::view(&l.model, frame, &self.gpu.text);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let at = place::position(&style, monitor, bar, layout.size);
        let rect = PxRect {
            x: mon.rect.left + (at.x * scale.0).round() as i32,
            y: mon.rect.top + (at.y * scale.0).round() as i32,
            w: (layout.size.w * scale.0).ceil() as i32,
            h: (layout.size.h * scale.0).ceil() as i32,
        };
        l.window.set_rect(rect);
        let (w, h) = (rect.w.max(1) as u32, rect.h.max(1) as u32);
        match l.surface.as_mut() {
            Some(s) if s.size() != (w, h) => {
                if let Err(e) = s.resize(&self.gpu, w, h) {
                    tracing::warn!(error = %e, "launcher surface not resized");
                }
            }
            Some(_) => {}
            None => match WindowSurface::new(&self.gpu, l.window.hwnd().0 as isize, w, h) {
                Ok(s) => l.surface = Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "launcher surface unavailable");
                    return;
                }
            },
        }
        if let Some(s) = l.surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "launcher not drawn");
            }
            let _ = self.gpu.commit();
        }
        l.rect = rect;
        l.scale = scale;
        let titles = l.row_titles();
        tracing::debug!(query = %l.model.query(), rows = %titles.join(" | "), "launcher rows");
        // What screen readers find in it: its rows by their titles, the selected one
        // holding the keyboard (D97).
        let parts = crate::bar::access::popup_parts(
            &layout,
            scale,
            (HIT_ROW, kabuk_win::uia::Role::ListItem),
            u32::try_from(l.model.selected()).ok(),
            |v, drawn| {
                usize::try_from(v)
                    .ok()
                    .and_then(|v| titles.get(v).cloned())
                    .or_else(|| Some(drawn.to_string()))
            },
        );
        l.window
            .set_accessible(kabuk_win::uia::Container::List, &self.i18n.tr("l-name"), parts);
        l.layout = layout;
    }

    /// The launcher window's messages.
    pub(super) fn on_launcher(&mut self, m: Msg) -> Option<isize> {
        let key = match m.id {
            msg::KEYDOWN => {
                let ctrl = msgloop::ctrl_held();
                match m.wparam {
                    vk::UP => Some(Key::Up),
                    vk::DOWN => Some(Key::Down),
                    vk::PRIOR => Some(Key::PageUp),
                    vk::NEXT => Some(Key::PageDown),
                    vk::TAB if msgloop::shift_held() => Some(Key::Previous),
                    vk::TAB => Some(Key::Next),
                    vk::HOME if ctrl => Some(Key::First),
                    vk::END if ctrl => Some(Key::Last),
                    vk::HOME => Some(Key::Home),
                    vk::END => Some(Key::End),
                    vk::LEFT if ctrl => Some(Key::WordLeft),
                    vk::RIGHT if ctrl => Some(Key::WordRight),
                    vk::LEFT => Some(Key::Left),
                    vk::RIGHT => Some(Key::Right),
                    vk::RETURN => Some(Key::Enter),
                    vk::ESCAPE => Some(Key::Escape),
                    vk::BACK if ctrl => Some(Key::DeleteWord),
                    vk::BACK => Some(Key::Backspace),
                    vk::DELETE => Some(Key::Delete),
                    vk::A if ctrl => Some(Key::SelectAll),
                    vk::V if ctrl => kabuk_win::clipboard::text().map(Key::Text),
                    _ => None,
                }
            }
            msg::CHAR => self.launcher_char(m.wparam as u16).map(Key::Text),
            msg::LBUTTONUP => {
                let l = self.launcher.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let p = Point {
                    x: l.scale.dips(x),
                    y: l.scale.dips(y),
                };
                if let Some(h) = l.layout.hit_test(p).filter(|h| h.module == HIT_ROW) {
                    let i = h.value as usize;
                    let outcome = self.launcher.as_mut()?.model.choose(i);
                    self.launcher_outcome(outcome);
                }
                return Some(0);
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    // A click elsewhere or another window took the keyboard.
                    self.close_launcher();
                }
                return None;
            }
            _ => return None,
        };
        if let Some(k) = key {
            let outcome = self.launcher.as_mut()?.model.key(k);
            self.launcher_outcome(outcome);
        }
        Some(0)
    }

    /// A `WM_CHAR`: text, or nothing for control characters (Enter, Backspace, Ctrl+…).
    fn launcher_char(&mut self, unit: u16) -> Option<String> {
        let l = self.launcher.as_mut()?;
        if (0xD800..0xDC00).contains(&unit) {
            l.surrogate = Some(unit);
            return None;
        }
        let units: Vec<u16> = match l.surrogate.take() {
            Some(high) if (0xDC00..0xE000).contains(&unit) => vec![high, unit],
            _ => vec![unit],
        };
        let s = String::from_utf16(&units).ok()?;
        (!s.chars().any(char::is_control)).then_some(s)
    }

    fn launcher_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Same => {}
            Outcome::Changed => self.render_launcher(),
            Outcome::Close => self.close_launcher(),
            Outcome::Run(target) => {
                self.close_launcher();
                self.run_launcher_target(target);
            }
        }
    }

    pub(super) fn run_launcher_target(&mut self, target: Target) {
        tracing::info!(target = ?target, "launcher ran");
        self.remember_run(&target);
        match target {
            Target::Shell(t) => {
                // The shell can take a while (a network path, a slow program); never on
                // the bar thread.
                self.open_off_thread(t, crate::notify::Failure::Open);
            }
            Target::Window(h) => match &self.wm {
                Some(link) => link.send(wm::Command::Reveal(h)),
                None => {
                    Hwnd(h).focus();
                }
            },
            Target::Copy(text) => match kabuk_win::clipboard::set_text(&text) {
                Ok(()) => self.notify(crate::notify::Notice::Copied),
                Err(e) => {
                    tracing::warn!(error = %e, "result not copied");
                    self.notify(crate::notify::Notice::Failed(crate::notify::Failure::Copy));
                }
            },
            Target::Profile(name) => {
                if let Some(link) = &self.wm {
                    link.send(crate::wm::Command::OpenProfile(name));
                }
            }
            Target::Command(c) => {
                let r = match c {
                    SystemCommand::Settings => self.open_settings(),
                    SystemCommand::Lock => kabuk_win::power::lock(),
                    SystemCommand::Sleep => kabuk_win::power::sleep(),
                    SystemCommand::ScreenSaver => {
                        if let Some(l) = self.launcher.as_ref() {
                            kabuk_win::power::start_screen_saver(&l.window);
                        }
                        Ok(())
                    }
                    SystemCommand::Restart => kabuk_win::power::restart(false),
                    SystemCommand::ShutDown => kabuk_win::power::restart(true),
                };
                if let Err(e) = r {
                    tracing::warn!(command = ?c, error = %e, "command failed");
                    let what = if c == SystemCommand::Settings {
                        crate::notify::Failure::Settings
                    } else {
                        crate::notify::Failure::Command
                    };
                    self.notify(crate::notify::Notice::Failed(what));
                }
            }
        }
    }
}

impl Shared {
    /// Starts the settings app next to this program, on this instance's configuration
    /// and state, and lets it come to the front (it shows its open window if it runs).
    pub(super) fn open_settings(&self) -> std::io::Result<()> {
        let exe = std::env::current_exe()?.with_file_name(SETTINGS_EXE);
        let state_dir = self.state.path().parent().map(std::path::Path::to_path_buf);
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--config").arg(&self.config_file);
        if let Some(dir) = state_dir {
            cmd.arg("--state-dir").arg(dir);
        }
        let child = cmd.spawn()?;
        kabuk_win::process::allow_foreground(child.id());
        tracing::info!(pid = child.id(), "settings opened");
        Ok(())
    }
}

/// The settings app, next to `kabuk.exe`.
const SETTINGS_EXE: &str = "kabuk-settings.exe";

impl LauncherState {
    fn model_bottom_up(&self) -> bool {
        self.model.bottom_up()
    }

    /// The shown rows' titles, for the log.
    fn row_titles(&self) -> Vec<String> {
        self.model
            .rows()
            .iter()
            .map(|r| match r {
                crate::launcher::model::Row::Item { index, .. } => {
                    self.model.item(*index).map(|i| i.title.clone()).unwrap_or_default()
                }
                crate::launcher::model::Row::Calc { value } => format!("= {value}"),
            })
            .collect()
    }
}
