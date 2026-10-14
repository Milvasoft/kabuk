//! Named mutexes (single instance) and events (emergency stop) shared between the
//! shell, the watchdog and `kabuk --restore`.

use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent};

use crate::handle::{OwnedHandle, Waitable};

/// Held while this process is the single instance; released on drop.
#[derive(Debug)]
pub struct InstanceLock {
    _handle: OwnedHandle,
}

/// Creates the named mutex. Returns `None` when another process already holds it.
pub fn acquire_instance(name: &str) -> std::io::Result<Option<InstanceLock>> {
    // SAFETY: plain call with a valid, NUL-terminated name; no security attributes.
    let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(name)) }.map_err(io_error)?;
    // SAFETY: reads this thread's last error, set by the CreateMutexW call above.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = OwnedHandle::new(handle);
    if existed {
        return Ok(None);
    }
    Ok(Some(InstanceLock { _handle: handle }))
}

/// A named manual-reset event.
#[derive(Debug)]
pub struct NamedEvent {
    handle: OwnedHandle,
}

impl NamedEvent {
    /// Creates the event, or opens it if it already exists (unsignalled when created).
    pub fn create_or_open(name: &str) -> std::io::Result<NamedEvent> {
        // SAFETY: plain call with a valid, NUL-terminated name; manual reset, initially clear.
        let handle = unsafe { CreateEventW(None, true, false, &HSTRING::from(name)) }.map_err(io_error)?;
        Ok(NamedEvent {
            handle: OwnedHandle::new(handle),
        })
    }

    pub fn set(&self) -> std::io::Result<()> {
        // SAFETY: the handle is a valid event handle owned by `self`.
        unsafe { SetEvent(self.handle.raw()) }.map_err(io_error)
    }

    /// Clears a manual-reset event after it has been handled.
    pub fn reset(&self) -> std::io::Result<()> {
        use windows::Win32::System::Threading::ResetEvent;
        // SAFETY: the handle is a valid event handle owned by `self`.
        unsafe { ResetEvent(self.handle.raw()) }.map_err(io_error)
    }

    pub fn is_set(&self) -> bool {
        self.handle.is_signalled()
    }
}

impl Waitable for NamedEvent {
    fn wait_handle(&self) -> &OwnedHandle {
        &self.handle
    }
}

pub(crate) fn io_error(e: windows::core::Error) -> std::io::Error {
    std::io::Error::other(e)
}

#[cfg(test)]
mod tests {
    //! Edge cases: second acquire fails while the first lock lives, succeeds after drop;
    //! an event opened twice shares state; a fresh event is clear.
    use super::*;

    fn unique(prefix: &str) -> String {
        format!(
            "Local\\kabuk-test-{prefix}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )
    }

    #[test]
    fn instance_lock_is_exclusive_until_dropped() {
        let name = unique("mutex");
        let first = acquire_instance(&name).unwrap();
        assert!(first.is_some());
        assert!(acquire_instance(&name).unwrap().is_none());
        drop(first);
        assert!(acquire_instance(&name).unwrap().is_some());
    }

    #[test]
    fn event_is_shared_by_name() {
        let name = unique("event");
        let a = NamedEvent::create_or_open(&name).unwrap();
        let b = NamedEvent::create_or_open(&name).unwrap();
        assert!(!a.is_set() && !b.is_set());
        b.set().unwrap();
        assert!(a.is_set());
        assert!(a.wait_handle().wait(Some(std::time::Duration::ZERO)));
    }
}
