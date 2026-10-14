//! Owned kernel handles and waiting on them.

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};

/// A kernel handle closed on drop with the call that matches how it was opened.
#[derive(Debug)]
pub struct OwnedHandle(HANDLE, Close);

#[derive(Clone, Copy, Debug)]
enum Close {
    Handle,
    ChangeNotification,
}

// SAFETY: kernel handles are process-wide values; any thread may wait on or close them.
unsafe impl Send for OwnedHandle {}
// SAFETY: as above; the wrapper exposes no interior mutability.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    /// Takes ownership of `handle`, which must be a valid handle nobody else closes.
    pub(crate) fn new(handle: HANDLE) -> OwnedHandle {
        OwnedHandle(handle, Close::Handle)
    }

    /// A `FindFirstChangeNotification` handle (closed with `FindCloseChangeNotification`).
    pub(crate) fn new_change_notification(handle: HANDLE) -> OwnedHandle {
        OwnedHandle(handle, Close::ChangeNotification)
    }

    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }

    /// Waits until the object is signalled. `None` waits forever. Returns true when signalled.
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        // SAFETY: the handle is valid for the lifetime of `self`.
        let r = unsafe { WaitForSingleObject(self.0, timeout_ms(timeout)) };
        r == WAIT_OBJECT_0
    }

    /// True if the object is signalled right now.
    pub fn is_signalled(&self) -> bool {
        // SAFETY: the handle is valid for the lifetime of `self`; a zero timeout never blocks.
        let r = unsafe { WaitForSingleObject(self.0, 0) };
        r == WAIT_OBJECT_0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            match self.1 {
                Close::Handle => {
                    // SAFETY: we own the handle and close it exactly once.
                    let _ = unsafe { CloseHandle(self.0) };
                }
                Close::ChangeNotification => {
                    // SAFETY: as above, with the call matching FindFirstChangeNotification.
                    let _ = unsafe { windows::Win32::Storage::FileSystem::FindCloseChangeNotification(self.0) };
                }
            }
        }
    }
}

/// Something a message loop can wait on.
pub trait Waitable {
    fn wait_handle(&self) -> &OwnedHandle;
}

impl Waitable for OwnedHandle {
    fn wait_handle(&self) -> &OwnedHandle {
        self
    }
}

pub(crate) fn timeout_ms(timeout: Option<Duration>) -> u32 {
    match timeout {
        None => INFINITE,
        // INFINITE is u32::MAX; cap just below so a long finite wait stays finite.
        Some(d) => u32::try_from(d.as_millis()).unwrap_or(INFINITE - 1).min(INFINITE - 1),
    }
}
