//! The media card on the bar thread (`bar::media_card`, DECISIONS D66): opened by a right
//! click on the media module and placed like the tray panel; its buttons step the
//! session; a change of what plays draws it again; a click elsewhere or Esc closes it.
//! The keyboard moves over its buttons as in every popup ([`crate::bar::popup_keys`];
//! Left and Right too, they stand in a row) and Enter or Space presses one.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::media_card::{self, Button, Card};
use crate::bar::popup_keys::{self, Step};

const CARD_CLASS: &str = "Kabuk.MediaCard";
/// Integration tests open (`wparam` 1) the card without taking the keyboard, or close it.
pub const MSG_TEST_MEDIA_CARD: u32 = msg::APP + 50;

pub(super) struct MediaCard {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
    /// What it shows, to draw it again with another button selected, and the button the
    /// keyboard is on.
    card: Card,
    selected: Option<u32>,
}

impl Shared {
    pub(super) fn toggle_media_card(&mut self, bar: usize) {
        if self.media_card.is_some() {
            self.close_media_card();
        } else {
            self.open_media_card(bar, true, true);
        }
    }

    pub(super) fn close_media_card(&mut self) {
        if self.media_card.take().is_some() {
            tracing::debug!("media card closed");
        }
    }

    /// What plays, with its cover put among the renderer's pictures.
    fn media_card_data(&mut self) -> Card {
        let info = self
            .media
            .as_ref()
            .map(kabuk_win::media::Media::info)
            .unwrap_or_default();
        let cover = info.cover.as_ref().map(|c| {
            self.gpu
                .images
                .set(c.id, kabuk_win::media::COVER_SIDE, c.pixels.as_ref().clone());
            c.id
        });
        Card {
            title: info.title.unwrap_or_default(),
            artist: info.artist,
            album: info.album,
            playing: info.playing,
            can_previous: info.can_previous,
            can_next: info.can_next,
            cover,
        }
    }

    /// The card under bar `bar`'s media module; with the keyboard when `activate`, coming
    /// in when `animate` (not when it is drawn again for a change).
    pub(super) fn open_media_card(&mut self, bar: usize, activate: bool, animate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_calendar_panel();
        self.close_privacy_panel();
        self.close_keyboard_panel();
        self.close_network_panel();
        // Drawn again for a change: the keyboard stays on its button.
        let selected = self.media_card.take().and_then(|c| c.selected);
        let weak = self.me.clone();
        let window = match Window::input_surface(CARD_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_media_card(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "media card window failed");
                return;
            }
        };
        let Some(b) = self.bars.get(bar) else { return };
        let (scale, edge, strip, monitor) = (b.scale, b.edge, b.rect, super::px_rect(b.monitor.rect));
        let anchor = self.hit_rect(bar, hit::MEDIA, 0).unwrap_or(strip);
        let card = self.media_card_data();
        let layout = self.media_card_layout(&card, selected);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let rect = tooltip::place(edge, strip, anchor, size, scale.px(TIP.gap), monitor);
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "media card surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "media card not drawn");
            }
        }
        let level = if animate { self.motion_level() } else { Level::Off };
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), level);
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(title = %card.title, cover = card.cover.is_some(), "media card opened");
        self.media_card = Some(MediaCard {
            shown,
            layout,
            scale,
            bar,
            card,
            selected,
        });
    }

    /// The card for `card` laid out with button `selected` drawn selected.
    fn media_card_layout(&self, card: &Card, selected: Option<u32>) -> Layout {
        let node = media_card::view(card, &self.i18n, self.look.style.text_scale, &TIP);
        let node = popup_keys::marked(node, media_card::HIT, selected, &TIP);
        kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text)
    }

    /// A click on button `value` (or Enter on it).
    fn media_card_click(&mut self, value: u32) {
        let Some(b) = Button::from_value(value) else { return };
        tracing::info!(button = ?b, "media card");
        if let Some(media) = &self.media {
            match b {
                Button::Previous => media.previous(),
                Button::Toggle => media.toggle(),
                Button::Next => media.next(),
            }
        }
    }

    /// What plays changed: an open card shows it.
    pub(super) fn media_card_changed(&mut self) {
        if let Some(bar) = self.media_card.as_ref().map(|c| c.bar) {
            self.open_media_card(bar, true, false);
        }
    }

    pub(super) fn on_media_card(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let c = self.media_card.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: c.scale.dips(x),
                    y: c.scale.dips(y),
                };
                let value = c.layout.hit_test(at).filter(|h| h.module == media_card::HIT)?.value;
                self.media_card_click(value);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_media_card();
                Some(0)
            }
            msg::KEYDOWN => {
                let key = match popup_keys::Key::of(m.wparam, msgloop::shift_held(), false)? {
                    // The buttons stand in a row.
                    popup_keys::Key::Less => popup_keys::Key::Up,
                    popup_keys::Key::More => popup_keys::Key::Down,
                    k => k,
                };
                let c = self.media_card.as_ref()?;
                match popup_keys::step(&popup_keys::entries(&c.layout, media_card::HIT), c.selected, key) {
                    Step::Select(v) => {
                        let layout = self.media_card_layout(&c.card, Some(v));
                        let c = self.media_card.as_mut()?;
                        c.shown.repaint(&self.gpu, &layout, &self.look.palette, c.scale);
                        let _ = self.gpu.commit();
                        c.layout = layout;
                        c.selected = Some(v);
                    }
                    Step::Activate(v) => self.media_card_click(v),
                    Step::Same | Step::Adjust(..) => {}
                }
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_media_card();
                }
                None
            }
            _ => None,
        }
    }
}
