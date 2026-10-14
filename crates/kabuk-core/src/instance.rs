//! Names of the kernel objects shared by one Kabuk instance. An instance is identified
//! by its state directory, so tests with their own state directory never collide with
//! the user's running shell.

use std::path::Path;
use std::time::Duration;

/// How long an emergency stop waits for the shell to exit on its own before
/// terminating it. Restore must finish within 2 s (SPEC §8.1).
pub const EMERGENCY_GRACE: Duration = Duration::from_millis(1000);

/// Exit codes shared by `kabuk.exe` and `kabuk-watchdog.exe`.
pub mod exit {
    pub const OK: i32 = 0;
    /// Bad command line.
    pub const USAGE: i32 = 2;
    /// Left after an emergency stop (hotkey or `--restore`); do not restart.
    pub const EMERGENCY: i32 = 10;
    /// Another instance with the same state directory is running; do not restore.
    pub const ALREADY_RUNNING: i32 = 11;
    /// The watchdog stopped restarting a crashing shell.
    pub const GAVE_UP: i32 = 12;
    /// Startup failed before anything was changed (e.g. the state file is unwritable).
    pub const STARTUP_FAILED: i32 = 13;
    /// A part of the running shell failed (the window manager panicked). Treated as a
    /// crash: the watchdog restores the desktop and restarts the shell.
    pub const INTERNAL_FAILURE: i32 = 14;
    /// `kabuk msg`: no shell of this instance is running.
    pub const NOT_RUNNING: i32 = 15;
    /// `kabuk msg`: the shell refused the command or its answer was unreadable.
    pub const COMMAND_FAILED: i32 = 16;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    id: String,
}

impl Instance {
    pub fn for_state_dir(state_dir: &Path) -> Instance {
        // Windows paths are case-insensitive; normalise separators and case first.
        let normalised = state_dir
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase();
        Instance {
            id: format!("{:016x}", fnv1a64(normalised.as_bytes())),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Single-instance mutex of the shell.
    pub fn shell_mutex(&self) -> String {
        format!("Local\\kabuk-shell-{}", self.id)
    }

    /// Single-instance mutex of the watchdog.
    pub fn watchdog_mutex(&self) -> String {
        format!("Local\\kabuk-watchdog-{}", self.id)
    }

    /// Manual-reset event set to request an emergency stop and restore.
    pub fn emergency_event(&self) -> String {
        format!("Local\\kabuk-emergency-{}", self.id)
    }
}

/// The session's shared block naming the state directory of the shell that has the desk
/// (asked for by the user: never two shells on one desk; a shell starting takes over
/// from the one there, whatever its state directory).
pub const DESK_OWNER: &str = "Local\\kabuk-desk-owner";
/// The block's size in bytes: the path in UTF-8, the rest zero.
pub const DESK_OWNER_SIZE: usize = 4096;

/// The desk owner block for `state_dir`; `None` for a path too long for it.
pub fn desk_owner_block(state_dir: &Path) -> Option<Vec<u8>> {
    let text = state_dir.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() >= DESK_OWNER_SIZE {
        return None;
    }
    let mut out = vec![0u8; DESK_OWNER_SIZE];
    out[..bytes.len()].copy_from_slice(bytes);
    Some(out)
}

/// The state directory a desk owner block names; `None` when it names none.
pub fn desk_owner(block: &[u8]) -> Option<std::path::PathBuf> {
    let end = block.iter().position(|b| *b == 0).unwrap_or(block.len());
    let text = std::str::from_utf8(&block[..end]).ok()?;
    (!text.is_empty()).then(|| std::path::PathBuf::from(text))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    //! Edge cases: same directory spelled with different case, separators and a
    //! trailing separator; different directories; name prefixes; the desk owner block
    //! round-trips a path with non-ASCII letters, and refuses an empty, too long or empty
    //! block.
    use super::*;

    #[test]
    fn the_desk_owner_block_names_a_state_directory() {
        let dir = Path::new(r"C:\Users\Buğra\AppData\Local\kabuk");
        let block = desk_owner_block(dir).unwrap();
        assert_eq!(block.len(), DESK_OWNER_SIZE);
        assert_eq!(desk_owner(&block).as_deref(), Some(dir));
        assert_eq!(desk_owner(&[0u8; DESK_OWNER_SIZE]), None);
        assert_eq!(desk_owner_block(Path::new("")), None);
        let long = "x".repeat(DESK_OWNER_SIZE);
        assert_eq!(desk_owner_block(Path::new(&long)), None);
        assert!(DESK_OWNER.starts_with("Local\\"));
    }

    #[test]
    fn same_directory_same_instance() {
        let a = Instance::for_state_dir(Path::new("C:\\Users\\X\\AppData\\Local\\kabuk"));
        let b = Instance::for_state_dir(Path::new("c:/users/x/appdata/local/KABUK/"));
        assert_eq!(a, b);
        assert_eq!(a.id().len(), 16);
    }

    #[test]
    fn different_directories_differ() {
        let a = Instance::for_state_dir(Path::new("C:\\a"));
        let b = Instance::for_state_dir(Path::new("C:\\b"));
        assert_ne!(a, b);
        assert_ne!(a.shell_mutex(), b.shell_mutex());
    }

    #[test]
    fn names_are_session_local_and_distinct() {
        let i = Instance::for_state_dir(Path::new("C:\\a"));
        let names = [i.shell_mutex(), i.watchdog_mutex(), i.emergency_event()];
        assert!(names
            .iter()
            .all(|n| n.starts_with("Local\\kabuk-") && n.ends_with(i.id())));
        assert_ne!(names[0], names[1]);
        assert_ne!(names[1], names[2]);
    }
}
