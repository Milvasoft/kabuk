//! The volume panel on the bar thread ([`crate::bar::volume_panel`]): placed like the
//! power menu under the volume; a press on the track sets the level and a drag follows
//! the mouse, the speaker mutes, an entry opens Windows' page; a click elsewhere or Esc
//! closes it. The level is redrawn whenever the output's changes.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::volume_panel::{self, Menu, FIRST_ENTRY, MUTE, TRACK};
use crate::notify::style::NotifyStyle;

const PANEL_CLASS: &str = "Kabuk.VolumePanel";
/// Integration tests open (`wparam` 1: with the track, 2: the volume's right click menu,
/// 3: the clock's) or close (0) the panel without taking the keyboard.
pub const MSG_TEST_VOLUME_PANEL: u32 = msg::APP + 35;

pub(super) struct VolumePanel {
    shown: Shown,
    surface_size: (i32, i32),
    layout: Layout,
    scale: Scale,
    with_track: bool,
    menu: Menu,
    /// The track is held (a drag sets the level as the mouse moves).
    dragging: bool,
}

impl Shared {
    pub(super) fn toggle_volume_panel(&mut self, bar: usize, with_track: bool) {
        self.toggle_system_menu(bar, Menu::Volume, with_track, (hit::VOLUME, 0));
    }

    /// Opens an icon's menu under `anchor` (its hit), or closes it if it was the one open.
    /// Another icon's open menu gives way to this one at once (found by the user: the
    /// click only closed it, and a second was needed).
    pub(super) fn toggle_system_menu(&mut self, bar: usize, menu: Menu, with_track: bool, anchor: (&str, u32)) {
        let same = self
            .volume_panel
            .as_ref()
            .is_some_and(|p| p.menu == menu && p.with_track == with_track);
        self.close_volume_panel();
        if !same {
            self.open_system_menu(bar, menu, with_track, anchor, true);
        }
    }

    pub(super) fn close_volume_panel(&mut self) {
        if self.volume_panel.take().is_some() {
            tracing::debug!("volume panel closed");
        }
    }

    fn volume_view(&self, menu: Menu, with_track: bool) -> kabuk_ui::node::Node {
        let level = with_track.then(|| self.live.volume.map_or((0, false), |v| (v.percent, v.muted)));
        volume_panel::view(
            &self.i18n,
            self.look.style.text_scale,
            &TIP,
            &NotifyStyle::of(self.config().appearance.look),
            level,
            menu.entries(),
        )
    }

    pub(super) fn open_system_menu(
        &mut self,
        bar: usize,
        menu: Menu,
        with_track: bool,
        (module, value): (&str, u32),
        activate: bool,
    ) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        let weak = self.me.clone();
        let window = match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_volume_panel(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "volume panel window failed");
                return;
            }
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let layout = kabuk_ui::node::layout(&self.volume_view(menu, with_track), Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let button = self.hit_rect(bar, module, value).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            button,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "volume panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "volume panel not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(?menu, with_track, "volume panel opened");
        self.volume_panel = Some(VolumePanel {
            shown,
            surface_size: (rect.w, rect.h),
            layout,
            scale,
            with_track,
            menu,
            dragging: false,
        });
    }

    /// Draws the panel again with the output's level now (it changed).
    pub(super) fn redraw_volume_panel(&mut self) {
        let Some((menu, with_track)) = self.volume_panel.as_ref().map(|p| (p.menu, p.with_track)) else {
            return;
        };
        let layout = kabuk_ui::node::layout(&self.volume_view(menu, with_track), Point::default(), &self.gpu.text);
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

    /// Sets the level from a press or a drag at `x` (the panel's pixels).
    fn volume_from(&mut self, x: i32) {
        let Some(p) = self.volume_panel.as_ref() else { return };
        let Some((track, _)) = p
            .layout
            .hits
            .iter()
            .find(|(_, h)| h.module == volume_panel::HIT && h.value == TRACK)
        else {
            return;
        };
        let percent = volume_panel::percent_at(p.scale.dips(x), *track);
        if let Some(a) = &self.audio {
            a.set_percent(percent);
            self.live.volume = a.volume();
        }
        self.redraw_volume_panel();
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
                if h == TRACK {
                    if let Some(p) = self.volume_panel.as_mut() {
                        p.dragging = true;
                        p.shown.window().capture_mouse(true);
                    }
                    self.volume_from(x);
                }
                Some(0)
            }
            msg::MOUSEMOVE => {
                if self.volume_panel.as_ref().is_some_and(|p| p.dragging) {
                    self.volume_from(msg::point(m.lparam).0);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let p = self.volume_panel.as_mut()?;
                if p.dragging {
                    p.dragging = false;
                    p.shown.window().capture_mouse(false);
                    return Some(0);
                }
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let h = p.layout.hit_test(at).filter(|h| h.module == volume_panel::HIT)?.value;
                match h {
                    MUTE => {
                        if let Some(a) = &self.audio {
                            a.toggle_mute();
                            self.live.volume = a.volume();
                        }
                        self.redraw_volume_panel();
                        self.render_all(false);
                    }
                    TRACK => {}
                    entry => {
                        let (_, _, page) = *p.menu.entries().get(usize::try_from(entry - FIRST_ENTRY).ok()?)?;
                        self.close_volume_panel();
                        tracing::info!(page, "volume panel entry");
                        std::thread::spawn(move || {
                            kabuk_win::system::com_init();
                            if let Err(e) = kabuk_win::shellitems::open(page) {
                                tracing::warn!(error = %e, page, "settings page not opened");
                            }
                        });
                    }
                }
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
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE && !self.volume_panel.as_ref().is_some_and(|p| p.dragging) {
                    self.close_volume_panel();
                }
                None
            }
            _ => None,
        }
    }
}
