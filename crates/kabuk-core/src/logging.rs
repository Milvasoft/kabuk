//! Log files: `<state dir>\logs\<name>.<date>.log`, rotated daily, 7 kept.
//!
//! Writes are synchronous so the last lines before a crash are on disk. Level comes
//! from `KABUK_LOG` (e.g. `debug`), default `info`.

use std::path::Path;

use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;

pub const KEEP_DAYS: usize = 7;
pub const LEVEL_ENV: &str = "KABUK_LOG";

/// Installs the global logger. Returns false if the log directory cannot be used;
/// the program still runs, without file logs.
pub fn init(log_dir: &Path, name: &str) -> bool {
    // The appender prunes old files on start and complains on stderr if the directory is missing.
    if std::fs::create_dir_all(log_dir).is_err() {
        return false;
    }
    let appender = match Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(name)
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(log_dir)
    {
        Ok(a) => a,
        Err(_) => return false,
    };
    let filter = EnvFilter::try_from_env(LEVEL_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(appender)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init()
        .is_ok()
}

/// Writes every panic to the log before the default handling. A panic inside a window
/// procedure or WinEvent callback aborts the process (it cannot unwind into Windows), so
/// this line is the only trace of why the watchdog had to restore the desktop.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "panic");
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    //! Edge cases: the directory is created; a line reaches the file synchronously;
    //! an unusable directory (a file in its place) reports failure.
    use super::*;

    #[test]
    fn writes_synchronously_and_reports_bad_directories() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"file, not a directory").unwrap();
        assert!(!init(&blocked.join("logs"), "test"));

        let logs = dir.path().join("logs");
        assert!(init(&logs, "test"));
        tracing::info!("hello from the logging test");
        let file = std::fs::read_dir(&logs).unwrap().flatten().next().unwrap().path();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("test.") && name.ends_with(".log"), "{name}");
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("hello from the logging test"));

        log_panics();
        let _ = std::panic::catch_unwind(|| panic!("panic for the logging test"));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("ERROR") && text.contains("panic for the logging test"),
            "{text}"
        );
    }
}
