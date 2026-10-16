//! File helpers.

use std::io::Write;
use std::path::Path;

/// Writes `bytes` to `path` so that readers see either the old or the new content,
/// never a partial file: write a sibling temporary file, flush it to disk, then
/// replace the target (`std::fs::rename` uses `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
/// on Windows).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, true)
}

/// As [`write_atomic`], without waiting for the disk (`FlushFileBuffers` takes tens of
/// milliseconds on a slow one): every program reads the new content at once and it
/// stays after this one ends in any way, but Windows stopping (a power cut) before it
/// wrote the file out can leave it filled with zeros.
pub fn write_atomic_cached(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace(path, bytes, false)
}

fn replace(path: &Path, bytes: &[u8], flush: bool) -> std::io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        if flush {
            file.sync_all()?;
        }
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    //! Edge cases: new file in a missing directory, replacing an existing file,
    //! no temporary file left behind, failure when the target is a directory; the same
    //! for a write that does not wait for the disk.
    use super::*;

    #[test]
    fn creates_parent_directories_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a").join("b").join("x.json");
        write_atomic(&file, b"one").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"one");
    }

    #[test]
    fn replaces_existing_content_without_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.json");
        write_atomic(&file, b"first, longer content").unwrap();
        write_atomic(&file, b"second").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn fails_cleanly_when_target_is_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("taken");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_cached_write_replaces_the_content_alike() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a").join("x.json");
        write_atomic_cached(&file, b"first, longer content").unwrap();
        write_atomic_cached(&file, b"second").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
        let taken = dir.path().join("taken");
        std::fs::create_dir(&taken).unwrap();
        assert!(write_atomic_cached(&taken, b"x").is_err());
    }
}
