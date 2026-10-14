//! The bar's apps module (DECISIONS D25, D45): the pinned apps and the programs with
//! open windows, one entry per program, as Windows' taskbar has them: dragged into
//! another order, pinned and unpinned. Kabuk keeps its own pins (first read from the
//! Windows taskbar's); Windows' own are never changed. Pure.

/// A pinned app.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pin {
    /// The shortcut, which starts the app.
    pub link: String,
    /// The program it starts, when the shortcut points at a file.
    pub exe: Option<String>,
    /// The id Windows groups its windows by (`AppUserModelID`), when the shortcut or the
    /// packaged app has one: a web app's pin starts `chrome_proxy.exe` while its windows
    /// are `chrome.exe`'s, so only this tells them from the browser's. Read again from
    /// the shortcut at each start; saved for a program pinned from its window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

/// A managed window and its program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    pub hwnd: u64,
    pub exe: String,
    /// Where its picture comes from: the program, or a packaged app's
    /// `shell:AppsFolder\<app id>` (their programs carry no icon of their own).
    pub icon: String,
    /// The id Windows groups it by on the taskbar (`AppUserModelID`), when it has one.
    pub app_id: Option<String>,
}

/// One icon of the module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    /// The program's path (or the shortcut's, for a pin that is not a program).
    pub source: String,
    /// Where the picture comes from: a pin's shortcut (which names its own icon, as web
    /// apps' shortcuts do), else the program's [`Running::icon`].
    pub icon: String,
    /// The pin's shortcut, to start it when it is not running.
    pub link: Option<String>,
    /// Its windows, front first.
    pub windows: Vec<u64>,
    pub focused: bool,
    /// The id Windows groups it by (its pin's, else its first window's), kept when it is
    /// pinned.
    pub app_id: Option<String>,
}

/// What a click on an app does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClickAction {
    Minimize(u64),
    Show(u64),
    /// Start the pinned app, or another window of the program (`target` for the shell).
    Start(String),
    Nothing,
}

/// A click on `app` with `front` the window in front, as on Windows' taskbar: a middle
/// or Shift click (`new_window`) starts another window; a click on the app in front
/// minimises its window, or, with several windows, shows the next one (in a fixed
/// order, so repeated clicks go through all of them); otherwise its front window is
/// shown, or the pinned app started.
pub fn click_action(app: &App, front: Option<u64>, new_window: bool) -> ClickAction {
    let target = || app.link.clone().unwrap_or_else(|| app.source.clone());
    if new_window {
        return ClickAction::Start(target());
    }
    if let Some(f) = front.filter(|f| app.windows.contains(f)) {
        if app.windows.len() == 1 {
            return ClickAction::Minimize(f);
        }
        let mut ring = app.windows.clone();
        ring.sort_unstable();
        let at = ring.iter().position(|w| *w == f).unwrap_or(0);
        return ClickAction::Show(ring[(at + 1) % ring.len()]);
    }
    match (app.windows.first(), &app.link) {
        (Some(&w), _) => ClickAction::Show(w),
        (None, Some(_)) => ClickAction::Start(target()),
        (None, None) => ClickAction::Nothing,
    }
}

/// One pin as Windows' taskbar lists it (`Taskband\Favorites`): a shortcut in its pin
/// folder, by file name, or a packaged app, by its app id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskbandPin {
    Shortcut(String),
    App(String),
}

/// Whether `c` can be part of an app id (`Publisher.Name_hash!App`).
fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '!')
}

/// The pins in the taskbar's own list, in the order the taskbar shows them. The list is
/// Windows' undocumented binary record of shell items; what it names is found as text in
/// it (UTF-16, at either byte alignment): shortcut file names (`… .lnk`), each matched to
/// one of `shortcuts` (the pinned shortcuts' file names; the record's own bytes can read
/// as letters before a name), and packaged apps' ids (`Publisher.Name_hash!App`), each
/// once.
pub fn taskband_pins(blob: &[u8], shortcuts: &[String]) -> Vec<TaskbandPin> {
    let mut found: Vec<(usize, TaskbandPin)> = Vec::new();
    for start in [0usize, 1] {
        let units: Vec<(usize, u16)> = blob
            .get(start..)
            .unwrap_or_default()
            .chunks_exact(2)
            .enumerate()
            .map(|(i, b)| (start + 2 * i, u16::from_le_bytes([b[0], b[1]])))
            .collect();
        // Runs of printable characters, with where each starts.
        let mut run: Vec<(usize, char)> = Vec::new();
        let mut runs = Vec::new();
        for (at, u) in units {
            match char::from_u32(u32::from(u)).filter(|c| !c.is_control() && *c != '\u{fffd}') {
                Some(c) => run.push((at, c)),
                None => runs.push(std::mem::take(&mut run)),
            }
        }
        runs.push(run);
        for run in runs.into_iter().filter(|r| r.len() > 4) {
            let text: String = run.iter().map(|(_, c)| *c).collect();
            let chars: Vec<char> = text.chars().collect();
            // Shortcuts: the file name before each ".lnk".
            let lower = text.to_lowercase();
            let mut from = 0;
            while let Some(i) = lower[from..].find(".lnk") {
                let end_byte = from + i + 4;
                let end = text[..end_byte].chars().count();
                let begin = chars[..end - 4]
                    .iter()
                    .rposition(|c| matches!(c, '\\' | '/' | ':' | '"'))
                    .map_or(0, |b| b + 1);
                let text: String = chars[begin..end].iter().collect::<String>().to_lowercase();
                // The longest known name the text ends with.
                let name = shortcuts
                    .iter()
                    .filter(|n| text.ends_with(&n.to_lowercase()))
                    .max_by_key(|n| n.len());
                if let Some(name) = name {
                    found.push((run[begin].0, TaskbandPin::Shortcut(name.clone())));
                }
                from = end_byte;
            }
            // Packaged apps: a word with one '!' after a '_'.
            let mut i = 0;
            while i < chars.len() {
                if !id_char(chars[i]) {
                    i += 1;
                    continue;
                }
                let j = chars[i..]
                    .iter()
                    .position(|c| !id_char(*c))
                    .map_or(chars.len(), |n| i + n);
                let word: String = chars[i..j].iter().collect();
                let bang = word.find('!');
                if word.matches('!').count() == 1 && bang.is_some_and(|b| word[..b].contains('_') && b + 1 < word.len())
                {
                    found.push((run[i].0, TaskbandPin::App(word)));
                }
                i = j;
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    let mut out: Vec<TaskbandPin> = Vec::new();
    for (_, pin) in found {
        let dup = out.iter().any(|o| match (o, &pin) {
            (TaskbandPin::Shortcut(a), TaskbandPin::Shortcut(b)) | (TaskbandPin::App(a), TaskbandPin::App(b)) => {
                same(a, b)
            }
            _ => false,
        });
        if !dup {
            out.push(pin);
        }
    }
    out
}

/// Kabuk's pins with every pin of Windows' it has not seen before added after them
/// (`seen` grows with them): a pin the user took off in Kabuk does not come back, and
/// one pinned on Windows' taskbar later does.
pub fn merged(mut pins: Vec<Pin>, seen: &mut Vec<String>, windows: &[Pin]) -> Vec<Pin> {
    for w in windows {
        if !seen.iter().any(|s| same(s, &w.link)) {
            seen.push(w.link.clone());
            if !pins.iter().any(|p| same(&p.link, &w.link)) {
                pins.push(w.clone());
            }
        }
    }
    pins
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The key an app has in `order`: its pin's shortcut, else its program.
fn key(a: &App) -> &str {
    a.link.as_deref().unwrap_or(&a.source)
}

/// Pins first in their order, then other programs in `order` (the order they first had
/// a window, kept by the caller), each with its windows in `running`'s order (front first).
/// Where `order` also places pins (an app dragged among them), its places win; an app it
/// does not place stays right after the one before it.
pub fn apps(pins: &[Pin], running: &[Running], order: &[String], focused: Option<u64>) -> Vec<App> {
    let windows_of = |exe: &str| -> Vec<u64> { running.iter().filter(|r| same(&r.exe, exe)).map(|r| r.hwnd).collect() };
    // As Windows' taskbar: a window whose app id is a pin's goes to that pin; one whose
    // id no pin has goes by its program (a packaged app's pin is its
    // `shell:AppsFolder\<id>`, which its windows' icon is too).
    let claimed = |id: &str| pins.iter().any(|p| p.app_id.as_deref().is_some_and(|c| same(c, id)));
    let belongs = |p: &Pin, r: &Running| -> bool {
        if let (Some(pin), Some(window)) = (p.app_id.as_deref(), r.app_id.as_deref()) {
            if same(pin, window) {
                return true;
            }
        }
        if r.app_id.as_deref().is_some_and(claimed) {
            return false;
        }
        match p.exe.as_deref() {
            Some(exe) => same(&r.exe, exe),
            None => same(&r.icon, &p.link),
        }
    };
    // The app id of an app's first window that has one.
    let id_of = |windows: &[u64]| -> Option<String> {
        windows
            .iter()
            .find_map(|h| running.iter().find(|r| r.hwnd == *h)?.app_id.clone())
    };
    // A window goes to the first pin of its program only (Chrome and its profiles' pins
    // share chrome.exe).
    let mut taken: Vec<u64> = Vec::new();
    let mut out: Vec<App> = pins
        .iter()
        .map(|p| {
            let windows: Vec<u64> = running
                .iter()
                .filter(|r| belongs(p, r) && !taken.contains(&r.hwnd))
                .map(|r| r.hwnd)
                .collect();
            taken.extend(&windows);
            App {
                source: p.exe.clone().unwrap_or_else(|| p.link.clone()),
                icon: p.link.clone(),
                link: Some(p.link.clone()),
                focused: focused.is_some_and(|f| windows.contains(&f)),
                app_id: p.app_id.clone().or_else(|| id_of(&windows)),
                windows,
            }
        })
        .collect();
    // Programs with windows no pin took (a browser beside its pinned web apps).
    for exe in order {
        let windows: Vec<u64> = windows_of(exe).into_iter().filter(|h| !taken.contains(h)).collect();
        if windows.is_empty() {
            continue;
        }
        let icon = running
            .iter()
            .find(|r| same(&r.exe, exe))
            .map_or_else(|| exe.clone(), |r| r.icon.clone());
        out.push(App {
            source: exe.clone(),
            icon,
            link: None,
            focused: focused.is_some_and(|f| windows.contains(&f)),
            app_id: id_of(&windows),
            windows,
        });
    }
    // Each app's rank: its place in `order`, or just after the app before it.
    let mut rank = (-1.0_f64, 0u32);
    let mut ranked: Vec<((f64, u32), App)> = out
        .into_iter()
        .map(|a| {
            rank = match order.iter().position(|o| same(o, key(&a))) {
                Some(i) => (i as f64, 0),
                None => (rank.0, rank.1 + 1),
            };
            (rank, a)
        })
        .collect();
    ranked.sort_by(|(x, _), (y, _)| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    ranked.into_iter().map(|(_, a)| a).collect()
}

/// `order` with programs that opened their first window appended (and nothing removed,
/// so an app keeps its place while its windows come and go within a session).
pub fn remember_order(order: &mut Vec<String>, running: &[Running]) {
    for r in running {
        if !order.iter().any(|o| same(o, &r.exe)) {
            order.push(r.exe.clone());
        }
    }
}

/// How far an app's icon has grown `elapsed` seconds after the mouse came onto it
/// (`entering`) or left it, over `duration` seconds: eased in and out, 0-1. A zero
/// duration jumps.
pub fn growth(elapsed: f32, duration: f32, entering: bool) -> f32 {
    let t = if duration > 0.0 {
        (elapsed / duration).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let eased = t * t * (3.0 - 2.0 * t);
    if entering {
        eased
    } else {
        1.0 - eased
    }
}

/// The pins after app `from` of `list` was dragged to place `to`: pinned apps keep their
/// new order among themselves; `order` holds every app's new place (pins by their
/// shortcut), then the closed programs it had.
pub fn dragged(list: &[App], pins: &[Pin], order: &mut Vec<String>, from: usize, to: usize) -> Vec<Pin> {
    if from >= list.len() {
        return pins.to_vec();
    }
    let mut moved = list.to_vec();
    let app = moved.remove(from);
    moved.insert(to.min(moved.len()), app);
    let pinned: Vec<Pin> = moved
        .iter()
        .filter_map(|a| {
            pins.iter()
                .find(|p| a.link.as_deref().is_some_and(|l| same(l, &p.link)))
                .cloned()
        })
        .collect();
    // Every app in its new place, the rest of `order` (closed programs) after.
    let mut new_order: Vec<String> = moved.iter().map(|a| key(a).to_string()).collect();
    let rest: Vec<String> = order
        .iter()
        .filter(|o| !new_order.iter().any(|n| same(n, o)))
        .cloned()
        .collect();
    new_order.extend(rest);
    *order = new_order;
    pinned
}

/// The pins with `app` pinned at the end (a running program pins as itself).
pub fn pinned(pins: &[Pin], app: &App) -> Vec<Pin> {
    let mut out = pins.to_vec();
    if app.link.is_none() {
        out.push(Pin {
            app_id: app.app_id.clone(),
            link: app.source.clone(),
            exe: Some(app.source.clone()),
        });
    }
    out
}

/// The pins without `app`.
pub fn unpinned(pins: &[Pin], app: &App) -> Vec<Pin> {
    pins.iter()
        .filter(|p| !app.link.as_deref().is_some_and(|l| same(l, &p.link)))
        .cloned()
        .collect()
}

/// A name for each app of `list` that no other has, kept while apps change places: its
/// key in `order` (two pins of one program differ by shortcut), then how many before it
/// had the same key.
pub fn slide_ids(list: &[App]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for (i, a) in list.iter().enumerate() {
        let k = key(a).to_lowercase();
        let n = list[..i].iter().filter(|b| same(key(b), &k)).count();
        out.push(format!("{k}#{n}"));
    }
    out
}

/// The apps that changed places between `before` and `after` (both [`slide_ids`]),
/// other than `moved`: each one's name and how many places it went back (negative:
/// forward).
pub fn slides(before: &[String], after: &[String], moved: &str) -> Vec<(String, f32)> {
    after
        .iter()
        .enumerate()
        .filter(|(_, id)| id.as_str() != moved)
        .filter_map(|(new, id)| {
            let old = before.iter().position(|b| b == id)?;
            (old != new).then(|| (id.clone(), old as f32 - new as f32))
        })
        .collect()
}

/// A picture key for an app's icon that never collides with a tray icon's (an `HICON`
/// value, far below the top bit).
pub fn image_key(source: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.to_lowercase().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash | (1 << 63)
}

#[cfg(test)]
mod tests {
    //! Edge cases: no pins and no windows, a pin running and not, a pin that is not a
    //! program, two windows of one program (case of the path differs), the focused
    //! window's app, the order kept when windows close, picture keys.
    use super::*;

    const CODE: &str = r"C:\Apps\Code\Code.exe";
    const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";

    fn pin(link: &str, exe: Option<&str>) -> Pin {
        Pin {
            app_id: None,
            link: link.into(),
            exe: exe.map(String::from),
        }
    }

    fn run(hwnd: u64, exe: &str) -> Running {
        Running {
            app_id: None,
            hwnd,
            exe: exe.into(),
            icon: exe.into(),
        }
    }

    #[test]
    fn pins_come_first_and_running_ones_carry_their_windows() {
        assert!(apps(&[], &[], &[], None).is_empty());
        let pins = [pin("Chrome.lnk", Some(CHROME)), pin("Store app.lnk", None)];
        let running = [run(1, &CODE.to_uppercase()), run(2, CHROME), run(3, CODE)];
        let mut order = Vec::new();
        remember_order(&mut order, &running);
        let a = apps(&pins, &running, &order, Some(3));
        assert_eq!(a.len(), 3);
        assert_eq!(
            (a[0].source.as_str(), a[0].windows.clone(), a[0].focused),
            (CHROME, vec![2], false)
        );
        assert_eq!((a[1].source.as_str(), a[1].windows.len()), ("Store app.lnk", 0));
        assert_eq!(a[1].link.as_deref(), Some("Store app.lnk"));
        // Code: both windows (the path's case differs), focused, not pinned.
        assert_eq!(
            (a[2].windows.clone(), a[2].focused, a[2].link.clone()),
            (vec![1, 3], true, None)
        );
    }

    #[test]
    fn a_program_keeps_its_place_while_its_windows_come_and_go() {
        let mut order = Vec::new();
        remember_order(&mut order, &[run(1, CODE), run(2, CHROME)]);
        // Code's window closed, Chrome stays: only Chrome shows.
        let a = apps(&[], &[run(2, CHROME)], &order, None);
        assert_eq!(a.iter().map(|x| x.source.as_str()).collect::<Vec<_>>(), vec![CHROME]);
        // Code opens again: back before Chrome, where it was.
        remember_order(&mut order, &[run(3, CODE), run(2, CHROME)]);
        let a = apps(&[], &[run(2, CHROME), run(3, CODE)], &order, None);
        assert_eq!(
            a.iter().map(|x| x.source.as_str()).collect::<Vec<_>>(),
            vec![CODE, CHROME]
        );
    }

    /// Reported by the user: web apps pinned from Chrome and Store apps showed a blank
    /// program icon.
    #[test]
    fn pictures_come_from_the_pin_or_the_packaged_app() {
        const PROXY: &str = r"C:\Program Files\Google\Chrome\chrome_proxy.exe";
        const WHATSAPP: &str = r"C:\Program Files\WindowsApps\WhatsApp\WhatsApp.Root.exe";
        let store = Running {
            app_id: None,
            hwnd: 4,
            exe: WHATSAPP.into(),
            icon: r"shell:AppsFolder\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into(),
        };
        let pins = [pin("Jitsi Meet.lnk", Some(PROXY))];
        let running = [store.clone(), run(1, CODE)];
        let mut order = Vec::new();
        remember_order(&mut order, &running);
        let a = apps(&pins, &running, &order, None);
        let icons: Vec<&str> = a.iter().map(|x| x.icon.as_str()).collect();
        assert_eq!(icons, vec!["Jitsi Meet.lnk", store.icon.as_str(), CODE]);
    }

    #[test]
    fn picture_keys_are_per_program_and_apart_from_tray_icons() {
        assert_eq!(image_key(CODE), image_key(&CODE.to_lowercase()));
        assert_ne!(image_key(CODE), image_key(CHROME));
        assert!(image_key(CODE) >= 1 << 63);
    }

    fn app(source: &str, link: Option<&str>, windows: Vec<u64>) -> App {
        App {
            source: source.into(),
            icon: source.into(),
            link: link.map(Into::into),
            windows,
            focused: false,
            app_id: None,
        }
    }

    /// Reported by the user (a video): while a Chrome profile's pin was dragged past the
    /// other Chrome pin, apps of a shared program vanished or flew far off, their slides
    /// told apart only by program. Each slides on its own: only the one passed over.
    #[test]
    fn apps_of_one_program_slide_on_their_own() {
        let explorer = r"C:\Windows\explorer.exe";
        let list = vec![
            app(explorer, Some(r"C:\L\This PC.lnk"), vec![]),
            app(explorer, Some(r"C:\L\Explorer.lnk"), vec![]),
            app(CHROME, Some(r"C:\L\Chrome (Work).lnk"), vec![1]),
            app(CHROME, Some(r"C:\L\Chrome.lnk"), vec![]),
            app(CODE, None, vec![2]),
            app(CODE, None, vec![3]),
        ];
        let ids = slide_ids(&list);
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[..i].contains(id), "{id} twice");
        }
        // The profile's pin (third) dragged one place on: only the other Chrome slides
        // back a place; the rest stay.
        let mut after = list.clone();
        after.swap(2, 3);
        let moved = ids[2].clone();
        assert_eq!(slides(&ids, &slide_ids(&after), &moved), vec![(ids[3].clone(), 1.0)]);
        // Dragged two places back: the two passed over each slide a place on.
        let mut back = list.clone();
        let a = back.remove(3);
        back.insert(1, a);
        let got = slides(&ids, &slide_ids(&back), &ids[3]);
        assert_eq!(got, vec![(ids[1].clone(), -1.0), (ids[2].clone(), -1.0)]);
        // Nothing moved, nothing slides.
        assert!(slides(&ids, &ids, &ids[0]).is_empty());
    }

    #[test]
    fn a_drag_reorders_pins_and_programs() {
        let pins = vec![
            pin(r"C:\L\a.lnk", Some(r"C:\A.exe")),
            pin(r"C:\L\b.lnk", Some(r"C:\B.exe")),
        ];
        let list = vec![
            app(r"C:\A.exe", Some(r"C:\L\a.lnk"), vec![]),
            app(r"C:\B.exe", Some(r"C:\L\b.lnk"), vec![1]),
            app(r"C:\X.exe", None, vec![2]),
            app(r"C:\Y.exe", None, vec![3]),
        ];
        let mut order = vec![
            r"C:\X.exe".to_string(),
            r"C:\Y.exe".to_string(),
            r"C:\Gone.exe".to_string(),
        ];
        // B to the front: the pins swap; the programs stay after them.
        let p = dragged(&list, &pins, &mut order, 1, 0);
        assert_eq!(
            p.iter().map(|p| p.link.as_str()).collect::<Vec<_>>(),
            [r"C:\L\b.lnk", r"C:\L\a.lnk"]
        );
        // Every app's place is kept (pins by their shortcut), a closed program after.
        assert_eq!(
            order,
            [r"C:\L\b.lnk", r"C:\L\a.lnk", r"C:\X.exe", r"C:\Y.exe", r"C:\Gone.exe"]
        );
        // Y before X: the programs' order follows, a closed one stays last.
        let p = dragged(&list, &pins, &mut order, 3, 2);
        assert_eq!(p, pins);
        assert_eq!(
            order,
            [r"C:\L\a.lnk", r"C:\L\b.lnk", r"C:\Y.exe", r"C:\X.exe", r"C:\Gone.exe"]
        );
        // Past the end, or from nowhere.
        assert_eq!(
            dragged(&list, &pins, &mut order, 0, 99).last().unwrap().link,
            r"C:\L\a.lnk"
        );
        assert_eq!(dragged(&list, &pins, &mut order, 9, 0), pins);
    }

    /// Asked for by the user (as on Windows' taskbar): a running program dragged among
    /// the pins stays there for the session; the pins keep their own order.
    #[test]
    fn a_program_dragged_among_the_pins_stays_there() {
        let pins = vec![
            pin(r"C:\L\a.lnk", Some(r"C:\A.exe")),
            pin(r"C:\L\b.lnk", Some(r"C:\B.exe")),
        ];
        let running = vec![
            Running {
                app_id: None,
                hwnd: 2,
                exe: r"C:\X.exe".into(),
                icon: r"C:\X.exe".into(),
            },
            Running {
                app_id: None,
                hwnd: 3,
                exe: r"C:\Y.exe".into(),
                icon: r"C:\Y.exe".into(),
            },
        ];
        let mut order = vec![r"C:\X.exe".to_string(), r"C:\Y.exe".to_string()];
        let sources = |l: &[App]| l.iter().map(|a| a.source.clone()).collect::<Vec<_>>();
        let list = apps(&pins, &running, &order, None);
        assert_eq!(sources(&list), [r"C:\A.exe", r"C:\B.exe", r"C:\X.exe", r"C:\Y.exe"]);
        // X to the front, before both pins.
        let p = dragged(&list, &pins, &mut order, 2, 0);
        assert_eq!(p, pins, "the pins keep their order");
        let list = apps(&p, &running, &order, None);
        assert_eq!(sources(&list), [r"C:\X.exe", r"C:\A.exe", r"C:\B.exe", r"C:\Y.exe"]);
        // Y between the pins.
        let p = dragged(&list, &p, &mut order, 3, 2);
        let list = apps(&p, &running, &order, None);
        assert_eq!(sources(&list), [r"C:\X.exe", r"C:\A.exe", r"C:\Y.exe", r"C:\B.exe"]);
        // A pin added later goes after the last pin; a program opened later goes last.
        let mut more = p.clone();
        more.push(pin(r"C:\L\c.lnk", None));
        let mut running = running;
        running.push(Running {
            app_id: None,
            hwnd: 4,
            exe: r"C:\Z.exe".into(),
            icon: r"C:\Z.exe".into(),
        });
        remember_order(&mut order, &running);
        let list = apps(&more, &running, &order, None);
        assert_eq!(
            sources(&list),
            [
                r"C:\X.exe",
                r"C:\A.exe",
                r"C:\Y.exe",
                r"C:\B.exe",
                r"C:\L\c.lnk",
                r"C:\Z.exe"
            ]
        );
    }

    #[test]
    fn a_running_program_is_pinned_and_a_pin_unpinned() {
        let pins = vec![pin(r"C:\L\a.lnk", Some(r"C:\A.exe"))];
        let x = app(r"C:\X.exe", None, vec![2]);
        let p = pinned(&pins, &x);
        assert_eq!(p.len(), 2);
        assert_eq!(
            (p[1].link.as_str(), p[1].exe.as_deref()),
            (r"C:\X.exe", Some(r"C:\X.exe"))
        );
        // Pinning a pinned one changes nothing.
        let a = app(r"C:\A.exe", Some(r"C:\L\a.lnk"), vec![]);
        assert_eq!(pinned(&pins, &a), pins);
        assert!(unpinned(&pins, &a).is_empty());
        assert_eq!(unpinned(&pins, &x), pins, "not pinned");
    }

    #[test]
    fn an_icon_grows_smoothly_and_shrinks_back() {
        assert_eq!(growth(0.0, 0.12, true), 0.0);
        assert_eq!(growth(0.12, 0.12, true), 1.0);
        assert_eq!(growth(5.0, 0.12, true), 1.0);
        assert_eq!(growth(0.0, 0.12, false), 1.0);
        assert_eq!(growth(0.12, 0.12, false), 0.0);
        let half = growth(0.06, 0.12, true);
        assert!((half - 0.5).abs() < 1e-6, "{half}");
        let early = growth(0.012, 0.12, true);
        assert!(early > 0.0 && early < 0.1, "eased in: {early}");
        assert_eq!(growth(0.0, 0.0, true), 1.0, "no time: it jumps");
        assert_eq!(growth(-1.0, 0.12, true), 0.0);
    }

    /// Found on the user's machine: WhatsApp and Outlook, pinned on Windows' taskbar,
    /// were missing (packaged apps have no shortcut in the pin folder; the taskbar's own
    /// list names them), and the pins came in name order instead of the taskbar's.
    #[test]
    fn the_taskbars_own_list_gives_its_pins_in_order() {
        let mut blob = vec![0x00, 0x88, 0x01, 0x00, 0x14, 0x1f];
        let put = |blob: &mut Vec<u8>, text: &str| blob.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        put(
            &mut blob,
            r"C:\Users\u\AppData\Roaming\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\Google Chrome.lnk",
        );
        blob.extend([0, 0, 7, 0x31, 0x9e]);
        // Odd alignment, as in the user's list.
        blob.push(0x42);
        put(&mut blob, "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App");
        blob.extend([0, 0, 0x2a]);
        put(&mut blob, "Visual Studio Code.lnk");
        blob.extend([0, 0]);
        put(&mut blob, "VISUAL STUDIO CODE.LNK");
        blob.extend([0, 0, 1, 0]);
        put(
            &mut blob,
            "Microsoft.OutlookForWindows_8wekyb3d8bbwe!Microsoft.OutlookforWindows",
        );
        blob.extend([0, 0, 5]);
        let known = ["Google Chrome.lnk".to_string(), "Visual Studio Code.lnk".to_string()];
        assert_eq!(
            taskband_pins(&blob, &known),
            vec![
                TaskbandPin::Shortcut("Google Chrome.lnk".into()),
                TaskbandPin::App("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into()),
                TaskbandPin::Shortcut("Visual Studio Code.lnk".into()),
                TaskbandPin::App("Microsoft.OutlookForWindows_8wekyb3d8bbwe!Microsoft.OutlookforWindows".into()),
            ]
        );
        assert!(taskband_pins(&[], &known).is_empty());
        assert!(taskband_pins(&[1, 2, 3], &known).is_empty());
        // A name no pinned shortcut has (unpinned since) is left out.
        assert!(!taskband_pins(&blob, &known[..1]).contains(&TaskbandPin::Shortcut("Visual Studio Code.lnk".into())));
    }

    #[test]
    fn new_windows_pins_are_added_once_and_unpinned_ones_stay_off() {
        let ours = vec![pin(r"C:\L\b.lnk", None), pin(r"C:\L\a.lnk", None)];
        let theirs = vec![
            pin(r"C:\L\a.lnk", None),
            pin(r"shell:AppsFolder\W_x!App", None),
            pin(r"C:\L\gone.lnk", None),
        ];
        // An old pins file: what it holds counts as seen; the user had unpinned gone.lnk.
        let mut seen = vec![
            r"C:\L\b.lnk".to_string(),
            r"C:\L\a.lnk".to_string(),
            r"C:\L\gone.lnk".to_string(),
        ];
        let out = merged(ours.clone(), &mut seen, &theirs);
        let links: Vec<&str> = out.iter().map(|p| p.link.as_str()).collect();
        assert_eq!(links, [r"C:\L\b.lnk", r"C:\L\a.lnk", r"shell:AppsFolder\W_x!App"]);
        assert_eq!(seen.len(), 4);
        // Unpinned in Kabuk later: it stays off.
        let again = merged(vec![ours[0].clone()], &mut seen, &theirs);
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn a_packaged_apps_pin_holds_its_windows() {
        let pins = vec![pin(
            r"shell:AppsFolder\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App",
            None,
        )];
        let running = vec![Running {
            app_id: None,
            hwnd: 9,
            exe: r"C:\Program Files\WindowsApps\WhatsApp\WhatsApp.exe".into(),
            icon: r"shell:AppsFolder\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into(),
        }];
        let order = vec![running[0].exe.clone()];
        let list = apps(&pins, &running, &order, Some(9));
        assert_eq!(list.len(), 1, "one icon, not a second for the program");
        assert_eq!(list[0].windows, vec![9]);
        assert!(list[0].focused && list[0].link.is_some());
    }

    #[test]
    fn a_window_goes_to_the_first_pin_of_its_program_only() {
        let pins = vec![
            pin(r"C:\L\Google Chrome.lnk", Some(r"C:\Chrome\chrome.exe")),
            pin(r"C:\L\Ahmet (work) - Chrome.lnk", Some(r"C:\Chrome\chrome.exe")),
        ];
        let running = vec![Running {
            app_id: None,
            hwnd: 4,
            exe: r"C:\Chrome\chrome.exe".into(),
            icon: r"C:\Chrome\chrome.exe".into(),
        }];
        let list = apps(&pins, &running, &[], None);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].windows.clone(), list[1].windows.clone()), (vec![4], vec![]));
    }

    /// Reported by the user: a web app's pin (Jitsi Meet, started by `chrome_proxy.exe`)
    /// opened a new window at every click, as its open window (`chrome.exe`'s) was never
    /// found. Windows' taskbar goes by the app id; so does the bar.
    #[test]
    fn a_web_apps_pin_holds_its_windows_by_app_id_and_the_browser_keeps_its_own() {
        const PROXY: &str = r"C:\Program Files\Google\Chrome\chrome_proxy.exe";
        let with_id = |p: Pin, id: &str| Pin {
            app_id: Some(id.into()),
            ..p
        };
        let window = |hwnd: u64, id: Option<&str>| Running {
            app_id: id.map(String::from),
            ..run(hwnd, CHROME)
        };
        let pins = vec![
            with_id(pin(r"C:\L\Google Chrome.lnk", Some(CHROME)), "Chrome"),
            with_id(pin(r"C:\L\Jitsi Meet.lnk", Some(PROXY)), "Chrome._crx_jitsi"),
            with_id(pin(r"C:\L\Milvasoft.lnk", Some(PROXY)), "Chrome._crx_milva"),
        ];
        let running = vec![
            window(1, Some("Chrome._crx_jitsi")),
            window(2, Some("Chrome")),
            window(3, None),
            window(4, Some("Chrome._crx_milva")),
        ];
        let list = apps(&pins, &running, &[], Some(1));
        let windows: Vec<Vec<u64>> = list.iter().map(|a| a.windows.clone()).collect();
        assert_eq!(windows, [vec![2, 3], vec![1], vec![4]]);
        assert!(list[1].focused && !list[0].focused);
    }

    /// The browser unpinned: a web app's pinned windows stay with their pin, and the
    /// browser's still get an icon of their own.
    #[test]
    fn the_browsers_windows_show_beside_a_pinned_web_app() {
        const PROXY: &str = r"C:\Program Files\Google\Chrome\chrome_proxy.exe";
        let pins = vec![Pin {
            app_id: Some("Chrome._crx_jitsi".into()),
            ..pin(r"C:\L\Jitsi Meet.lnk", Some(PROXY))
        }];
        let running = vec![
            Running {
                app_id: Some("Chrome._crx_jitsi".into()),
                ..run(1, CHROME)
            },
            Running {
                app_id: Some("Chrome".into()),
                ..run(2, CHROME)
            },
        ];
        let order = vec![CHROME.to_string()];
        let list = apps(&pins, &running, &order, None);
        let windows: Vec<Vec<u64>> = list.iter().map(|a| a.windows.clone()).collect();
        assert_eq!(windows, [vec![1], vec![2]]);
        assert_eq!(list[1].source, CHROME);
    }

    /// Pinned from its window, a program keeps the id its windows go by, in the pins file
    /// too: its menu finds its jump list with no window open (found on the user's
    /// machine: Chrome pinned from a window showed no jump list once closed).
    #[test]
    fn a_program_pinned_from_its_window_keeps_its_app_id() {
        let running = vec![Running {
            app_id: Some("Chrome".into()),
            ..run(4, CHROME)
        }];
        let order = vec![CHROME.to_string()];
        let list = apps(&[], &running, &order, None);
        assert_eq!(list[0].app_id.as_deref(), Some("Chrome"));
        let pins = pinned(&[], &list[0]);
        assert_eq!(pins[0].app_id.as_deref(), Some("Chrome"));
        let text = serde_json::to_string(&pins).unwrap();
        let back: Vec<Pin> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, pins);
        // Closed, its pin still says it.
        let list = apps(&pins, &[], &order, None);
        assert_eq!(list[0].app_id.as_deref(), Some("Chrome"));
        // An old pins file without ids still reads.
        let old: Vec<Pin> = serde_json::from_str(r#"[{"link":"a.lnk","exe":null}]"#).unwrap();
        assert_eq!(old[0].app_id, None);
    }

    /// Found using the bar: a middle click did nothing (Windows starts another window),
    /// and a click on a program in front with several windows minimised it instead of
    /// going to its next window.
    #[test]
    fn clicks_on_an_app_do_what_windows_taskbar_does() {
        let one = app(CODE, None, vec![5]);
        assert_eq!(click_action(&one, Some(5), false), ClickAction::Minimize(5));
        assert_eq!(click_action(&one, Some(9), false), ClickAction::Show(5));
        assert_eq!(click_action(&one, Some(5), true), ClickAction::Start(CODE.into()));
        // Several windows: the next one each time, through all of them, front order aside.
        let three = app(CHROME, Some(r"C:\L\Chrome.lnk"), vec![30, 10, 20]);
        assert_eq!(click_action(&three, Some(30), false), ClickAction::Show(10));
        assert_eq!(click_action(&three, Some(10), false), ClickAction::Show(20));
        assert_eq!(click_action(&three, Some(20), false), ClickAction::Show(30));
        // A middle click starts it again from its pin.
        assert_eq!(
            click_action(&three, None, true),
            ClickAction::Start(r"C:\L\Chrome.lnk".into())
        );
        // A pin with no window starts; nothing to do for an app with neither.
        let pin = app(CHROME, Some("p.lnk"), Vec::new());
        assert_eq!(click_action(&pin, None, false), ClickAction::Start("p.lnk".into()));
        assert_eq!(
            click_action(&app(CODE, None, Vec::new()), None, false),
            ClickAction::Nothing
        );
    }
}
