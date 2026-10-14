//! `kabuk.toml`: schema, parsing with line/column errors, unknown-key warnings,
//! schema migration, and keeping the last valid configuration on a bad edit.

pub mod edit;
mod error;
mod schema;
pub mod shortcut;
mod validate;

use std::ops::Range;
use std::path::Path;

use toml::de::{DeTable, DeValue};
use toml::Spanned;

pub use error::{ConfigError, ConfigWarning, KeyPath, Location, Problem, Segment, WarningKind};
pub use schema::*;
pub use validate::parse_hex_color;

/// A successfully parsed configuration and the warnings found on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<ConfigWarning>,
}

/// What the configuration file held when it was last looked at, so a change in its folder
/// (Kabuk's own state file, a temporary file of an editor's save) reloads only when the
/// file itself changed.
#[derive(Debug, Default)]
pub struct FileSeen {
    last: Option<Option<String>>,
}

impl FileSeen {
    /// Starts from the text loaded at startup (`None`: no file).
    pub fn new(text: Option<String>) -> FileSeen {
        FileSeen { last: Some(text) }
    }

    /// True when `now` (the file's text, `None` when it is gone) differs from what was seen.
    pub fn changed(&mut self, now: Option<String>) -> bool {
        let changed = self.last.as_ref() != Some(&now);
        self.last = Some(now);
        changed
    }
}

/// Upgrades a document from schema `N` to `N + 1`. `MIGRATIONS[i]` upgrades `i + 1` to `i + 2`.
pub type Migration = fn(&mut toml::Table);

/// No older schema exists yet; the first migration is added with schema 2.
const MIGRATIONS: &[Migration] = &[];

/// Parses and validates a `kabuk.toml` document. All errors are returned at once.
pub fn parse(source: &str) -> Result<Loaded, Vec<ConfigError>> {
    parse_with(source, SCHEMA_VERSION, MIGRATIONS)
}

/// Serializes a configuration as a `kabuk.toml` document.
pub fn to_toml(config: &Config) -> String {
    // Serializing plain structs, strings, integers and booleans cannot fail.
    toml::to_string(config).unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{} error(s) in the configuration", .0.len())]
    Invalid(Vec<ConfigError>),
}

/// Reads `path`. A missing file yields the defaults.
pub fn load_file(path: &Path) -> Result<Loaded, LoadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(LoadError::Invalid),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Loaded {
            config: Config::default(),
            warnings: Vec::new(),
        }),
        Err(source) => Err(LoadError::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

/// Writes the configuration atomically (temporary file, then replace).
pub fn write_file(path: &Path, config: &Config) -> std::io::Result<()> {
    crate::fsutil::write_atomic(path, to_toml(config).as_bytes())
}

/// Holds the configuration in use. A reload that fails keeps the previous one.
#[derive(Clone, Debug, Default)]
pub struct LiveConfig {
    current: Config,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reload {
    Applied { warnings: Vec<ConfigWarning> },
    Rejected { errors: Vec<ConfigError> },
}

impl LiveConfig {
    pub fn new(config: Config) -> LiveConfig {
        LiveConfig { current: config }
    }

    pub fn current(&self) -> &Config {
        &self.current
    }

    pub fn reload(&mut self, source: &str) -> Reload {
        match parse(source) {
            Ok(loaded) => {
                self.current = loaded.config;
                Reload::Applied {
                    warnings: loaded.warnings,
                }
            }
            Err(errors) => Reload::Rejected { errors },
        }
    }
}

fn parse_with(source: &str, current: u32, migrations: &[Migration]) -> Result<Loaded, Vec<ConfigError>> {
    let root = DeTable::parse(source).map_err(|e| vec![syntax_error(source, &e)])?;
    let mut warnings = Vec::new();

    let version = match find(&root, &KeyPath::of(&["schema"])) {
        None => {
            warnings.push(ConfigWarning {
                path: KeyPath::of(&["schema"]),
                location: None,
                kind: WarningKind::MissingSchema,
            });
            current
        }
        Some(node) => {
            let error = |problem| {
                vec![ConfigError {
                    path: Some(KeyPath::of(&["schema"])),
                    location: Some(Location::from_offset(source, node.value.span().start)),
                    problem,
                }]
            };
            let found = match node.value.get_ref() {
                DeValue::Integer(i) => i.as_str().parse::<i64>().ok(),
                _ => None,
            };
            match found {
                Some(v) if v >= 1 && v <= i64::from(current) => u32::try_from(v).unwrap_or(current),
                Some(v) => {
                    return Err(error(Problem::UnsupportedSchema {
                        found: v,
                        supported: current,
                    }))
                }
                None => return Err(error(Problem::Invalid("schema must be an integer".into()))),
            }
        }
    };

    // A migrated document no longer matches the file, so its errors carry no location.
    let (text, located) = if version < current {
        let mut table: toml::Table = toml::from_str(source).map_err(|e| vec![syntax_error(source, &e)])?;
        let first = usize::try_from(version - 1).unwrap_or(0);
        let last = usize::try_from(current - 1).unwrap_or(0);
        for migrate in migrations.get(first..last).unwrap_or_default() {
            migrate(&mut table);
        }
        table.insert("schema".into(), toml::Value::Integer(i64::from(current)));
        (toml::to_string(&table).unwrap_or_default(), false)
    } else {
        (source.to_string(), true)
    };
    let locate = |offset: usize| located.then(|| Location::from_offset(source, offset));

    let doc = DeTable::parse(&text).map_err(|e| vec![syntax_error(&text, &e)])?;
    let mut unknown = Vec::new();
    let config: Config = serde_ignored::deserialize(toml::Deserializer::from(doc), |path| {
        unknown.push(path_from_ignored(&path));
    })
    .map_err(|e| {
        let span = e.span();
        vec![ConfigError {
            path: span
                .as_ref()
                .map(|s| path_at(&root, s.start))
                .filter(|p| !p.0.is_empty()),
            location: span.and_then(|s| locate(s.start)),
            problem: Problem::Invalid(e.message().to_string()),
        }]
    })?;

    for path in unknown {
        let location = find(&root, &path).and_then(|n| locate(n.key.start));
        warnings.push(ConfigWarning {
            path,
            location,
            kind: WarningKind::UnknownKey,
        });
    }

    let errors: Vec<ConfigError> = validate::validate(&config)
        .into_iter()
        .map(|(path, problem)| {
            // A conflict with a default shortcut is reported on the shortcut the user wrote.
            let (path, problem) = match problem {
                Problem::ShortcutConflict { other, chord } if !in_file(&root, &path) && in_file(&root, &other) => {
                    (other, Problem::ShortcutConflict { other: path, chord })
                }
                problem => (path, problem),
            };
            ConfigError {
                location: find(&root, &path).and_then(|n| locate(n.value.span().start)),
                path: Some(path),
                problem,
            }
        })
        .collect();
    if errors.is_empty() {
        Ok(Loaded { config, warnings })
    } else {
        Err(errors)
    }
}

fn syntax_error(source: &str, e: &toml::de::Error) -> ConfigError {
    ConfigError {
        path: None,
        location: e.span().map(|s| Location::from_offset(source, s.start)),
        problem: Problem::Syntax(e.message().to_string()),
    }
}

fn path_from_ignored(path: &serde_ignored::Path<'_>) -> KeyPath {
    fn walk(path: &serde_ignored::Path<'_>, out: &mut Vec<Segment>) {
        match path {
            serde_ignored::Path::Root => {}
            serde_ignored::Path::Seq { parent, index } => {
                walk(parent, out);
                out.push(Segment::Index(*index));
            }
            serde_ignored::Path::Map { parent, key } => {
                walk(parent, out);
                out.push(Segment::Key(key.clone()));
            }
            serde_ignored::Path::Some { parent }
            | serde_ignored::Path::NewtypeStruct { parent }
            | serde_ignored::Path::NewtypeVariant { parent } => walk(parent, out),
        }
    }
    let mut out = Vec::new();
    walk(path, &mut out);
    KeyPath(out)
}

/// A located node: the span of its key (or index entry) and its value.
struct Node<'a, 'i> {
    key: Range<usize>,
    value: &'a Spanned<DeValue<'i>>,
}

/// Finds the deepest existing node along `path`; `None` when even the first segment is absent.
fn find<'a, 'i>(root: &'a Spanned<DeTable<'i>>, path: &KeyPath) -> Option<Node<'a, 'i>> {
    find_with_depth(root, path).map(|(node, _)| node)
}

/// True when the whole `path` is written in the file.
fn in_file(root: &Spanned<DeTable<'_>>, path: &KeyPath) -> bool {
    find_with_depth(root, path).is_some_and(|(_, depth)| depth == path.0.len())
}

fn find_with_depth<'a, 'i>(root: &'a Spanned<DeTable<'i>>, path: &KeyPath) -> Option<(Node<'a, 'i>, usize)> {
    let mut depth = 0;
    let mut table = Some(root.get_ref());
    let mut array: Option<&'a [Spanned<DeValue<'i>>]> = None;
    let mut found: Option<Node<'a, 'i>> = None;
    for seg in &path.0 {
        let next = match (seg, table, array) {
            (Segment::Key(k), Some(t), _) => t
                .iter()
                .find(|(key, _)| key.get_ref().as_ref() == k.as_str())
                .map(|(key, value)| Node { key: key.span(), value }),
            (Segment::Index(i), _, Some(items)) => items.get(*i).map(|value| Node {
                key: value.span(),
                value,
            }),
            _ => None,
        };
        let Some(node) = next else { break };
        table = node.value.get_ref().as_table();
        array = node.value.get_ref().as_array().map(|a| &a[..]);
        found = Some(node);
        depth += 1;
    }
    found.map(|node| (node, depth))
}

/// The path of the deepest node whose key or value span contains `offset`. A table
/// opened by a `[header]` does not span its body, so tables are searched even when
/// their own span does not contain the offset.
fn path_at(root: &Spanned<DeTable<'_>>, offset: usize) -> KeyPath {
    fn children<'a, 'i>(value: &'a DeValue<'i>) -> Vec<(Segment, Range<usize>, &'a Spanned<DeValue<'i>>)> {
        if let Some(table) = value.as_table() {
            table
                .iter()
                .map(|(k, v)| (Segment::Key(k.get_ref().to_string()), k.span(), v))
                .collect()
        } else if let Some(items) = value.as_array() {
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Segment::Index(i), v.span(), v))
                .collect()
        } else {
            Vec::new()
        }
    }
    fn walk(value: &DeValue<'_>, offset: usize, out: &mut Vec<Segment>) -> bool {
        for (seg, key_span, v) in children(value) {
            let direct = key_span.contains(&offset) || v.span().contains(&offset);
            out.push(seg);
            if walk(v.get_ref(), offset, out) || direct {
                return true;
            }
            out.pop();
        }
        false
    }
    let mut out = Vec::new();
    walk(&DeValue::Table(root.get_ref().clone()), offset, &mut out);
    KeyPath(out)
}

#[cfg(test)]
mod tests;
