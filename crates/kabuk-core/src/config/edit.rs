//! Changing one setting in `kabuk.toml` the way the settings app does: only that key
//! changes, the user's comments, order and other keys stay as written, and a change that
//! would make the file invalid is refused (the file is left as it was). A file without
//! a `schema` gets the current one.

use serde_json::Value;
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table};

use super::{parse, ConfigError};

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("the file is not valid TOML: {0}")]
    Syntax(String),
    #[error("`{0}` is not a setting path")]
    BadPath(String),
    #[error("a value of this kind cannot be written: {0}")]
    BadValue(String),
    #[error("{} error(s) after the change", .0.len())]
    Invalid(Vec<ConfigError>),
}

fn scalar(v: &Value) -> Result<toml_edit::Value, EditError> {
    Ok(match v {
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.into(),
            (None, Some(f)) => f.into(),
            _ => return Err(EditError::BadValue(n.to_string())),
        },
        Value::String(s) => s.as_str().into(),
        Value::Array(items) => {
            let mut a = Array::new();
            for i in items {
                a.push(scalar(i)?);
            }
            a.into()
        }
        Value::Object(map) => {
            let mut t = InlineTable::new();
            for (k, v) in map {
                t.insert(k, scalar(v)?);
            }
            t.into()
        }
        Value::Null => return Err(EditError::BadValue("null".into())),
    })
}

/// An array of objects becomes an array of tables (`[[rules.rule]]`), anything else a value.
fn item(v: &Value) -> Result<Item, EditError> {
    if let Value::Array(items) = v {
        if !items.is_empty() && items.iter().all(Value::is_object) {
            let mut tables = ArrayOfTables::new();
            for o in items {
                let mut t = Table::new();
                for (k, v) in o.as_object().into_iter().flatten() {
                    t.insert(k, value(scalar(v)?));
                }
                tables.push(t);
            }
            return Ok(Item::ArrayOfTables(tables));
        }
    }
    Ok(value(scalar(v)?))
}

/// `source` with the setting at `path` (`"windows.gap"`) set to `new`, or removed (back
/// to its default) when `new` is `null`. Missing tables are created.
pub fn set(source: &str, path: &str, new: &Value) -> Result<String, EditError> {
    let mut doc: DocumentMut = source
        .parse()
        .map_err(|e: toml_edit::TomlError| EditError::Syntax(e.to_string()))?;
    let keys: Vec<&str> = path.split('.').collect();
    if keys.iter().any(|k| k.is_empty()) {
        return Err(EditError::BadPath(path.into()));
    }
    let (last, parents) = keys.split_last().ok_or_else(|| EditError::BadPath(path.into()))?;
    let mut table: &mut Table = doc.as_table_mut();
    for k in parents {
        let entry = table.entry(k).or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        table = entry.as_table_mut().ok_or_else(|| EditError::BadPath(path.into()))?;
    }
    if new.is_null() {
        table.remove(last);
    } else {
        let it = item(new)?;
        // Keep the comment and spacing in front of an existing value.
        match table.get_mut(last) {
            Some(Item::Value(old)) if it.is_value() => {
                let decor = old.decor().clone();
                let mut v = it.into_value().map_err(|_| EditError::BadValue(path.into()))?;
                *v.decor_mut() = decor;
                *old = v;
            }
            _ => {
                table.insert(last, it);
            }
        }
    }
    // A file Settings started has no version yet: it gets the current one, first.
    if !doc.contains_key("schema") {
        doc.insert("schema", value(i64::from(super::SCHEMA_VERSION)));
        if let Some(mut key) = doc.key_mut("schema") {
            key.leaf_decor_mut().set_prefix("");
        }
    }
    let out = doc.to_string();
    parse(&out).map_err(EditError::Invalid)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a value changed with its comment and the other keys kept, a missing
    //! table created, a key removed back to its default, lists, rules as an array of
    //! tables, an out-of-range value and a wrong type refused, a broken file, bad paths.
    use super::*;
    use serde_json::json;

    const FILE: &str = "# My Kabuk\nschema = 1\n\n[windows]\n# tighter\ngap = 8 # px\nworkspaces = 6\n";

    #[test]
    fn a_value_changes_and_everything_else_stays() {
        let out = set(FILE, "windows.gap", &json!(14)).unwrap();
        assert_eq!(
            out,
            "# My Kabuk\nschema = 1\n\n[windows]\n# tighter\ngap = 14 # px\nworkspaces = 6\n"
        );
        assert_eq!(parse(&out).unwrap().config.windows.gap, Some(14));
    }

    /// Found on the user's machine: Settings had written a file with no `schema`, and
    /// Kabuk warned about it at every start. A file without one gets the current version
    /// at its top; one that has it keeps it where it is.
    #[test]
    fn a_file_without_a_schema_gets_one() {
        let out = set("", "wallpaper.kind", &json!("circuit")).unwrap();
        assert!(
            out.starts_with(&format!(
                "schema = {}
",
                super::super::SCHEMA_VERSION
            )),
            "{out}"
        );
        assert!(parse(&out).unwrap().warnings.is_empty(), "{out}");
        let theirs = "[appearance]
look = \"ada\"
";
        let out = set(theirs, "appearance.mode", &json!("light")).unwrap();
        assert!(
            out.starts_with(
                "schema = 1
"
            ) && out.contains("look = \"ada\""),
            "{out}"
        );
        let out = set(FILE, "windows.gap", &json!(14)).unwrap();
        assert_eq!(out.matches("schema").count(), 1);
        assert!(out.starts_with(
            "# My Kabuk
schema = 1
"
        ));
    }

    #[test]
    fn missing_tables_are_made_and_keys_removed() {
        let out = set("schema = 1\n", "bar.modules.cpu", &json!(false)).unwrap();
        assert!(!parse(&out).unwrap().config.bar.modules.cpu, "{out}");
        let back = set(&out, "bar.modules.cpu", &Value::Null).unwrap();
        assert!(parse(&back).unwrap().config.bar.modules.cpu);
        let out = set(FILE, "windows.gap", &Value::Null).unwrap();
        assert_eq!(parse(&out).unwrap().config.windows.gap, None);
        assert!(out.contains("workspaces = 6"));
    }

    #[test]
    fn lists_and_rules_are_written() {
        let out = set(
            "schema = 1\n",
            "shortcuts.launcher",
            &json!(["Ctrl+Shift+Space", "Alt+F1"]),
        )
        .unwrap();
        assert_eq!(
            parse(&out).unwrap().config.shortcuts.launcher,
            vec!["Ctrl+Shift+Space", "Alt+F1"]
        );
        let rules = json!([{ "exe": "Discord.exe", "workspace": 5, "monitor": 2 }, { "title": "Picture-in-picture", "float": true }]);
        let out = set("schema = 1\n", "rules.rule", &rules).unwrap();
        assert!(out.contains("[[rules.rule]]"), "{out}");
        let c = parse(&out).unwrap().config;
        assert_eq!(c.rules.user.len(), 2);
        assert_eq!((c.rules.user[0].workspace, c.rules.user[1].float), (Some(5), true));
    }

    #[test]
    fn invalid_changes_are_refused() {
        assert!(matches!(
            set(FILE, "windows.gap", &json!(99)),
            Err(EditError::Invalid(_))
        ));
        assert!(matches!(
            set(FILE, "windows.gap", &json!("wide")),
            Err(EditError::Invalid(_))
        ));
        assert!(matches!(
            set("[windows\n", "windows.gap", &json!(1)),
            Err(EditError::Syntax(_))
        ));
        assert!(matches!(set(FILE, "", &json!(1)), Err(EditError::BadPath(_))));
        assert!(matches!(
            set(FILE, "windows..gap", &json!(1)),
            Err(EditError::BadPath(_))
        ));
        // `windows.gap` is a number, not a table.
        assert!(matches!(
            set(FILE, "windows.gap.x", &json!(1)),
            Err(EditError::BadPath(_))
        ));
    }
}
