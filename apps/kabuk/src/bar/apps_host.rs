//! The apps module on the bar thread (DECISIONS D25): reads the Windows taskbar's pins,
//! follows the window manager's windows and the foreground, loads the programs' icons
//! and handles clicks (show the app's window, or start a pinned app).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kabuk_ui::geom::Scale;
use kabuk_win::window::Hwnd;

use super::dock::DockApp;
use super::view::AppIcon;
use super::Shared;
use crate::bar::apps::{self, App, Pin, Running};
use crate::wm;

/// Kabuk's own pins in the state folder (D45).
const PINS_FILE: &str = "pins.json";
/// The taskbar's own list of its pins, in its order (packaged apps too).
const TASKBAND: (&str, &str) = (
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband",
    "Favorites",
);
/// Where Windows keeps the taskbar's pinned shortcuts, under `%APPDATA%`.
const PIN_FOLDER: [&str; 5] = [
    "Microsoft",
    "Internet Explorer",
    "Quick Launch",
    "User Pinned",
    "TaskBar",
];
/// App icons are loaded at this many times their drawn size and scaled down when drawn,
/// so they stay smooth (the shell's own small sizes look coarse, reported by the user).
pub(super) const OVERSAMPLE: u32 = 2;
/// How the shell names a packaged app by its app id.
const APPS_FOLDER: &str = r"shell:AppsFolder\";
/// Integration tests click the app owning window `wparam`, as a click on its icon would;
/// `lparam` 1: as if that window were in front.
pub const MSG_TEST_APP_CLICK: u32 = kabuk_win::msgloop::msg::APP + 28;

/// What the module keeps between redraws.
#[derive(Default)]
pub(super) struct AppsState {
    pub(super) pins: Vec<Pin>,
    /// Every Windows pin Kabuk has taken in (or the user took off), so it is not added
    /// again (D45).
    seen: Vec<String>,
    /// Programs in the order they first had a window.
    order: Vec<String>,
    /// The apps as last drawn, for clicks.
    pub(super) list: Vec<App>,
    /// Program path and picture source of each process seen, so a redraw does not open
    /// them again.
    pub(super) exes: HashMap<u32, (String, String)>,
    /// Each managed window's app id, read once.
    ids: HashMap<u64, Option<String>>,
    /// The pictures' side in pixels (bar or dock size at the monitor's scale, oversampled).
    side: u32,
}

/// Kabuk's pins file: the pins in their order and the Windows pins already taken in.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PinsFile {
    pins: Vec<Pin>,
    #[serde(default)]
    seen: Vec<String>,
}

/// The pins file's text, in the old form (a plain list) too; the list's pins count as
/// seen.
fn read_pins_file(text: &str) -> Option<PinsFile> {
    serde_json::from_str::<PinsFile>(text).ok().or_else(|| {
        let pins = serde_json::from_str::<Vec<Pin>>(text).ok()?;
        let seen = pins.iter().map(|p| p.link.clone()).collect();
        Some(PinsFile { pins, seen })
    })
}

/// Windows' taskbar pins in the taskbar's order: the shortcuts its own list names and
/// the packaged apps it names, then any other shortcut in the pin folder.
fn windows_pins(folder: &Path) -> Vec<Pin> {
    let files = read_pins(folder);
    // Programs pinned by their own app id (a Chrome profile, a game) keep their shortcut
    // in a folder of its own beside the pin folder; they count only where the list names
    // them.
    let implicit: Vec<Pin> = folder
        .parent()
        .map(|p| p.join("ImplicitAppShortcuts"))
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|d| read_pins(&d.path()))
        .collect();
    let name_of = |p: &Pin| {
        Path::new(&p.link)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let names: Vec<String> = files.iter().chain(&implicit).map(name_of).collect();
    let listed = kabuk_win::registry::read_binary(TASKBAND.0, TASKBAND.1)
        .map(|b| apps::taskband_pins(&b, &names))
        .unwrap_or_default();
    let mut out: Vec<Pin> = Vec::new();
    for item in listed {
        let pin = match item {
            apps::TaskbandPin::Shortcut(name) => files.iter().chain(&implicit).find(|f| name_of(f) == name).cloned(),
            apps::TaskbandPin::App(id) => Some(Pin {
                link: format!("{APPS_FOLDER}{id}"),
                exe: None,
                app_id: Some(id),
            }),
        };
        if let Some(pin) = pin.filter(|p| !out.iter().any(|o| o.link.eq_ignore_ascii_case(&p.link))) {
            out.push(pin);
        }
    }
    for f in files {
        if !out.iter().any(|o| o.link.eq_ignore_ascii_case(&f.link)) {
            out.push(f);
        }
    }
    out
}

/// The pins with the app id each one's shortcut or packaged app gives (not saved, so read
/// again; COM must be initialised).
fn with_app_ids(pins: Vec<Pin>) -> Vec<Pin> {
    pins.into_iter()
        .map(|p| {
            let app_id = p.app_id.clone().or_else(|| match p.link.strip_prefix(APPS_FOLDER) {
                Some(id) => Some(id.to_string()),
                None => kabuk_win::shellitems::shortcut_app_id(Path::new(&p.link)),
            });
            Pin { app_id, ..p }
        })
        .collect()
}

/// The shortcuts pinned to the Windows taskbar, by name (COM must be initialised).
pub(super) fn read_pins(folder: &Path) -> Vec<Pin> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut links: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .collect();
    links.sort();
    links
        .into_iter()
        .map(|link| Pin {
            exe: kabuk_win::shellitems::shortcut_target(&link)
                .filter(|t| t.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
                .map(|t| t.display().to_string()),
            app_id: kabuk_win::shellitems::shortcut_app_id(&link),
            link: link.display().to_string(),
        })
        .collect()
}

impl Shared {
    /// Kabuk's own pins (D45), in the state folder.
    fn pins_file(&self) -> Option<PathBuf> {
        Some(wm::lock(&self.state).path().parent()?.join(PINS_FILE))
    }

    /// Kabuk's pins, or, until the user changes them, the Windows taskbar's; pins added
    /// on Windows' taskbar since are taken in once (found on the user's machine: Windows'
    /// packaged pins were missing).
    pub(super) fn load_pins(&mut self) {
        let theirs = kabuk_win::shellitems::known_folder(kabuk_win::shellitems::Folder::RoamingAppData)
            .map(|base| windows_pins(&PIN_FOLDER.iter().fold(base, |p, part| p.join(part))))
            .unwrap_or_default();
        let saved = self
            .pins_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| read_pins_file(&t));
        let theirs = with_app_ids(theirs);
        match saved {
            Some(file) => {
                let file = PinsFile {
                    pins: with_app_ids(file.pins),
                    ..file
                };
                let mut seen = file.seen;
                let before = file.pins.len();
                self.apps.pins = apps::merged(file.pins, &mut seen, &theirs);
                self.apps.seen = seen;
                tracing::debug!(pins = self.apps.pins.len(), "Kabuk's pins read");
                if self.apps.pins.len() != before {
                    tracing::info!(
                        added = self.apps.pins.len() - before,
                        "pins taken in from Windows' taskbar"
                    );
                    let pins = self.apps.pins.clone();
                    self.write_pins(&pins);
                }
            }
            None => {
                self.apps.seen = theirs.iter().map(|p| p.link.clone()).collect();
                self.apps.pins = theirs;
                tracing::debug!(pins = self.apps.pins.len(), "taskbar pins read");
            }
        }
    }

    fn write_pins(&mut self, pins: &[Pin]) {
        for p in pins {
            if !self.apps.seen.iter().any(|s| s.eq_ignore_ascii_case(&p.link)) {
                self.apps.seen.push(p.link.clone());
            }
        }
        let file = PinsFile {
            pins: pins.to_vec(),
            seen: self.apps.seen.clone(),
        };
        let written = self.pins_file().map(|f| {
            let text = serde_json::to_string_pretty(&file).unwrap_or_default();
            kabuk_core::fsutil::write_atomic(&f, text.as_bytes())
        });
        match written {
            Some(Ok(())) => tracing::info!(pins = pins.len(), "pins saved"),
            Some(Err(e)) => tracing::error!(error = %e, "pins not saved"),
            None => tracing::error!("pins not saved: no state folder"),
        }
    }

    /// Keeps `pins` as Kabuk's (the Windows taskbar's stay as they are) and redraws.
    pub(super) fn save_pins(&mut self, pins: Vec<Pin>) {
        let pins = with_app_ids(pins);
        self.write_pins(&pins);
        self.apps.pins = pins;
        self.refresh_apps();
        self.render_all(true);
    }

    /// App `from` dragged to place `to` on the bar: the new order is kept.
    pub(super) fn app_dragged(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let before = apps::slide_ids(&self.apps.list);
        let pins = apps::dragged(&self.apps.list, &self.apps.pins, &mut self.apps.order, from, to);
        tracing::info!(from, to, "app moved on the bar");
        if pins != self.apps.pins {
            self.write_pins(&pins);
            self.apps.pins = pins;
        }
        self.refresh_apps();
        // The others that changed places slide there from where they were.
        let after = apps::slide_ids(&self.apps.list);
        let moved = before[from].clone();
        self.slide_apps(&before, &after, &moved);
        self.render_all(true);
    }

    /// Recomputes the apps from the managed windows and the foreground; true if changed.
    pub(super) fn refresh_apps(&mut self) -> bool {
        if !self.config().bar.modules.taskbar {
            let had = !self.apps.list.is_empty();
            self.apps.list.clear();
            return had;
        }
        let view = self.wm.as_ref().map(wm::WmLink::view).unwrap_or_default();
        let z: HashMap<u64, usize> = kabuk_win::window::top_level_windows()
            .into_iter()
            .enumerate()
            .map(|(i, h)| (h.0, i))
            .collect();
        let mut windows: Vec<u64> = view.windows.iter().map(|w| w.hwnd).collect();
        windows.sort_by_key(|h| z.get(h).copied().unwrap_or(usize::MAX));
        let mut running = Vec::new();
        for hwnd in windows {
            let Some(pid) = Hwnd(hwnd).pid() else { continue };
            let (exe, icon) = match self.apps.exes.get(&pid) {
                Some(e) => e.clone(),
                None => {
                    let Ok(process) = kabuk_win::process::Process::open(pid) else {
                        continue;
                    };
                    let Some(exe) = process.image_path() else {
                        continue;
                    };
                    let icon = process
                        .app_user_model_id()
                        .map_or_else(|| exe.clone(), |id| format!("{APPS_FOLDER}{id}"));
                    self.apps.exes.insert(pid, (exe.clone(), icon.clone()));
                    (exe, icon)
                }
            };
            // A window's own id can differ from its program's (a web app's in Chrome's).
            let app_id = self
                .apps
                .ids
                .entry(hwnd)
                .or_insert_with(|| kabuk_win::shellitems::window_app_id(Hwnd(hwnd)))
                .clone();
            running.push(Running {
                hwnd,
                exe,
                icon,
                app_id,
            });
        }
        self.apps.ids.retain(|h, _| running.iter().any(|r| r.hwnd == *h));
        apps::remember_order(&mut self.apps.order, &running);
        let focused = kabuk_win::events::foreground().map(|h| h.0);
        let list = apps::apps(&self.apps.pins, &running, &self.apps.order, focused);
        let side = self.picture_side();
        let changed = list != self.apps.list || side != self.apps.side;
        self.apps.list = list;
        self.apps.side = side;
        if changed {
            self.load_app_images();
        }
        changed
    }

    /// The pictures' side: the dock's tiles in Rıhtım (DECISIONS D17), the bar's icons
    /// otherwise, at the primary monitor's scale.
    fn picture_side(&self) -> u32 {
        let scale = self.bars.iter().find(|b| b.primary).map_or(Scale(1.0), |b| b.scale);
        let dips = if super::dock::shown(self.look.style.look) {
            super::dock::STYLE.app_icon()
        } else {
            self.look.style.app_icon
        };
        u32::try_from(scale.px(dips)).unwrap_or(1).max(1) * OVERSAMPLE
    }

    /// The key of an app's picture at the current size.
    pub(super) fn picture_key(&self, icon: &str) -> u64 {
        apps::image_key(&format!("{icon}@{}", self.apps.side))
    }

    fn load_app_images(&mut self) {
        let side = self.apps.side;
        for app in &self.apps.list {
            let key = self.picture_key(&app.icon);
            if self.gpu.images.contains(key) {
                continue;
            }
            let px = kabuk_win::icon::shell_item_icon_pixels(&app.icon, side)
                .or_else(|| kabuk_win::icon::file_icon_pixels(Path::new(&app.icon), side));
            match px {
                Some(px) => self.gpu.images.set(key, side, px),
                None => tracing::debug!(icon = %app.icon, "app icon unavailable"),
            }
        }
    }

    /// The picture keys of the apps, which the image store keeps.
    pub(super) fn app_image_keys(&self) -> Vec<u64> {
        self.apps.list.iter().map(|a| self.picture_key(&a.icon)).collect()
    }

    pub(super) fn app_icons(&self) -> Vec<AppIcon> {
        let ids = apps::slide_ids(&self.apps.list);
        self.apps
            .list
            .iter()
            .enumerate()
            .map(|(i, a)| AppIcon {
                image: self.picture_key(&a.icon),
                running: !a.windows.is_empty(),
                focused: a.focused,
                grow: self.app_growth(i),
                shift: self.app_shift(i, &ids[i]),
            })
            .collect()
    }

    /// The apps as Rıhtım's dock shows them (DECISIONS D17).
    pub(super) fn dock_apps(&self) -> Vec<DockApp> {
        if !self.config().bar.modules.taskbar {
            return Vec::new();
        }
        self.apps
            .list
            .iter()
            .map(|a| DockApp {
                image: self.picture_key(&a.icon),
                running: !a.windows.is_empty(),
            })
            .collect()
    }

    /// A click on app `i`, as on Windows' taskbar: the app's window in front (`front`)
    /// is minimized; otherwise its front window is shown, or the pinned app started.
    pub(super) fn app_click(&mut self, i: usize, front: Option<u64>) {
        let Some(app) = self.apps.list.get(i).cloned() else {
            return;
        };
        if let Some(hwnd) = front.filter(|f| app.windows.contains(f)) {
            tracing::info!(hwnd, source = %app.source, "app minimized from the bar");
            match &self.wm {
                // So its genie is on screen before it goes.
                Some(link) => link.send(wm::Command::Minimize(hwnd)),
                None => {
                    Hwnd(hwnd).minimize_async();
                }
            }
            return;
        }
        match (app.windows.first(), &app.link) {
            (Some(&hwnd), _) => {
                tracing::info!(hwnd, source = %app.source, "app shown from the bar");
                match &self.wm {
                    Some(link) => link.send(wm::Command::Reveal(hwnd)),
                    None => {
                        Hwnd(hwnd).focus();
                    }
                }
            }
            (None, Some(link)) => {
                tracing::info!(link = %link, "pinned app started from the bar");
                let link = link.clone();
                let started = std::thread::Builder::new().name("app-start".into()).spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(&link) {
                        tracing::warn!(link = %link, error = %e, "pinned app not started");
                    }
                });
                if let Err(e) = started {
                    tracing::warn!(error = %e, "pinned app not started");
                }
            }
            (None, None) => {}
        }
    }

    /// Integration tests: a click on the app owning `hwnd`, with `hwnd` in front when
    /// `in_front` (a test cannot bring a window to the front itself: Windows only lets
    /// the foreground process do that).
    #[cfg(feature = "integration")]
    pub(super) fn test_app_click(&mut self, hwnd: u64, in_front: bool) {
        self.refresh_apps();
        if let Some(i) = self.apps.list.iter().position(|a| a.windows.contains(&hwnd)) {
            self.app_click(i, in_front.then_some(hwnd));
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: pins read in name order with their programs, a shortcut to a folder
    //! (not a program), a missing folder.
    use super::*;

    #[test]
    fn pins_are_read_with_their_programs() {
        kabuk_win::system::com_init();
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::var("SystemRoot")
            .map(|r| Path::new(&r).join("notepad.exe"))
            .unwrap();
        kabuk_win::shellitems::write_shortcut(&dir.path().join("b Notepad.lnk"), &exe).unwrap();
        kabuk_win::shellitems::write_shortcut(&dir.path().join("a Folder.lnk"), dir.path()).unwrap();
        std::fs::write(dir.path().join("desktop.ini"), "").unwrap();
        let pins = read_pins(dir.path());
        assert_eq!(pins.len(), 2);
        assert!(pins[0].link.ends_with("a Folder.lnk") && pins[0].exe.is_none());
        assert!(pins[1]
            .exe
            .as_deref()
            .is_some_and(|e| e.eq_ignore_ascii_case(&exe.display().to_string())));
        assert!(read_pins(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn the_pins_file_reads_in_both_forms() {
        let old = r#"[{"link":"C:\\L\\a.lnk","exe":null}]"#;
        let f = read_pins_file(old).unwrap();
        assert_eq!((f.pins.len(), f.seen.clone()), (1, vec![r"C:\L\a.lnk".to_string()]));
        let new = r#"{"pins":[],"seen":["x"]}"#;
        let f = read_pins_file(new).unwrap();
        assert!(f.pins.is_empty() && f.seen == ["x"]);
        assert!(read_pins_file("nonsense").is_none());
    }
}
