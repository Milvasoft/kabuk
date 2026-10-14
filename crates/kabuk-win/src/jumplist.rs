//! A program's jump list as Windows' taskbar shows it on a right click (the formats are
//! read by [`kabuk_core::jumplist`]): its pinned entries, its own categories and tasks,
//! and Windows' frequent or recent files for it. Read only; nothing here starts anything
//! (see [`run`]).

use std::path::{Path, PathBuf};

use kabuk_core::jumplist::{self, Entry, Group, Known, Launch, Part};
use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Storage::EnhancedStorage::{PKEY_AppUserModel_IsDestListSeparator, PKEY_Title};
use windows::Win32::System::Com::StructuredStorage::{
    CreateILockBytesOnHGlobal, IStorage, PropVariantToBoolean, PropVariantToStringAlloc, StgOpenStorageOnILockBytes,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, IPersistStream, IStream, CLSCTX_INPROC_SERVER, STATFLAG_NONAME, STGM_READ,
    STGM_SHARE_EXCLUSIVE,
};
use windows::Win32::UI::Shell::Common::IObjectArray;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    ApplicationDocumentLists, IApplicationDocumentLists, IShellItem, IShellLinkW, SHCreateItemFromParsingName,
    SHCreateMemStream, ShellExecuteW, ShellLink, ADLT_FREQUENT, ADLT_RECENT, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
    SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::shellitems::{known_folder, Folder};

/// Longest path, arguments or description a link holds that is read.
const TEXT: usize = 2048;

/// The program with app id `app_id`'s jump list, each group with at most `max` entries.
/// COM must be initialised on this thread.
pub fn jump_list(app_id: &str, max: usize) -> Vec<Group> {
    let Some(recent) = known_folder(Folder::Recent) else {
        return Vec::new();
    };
    let stem = jumplist::file_stem(app_id);
    let custom = recent
        .join(jumplist::CUSTOM_FOLDER)
        .join(format!("{stem}.{}", jumplist::CUSTOM_EXTENSION));
    let automatic = recent
        .join(jumplist::AUTOMATIC_FOLDER)
        .join(format!("{stem}.{}", jumplist::AUTOMATIC_EXTENSION));
    let pinned: Vec<Entry> = dest_list(&automatic)
        .map(|d| jumplist::pinned(&d))
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let (title, path) = shell_names(&p);
            Entry {
                title: title.unwrap_or_else(|| p.clone()),
                // A known folder as its path, so it opens and shows once.
                launch: Launch::File(path.unwrap_or(p)),
            }
        })
        .collect();
    let own = own_list(&custom);
    jumplist::arrange(pinned, own, |k| documents(app_id, k, max), max)
}

/// The program's own categories with their links read; `None` without a list of its own
/// (or one not understood).
fn own_list(file: &Path) -> Option<Vec<Part>> {
    let bytes = std::fs::read(file).ok()?;
    let categories = jumplist::custom_destinations(&bytes)?;
    let read = |ranges: &[std::ops::Range<usize>]| -> Vec<Entry> {
        ranges
            .iter()
            .filter_map(|r| link_entry(bytes.get(r.clone())?))
            .collect()
    };
    Some(
        categories
            .into_iter()
            .map(|c| match c {
                jumplist::Category::Custom { name, links } => Part::Named(name, read(&links)),
                jumplist::Category::Known(k) => Part::Known(k),
                jumplist::Category::Tasks(links) => Part::Tasks(read(&links)),
            })
            .collect(),
    )
}

/// Reads a string a COM getter writes into a buffer.
fn buffer_text(get: impl FnOnce(&mut [u16]) -> bool) -> String {
    let mut buf = vec![0u16; TEXT];
    if !get(&mut buf) {
        return String::new();
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// A serialised shell link as an entry: its title (the link's own, its description, or
/// its program's name) and what it runs. Separators and links to no file are left out.
fn link_entry(bytes: &[u8]) -> Option<Entry> {
    // SAFETY: (whole block) a memory stream over `bytes` (copied by Windows) loaded into a
    // ShellLink object, whose getters write into buffers of the size passed; strings
    // Windows allocates are freed after they are copied.
    unsafe {
        let stream: IStream = SHCreateMemStream(Some(bytes))?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistStream>().ok()?.Load(&stream).ok()?;
        let store: Option<IPropertyStore> = link.cast().ok();
        if let Some(s) = &store {
            if let Ok(v) = s.GetValue(&PKEY_AppUserModel_IsDestListSeparator) {
                if PropVariantToBoolean(&v).is_ok_and(|b| b.as_bool()) {
                    return None;
                }
            }
        }
        let path = buffer_text(|b| link.GetPath(b, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).is_ok());
        if path.is_empty() {
            return None;
        }
        let args = buffer_text(|b| link.GetArguments(b).is_ok());
        let dir = buffer_text(|b| link.GetWorkingDirectory(b).is_ok());
        let title = store
            .and_then(|s| s.GetValue(&PKEY_Title).ok())
            .and_then(|v| PropVariantToStringAlloc(&v).ok())
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            })
            .filter(|t| !t.is_empty())
            .or_else(|| Some(buffer_text(|b| link.GetDescription(b).is_ok())).filter(|d| !d.is_empty()))
            .unwrap_or_else(|| {
                Path::new(&path)
                    .file_stem()
                    .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned())
            });
        Some(Entry {
            title: resolve_resource(&title),
            launch: Launch::Run { path, args, dir },
        })
    }
}

/// A title given as a resource reference (`@C:\…\x.dll,-123`) in the user's words.
fn resolve_resource(title: &str) -> String {
    use windows::Win32::UI::Shell::SHLoadIndirectString;
    if !title.starts_with('@') {
        return title.to_string();
    }
    let mut buf = vec![0u16; TEXT];
    // SAFETY: a plain string in and a buffer of the size passed out.
    match unsafe { SHLoadIndirectString(&HSTRING::from(title), &mut buf, None) } {
        Ok(()) => {
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..len])
        }
        Err(_) => title.to_string(),
    }
}

/// The `DestList` of an automatic destinations file (a compound file, read whole first
/// so Windows can keep it open for writing meanwhile).
fn dest_list(file: &Path) -> Option<Vec<jumplist::Destination>> {
    let bytes = std::fs::read(file).ok()?;
    // SAFETY: (whole block) a lock-bytes object over a copy of `bytes`, opened read only
    // as a compound file; the stream is read into a buffer of its stated size.
    unsafe {
        let lock = CreateILockBytesOnHGlobal(None, true).ok()?;
        let mut written = 0u32;
        lock.WriteAt(
            0,
            bytes.as_ptr().cast(),
            u32::try_from(bytes.len()).ok()?,
            Some(&mut written),
        )
        .ok()?;
        let storage: IStorage =
            StgOpenStorageOnILockBytes(&lock, None, STGM_READ | STGM_SHARE_EXCLUSIVE, None, None).ok()?;
        let stream = storage
            .OpenStream(
                &HSTRING::from(jumplist::DEST_LIST_STREAM),
                None,
                STGM_READ | STGM_SHARE_EXCLUSIVE,
                0,
            )
            .ok()?;
        let mut stat = Default::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let size = usize::try_from(stat.cbSize).ok()?;
        let mut data = vec![0u8; size];
        let mut read = 0u32;
        stream
            .Read(data.as_mut_ptr().cast(), u32::try_from(size).ok()?, Some(&mut read))
            .ok()
            .ok()?;
        data.truncate(read as usize);
        jumplist::dest_list(&data)
    }
}

/// How the shell names a path or parsing name (a folder's own name, "This PC"), and its
/// path when it has one (`knownfolder:{…}` names one).
fn shell_names(path: &str) -> (Option<String>, Option<String>) {
    // SAFETY: a shell item for a parsing name; the strings Windows allocates are freed.
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path), None) else {
            return (None, None);
        };
        let name = |kind| {
            item.GetDisplayName(kind).ok().map(|p: PWSTR| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            })
        };
        (name(SIGDN_NORMALDISPLAY), name(SIGDN_FILESYSPATH))
    }
}

/// Windows' frequent or recent files of the program with app id `id`, newest (or most
/// used) first.
pub fn documents(id: &str, kind: Known, max: usize) -> Vec<Entry> {
    let list = match kind {
        Known::Frequent => ADLT_FREQUENT,
        Known::Recent => ADLT_RECENT,
    };
    // SAFETY: COM objects created and used here; each item's strings are freed after
    // they are copied.
    unsafe {
        let Ok(lists) =
            CoCreateInstance::<_, IApplicationDocumentLists>(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER)
        else {
            return Vec::new();
        };
        if lists.SetAppID(&HSTRING::from(id)).is_err() {
            return Vec::new();
        }
        let Ok(items) = lists.GetList::<IObjectArray>(list, u32::try_from(max).unwrap_or(u32::MAX)) else {
            return Vec::new();
        };
        let text = |item: &IShellItem, kind| {
            item.GetDisplayName(kind).ok().map(|p: PWSTR| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            })
        };
        (0..items.GetCount().unwrap_or(0))
            .filter_map(|i| items.GetAt::<IShellItem>(i).ok())
            .filter_map(|item| {
                Some(Entry {
                    launch: Launch::File(text(&item, SIGDN_FILESYSPATH)?),
                    title: text(&item, SIGDN_NORMALDISPLAY)?,
                })
            })
            .take(max)
            .collect()
    }
}

/// Runs a link's program with its arguments in its folder (a task, a site of Chrome's).
pub fn run(path: &str, args: &str, dir: &str) -> std::io::Result<()> {
    let expand = |s: &str| -> PathBuf { PathBuf::from(kabuk_core::paths::expand_env(s, |k| std::env::var(k).ok())) };
    let (path, dir) = (expand(path), expand(dir));
    let dir = (!dir.as_os_str().is_empty()).then(|| HSTRING::from(dir.as_os_str()));
    // SAFETY: plain strings; ShellExecuteW returns a value above 32 on success.
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(path.as_os_str()),
            &HSTRING::from(args),
            dir.as_ref()
                .map_or(windows::core::PCWSTR::null(), |d| windows::core::PCWSTR(d.as_ptr())),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("ShellExecute failed ({})", r.0 as isize)))
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases (nothing is started): a link saved by Windows' own ShellLink reads back
    //! as an entry; an unknown app id has no list; this machine's lists read without
    //! failing (File Explorer's has a `DestList` whenever its file exists).
    use super::*;

    #[test]
    fn a_link_saved_by_windows_reads_back_as_an_entry() {
        crate::system::com_init_mta();
        // SAFETY: a ShellLink object saved into a memory stream, read back whole.
        let bytes = unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(r"C:\Windows\System32\notepad.exe"))
                .unwrap();
            link.SetArguments(&HSTRING::from("--task")).unwrap();
            link.SetDescription(&HSTRING::from("A task")).unwrap();
            let stream: IStream = SHCreateMemStream(None).unwrap();
            link.cast::<IPersistStream>().unwrap().Save(&stream, true).unwrap();
            let mut stat = Default::default();
            stream.Stat(&mut stat, STATFLAG_NONAME).unwrap();
            stream
                .Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None)
                .unwrap();
            let mut b = vec![0u8; stat.cbSize as usize];
            let mut read = 0;
            stream
                .Read(b.as_mut_ptr().cast(), b.len() as u32, Some(&mut read))
                .ok()
                .unwrap();
            b
        };
        // The pure reader measures it as Windows wrote it.
        let mut file = 2u32.to_le_bytes().to_vec();
        file.extend(1u32.to_le_bytes());
        file.extend(0u32.to_le_bytes());
        file.extend(2u32.to_le_bytes());
        file.extend(1u32.to_le_bytes());
        file.extend([0x01, 0x14, 0x02, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
        let at = file.len();
        file.extend(&bytes);
        let parts = jumplist::custom_destinations(&file).expect("measured");
        assert_eq!(
            parts,
            vec![jumplist::Category::Tasks(
                std::iter::once(at..at + bytes.len()).collect()
            )]
        );
        let e = link_entry(&bytes).expect("an entry");
        assert_eq!(e.title, "A task");
        assert_eq!(
            e.launch,
            Launch::Run {
                path: r"C:\Windows\System32\notepad.exe".into(),
                args: "--task".into(),
                dir: String::new()
            }
        );
    }

    #[test]
    fn this_machines_lists_read_without_failing() {
        crate::system::com_init_mta();
        assert!(jump_list("Kabuk.Test.NoSuchApp", 5).is_empty());
        let explorer = known_folder(Folder::Recent).map(|r| {
            r.join(jumplist::AUTOMATIC_FOLDER).join(format!(
                "{}.{}",
                jumplist::file_stem("Microsoft.Windows.Explorer"),
                jumplist::AUTOMATIC_EXTENSION
            ))
        });
        if let Some(f) = explorer.filter(|f| f.exists()) {
            assert!(dest_list(&f).is_some(), "File Explorer's DestList read");
        }
        let _ = jump_list("Microsoft.Windows.Explorer", 5);
        let _ = jump_list("Chrome", 5);
    }
}
