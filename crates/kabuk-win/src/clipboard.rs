//! Putting text on the clipboard (the launcher's calculator result).

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use crate::sync::io_error;

/// Closes the clipboard when dropped.
struct Open;

impl Open {
    fn new() -> std::io::Result<Open> {
        // SAFETY: opens the clipboard for this thread without an owner window.
        unsafe { OpenClipboard(None) }.map_err(io_error)?;
        Ok(Open)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: closes the clipboard this thread opened.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Replaces the clipboard's contents with `text`.
pub fn set_text(text: &str) -> std::io::Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let _open = Open::new()?;
    // SAFETY: (whole block) a movable global block of the right size is filled while
    // locked; once SetClipboardData succeeds the clipboard owns it, otherwise it is freed.
    unsafe {
        EmptyClipboard().map_err(io_error)?;
        let block = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(io_error)?;
        let dst = GlobalLock(block) as *mut u16;
        if dst.is_null() {
            let _ = GlobalFree(Some(block));
            return Err(std::io::Error::other("GlobalLock failed"));
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        let _ = GlobalUnlock(block);
        if let Err(e) = SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(block.0))) {
            let _ = GlobalFree(Some(block));
            return Err(io_error(e));
        }
    }
    Ok(())
}

/// The clipboard's text, if it holds any.
pub fn text() -> Option<String> {
    let _open = Open::new().ok()?;
    // SAFETY: (whole block) the handle belongs to the clipboard and is only read while
    // locked; the text is NUL-terminated UTF-16.
    unsafe {
        let h = GetClipboardData(u32::from(CF_UNICODETEXT.0)).ok()?;
        let block = HGLOBAL(h.0);
        let p = GlobalLock(block) as *const u16;
        if p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(block);
        Some(s)
    }
}

/// Whether the clipboard holds something other than text (a picture, files), which a
/// test could not put back.
pub fn holds_more_than_text() -> bool {
    use windows::Win32::System::DataExchange::{CountClipboardFormats, IsClipboardFormatAvailable};
    // SAFETY: plain queries.
    unsafe {
        let n = CountClipboardFormats();
        // Text comes with its synthesised variants (ANSI, OEM, locale).
        n > 0 && (IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).is_err() || n > TEXT_FORMATS)
    }
}

/// Formats Windows lists for plain text on the clipboard (Unicode, ANSI, OEM, locale).
const TEXT_FORMATS: i32 = 4;

#[cfg(test)]
mod tests {
    //! Edge cases: Turkish letters and an empty string go through unchanged. The user's
    //! clipboard text is put back afterwards; when it holds a picture or files, which
    //! could not be put back, the test leaves it alone and says so.
    use super::*;

    #[test]
    fn text_goes_in_and_comes_back() {
        if holds_more_than_text() {
            eprintln!("clipboard holds more than text; left alone");
            return;
        }
        let before = text();
        set_text("12*8 = 96 · ışık İ").unwrap();
        assert_eq!(text().as_deref(), Some("12*8 = 96 · ışık İ"));
        set_text("").unwrap();
        assert_eq!(text().as_deref(), Some(""));
        if let Some(b) = before {
            set_text(&b).unwrap();
        }
    }
}
