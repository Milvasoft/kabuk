//! The input languages (DECISIONS D88): the ones installed, the one the window in front
//! types in, and asking that window to switch, as Windows' input indicator does.

use kabuk_core::input_lang::language_id;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use crate::window::Hwnd;

/// `LOCALE_NAME_MAX_LENGTH`.
const NAME_MAX: usize = 85;
/// `SORT_DEFAULT` in a locale id's high bits.
const SORT_DEFAULT: u32 = 0;

/// An input language: its layout handle, locale name (`tr-TR`) and display name in
/// Windows' own language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputLang {
    pub hkl: isize,
    pub locale: String,
    pub name: String,
}

fn locale_of(hkl: isize) -> String {
    let mut buf = [0u16; NAME_MAX];
    let lcid = u32::from(language_id(hkl)) | (SORT_DEFAULT << 16);
    // SAFETY: a buffer of the length passed.
    let n = unsafe { LCIDToLocaleName(lcid, Some(&mut buf), 0) };
    String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0).saturating_sub(1)])
}

fn display_name(locale: &str) -> String {
    let name: Vec<u16> = locale.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 256];
    // SAFETY: a terminated name and a buffer of the length passed.
    let n = unsafe { GetLocaleInfoEx(PCWSTR(name.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut buf)) };
    String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0).saturating_sub(1)])
}

/// The input languages installed, one per language (a second layout of the same language
/// is reached by Windows' own switching), in Windows' order.
pub fn installed() -> Vec<InputLang> {
    // SAFETY: the two-call form: the count, then a buffer that large.
    let count = unsafe { GetKeyboardLayoutList(None) };
    let mut layouts = vec![HKL::default(); usize::try_from(count).unwrap_or(0)];
    // SAFETY: a buffer of the length passed.
    let n = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    layouts.truncate(usize::try_from(n).unwrap_or(0));
    let mut out: Vec<InputLang> = Vec::new();
    for hkl in layouts.into_iter().map(|h| h.0 as isize) {
        if out.iter().any(|l| language_id(l.hkl) == language_id(hkl)) {
            continue;
        }
        let locale = locale_of(hkl);
        let name = display_name(&locale);
        out.push(InputLang { hkl, locale, name });
    }
    out
}

/// The layout the window in front types with.
pub fn current() -> Option<isize> {
    // SAFETY: plain queries; a missing window gives thread 0, whose layout is ours.
    unsafe {
        let front = GetForegroundWindow();
        if front.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(front, None);
        Some(GetKeyboardLayout(thread).0 as isize)
    }
}

/// Asks `window` to type in `hkl` from now on (what Windows' input switcher does).
pub fn switch(window: Hwnd, hkl: isize) {
    // SAFETY: a request message; a window that refuses or is gone simply does not switch.
    let _ = unsafe { PostMessageW(Some(window.raw()), WM_INPUTLANGCHANGEREQUEST, WPARAM(0), LPARAM(hkl)) };
}

#[cfg(test)]
mod tests {
    //! On the machine running the tests: at least one language is installed, each named;
    //! the one in front is among them when there is a window in front.
    use super::*;

    #[test]
    fn the_installed_languages_are_named() {
        let all = installed();
        assert!(!all.is_empty());
        for l in &all {
            assert!(!l.locale.is_empty() && !l.name.is_empty(), "{l:?}");
        }
        if let Some(now) = current() {
            assert!(all.iter().any(|l| language_id(l.hkl) == language_id(now)) || all.len() == 1);
        }
    }
}
