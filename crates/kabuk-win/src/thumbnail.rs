//! A live DWM thumbnail of another window, drawn on one of Kabuk's windows (the
//! overview). Gone when dropped. A [`Flight`] moves thumbnails on a thread of its own, a
//! step per compositor frame, as the window moves do (a timer on the bar's thread
//! stuttered; found by the user).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::{
    DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES,
    DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};

use crate::msgloop::PxRect;
use crate::window::Hwnd;

pub struct Thumbnail(isize);

/// A thumbnail's handle for a [`Flight`]; it must not outlive its [`Thumbnail`]'s use
/// (the flight is stopped before the thumbnails go).
#[derive(Clone, Copy, Debug)]
pub struct ThumbId(isize);

fn place_handle(h: isize, rect: PxRect) -> bool {
    let p = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.w,
            bottom: rect.y + rect.h,
        },
        opacity: u8::MAX,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
        ..Default::default()
    };
    // SAFETY: a registered thumbnail (an unregistered one fails harmlessly) and a fully
    // set properties struct.
    unsafe { DwmUpdateThumbnailProperties(h, &p) }.is_ok()
}

impl Thumbnail {
    pub fn id(&self) -> ThumbId {
        ThumbId(self.0)
    }

    /// `source` drawn into `dest` at `rect` (in `dest`'s client pixels); `None` when DWM
    /// refuses (the source is gone or hidden).
    pub fn new(dest: Hwnd, source: Hwnd, rect: PxRect) -> Option<Thumbnail> {
        // SAFETY: registers a thumbnail between two windows; unregistered on drop.
        let t = Thumbnail(unsafe { DwmRegisterThumbnail(dest.raw(), source.raw()) }.ok()?);
        t.place(rect).then_some(t)
    }

    /// Moves it; false when DWM refuses.
    pub fn place(&self, rect: PxRect) -> bool {
        place_handle(self.0, rect)
    }
}

/// One thumbnail's way: from `from` to `to`.
#[derive(Clone, Copy, Debug)]
pub struct Way {
    pub thumb: ThumbId,
    pub from: PxRect,
    pub to: PxRect,
}

/// Where a thumbnail is when `u` (0-1) of the way is done.
fn between(a: PxRect, b: PxRect, u: f32) -> PxRect {
    let at = |x: i32, y: i32| x + ((y - x) as f32 * u).round() as i32;
    PxRect {
        x: at(a.x, b.x),
        y: at(a.y, b.y),
        w: at(a.w, b.w),
        h: at(a.h, b.h),
    }
}

/// Thumbnails moving along their ways over `seconds`, eased by `ease`; when they are
/// there, `done` (a window and a message) is posted. Dropping it stops them where they
/// are.
pub struct Flight {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Flight {
    pub fn start(
        ways: Vec<Way>,
        seconds: f32,
        ease: fn(f32) -> f32,
        done: Option<(Hwnd, u32)>,
    ) -> std::io::Result<Flight> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use windows::Win32::Graphics::Dwm::DwmFlush;
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handles: Vec<(isize, PxRect, PxRect)> = ways.iter().map(|w| (w.thumb.0, w.from, w.to)).collect();
        let thread = std::thread::Builder::new()
            .name("overview-flight".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                loop {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let t = if seconds > 0.0 {
                        (started.elapsed().as_secs_f32() / seconds).min(1.0)
                    } else {
                        1.0
                    };
                    let u = ease(t);
                    for (h, from, to) in &handles {
                        place_handle(*h, between(*from, *to, u));
                    }
                    if t >= 1.0 {
                        break;
                    }
                    // SAFETY: plain call; waits for the compositor's next frame.
                    if unsafe { DwmFlush() }.is_err() {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                if let Some((window, id)) = done {
                    let _ = crate::msgloop::post_message(
                        window,
                        crate::msgloop::Msg {
                            id,
                            wparam: 0,
                            lparam: 0,
                        },
                    );
                }
            })?;
        Ok(Flight {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Flight {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        // SAFETY: our registered thumbnail, unregistered once.
        let _ = unsafe { DwmUnregisterThumbnail(self.0) };
    }
}

#[cfg(test)]
mod tests {
    //! Edge case: a thumbnail of a window that does not exist is refused.
    use super::*;

    #[test]
    fn a_missing_source_is_refused() {
        let dest = crate::msgloop::Window::shell_surface("Kabuk.Test.Thumb", "", PxRect::default(), |_| None).unwrap();
        let gone = Hwnd(0x7fff_fff0);
        assert!(Thumbnail::new(dest.hwnd(), gone, PxRect::default()).is_none());
    }
}
