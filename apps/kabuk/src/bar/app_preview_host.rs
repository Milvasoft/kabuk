//! Previews of an app's windows on the bar thread ([`crate::bar::app_preview`]): shown
//! after the mouse rests on an app with windows, placed like a tooltip, with a live DWM
//! thumbnail in each card's frame. A click on a card shows that window; the previews go
//! a moment after the mouse leaves both the app and them, on a click, or on a drag.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, Msg, PxRect, Window};
use kabuk_win::thumbnail::Thumbnail;
use kabuk_win::window::Hwnd;

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::{timer, Shared};
use crate::bar::app_preview::{self, Shot, PREVIEW};
use crate::wm;

const PREVIEW_CLASS: &str = "Kabuk.AppPreview";
/// Integration tests show the previews of the app holding window `lparam` at once
/// (`wparam` 1), click its first card (2) or hide them (0).
pub const MSG_TEST_APP_PREVIEW: u32 = msg::APP + 37;

pub(super) struct AppPreview {
    /// The thumbnails go before the window they are drawn on.
    _thumbs: Vec<Thumbnail>,
    shown: Option<Shown>,
    layout: Layout,
    scale: Scale,
    /// The app's index in the list and the windows of its cards, in order.
    pub(super) app: usize,
    windows: Vec<u64>,
    /// Shown by a click on the app (a second click takes them away).
    pub(super) clicked: bool,
}

impl Shared {
    /// The mouse rests on app `app` of bar `bar` (or left the apps): the previews wait
    /// their delay, follow at once when they are already up, or go after their grace.
    pub(super) fn preview_hover(&mut self, bar: usize, app: Option<usize>) {
        self.kill(timer::APP_PREVIEW);
        if app != self.preview_held {
            self.preview_held = None;
        }
        let has_windows = |i: usize| self.apps.list.get(i).is_some_and(|a| !a.windows.is_empty());
        match app.filter(|i| has_windows(*i)) {
            Some(i) if self.app_preview.is_some() => {
                self.kill(timer::APP_PREVIEW_HIDE);
                if self.app_preview.as_ref().is_some_and(|p| p.app != i) {
                    self.show_app_preview(bar, i);
                }
            }
            Some(_) => {
                self.preview_bar = bar;
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW, TIP.delay);
                }
            }
            None if self.app_preview.is_some() => {
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW_HIDE, PREVIEW.grace);
                }
            }
            None => {}
        }
    }

    /// The delay passed: the previews of the app under the mouse.
    pub(super) fn preview_due(&mut self) {
        self.kill(timer::APP_PREVIEW);
        if let Some(i) = self.app_hover_index().filter(|i| Some(*i) != self.preview_held) {
            self.show_app_preview(self.preview_bar, i);
        }
    }

    pub(super) fn hide_app_preview(&mut self) {
        self.kill(timer::APP_PREVIEW);
        self.kill(timer::APP_PREVIEW_HIDE);
        if let Some(mut p) = self.app_preview.take() {
            p._thumbs.clear();
            self.retire(p.shown.take());
            tracing::debug!("app previews hidden");
        }
    }

    pub(super) fn show_app_preview(&mut self, bar: usize, index: usize) {
        self.hide_app_preview();
        self.hide_tooltip();
        let Some(app) = self.apps.list.get(index) else { return };
        let windows: Vec<u64> = app.windows.iter().copied().take(PREVIEW.max_windows).collect();
        // Which workspace each is on, said when they are on several (PLAN M19 U20).
        let view = self.wm.as_ref().map(wm::WmLink::view);
        let workspaces: Vec<Option<u32>> = windows
            .iter()
            .map(|h| {
                view.as_ref()
                    .and_then(|v| v.windows.iter().find(|w| w.hwnd == *h))
                    .map(|w| w.workspace)
            })
            .collect();
        let several = app_preview::on_several_workspaces(&workspaces);
        let names = &self.config().windows;
        let shots: Vec<Shot> = windows
            .iter()
            .zip(&workspaces)
            .map(|(h, ws)| Shot {
                title: Hwnd(*h).title(),
                size: Hwnd(*h).restored_size().unwrap_or_default(),
                workspace: ws
                    .filter(|_| several)
                    .map(|n| crate::text::workspace_title(&self.i18n, n, names.workspace_name(n).as_deref())),
            })
            .collect();
        if shots.is_empty() {
            return;
        }
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let node = app_preview::view(self.look.style.text_scale, &TIP, &PREVIEW, &shots);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let anchor = self
            .hit_rect(bar, hit::APP, u32::try_from(index).unwrap_or(u32::MAX))
            .unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            anchor,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        let weak = self.me.clone();
        let window = match Window::shell_surface(PREVIEW_CLASS, "", rect, move |m| {
            if m.id == msg::MOUSEACTIVATE {
                return Some(msg::MA_NOACTIVATE);
            }
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_app_preview(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "app preview window failed");
                return;
            }
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "app preview surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "app preview not drawn");
            }
        }
        let thumbs: Vec<Thumbnail> = layout
            .hits
            .iter()
            .filter(|(_, h)| h.module == app_preview::FRAME)
            .filter_map(|(r, h)| {
                let i = h.value as usize;
                let (x, y, w, hh) = app_preview::fit(shots.get(i)?.size, (r.x, r.y, r.w, r.h));
                let px = PxRect {
                    x: scale.px(x),
                    y: scale.px(y),
                    w: scale.px(w),
                    h: scale.px(hh),
                };
                Thumbnail::new(window.hwnd(), Hwnd(*windows.get(i)?), px)
            })
            .collect();
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        shown.window().show();
        tracing::info!(
            source = %self.apps.list[index].source,
            windows = windows.len(),
            pictures = thumbs.len(),
            "app previews shown"
        );
        self.app_preview = Some(AppPreview {
            _thumbs: thumbs,
            shown: Some(shown),
            layout,
            scale,
            app: index,
            windows,
            clicked: false,
        });
    }

    /// Card `i`'s × was clicked: its window is asked to close, as Windows' preview does.
    fn preview_close_clicked(&mut self, i: usize) {
        let Some(hwnd) = self.app_preview.as_ref().and_then(|p| p.windows.get(i).copied()) else {
            return;
        };
        self.hide_app_preview();
        tracing::info!(hwnd, "window closed from its preview");
        Hwnd(hwnd).close_async();
    }

    /// Card `i` was clicked: its window comes to the front.
    fn preview_clicked(&mut self, i: usize) {
        let Some(hwnd) = self.app_preview.as_ref().and_then(|p| p.windows.get(i).copied()) else {
            return;
        };
        self.hide_app_preview();
        tracing::info!(hwnd, "window shown from its preview");
        match &self.wm {
            Some(link) => link.send(wm::Command::Reveal(hwnd)),
            None => {
                Hwnd(hwnd).focus();
            }
        }
    }

    #[cfg(feature = "integration")]
    pub(super) fn test_app_preview(&mut self, what: usize, hwnd: u64) {
        if what == 0 {
            self.hide_app_preview();
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
        if what == 1 {
            self.show_app_preview(bar, index);
        } else {
            self.preview_clicked(0);
        }
    }

    fn on_app_preview(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::MOUSEMOVE => {
                let p = self.app_preview.as_ref()?;
                kabuk_win::msgloop::track_mouse_leave(p.shown.as_ref()?.window().hwnd());
                self.kill(timer::APP_PREVIEW_HIDE);
                Some(0)
            }
            msg::MOUSELEAVE => {
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW_HIDE, PREVIEW.grace);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let p = self.app_preview.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let hit = p.layout.hit_test(at)?;
                let i = hit.value as usize;
                if hit.module == app_preview::CLOSE {
                    self.preview_close_clicked(i);
                } else if hit.module == app_preview::HIT || hit.module == app_preview::FRAME {
                    self.preview_clicked(i);
                }
                Some(0)
            }
            _ => None,
        }
    }
}
