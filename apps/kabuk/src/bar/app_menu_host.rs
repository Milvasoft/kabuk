//! An app's menu on the bar thread ([`crate::bar::app_menu`]), opened by a right click
//! on the app, on a bar or on Rıhtım's dock, and placed like the power menu; an entry opens a recent file, another
//! window, pins, unpins or closes; a click elsewhere or Esc closes it.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};
use kabuk_win::window::Hwnd;

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::app_menu::{self, Action, Subject};
use kabuk_core::jumplist::Launch;

const MENU_CLASS: &str = "Kabuk.AppMenu";
/// Integration tests work the menu of the app holding window `lparam` (by window, so a
/// test only touches its own program) without taking the keyboard: `wparam` [`TEST_OPEN`]
/// opens it, [`TEST_CLOSE`] closes it, [`TEST_PIN`] and [`TEST_CLOSE_WINDOWS`] choose
/// those entries, [`TEST_TO_FRONT`] drags the app to the front of the bar.
pub const MSG_TEST_APP_MENU: u32 = msg::APP + 36;
pub const TEST_CLOSE: usize = 0;
pub const TEST_OPEN: usize = 1;
pub const TEST_PIN: usize = 2;
pub const TEST_CLOSE_WINDOWS: usize = 3;
pub const TEST_TO_FRONT: usize = 4;
/// The app's own rule: always floating (on, then off again).
pub const TEST_FLOAT_RULE: usize = 5;

/// Where an app's menu was asked for: a bar (by index) or Rıhtım's dock.
#[derive(Clone, Copy, Debug)]
pub(super) enum MenuHost {
    Bar(usize),
    Dock,
}

/// What the menu is placed against: the edge and strip of the bar or dock, the app's
/// rectangle on it, its scale and its monitor (screen pixels).
struct Anchor {
    edge: kabuk_core::config::Edge,
    strip: PxRect,
    app: PxRect,
    scale: Scale,
    monitor: PxRect,
}

pub(super) struct AppMenu {
    _shown: Shown,
    layout: Layout,
    scale: Scale,
    /// The app's index in the list and its entries, as drawn.
    app: usize,
    entries: Vec<Action>,
}

/// The name an app goes by, as Windows' taskbar names it: a packaged app's own name, a
/// program's name in the Start menu ("Google Chrome", not "chrome"), else its pin's or
/// program's file name without the extension.
fn display_name(app: &crate::bar::apps::App) -> String {
    let path = app.link.as_deref().unwrap_or(&app.source);
    /// How the shell names a packaged app by its app id.
    const APPS_FOLDER: &str = "shell:AppsFolder\\";
    if let Some(id) = app
        .icon
        .strip_prefix(APPS_FOLDER)
        .or_else(|| path.strip_prefix(APPS_FOLDER))
    {
        if let Some(a) = kabuk_win::shellitems::installed_apps_cached()
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(id))
        {
            return a.name.clone();
        }
    }
    if path.eq_ignore_ascii_case(&app.source) {
        if let Some(a) = kabuk_win::shellitems::program_entry(&app.source) {
            return a.name;
        }
    }
    std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

impl Shared {
    /// A right click on app `app`: its menu, or none if its menu was open. Another app's
    /// open menu gives way to this one at once (found by the user: the click only closed
    /// it, and a second was needed).
    pub(super) fn toggle_app_menu(&mut self, host: MenuHost, app: usize) {
        let same = self.app_menu.as_ref().is_some_and(|m| m.app == app);
        self.close_app_menu();
        if !same {
            self.open_app_menu(host, app, true);
        }
    }

    /// Where app `index`'s menu goes for `host`; `None` when that bar or the dock is gone.
    fn menu_anchor(&self, host: MenuHost, index: usize) -> Option<Anchor> {
        let value = u32::try_from(index).unwrap_or(u32::MAX);
        match host {
            MenuHost::Bar(bar) => {
                let b = self.bars.get(bar)?;
                Some(Anchor {
                    edge: b.edge,
                    strip: b.rect,
                    app: self.hit_rect(bar, hit::APP, value).unwrap_or(b.rect),
                    scale: b.scale,
                    monitor: super::px_rect(b.monitor.rect),
                })
            }
            MenuHost::Dock => {
                let d = self.dock.as_ref()?;
                Some(Anchor {
                    edge: d.edge,
                    strip: d.rect,
                    app: self.dock_hit_rect(value).unwrap_or(d.rect),
                    scale: d.scale,
                    monitor: super::px_rect(d.monitor.rect),
                })
            }
        }
    }

    pub(super) fn close_app_menu(&mut self) {
        if self.app_menu.take().is_some() {
            tracing::debug!("app menu closed");
        }
    }

    pub(super) fn open_app_menu(&mut self, host: MenuHost, index: usize, activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_volume_panel();
        let Some(app) = self.apps.list.get(index).cloned() else {
            return;
        };
        let exe = program_file(&app.source);
        let rules = &self.config().rules.user;
        let workspace = self.wm.as_ref().and_then(|l| {
            l.view()
                .windows
                .into_iter()
                .find(|w| app.windows.contains(&w.hwnd) && !w.minimized)
                .map(|w| w.workspace)
                .filter(|n| *n != kabuk_layout::desk::SCRATCHPAD)
        });
        let mut subject = Subject {
            name: display_name(&app),
            pinned: app.link.is_some(),
            windows: app.windows.len(),
            groups: kabuk_win::jumplist::jump_list(&self.app_ids_of(&app), app_menu::MAX_PER_GROUP),
            pictures: Vec::new(),
            app_picture: Some(self.picture_key(&app.icon)).filter(|k| self.gpu.images.contains(*k)),
            floats: exe
                .as_deref()
                .is_some_and(|e| kabuk_core::rules::floats_by_rule(rules, e)),
            rule_workspace: exe
                .as_deref()
                .and_then(|e| kabuk_core::rules::workspace_by_rule(rules, e)),
            exe,
            workspace,
        };
        let weak = self.me.clone();
        let window = match Window::input_surface(MENU_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_app_menu(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "app menu window failed");
                return;
            }
        };
        let Some(a) = self.menu_anchor(host, index) else { return };
        let scale = a.scale;
        subject.pictures = self.jump_pictures(&subject, scale);
        let node = app_menu::view(&self.i18n, self.look.style.text_scale, &TIP, &subject);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let rect = tooltip::place(a.edge, a.strip, a.app, size, scale.px(TIP.gap), a.monitor);
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "app menu surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "app menu not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        let entries: Vec<Action> = app_menu::entries(&subject).into_iter().map(|(_, _, a)| a).collect();
        tracing::info!(source = %app.source, entries = entries.len(), "app menu opened");
        self.app_menu = Some(AppMenu {
            _shown: shown,
            layout,
            scale,
            app: index,
            entries,
        });
    }

    /// Each jump list entry's picture, as Windows' taskbar shows it (a site's favicon, a
    /// task's program, a file's or folder's own icon), made once per picture and size.
    fn jump_pictures(&mut self, subject: &Subject, scale: kabuk_ui::geom::Scale) -> Vec<Option<u64>> {
        let side = (app_menu::PICTURE * scale.0).ceil() as u32;
        subject
            .groups
            .iter()
            .flat_map(|g| g.entries.iter().take(app_menu::MAX_PER_GROUP))
            .map(|e| {
                let (file, index) = e.picture();
                let key = crate::bar::apps::image_key(&format!("jump|{file}|{index:?}|{side}"));
                if self.gpu.images.contains(key) {
                    return Some(key);
                }
                let path = std::path::Path::new(file);
                let px = match index {
                    Some(i) => kabuk_win::icon::icon_in_file_pixels(path, i, side),
                    None => kabuk_win::icon::file_icon_pixels(path, side),
                }?;
                self.gpu.images.set(key, side, px);
                Some(key)
            })
            .collect()
    }

    /// The ids Windows may keep the app's jump list by: its window's, else its pin's, else
    /// the ones Windows makes up for a program that sets none.
    fn app_ids_of(&self, app: &crate::bar::apps::App) -> Vec<String> {
        let own = app
            .windows
            .first()
            .and_then(|h| kabuk_win::shellitems::window_app_id(Hwnd(*h)))
            .or_else(|| {
                let link = app.link.as_deref()?;
                self.apps
                    .pins
                    .iter()
                    .find(|p| p.link.eq_ignore_ascii_case(link))?
                    .app_id
                    .clone()
            });
        // A program pinned by its path (from a window): its Start menu entry's id, as the
        // id it sets itself is that one ("Chrome").
        let own = own.or_else(|| kabuk_win::shellitems::program_entry(&app.source).map(|a| a.id));
        match own {
            Some(id) => vec![id],
            None => kabuk_win::jumplist::implicit_app_ids(&app.source),
        }
    }

    fn app_action(&mut self, index: usize, action: Action) {
        self.close_app_menu();
        let Some(app) = self.apps.list.get(index).cloned() else {
            return;
        };
        tracing::info!(source = %app.source, ?action, "app menu entry");
        match action {
            Action::Launch(launch) => {
                let program = app.source.clone();
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    let started = match &launch {
                        Launch::File(path) => kabuk_win::shellitems::open_with(&program, path),
                        Launch::Run { path, args, dir } => kabuk_win::jumplist::run(path, args, dir),
                    };
                    if let Err(e) = started {
                        tracing::warn!(error = %e, "jump list entry not started");
                    }
                });
            }
            Action::Open => {
                let target = app.link.clone().unwrap_or_else(|| app.source.clone());
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(&target) {
                        tracing::warn!(error = %e, "new window not opened");
                    }
                });
            }
            Action::Pin => {
                let pins = crate::bar::apps::pinned(&self.apps.pins, &app);
                self.save_pins(pins);
            }
            Action::Unpin => {
                let pins = crate::bar::apps::unpinned(&self.apps.pins, &app);
                self.save_pins(pins);
            }
            Action::Close => {
                for h in &app.windows {
                    Hwnd(*h).close_async();
                }
            }
            Action::FloatRule | Action::WorkspaceRule(_) => {
                let Some(exe) = program_file(&app.source) else { return };
                self.edit_own_rule(&exe, |r| match action {
                    Action::FloatRule => r.float = !r.float,
                    Action::WorkspaceRule(n) => r.workspace = if r.workspace == Some(n) { None } else { Some(n) },
                    _ => {}
                });
            }
        }
    }

    /// Changes the user's own rule for program `exe` in the configuration file (the
    /// settings' rules page shows it; Kabuk reloads the file and opens its next windows
    /// by it, DECISIONS D63).
    fn edit_own_rule(&mut self, exe: &str, change: impl FnOnce(&mut kabuk_core::config::Rule)) {
        // The file as it is now: a change made a moment ago may not be read back yet.
        let source = std::fs::read_to_string(&self.config_file).unwrap_or_default();
        let mut rules = kabuk_core::config::parse(&source)
            .map(|l| l.config.rules.user)
            .unwrap_or_else(|_| self.config().rules.user.clone());
        kabuk_core::rules::set_own_rule(&mut rules, exe, change);
        let value = serde_json::to_value(&rules).unwrap_or(serde_json::Value::Null);
        let written = kabuk_core::config::edit::set(&source, "rules.rule", &value)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                kabuk_core::fsutil::write_atomic(&self.config_file, text.as_bytes()).map_err(|e| e.to_string())
            });
        match written {
            Ok(()) => tracing::info!(exe, rules = rules.len(), "app rule saved"),
            Err(e) => tracing::warn!(exe, error = %e, "app rule not saved"),
        }
    }

    #[cfg(feature = "integration")]
    pub(super) fn test_app_menu(&mut self, what: usize, hwnd: u64) {
        if what == TEST_CLOSE {
            self.close_app_menu();
            return;
        }
        self.refresh_apps();
        let Some(index) = self.apps.list.iter().position(|a| a.windows.contains(&hwnd)) else {
            tracing::warn!(hwnd, "test: no app holds the window");
            return;
        };
        let Some(bar) = self.bars.iter().position(|b| b.primary) else {
            return;
        };
        match what {
            TEST_OPEN => self.open_app_menu(MenuHost::Bar(bar), index, false),
            TEST_PIN | TEST_CLOSE_WINDOWS => {
                let pinned = self.apps.list[index].link.is_some();
                let action = match what {
                    TEST_PIN if pinned => Action::Unpin,
                    TEST_PIN => Action::Pin,
                    _ => Action::Close,
                };
                self.app_action(index, action);
            }
            TEST_TO_FRONT => self.app_dragged(index, 0),
            TEST_FLOAT_RULE => self.app_action(index, Action::FloatRule),
            _ => {}
        }
    }

    pub(super) fn on_app_menu(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let p = self.app_menu.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let i = p.layout.hit_test(at).filter(|h| h.module == app_menu::HIT)?.value as usize;
                let action = p.entries.get(i)?.clone();
                let app = p.app;
                self.app_action(app, action);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_app_menu();
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_app_menu();
                }
                None
            }
            _ => None,
        }
    }
}

/// The program file's name of an app (rules match it); `None` for one that is not a
/// program file (a packaged app's id).
fn program_file(source: &str) -> Option<String> {
    let name = source.rsplit(['\\', '/']).next()?;
    name.to_ascii_lowercase().ends_with(".exe").then(|| name.to_string())
}
