//! The input languages as the bar's keyboard language indicator shows them (DECISIONS
//! D88). Pure.

/// The short code the bar shows for a locale name: its language, upper case
/// (`"tr-TR"` → `"TR"`, `"zh-Hans-CN"` → `"ZH"`).
pub fn short(locale: &str) -> String {
    locale.split('-').next().unwrap_or_default().to_uppercase()
}

/// Whether the indicator shows: only with more than one input language, as Windows'
/// taskbar.
pub fn shown(languages: usize) -> bool {
    languages > 1
}

/// The language id of a keyboard layout handle (its low word).
pub fn language_id(hkl: isize) -> u16 {
    (hkl as usize & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    //! Edge cases: a region, a script and a region, a bare language, nothing; one and two
    //! languages; a layout handle with a device word over the language word.
    use super::*;

    #[test]
    fn a_locale_shows_its_language() {
        assert_eq!(short("tr-TR"), "TR");
        assert_eq!(short("zh-Hans-CN"), "ZH");
        assert_eq!(short("en"), "EN");
        assert_eq!(short(""), "");
    }

    #[test]
    fn one_language_shows_nothing() {
        assert!(!shown(0));
        assert!(!shown(1));
        assert!(shown(2));
    }

    #[test]
    fn a_layout_names_its_language_in_its_low_word() {
        assert_eq!(language_id(0x041F_041F), 0x041F);
        assert_eq!(language_id(0xF002_0409_u32 as isize), 0x0409);
    }
}
