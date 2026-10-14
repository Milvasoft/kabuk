//! Created wallpapers on disk, in the state folder: `walls/<id>/wall.toml` and
//! `wall.hlsl`. An id is `gen-<seconds>`; anything else (a hand-edited
//! `wallpaper.generated`) is refused, so no path leaves the folder.

use std::path::{Path, PathBuf};

use crate::format::{check_source, Manifest};

/// The folder in the state folder.
pub const WALLS_DIR: &str = "walls";
const TOML_FILE: &str = "wall.toml";
const HLSL_FILE: &str = "wall.hlsl";
const PREFIX: &str = "gen-";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not a created wallpaper's id: {0}")]
    BadId(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Format(#[from] crate::format::FormatError),
}

/// A new id for a wallpaper created at `seconds` since 1970 (one later than any taken).
pub fn new_id(dir: &Path, seconds: u64) -> String {
    let mut s = seconds;
    while dir.join(format!("{PREFIX}{s}")).exists() {
        s += 1;
    }
    format!("{PREFIX}{s}")
}

fn valid(id: &str) -> bool {
    id.strip_prefix(PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn folder(dir: &Path, id: &str) -> Result<PathBuf, StoreError> {
    if valid(id) {
        Ok(dir.join(id))
    } else {
        Err(StoreError::BadId(id.to_string()))
    }
}

/// Writes a wallpaper's two files under `id`.
pub fn save(dir: &Path, id: &str, toml: &str, hlsl: &str) -> Result<PathBuf, StoreError> {
    let f = folder(dir, id)?;
    std::fs::create_dir_all(&f)?;
    kabuk_core::fsutil::write_atomic(&f.join(TOML_FILE), toml.as_bytes())?;
    kabuk_core::fsutil::write_atomic(&f.join(HLSL_FILE), hlsl.as_bytes())?;
    Ok(f)
}

/// Reads a wallpaper and checks it again (the files may have been edited by hand).
pub fn load(dir: &Path, id: &str) -> Result<(Manifest, String), StoreError> {
    let f = folder(dir, id)?;
    let manifest = Manifest::parse(&std::fs::read_to_string(f.join(TOML_FILE))?)?;
    let hlsl = std::fs::read_to_string(f.join(HLSL_FILE))?;
    check_source(&hlsl)?;
    Ok((manifest, hlsl))
}

/// Every readable wallpaper, the newest first.
pub fn list(dir: &Path) -> Vec<(String, Manifest)> {
    let mut out: Vec<(u64, String, Manifest)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            let n = id.strip_prefix(PREFIX)?.parse::<u64>().ok()?;
            let (m, _) = load(dir, &id).ok()?;
            Some((n, id, m))
        })
        .collect();
    out.sort_by_key(|w| std::cmp::Reverse(w.0));
    out.into_iter().map(|(_, id, m)| (id, m)).collect()
}

/// Deletes a wallpaper's folder.
pub fn remove(dir: &Path, id: &str) -> Result<(), StoreError> {
    let f = folder(dir, id)?;
    match std::fs::remove_dir_all(f) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: saved and loaded back; listed newest first, a broken one left out; ids
    //! that would leave the folder refused; a new id skips taken ones; removing twice.
    use super::*;
    use crate::prompt::{SAMPLE_HLSL, SAMPLE_TOML};

    #[test]
    fn a_wallpaper_is_saved_listed_loaded_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let a = new_id(d, 100);
        save(d, &a, SAMPLE_TOML, SAMPLE_HLSL).unwrap();
        let b = new_id(d, 100);
        assert_eq!((a.as_str(), b.as_str()), ("gen-100", "gen-101"));
        save(d, &b, &SAMPLE_TOML.replace("Slow ribbons", "İkinci"), SAMPLE_HLSL).unwrap();
        save(d, "gen-102", SAMPLE_TOML, "no function here").unwrap();
        let l = list(d);
        let names: Vec<(&str, &str)> = l.iter().map(|(id, m)| (id.as_str(), m.name.as_str())).collect();
        assert_eq!(names, vec![("gen-101", "İkinci"), ("gen-100", "Slow ribbons")]);
        let (m, hlsl) = load(d, "gen-100").unwrap();
        assert_eq!((m.name.as_str(), hlsl.as_str()), ("Slow ribbons", SAMPLE_HLSL));
        assert!(matches!(load(d, "gen-102"), Err(StoreError::Format(_))));
        remove(d, "gen-100").unwrap();
        remove(d, "gen-100").unwrap();
        assert_eq!(list(d).len(), 1);
    }

    #[test]
    fn ids_that_leave_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["..", "gen-", "gen-1/../../x", "gen-1\\..", "other", "gen-1a", ""] {
            assert!(matches!(load(dir.path(), bad), Err(StoreError::BadId(_))), "{bad}");
            assert!(
                matches!(save(dir.path(), bad, "", ""), Err(StoreError::BadId(_))),
                "{bad}"
            );
            assert!(matches!(remove(dir.path(), bad), Err(StoreError::BadId(_))), "{bad}");
        }
        assert!(list(&dir.path().join("missing")).is_empty());
    }
}
