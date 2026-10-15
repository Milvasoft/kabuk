//! The media card on the bar thread (`bar::media_card`, DECISIONS D66): opened by a right
//! click on the media module and placed like the tray panel; its buttons step the
//! session; a change of what plays draws it again; a click elsewhere or Esc closes it.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::media_card::{self, Button, Card};

const CARD_CLASS: &str = "Kabuk.MediaCard";
/// Integration tests open (`wparam` 1) the card without taking the keyboard, or close it.
pub const MSG_TEST_MEDIA_CARD: u32 = msg::APP + 50;

pub(super) struct MediaCard {
    _shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
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
        self.media_card = None;
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
        let node = media_card::view(&card, &self.i18n, self.look.style.text_scale, &TIP);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
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
            _shown: shown,
            layout,
            scale,
            bar,
        });
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
                let b = c
                    .layout
                    .hit_test(at)
                    .filter(|h| h.module == media_card::HIT)
                    .and_then(|h| Button::from_value(h.value))?;
                tracing::info!(button = ?b, "media card");
                if let Some(media) = &self.media {
                    match b {
                        Button::Previous => media.previous(),
                        Button::Toggle => media.toggle(),
                        Button::Next => media.next(),
                    }
                }
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_media_card();
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
