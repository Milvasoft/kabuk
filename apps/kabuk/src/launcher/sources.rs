//! Where the launcher's entries come from (SPEC §2.5): open windows, installed apps,
//! system commands, recent files, files in the indexed folders and browser bookmarks.
//! The slow sources run on a worker thread ([`gather`]); open windows are read when the
//! launcher opens ([`window_items`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kabuk_core::config::Launcher as LauncherConfig;
use kabuk_i18n::{Arg, I18n};
use kabuk_win::shellitems::{self, Folder};

use super::item::{Item, Kind, SystemCommand, Target};

/// At most this many files and folders are taken from the indexed folders (PLAN M6
/// measures a keystroke over 5,000 entries).
pub const MAX_FILES: usize = 5_000;
/// How deep the indexed folders are walked.
pub const MAX_DEPTH: usize = 6;
/// Recent items kept, newest first.
pub const MAX_RECENT: usize = 100;
/// Folders never walked: build output and package caches that hold thousands of files
/// nobody opens by name.
const SKIPPED: [&str; 4] = ["node_modules", "target", "bin", "obj"];

/// A managed window as the window manager reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowEntry {
    pub hwnd: u64,
    pub title: String,
    pub workspace: u32,
    /// Windows' display number ("Monitor n").
    pub monitor: u32,
    /// Where its program's picture comes from, as the bar's apps have it.
    pub picture: Option<String>,
}

/// Open windows, in the order given (the front window first).
pub fn window_items(windows: &[WindowEntry], i18n: &I18n) -> Vec<Item> {
    windows
        .iter()
        .filter(|w| !w.title.trim().is_empty())
        .map(|w| {
            let where_ = i18n.tr_args(
                "launcher-window-where",
                &[
                    ("workspace", Arg::Num(f64::from(w.workspace))),
                    ("monitor", Arg::Num(f64::from(w.monitor))),
                ],
            );
            let item = Item::new(w.title.clone(), where_, Kind::Window, Target::Window(w.hwnd));
            match &w.picture {
                Some(p) => item.with_picture(p.clone()),
                None => item,
            }
        })
        .collect()
}

/// The slow sources, in the order they are listed: apps, commands, recent files,
/// indexed files, bookmarks. Call on a thread with COM initialised.
pub fn gather(cfg: &LauncherConfig, i18n: &I18n) -> Vec<Item> {
    let s = &cfg.sources;
    let mut items = Vec::new();
    if s.apps {
        match shellitems::installed_apps() {
            Ok(apps) => {
                let label = i18n.tr("k-app");
                let mut seen = HashSet::new();
                items.extend(
                    apps.into_iter()
                        .filter(|a| seen.insert((a.name.clone(), a.id.clone())))
                        .map(|a| {
                            Item::new(a.name.clone(), label.clone(), Kind::App, Target::Shell(a.target()))
                                .with_picture(a.target())
                        }),
                );
            }
            Err(e) => tracing::warn!(error = %e, "installed apps unavailable"),
        }
    }
    if s.commands {
        items.extend(commands(i18n));
    }
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    if s.recent {
        if let Some(dir) = shellitems::known_folder(Folder::Recent) {
            for (path, is_dir) in recent(&dir) {
                if seen_paths.insert(path.clone()) {
                    items.push(path_item(&path, is_dir));
                }
            }
        }
    }
    if s.files {
        let env = |name: &str| std::env::var(name).ok();
        let mut budget = MAX_FILES;
        for folder in &cfg.indexed_folders {
            let root = PathBuf::from(kabuk_core::paths::expand_env(folder, env));
            for (path, is_dir) in walk(&root, MAX_DEPTH, budget) {
                if seen_paths.insert(path.clone()) {
                    items.push(path_item(&path, is_dir));
                    budget -= 1;
                }
            }
            if budget == 0 {
                break;
            }
        }
    }
    if s.bookmarks {
        if let Some(local) = shellitems::known_folder(Folder::LocalAppData) {
            let mut seen_urls = HashSet::new();
            for file in bookmark_files(&local) {
                let Ok(text) = std::fs::read_to_string(&file) else {
                    continue;
                };
                for (name, url) in bookmarks(&text) {
                    if seen_urls.insert(url.clone()) {
                        items.push(Item::new(name, short_url(&url), Kind::Bookmark, Target::Shell(url)));
                    }
                }
            }
        }
    }
    items
}

/// The system commands, with their titles in the user's language.
pub fn commands(i18n: &I18n) -> Vec<Item> {
    let label = i18n.tr("k-cmd");
    SystemCommand::ALL
        .into_iter()
        .map(|c| Item {
            icon: c.icon(),
            ..Item::new(i18n.tr(c.title_key()), label.clone(), Kind::Command, Target::Command(c))
        })
        .collect()
}

fn path_item(path: &Path, is_dir: bool) -> Item {
    let title = path
        .file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let subtitle = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let kind = if is_dir { Kind::Folder } else { Kind::File };
    let path = path.display().to_string();
    Item::new(title, subtitle, kind, Target::Shell(path.clone())).with_picture(path)
}

/// The targets of the shortcuts in Windows' recent items folder, newest first; shortcuts
/// to things that no longer exist are left out.
pub fn recent(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut links: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    links.sort_by_key(|l| std::cmp::Reverse(l.0));
    links
        .into_iter()
        .filter_map(|(_, lnk)| {
            let target = shellitems::shortcut_target(&lnk)?;
            let meta = std::fs::metadata(&target).ok()?;
            Some((target, meta.is_dir()))
        })
        .take(MAX_RECENT)
        .collect()
}

/// Files and folders under `root`, breadth first, at most `max` of them and `depth`
/// levels down; hidden ones (a leading dot or the hidden attribute) and [`SKIPPED`]
/// folders are left out.
pub fn walk(root: &Path, depth: usize, max: usize) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    let mut level = vec![root.to_path_buf()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                if out.len() >= max {
                    return out;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(meta) = e.metadata() else { continue };
                if name.starts_with('.') || is_hidden(&meta) {
                    continue;
                }
                let is_dir = meta.is_dir();
                if is_dir && SKIPPED.iter().any(|s| name.eq_ignore_ascii_case(s)) {
                    continue;
                }
                out.push((e.path(), is_dir));
                if is_dir {
                    next.push(e.path());
                }
            }
        }
        level = next;
    }
    out
}

fn is_hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    /// `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM`.
    const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
    meta.file_attributes() & HIDDEN_OR_SYSTEM != 0
}

/// The `Bookmarks` files of every Chrome and Edge profile under `%LOCALAPPDATA%`.
pub fn bookmark_files(local_app_data: &Path) -> Vec<PathBuf> {
    let browsers = [
        local_app_data.join("Google").join("Chrome").join("User Data"),
        local_app_data.join("Microsoft").join("Edge").join("User Data"),
    ];
    let mut out = Vec::new();
    for base in browsers {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut profiles: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == "Default" || n.starts_with("Profile "))
            })
            .collect();
        profiles.sort();
        out.extend(
            profiles
                .into_iter()
                .map(|p| p.join("Bookmarks"))
                .filter(|f| f.is_file()),
        );
    }
    out
}

/// Every bookmark (name, URL) in a Chromium `Bookmarks` file, folders flattened, in file
/// order; anything unreadable gives nothing.
pub fn bookmarks(json: &str) -> Vec<(String, String)> {
    fn visit(node: &serde_json::Value, out: &mut Vec<(String, String)>) {
        match node.get("type").and_then(|t| t.as_str()) {
            Some("url") => {
                if let (Some(name), Some(url)) = (
                    node.get("name").and_then(|n| n.as_str()),
                    node.get("url").and_then(|u| u.as_str()),
                ) {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        let name = if name.trim().is_empty() { url } else { name };
                        out.push((name.to_string(), url.to_string()));
                    }
                }
            }
            Some("folder") => {
                for child in node.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
                    visit(child, out);
                }
            }
            _ => {}
        }
    }
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(roots) = root.get("roots").and_then(|r| r.as_object()) {
        for key in ["bookmark_bar", "other", "synced"] {
            if let Some(node) = roots.get(key) {
                visit(node, &mut out);
            }
        }
    }
    out
}

/// A URL as the design page writes it under a bookmark: no scheme, no trailing slash.
pub fn short_url(url: &str) -> String {
    let s = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    s.strip_suffix('/').unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    //! Edge cases: bookmarks nested in folders, the three roots, non-web URLs and empty
    //! names, broken JSON; URLs shortened; the walk's depth and count limits, hidden and
    //! skipped folders, a missing root; recent shortcuts newest first with dead ones
    //! dropped; windows without a title left out; the commands in both languages.
    use super::*;
    use kabuk_i18n::Lang;

    const BOOKMARKS: &str = r#"{
      "roots": {
        "bookmark_bar": { "type": "folder", "children": [
          { "type": "url", "name": "Milvaion Dashboard", "url": "http://localhost:5000/jobs" },
          { "type": "folder", "name": "İş", "children": [
            { "type": "url", "name": "Opsiyon ERP yönetim paneli", "url": "https://opsiyonerp.com/" }
          ]},
          { "type": "url", "name": "settings", "url": "chrome://settings" }
        ]},
        "other": { "type": "folder", "children": [
          { "type": "url", "name": " ", "url": "https://example.com/a" }
        ]},
        "synced": { "type": "folder", "children": [] }
      }
    }"#;

    #[test]
    fn bookmarks_are_read_from_every_root_and_folder() {
        assert_eq!(
            bookmarks(BOOKMARKS),
            vec![
                (
                    "Milvaion Dashboard".to_string(),
                    "http://localhost:5000/jobs".to_string()
                ),
                (
                    "Opsiyon ERP yönetim paneli".to_string(),
                    "https://opsiyonerp.com/".to_string()
                ),
                ("https://example.com/a".to_string(), "https://example.com/a".to_string()),
            ]
        );
        assert!(bookmarks("{ not json").is_empty());
        assert!(bookmarks("{}").is_empty());
        assert_eq!(short_url("https://opsiyonerp.com/"), "opsiyonerp.com");
        assert_eq!(short_url("http://localhost:5000/jobs"), "localhost:5000/jobs");
        assert_eq!(short_url("ftp://x"), "ftp://x");
    }

    #[test]
    fn bookmark_files_are_found_in_every_profile() {
        let dir = tempfile::tempdir().unwrap();
        let chrome = dir.path().join("Google").join("Chrome").join("User Data");
        for p in ["Default", "Profile 2", "System Profile"] {
            std::fs::create_dir_all(chrome.join(p)).unwrap();
            std::fs::write(chrome.join(p).join("Bookmarks"), "{}").unwrap();
        }
        let edge = dir
            .path()
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
            .join("Default");
        std::fs::create_dir_all(&edge).unwrap();
        let files = bookmark_files(dir.path());
        // Edge's profile has no Bookmarks file; "System Profile" is not a user profile.
        assert_eq!(
            files,
            vec![
                chrome.join("Default").join("Bookmarks"),
                chrome.join("Profile 2").join("Bookmarks")
            ]
        );
    }

    #[test]
    fn the_walk_keeps_its_limits_and_skips_hidden_and_build_folders() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("a").join("b").join("c")).unwrap();
        std::fs::write(r.join("a").join("b").join("c").join("deep.txt"), "").unwrap();
        std::fs::write(r.join("top.txt"), "").unwrap();
        std::fs::create_dir_all(r.join(".git")).unwrap();
        std::fs::write(r.join(".git").join("HEAD"), "").unwrap();
        std::fs::create_dir_all(r.join("node_modules").join("x")).unwrap();
        let names = |v: Vec<(PathBuf, bool)>| -> Vec<String> {
            v.into_iter()
                .map(|(p, _)| p.strip_prefix(r).unwrap().display().to_string())
                .collect()
        };
        let all = names(walk(r, 10, 100));
        assert!(all.contains(&"top.txt".to_string()));
        assert!(all.iter().any(|n| n.ends_with("deep.txt")));
        assert!(all
            .iter()
            .all(|n| !n.starts_with(".git") && !n.starts_with("node_modules")));
        // Breadth first: the top level comes before anything deeper.
        assert_eq!(all[..2], ["a".to_string(), "top.txt".to_string()]);
        // Two levels: a, top.txt, a\b.
        assert_eq!(walk(r, 2, 100).len(), 3);
        assert_eq!(walk(r, 10, 2).len(), 2);
        assert!(walk(&r.join("missing"), 3, 10).is_empty());
        assert!(walk(r, 10, 100).iter().any(|(p, d)| *d && p.ends_with("a")));
    }

    #[test]
    fn recent_items_are_the_live_shortcut_targets_newest_first() {
        kabuk_win::system::com_init();
        let dir = tempfile::tempdir().unwrap();
        let files = dir.path().join("files");
        std::fs::create_dir_all(&files).unwrap();
        let recent_dir = dir.path().join("Recent");
        std::fs::create_dir_all(&recent_dir).unwrap();
        let make = |name: &str, target: &Path| {
            let lnk = recent_dir.join(format!("{name}.lnk"));
            kabuk_win::shellitems::write_shortcut(&lnk, target).unwrap();
            // Distinct modification times, oldest first.
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let old = files.join("old.txt");
        let new = files.join("new.txt");
        let gone = files.join("gone.txt");
        for f in [&old, &new, &gone] {
            std::fs::write(f, "").unwrap();
        }
        make("old", &old);
        make("folder", &files);
        make("gone", &gone);
        make("new", &new);
        std::fs::remove_file(&gone).unwrap();
        let got = recent(&recent_dir);
        assert_eq!(got, vec![(new, false), (files.clone(), true), (old, false)]);
        assert!(recent(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn windows_and_commands_become_entries() {
        let tr = I18n::new(Lang::Tr);
        let w = [
            WindowEntry {
                picture: None,
                hwnd: 7,
                title: "kabuk – Visual Studio Code".into(),
                workspace: 2,
                monitor: 1,
            },
            WindowEntry {
                picture: None,
                hwnd: 8,
                title: " ".into(),
                workspace: 1,
                monitor: 2,
            },
        ];
        let items = window_items(&w, &tr);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].subtitle, "Workspace 2, Monitör 1");
        assert_eq!(items[0].target, Target::Window(7));
        let c = commands(&tr);
        assert_eq!(c.len(), SystemCommand::ALL.len());
        assert_eq!(c[0].title, "Kabuk ayarları");
        assert_eq!(c[1].title, "Ekranı kilitle");
        assert!(commands(&I18n::new(Lang::En)).iter().any(|i| i.title == "Shut down"));
    }
}
