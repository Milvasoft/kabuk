//! The volume panel on the bar thread ([`crate::bar::volume_panel`]): placed like the
//! power menu under the volume; a press on a level (the output's or an app's in the
//! mixer) sets it and a drag follows the mouse, the speaker mutes the output and an
//! app's icon the app, an entry opens Windows' page; a click elsewhere or Esc closes it.
//! The level is redrawn whenever the output's changes, and the mixer whenever an app's
//! session does (Core Audio tells; no polling). The keyboard moves over its parts as in
//! every popup ([`crate::bar::popup_keys`]): Enter or Space clicks the one selected, and
//! Left and Right move a selected level a wheel notch's step.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::popup_keys::{self, Step};
use crate::bar::volume_panel::{
    self, Menu, MixerRow, MixerView, FIRST_APP, FIRST_APP_MUTE, FIRST_ENTRY, FIRST_OUTPUT, MUTE, TRACK,
};
use crate::notify::style::NotifyStyle;

const PANEL_CLASS: &str = "Kabuk.VolumePanel";
/// Integration tests open (`wparam` 1: with the track, 2: the volume's right click menu,
/// 3: the clock's) or close (0) the panel without taking the keyboard; 4 right-clicks the
/// first empty spot of the primary bar (its menu takes the keyboard, as a click's does).
pub const MSG_TEST_VOLUME_PANEL: u32 = msg::APP + 35;
/// `kabuk msg panel volume`: open or close the level and mixer on the bar of the monitor
/// in use, as a left click on the volume does (posted to the control window).
pub const MSG_OPEN_VOLUME_PANEL: u32 = msg::APP + 84;

pub(super) struct VolumePanel {
    shown: Shown,
    surface_size: (i32, i32),
    layout: Layout,
    scale: Scale,
    with_track: bool,
    menu: Menu,
    /// The bar and the icon it opened under, to be placed again when it changes size.
    bar: usize,
    anchor: (&'static str, u32),
    /// The level held (the output's [`TRACK`] or an app's): a drag sets it as the mouse
    /// moves.
    dragging: Option<u32>,
    /// The mixer's apps, and the watch on their sessions while the panel is open.
    mixer: Vec<MixerRow>,
    _sessions: Option<kabuk_win::audio::SessionWatch>,
    /// The part the keyboard is on.
    selected: Option<u32>,
}

/// Whether part `h` is a level: the output's track or an app's in the mixer.
fn is_level(h: u32) -> bool {
    h == TRACK || (FIRST_APP..FIRST_APP_MUTE).contains(&h)
}

impl Shared {
    /// The volume's panel (its level and mixer when `with_track`); with the keyboard when
    /// `activate`.
    pub(super) fn toggle_volume_panel(&mut self, bar: usize, with_track: bool, activate: bool) {
        self.toggle_menu(bar, Menu::Volume, with_track, (hit::VOLUME, 0), activate);
    }

    /// Opens an icon's menu under `anchor` (its hit), or closes it if it was the one open.
    /// Another icon's open menu gives way to this one at once (found by the user: the
    /// click only closed it, and a second was needed).
    pub(super) fn toggle_system_menu(&mut self, bar: usize, menu: Menu, with_track: bool, anchor: (&'static str, u32)) {
        self.toggle_menu(bar, menu, with_track, anchor, true);
    }

    fn toggle_menu(&mut self, bar: usize, menu: Menu, with_track: bool, anchor: (&'static str, u32), activate: bool) {
        let same = self
            .volume_panel
            .as_ref()
            .is_some_and(|p| p.menu == menu && p.with_track == with_track);
        self.close_volume_panel();
        if !same {
            self.open_system_menu(bar, menu, with_track, anchor, activate);
        }
    }

    pub(super) fn close_volume_panel(&mut self) {
        if self.volume_panel.take().is_some() {
            tracing::debug!("volume panel closed");
        }
    }

    /// The mixer's apps now (the volume's left click only).
    fn mixer_now(&self, menu: Menu, with_track: bool) -> Vec<MixerRow> {
        match (&self.audio, menu, with_track) {
            (Some(a), Menu::Volume, true) => volume_panel::mixer_rows(&a.sessions()),
            _ => Vec::new(),
        }
    }

    /// The panel's tree, with part `selected` drawn selected.
    fn volume_view(
        &self,
        menu: Menu,
        with_track: bool,
        mixer: &[MixerRow],
        selected: Option<u32>,
    ) -> kabuk_ui::node::Node {
        let level = with_track.then(|| self.live.volume.map_or((0, false), |v| (v.percent, v.muted)));
        let outputs: Vec<(String, bool)> = self
            .outputs(menu, with_track)
            .into_iter()
            .map(|o| (o.name, o.default))
            .collect();
        let apps: Vec<MixerView> = mixer
            .iter()
            .map(|r| {
                let (name, image) = self.program_look(&r.exe, &r.session_name);
                MixerView {
                    name,
                    image,
                    percent: r.percent,
                    muted: r.muted,
                }
            })
            .collect();
        let node = volume_panel::view(
            &self.i18n,
            self.look.style.text_scale,
            &TIP,
            &NotifyStyle::of(self.config().appearance.look),
            level,
            &outputs,
            &apps,
            menu.entries(),
        );
        popup_keys::marked(node, volume_panel::HIT, selected, &TIP)
    }

    /// The output devices the panel lists (the volume's left click only).
    fn outputs(&self, menu: Menu, with_track: bool) -> Vec<kabuk_win::audio::Output> {
        match (&self.audio, menu, with_track) {
            (Some(a), Menu::Volume, true) => a.outputs(),
            _ => Vec::new(),
        }
    }

    /// Opens the menu, or lays it out again in the window it has (it changed size: an
    /// app joined or left the mixer), keeping the keyboard and not entering again.
    pub(super) fn open_system_menu(
        &mut self,
        bar: usize,
        menu: Menu,
        with_track: bool,
        anchor: (&'static str, u32),
        activate: bool,
    ) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_network_panel();
        self.close_keyboard_panel();
        self.close_privacy_panel();
        self.close_calendar_panel();
        self.close_quick_panel();
        let mixer = self.mixer_now(menu, with_track);
        // Open already: drawn again in place (`Ok`), else a new window (`Err`); the
        // keyboard stays on its part.
        let mut selected = None;
        let (held, again, sessions) = match self.volume_panel.take() {
            Some(p) => {
                selected = p.selected;
                (Ok(p.shown), true, p._sessions)
            }
            None => {
                let weak = self.me.clone();
                match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
                    let rc = weak.upgrade()?;
                    let mut s = rc.try_borrow_mut().ok()?;
                    s.on_volume_panel(m)
                }) {
                    Ok(w) => (Err(w), false, None),
                    Err(e) => {
                        tracing::warn!(error = %e, "volume panel window failed");
                        return;
                    }
                }
            }
        };
        // The sessions are watched while the mixer shows; a new app's needs its own.
        let sessions = if !(menu == Menu::Volume && with_track) {
            None
        } else if again {
            sessions
        } else {
            self.audio.as_ref().and_then(kabuk_win::audio::Audio::watch_sessions)
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let layout = kabuk_ui::node::layout(
            &self.volume_view(menu, with_track, &mixer, selected),
            Point::default(),
            &self.gpu.text,
        );
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let (module, value) = anchor;
        let button = self.hit_rect(bar, module, value).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            button,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        let shown = match held {
            Ok(mut shown) => {
                shown.redraw(&self.gpu, rect, &layout, &self.look.palette, scale);
                shown
            }
            Err(window) => {
                window.set_rect(rect);
                let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
                    .map_err(|e| tracing::warn!(error = %e, "volume panel surface failed"))
                    .ok();
                if let Some(s) = surface.as_mut() {
                    if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                        tracing::warn!(error = %e, "volume panel not drawn");
                    }
                }
                Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level())
            }
        };
        let _ = self.gpu.commit();
        if !again {
            if activate {
                shown.window().activate();
            } else {
                shown.window().show();
            }
        }
        tracing::info!(?menu, with_track, apps = mixer.len(), again, "volume panel opened");
        self.volume_panel = Some(VolumePanel {
            shown,
            surface_size: (rect.w, rect.h),
            layout,
            scale,
            with_track,
            menu,
            bar,
            anchor,
            dragging: None,
            mixer,
            _sessions: sessions,
            selected,
        });
    }

    /// Draws the panel again with the output's level now (it changed).
    pub(super) fn redraw_volume_panel(&mut self) {
        let Some(p) = self.volume_panel.as_ref() else {
            return;
        };
        let layout = kabuk_ui::node::layout(
            &self.volume_view(p.menu, p.with_track, &p.mixer, p.selected),
            Point::default(),
            &self.gpu.text,
        );
        let Some(p) = self.volume_panel.as_mut() else { return };
        let (w, h) = p.surface_size;
        if let Some(s) = p.shown.surface_mut() {
            if (w, h) != (0, 0) {
                if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, p.scale, (0.0, 0.0), None) {
                    tracing::warn!(error = %e, "volume panel not redrawn");
                }
            }
        }
        p.layout = layout;
        let _ = self.gpu.commit();
    }

    /// An app's session changed (Core Audio told): the mixer shows the levels now, and
    /// is laid out again when an app joined or left it.
    pub(super) fn mixer_changed(&mut self) {
        let Some(p) = self.volume_panel.as_ref() else { return };
        let (menu, with_track, bar, anchor) = (p.menu, p.with_track, p.bar, p.anchor);
        let now = self.mixer_now(menu, with_track);
        let programs = |rows: &[MixerRow]| {
            rows.iter()
                .map(|r| (r.exe.to_lowercase(), r.pids.clone()))
                .collect::<Vec<_>>()
        };
        if programs(&now) != programs(&p.mixer) {
            tracing::debug!(apps = now.len(), "volume mixer changed");
            // A new session is watched too: the watch starts again with the panel's
            // window kept.
            if let Some(p) = self.volume_panel.as_mut() {
                p._sessions = None;
                p._sessions = self.audio.as_ref().and_then(kabuk_win::audio::Audio::watch_sessions);
            }
            self.open_system_menu(bar, menu, with_track, anchor, false);
        } else if let Some(p) = self.volume_panel.as_mut() {
            if p.mixer != now {
                p.mixer = now;
                self.redraw_volume_panel();
            }
        }
    }

    /// Sets level `which` (the output's [`TRACK`] or an app's) from a press or a drag at
    /// `x` (the panel's pixels).
    fn level_from(&mut self, which: u32, x: i32) {
        let Some(p) = self.volume_panel.as_ref() else { return };
        let Some((track, _)) = p
            .layout
            .hits
            .iter()
            .find(|(_, h)| h.module == volume_panel::HIT && h.value == which)
        else {
            return;
        };
        let percent = volume_panel::percent_at(p.scale.dips(x), *track);
        if which == TRACK {
            if let Some(a) = &self.audio {
                a.set_percent(percent);
                self.live.volume = a.volume();
            }
        } else {
            let i = usize::try_from(which - FIRST_APP).unwrap_or(usize::MAX);
            let Some(row) = self.volume_panel.as_mut().and_then(|p| p.mixer.get_mut(i)) else {
                return;
            };
            row.percent = percent;
            let pids = row.pids.clone();
            if let Some(a) = &self.audio {
                a.set_sessions_percent(&pids, percent);
            }
        }
        self.redraw_volume_panel();
    }

    /// A click on part `h` that is not a level (or Enter on it): mutes, an output, a
    /// settings page.
    fn volume_panel_click(&mut self, h: u32) -> Option<()> {
        let p = self.volume_panel.as_mut()?;
        match h {
            MUTE => {
                if let Some(a) = &self.audio {
                    a.toggle_mute();
                    self.live.volume = a.volume();
                }
                self.redraw_volume_panel();
                self.render_all(false);
            }
            level if is_level(level) => {}
            app if app >= FIRST_APP_MUTE => {
                let row = p.mixer.get_mut(usize::try_from(app - FIRST_APP_MUTE).ok()?)?;
                row.muted = !row.muted;
                let pids = row.pids.clone();
                tracing::info!(exe = %row.exe, muted = row.muted, "app muted from the mixer");
                if let Some(a) = &self.audio {
                    a.toggle_sessions_mute(&pids);
                }
                self.redraw_volume_panel();
            }
            output if output >= FIRST_OUTPUT => {
                let (menu, with_track) = (p.menu, p.with_track);
                let chosen = self
                    .outputs(menu, with_track)
                    .into_iter()
                    .nth(usize::try_from(output - FIRST_OUTPUT).ok()?)?;
                tracing::info!(name = %chosen.name, "audio output chosen");
                if let Err(e) = kabuk_win::audio::set_default_output(&chosen.id) {
                    tracing::warn!(error = %e, "audio output not changed");
                    self.notify(crate::notify::Notice::Failed(crate::notify::Failure::Output));
                }
                // The level is the new device's: Core Audio says so, and the panel is
                // drawn again then; this redraw shows the choice now.
                self.redraw_volume_panel();
            }
            entry => {
                let (_, _, page) = *p.menu.entries().get(usize::try_from(entry - FIRST_ENTRY).ok()?)?;
                self.close_volume_panel();
                tracing::info!(page, "volume panel entry");
                if page == volume_panel::KABUK_SETTINGS {
                    if let Err(e) = self.open_settings() {
                        tracing::warn!(error = %e, "settings not opened");
                        self.notify(crate::notify::Notice::Failed(crate::notify::Failure::Settings));
                    }
                } else {
                    self.open_off_thread(page.into(), crate::notify::Failure::Settings);
                }
            }
        }
        Some(())
    }

    /// Level `which` (the output's [`TRACK`] or an app's) moved `way` (-1, +1) steps of a
    /// wheel notch.
    fn step_volume_level(&mut self, which: u32, way: i32) {
        if which == TRACK {
            if let Some(a) = &self.audio {
                a.step(way * super::VOLUME_STEP);
                self.live.volume = a.volume();
            }
            self.render_all(false);
        } else if which >= FIRST_APP {
            let i = usize::try_from(which - FIRST_APP).unwrap_or(usize::MAX);
            let Some(row) = self.volume_panel.as_mut().and_then(|p| p.mixer.get_mut(i)) else {
                return;
            };
            row.percent = popup_keys::stepped(row.percent, way, super::VOLUME_STEP);
            if let Some(a) = &self.audio {
                a.set_sessions_percent(&row.pids, row.percent);
            }
        }
        self.redraw_volume_panel();
    }

    /// A key on the panel other than Esc.
    fn volume_panel_key(&mut self, key: popup_keys::Key) {
        let Some(p) = self.volume_panel.as_mut() else { return };
        match popup_keys::step(&popup_keys::entries(&p.layout, volume_panel::HIT), p.selected, key) {
            Step::Select(h) => {
                p.selected = Some(h);
                self.redraw_volume_panel();
            }
            Step::Activate(h) if !is_level(h) => {
                self.volume_panel_click(h);
            }
            Step::Adjust(h, way) if is_level(h) => self.step_volume_level(h, way),
            Step::Activate(_) | Step::Adjust(..) | Step::Same => {}
        }
    }

    pub(super) fn on_volume_panel(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONDOWN => {
                let p = self.volume_panel.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let h = p.layout.hit_test(at).filter(|h| h.module == volume_panel::HIT)?.value;
                if is_level(h) {
                    if let Some(p) = self.volume_panel.as_mut() {
                        p.dragging = Some(h);
                        p.shown.window().capture_mouse(true);
                    }
                    self.level_from(h, x);
                }
                Some(0)
            }
            msg::MOUSEMOVE => {
                if let Some(h) = self.volume_panel.as_ref().and_then(|p| p.dragging) {
                    self.level_from(h, msg::point(m.lparam).0);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let p = self.volume_panel.as_mut()?;
                if p.dragging.take().is_some() {
                    p.shown.window().capture_mouse(false);
                    return Some(0);
                }
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let h = p.layout.hit_test(at).filter(|h| h.module == volume_panel::HIT)?.value;
                self.volume_panel_click(h);
                Some(0)
            }
            msg::MOUSEWHEEL if self.volume_panel.as_ref().is_some_and(|p| p.menu == Menu::Volume) => {
                let notches = msg::wheel(m.wparam) / msg::WHEEL_DELTA;
                if let Some(a) = &self.audio {
                    a.step(notches * super::VOLUME_STEP);
                    self.live.volume = a.volume();
                }
                self.redraw_volume_panel();
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_volume_panel();
                Some(0)
            }
            msg::KEYDOWN => {
                let key = popup_keys::Key::of(m.wparam, msgloop::shift_held(), false)?;
                self.volume_panel_key(key);
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE
                    && !self.volume_panel.as_ref().is_some_and(|p| p.dragging.is_some())
                {
                    self.close_volume_panel();
                }
                None
            }
            _ => None,
        }
    }
}
