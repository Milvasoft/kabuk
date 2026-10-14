//! The Windows screen saver settings (`HKCU\Control Panel\Desktop`: which saver, whether
//! it is on, after how long) and OledSaver's theme (`HKCU\Software\OledSaver`), read for
//! the state file's backup, set while Kabuk runs and written back after (SPEC §2.7,
//! DECISIONS D12). Changes apply at once through `SystemParametersInfo`, like the
//! Screen Saver Settings do.

use std::path::Path;

use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETSCREENSAVEACTIVE, SPI_SETSCREENSAVETIMEOUT,
    SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::registry::{read_dword, read_text, Key, Text};
use crate::sync::io_error;

const DESKTOP: &str = r"Control Panel\Desktop";
const SAVER: &str = "SCRNSAVE.EXE";
const ACTIVE: &str = "ScreenSaveActive";
const TIMEOUT: &str = "ScreenSaveTimeOut";
/// Every value Kabuk writes under `DESKTOP`.
pub const VALUES: [&str; 3] = [SAVER, ACTIVE, TIMEOUT];
const OLED: &str = r"Software\OledSaver";
const THEME: &str = "Theme";

/// The settings as they are now: [`VALUES`] with their text (`None`: not set), and
/// OledSaver's theme.
pub fn current() -> (Vec<(String, Option<String>)>, Option<u32>) {
    let values = VALUES.iter().map(|n| (n.to_string(), read_text(DESKTOP, n))).collect();
    (values, read_dword(OLED, THEME))
}

fn spi(action: SYSTEM_PARAMETERS_INFO_ACTION, value: u32) -> std::io::Result<()> {
    let flags = SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0);
    // SAFETY: both actions take their value in `uiParam` and no buffer.
    match unsafe { SystemParametersInfoW(action, value, None, flags) } {
        Ok(()) => Ok(()),
        Err(e) if e.code().is_ok() => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

/// The 8.3 form of `path` where the volume has one (Windows has been picky about spaces
/// in `SCRNSAVE.EXE`), else `path` as it is.
fn short(path: &Path) -> String {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let long = HSTRING::from(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    // SAFETY: a valid path and a buffer of the length given.
    let n = unsafe { GetShortPathNameW(&long, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return path.display().to_string();
    }
    String::from_utf16_lossy(&buf[..n])
}

/// Uses `saver` as the screen saver after `timeout_secs` (no saver at all with `None`),
/// OledSaver drawing `theme` when given.
pub fn apply(saver: Option<&Path>, timeout_secs: u32, theme: Option<u32>) -> std::io::Result<()> {
    let key = Key::open(DESKTOP)?;
    match saver {
        Some(p) => key.set_text(SAVER, &short(p), Text::Plain)?,
        None => key.delete(SAVER),
    }
    if let Some(t) = theme {
        Key::open(OLED)?.set_dword(THEME, t)?;
    }
    spi(SPI_SETSCREENSAVETIMEOUT, timeout_secs)?;
    spi(SPI_SETSCREENSAVEACTIVE, u32::from(saver.is_some()))
}

/// Puts back what [`current`] read: every value, even when Windows refuses one (it
/// refuses the timeout and the switch while the machine saves power or is locked); the
/// first refusal is returned.
pub fn restore(values: &[(String, Option<String>)], theme: Option<u32>) -> std::io::Result<()> {
    let key = Key::open(DESKTOP)?;
    let get = |name: &str| values.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.clone());
    let mut first: Option<std::io::Error> = None;
    let mut note = |r: std::io::Result<()>| {
        if let Err(e) = r {
            first.get_or_insert(e);
        }
    };
    match get(SAVER) {
        Some(v) => note(key.set_text(SAVER, &v, Text::Plain)),
        None => key.delete(SAVER),
    }
    for (name, action) in [(TIMEOUT, SPI_SETSCREENSAVETIMEOUT), (ACTIVE, SPI_SETSCREENSAVEACTIVE)] {
        match get(name).and_then(|v| v.trim().parse::<u32>().ok()) {
            Some(n) => note(spi(action, n)),
            None => key.delete(name),
        }
    }
    match Key::open(OLED) {
        Ok(oled) => match theme {
            Some(t) => note(oled.set_dword(THEME, t)),
            None => oled.delete(THEME),
        },
        Err(e) => note(Err(e)),
    }
    first.map_or(Ok(()), Err)
}

/// Starts `saver` full screen now (the Screen Saver Settings' "Preview").
pub fn start(saver: &Path) -> std::io::Result<()> {
    std::process::Command::new(saver).arg("/s").spawn().map(|_| ())
}
