//! Keyboard shortcut chords: parsing, canonical formatting and the rules that keep
//! shortcuts away from keys Windows reserves or that Turkish keyboards need.
//!
//! Edge cases tested here: empty input, stray `+`, duplicate modifiers, modifier-only
//! chords, unknown keys, case-insensitivity, aliases, every key class (letter, digit,
//! function, named), canonical round trip, reserved chords, AltGr (Ctrl+Alt) chords on
//! character keys, chords without a "command" modifier.

use std::fmt;

/// A set of modifier keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    pub const ALT: Mods = Mods {
        ctrl: false,
        alt: true,
        shift: false,
        win: false,
    };
    pub const ALT_SHIFT: Mods = Mods {
        ctrl: false,
        alt: true,
        shift: true,
        win: false,
    };

    pub fn is_empty(self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.win)
    }

    /// Ctrl, Alt or Win: a modifier that turns typing into a command.
    pub fn has_command_modifier(self) -> bool {
        self.ctrl || self.alt || self.win
    }

    /// Parses a modifier-only value such as `Alt` or `Alt+Shift`.
    pub fn parse(text: &str) -> Result<Mods, ShortcutError> {
        let mut mods = Mods::default();
        for part in split_parts(text)? {
            match modifier_of(part) {
                Some(m) => add_modifier(&mut mods, m)?,
                None => return Err(ShortcutError::NotAModifier(part.to_string())),
            }
        }
        if !mods.has_command_modifier() {
            return Err(ShortcutError::NeedsCommandModifier);
        }
        Ok(mods)
    }
}

impl fmt::Display for Mods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        // The Win key first, as Windows writes its own (Win+Shift+S, Win+Ctrl+D).
        for (on, name) in [
            (self.win, "Win"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ] {
            if on {
                if !first {
                    f.write_str("+")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        Ok(())
    }
}

/// A non-modifier key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// `A`..=`Z`, stored upper case.
    Letter(char),
    /// `0`..=`9`.
    Digit(u8),
    /// `F1`..=`F24`.
    Function(u8),
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
}

const NAMED_KEYS: &[(Key, &str, &[&str])] = &[
    (Key::Space, "Space", &[]),
    (Key::Enter, "Enter", &["Return"]),
    (Key::Tab, "Tab", &[]),
    (Key::Escape, "Esc", &["Escape"]),
    (Key::Backspace, "Backspace", &[]),
    (Key::Delete, "Delete", &["Del"]),
    (Key::Insert, "Insert", &["Ins"]),
    (Key::Home, "Home", &[]),
    (Key::End, "End", &[]),
    (Key::PageUp, "PageUp", &["PgUp"]),
    (Key::PageDown, "PageDown", &["PgDn"]),
    (Key::Left, "Left", &[]),
    (Key::Right, "Right", &[]),
    (Key::Up, "Up", &[]),
    (Key::Down, "Down", &[]),
];

impl Key {
    fn parse(text: &str) -> Option<Key> {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_ascii_alphabetic() {
                return Some(Key::Letter(c.to_ascii_uppercase()));
            }
            if let Some(d) = c.to_digit(10) {
                return u8::try_from(d).ok().map(Key::Digit);
            }
            return None;
        }
        if let Some(rest) = text.strip_prefix(['F', 'f']) {
            if let Ok(n) = rest.parse::<u8>() {
                return (1..=24).contains(&n).then_some(Key::Function(n));
            }
        }
        NAMED_KEYS.iter().find_map(|(key, name, aliases)| {
            let hit = name.eq_ignore_ascii_case(text) || aliases.iter().any(|a| a.eq_ignore_ascii_case(text));
            hit.then_some(*key)
        })
    }

    /// Keys that type a character; Ctrl+Alt on these is AltGr on Turkish layouts.
    pub fn is_character(self) -> bool {
        matches!(self, Key::Letter(_) | Key::Digit(_))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Letter(c) => write!(f, "{c}"),
            Key::Digit(d) => write!(f, "{d}"),
            Key::Function(n) => write!(f, "F{n}"),
            named => {
                let name = NAMED_KEYS
                    .iter()
                    .find(|(k, _, _)| k == named)
                    .map(|(_, name, _)| *name)
                    .unwrap_or_default();
                f.write_str(name)
            }
        }
    }
}

/// Modifiers plus one key, e.g. `Alt+Shift+Q`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

/// The emergency restore chord. Fixed; never configurable.
pub const EMERGENCY_CHORD: Chord = Chord {
    mods: Mods {
        ctrl: true,
        alt: true,
        shift: true,
        win: false,
    },
    key: Key::End,
};

/// Chords Windows keeps for itself (cannot be captured) or Kabuk reserves.
const RESERVED: &[Chord] = &[
    // Win+L locks the workstation and cannot be intercepted.
    Chord {
        mods: Mods {
            ctrl: false,
            alt: false,
            shift: false,
            win: true,
        },
        key: Key::Letter('L'),
    },
    // Secure attention sequence.
    Chord {
        mods: Mods {
            ctrl: true,
            alt: true,
            shift: false,
            win: false,
        },
        key: Key::Delete,
    },
    // Task Manager.
    Chord {
        mods: Mods {
            ctrl: true,
            alt: false,
            shift: true,
            win: false,
        },
        key: Key::Escape,
    },
    // Input language switch.
    Chord {
        mods: Mods {
            ctrl: false,
            alt: false,
            shift: false,
            win: true,
        },
        key: Key::Space,
    },
    EMERGENCY_CHORD,
];

impl Chord {
    /// Parses a chord without applying the usage rules (see [`Chord::parse_bindable`]).
    pub fn parse(text: &str) -> Result<Chord, ShortcutError> {
        let mut mods = Mods::default();
        let mut key = None;
        for part in split_parts(text)? {
            if let Some(m) = modifier_of(part) {
                if key.is_some() {
                    return Err(ShortcutError::ModifierAfterKey(part.to_string()));
                }
                add_modifier(&mut mods, m)?;
            } else if key.is_some() {
                return Err(ShortcutError::MoreThanOneKey);
            } else {
                key = Some(Key::parse(part).ok_or_else(|| ShortcutError::UnknownKey(part.to_string()))?);
            }
        }
        let key = key.ok_or(ShortcutError::MissingKey)?;
        Ok(Chord { mods, key })
    }

    /// Parses a chord and checks it can be bound as a Kabuk shortcut.
    pub fn parse_bindable(text: &str) -> Result<Chord, ShortcutError> {
        let chord = Chord::parse(text)?;
        chord.check_bindable()?;
        Ok(chord)
    }

    /// Rules for a user-assignable chord.
    pub fn check_bindable(&self) -> Result<(), ShortcutError> {
        if RESERVED.contains(self) {
            return Err(ShortcutError::Reserved);
        }
        if !self.mods.has_command_modifier() {
            return Err(ShortcutError::NeedsCommandModifier);
        }
        if self.mods.ctrl && self.mods.alt && self.key.is_character() {
            return Err(ShortcutError::AltGrConflict);
        }
        Ok(())
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.is_empty() {
            write!(f, "{}", self.key)
        } else {
            write!(f, "{}+{}", self.mods, self.key)
        }
    }
}

/// Why a shortcut text was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    #[error("shortcut is empty")]
    Empty,
    #[error("shortcut has an empty part (stray `+`)")]
    EmptyPart,
    #[error("unknown key `{0}`")]
    UnknownKey(String),
    #[error("`{0}` is not a modifier (expected Ctrl, Alt, Shift or Win)")]
    NotAModifier(String),
    #[error("modifier `{0}` appears more than once")]
    DuplicateModifier(String),
    #[error("modifier `{0}` comes after the key")]
    ModifierAfterKey(String),
    #[error("shortcut has more than one key")]
    MoreThanOneKey,
    #[error("shortcut has no key")]
    MissingKey,
    #[error("shortcut needs Ctrl, Alt or Win")]
    NeedsCommandModifier,
    #[error("shortcut is reserved by Windows or by Kabuk")]
    Reserved,
    #[error("Ctrl+Alt on a character key is AltGr on Turkish keyboards")]
    AltGrConflict,
}

#[derive(Clone, Copy)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

fn split_parts(text: &str) -> Result<Vec<&str>, ShortcutError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ShortcutError::Empty);
    }
    text.split('+')
        .map(|p| {
            let p = p.trim();
            if p.is_empty() {
                Err(ShortcutError::EmptyPart)
            } else {
                Ok(p)
            }
        })
        .collect()
}

fn modifier_of(part: &str) -> Option<Modifier> {
    let is = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(part));
    if is(&["Ctrl", "Control"]) {
        Some(Modifier::Ctrl)
    } else if is(&["Alt"]) {
        Some(Modifier::Alt)
    } else if is(&["Shift"]) {
        Some(Modifier::Shift)
    } else if is(&["Win", "Super", "Meta"]) {
        Some(Modifier::Win)
    } else {
        None
    }
}

fn add_modifier(mods: &mut Mods, m: Modifier) -> Result<(), ShortcutError> {
    let (slot, name) = match m {
        Modifier::Ctrl => (&mut mods.ctrl, "Ctrl"),
        Modifier::Alt => (&mut mods.alt, "Alt"),
        Modifier::Shift => (&mut mods.shift, "Shift"),
        Modifier::Win => (&mut mods.win, "Win"),
    };
    if *slot {
        return Err(ShortcutError::DuplicateModifier(name.to_string()));
    }
    *slot = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    #[test]
    fn parses_modifiers_and_letter() {
        let c = chord("Alt+Shift+q");
        assert_eq!(c.mods, Mods::ALT_SHIFT);
        assert_eq!(c.key, Key::Letter('Q'));
    }

    #[test]
    fn parsing_is_case_insensitive_and_trims_spaces() {
        assert_eq!(chord(" alt + SPACE "), chord("Alt+Space"));
        assert_eq!(chord("control+escape"), chord("Ctrl+Esc"));
        assert_eq!(chord("super+k"), chord("Win+K"));
    }

    #[test]
    fn parses_every_key_class() {
        assert_eq!(chord("Alt+7").key, Key::Digit(7));
        assert_eq!(chord("Alt+F12").key, Key::Function(12));
        assert_eq!(chord("Alt+f24").key, Key::Function(24));
        assert_eq!(chord("Alt+PgUp").key, Key::PageUp);
        assert_eq!(chord("Alt+Return").key, Key::Enter);
        for (key, name, _) in NAMED_KEYS {
            assert_eq!(chord(&format!("Alt+{name}")).key, *key);
        }
    }

    #[test]
    fn rejects_malformed_text() {
        assert_eq!(Chord::parse(""), Err(ShortcutError::Empty));
        assert_eq!(Chord::parse("   "), Err(ShortcutError::Empty));
        assert_eq!(Chord::parse("Alt++Q"), Err(ShortcutError::EmptyPart));
        assert_eq!(Chord::parse("Alt+"), Err(ShortcutError::EmptyPart));
        assert_eq!(Chord::parse("Alt+Shift"), Err(ShortcutError::MissingKey));
        assert_eq!(Chord::parse("Alt+Q+W"), Err(ShortcutError::MoreThanOneKey));
        assert_eq!(
            Chord::parse("Alt+Alt+Q"),
            Err(ShortcutError::DuplicateModifier("Alt".into()))
        );
        assert_eq!(
            Chord::parse("Q+Alt"),
            Err(ShortcutError::ModifierAfterKey("Alt".into()))
        );
        assert_eq!(Chord::parse("Alt+F25"), Err(ShortcutError::UnknownKey("F25".into())));
        assert_eq!(Chord::parse("Alt+F0"), Err(ShortcutError::UnknownKey("F0".into())));
        assert_eq!(Chord::parse("Alt+ş"), Err(ShortcutError::UnknownKey("ş".into())));
    }

    #[test]
    fn formats_canonically() {
        assert_eq!(chord("shift+win+alt+ctrl+k").to_string(), "Win+Ctrl+Alt+Shift+K");
        assert_eq!(chord("alt+escape").to_string(), "Alt+Esc");
        assert_eq!(chord("F5").to_string(), "F5");
    }

    #[test]
    fn reserved_chords_are_not_bindable() {
        for text in [
            "Win+L",
            "Ctrl+Alt+Delete",
            "Ctrl+Shift+Esc",
            "Win+Space",
            "Ctrl+Alt+Shift+End",
        ] {
            assert_eq!(Chord::parse_bindable(text), Err(ShortcutError::Reserved), "{text}");
        }
    }

    #[test]
    fn altgr_chords_on_character_keys_are_not_bindable() {
        assert_eq!(Chord::parse_bindable("Ctrl+Alt+Q"), Err(ShortcutError::AltGrConflict));
        assert_eq!(Chord::parse_bindable("Ctrl+Alt+2"), Err(ShortcutError::AltGrConflict));
        // Not a character key: fine.
        assert!(Chord::parse_bindable("Ctrl+Alt+F1").is_ok());
    }

    #[test]
    fn chords_need_a_command_modifier() {
        assert_eq!(Chord::parse_bindable("Q"), Err(ShortcutError::NeedsCommandModifier));
        assert_eq!(
            Chord::parse_bindable("Shift+Q"),
            Err(ShortcutError::NeedsCommandModifier)
        );
        assert!(Chord::parse_bindable("Alt+Shift+Q").is_ok());
    }

    #[test]
    fn default_style_chords_are_bindable() {
        for text in [
            "Alt+Space",
            "Alt+1",
            "Alt+Shift+9",
            "Alt+H",
            "Alt+Left",
            "Alt+Shift+Space",
            "Alt+Shift+Q",
            "Alt+Shift+T",
            "Win+Q",
            "Win+H",
        ] {
            assert!(Chord::parse_bindable(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn modifier_only_values() {
        assert_eq!(Mods::parse("Alt"), Ok(Mods::ALT));
        assert_eq!(Mods::parse("shift + alt"), Ok(Mods::ALT_SHIFT));
        assert_eq!(Mods::parse("Shift"), Err(ShortcutError::NeedsCommandModifier));
        assert_eq!(Mods::parse("Alt+Q"), Err(ShortcutError::NotAModifier("Q".into())));
        assert_eq!(Mods::parse(""), Err(ShortcutError::Empty));
        assert_eq!(Mods::ALT_SHIFT.to_string(), "Alt+Shift");
    }

    fn arb_key() -> impl Strategy<Value = Key> {
        prop_oneof![
            (b'A'..=b'Z').prop_map(|b| Key::Letter(b as char)),
            (0u8..=9).prop_map(Key::Digit),
            (1u8..=24).prop_map(Key::Function),
            proptest::sample::select(NAMED_KEYS.iter().map(|(k, _, _)| *k).collect::<Vec<_>>()),
        ]
    }

    proptest! {
        #[test]
        fn display_then_parse_round_trips(ctrl: bool, alt: bool, shift: bool, win: bool, key in arb_key()) {
            let c = Chord { mods: Mods { ctrl, alt, shift, win }, key };
            prop_assert_eq!(Chord::parse(&c.to_string()), Ok(c));
        }
    }
}
