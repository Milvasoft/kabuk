//! Watching a directory for file changes (for live configuration reload). The watch
//! is a waitable object, so the message loop sleeps until something changes.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
};

use crate::handle::{OwnedHandle, Waitable};
use crate::sync::io_error;

pub struct DirWatch {
    handle: OwnedHandle,
}

impl DirWatch {
    /// Watches `dir` (not its subdirectories) for created, renamed, written or resized files.
    pub fn new(dir: &Path) -> std::io::Result<DirWatch> {
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;
        // SAFETY: valid path string; the handle is closed by OwnedHandle with the matching call.
        let h = unsafe { FindFirstChangeNotificationW(&HSTRING::from(dir.as_os_str()), false, filter) }
            .map_err(io_error)?;
        Ok(DirWatch {
            handle: OwnedHandle::new_change_notification(h),
        })
    }

    /// Arms the watch for the next change; call after each signal.
    pub fn rearm(&self) -> std::io::Result<()> {
        // SAFETY: the handle is a live change-notification handle.
        unsafe { FindNextChangeNotification(self.handle.raw()) }.map_err(io_error)
    }
}

impl Waitable for DirWatch {
    fn wait_handle(&self) -> &OwnedHandle {
        &self.handle
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no signal without a change, a write signals, rearming clears it, a
    //! missing directory fails.
    use super::*;
    use std::time::Duration;

    #[test]
    fn signals_on_write_and_rearms() {
        let dir = tempfile::tempdir().unwrap();
        let w = DirWatch::new(dir.path()).unwrap();
        assert!(!w.wait_handle().wait(Some(Duration::ZERO)));
        std::fs::write(dir.path().join("kabuk.toml"), "schema = 1\n").unwrap();
        assert!(w.wait_handle().wait(Some(Duration::from_secs(5))));
        w.rearm().unwrap();
        // Let late notifications for the same write arrive, then re-arm once more.
        while w.wait_handle().wait(Some(Duration::from_millis(100))) {
            w.rearm().unwrap();
        }
        assert!(!w.wait_handle().wait(Some(Duration::ZERO)));
        assert!(DirWatch::new(&dir.path().join("missing")).is_err());
    }
}
