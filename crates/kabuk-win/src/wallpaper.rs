//! Windows' wallpaper through the documented `IDesktopWallpaper`: set per monitor while
//! Kabuk runs, the user's own recorded first and given back on exit (SPEC §2.8).

use kabuk_core::state::{Rect, WallpaperBackup};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_FILL};

fn wallpapers() -> Option<IDesktopWallpaper> {
    // SAFETY: plain COM creation; COM is initialised on the calling thread.
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.ok()
}

/// Windows' monitor ids with their rectangles. COM must be initialised on this thread.
fn monitor_ids(dw: &IDesktopWallpaper) -> Vec<(String, Rect)> {
    let mut out = Vec::new();
    // SAFETY: COM calls on a live object; each returned string is freed after copying.
    unsafe {
        let count = dw.GetMonitorDevicePathCount().unwrap_or(0);
        for i in 0..count {
            let Ok(id) = dw.GetMonitorDevicePathAt(i) else { continue };
            let r = dw.GetMonitorRECT(PCWSTR(id.0)).unwrap_or_default();
            out.push((
                id.to_string().unwrap_or_default(),
                Rect {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
            ));
            CoTaskMemFree(Some(id.0 as *const _));
        }
    }
    out
}

/// Where Windows keeps its recent wallpapers, newest first, under HKCU.
const HISTORY_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers";
const HISTORY_VALUE: &str = "BackgroundHistoryPath";
const HISTORY_LENGTH: usize = 5;

/// Windows' recent wallpapers, newest first.
fn history() -> Vec<String> {
    (0..HISTORY_LENGTH)
        .filter_map(|i| crate::registry::read_text(HISTORY_KEY, &format!("{HISTORY_VALUE}{i}")))
        .collect()
}

/// What Windows shows now, to give back later; a picture of Kabuk's still showing (from a
/// run that did not give the user's back) is recorded as the user's newest own picture
/// instead. COM must be initialised on this thread.
pub fn backup() -> Option<WallpaperBackup> {
    let history = history();
    let mut b = current()?;
    for (_, path) in &mut b.monitors {
        *path = kabuk_core::wallpaper::users_own(path.as_deref(), &history);
    }
    Some(b)
}

/// What Windows shows now, Kabuk's pictures included. COM must be initialised.
pub fn current() -> Option<WallpaperBackup> {
    let dw = wallpapers()?;
    let monitors = monitor_ids(&dw)
        .into_iter()
        .map(|(id, _)| {
            // SAFETY: COM call on a live object; the string is freed after copying.
            let path = unsafe {
                dw.GetWallpaper(&HSTRING::from(id.as_str())).ok().map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
            };
            (id, path.filter(|p| !p.is_empty()))
        })
        .collect();
    // SAFETY: COM calls on a live object.
    let (position, color) = unsafe {
        (
            dw.GetPosition().unwrap_or(DWPOS_FILL).0,
            dw.GetBackgroundColor().map_or(0, |c| c.0),
        )
    };
    Some(WallpaperBackup {
        monitors,
        position,
        color,
    })
}

/// Shows `picture` on the monitor at `monitor`, filling it. COM must be initialised.
pub fn set(monitor: Rect, picture: &std::path::Path) -> std::io::Result<()> {
    let dw = wallpapers().ok_or_else(|| std::io::Error::other("IDesktopWallpaper unavailable"))?;
    let (id, _) = monitor_ids(&dw)
        .into_iter()
        .find(|(_, r)| *r == monitor)
        .ok_or_else(|| std::io::Error::other("monitor not known to Windows' wallpaper"))?;
    // SAFETY: COM calls on a live object with valid strings.
    unsafe {
        dw.SetPosition(DWPOS_FILL).map_err(std::io::Error::other)?;
        dw.SetWallpaper(&HSTRING::from(id.as_str()), &HSTRING::from(picture.as_os_str()))
            .map_err(std::io::Error::other)
    }
}

/// True while Windows changes the wallpaper by itself (a slideshow): Kabuk then leaves
/// it alone, as setting a picture would end the user's slideshow.
pub fn slideshow_on() -> bool {
    use windows::Win32::UI::Shell::{DSS_ENABLED, DSS_SLIDESHOW};
    let Some(dw) = wallpapers() else { return false };
    // SAFETY: COM call on a live object.
    let state = unsafe { dw.GetStatus() }.map_or(0, |s| s.0);
    state & DSS_ENABLED.0 != 0 && state & DSS_SLIDESHOW.0 != 0
}

/// Gives back what [`backup`] read. Monitors no longer connected are skipped. Works on
/// any thread (it initialises COM for itself: `--restore` and the watchdog call it).
pub fn restore(b: &WallpaperBackup) -> std::io::Result<()> {
    crate::system::com_init();
    let dw = wallpapers().ok_or_else(|| std::io::Error::other("IDesktopWallpaper unavailable"))?;
    let known: Vec<String> = monitor_ids(&dw).into_iter().map(|(id, _)| id).collect();
    // SAFETY: COM calls on a live object with valid strings.
    unsafe {
        dw.SetBackgroundColor(windows::Win32::Foundation::COLORREF(b.color))
            .map_err(std::io::Error::other)?;
        dw.SetPosition(DESKTOP_WALLPAPER_POSITION(b.position))
            .map_err(std::io::Error::other)?;
        for (id, path) in b.monitors.iter().filter(|(id, _)| known.contains(id)) {
            dw.SetWallpaper(
                &HSTRING::from(id.as_str()),
                &HSTRING::from(path.clone().unwrap_or_default()),
            )
            .map_err(std::io::Error::other)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Edge case: every monitor's wallpaper is read for the backup.
    use super::*;

    #[test]
    fn the_wallpaper_is_backed_up_for_every_monitor() {
        crate::system::com_init_mta();
        let b = backup().expect("IDesktopWallpaper answers");
        assert_eq!(b.monitors.len(), crate::shell::monitors().len());
        assert!(b.monitors.iter().all(|(id, _)| !id.is_empty()));
    }
}
