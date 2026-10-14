//! Case folding for matching, with Turkish rules: in Turkish `I` lowers to `ı` and `İ` to
//! `i`, so "EKRANI" finds "Ekranı kilitle" and "ılık" never matches "ilik" (SPEC §2.5).

/// Which case rules apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Casing {
    #[default]
    Turkish,
    /// Unicode default rules (English and other languages).
    Default,
}

/// One folded character and the index of the character it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Folded {
    pub ch: char,
    /// Index into the original text's `chars()`.
    pub from: usize,
}

/// Lowers `c` under `casing`; a few characters lower to more than one.
fn lower(c: char, casing: Casing, out: &mut Vec<char>) {
    match (casing, c) {
        (Casing::Turkish, 'I') => out.push('ı'),
        (Casing::Turkish, 'İ') => out.push('i'),
        // Without Turkish rules the dotted capital is just an i.
        (Casing::Default, 'İ') => out.push('i'),
        _ => out.extend(c.to_lowercase()),
    }
}

/// The folded characters of `text`, each with the index of its original character.
pub fn fold(text: &str, casing: Casing) -> Vec<Folded> {
    let mut out = Vec::with_capacity(text.len());
    let mut buf = Vec::with_capacity(2);
    for (from, c) in text.chars().enumerate() {
        buf.clear();
        lower(c, casing, &mut buf);
        out.extend(buf.iter().map(|&ch| Folded { ch, from }));
    }
    out
}

/// The folded text alone, for sorting and equality.
pub fn fold_str(text: &str, casing: Casing) -> String {
    fold(text, casing).into_iter().map(|f| f.ch).collect()
}

/// The Turkish alphabet, with q, w and x where Latin puts them.
const TURKISH_ORDER: &str = "abcçdefgğhıijklmnoöpqrsştuüvwxyz";

/// A key that sorts folded text alphabetically: in Turkish by the Turkish alphabet (so
/// `ı` comes right before `i`, `ç` after `c`), otherwise by code point. Characters outside
/// the alphabet sort before letters (digits, spaces) or after them, by code point.
pub fn collation_key(folded: &[Folded], casing: Casing) -> Vec<u32> {
    const LETTERS: u32 = 0x10_0000;
    folded
        .iter()
        .map(|f| match casing {
            Casing::Turkish => match TURKISH_ORDER.chars().position(|c| c == f.ch) {
                Some(rank) => LETTERS + rank as u32,
                None if f.ch.is_alphabetic() => 2 * LETTERS + f.ch as u32,
                None => f.ch as u32,
            },
            Casing::Default => f.ch as u32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: the four Turkish i letters both ways, other Turkish letters, the
    //! Unicode default rules, a character that lowers to two, empty text.
    use super::*;

    #[test]
    fn turkish_i_letters_keep_their_dots() {
        assert_eq!(fold_str("EKRANI", Casing::Turkish), "ekranı");
        assert_eq!(fold_str("İstanbul", Casing::Turkish), "istanbul");
        assert_eq!(fold_str("ILIK", Casing::Turkish), "ılık");
        assert_ne!(fold_str("ılık", Casing::Turkish), fold_str("ilik", Casing::Turkish));
        assert_eq!(fold_str("ÇĞÖŞÜ", Casing::Turkish), "çğöşü");
    }

    #[test]
    fn default_rules_lower_i_without_a_dot_change() {
        assert_eq!(fold_str("EKRANI", Casing::Default), "ekrani");
        assert_eq!(fold_str("İstanbul", Casing::Default), "istanbul");
    }

    #[test]
    fn turkish_order_puts_dotless_i_before_i_and_c_cedilla_after_c() {
        let key = |t: &str| collation_key(&fold(t, Casing::Turkish), Casing::Turkish);
        let mut words = vec![
            "ilik",
            "ılık",
            "çam",
            "cam",
            "dut",
            "zil",
            "öykü",
            "ozan",
            "1. bölüm",
            "ä",
        ];
        words.sort_by_key(|w| key(w));
        assert_eq!(
            words,
            vec![
                "1. bölüm",
                "cam",
                "çam",
                "dut",
                "ılık",
                "ilik",
                "ozan",
                "öykü",
                "zil",
                "ä"
            ]
        );
        // Default rules: code point order.
        let d = |t: &str| collation_key(&fold(t, Casing::Default), Casing::Default);
        assert!(d("cam") < d("dut"));
    }

    #[test]
    fn positions_point_at_the_original_characters() {
        let f = fold("Aİb", Casing::Turkish);
        assert_eq!(
            f,
            vec![
                Folded { ch: 'a', from: 0 },
                Folded { ch: 'i', from: 1 },
                Folded { ch: 'b', from: 2 }
            ]
        );
        // Every folded character maps back to a character of the text.
        let text = "ǄxΣ";
        let f = fold(text, Casing::Default);
        assert!(f.iter().all(|c| c.from < text.chars().count()));
        assert_eq!(f.last().map(|c| c.from), Some(2));
        assert!(fold("", Casing::Turkish).is_empty());
    }
}
