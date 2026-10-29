//! Configuration errors and warnings. They are structured so the UI can show them
//! in the user's language; `Display` gives an English line for logs.

use std::fmt;

use super::shortcut::ShortcutError;

/// A path into the configuration, e.g. `rules.rule[2].workspace`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyPath(pub Vec<Segment>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
}

impl KeyPath {
    pub fn of(keys: &[&str]) -> KeyPath {
        KeyPath(keys.iter().map(|k| Segment::Key((*k).to_string())).collect())
    }

    pub fn key(&self, key: &str) -> KeyPath {
        let mut p = self.clone();
        p.0.push(Segment::Key(key.to_string()));
        p
    }

    pub fn index(&self, index: usize) -> KeyPath {
        let mut p = self.clone();
        p.0.push(Segment::Index(index));
        p
    }
}

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                Segment::Key(k) if i == 0 => f.write_str(k)?,
                Segment::Key(k) => write!(f, ".{k}")?,
                Segment::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

/// 1-based line and column (in characters) inside `kabuk.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl Location {
    pub fn from_offset(source: &str, offset: usize) -> Location {
        let offset = offset.min(source.len());
        let before = source.get(..offset).unwrap_or(source);
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        Location { line, column }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file is not valid TOML. Carries the parser's description.
    Syntax(String),
    /// A value has the wrong type or is not one of the allowed words. Carries the decoder's description.
    Invalid(String),
    UnsupportedSchema {
        found: i64,
        supported: u32,
    },
    OutOfRange {
        min: i64,
        max: i64,
    },
    NotOneOf(Vec<i64>),
    InvalidColor(String),
    InvalidShortcut(ShortcutError),
    ShortcutConflict {
        other: KeyPath,
        chord: String,
    },
    InvalidRegex(String),
    InvalidUrl(String),
    RuleWithoutMatch,
    RuleWithoutAction,
    /// A value required by another setting is missing.
    Missing,
    Empty,
    Duplicate,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::Syntax(d) => write!(f, "invalid TOML: {d}"),
            Problem::Invalid(d) => write!(f, "invalid value: {d}"),
            Problem::UnsupportedSchema { found, supported } => {
                write!(f, "schema {found} is newer than the supported schema {supported}")
            }
            Problem::OutOfRange { min, max } => write!(f, "must be between {min} and {max}"),
            Problem::NotOneOf(v) => {
                let list: Vec<String> = v.iter().map(i64::to_string).collect();
                write!(f, "must be one of {}", list.join(", "))
            }
            Problem::InvalidColor(c) => write!(f, "`{c}` is not a #rrggbb colour or \"theme\""),
            Problem::InvalidShortcut(e) => write!(f, "invalid shortcut: {e}"),
            Problem::ShortcutConflict { other, chord } => {
                write!(f, "{chord} is already bound by {other}")
            }
            Problem::InvalidRegex(e) => write!(f, "invalid regular expression: {e}"),
            Problem::InvalidUrl(u) => write!(f, "`{u}` is not an http(s) address"),
            Problem::RuleWithoutMatch => f.write_str("rule needs exe, class or title"),
            Problem::RuleWithoutAction => f.write_str("rule has no action"),
            Problem::Missing => f.write_str("value is required here"),
            Problem::Empty => f.write_str("value is empty"),
            Problem::Duplicate => f.write_str("value is listed twice"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub path: Option<KeyPath>,
    pub location: Option<Location>,
    pub problem: Problem,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = self.location {
            write!(f, "line {}, column {}: ", loc.line, loc.column)?;
        }
        if let Some(path) = &self.path {
            write!(f, "{path}: ")?;
        }
        write!(f, "{}", self.problem)
    }
}

impl std::error::Error for ConfigError {}

/// Non-fatal findings: the file still loads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarningKind {
    UnknownKey,
    /// `schema` was absent; the current version was assumed.
    MissingSchema,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigWarning {
    pub path: KeyPath,
    pub location: Option<Location>,
    pub kind: WarningKind,
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = self.location {
            write!(f, "line {}, column {}: ", loc.line, loc.column)?;
        }
        match self.kind {
            WarningKind::UnknownKey => write!(f, "unknown key `{}` ignored", self.path),
            WarningKind::MissingSchema => f.write_str("`schema` missing, assuming the current version"),
        }
    }
}

/// A shortcut path's words: `shortcuts.focus_left[0]` is the setting "Focus left".
fn setting_name(path: &KeyPath, i18n: &kabuk_i18n::I18n) -> String {
    let key = match path.0.as_slice() {
        [Segment::Key(group), Segment::Key(name), ..] if group == "shortcuts" => {
            format!("set-key-{}", name.replace('_', "-"))
        }
        _ => return path.to_string(),
    };
    let words = i18n.tr(&key);
    if words == key {
        path.to_string()
    } else {
        words
    }
}

impl ShortcutError {
    /// What is wrong with the chord, in the user's language.
    pub fn describe(&self, i18n: &kabuk_i18n::I18n) -> String {
        use kabuk_i18n::Arg;
        let key = |k: &String| [("key", Arg::Str(k.clone()))];
        match self {
            ShortcutError::Empty | ShortcutError::EmptyPart => i18n.tr("cfg-key-empty"),
            ShortcutError::UnknownKey(k) => i18n.tr_args("cfg-key-unknown", &key(k)),
            ShortcutError::NotAModifier(k) => i18n.tr_args("cfg-key-not-modifier", &key(k)),
            ShortcutError::DuplicateModifier(k) => i18n.tr_args("cfg-key-twice", &key(k)),
            ShortcutError::ModifierAfterKey(k) => i18n.tr_args("cfg-key-order", &key(k)),
            ShortcutError::MoreThanOneKey => i18n.tr("cfg-key-two-keys"),
            ShortcutError::MissingKey => i18n.tr("cfg-key-no-key"),
            ShortcutError::NeedsCommandModifier => i18n.tr("cfg-key-needs-modifier"),
            ShortcutError::Reserved => i18n.tr("cfg-key-reserved"),
            ShortcutError::AltGrConflict => i18n.tr("cfg-key-altgr"),
        }
    }
}

impl Problem {
    /// What is wrong, in the user's language (PLAN M19 U1, U2: the notices and Settings
    /// said only that something was refused).
    pub fn describe(&self, i18n: &kabuk_i18n::I18n) -> String {
        use kabuk_i18n::Arg;
        let s = |v: &str| Arg::Str(v.to_string());
        let n = |v: i64| Arg::Num(v as f64);
        match self {
            Problem::Syntax(d) => i18n.tr_args("cfg-syntax", &[("detail", s(d))]),
            Problem::Invalid(d) => i18n.tr_args("cfg-invalid", &[("detail", s(d))]),
            Problem::UnsupportedSchema { found, supported } => i18n.tr_args(
                "cfg-schema",
                &[("found", n(*found)), ("supported", n(i64::from(*supported)))],
            ),
            Problem::OutOfRange { min, max } => i18n.tr_args("cfg-range", &[("min", n(*min)), ("max", n(*max))]),
            Problem::NotOneOf(v) => {
                let list: Vec<String> = v.iter().map(i64::to_string).collect();
                i18n.tr_args("cfg-one-of", &[("list", s(&list.join(", ")))])
            }
            Problem::InvalidColor(c) => i18n.tr_args("cfg-color", &[("value", s(c))]),
            Problem::InvalidShortcut(e) => e.describe(i18n),
            Problem::ShortcutConflict { other, chord } => i18n.tr_args(
                "cfg-shortcut-taken",
                &[("chord", s(chord)), ("other", s(&setting_name(other, i18n)))],
            ),
            Problem::InvalidRegex(d) => i18n.tr_args("cfg-regex", &[("detail", s(d))]),
            Problem::InvalidUrl(u) => i18n.tr_args("cfg-url", &[("value", s(u))]),
            Problem::RuleWithoutMatch => i18n.tr("cfg-rule-match"),
            Problem::RuleWithoutAction => i18n.tr("cfg-rule-action"),
            Problem::Missing => i18n.tr("cfg-missing"),
            Problem::Empty => i18n.tr("cfg-empty"),
            Problem::Duplicate => i18n.tr("cfg-duplicate"),
        }
    }
}

/// `what`, said to be on `line` of the file when it is known.
fn at_line(location: Option<Location>, what: String, i18n: &kabuk_i18n::I18n) -> String {
    match location {
        Some(at) => i18n.tr_args(
            "cfg-at-line",
            &[
                ("line", kabuk_i18n::Arg::Num(at.line as f64)),
                ("what", kabuk_i18n::Arg::Str(what)),
            ],
        ),
        None => what,
    }
}

impl ConfigError {
    /// What is wrong and where, in the user's language.
    pub fn describe(&self, i18n: &kabuk_i18n::I18n) -> String {
        at_line(self.location, self.problem.describe(i18n), i18n)
    }
}

impl ConfigWarning {
    /// What was ignored and where, in the user's language.
    pub fn describe(&self, i18n: &kabuk_i18n::I18n) -> String {
        let what = match self.kind {
            WarningKind::UnknownKey => i18n.tr_args(
                "cfg-unknown-key",
                &[("key", kabuk_i18n::Arg::Str(self.path.to_string()))],
            ),
            WarningKind::MissingSchema => i18n.tr("cfg-no-schema"),
        };
        at_line(self.location, what, i18n)
    }
}

#[cfg(test)]
mod tests {
    //! Every problem and warning has words in both languages, with its values in them;
    //! a shortcut taken is named by the setting that has it.
    use super::*;
    use kabuk_i18n::{I18n, Lang};

    fn all() -> Vec<Problem> {
        let mut v = vec![
            Problem::Syntax("x".into()),
            Problem::Invalid("x".into()),
            Problem::UnsupportedSchema { found: 9, supported: 1 },
            Problem::OutOfRange { min: 1, max: 9 },
            Problem::NotOneOf(vec![1, 2]),
            Problem::InvalidColor("#zz".into()),
            Problem::ShortcutConflict {
                other: KeyPath::of(&["shortcuts", "focus_left"]).index(0),
                chord: "Win+H".into(),
            },
            Problem::InvalidRegex("x".into()),
            Problem::InvalidUrl("ftp://x".into()),
            Problem::RuleWithoutMatch,
            Problem::RuleWithoutAction,
            Problem::Missing,
            Problem::Empty,
            Problem::Duplicate,
        ];
        for e in [
            ShortcutError::Empty,
            ShortcutError::EmptyPart,
            ShortcutError::UnknownKey("Q2".into()),
            ShortcutError::NotAModifier("Q".into()),
            ShortcutError::DuplicateModifier("Ctrl".into()),
            ShortcutError::ModifierAfterKey("Ctrl".into()),
            ShortcutError::MoreThanOneKey,
            ShortcutError::MissingKey,
            ShortcutError::NeedsCommandModifier,
            ShortcutError::Reserved,
            ShortcutError::AltGrConflict,
        ] {
            v.push(Problem::InvalidShortcut(e));
        }
        v
    }

    #[test]
    fn every_problem_has_words_in_both_languages() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for p in all() {
                let words = p.describe(&i18n);
                assert!(
                    !words.is_empty() && !words.starts_with("cfg-"),
                    "{p:?} {lang:?}: {words}"
                );
            }
            let w = ConfigWarning {
                path: KeyPath::of(&["bar", "colour"]),
                location: Some(Location { line: 4, column: 1 }),
                kind: WarningKind::UnknownKey,
            };
            let words = w.describe(&i18n);
            assert!(words.contains("bar.colour") && words.contains('4'), "{words}");
        }
    }

    #[test]
    fn a_taken_shortcut_names_the_setting_that_has_it_and_the_line() {
        let en = I18n::new(Lang::En);
        let e = ConfigError {
            path: None,
            location: Some(Location { line: 12, column: 3 }),
            problem: Problem::ShortcutConflict {
                other: KeyPath::of(&["shortcuts", "focus_left"]).index(0),
                chord: "Win+H".into(),
            },
        };
        let words = e.describe(&en);
        assert!(
            words.contains("Focus left") && words.contains("Win+H") && words.contains("12"),
            "{words}"
        );
        // A path that is no shortcut is named as it is written.
        let other = Problem::ShortcutConflict {
            other: KeyPath::of(&["bar", "x"]),
            chord: "Win+H".into(),
        };
        assert!(other.describe(&en).contains("bar.x"));
    }
}
