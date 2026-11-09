//! Saved desktops (DECISIONS D101): the windows on screen kept as a named set and brought
//! back later as they were. Unlike a layout profile, which only says where a program goes,
//! each window here keeps what it was showing: the arguments its program was started
//! with, its folder, a browser's address and profile, a File Explorer window's folder.
//! Pure: what a set holds, what opening it does, and the order programs start in.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::config::{SavedDesktop, SavedWindow};
use crate::state::Rect;

/// How long after a program started its window may still come; past it, it is reported.
pub const WAIT: Duration = Duration::from_secs(30);
/// How long a program is waited for before the next one starts anyway. Programs start one
/// at a time so their windows arrive in the saved order: a browser opens every window in
/// the one process it already has, so only the order tells them apart.
pub const PATIENCE: Duration = Duration::from_secs(6);

/// Kabuk's own programs: its settings window is never part of a set.
const OWN_PROGRAMS: [&str; 2] = ["kabuk.exe", "kabuk-settings.exe"];
const EXPLORER: &str = "explorer.exe";
/// Programs built on Chromium, which all read the same switches.
const CHROMIUM: [&str; 6] = [
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "vivaldi.exe",
    "opera.exe",
    "chromium.exe",
];
const FIREFOX: [&str; 2] = ["firefox.exe", "librewolf.exe"];
/// What marks a web app's window in a Chromium app id (`Chrome._crx_abc.UserData.Default`).
const WEB_APP_MARK: &str = "_crx_";

/// A window as a set sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub hwnd: u64,
    /// Its program's file name.
    pub exe: String,
    /// What opens the program again (its file, or a packaged app's `shell:AppsFolder\…`).
    pub open: String,
    /// The command line it was started with, program included.
    pub command_line: String,
    /// The folder it works in, when it could be read.
    pub dir: Option<String>,
    /// The window's own app id, which tells a browser's profile.
    pub app_id: Option<String>,
    /// What a browser's address bar shows, or the folder a File Explorer window is in.
    pub showing: Option<String>,
    /// Windows' number of its monitor, and its workspace (1-based; 0 the hidden one).
    pub monitor: u32,
    pub workspace: u32,
    pub floating: bool,
    /// Its place, relative to its monitor's work area.
    pub at: Option<Rect>,
    pub elevated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Chromium,
    Firefox,
    Explorer,
    Other,
}

fn is_one_of(exe: &str, names: &[&str]) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(exe))
}

fn kind(exe: &str) -> Kind {
    if is_one_of(exe, &CHROMIUM) {
        Kind::Chromium
    } else if is_one_of(exe, &FIREFOX) {
        Kind::Firefox
    } else if exe.eq_ignore_ascii_case(EXPLORER) {
        Kind::Explorer
    } else {
        Kind::Other
    }
}

/// A browser whose address bar is worth reading.
pub fn is_browser(exe: &str) -> bool {
    matches!(kind(exe), Kind::Chromium | Kind::Firefox)
}

pub fn is_explorer(exe: &str) -> bool {
    kind(exe) == Kind::Explorer
}

/// The command line's words, each without its quotes.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// `value` as one word of a command line.
fn quote(value: &str) -> String {
    if value.is_empty() || value.contains(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// What follows the program on a command line, as it was written.
fn after_program(line: &str) -> &str {
    let line = line.trim_start();
    let rest = match line.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').map_or("", |(_, rest)| rest),
        None => line.split_once(char::is_whitespace).map_or("", |(_, rest)| rest),
    };
    rest.trim()
}

/// The folder of Chromium's profile from a window's app id (`Chrome.UserData.Profile7`:
/// spaces are left out there); `None` when the id does not name one. Each window names its
/// own: the process's command line only tells which profile opened first.
fn profile_in_app_id(app_id: &str) -> Option<String> {
    let last = app_id.rsplit('.').next()?;
    if last == "Default" {
        return Some(last.to_string());
    }
    let number = last.strip_prefix("Profile")?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then(|| format!("Profile {number}"))
}

/// A web app's id in a Chromium app id.
fn web_app_in_app_id(app_id: &str) -> Option<&str> {
    let (_, after) = app_id.split_once(WEB_APP_MARK)?;
    after.split('.').next().filter(|id| !id.is_empty())
}

/// The switches a Chromium window comes back with. Its process's command line is the first
/// window's, so an address on it is stale and only what names the whole process is kept.
fn chromium_args(command_line: &str, app_id: Option<&str>) -> String {
    const USER_DATA: &str = "--user-data-dir=";
    const PROFILE: &str = "--profile-directory=";
    let given = words(after_program(command_line));
    let of = |prefix: &str| given.iter().find_map(|w| w.strip_prefix(prefix).map(str::to_string));
    let profile = app_id.and_then(profile_in_app_id).or_else(|| of(PROFILE));
    let mut out = Vec::new();
    if let Some(dir) = of(USER_DATA) {
        out.push(format!("{USER_DATA}{}", quote(&dir)));
    }
    if let Some(profile) = profile {
        out.push(format!("{PROFILE}{}", quote(&profile)));
    }
    if let Some(web_app) = app_id.and_then(web_app_in_app_id) {
        out.push(format!("--app-id={web_app}"));
    }
    out.join(" ")
}

/// Firefox's profile, the only thing of its command line that still holds.
fn firefox_args(command_line: &str) -> String {
    let given = words(after_program(command_line));
    let mut out = Vec::new();
    let mut it = given.iter();
    while let Some(w) = it.next() {
        if w.eq_ignore_ascii_case("-P") || w.eq_ignore_ascii_case("-profile") {
            if let Some(value) = it.next() {
                out.push(format!("{w} {}", quote(value)));
            }
        }
    }
    out.join(" ")
}

/// Whether `host` (without its port) is this computer or the local network: such a site
/// is served over plain HTTP, and `localhost:6179` alone would read as a scheme.
fn is_local(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.starts_with("[::1]")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host == "0.0.0.0"
}

/// The address to open for what a browser's address bar shows, which leaves the scheme
/// out; `None` when it shows nothing to come back to: a new tab, or words being typed. It
/// goes on a command line, so nothing that could read as a switch or end its quotes passes.
pub fn address_to_open(shown: &str) -> Option<String> {
    let shown = shown.trim();
    if shown.is_empty() || shown.starts_with('-') || shown.contains(|c: char| c.is_whitespace() || c == '"') {
        return None;
    }
    if shown.contains("://") {
        return Some(shown.to_string());
    }
    if shown.contains(':') && !shown.split(['/', '?', '#']).next().is_some_and(is_host_and_port) {
        // about:blank and the like.
        return None;
    }
    let authority = shown.split(['/', '?', '#']).next().unwrap_or(shown);
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    if is_local(host) {
        return Some(format!("http://{shown}"));
    }
    host.contains('.').then(|| shown.to_string())
}

fn is_host_and_port(authority: &str) -> bool {
    authority
        .rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

fn hex(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// The folder of a `file:` address as File Explorer gives it (`file:///C:/My%20Files`,
/// `file://server/share`); `None` for anything else (Home, This PC).
pub fn folder_of_file_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:")?.strip_prefix("//")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let escaped = (raw[i] == b'%')
            .then(|| Some(hex(*raw.get(i + 1)?)? * 16 + hex(*raw.get(i + 2)?)?))
            .flatten();
        match escaped {
            Some(b) => {
                bytes.push(b);
                i += 3;
            }
            None => {
                bytes.push(raw[i]);
                i += 1;
            }
        }
    }
    let path = String::from_utf8(bytes).ok()?.replace('/', "\\");
    match path.strip_prefix('\\') {
        // `file:///C:\…`: a drive.
        Some(drive) => Some(drive.to_string()),
        // `file://server\share`: a network folder.
        None => Some(format!("\\\\{path}")),
    }
}

/// What a set keeps of window `w`.
pub fn saved(w: &Seen) -> SavedWindow {
    let app_id = w.app_id.as_deref();
    let args = match kind(&w.exe) {
        Kind::Chromium => chromium_args(&w.command_line, app_id),
        Kind::Firefox => firefox_args(&w.command_line),
        // Explorer's command line is its first window's too.
        Kind::Explorer => String::new(),
        Kind::Other => after_program(&w.command_line).to_string(),
    };
    let address = match kind(&w.exe) {
        Kind::Chromium | Kind::Firefox => w.showing.as_deref().and_then(address_to_open),
        Kind::Explorer => w.showing.clone(),
        Kind::Other => None,
    };
    let at = w.at.unwrap_or_default();
    SavedWindow {
        exe: w.exe.clone(),
        open: w.open.clone(),
        args,
        dir: w.dir.clone().unwrap_or_default(),
        address: address.unwrap_or_default(),
        monitor: w.monitor,
        workspace: w.workspace,
        floating: w.floating,
        x: at.left,
        y: at.top,
        w: at.right - at.left,
        h: at.bottom - at.top,
        elevated: w.elevated,
    }
}

/// The set `name` of the windows open now, one entry for each window; the hidden
/// workspace's, Kabuk's own and those without a program left out.
pub fn capture(name: &str, windows: &[Seen]) -> SavedDesktop {
    SavedDesktop {
        name: name.trim().to_string(),
        windows: windows
            .iter()
            .filter(|w| w.workspace > 0 && !w.exe.is_empty() && !w.open.is_empty() && !is_one_of(&w.exe, &OWN_PROGRAMS))
            .map(saved)
            .collect(),
        ..SavedDesktop::default()
    }
}

/// What the program of `w` starts with: its saved arguments, then what it was showing.
pub fn launch_args(w: &SavedWindow) -> String {
    let showing = (!w.address.is_empty()).then(|| quote(&w.address));
    let parts: Vec<String> = match kind(&w.exe) {
        Kind::Chromium => vec![w.args.clone(), "--new-window".to_string(), showing.unwrap_or_default()],
        Kind::Firefox => match showing {
            Some(address) => vec![w.args.clone(), "-new-window".to_string(), address],
            None => vec![w.args.clone(), "-new-window".to_string()],
        },
        Kind::Explorer | Kind::Other => vec![w.args.clone(), showing.unwrap_or_default()],
    };
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The folder a program starts in: the one it worked in, else its own.
pub fn start_folder(w: &SavedWindow) -> Option<String> {
    if !w.dir.is_empty() {
        return Some(w.dir.clone());
    }
    if w.open.starts_with("shell:") {
        return None;
    }
    std::path::Path::new(&w.open)
        .parent()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty())
}

/// The place of `w` in work area `area`, moved inside it if the monitor has changed.
pub fn place_in(w: &SavedWindow, area: Rect) -> Option<Rect> {
    if w.w <= 0 || w.h <= 0 {
        return None;
    }
    let (width, height) = (w.w.min(area.right - area.left), w.h.min(area.bottom - area.top));
    let left = (area.left + w.x).clamp(area.left, area.right - width);
    let top = (area.top + w.y).clamp(area.top, area.bottom - height);
    Some(Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    })
}

/// What opening a set does for one of its windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The very window is open already (same program, arguments and address): it goes there.
    Move { hwnd: u64, window: SavedWindow },
    /// It is not: its program starts, and its window goes there when it comes.
    Launch(SavedWindow),
}

fn same_window(a: &SavedWindow, b: &SavedWindow) -> bool {
    a.exe.eq_ignore_ascii_case(&b.exe) && a.args == b.args && a.address.eq_ignore_ascii_case(&b.address)
}

/// The steps that open `set` with `open` windows on screen: a window already showing the
/// same thing is reused, each once, unless the set always starts its programs again.
pub fn plan(set: &SavedDesktop, open: &[Seen]) -> Vec<Step> {
    let mut taken: Vec<u64> = Vec::new();
    set.windows
        .iter()
        .map(|window| {
            let found = (!set.start_again)
                .then(|| {
                    open.iter()
                        .find(|o| !taken.contains(&o.hwnd) && same_window(&saved(o), window))
                })
                .flatten();
            match found {
                Some(o) => {
                    taken.push(o.hwnd);
                    Step::Move {
                        hwnd: o.hwnd,
                        window: window.clone(),
                    }
                }
                None => Step::Launch(window.clone()),
            }
        })
        .collect()
}

/// The programs of the sets being opened: those still to start, one at a time, and those
/// started whose windows have not come yet.
#[derive(Debug, Default)]
pub struct Opening {
    queue: VecDeque<SavedWindow>,
    started: Vec<(SavedWindow, Instant)>,
}

impl Opening {
    pub fn add(&mut self, windows: impl IntoIterator<Item = SavedWindow>) {
        self.queue.extend(windows);
    }

    /// The next program to start now, if the last has had its time.
    pub fn next(&mut self, now: Instant) -> Option<SavedWindow> {
        if self.started.iter().any(|(_, at)| now.duration_since(*at) < PATIENCE) {
            return None;
        }
        let window = self.queue.pop_front()?;
        self.started.push((window.clone(), now));
        Some(window)
    }

    /// A window of `exe` came: the saved window it is, the oldest started. A window that
    /// floats by itself (a splash screen) is not taken for one that was tiled.
    pub fn came(&mut self, exe: &str, floats_by_itself: bool) -> Option<SavedWindow> {
        let i = self
            .started
            .iter()
            .position(|(w, _)| w.exe.eq_ignore_ascii_case(exe) && (w.floating || !floats_by_itself))?;
        Some(self.started.remove(i).0)
    }

    /// The program of `exe` could not be started.
    pub fn failed(&mut self, exe: &str) -> bool {
        match self.started.iter().position(|(w, _)| w.exe.eq_ignore_ascii_case(exe)) {
            Some(i) => {
                self.started.remove(i);
                true
            }
            None => false,
        }
    }

    /// Forgets the programs whose windows never came, and says how many.
    pub fn expired(&mut self, now: Instant) -> u32 {
        let before = self.started.len();
        self.started.retain(|(_, at)| now.duration_since(*at) < WAIT);
        u32::try_from(before - self.started.len()).unwrap_or(u32::MAX)
    }

    /// When something next needs doing, from `now`; `None` once it is all over.
    pub fn wake_in(&self, now: Instant) -> Option<Duration> {
        let waiting = !self.queue.is_empty();
        self.started
            .iter()
            .flat_map(|(_, at)| {
                let gone = (*at + WAIT).saturating_duration_since(now);
                let next = (*at + PATIENCE).saturating_duration_since(now);
                [Some(gone), (waiting && !next.is_zero()).then_some(next)]
            })
            .flatten()
            .min()
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a program's arguments kept as written; a browser's stale first address
    //! dropped and its window's own profile taken over the process's; a web app; addresses
    //! without a scheme, new tabs, typed words and what could read as a switch; File
    //! Explorer's folders with escapes and on the network; the hidden workspace and Kabuk's
    //! own windows left out; two windows of one program both kept; a place moved inside a
    //! smaller monitor; reuse only of the very same window, each once; programs starting
    //! one at a time, the slow one not holding the rest, a splash not taken for a window.
    use super::*;

    const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

    fn seen(hwnd: u64, exe: &str, line: &str) -> Seen {
        Seen {
            hwnd,
            exe: exe.into(),
            open: format!(r"C:\Apps\{exe}"),
            command_line: line.into(),
            monitor: 1,
            workspace: 1,
            ..Seen::default()
        }
    }

    #[test]
    fn a_programs_arguments_are_kept_as_written() {
        let w = saved(&seen(1, "node.exe", r#""C:\n\node.exe" server.js --port "61 79""#));
        assert_eq!(w.args, r#"server.js --port "61 79""#);
        assert_eq!(launch_args(&w), r#"server.js --port "61 79""#);
        let w = saved(&seen(1, "app.exe", "app.exe -x"));
        assert_eq!(w.args, "-x");
        assert_eq!(saved(&seen(1, "app.exe", "app.exe")).args, "");
    }

    /// The user's case: Chrome at `localhost:6179` comes back there.
    #[test]
    fn a_browser_comes_back_at_its_address() {
        let w = saved(&Seen {
            showing: Some("localhost:6179".into()),
            ..seen(1, "chrome.exe", &format!("\"{CHROME}\""))
        });
        assert_eq!(w.address, "http://localhost:6179");
        assert_eq!(w.args, "");
        assert_eq!(launch_args(&w), "--new-window http://localhost:6179");
    }

    #[test]
    fn a_browsers_first_address_is_not_kept_and_each_window_has_its_profile() {
        let line = format!("\"{CHROME}\" --user-data-dir=\"C:\\My Data\" --profile-directory=\"Profile 7\" --no-first-run --new-window http://old/");
        let first = saved(&Seen {
            showing: Some("example.com/a".into()),
            app_id: Some("Chrome.MyData.Profile7".into()),
            ..seen(1, "chrome.exe", &line)
        });
        assert_eq!(
            first.args,
            "--user-data-dir=\"C:\\My Data\" --profile-directory=\"Profile 7\""
        );
        assert_eq!(first.address, "example.com/a");
        // A second window of that process, in another profile.
        let second = saved(&Seen {
            app_id: Some("Chrome.MyData.Default".into()),
            ..seen(2, "chrome.exe", &line)
        });
        assert_eq!(
            second.args,
            "--user-data-dir=\"C:\\My Data\" --profile-directory=Default"
        );
        assert_eq!(second.address, "");
        assert_eq!(
            launch_args(&second),
            "--user-data-dir=\"C:\\My Data\" --profile-directory=Default --new-window"
        );
        // An app id that does not name one: the process's.
        let plain = saved(&Seen {
            app_id: Some("Chrome".into()),
            ..seen(3, "chrome.exe", &line)
        });
        assert!(
            plain.args.ends_with("--profile-directory=\"Profile 7\""),
            "{}",
            plain.args
        );
    }

    #[test]
    fn a_web_app_comes_back_as_itself() {
        let w = saved(&Seen {
            app_id: Some("Chrome._crx_abcdef.UserData.Profile2".into()),
            ..seen(1, "chrome.exe", "chrome.exe")
        });
        assert_eq!(w.args, "--profile-directory=\"Profile 2\" --app-id=abcdef");
    }

    #[test]
    fn firefox_keeps_its_profile_and_opens_a_new_window() {
        let w = saved(&Seen {
            showing: Some("https://example.com/".into()),
            ..seen(1, "firefox.exe", r#"firefox.exe -P "İş" https://old/"#)
        });
        assert_eq!(w.args, "-P İş");
        assert_eq!(launch_args(&w), "-P İş -new-window https://example.com/");
    }

    #[test]
    fn what_an_address_bar_shows_becomes_an_address_or_nothing() {
        for (shown, want) in [
            ("localhost:6179", Some("http://localhost:6179")),
            ("localhost:6179/a?b=1#c", Some("http://localhost:6179/a?b=1#c")),
            ("127.0.0.1:8080", Some("http://127.0.0.1:8080")),
            ("192.168.1.20/x", Some("http://192.168.1.20/x")),
            ("https://a.b/c", Some("https://a.b/c")),
            ("chrome://settings", Some("chrome://settings")),
            ("github.com/x", Some("github.com/x")),
            ("", None),
            ("  ", None),
            ("about:blank", None),
            ("iki kelime", None),
            ("kelime", None),
            ("--disable-web-security", None),
            ("a.b\"c", None),
        ] {
            assert_eq!(address_to_open(shown).as_deref(), want, "{shown:?}");
        }
    }

    #[test]
    fn explorer_comes_back_at_its_folder() {
        assert_eq!(
            folder_of_file_url("file:///C:/My%20Files/%C4%B0%C5%9F").as_deref(),
            Some(r"C:\My Files\İş")
        );
        assert_eq!(
            folder_of_file_url("file://server/share").as_deref(),
            Some(r"\\server\share")
        );
        assert_eq!(folder_of_file_url("file:///C:/50%").as_deref(), Some(r"C:\50%"));
        assert_eq!(folder_of_file_url("shell:x"), None);
        assert_eq!(folder_of_file_url(""), None);
        let w = saved(&Seen {
            showing: Some(r"C:\My Files".into()),
            ..seen(1, "explorer.exe", "explorer.exe /factory")
        });
        assert_eq!(launch_args(&w), r#""C:\My Files""#);
    }

    #[test]
    fn a_set_is_every_window_but_the_hidden_and_kabuks() {
        let windows = [
            seen(1, "Code.exe", "Code.exe"),
            seen(2, "Code.exe", "Code.exe"),
            Seen {
                workspace: 0,
                ..seen(3, "Notes.exe", "Notes.exe")
            },
            seen(4, "kabuk-settings.exe", "kabuk-settings.exe"),
            Seen {
                open: String::new(),
                ..seen(5, "x.exe", "x.exe")
            },
        ];
        let set = capture("  Kodlama ", &windows);
        assert_eq!(set.name, "Kodlama");
        assert_eq!(set.windows.len(), 2, "both windows of the one program");
    }

    #[test]
    fn a_program_starts_in_the_folder_it_worked_in_else_its_own() {
        let mut w = saved(&seen(1, "app.exe", "app.exe"));
        assert_eq!(start_folder(&w).as_deref(), Some(r"C:\Apps"));
        w.dir = r"D:\Is".into();
        assert_eq!(start_folder(&w).as_deref(), Some(r"D:\Is"));
        let packaged = SavedWindow {
            open: r"shell:AppsFolder\a!b".into(),
            ..SavedWindow::default()
        };
        assert_eq!(start_folder(&packaged), None);
    }

    fn area(right: i32, bottom: i32) -> Rect {
        Rect {
            left: 0,
            top: 40,
            right,
            bottom,
        }
    }

    #[test]
    fn a_place_stays_inside_the_monitor() {
        let w = SavedWindow {
            x: 1000,
            y: 500,
            w: 800,
            h: 600,
            ..SavedWindow::default()
        };
        let big = place_in(&w, area(2560, 1440)).unwrap();
        assert_eq!((big.left, big.top, big.right, big.bottom), (1000, 540, 1800, 1140));
        let small = place_in(&w, area(1280, 720)).unwrap();
        assert_eq!((small.right, small.bottom), (1280, 720), "pulled back");
        assert_eq!((small.left, small.top), (480, 120));
        let huge = SavedWindow {
            w: 5000,
            h: 5000,
            ..w.clone()
        };
        let fit = place_in(&huge, area(1280, 720)).unwrap();
        assert_eq!((fit.left, fit.top, fit.right, fit.bottom), (0, 40, 1280, 720));
        assert_eq!(place_in(&SavedWindow::default(), area(100, 100)), None, "never known");
    }

    fn browser(hwnd: u64, shown: &str) -> Seen {
        Seen {
            showing: Some(shown.into()),
            ..seen(hwnd, "chrome.exe", "chrome.exe")
        }
    }

    #[test]
    fn only_the_very_same_window_is_reused_each_once() {
        let set = capture(
            "x",
            &[
                browser(1, "localhost:1"),
                browser(2, "localhost:2"),
                browser(3, "localhost:2"),
            ],
        );
        // Open now: the second address once, and the first program at another address.
        let open = [browser(10, "localhost:2"), browser(11, "localhost:9")];
        let steps = plan(&set, &open);
        assert!(matches!(&steps[0], Step::Launch(w) if w.address == "http://localhost:1"));
        assert!(matches!(&steps[1], Step::Move { hwnd: 10, .. }));
        assert!(matches!(&steps[2], Step::Launch(w) if w.address == "http://localhost:2"));

        let always = SavedDesktop {
            start_again: true,
            ..set.clone()
        };
        assert!(plan(&always, &open).iter().all(|s| matches!(s, Step::Launch(_))));
        assert_eq!(plan(&SavedDesktop::default(), &open), vec![]);
    }

    fn window(exe: &str, floating: bool) -> SavedWindow {
        SavedWindow {
            exe: exe.into(),
            floating,
            ..SavedWindow::default()
        }
    }

    #[test]
    fn programs_start_one_at_a_time_and_a_slow_one_does_not_hold_the_rest() {
        let t = Instant::now();
        let mut o = Opening::default();
        assert_eq!(o.next(t), None);
        assert_eq!(o.wake_in(t), None, "nothing to do");
        o.add([window("a.exe", false), window("b.exe", false), window("c.exe", false)]);
        assert_eq!(o.next(t).unwrap().exe, "a.exe");
        assert_eq!(o.next(t), None, "the first has its time");
        assert_eq!(o.wake_in(t), Some(PATIENCE));
        // Its window comes: the next starts at once.
        assert_eq!(o.came("A.EXE", false).unwrap().exe, "a.exe");
        assert_eq!(o.next(t).unwrap().exe, "b.exe");
        // No window: past the patience the last starts anyway, and b may still come.
        let later = t + PATIENCE;
        assert_eq!(o.next(later).unwrap().exe, "c.exe");
        assert_eq!(o.next(later), None, "nothing left to start");
        assert_eq!(o.came("b.exe", false).unwrap().exe, "b.exe");
        assert_eq!(o.expired(later), 0);
        assert_eq!(o.wake_in(later), Some(WAIT));
        assert_eq!(o.expired(later + WAIT), 1, "c never came");
        assert_eq!(o.wake_in(later + WAIT), None);
    }

    #[test]
    fn a_splash_is_not_taken_for_a_tiled_window() {
        let t = Instant::now();
        let mut o = Opening::default();
        o.add([window("a.exe", false)]);
        o.next(t);
        assert_eq!(o.came("a.exe", true), None, "floats by itself");
        assert!(o.came("a.exe", false).is_some());
        // A window saved floating is that kind of window.
        o.add([window("b.exe", true)]);
        o.next(t);
        assert!(o.came("b.exe", true).is_some());
    }

    #[test]
    fn a_program_that_cannot_start_is_forgotten() {
        let t = Instant::now();
        let mut o = Opening::default();
        o.add([window("a.exe", false), window("b.exe", false)]);
        o.next(t);
        assert!(o.failed("a.exe"));
        assert!(!o.failed("a.exe"), "only once");
        assert_eq!(o.next(t).unwrap().exe, "b.exe", "the rest go on");
    }
}
