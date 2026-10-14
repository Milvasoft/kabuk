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
