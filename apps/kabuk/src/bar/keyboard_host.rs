//! The bar's keyboard language (DECISIONS D88): the input language of the window in
//! front, read when the window in front changes and after one of Windows' language
//! chords (seen by the keyboard hook, which lets it through), and its panel, which lists
//! the languages and switches the window that was in front. The keyboard selects and
//! chooses its rows as in every popup ([`crate::bar::popup_keys`]; a digit chooses the
//! language at that place).

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};
use kabuk_win::window::Hwnd;

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::{timer, Shared, MSG_LANGUAGE_KEYS};
use crate::bar::keyboard_panel::{self, Row, SETTINGS, SETTINGS_PAGE};
use crate::bar::popup_keys::{self, Step};

const PANEL_CLASS: &str = "Kabuk.KeyboardPanel";
/// Integration tests open (`wparam` 1) or close (0) the panel without taking the keyboard.
pub const MSG_TEST_KEYBOARD_PANEL: u32 = msg::APP + 61;
/// After a language chord, Windows switches the window in front a moment later: the
/// language is read then.
pub(super) const KEYBOARD_READ_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

pub(super) struct KeyboardPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    /// The rows drawn, to draw them again with another selected, and the one the
    /// keyboard is on.
    rows: Vec<Row>,
    selected: Option<u32>,
    /// The languages listed, and the window that was in front when it opened (the one a
    /// choice switches).
    languages: Vec<kabuk_win::input_lang::InputLang>,
    target: Option<Hwnd>,
}

impl Shared {
    /// Starts or stops following the input language with the module.
    pub(super) fn watch_keyboard(&mut self) {
        if !self.config().bar.modules.keyboard {
            self.language_keys = None;
            self.live.keyboard = None;
            self.close_keyboard_panel();
            return;
        }
        if self.language_keys.is_none() && self.shortcuts {
            if let Some(control) = self.control.as_ref().map(Window::hwnd) {
                self.language_keys = kabuk_win::keyhook::LanguageKeys::watch(control, MSG_LANGUAGE_KEYS)
                    .map_err(|e| tracing::warn!(error = %e, "language chords not seen"))
                    .ok();
            }
        }
        self.read_keyboard();
    }

    /// Reads the input language in front; true when what the bar shows changed.
    pub(super) fn read_keyboard(&mut self) -> bool {
        if !self.config().bar.modules.keyboard {
            return false;
        }
        // The installed languages change only in Windows' settings: read once, and again
        // after a language chord or a settings change (PLAN M19 P18: read on every focus).
        let all = self
            .input_langs
            .get_or_insert_with(kabuk_win::input_lang::installed)
            .clone();
        let shown = kabuk_core::input_lang::shown(all.len())
            .then(|| {
                let now = kabuk_win::input_lang::current()?;
                let id = kabuk_core::input_lang::language_id(now);
                all.iter()
                    .find(|l| kabuk_core::input_lang::language_id(l.hkl) == id)
                    .map(|l| kabuk_core::input_lang::short(&l.locale))
            })
            .flatten();
        if shown == self.live.keyboard {
            return false;
        }
        tracing::debug!(language = ?shown, "input language");
        self.live.keyboard = shown;
        true
    }

    /// A language chord ended: the language is read once Windows has switched.
    pub(super) fn language_chord(&mut self) {
        self.input_langs = None;
        if let Some(c) = &self.control {
            c.set_timer(timer::KEYBOARD_READ, KEYBOARD_READ_AFTER);
        }
    }

    pub(super) fn keyboard_read_due(&mut self) {
        self.kill(timer::KEYBOARD_READ);
        if self.read_keyboard() {
            self.render_all(false);
        }
    }

    pub(super) fn toggle_keyboard_panel(&mut self, bar: usize, activate: bool) {
        let open = self.keyboard_panel.is_some();
        self.close_keyboard_panel();
        if !open {
            self.open_keyboard_panel(bar, activate);
        }
    }

    pub(super) fn close_keyboard_panel(&mut self) {
        if self.keyboard_panel.take().is_some() {
            tracing::debug!("keyboard panel closed");
        }
    }

    fn open_keyboard_panel(&mut self, bar: usize, activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_calendar_panel();
        self.close_privacy_panel();
        self.close_network_panel();
        self.close_media_card();
        let target = kabuk_win::events::foreground();
        let languages = kabuk_win::input_lang::installed();
        let now = kabuk_win::input_lang::current().map(kabuk_core::input_lang::language_id);
        let rows: Vec<Row> = languages
            .iter()
            .map(|l| Row {
                code: kabuk_core::input_lang::short(&l.locale),
                name: l.name.clone(),
                current: Some(kabuk_core::input_lang::language_id(l.hkl)) == now,
            })
            .collect();
        let weak = self.me.clone();
        let window = match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_keyboard_panel(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "keyboard panel window failed");
                return;
            }
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let layout = self.keyboard_panel_layout(&rows, None);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let chip = self.hit_rect(bar, hit::KEYBOARD, 0).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            chip,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "keyboard panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "keyboard panel not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(languages = rows.len(), "keyboard panel opened");
        self.keyboard_panel = Some(KeyboardPanel {
            shown,
            layout,
            scale,
            rows,
            selected: None,
            languages,
            target,
        });
        self.tell_keyboard_panel();
    }

    /// Tells screen readers its languages and link by their words (D97).
    fn tell_keyboard_panel(&self) {
        let Some(p) = self.keyboard_panel.as_ref() else { return };
        let name = self.i18n.tr("mod-keyboard");
        let drawn = (&p.layout, p.scale);
        super::access_host::tell_menu(&p.shown, &name, drawn, keyboard_panel::HIT, p.selected);
    }

    /// The panel laid out with row `selected` drawn selected.
    fn keyboard_panel_layout(&self, rows: &[Row], selected: Option<u32>) -> Layout {
        let node = keyboard_panel::view(rows, &self.i18n, self.look.style.text_scale, &TIP);
        let node = popup_keys::marked(node, keyboard_panel::HIT, selected, &TIP);
        kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text)
    }

    /// A click on row `value` (or Enter on it, or its digit): a language, or the settings.
    fn keyboard_panel_click(&mut self, value: u32) {
        let Some(p) = self.keyboard_panel.as_ref() else { return };
        let chosen = usize::try_from(value)
            .ok()
            .and_then(|i| p.languages.get(i))
            .map(|l| l.hkl);
        let target = p.target;
        self.close_keyboard_panel();
        if value == SETTINGS {
            tracing::info!(page = SETTINGS_PAGE, "keyboard panel link");
            self.open_off_thread(SETTINGS_PAGE.into(), crate::notify::Failure::Settings);
        } else if let (Some(hkl), Some(window)) = (chosen, target) {
            tracing::info!("input language chosen");
            kabuk_win::input_lang::switch(window, hkl);
            self.language_chord();
        }
    }

    pub(super) fn on_keyboard_panel(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let p = self.keyboard_panel.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let value = p.layout.hit_test(at).filter(|h| h.module == keyboard_panel::HIT)?.value;
                self.keyboard_panel_click(value);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_keyboard_panel();
                Some(0)
            }
            msg::KEYDOWN => {
                let key = popup_keys::Key::of(m.wparam, msgloop::shift_held(), true)?;
                let p = self.keyboard_panel.as_ref()?;
                match popup_keys::step(&popup_keys::entries(&p.layout, keyboard_panel::HIT), p.selected, key) {
                    Step::Select(v) => {
                        let layout = self.keyboard_panel_layout(&p.rows, Some(v));
                        let p = self.keyboard_panel.as_mut()?;
                        p.shown.repaint(&self.gpu, &layout, &self.look.palette, p.scale);
                        let _ = self.gpu.commit();
                        p.layout = layout;
                        p.selected = Some(v);
                        self.tell_keyboard_panel();
                    }
                    Step::Activate(v) => self.keyboard_panel_click(v),
                    Step::Same | Step::Adjust(..) => {}
                }
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_keyboard_panel();
                }
                None
            }
            _ => None,
        }
    }

    /// Integration tests: open (`true`) or close the panel.
    #[cfg(feature = "integration")]
    pub(super) fn test_keyboard_panel(&mut self, open: bool) {
        self.close_keyboard_panel();
        if open {
            let bar = self.bars.iter().position(|b| b.primary).unwrap_or(0);
            self.open_keyboard_panel(bar, false);
        }
    }
}
