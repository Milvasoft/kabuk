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

/// How many times the wallpaper or the monitors may have changed: Kabuk set or gave back
/// a picture, or the bar heard of a setting or display change ([`changed`]). Those who
/// keep what [`files`] said ask again once it moved (PLAN M19 P18).
static CHANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The wallpaper or the monitors may have changed (`WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`).
pub fn changed() {
    CHANGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// The count of [`changed`] so far.
pub fn changes() -> u64 {
    CHANGES.load(std::sync::atomic::Ordering::Relaxed)
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

/// Each monitor's rectangle (physical pixels) with the picture file Windows shows on it
/// now; `None` for a plain colour. COM must be initialised.
pub fn files() -> Vec<(Rect, Option<String>)> {
    let Some(dw) = wallpapers() else {
        return Vec::new();
    };
    monitor_ids(&dw)
        .into_iter()
        .map(|(id, rect)| {
            // SAFETY: COM call on a live object; the string is freed after copying.
            let path = unsafe {
                dw.GetWallpaper(&HSTRING::from(id.as_str())).ok().map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
            };
            (rect, path.filter(|p| !p.is_empty()))
        })
        .collect()
}

/// What Windows shows now, Kabuk's pictures included, its monitors in the order of their
/// ids (Windows' own order changes, e.g. while the screens sleep, and a backup compared
/// with the wallpaper now differed only in it). COM must be initialised.
pub fn current() -> Option<WallpaperBackup> {
    let dw = wallpapers()?;
    let mut monitors: Vec<(String, Option<String>)> = monitor_ids(&dw)
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
    monitors.sort_by(|a, b| a.0.cmp(&b.0));
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
    let set = unsafe {
        dw.SetPosition(DWPOS_FILL)
            .and_then(|()| dw.SetWallpaper(&HSTRING::from(id.as_str()), &HSTRING::from(picture.as_os_str())))
            .map_err(std::io::Error::other)
    };
    // Once Windows shows it (or a part of the change), not before.
    changed();
    set
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
    let given = unsafe {
        dw.SetBackgroundColor(windows::Win32::Foundation::COLORREF(b.color))
            .and_then(|()| dw.SetPosition(DESKTOP_WALLPAPER_POSITION(b.position)))
            .and_then(|()| {
                b.monitors
                    .iter()
                    .filter(|(id, _)| known.contains(id))
                    .try_for_each(|(id, path)| {
                        dw.SetWallpaper(
                            &HSTRING::from(id.as_str()),
                            &HSTRING::from(path.clone().unwrap_or_default()),
                        )
                    })
            })
            .map_err(std::io::Error::other)
    };
    changed();
    given
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

    /// Found by a test: Windows listed the monitors in another order after the screens
    /// slept, so the wallpaper given back did not equal its backup.
    #[test]
    fn the_monitors_come_in_the_order_of_their_ids() {
        crate::system::com_init_mta();
        let ids: Vec<String> = current()
            .expect("IDesktopWallpaper answers")
            .monitors
            .into_iter()
            .map(|m| m.0)
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    /// The app grid finds each monitor's picture by its rectangle, as the shell gives it.
    #[test]
    fn every_monitors_picture_is_found_by_its_rectangle() {
        crate::system::com_init_mta();
        let files = files();
        let monitors = crate::shell::monitors();
        assert_eq!(files.len(), monitors.len());
        for m in &monitors {
            assert!(
                files.iter().any(|(r, _)| (r.left, r.top, r.right, r.bottom)
                    == (m.rect.left, m.rect.top, m.rect.right, m.rect.bottom)),
                "{:?}",
                m.rect
            );
        }
    }
}
