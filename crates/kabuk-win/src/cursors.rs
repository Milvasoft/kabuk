//! The user's mouse cursors (`HKCU\Control Panel\Cursors`, DECISIONS D20): read them
//! for the state file's backup, set a cursor set and put the backup back, each applied
//! with `SPI_SETCURSORS` (the way the Mouse settings do it).

use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETCURSORS, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::registry::{read_dword, read_text, Key, Text};
use crate::sync::io_error;

const KEY: &str = r"Control Panel\Cursors";
/// The key's default value: the name of the scheme in use.
const SCHEME: &str = "";
/// Where the scheme comes from (0 Windows default, 1 the user's, 2 a system scheme).
const SCHEME_SOURCE: &str = "Scheme Source";
/// The size Windows loads cursors at, which its pointer size slider writes (D41).
const BASE_SIZE: &str = "CursorBaseSize";

/// The calls Windows' pointer size slider makes (`SPI_GETCURSORBASESIZE` and
/// `SPI_SETCURSORBASESIZE`; not in the SDK's headers): writing the value alone changes
/// nothing until the next sign-in.
const SPI_GETCURSORBASESIZE: u32 = 0x2028;
const SPI_SETCURSORBASESIZE: u32 = 0x2029;
/// Windows' pointer size when none is set.
const DEFAULT_BASE_SIZE: u32 = 32;

/// Windows' pointer size now; `None` when it is not set.
pub fn base_size() -> Option<u32> {
    read_dword(KEY, BASE_SIZE)
}

/// The pointer size Windows loads cursors at right now.
pub fn base_size_in_use() -> Option<u32> {
    let mut v = 0u32;
    // SAFETY: the get call writes one u32 to the pointer given.
    unsafe {
        SystemParametersInfoW(
            SYSTEM_PARAMETERS_INFO_ACTION(SPI_GETCURSORBASESIZE),
            0,
            Some((&raw mut v).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()
    .map(|()| v)
}

/// Windows loads cursors at `size` from now on (`keep`: also written to the profile).
fn set_base_size_now(size: u32, keep: bool) -> std::io::Result<()> {
    let flags = if keep {
        SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0
    } else {
        SPIF_SENDCHANGE.0
    };
    // SAFETY: the set call takes the size itself in the pointer argument.
    unsafe {
        SystemParametersInfoW(
            SYSTEM_PARAMETERS_INFO_ACTION(SPI_SETCURSORBASESIZE),
            0,
            Some(std::ptr::without_provenance_mut(size as usize)),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(flags),
        )
    }
    .map_err(io_error)
}

/// The cursor values as they are now: `names` (and the scheme's name, as `""`) with their
/// text, `None` for a value that is not set; and the scheme source.
pub fn current(names: &[&str]) -> (Vec<(String, Option<String>)>, Option<u32>) {
    let values = std::iter::once(SCHEME)
        .chain(names.iter().copied())
        .map(|n| (n.to_string(), read_text(KEY, n)))
        .collect();
    (values, read_dword(KEY, SCHEME_SOURCE))
}

/// Windows re-reads the key and every program gets the new cursors.
fn reload() -> std::io::Result<()> {
    let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0);
    // SAFETY: SPI_SETCURSORS takes no parameter.
    match unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, None, flags) } {
        Ok(()) => Ok(()),
        // It can answer FALSE without an error once the cursors are reloaded.
        Err(e) if e.code().is_ok() => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

/// Uses `files` (cursor value name, file path) as the scheme `scheme` (a user scheme),
/// loaded at pointer size `base_size`.
pub fn apply(scheme: &str, files: &[(&str, String)], base_size: u32) -> std::io::Result<()> {
    let key = Key::open(KEY)?;
    key.set_text(SCHEME, scheme, Text::Expandable)?;
    for (name, path) in files {
        key.set_text(name, path, Text::Expandable)?;
    }
    key.set_dword(SCHEME_SOURCE, 1)?;
    key.set_dword(BASE_SIZE, base_size)?;
    reload()?;
    set_base_size_now(base_size, true)
}

/// Puts back what [`current`] read: each value as it was, missing ones removed; and the
/// pointer size when `base_size` says what it was (`Some(None)`: it was not set).
pub fn restore(
    values: &[(String, Option<String>)],
    scheme_source: Option<u32>,
    base_size: Option<Option<u32>>,
) -> std::io::Result<()> {
    let key = Key::open(KEY)?;
    match base_size {
        Some(Some(v)) => {
            key.set_dword(BASE_SIZE, v)?;
            set_base_size_now(v, true)?;
        }
        Some(None) => {
            set_base_size_now(DEFAULT_BASE_SIZE, false)?;
            key.delete(BASE_SIZE);
        }
        None => {}
    }
    for (name, value) in values {
        match value {
            Some(v) => key.set_text(name, v, Text::Expandable)?,
            None => key.delete(name),
        }
    }
    match scheme_source {
        Some(v) => key.set_dword(SCHEME_SOURCE, v)?,
        None => key.delete(SCHEME_SOURCE),
    }
    reload()
}
