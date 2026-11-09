//! The keyboard layouts as the bar's keyboard language indicator shows them (DECISIONS
//! D88, D105). Pure.

/// The short code the bar shows for a locale name: its language, upper case
/// (`"tr-TR"` → `"TR"`, `"zh-Hans-CN"` → `"ZH"`).
pub fn short(locale: &str) -> String {
    locale.split('-').next().unwrap_or_default().to_uppercase()
}

/// Whether the indicator shows: only with more than one keyboard layout to choose from,
/// as Windows' taskbar (two layouts of one language count, D105).
pub fn shown(layouts: usize) -> bool {
    layouts > 1
}

/// The language id of a keyboard layout handle (its low word).
pub fn language_id(hkl: isize) -> u16 {
    (hkl as usize & 0xFFFF) as u16
}

/// Where a layout handle's keyboard is described under Windows' `Keyboard Layouts` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// The key's number (`0x041F` is the key `0000041f`).
    Key(u32),
    /// A variant such as Dvorak: the key whose `Layout Id` value is this.
    Variant(u16),
}

/// The keyboard of a layout handle, from its high word: `0xFnnn` is a variant, `0xEnnn`
/// an input method whose key is the whole handle, anything else the key's number.
pub fn keyboard(hkl: isize) -> Keyboard {
    const KIND: u16 = 0xF000;
    const VARIANT: u16 = 0xF000;
    const INPUT_METHOD: u16 = 0xE000;
    let word = ((hkl as usize >> 16) & 0xFFFF) as u16;
    match word & KIND {
        VARIANT => Keyboard::Variant(word & !KIND),
        INPUT_METHOD => Keyboard::Key(hkl as usize as u32),
        _ => Keyboard::Key(u32::from(word)),
    }
}

/// The codes the bar shows for layouts given as (language locale, keyboard locale), in
/// their order (D105). A layout alone in its language shows the language; layouts sharing
/// one are told apart by their keyboards (`en-US` typed on Turkish Q and on US: `TR`,
/// `EN`). A code that would still repeat takes its count (`EN`, `EN2`).
pub fn codes(layouts: &[(&str, &str)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(layouts.len());
    for (language, keyboard) in layouts {
        let shared = layouts.iter().filter(|(l, _)| l == language).count() > 1;
        let code = short(if shared { keyboard } else { language });
        let same = out
            .iter()
            .filter(|c| c.trim_end_matches(|d: char| d.is_ascii_digit()) == code)
            .count();
        out.push(if same == 0 { code } else { format!("{code}{}", same + 1) });
    }
    out
}

/// The place in `layouts` of the layout `now`: that layout, or else the first of its
/// language (a window can hold a layout that is not listed).
pub fn in_use(layouts: &[isize], now: isize) -> Option<usize> {
    layouts
        .iter()
        .position(|l| *l == now)
        .or_else(|| layouts.iter().position(|l| language_id(*l) == language_id(now)))
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

    #[test]
    fn a_layout_names_its_keyboard_in_its_high_word() {
        assert_eq!(keyboard(0x041F_0409), Keyboard::Key(0x041F));
        assert_eq!(keyboard(0x0409_0409), Keyboard::Key(0x0409));
        assert_eq!(keyboard(0xF002_0409_u32 as isize), Keyboard::Variant(2));
        assert_eq!(keyboard(0xE001_0411_u32 as isize), Keyboard::Key(0xE001_0411));
    }

    /// Found using Kabuk: English typed on Turkish Q and on US are two layouts of one
    /// language; counted per language the indicator never showed.
    #[test]
    fn two_keyboards_of_one_language_are_told_apart() {
        assert_eq!(codes(&[("en-US", "tr-TR"), ("en-US", "en-US")]), ["TR", "EN"]);
    }

    #[test]
    fn a_layout_alone_in_its_language_shows_the_language() {
        assert_eq!(codes(&[("tr-TR", "tr-TR"), ("en-US", "tr-TR")]), ["TR", "EN"]);
    }

    #[test]
    fn a_code_that_would_repeat_takes_its_count() {
        assert_eq!(
            codes(&[("en-US", "en-US"), ("en-US", "en-US"), ("en-US", "en-US")]),
            ["EN", "EN2", "EN3"]
        );
        assert_eq!(
            codes(&[("en-US", "tr-TR"), ("en-US", "en-US"), ("tr-TR", "tr-TR")]),
            ["TR", "EN", "TR2"]
        );
        assert!(codes(&[]).is_empty());
    }

    #[test]
    fn the_layout_in_use_is_found_by_its_whole_handle() {
        let layouts = [0x041F_0409, 0x0409_0409];
        assert_eq!(in_use(&layouts, 0x0409_0409), Some(1));
        assert_eq!(in_use(&layouts, 0x041F_0409), Some(0));
    }

    #[test]
    fn a_layout_not_listed_falls_to_its_language() {
        let layouts = [0x041F_041F, 0x0409_0409];
        assert_eq!(in_use(&layouts, 0x0809_0409), Some(1));
        assert_eq!(in_use(&layouts, 0x0411_0411), None);
    }
}
