//! The quick settings panel on the bar thread ([`crate::bar::quick_panel`], DECISIONS
//! D88): opened by the bar's quick settings button or the battery, placed under it. What
//! is slow to ask Windows (the radios, the hotspot, the brightness) is read and set on
//! threads of their own, and the panel is drawn again when they answer; the volume is
//! the bar's own. A level follows a press and a drag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::brightness;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::{Shared, MSG_QUICK_READ};
use crate::bar::quick_panel::{self, Action, Quick, Screen, Tile, BRIGHTNESS, VOLUME};
use crate::notify::style::NotifyStyle;

const PANEL_CLASS: &str = "Kabuk.QuickPanel";
/// Integration tests open (`wparam` 1) or close (0) the panel without taking the keyboard,
/// or click its part `lparam` (2).
pub const MSG_TEST_QUICK_PANEL: u32 = msg::APP + 68;

/// What the slow reads found, waiting for the bar's thread.
#[derive(Clone, Debug, Default)]
pub(super) struct QuickRead {
    radios: kabuk_win::radios::Radios,
    hotspot: Option<bool>,
    screens: Vec<(Screen, brightness::Panel)>,
}

/// Every monitor whose brightness answers, in Windows' order (the main one first), and
/// where each is set (DECISIONS D91). Slow: off the bar's thread.
fn read_screens() -> Vec<(Screen, brightness::Panel)> {
    kabuk_win::shell::monitors()
        .into_iter()
        .filter_map(|m| {
            brightness::read(m.handle, &m.device).map(|(panel, percent)| {
                (
                    Screen {
                        number: m.number,
                        main: m.primary,
                        percent,
                    },
                    panel,
                )
            })
        })
        .collect()
}

pub(super) type QuickSlot = Arc<Mutex<Option<QuickRead>>>;

pub(super) struct QuickPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
    anchor: (&'static str, u32),
    quick: Quick,
    /// Where each monitor's brightness is set (by its number), once read.
    panels: Vec<(u32, brightness::Panel)>,
    /// The level held by a press, followed while the mouse moves.
    dragging: Option<u32>,
    /// The brightness asked for (and where) while one is being set, and whether one is.
    pending: Arc<Mutex<Option<(brightness::Panel, u8)>>>,
    setting: Arc<AtomicBool>,
}

impl Shared {
    pub(super) fn toggle_quick_panel(&mut self, bar: usize, anchor: (&'static str, u32), activate: bool) {
        let open = self.quick_panel.is_some();
        self.close_quick_panel();
        if !open {
            self.open_quick_panel(bar, anchor, activate);
        }
    }

    pub(super) fn close_quick_panel(&mut self) {
        if self.quick_panel.take().is_some() {
            tracing::debug!("quick settings closed");
        }
    }

    /// What the bar knows at once; the slow parts come later.
    fn quick_now(&self, before: Option<&Quick>) -> Quick {
        let mut q = before.cloned().unwrap_or_default();
        q.volume = self.live.volume.map(|v| (v.percent, v.muted));
        q.battery = self.live.battery;
        q.ssid = match &self.live.network {
            Some(kabuk_core::network::Link::Wifi { ssid, .. }) => Some(ssid.clone()),
            _ => None,
        };
        q
    }

    /// Reads the radios, the hotspot and every monitor's brightness on a thread of its
    /// own; [`MSG_QUICK_READ`] says they came.
    fn read_quick(&self) {
        let slot = self.quick_slot.clone();
        let control = self.control.as_ref().map(Window::hwnd);
        std::thread::spawn(move || {
            kabuk_win::system::com_init_mta();
            let read = QuickRead {
                radios: kabuk_win::radios::read(),
                hotspot: kabuk_win::hotspot::state(),
                screens: read_screens(),
            };
            if let Ok(mut s) = slot.lock() {
                *s = Some(read);
            }
            if let Some(c) = control {
                let _ = msgloop::post_message(
                    c,
                    Msg {
                        id: MSG_QUICK_READ,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            }
        });
    }

    /// The slow parts came.
    pub(super) fn quick_read(&mut self) {
        let Some(read) = self.quick_slot.lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        let Some(bar) = self.quick_panel.as_ref().map(|p| p.bar) else {
            return;
        };
        // At first the level sets the monitor under the panel.
        let home = self.bars.get(bar).map_or(0, |b| b.monitor.number);
        let Some(p) = self.quick_panel.as_mut() else { return };
        let (screens, panels): (Vec<Screen>, Vec<(u32, brightness::Panel)>) = read
            .screens
            .into_iter()
            .map(|(s, panel)| (s, (s.number, panel)))
            .unzip();
        // A level being dragged keeps the value under the mouse.
        p.quick.take_screens(screens, home, p.dragging == Some(BRIGHTNESS));
        p.panels = panels;
        p.quick.wifi = read.radios.wifi;
        p.quick.bluetooth = read.radios.bluetooth;
        p.quick.airplane = read.radios.airplane();
        p.quick.hotspot = read.hotspot;
        let quick = p.quick.clone();
        tracing::debug!(?quick, "quick settings read");
        self.reshow_quick_panel(quick);
    }

    fn open_quick_panel(&mut self, bar: usize, anchor: (&'static str, u32), activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_network_panel();
        self.close_keyboard_panel();
        self.close_privacy_panel();
        self.close_calendar_panel();
        self.close_media_card();
        let quick = self.quick_now(None);
        self.show_quick_panel(bar, anchor, quick, false, activate);
        self.read_quick();
    }

    fn show_quick_panel(&mut self, bar: usize, anchor: (&'static str, u32), quick: Quick, again: bool, activate: bool) {
        if again {
            if let Some((layout, rect, scale)) = self.quick_place(bar, anchor, &quick) {
                if let Some(p) = self.quick_panel.as_mut() {
                    p.shown.redraw(&self.gpu, rect, &layout, &self.look.palette, scale);
                    let _ = self.gpu.commit();
                    p.layout = layout;
                    p.scale = scale;
                    p.quick = quick;
                    return;
                }
            }
        }
        let (window, panels, dragging, pending, setting) = match self.quick_panel.take() {
            Some(p) => (p.shown.into_window(), p.panels, p.dragging, p.pending, p.setting),
            None => {
                let weak = self.me.clone();
                match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
                    let rc = weak.upgrade()?;
                    let mut s = rc.try_borrow_mut().ok()?;
                    s.on_quick_panel(m)
                }) {
                    Ok(w) => (w, Vec::new(), None, Arc::default(), Arc::default()),
                    Err(e) => {
                        tracing::warn!(error = %e, "quick settings window failed");
                        return;
                    }
                }
            }
        };
        let Some((layout, rect, scale)) = self.quick_place(bar, anchor, &quick) else {
            return;
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "quick settings surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "quick settings not drawn");
            }
        }
        let level = if again { Level::Off } else { self.motion_level() };
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), level);
        let _ = self.gpu.commit();
        if !again {
            if activate {
                shown.window().activate();
            } else {
                shown.window().show();
            }
            tracing::info!("quick settings opened");
        }
        self.quick_panel = Some(QuickPanel {
            shown,
            layout,
            scale,
            bar,
            anchor,
            quick,
            panels,
            dragging,
            pending,
            setting,
        });
    }

    /// The panel laid out for `quick` and its place under `anchor` on bar `bar`.
    fn quick_place(&self, bar: usize, anchor: (&'static str, u32), quick: &Quick) -> Option<(Layout, PxRect, Scale)> {
        let b = self.bars.get(bar)?;
        let scale = b.scale;
        let node = quick_panel::view(
            quick,
            &self.i18n,
            self.look.style.text_scale,
            &TIP,
            &NotifyStyle::of(self.config().appearance.look),
        );
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let (module, value) = anchor;
        let at = self.hit_rect(bar, module, value).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            at,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        Some((layout, rect, scale))
    }

    fn reshow_quick_panel(&mut self, quick: Quick) {
        let Some((bar, anchor)) = self.quick_panel.as_ref().map(|p| (p.bar, p.anchor)) else {
            return;
        };
        self.show_quick_panel(bar, anchor, quick, true, false);
    }

    /// The volume changed elsewhere (keys, the wheel): the panel's level follows.
    pub(super) fn quick_volume_changed(&mut self) {
        let Some(before) = self.quick_panel.as_ref().map(|p| p.quick.clone()) else {
            return;
        };
        let now = self.quick_now(Some(&before));
        if now != before {
            self.reshow_quick_panel(now);
        }
    }

    fn quick_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Open(page) => {
                self.close_quick_panel();
                tracing::info!(page, "quick settings link");
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(page) {
                        tracing::warn!(error = %e, page, "settings page not opened");
                    }
                });
            }
            Action::Screen(i) => {
                let Some(p) = self.quick_panel.as_mut() else { return };
                p.quick.screen = i;
                let quick = p.quick.clone();
                tracing::info!(screen = ?quick.screens.get(i), "brightness of another monitor");
                self.reshow_quick_panel(quick);
            }
            Action::Switch(tile, on) => {
                tracing::info!(?tile, on, "quick settings switched");
                let Some(p) = self.quick_panel.as_mut() else { return };
                // Shown at once; Windows' answer is read again after.
                match tile {
                    Tile::Wifi => p.quick.wifi = Some(on),
                    Tile::Bluetooth => p.quick.bluetooth = Some(on),
                    Tile::Airplane => p.quick.airplane = Some(on),
                    Tile::Hotspot => p.quick.hotspot = Some(on),
                    _ => {}
                }
                let quick = p.quick.clone();
                self.reshow_quick_panel(quick);
                let slot = self.quick_slot.clone();
                let control = self.control.as_ref().map(Window::hwnd);
                std::thread::spawn(move || {
                    kabuk_win::system::com_init_mta();
                    let r = match tile {
                        Tile::Wifi => kabuk_win::radios::set_wifi(on),
                        Tile::Bluetooth => kabuk_win::radios::set_bluetooth(on),
                        Tile::Airplane => kabuk_win::radios::set_airplane(on),
                        Tile::Hotspot => kabuk_win::hotspot::set(on),
                        _ => Ok(()),
                    };
                    if let Err(e) = r {
                        tracing::warn!(error = %e, ?tile, on, "quick setting not switched");
                    }
                    let read = QuickRead {
                        radios: kabuk_win::radios::read(),
                        hotspot: kabuk_win::hotspot::state(),
                        screens: read_screens(),
                    };
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(read);
                    }
                    if let Some(c) = control {
                        let _ = msgloop::post_message(
                            c,
                            Msg {
                                id: MSG_QUICK_READ,
                                wparam: 0,
                                lparam: 0,
                            },
                        );
                    }
                });
            }
        }
    }

    /// Sets level `which` from a press or a drag at `x` (the panel's pixels).
    fn quick_level_from(&mut self, which: u32, x: i32) {
        let Some(p) = self.quick_panel.as_ref() else { return };
        let Some((track, _)) = p
            .layout
            .hits
            .iter()
            .find(|(_, h)| h.module == quick_panel::HIT && h.value == which)
        else {
            return;
        };
        let percent = quick_panel::level_at(p.scale.dips(x), *track);
        let mut quick = p.quick.clone();
        if which == VOLUME {
            if let Some(a) = &self.audio {
                a.set_percent(percent);
                self.live.volume = a.volume();
            }
            quick.volume = self.live.volume.map(|v| (v.percent, v.muted));
            self.render_all(false);
        } else {
            let value = u8::try_from(percent.min(100)).unwrap_or(100);
            let screen = quick.screen;
            let number = quick.screens.get_mut(screen).map(|s| {
                s.percent = value;
                s.number
            });
            let panel = p
                .panels
                .iter()
                .find(|(n, _)| Some(*n) == number)
                .map(|(_, panel)| *panel);
            if let Some(panel) = panel {
                if let Ok(mut pending) = p.pending.lock() {
                    *pending = Some((panel, value));
                }
                // One thread sets the latest value asked for while the mouse moves on.
                if !p.setting.swap(true, Ordering::SeqCst) {
                    let (pending, setting) = (p.pending.clone(), p.setting.clone());
                    std::thread::spawn(move || {
                        kabuk_win::system::com_init_mta();
                        loop {
                            let next = pending.lock().ok().and_then(|mut v| v.take());
                            let Some((panel, v)) = next else { break };
                            if let Err(e) = brightness::set(panel, v) {
                                tracing::warn!(error = %e, "brightness not set");
                            }
                        }
                        setting.store(false, Ordering::SeqCst);
                    });
                }
            }
        }
        self.reshow_quick_panel(quick);
    }

    pub(super) fn on_quick_panel(&mut self, m: Msg) -> Option<isize> {
        let is_level = |v: u32| v == BRIGHTNESS || v == VOLUME;
        match m.id {
            msg::LBUTTONDOWN => {
                let p = self.quick_panel.as_mut()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let v = p.layout.hit_test(at).filter(|h| h.module == quick_panel::HIT)?.value;
                if is_level(v) {
                    p.dragging = Some(v);
                    p.shown.window().capture_mouse(true);
                    self.quick_level_from(v, x);
                }
                Some(0)
            }
            msg::MOUSEMOVE => {
                if let Some(v) = self.quick_panel.as_ref().and_then(|p| p.dragging) {
                    self.quick_level_from(v, msg::point(m.lparam).0);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let p = self.quick_panel.as_mut()?;
                if p.dragging.take().is_some() {
                    p.shown.window().capture_mouse(false);
                    return Some(0);
                }
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let v = p.layout.hit_test(at).filter(|h| h.module == quick_panel::HIT)?.value;
                if is_level(v) {
                    return Some(0);
                }
                let action = p.quick.click(v);
                self.quick_action(action);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_quick_panel();
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE
                    && !self.quick_panel.as_ref().is_some_and(|p| p.dragging.is_some())
                {
                    self.close_quick_panel();
                }
                None
            }
            _ => None,
        }
    }

    /// Integration tests: open, close, or click part `value`.
    #[cfg(feature = "integration")]
    pub(super) fn test_quick_panel(&mut self, what: usize, value: u32) {
        match what {
            1 => {
                let bar = self.bars.iter().position(|b| b.primary).unwrap_or(0);
                self.close_quick_panel();
                self.open_quick_panel(bar, (super::view::hit::QUICK_SETTINGS, 0), false);
            }
            2 => {
                if let Some(p) = self.quick_panel.as_ref() {
                    let action = p.quick.click(value);
                    self.quick_action(action);
                }
            }
            _ => self.close_quick_panel(),
        }
    }
}
