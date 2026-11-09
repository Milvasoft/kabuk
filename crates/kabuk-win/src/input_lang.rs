//! The keyboard layouts (DECISIONS D88, D105): the ones installed, the one the window in
//! front types with, and asking that window to switch, as Windows' input indicator does.

use kabuk_core::input_lang::{self, language_id, Keyboard};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, KEY_READ};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList, HKL};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

use crate::jumplist::resolve_resource;
use crate::privacy::Key;
use crate::window::Hwnd;

/// `LOCALE_NAME_MAX_LENGTH`.
const NAME_MAX: usize = 85;
/// `SORT_DEFAULT` in a locale id's high bits.
const SORT_DEFAULT: u32 = 0;
/// Where Windows describes its keyboards, a key each.
const KEYBOARDS: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

/// A keyboard layout: its handle, the short code the bar shows, its language's locale
/// name (`tr-TR`) and the name it is listed by, in Windows' own language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputLang {
    pub hkl: isize,
    pub code: String,
    pub locale: String,
    pub name: String,
}

/// The name of the key describing the keyboard of `hkl`, if Windows has one.
fn keyboard_key(keyboards: &Key, hkl: isize) -> Option<String> {
    match input_lang::keyboard(hkl) {
        Keyboard::Key(number) => Some(format!("{number:08x}")),
        Keyboard::Variant(id) => keyboards
            .children()
            .into_iter()
            .find(|key| u16::from_str_radix(&keyboards.text(key, "Layout Id"), 16).is_ok_and(|found| found == id)),
    }
}

/// The locale of the keyboard in key `key` (a key's low word is its keyboard's language).
fn keyboard_locale(key: &str) -> Option<String> {
    let number = u32::from_str_radix(key, 16).ok()?;
    Some(locale_of(isize::try_from(number).ok()?)).filter(|l| !l.is_empty())
}

/// The keyboard's name in Windows' own language (`Türkçe Q`), empty if it has none.
fn keyboard_name(keyboards: &Key, key: &str) -> String {
    let shown = keyboards.text(key, "Layout Display Name");
    let name = resolve_resource(&shown);
    if name.is_empty() || name.starts_with('@') {
        keyboards.text(key, "Layout Text")
    } else {
        name
    }
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

/// The keyboard layouts installed, in Windows' order (two layouts of one language are two
/// entries, D105). Named by language; by keyboard where a language has several.
pub fn installed() -> Vec<InputLang> {
    // SAFETY: the two-call form: the count, then a buffer that large.
    let count = unsafe { GetKeyboardLayoutList(None) };
    let mut layouts = vec![HKL::default(); usize::try_from(count).unwrap_or(0)];
    // SAFETY: a buffer of the length passed.
    let n = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    layouts.truncate(usize::try_from(n).unwrap_or(0));
    let keyboards = Key::open(HKEY_LOCAL_MACHINE, KEYBOARDS, KEY_READ);
    let hkls: Vec<isize> = layouts.into_iter().map(|h| h.0 as isize).collect();
    let found: Vec<(String, String, String)> = hkls
        .iter()
        .map(|hkl| {
            let language = locale_of(*hkl);
            let key = keyboards.as_ref().and_then(|k| Some((k, keyboard_key(k, *hkl)?)));
            let locale = key.as_ref().and_then(|(_, key)| keyboard_locale(key));
            let name = key.as_ref().map(|(k, key)| keyboard_name(k, key)).unwrap_or_default();
            let keyboard = locale.unwrap_or_else(|| language.clone());
            (language, keyboard, name)
        })
        .collect();
    let pairs: Vec<(&str, &str)> = found.iter().map(|(l, k, _)| (l.as_str(), k.as_str())).collect();
    let codes = input_lang::codes(&pairs);
    hkls.iter()
        .zip(codes)
        .zip(&found)
        .map(|((hkl, code), (language, _, keyboard))| {
            let shared = found.iter().filter(|(l, _, _)| l == language).count() > 1;
            InputLang {
                hkl: *hkl,
                code,
                locale: language.clone(),
                name: if shared && !keyboard.is_empty() {
                    keyboard.clone()
                } else {
                    display_name(language)
                },
            }
        })
        .collect()
}

/// The place in `all` of the layout the window in front types with.
pub fn in_front(all: &[InputLang]) -> Option<usize> {
    let hkls: Vec<isize> = all.iter().map(|l| l.hkl).collect();
    input_lang::in_use(&hkls, current()?)
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
    //! On the machine running the tests: at least one layout is installed, each named and
    //! coded; every layout Windows has is listed, also two of one language; the one in
    //! front is among them when there is a window in front.
    use super::*;

    #[test]
    fn the_installed_languages_are_named() {
        let all = installed();
        assert!(!all.is_empty());
        for l in &all {
            assert!(
                !l.locale.is_empty() && !l.name.is_empty() && !l.code.is_empty(),
                "{l:?}"
            );
        }
        if let Some(now) = current() {
            assert!(all.iter().any(|l| language_id(l.hkl) == language_id(now)) || all.len() == 1);
        }
    }

    /// Found using Kabuk: two layouts of one language were listed as one, and the bar's
    /// indicator hid.
    #[test]
    fn every_layout_windows_has_is_listed() {
        // SAFETY: the count form of the call, with no buffer.
        let count = unsafe { GetKeyboardLayoutList(None) };
        assert_eq!(installed().len(), usize::try_from(count).unwrap_or(0));
    }

    #[test]
    fn the_codes_do_not_repeat() {
        let all = installed();
        for l in &all {
            assert_eq!(all.iter().filter(|o| o.code == l.code).count(), 1, "{all:?}");
        }
    }
}
