//! The programs using the microphone, the camera or the location now (DECISIONS D88),
//! from Windows' capability access store (`HKCU\…\CapabilityAccessManager\ConsentStore`),
//! where Windows' own privacy indicator reads them: a record per program, "in use" while
//! its last use has started and not stopped. A watch tells a window when the store
//! changes (a registry change notification on a thread of its own; no polling).

use kabuk_core::privacy::{in_use, program_name, Kind, Use};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_QWORD,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE};

use crate::msgloop::{post_message, Msg};
use crate::window::Hwnd;

const STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
/// The subkey holding desktop programs' records.
const DESKTOP: &str = "NonPackaged";
/// A registry key name's longest length.
const NAME_MAX: usize = 256;

/// An open registry key, closed when dropped.
struct Key(HKEY);

impl Key {
    fn open(parent: HKEY, path: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<Key> {
        let mut h = HKEY::default();
        // SAFETY: valid strings and an out handle.
        let r = unsafe { RegOpenKeyExW(parent, &HSTRING::from(path), None, access, &mut h) };
        (r == ERROR_SUCCESS).then_some(Key(h))
    }

    /// The names of the subkeys.
    fn children(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; NAME_MAX];
            let mut len = u32::try_from(buf.len()).unwrap_or(0);
            // SAFETY: a buffer of the length passed; the index walks the subkeys.
            let r = unsafe {
                RegEnumKeyExW(
                    self.0,
                    i,
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    fn qword(&self, sub: &str, name: &str) -> u64 {
        let mut value = 0u64;
        let mut size = u32::try_from(std::mem::size_of::<u64>()).unwrap_or(0);
        // SAFETY: valid strings and an out buffer of the size passed.
        let r = unsafe {
            RegGetValueW(
                self.0,
                &HSTRING::from(sub),
                &HSTRING::from(name),
                RRF_RT_REG_QWORD,
                None,
                Some((&mut value as *mut u64).cast()),
                Some(&mut size),
            )
        };
        if r == ERROR_SUCCESS {
            value
        } else {
            0
        }
    }

    fn in_use(&self, sub: &str) -> bool {
        in_use(
            self.qword(sub, "LastUsedTimeStart"),
            self.qword(sub, "LastUsedTimeStop"),
        )
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key opened by `open`.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// What is in use now, sorted.
pub fn uses() -> Vec<Use> {
    let Some(store) = Key::open(HKEY_CURRENT_USER, STORE, KEY_READ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for kind in Kind::ALL {
        let Some(k) = Key::open(store.0, kind.store_key(), KEY_READ) else {
            continue;
        };
        for app in k.children() {
            if app == DESKTOP {
                if let Some(desktop) = Key::open(k.0, DESKTOP, KEY_READ) {
                    for program in desktop.children() {
                        if desktop.in_use(&program) {
                            out.push(Use {
                                kind,
                                program: program_name(&program),
                            });
                        }
                    }
                }
            } else if k.in_use(&app) {
                out.push(Use {
                    kind,
                    program: program_name(&app),
                });
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// A handle moved to the watch's thread.
struct SendHandle(HANDLE);
// SAFETY: event and registry handles may be used from any thread.
unsafe impl Send for SendHandle {}

/// Tells `window` with `message` whenever the store changes, until dropped.
pub struct Watch {
    stop: HANDLE,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watch {
    pub fn start(window: Hwnd, message: u32) -> std::io::Result<Watch> {
        let store = Key::open(HKEY_CURRENT_USER, STORE, KEY_NOTIFY)
            .ok_or_else(|| std::io::Error::other("capability access store missing"))?;
        // SAFETY: plain auto-reset events, closed by the watch.
        let (changed, stop) = unsafe {
            (
                CreateEventW(None, false, false, None).map_err(crate::sync::io_error)?,
                CreateEventW(None, false, false, None).map_err(crate::sync::io_error)?,
            )
        };
        let key = SendHandle(HANDLE(store.0 .0));
        // The key now belongs to the thread, which closes it.
        std::mem::forget(store);
        let (changed_t, stop_t) = (SendHandle(changed), SendHandle(stop));
        let thread = std::thread::Builder::new()
            .name("privacy-watch".into())
            .spawn(move || {
                // Whole, so the closure takes the Send wrappers, not their raw handles.
                let (key, changed, stop) = (key, changed_t, stop_t);
                let key = Key(HKEY(key.0 .0));
                loop {
                    // SAFETY: the key and event live until this thread ends; asynchronous,
                    // so the event is signalled on the next change under the store.
                    let r = unsafe {
                        RegNotifyChangeKeyValue(
                            key.0,
                            true,
                            REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                            Some(changed.0),
                            true,
                        )
                    };
                    if r != ERROR_SUCCESS {
                        tracing::warn!(code = r.0, "privacy store not watched");
                        break;
                    }
                    // SAFETY: two valid events.
                    let woke = unsafe { WaitForMultipleObjects(&[changed.0, stop.0], false, INFINITE) };
                    if woke != WAIT_OBJECT_0 {
                        break;
                    }
                    let _ = post_message(
                        window,
                        Msg {
                            id: message,
                            wparam: 0,
                            lparam: 0,
                        },
                    );
                }
                // SAFETY: the change event is this thread's to close.
                let _ = unsafe { CloseHandle(changed.0) };
            })?;
        Ok(Watch {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the stop event made in `start`; the thread ends on it.
        let _ = unsafe { SetEvent(self.stop) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: closed once, after the thread stopped waiting on it.
        let _ = unsafe { CloseHandle(self.stop) };
    }
}

#[cfg(test)]
mod tests {
    //! On the machine running the tests: the store reads (whatever is in use), and a
    //! watch starts and stops.
    use super::*;

    #[test]
    fn the_store_reads_and_a_watch_starts_and_stops() {
        for u in uses() {
            assert!(!u.program.is_empty(), "{u:?}");
        }
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Privacy", "", |_| None).unwrap();
        if let Ok(watch) = Watch::start(w.hwnd(), crate::msgloop::msg::APP + 903) {
            drop(watch);
        }
    }
}
