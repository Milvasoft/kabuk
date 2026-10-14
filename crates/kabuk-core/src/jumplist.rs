//! Windows' jump lists, pure: what a program's right-click menu on Windows' taskbar
//! shows besides its name. A program's list lives in two files under
//! `%APPDATA%\Microsoft\Windows\Recent`, named by its app id ([`file_stem`]):
//!
//! - `CustomDestinations\<stem>.customDestinations-ms`, the list the program set itself:
//!   its own categories of links (Chrome's "Most visited"), Windows' known ones it asks
//!   for (frequent or recent files) and its tasks (Chrome's "New window").
//! - `AutomaticDestinations\<stem>.automaticDestinations-ms`, the files Windows recorded
//!   for it; its `DestList` stream says which the user pinned ([`dest_list`]).
//!
//! The formats are Windows' own, undocumented but stable since Windows 7; anything not
//! understood gives `None` and the menu shows what Windows' documented calls give.

use std::ops::Range;

pub const CUSTOM_FOLDER: &str = "CustomDestinations";
pub const AUTOMATIC_FOLDER: &str = "AutomaticDestinations";
pub const CUSTOM_EXTENSION: &str = "customDestinations-ms";
pub const AUTOMATIC_EXTENSION: &str = "automaticDestinations-ms";
/// The stream of an automatic destinations file that lists its entries.
pub const DEST_LIST_STREAM: &str = "DestList";

/// CRC-64 (ECMA-182, reflected) Windows names a program's jump list files by.
const HASH_POLY: u64 = 0x92C6_4265_D321_39A4;

/// The known folders Windows names a program's folder by in the app id it makes up for a
/// program that sets none (System, SystemX86, Windows, the program files folders and
/// their common files), as `FOLDERID`s in the form the id carries.
pub const IMPLICIT_FOLDERS: [&str; 7] = [
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
    "{D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27}",
    "{F38BF404-1D43-42F2-9305-67DE0B28FC23}",
    "{6D809377-6AF0-444B-8957-A3773F02200E}",
    "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}",
    "{6365D5A7-0F0D-45E5-87F6-0DA56B6A4F7D}",
    "{DE974D24-D9C6-4D3E-BF91-F4455120B917}",
];

/// The ids Windows may have made up for the program at `exe` when it sets none, most
/// likely first: its path with the deepest of `folders` (a folder's path and its id from
/// [`IMPLICIT_FOLDERS`]) it lies in replaced by that folder's id
/// (`{1AC14E77-…}\notepad.exe`), then the path as it is.
pub fn implicit_app_ids(exe: &str, folders: &[(String, &str)]) -> Vec<String> {
    let lower = exe.to_lowercase();
    let mut inside: Vec<&(String, &str)> = folders
        .iter()
        .filter(|(path, _)| {
            let p = path.trim_end_matches('\\').to_lowercase();
            !p.is_empty() && lower.starts_with(&format!("{p}\\"))
        })
        .collect();
    inside.sort_by_key(|(path, _)| std::cmp::Reverse(path.trim_end_matches('\\').len()));
    let mut out: Vec<String> = inside
        .first()
        .map(|(path, id)| format!("{id}{}", &exe[path.trim_end_matches('\\').len()..]))
        .into_iter()
        .collect();
    out.push(exe.to_string());
    out
}

/// The file name (without extension) of the jump lists of the program with app id `id`:
/// the CRC-64 of the id upper-cased, in UTF-16, as hexadecimal without leading zeros.
pub fn file_stem(app_id: &str) -> String {
    let mut crc = u64::MAX;
    for unit in app_id.to_uppercase().encode_utf16() {
        for byte in unit.to_le_bytes() {
            crc ^= u64::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ HASH_POLY } else { crc >> 1 };
            }
        }
    }
    format!("{crc:x}")
}

/// A category Windows knows and fills itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Known {
    Frequent,
    Recent,
}

/// A category of a program's own list; links are byte ranges of the file, each a
/// serialised shell link ([MS-SHLLINK]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Category {
    /// A category the program named.
    Custom {
        name: String,
        links: Vec<Range<usize>>,
    },
    Known(Known),
    /// The program's tasks (shown last, under "Tasks").
    Tasks(Vec<Range<usize>>),
}

const CUSTOM_VERSION: u32 = 2;
const CATEGORY_CUSTOM: u32 = 0;
const CATEGORY_KNOWN: u32 = 1;
const CATEGORY_TASKS: u32 = 2;
const KNOWN_FREQUENT: u32 = 1;
const KNOWN_RECENT: u32 = 2;
/// Ends each category (and the file).
const FOOTER: u32 = 0xBABF_FBAB;
/// The class of an entry that is a shell link (`CLSID_ShellLink`), as stored.
const SHELL_LINK_CLASS: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// A little-endian reader over a byte slice.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn peek_u32(&self) -> Option<u32> {
        let b = self.bytes.get(self.at..self.at.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn utf16(&mut self, chars: usize) -> Option<String> {
        let b = self.take(chars.checked_mul(2)?)?;
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    }
}

/// Shell link header size and its flags' offset ([MS-SHLLINK] 2.1).
const LINK_HEADER: usize = 0x4C;
const LINK_FLAGS_AT: usize = 0x14;
const HAS_ID_LIST: u32 = 0x1;
const HAS_LINK_INFO: u32 = 0x2;
const IS_UNICODE: u32 = 0x80;
/// The string data's flags, in the order the strings follow.
const LINK_STRINGS: [u32; 5] = [0x4, 0x8, 0x10, 0x20, 0x40];
/// An extra data block shorter than this ends the list.
const EXTRA_END: u32 = 4;

/// The length of the shell link starting at `at`.
fn link_len(bytes: &[u8], at: usize) -> Option<usize> {
    let mut r = Reader { bytes, at };
    if r.u32()? as usize != LINK_HEADER {
        return None;
    }
    r.at = at.checked_add(LINK_FLAGS_AT)?;
    let flags = r.u32()?;
    r.at = at.checked_add(LINK_HEADER)?;
    if flags & HAS_ID_LIST != 0 {
        let n = r.u16()?;
        r.take(usize::from(n))?;
    }
    if flags & HAS_LINK_INFO != 0 {
        // The size counts itself.
        let n = r.peek_u32()? as usize;
        r.take(n.max(4))?;
    }
    let width = if flags & IS_UNICODE != 0 { 2 } else { 1 };
    for bit in LINK_STRINGS {
        if flags & bit != 0 {
            let n = usize::from(r.u16()?);
            r.take(n.checked_mul(width)?)?;
        }
    }
    loop {
        let size = r.peek_u32()?;
        if size < EXTRA_END {
            r.take(4)?;
            break;
        }
        r.take(size as usize)?;
    }
    Some(r.at - at)
}

/// The links of a category: `count` entries, each a class and a shell link.
fn links(r: &mut Reader, count: u32) -> Option<Vec<Range<usize>>> {
    let mut out = Vec::new();
    for _ in 0..count {
        if r.take(16)? != SHELL_LINK_CLASS {
            return None;
        }
        let len = link_len(r.bytes, r.at)?;
        out.push(r.at..r.at + len);
        r.take(len)?;
    }
    Some(out)
}

/// The categories of a program's own list (`.customDestinations-ms`), in its order.
pub fn custom_destinations(bytes: &[u8]) -> Option<Vec<Category>> {
    let mut r = Reader { bytes, at: 0 };
    if r.u32()? != CUSTOM_VERSION {
        return None;
    }
    let count = r.u32()?;
    r.u32()?;
    let mut out = Vec::new();
    for _ in 0..count {
        let category = match r.u32()? {
            CATEGORY_CUSTOM => {
                let chars = usize::from(r.u16()?);
                let name = r.utf16(chars)?;
                let n = r.u32()?;
                Category::Custom {
                    name,
                    links: links(&mut r, n)?,
                }
            }
            CATEGORY_KNOWN => match r.u32()? {
                KNOWN_FREQUENT => Category::Known(Known::Frequent),
                KNOWN_RECENT => Category::Known(Known::Recent),
                _ => return None,
            },
            CATEGORY_TASKS => {
                let n = r.u32()?;
                Category::Tasks(links(&mut r, n)?)
            }
            _ => return None,
        };
        if r.peek_u32() == Some(FOOTER) {
            r.u32()?;
        }
        out.push(category);
    }
    Some(out)
}

/// An entry of an automatic destinations file's `DestList`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    /// A path, or a shell item's parsing name (`::{…}`, `knownfolder:{…}`).
    pub path: String,
    /// Its place among the pinned ones; `None` when it is not pinned.
    pub pin: Option<u32>,
    /// When it was last used (a `FILETIME`).
    pub used: u64,
}

const DEST_HEADER: usize = 32;
/// Windows 7's entries; later versions add fields before the path and after it.
const DEST_VERSION_OLD: u32 = 1;
const DEST_USED_AT: usize = 100;
const DEST_PIN_AT: usize = 108;
const DEST_PATH_AT_OLD: usize = 112;
const DEST_PATH_AT: usize = 128;
const DEST_TAIL: usize = 4;
const NOT_PINNED: i32 = -1;

/// The entries of a `DestList` stream, newest first as Windows keeps them.
pub fn dest_list(bytes: &[u8]) -> Option<Vec<Destination>> {
    let mut r = Reader { bytes, at: 0 };
    let version = r.u32()?;
    let count = r.u32()?;
    let (path_at, tail) = if version == DEST_VERSION_OLD {
        (DEST_PATH_AT_OLD, 0)
    } else {
        (DEST_PATH_AT, DEST_TAIL)
    };
    r.at = DEST_HEADER;
    let mut out = Vec::new();
    for _ in 0..count {
        let start = r.at;
        r.at = start.checked_add(DEST_USED_AT)?;
        let used = u64::from(r.u32()?) | (u64::from(r.u32()?) << 32);
        r.at = start.checked_add(DEST_PIN_AT)?;
        let pin = r.u32()? as i32;
        r.at = start.checked_add(path_at)?;
        let chars = usize::from(r.u16()?);
        let path = r.utf16(chars)?;
        r.take(tail)?;
        out.push(Destination {
            path,
            pin: (pin != NOT_PINNED).then(|| u32::try_from(pin).ok()).flatten(),
            used,
        });
    }
    Some(out)
}

/// The pinned paths in their pinned order.
pub fn pinned(entries: &[Destination]) -> Vec<String> {
    let mut pins: Vec<(u32, &str)> = entries.iter().filter_map(|d| Some((d.pin?, d.path.as_str()))).collect();
    pins.sort_by_key(|(at, _)| *at);
    pins.into_iter().map(|(_, p)| p.to_string()).collect()
}

/// The entries that are not pinned, last used first (Windows' recent files for a program
/// it gives no other list of).
pub fn recent(entries: &[Destination]) -> Vec<String> {
    let mut rest: Vec<&Destination> = entries.iter().filter(|d| d.pin.is_none()).collect();
    rest.sort_by_key(|d| std::cmp::Reverse(d.used));
    rest.into_iter().map(|d| d.path.clone()).collect()
}

/// An entry of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub launch: Launch,
}

/// What an entry starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A file or folder, opened with the program.
    File(String),
    /// A link's program with its arguments, in its folder.
    Run { path: String, args: String, dir: String },
}

/// A heading of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Heading {
    Pinned,
    Frequent,
    Recent,
    Named(String),
    Tasks,
}

/// A heading with its entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub heading: Heading,
    pub entries: Vec<Entry>,
}

/// A category of the program's own list, its links read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Named(String, Vec<Entry>),
    Known(Known),
    Tasks(Vec<Entry>),
}

/// The menu's groups in the order of Windows' taskbar: the pinned entries, the program's
/// categories in its order (Windows' recent files when it set none), then its tasks.
/// `known` gives Windows' frequent or recent files; one that is pinned shows only under
/// "Pinned". Each group shows at most `max` entries (tasks all); empty ones none.
pub fn arrange(
    pinned: Vec<Entry>,
    own: Option<Vec<Part>>,
    mut known: impl FnMut(Known) -> Vec<Entry>,
    max: usize,
) -> Vec<Group> {
    let mut out = Vec::new();
    let mut tasks = Vec::new();
    for part in own.unwrap_or_else(|| vec![Part::Known(Known::Recent)]) {
        let group = match part {
            Part::Named(name, entries) => Group {
                heading: Heading::Named(name),
                entries: entries.into_iter().take(max).collect(),
            },
            Part::Known(k) => Group {
                heading: match k {
                    Known::Frequent => Heading::Frequent,
                    Known::Recent => Heading::Recent,
                },
                entries: known(k)
                    .into_iter()
                    .filter(|e| !pinned.iter().any(|p| p.launch == e.launch))
                    .take(max)
                    .collect(),
            },
            Part::Tasks(entries) => {
                tasks.extend(entries);
                continue;
            }
        };
        out.push(group);
    }
    let mut groups = vec![Group {
        heading: Heading::Pinned,
        entries: pinned,
    }];
    groups.extend(out);
    groups.push(Group {
        heading: Heading::Tasks,
        entries: tasks,
    });
    groups.retain(|g| !g.entries.is_empty());
    groups
}

#[cfg(test)]
mod tests {
    //! Edge cases: the file name of known app ids (Windows' own names, one without a
    //! leading zero); every kind of category, a link's every optional part, a missing
    //! footer, a cut file; old and new `DestList` entries, pinned order.
    use super::*;

    #[test]
    fn jump_list_files_are_named_as_windows_names_them() {
        // Found on Windows: File Explorer's and Chrome's lists.
        assert_eq!(file_stem("Microsoft.Windows.Explorer"), "f01b4d95cf55d32a");
        assert_eq!(file_stem("Chrome"), "5d696d521de238c3");
        // Upper case first: the same file for any case.
        assert_eq!(file_stem("chrome"), file_stem("CHROME"));
        assert!(!file_stem("x").starts_with('0') || file_stem("x") == "0");
    }

    /// A shell link of `flags` with `strings` (UTF-16) and one extra data block.
    fn link(flags: u32, strings: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; LINK_HEADER];
        b[0..4].copy_from_slice(&(LINK_HEADER as u32).to_le_bytes());
        b[LINK_FLAGS_AT..LINK_FLAGS_AT + 4].copy_from_slice(&flags.to_le_bytes());
        if flags & HAS_ID_LIST != 0 {
            b.extend_from_slice(&3u16.to_le_bytes());
            b.extend_from_slice(&[9, 9, 9]);
        }
        if flags & HAS_LINK_INFO != 0 {
            b.extend_from_slice(&8u32.to_le_bytes());
            b.extend_from_slice(&[7, 7, 7, 7]);
        }
        for s in strings {
            let u: Vec<u16> = s.encode_utf16().collect();
            b.extend_from_slice(&(u.len() as u16).to_le_bytes());
            for c in u {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        b.extend_from_slice(&10u32.to_le_bytes());
        b.extend_from_slice(&[5; 6]);
        b.extend_from_slice(&0u32.to_le_bytes());
        b
    }

    fn entry(l: &[u8]) -> Vec<u8> {
        let mut b = SHELL_LINK_CLASS.to_vec();
        b.extend_from_slice(l);
        b
    }

    fn u32s(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn a_programs_own_list_is_read_in_its_order() {
        let task = link(IS_UNICODE | 0x20 | 0x40, &["--incognito", r"C:\chrome.exe"]);
        let site = link(IS_UNICODE | HAS_ID_LIST | HAS_LINK_INFO | 0x4, &["Kabuk"]);
        let mut b = u32s(&[CUSTOM_VERSION, 3, 0]);
        // A named category with one link, its footer.
        b.extend(u32s(&[CATEGORY_CUSTOM]));
        let name: Vec<u16> = "Most visited".encode_utf16().collect();
        b.extend_from_slice(&(name.len() as u16).to_le_bytes());
        b.extend(name.iter().flat_map(|c| c.to_le_bytes()));
        b.extend(u32s(&[1]));
        let site_at = b.len() + 16;
        b.extend(entry(&site));
        b.extend(u32s(&[FOOTER]));
        // Windows' recent files, no footer.
        b.extend(u32s(&[CATEGORY_KNOWN, KNOWN_RECENT]));
        // Two tasks.
        b.extend(u32s(&[CATEGORY_TASKS, 2]));
        let task_at = b.len() + 16;
        b.extend(entry(&task));
        b.extend(entry(&task));
        b.extend(u32s(&[FOOTER]));
        let got = custom_destinations(&b).expect("read");
        assert_eq!(
            got,
            vec![
                Category::Custom {
                    name: "Most visited".into(),
                    links: std::iter::once(site_at..site_at + site.len()).collect()
                },
                Category::Known(Known::Recent),
                Category::Tasks(vec![
                    task_at..task_at + task.len(),
                    task_at + task.len() + 16..task_at + 2 * task.len() + 16
                ]),
            ]
        );
        // Cut short, or another format: nothing rather than a wrong list.
        assert_eq!(custom_destinations(&b[..b.len() - 30]), None);
        assert_eq!(custom_destinations(&u32s(&[3, 0, 0])), None);
        let mut other = b.clone();
        other[site_at - 16] ^= 1;
        assert_eq!(custom_destinations(&other), None, "an entry that is not a link");
    }

    #[test]
    fn a_links_length_counts_its_ansi_strings_too() {
        let ansi = {
            let mut b = vec![0u8; LINK_HEADER];
            b[0..4].copy_from_slice(&(LINK_HEADER as u32).to_le_bytes());
            b[LINK_FLAGS_AT..LINK_FLAGS_AT + 4].copy_from_slice(&0x20u32.to_le_bytes());
            b.extend_from_slice(&3u16.to_le_bytes());
            b.extend_from_slice(b"abc");
            b.extend_from_slice(&0u32.to_le_bytes());
            b
        };
        assert_eq!(link_len(&ansi, 0), Some(ansi.len()));
        assert_eq!(link_len(&[0u8; 8], 0), None);
    }

    #[test]
    fn a_program_without_an_id_goes_by_its_folders_id_then_its_path() {
        let folders = [
            (r"C:\Windows".to_string(), IMPLICIT_FOLDERS[2]),
            (r"C:\Windows\System32\".to_string(), IMPLICIT_FOLDERS[0]),
        ];
        let ids = implicit_app_ids(r"C:\WINDOWS\system32\WindowsPowerShell\v1.0\powershell.exe", &folders);
        assert_eq!(
            ids,
            [
                r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe",
                r"C:\WINDOWS\system32\WindowsPowerShell\v1.0\powershell.exe"
            ]
        );
        // Found on Windows: Notepad's list, from the deepest folder's id.
        assert_eq!(
            file_stem(&implicit_app_ids(r"C:\Windows\System32\notepad.exe", &folders)[0]),
            "9b9cdc69c1c24e2b"
        );
        // Outside every folder: the path only; a folder's name as a prefix is not inside.
        assert_eq!(implicit_app_ids(r"D:\Tools\x.exe", &folders), [r"D:\Tools\x.exe"]);
        assert_eq!(
            implicit_app_ids(r"C:\WindowsApps\x.exe", &folders),
            [r"C:\WindowsApps\x.exe"]
        );
    }

    fn dest(version: u32, entries: &[(&str, i32)]) -> Vec<u8> {
        let mut b = u32s(&[version, entries.len() as u32]);
        b.resize(DEST_HEADER, 0);
        for (path, pin) in entries {
            let start = b.len();
            let path_at = if version == DEST_VERSION_OLD {
                DEST_PATH_AT_OLD
            } else {
                DEST_PATH_AT
            };
            b.resize(start + path_at, 0);
            b[start + DEST_PIN_AT..start + DEST_PIN_AT + 4].copy_from_slice(&pin.to_le_bytes());
            // Used: later for each entry, so the last one is the newest.
            let used = 1000u64 + start as u64;
            b[start + DEST_USED_AT..start + DEST_USED_AT + 8].copy_from_slice(&used.to_le_bytes());
            let u: Vec<u16> = path.encode_utf16().collect();
            b.extend_from_slice(&(u.len() as u16).to_le_bytes());
            b.extend(u.iter().flat_map(|c| c.to_le_bytes()));
            if version != DEST_VERSION_OLD {
                b.extend_from_slice(&[0; DEST_TAIL]);
            }
        }
        b
    }

    fn file(p: &str) -> Entry {
        Entry {
            title: p.into(),
            launch: Launch::File(p.into()),
        }
    }

    fn task(t: &str) -> Entry {
        Entry {
            title: t.into(),
            launch: Launch::Run {
                path: "chrome.exe".into(),
                args: t.into(),
                dir: String::new(),
            },
        }
    }

    #[test]
    fn the_menu_follows_windows_taskbar_order() {
        let recent = |k: Known| match k {
            Known::Recent => vec![file("a"), file("pinned"), file("b"), file("c")],
            Known::Frequent => vec![file("f")],
        };
        // Chrome: its categories, its tasks last though listed first.
        let own = vec![
            Part::Tasks(vec![task("--new-window"), task("--incognito")]),
            Part::Named("Most visited".into(), vec![file("x"), file("y"), file("z")]),
            Part::Known(Known::Frequent),
        ];
        let g = arrange(vec![file("pinned")], Some(own), recent, 2);
        let headings: Vec<Heading> = g.iter().map(|g| g.heading.clone()).collect();
        assert_eq!(
            headings,
            [
                Heading::Pinned,
                Heading::Named("Most visited".into()),
                Heading::Frequent,
                Heading::Tasks
            ]
        );
        assert_eq!(g[1].entries.len(), 2, "capped");
        assert_eq!(g[3].entries.len(), 2, "every task");
        // No list of its own: Windows' recent files, a pinned one only under "Pinned".
        let g = arrange(vec![file("pinned")], None, recent, 9);
        assert_eq!(g.len(), 2);
        assert_eq!(g[1].heading, Heading::Recent);
        assert_eq!(g[1].entries, vec![file("a"), file("b"), file("c")]);
        // Nothing at all: no group.
        assert!(arrange(Vec::new(), Some(Vec::new()), |_| Vec::new(), 9).is_empty());
    }

    #[test]
    fn pinned_entries_come_in_their_pinned_order() {
        for version in [DEST_VERSION_OLD, 4] {
            let b = dest(version, &[(r"C:\a.txt", -1), (r"C:\Work", 1), (r"C:\b.txt", 0)]);
            let list = dest_list(&b).expect("read");
            assert_eq!(list.len(), 3);
            assert_eq!(list[0].pin, None);
            assert_eq!(pinned(&list), vec![r"C:\b.txt".to_string(), r"C:\Work".to_string()]);
            assert_eq!(recent(&list), vec![r"C:\a.txt".to_string()], "the unpinned one");
            assert!(list[2].used > list[0].used);
            assert_eq!(dest_list(&b[..b.len() - 3]), None, "cut short");
        }
    }
}
