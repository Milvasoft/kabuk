//! The window motion's backdrop (DECISIONS D81): each monitor's wallpaper picture, painted
//! on the overlay under the desktop's live thumbnails. On some machines the compositor
//! gives `Progman`'s thumbnail only one monitor's worth, and the rest showed black under
//! the genie (found on another computer: black on its primary monitor only, its log
//! saying the picture was 1920 × 1080 of a 3840-wide desktop); there the wallpaper shows
//! instead (without the icons, for the moment the scene plays). Where the thumbnail covers
//! it all, it is drawn over the backdrop and nothing changes.
//!
//! Lives on the window moves' thread: the pictures are decoded once per monitor and file,
//! kept while that thread keeps its renderer, and let go with it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use kabuk_core::state::Rect;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
};

/// A monitor's wallpaper, decoded: its rectangle, the file (and when it was written: a
/// picture written again under the same name, as Kabuk's drift may, is decoded again)
/// and the bitmap.
struct Picture {
    monitor: Rect,
    file: (String, Option<SystemTime>),
    bitmap: HBITMAP,
}

thread_local! {
    /// The decoded wallpapers, one per monitor that shows a picture.
    static PICTURES: RefCell<Vec<Picture>> = const { RefCell::new(Vec::new()) };
    /// Each overlay's area, for its paint.
    static OVERLAYS: RefCell<HashMap<isize, Rect>> = RefCell::new(HashMap::new());
    /// When Windows was last asked for its wallpapers ([`ask_again`]).
    static ASKED: Cell<Option<(u64, Instant)>> = const { Cell::new(None) };
    /// How many times Windows was asked for its wallpapers.
    #[cfg(test)]
    static ASKED_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// How long what Windows said its wallpapers are is trusted without a change heard of
/// (a slideshow's next picture may come unannounced); the pictures are let go after as
/// long idle anyway.
const TRUSTED: Duration = Duration::from_secs(60);

/// Whether to ask Windows for its wallpapers again, having asked at `asked` (the count
/// of changes then, and when; `None`: not since the pictures were let go), with `changes`
/// heard of by `now`.
fn ask_again(asked: Option<(u64, Instant)>, changes: u64, now: Instant) -> bool {
    asked.is_none_or(|(seen, at)| seen != changes || now.saturating_duration_since(at) >= TRUSTED)
}

/// Makes sure each monitor's wallpaper is decoded (a changed one again, a gone one let
/// go). Windows is asked only when [`ask_again`] says so (PLAN M19 P18: every window
/// move asked it). COM must be initialised on this thread.
pub(super) fn refresh() {
    // Read before asking: a change while Windows answers is asked about next time.
    let changes = crate::wallpaper::changes();
    let now = Instant::now();
    if !ask_again(ASKED.get(), changes, now) {
        return;
    }
    ASKED.set(Some((changes, now)));
    #[cfg(test)]
    ASKED_COUNT.with(|c| c.set(c.get() + 1));
    let files: Vec<(Rect, (String, Option<SystemTime>))> = crate::wallpaper::files()
        .into_iter()
        .filter_map(|(monitor, file)| {
            let file = file?;
            let written = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            Some((monitor, (file, written)))
        })
        .collect();
    PICTURES.with(|p| {
        let mut p = p.borrow_mut();
        p.retain(|pic| {
            let keep = files.iter().any(|(r, f)| *r == pic.monitor && *f == pic.file);
            if !keep {
                // SAFETY: our own bitmap, selected into no DC.
                let _ = unsafe { DeleteObject(pic.bitmap.into()) };
            }
            keep
        });
        for (monitor, file) in files {
            if p.iter().any(|pic| pic.monitor == monitor && pic.file == file) {
                continue;
            }
            match decode(&file.0, monitor) {
                Some(bitmap) => p.push(Picture { monitor, file, bitmap }),
                None => tracing::debug!(file = file.0, "wallpaper not decoded for the window motion"),
            }
        }
    });
}

/// Lets every decoded picture go (the window moves' thread is idle); Windows is asked
/// again next time.
pub(super) fn release() {
    ASKED.set(None);
    PICTURES.with(|p| {
        for pic in p.borrow_mut().drain(..) {
            // SAFETY: our own bitmap, selected into no DC.
            let _ = unsafe { DeleteObject(pic.bitmap.into()) };
        }
    });
}

/// The wallpaper in `file` covering `monitor`, as a top-down 32-bit bitmap.
fn decode(file: &str, monitor: Rect) -> Option<HBITMAP> {
    let (w, h) = (
        u32::try_from(monitor.right - monitor.left).ok()?,
        u32::try_from(monitor.bottom - monitor.top).ok()?,
    );
    let bgr = crate::picture::cover(std::path::Path::new(file), w, h).ok()?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).ok()?,
            biWidth: i32::try_from(w).ok()?,
            // Negative: top-down rows.
            biHeight: -i32::try_from(h).ok()?,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    // SAFETY: a DIB section of the size described, its memory owned by the bitmap; the
    // rows are written within `w × h` 32-bit pixels.
    unsafe {
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(bitmap.into());
            return None;
        }
        let out = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
        for (px, rgb) in out.chunks_exact_mut(4).zip(bgr.chunks_exact(3)) {
            px[..3].copy_from_slice(rgb);
            px[3] = u8::MAX;
        }
        Some(bitmap)
    }
}

/// Overlay `hwnd` covers `area`: its paint draws the wallpapers under it.
pub(super) fn attach(hwnd: HWND, area: Rect) {
    OVERLAYS.with(|o| o.borrow_mut().insert(hwnd.0 as isize, area));
}

/// Overlay `hwnd` is gone.
pub(super) fn detach(hwnd: HWND) {
    OVERLAYS.with(|o| o.borrow_mut().remove(&(hwnd.0 as isize)));
}

/// Paints overlay `hwnd` (its `WM_PAINT`): each monitor's wallpaper where it lies under
/// the overlay. False when the overlay has no backdrop (the default paint then runs).
pub(super) fn paint(hwnd: HWND) -> bool {
    let Some(area) = OVERLAYS.with(|o| o.borrow().get(&(hwnd.0 as isize)).copied()) else {
        return false;
    };
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: (whole block) the paint of our own window on its own thread; the memory DC
    // and the bitmaps selected into it are put back and the DC freed before returning.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        let mem = CreateCompatibleDC(Some(dc));
        PICTURES.with(|p| {
            for pic in p.borrow().iter() {
                let old: HGDIOBJ = SelectObject(mem, pic.bitmap.into());
                let _ = BitBlt(
                    dc,
                    pic.monitor.left - area.left,
                    pic.monitor.top - area.top,
                    pic.monitor.right - pic.monitor.left,
                    pic.monitor.bottom - pic.monitor.top,
                    Some(mem),
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(mem, old);
            }
        });
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
    true
}

#[cfg(test)]
mod tests {
    //! Edge cases: each monitor showing a picture file gets one decoded picture, decoded
    //! once (a second look keeps the same bitmaps); letting go leaves none; Windows asked
    //! once for many looks, again after a change, after letting go and after a while; an
    //! overlay with no backdrop is left to the default paint. Reads Windows' wallpapers
    //! as they are.
    use super::*;

    fn bitmaps() -> Vec<isize> {
        PICTURES.with(|p| p.borrow().iter().map(|pic| pic.bitmap.0 as isize).collect())
    }

    #[test]
    fn the_wallpapers_are_decoded_once_and_let_go() {
        crate::system::com_init();
        refresh();
        let shown = crate::wallpaper::files()
            .into_iter()
            .filter(|(_, f)| f.as_deref().is_some_and(|f| std::path::Path::new(f).is_file()))
            .count();
        let first = bitmaps();
        assert_eq!(first.len(), shown, "one picture per monitor showing a picture file");
        refresh();
        assert_eq!(bitmaps(), first, "decoded again though nothing changed");
        release();
        assert!(bitmaps().is_empty());
    }

    /// PLAN M19 P18: every window move, and every warm-up while the mouse was on the bar's
    /// apps, asked Windows for each monitor's wallpaper. It is asked again only after a
    /// change was heard of, once the pictures were let go, or a while after.
    #[test]
    fn windows_is_asked_for_the_wallpapers_again_only_after_a_change() {
        crate::system::com_init();
        let asked = || ASKED_COUNT.with(Cell::get);
        let first = bitmaps();
        for _ in 0..10 {
            refresh();
        }
        assert_eq!(asked(), 1, "ten window moves asked Windows {} times", asked());
        crate::wallpaper::changed();
        refresh();
        refresh();
        assert_eq!(asked(), 2, "a change asks again, once");
        release();
        refresh();
        assert_eq!(asked(), 3, "the pictures let go are asked for again");
        release();
        assert!(first.is_empty());
        // A while after, Windows is asked again without a change heard of (a slideshow
        // may not say).
        let now = Instant::now();
        let at = (crate::wallpaper::changes(), now);
        assert!(!ask_again(Some(at), at.0, now + TRUSTED / 2));
        assert!(ask_again(Some(at), at.0, now + TRUSTED));
        assert!(ask_again(Some(at), at.0 + 1, now));
        assert!(ask_again(None, at.0, now));
    }

    #[test]
    fn an_overlay_without_a_backdrop_keeps_the_default_paint() {
        assert!(!paint(HWND(0x1234_5678 as *mut _)));
        attach(HWND(0x1234_5678 as *mut _), Rect::default());
        detach(HWND(0x1234_5678 as *mut _));
        assert!(!paint(HWND(0x1234_5678 as *mut _)));
    }
}
