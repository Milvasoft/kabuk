//! The system theme's colour layer (SPEC §2.10, DECISIONS D42): Windows' app and system
//! mode follow Kabuk's mode and Windows' accent colour follows the look's, through the
//! per-user values Windows' own personalisation settings write; Windows is told with
//! `WM_SETTINGCHANGE` ("ImmersiveColorSet"). Every value is read for the state file
//! before it is written, and written back (or removed, when it was not set) by
//! [`restore`].
//!
//! The DWM's own accent and colorization words (the title bars' colour) are Windows'
//! business while the layer is on: Windows makes them from the accent, late, after each
//! change. They are recorded too, because that same late copy would otherwise replace a
//! title bar colour the user kept apart from the accent (found on the user's machine: a
//! red one became the accent's grey). A restore writes them back after the others and
//! keeps writing them while Windows' late copy can still land.

use kabuk_core::state::{RegistryData, RegistryValue};

use crate::registry::{read_binary, read_dword, Key};

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
const APPS_LIGHT: &str = "AppsUseLightTheme";
const SYSTEM_LIGHT: &str = "SystemUsesLightTheme";
const PALETTE: &str = "AccentPalette";
const ACCENT_MENU: &str = "AccentColorMenu";
const START_MENU: &str = "StartColorMenu";
const DWM_ACCENT: &str = "AccentColor";
const COLORIZATION: &str = "ColorizationColor";
const AFTERGLOW: &str = "ColorizationAfterglow";
/// How long Windows' late copy of the accent into the DWM's words can take, and how
/// often a restore looks in the meantime.
const DWM_SETTLE: std::time::Duration = std::time::Duration::from_millis(2000);
const DWM_LOOK: std::time::Duration = std::time::Duration::from_millis(100);
/// Windows' eighth palette colour when there is none to keep.
const PALETTE_LAST: [u8; 4] = [0x10, 0x7c, 0x10, 0x00];
/// How long Windows' programs may take to hear of the change.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(1000);

/// Every value the layer records: (key, name, binary?); the DWM's last.
const VALUES: [(&str, &str, bool); 8] = [
    (PERSONALIZE, APPS_LIGHT, false),
    (PERSONALIZE, SYSTEM_LIGHT, false),
    (ACCENT, PALETTE, true),
    (ACCENT, ACCENT_MENU, false),
    (ACCENT, START_MENU, false),
    (DWM, DWM_ACCENT, false),
    (DWM, COLORIZATION, false),
    (DWM, AFTERGLOW, false),
];

/// The values as they are now, for the state file.
pub fn current() -> Vec<RegistryValue> {
    VALUES
        .iter()
        .map(|(key, name, binary)| RegistryValue {
            key: key.to_string(),
            name: name.to_string(),
            data: if *binary {
                read_binary(key, name).map(RegistryData::Binary)
            } else {
                read_dword(key, name).map(RegistryData::Dword)
            },
        })
        .collect()
}

/// Tells every top-level window that the colours changed.
fn announce() {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let area = HSTRING::from(crate::system::IMMERSIVE_COLOR_SET);
    // SAFETY: a broadcast with a string that lives through the call; hung windows are
    // skipped after the timeout.
    let _ = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            u32::try_from(SETTLE.as_millis()).unwrap_or(u32::MAX),
            None,
        )
    };
}

/// An accent colour as Windows' values hold it (`kabuk_theme::windows_accent` makes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accent {
    /// The seven shades as RGBA bytes, lightest first (Windows' eighth is kept).
    pub shades: Vec<u8>,
    /// The accent and Start's shade, 0xAABBGGRR.
    pub accent: u32,
    pub start: u32,
}

/// Windows in light (`Some(true)`) or dark mode, or its mode left alone (`None`), with
/// `accent` as its accent colour.
pub fn apply(light: Option<bool>, accent: &Accent) -> std::io::Result<()> {
    if let Some(light) = light {
        let k = Key::open(PERSONALIZE)?;
        k.set_dword(APPS_LIGHT, u32::from(light))?;
        k.set_dword(SYSTEM_LIGHT, u32::from(light))?;
    }
    let last = read_binary(ACCENT, PALETTE)
        .filter(|b| b.len() == 32)
        .map_or(PALETTE_LAST, |b| [b[28], b[29], b[30], b[31]]);
    let mut palette = accent.shades.clone();
    palette.extend_from_slice(&last);
    let a = Key::open(ACCENT)?;
    a.set_binary(PALETTE, &palette)?;
    a.set_dword(ACCENT_MENU, accent.accent)?;
    a.set_dword(START_MENU, accent.start)?;
    let d = Key::open(DWM)?;
    d.set_dword(DWM_ACCENT, accent.accent)?;
    announce();
    Ok(())
}

use crate::registry::write_value as write;

fn read(v: &RegistryValue) -> Option<RegistryData> {
    match &v.data {
        Some(RegistryData::Binary(_)) => read_binary(&v.key, &v.name).map(RegistryData::Binary),
        _ => read_dword(&v.key, &v.name).map(RegistryData::Dword),
    }
}

/// Puts back what [`current`] read: every value, even when one fails (the first failure
/// is returned); the DWM's words after Windows was told, again while its late copy can
/// still land (this waits up to [`DWM_SETTLE`]).
pub fn restore(values: &[RegistryValue]) -> std::io::Result<()> {
    let mut first = None;
    let (dwm, others): (Vec<&RegistryValue>, Vec<&RegistryValue>) = values.iter().partition(|v| v.key == DWM);
    for v in &others {
        if let Err(e) = write(v) {
            first.get_or_insert(e);
        }
    }
    announce();
    let until = std::time::Instant::now() + DWM_SETTLE;
    loop {
        for v in &dwm {
            if read(v) != v.data {
                if let Err(e) = write(v) {
                    first.get_or_insert(e);
                }
            }
        }
        if std::time::Instant::now() >= until {
            break;
        }
        std::thread::sleep(DWM_LOOK);
    }
    first.map_or(Ok(()), Err)
}
