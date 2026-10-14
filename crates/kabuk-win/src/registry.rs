//! Values under `HKEY_CURRENT_USER` that Kabuk records and changes (CONTRIBUTING.md rule 5: every
//! one is backed up in the state file first and written back on exit): text and DWORD
//! values read, written and deleted by key path.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_SET_VALUE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RRF_NOEXPAND,
    RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

use crate::sync::io_error;

/// Longest text value read back (a path).
const MAX_TEXT: usize = 1024;

fn err(r: WIN32_ERROR) -> std::io::Error {
    io_error(windows::core::Error::from(r.to_hresult()))
}

/// The text of value `name` under `HKCU\key` (`""` is the key's default value), as stored
/// (environment variables not expanded); `None` when it is not set.
pub fn read_text(key: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; MAX_TEXT];
    let mut size = u32::try_from(buf.len() * 2).unwrap_or_default();
    // SAFETY: valid strings and an out buffer of the size passed (in bytes).
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The binary value `name` under `HKCU\key`; `None` when it is not set.
pub fn read_binary(key: &str, name: &str) -> Option<Vec<u8>> {
    use windows::Win32::System::Registry::RRF_RT_REG_BINARY;
    let mut size = 0u32;
    // SAFETY: valid strings; the first call only asks for the size.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: an out buffer of the size passed.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    buf.truncate(size as usize);
    (r == ERROR_SUCCESS).then_some(buf)
}

/// The text value `name` under `HKCU\key` and whether it is `REG_EXPAND_SZ`; `None`
/// when it is not set.
pub fn read_text_kind(key: &str, name: &str) -> Option<(String, bool)> {
    let mut buf = vec![0u16; MAX_TEXT];
    let mut size = u32::try_from(buf.len() * 2).unwrap_or_default();
    let mut kind = REG_VALUE_TYPE::default();
    // SAFETY: valid strings, a type out pointer and an out buffer of the size passed.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
            Some(&mut kind),
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some((String::from_utf16_lossy(&buf[..len]), kind == REG_EXPAND_SZ))
}

/// Whether `HKCU\key` exists.
pub fn key_exists(key: &str) -> bool {
    use windows::Win32::System::Registry::RegOpenKeyExW;
    let mut h = HKEY::default();
    // SAFETY: valid strings; an opened key is closed at once.
    unsafe {
        let r = RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, KEY_READ, &mut h);
        if r == ERROR_SUCCESS {
            let _ = RegCloseKey(h);
            true
        } else {
            false
        }
    }
}

/// The DWORD value `name` under `HKCU\key`; `None` when it is not set.
pub fn read_dword(key: &str, name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default();
    // SAFETY: valid strings and an out buffer of the size passed.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    (r == ERROR_SUCCESS).then_some(value)
}

/// Removes `HKCU\key` with everything under it; a missing key is fine.
pub fn delete_tree(key: &str) -> std::io::Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::RegDeleteTreeW;
    // SAFETY: a valid string; HKCU is always open.
    let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
    if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(err(r))
    }
}

/// How a text value is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// `REG_SZ`.
    Plain,
    /// `REG_EXPAND_SZ` (paths with `%VARIABLES%`).
    Expandable,
}

/// `HKCU\key`, open for writing (created if missing); closed when dropped.
pub struct Key(HKEY);

impl Key {
    pub fn open(key: &str) -> std::io::Result<Key> {
        let mut h = HKEY::default();
        // SAFETY: valid strings; `h` receives the handle closed in Drop.
        let r = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_READ,
                None,
                &mut h,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            return Err(err(r));
        }
        Ok(Key(h))
    }

    pub fn set_text(&self, name: &str, value: &str, kind: Text) -> std::io::Result<()> {
        let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
        let bytes: Vec<u8> = wide.iter().flat_map(|u| u.to_le_bytes()).collect();
        let kind: REG_VALUE_TYPE = match kind {
            Text::Plain => REG_SZ,
            Text::Expandable => REG_EXPAND_SZ,
        };
        // SAFETY: an open key and a NUL-terminated UTF-16 value of the length given.
        let r = unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, kind, Some(&bytes)) };
        if r != ERROR_SUCCESS {
            return Err(err(r));
        }
        Ok(())
    }

    pub fn set_binary(&self, name: &str, value: &[u8]) -> std::io::Result<()> {
        use windows::Win32::System::Registry::REG_BINARY;
        // SAFETY: an open key and a byte buffer of its own length.
        let r = unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, REG_BINARY, Some(value)) };
        if r != ERROR_SUCCESS {
            return Err(err(r));
        }
        Ok(())
    }

    pub fn set_dword(&self, name: &str, value: u32) -> std::io::Result<()> {
        // SAFETY: an open key and a 4-byte value.
        let r = unsafe {
            RegSetValueExW(
                self.0,
                &HSTRING::from(name),
                None,
                REG_DWORD,
                Some(&value.to_le_bytes()),
            )
        };
        if r != ERROR_SUCCESS {
            return Err(err(r));
        }
        Ok(())
    }

    /// Removes value `name`; a missing one is fine.
    pub fn delete(&self, name: &str) {
        // SAFETY: an open key.
        let _ = unsafe { RegDeleteValueW(self.0, &HSTRING::from(name)) };
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle RegCreateKeyExW gave us, closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// Writes a recorded value back as it was: its data with its type, or removes it when it
/// was not set.
pub fn write_value(v: &kabuk_core::state::RegistryValue) -> std::io::Result<()> {
    use kabuk_core::state::RegistryData;
    let k = Key::open(&v.key)?;
    match &v.data {
        Some(RegistryData::Dword(d)) => k.set_dword(&v.name, *d),
        Some(RegistryData::Binary(b)) => k.set_binary(&v.name, b),
        Some(RegistryData::Text { value, expand }) => {
            k.set_text(&v.name, value, if *expand { Text::Expandable } else { Text::Plain })
        }
        None => {
            k.delete(&v.name);
            Ok(())
        }
    }
}

/// Posts `message` to `window` whenever a value under `HKCU\key`, or a key below it, is
/// written, made or removed. Waits on its own thread without waking up in between;
/// dropping the watch stops it.
pub struct ChangeWatch {
    stop: crate::handle::OwnedHandle,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ChangeWatch {
    pub fn start(key: &str, window: crate::window::Hwnd, message: u32) -> std::io::Result<ChangeWatch> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Registry::{RegOpenKeyExW, KEY_NOTIFY};
        use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
        let mut h = HKEY::default();
        // SAFETY: a valid string; the key is closed by the thread below, or here on failure.
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, KEY_NOTIFY, &mut h) };
        if r != ERROR_SUCCESS {
            return Err(err(r));
        }
        let close = |h: HKEY| {
            // SAFETY: the key opened above, closed once.
            let _ = unsafe { RegCloseKey(h) };
        };
        // SAFETY: unnamed events (manual-reset for stopping, auto-reset for changes), owned
        // by the wrapper and the thread.
        let events = unsafe {
            (
                CreateEventW(None, true, false, None),
                CreateEventW(None, false, false, None),
            )
        };
        let (stop, changed) = match events {
            (Ok(s), Ok(c)) => (crate::handle::OwnedHandle::new(s), crate::handle::OwnedHandle::new(c)),
            (s, c) => {
                for e in [s, c].into_iter().flatten() {
                    drop(crate::handle::OwnedHandle::new(e));
                }
                close(h);
                return Err(std::io::Error::other("no event for the registry watch"));
            }
        };
        // The first request before returning, so a write right after is told.
        if let Err(e) = ask(h, &changed) {
            close(h);
            return Err(e);
        }
        let key_raw = h.0 as usize;
        let stop_raw = stop.raw().0 as usize;
        let thread = std::thread::Builder::new()
            .name("registry-watch".into())
            .spawn(move || {
                let key = HKEY(key_raw as *mut core::ffi::c_void);
                let handles = [changed.raw(), HANDLE(stop_raw as *mut core::ffi::c_void)];
                loop {
                    // SAFETY: both handles stay open for the wait: `changed` lives in this
                    // thread, `stop` until Drop has joined it.
                    let r = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                    if r.0 != 0 {
                        break;
                    }
                    let _ = crate::msgloop::post_message(
                        window,
                        crate::msgloop::Msg {
                            id: message,
                            wparam: 0,
                            lparam: 0,
                        },
                    );
                    if let Err(e) = ask(key, &changed) {
                        tracing::error!(error = %e, "registry watch stopped");
                        break;
                    }
                }
                close(key);
            })?;
        Ok(ChangeWatch {
            stop,
            thread: Some(thread),
        })
    }
}

/// Asks for `changed` to be set at the next write under `key` (any value, any key below).
/// The request outlives the thread that made it.
fn ask(key: HKEY, changed: &crate::handle::OwnedHandle) -> std::io::Result<()> {
    use windows::Win32::System::Registry::{
        RegNotifyChangeKeyValue, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME, REG_NOTIFY_THREAD_AGNOSTIC,
    };
    // SAFETY: an open key and event, both kept open while the request stands.
    let r = unsafe {
        RegNotifyChangeKeyValue(
            key,
            true,
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC,
            Some(changed.raw()),
            true,
        )
    };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(err(r))
    }
}

impl Drop for ChangeWatch {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::SetEvent;
        // SAFETY: the event handle is valid until this wrapper is dropped.
        let _ = unsafe { SetEvent(self.stop.raw()) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: text (plain and expandable, with a variable left as written), a DWORD,
    //! a missing value and a deleted one, in a key of Kabuk's own made for the test; a
    //! watch tells of a value written below its key, and stops when dropped.
    use super::*;

    #[test]
    fn a_watch_tells_of_writes_below_its_key() {
        use crate::msgloop::{run_until, MessageWindow, Wake};
        use std::time::Duration;
        const CHANGED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 9;
        let path = format!(r"Software\KabukWatchTest-{}", std::process::id());
        let icon = format!(r"{path}\DefaultIcon");
        Key::open(&icon).unwrap();
        let window = MessageWindow::new("Kabuk.Test.RegistryWatch", "", |m| {
            if m.id == CHANGED {
                crate::msgloop::post_quit(7);
                return Some(0);
            }
            None
        })
        .unwrap();
        let watch = ChangeWatch::start(&path, window.hwnd(), CHANGED).unwrap();
        Key::open(&icon)
            .unwrap()
            .set_text("", "x.ico,0", Text::Expandable)
            .unwrap();
        assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(7));
        // Stopped: a later write tells nothing, and drop does not hang.
        drop(watch);
        Key::open(&icon)
            .unwrap()
            .set_text("", "y.ico,0", Text::Expandable)
            .unwrap();
        assert_eq!(run_until(&[], Some(Duration::from_millis(300))), Wake::Timeout);
        assert!(ChangeWatch::start(&format!(r"{path}\Missing"), window.hwnd(), CHANGED).is_err());
        delete_tree(&path).unwrap();
    }

    #[test]
    fn values_are_written_read_and_removed() {
        let path = format!(r"Software\KabukTest-{}", std::process::id());
        let key = Key::open(&path).unwrap();
        key.set_text("plain", "ı İ ş", Text::Plain).unwrap();
        key.set_text("path", r"%SystemRoot%\cursors\a.cur", Text::Expandable)
            .unwrap();
        key.set_dword("n", 7).unwrap();
        assert_eq!(read_text(&path, "plain").as_deref(), Some("ı İ ş"));
        assert_eq!(read_text(&path, "path").as_deref(), Some(r"%SystemRoot%\cursors\a.cur"));
        assert_eq!(read_dword(&path, "n"), Some(7));
        assert_eq!(read_text(&path, "missing"), None);
        key.delete("n");
        key.delete("n");
        assert_eq!(read_dword(&path, "n"), None);
        drop(key);
        // The test key goes, leaving nothing behind.
        use windows::Win32::System::Registry::RegDeleteTreeW;
        // SAFETY: deletes the key this test made.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path.as_str())) };
    }
}
