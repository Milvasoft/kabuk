//! Reading what the screen shows, for tests and checks: a rectangle of the composed
//! desktop in physical pixels (the caller is per-monitor DPI aware).

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};

use crate::msgloop::PxRect;

/// The pixels of `r` as BGRA rows, top first; `None` when the screen cannot be read.
pub fn capture(r: PxRect) -> Option<Vec<u8>> {
    if r.w <= 0 || r.h <= 0 {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).ok()?,
            biWidth: r.w,
            // Negative: top-down rows.
            biHeight: -r.h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; usize::try_from(r.w).ok()? * usize::try_from(r.h).ok()? * 4];
    // SAFETY: the screen DC, a compatible DC and bitmap are created, used and released
    // here; the buffer holds exactly `w × h` 32-bit pixels as the header says.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, r.w, r.h);
        let old = SelectObject(dc, bmp.into());
        let copied = BitBlt(dc, 0, 0, r.w, r.h, Some(screen), r.x, r.y, SRCCOPY | CAPTUREBLT).is_ok();
        SelectObject(dc, old);
        let lines = GetDIBits(
            dc,
            bmp,
            0,
            u32::try_from(r.h).unwrap_or_default(),
            Some(px.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        (copied && lines == r.h).then_some(px)
    }
}

/// The desktop compositor's frame counters, to tell whether animations kept up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frames {
    /// Frames composed.
    pub composed: u64,
    /// Frames DWM could not compose in time (a whole refresh missed).
    pub missed: u64,
    /// Frames composed but not shown.
    pub dropped: u64,
}

impl Frames {
    /// The counters now; zeros when DWM does not answer.
    pub fn now() -> Frames {
        use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
        let mut t = DWM_TIMING_INFO {
            cbSize: u32::try_from(std::mem::size_of::<DWM_TIMING_INFO>()).unwrap_or_default(),
            ..Default::default()
        };
        // SAFETY: a NULL window asks for the whole desktop's timing (Windows 8 and later);
        // the struct is sized as the call expects.
        if unsafe { DwmGetCompositionTimingInfo(windows::Win32::Foundation::HWND::default(), &mut t) }.is_err() {
            return Frames::default();
        }
        // Fields of a packed struct, copied out.
        Frames {
            composed: { t.cFrame },
            missed: { t.cFramesMissed },
            dropped: { t.cFramesDropped },
        }
    }

    /// What happened between `self` and `later`.
    pub fn since(self, later: Frames) -> Frames {
        Frames {
            composed: later.composed.saturating_sub(self.composed),
            missed: later.missed.saturating_sub(self.missed),
            dropped: later.dropped.saturating_sub(self.dropped),
        }
    }
}

/// The mean absolute difference of two equal captures, per channel (0-255).
pub fn difference(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::MAX;
    }
    let sum: u64 = a.iter().zip(b).map(|(x, y)| u64::from(x.abs_diff(*y))).sum();
    sum as f32 / a.len() as f32
}

#[cfg(test)]
mod tests {
    //! Edge cases: an empty rectangle reads nothing; a read has the right size; the
    //! difference of a capture with itself is zero and of unequal sizes is the maximum; the
    //! frame counters only grow.
    use super::*;

    #[test]
    fn the_screen_reads_in_its_size() {
        assert!(capture(PxRect::default()).is_none());
        let px = capture(PxRect { x: 0, y: 0, w: 8, h: 4 }).expect("screen readable");
        assert_eq!(px.len(), 8 * 4 * 4);
        assert_eq!(difference(&px, &px), 0.0);
        assert_eq!(difference(&px, &px[4..]), f32::MAX);
        assert_eq!(difference(&[0, 10], &[10, 0]), 10.0);
    }

    #[test]
    fn the_compositor_counts_frames() {
        let a = Frames::now();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let b = Frames::now();
        // DWM keeps composing; nothing goes backwards.
        assert!(b.composed >= a.composed && b.missed >= a.missed && b.dropped >= a.dropped);
        assert_eq!(a.since(a), Frames::default());
        assert_eq!(b.since(a), Frames::default(), "earlier minus later saturates");
    }
}
