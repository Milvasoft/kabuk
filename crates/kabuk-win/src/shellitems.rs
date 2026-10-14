//! The shell's view of things for the launcher: the installed apps as the Start menu's
//! "All apps" lists them (`shell:AppsFolder`, desktop and Store apps alike), shortcut
//! targets, known folders, and opening a path, URL or app the way Explorer would.
//! COM must be initialised on the calling thread ([`crate::system::com_init`]).

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_LocalAppData, FOLDERID_Recent, FOLDERID_RoamingAppData,
    IEnumShellItems, IShellItem, IShellLinkW, SHGetKnownFolderItem, SHGetKnownFolderPath, ShellExecuteW, ShellLink,
    KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::sync::io_error;

/// An entry of the Start menu's "All apps".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// The shell's parsing name; `shell:AppsFolder\<id>` opens it. For a program it is
    /// the app id Windows groups its windows and keeps its jump list by.
    pub id: String,
    /// The program it starts, for a program's entry (not a packaged app's).
    pub target: Option<String>,
}

impl App {
    /// What [`open`] takes to start the app.
    pub fn target(&self) -> String {
        format!("shell:AppsFolder\\{}", self.id)
    }
}

/// Folders the launcher reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    /// The user's recent items (shortcuts).
    Recent,
    LocalAppData,
    /// `%APPDATA%` (the taskbar's pinned apps live under it).
    RoamingAppData,
    /// The user's desktop (it may have been moved, e.g. into OneDrive).
    Desktop,
}

/// Takes a shell-allocated string.
fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: `p` is a NUL-terminated string the shell allocated for us; freed once.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

pub fn known_folder(f: Folder) -> Option<PathBuf> {
    let id = match f {
        Folder::Recent => &FOLDERID_Recent,
        Folder::LocalAppData => &FOLDERID_LocalAppData,
        Folder::RoamingAppData => &FOLDERID_RoamingAppData,
        Folder::Desktop => &windows::Win32::UI::Shell::FOLDERID_Desktop,
    };
    known_folder_path(id)
}

/// The path of the known folder `id` (a `FOLDERID_…`); `None` for one this machine lacks.
pub fn known_folder_path(id: &windows::core::GUID) -> Option<PathBuf> {
    // SAFETY: a known folder id; the returned string is taken and freed by `take`.
    let p = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let s = take(p);
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// The installed apps, in the shell's order.
pub fn installed_apps() -> std::io::Result<Vec<App>> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath;
    use windows::Win32::UI::Shell::IShellItem2;
    // SAFETY: (whole block) COM calls on interfaces the shell returned; strings are
    // taken and freed by `take`; the enumerator is read until it reports no more items.
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(io_error)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems).map_err(io_error)?;
        let mut out = Vec::new();
        loop {
            let mut one = [None];
            let mut fetched = 0u32;
            if items.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = one[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(take).unwrap_or_default();
            let id = item
                .GetDisplayName(SIGDN_PARENTRELATIVEPARSING)
                .map(take)
                .unwrap_or_default();
            if !name.is_empty() && !id.is_empty() {
                let target = item
                    .cast::<IShellItem2>()
                    .ok()
                    .and_then(|i| i.GetString(&PKEY_Link_TargetParsingPath).ok())
                    .map(take)
                    .filter(|t| !t.is_empty());
                out.push(App { name, id, target });
            }
        }
        Ok(out)
    }
}

/// How long [`installed_apps_cached`] keeps the list before it reads it again (in the
/// background), and the list kept.
const INSTALLED_KEPT: std::time::Duration = std::time::Duration::from_secs(600);
type Installed = Option<(std::time::Instant, std::sync::Arc<Vec<App>>)>;
static INSTALLED: std::sync::Mutex<Installed> = std::sync::Mutex::new(None);
static INSTALLED_READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads the installed apps again on a thread of its own (once at a time).
pub fn refresh_installed_apps() {
    use std::sync::atomic::Ordering;
    if INSTALLED_READING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("installed-apps".into()).spawn(|| {
        crate::system::com_init();
        if let Ok(apps) = installed_apps() {
            if let Ok(mut c) = INSTALLED.lock() {
                *c = Some((std::time::Instant::now(), std::sync::Arc::new(apps)));
            }
        }
        INSTALLED_READING.store(false, Ordering::Release);
    });
    if spawned.is_err() {
        INSTALLED_READING.store(false, Ordering::Release);
    }
}

/// The installed apps as last read (reading them takes a few hundred milliseconds, too
/// long for a click): read here the first time, then again in the background when older
/// than [`INSTALLED_KEPT`]. COM must be initialised on this thread.
pub fn installed_apps_cached() -> std::sync::Arc<Vec<App>> {
    let kept = INSTALLED.lock().ok().and_then(|c| c.clone());
    match kept {
        Some((at, apps)) => {
            if at.elapsed() > INSTALLED_KEPT {
                refresh_installed_apps();
            }
            apps
        }
        None => {
            let apps = std::sync::Arc::new(installed_apps().unwrap_or_default());
            if let Ok(mut c) = INSTALLED.lock() {
                *c = Some((std::time::Instant::now(), apps.clone()));
            }
            apps
        }
    }
}

/// The Start menu's entry for the program at `exe`, if it has one.
pub fn program_entry(exe: &str) -> Option<App> {
    installed_apps_cached()
        .iter()
        .find(|a| a.target.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(exe)))
        .cloned()
}

/// The path a `.lnk` shortcut points at (environment variables left unexpanded only if
/// the shortcut stores them so). `None` for shortcuts to non-file items.
pub fn shortcut_target(lnk: &Path) -> Option<PathBuf> {
    // SAFETY: (whole block) a ShellLink object loaded from a file; the buffer is sized
    // for MAX_PATH-long paths as GetPath expects.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .ok()?;
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        (!s.is_empty()).then(|| PathBuf::from(s))
    }
}

/// The app id (`AppUserModelID`) a `.lnk` shortcut gives the program it starts, as a web
/// app's shortcut does; `None` when it gives none.
pub fn shortcut_app_id(lnk: &Path) -> Option<String> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    // SAFETY: (whole block) a ShellLink object loaded from a file and its property store,
    // read once; the string Windows allocates is freed after it is copied.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let store: IPropertyStore = link.cast().ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let p = PropVariantToStringAlloc(&value).ok()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        (!s.is_empty()).then_some(s)
    }
}

/// Writes a `.lnk` shortcut to `target` (tests make recent items with it).
pub fn write_shortcut(lnk: &Path, target: &Path) -> std::io::Result<()> {
    // SAFETY: (whole block) a ShellLink object saved to a file.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(io_error)?;
        link.SetPath(&HSTRING::from(target.as_os_str())).map_err(io_error)?;
        let file: IPersistFile = link.cast().map_err(io_error)?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(io_error)
    }
}

/// What [`pick`] asks the user for.
pub enum Pick<'a> {
    Folder,
    /// A file matching `patterns` (`*.png;*.jpg`), which the dialog names `kind`.
    File {
        kind: &'a str,
        patterns: &'a str,
    },
}

/// Asks the user for a folder or a file with Windows' own dialog, titled `title` and
/// owned by the calling thread's active window; `None` when cancelled.
pub fn pick(what: Pick, title: &str) -> Option<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    // SAFETY: (whole block) the shell's dialog object; the filter strings outlive the
    // call that copies them; the result's name is freed after it is copied.
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut options = dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM;
        match what {
            Pick::Folder => options |= FOS_PICKFOLDERS,
            Pick::File { kind, patterns } => {
                let (kind, patterns) = (HSTRING::from(kind), HSTRING::from(patterns));
                dialog
                    .SetFileTypes(&[COMDLG_FILTERSPEC {
                        pszName: PCWSTR(kind.as_ptr()),
                        pszSpec: PCWSTR(patterns.as_ptr()),
                    }])
                    .ok()?;
            }
        }
        dialog.SetOptions(options).ok()?;
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        let owner = GetActiveWindow();
        dialog.Show((!owner.is_invalid()).then_some(owner)).ok()?;
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
        path.map(PathBuf::from)
    }
}

/// Opens a path, URL or `shell:` name as Explorer would (the default program, the
/// browser, the app).
pub fn open(target: &str) -> std::io::Result<()> {
    // SAFETY: plain strings; ShellExecuteW returns a value above 32 on success.
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("ShellExecute failed ({})", r.0 as isize)))
    }
}

/// The id Windows groups a window's program by on the taskbar (`AppUserModelID`): the
/// one the window or its program sets, else a packaged app's own; `None` for a program
/// that sets none (Windows then makes one up that it does not give out).
pub fn window_app_id(window: crate::window::Hwnd) -> Option<String> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    // SAFETY: a property store of a live window, read once; the string Windows allocates
    // is freed after it is copied.
    let explicit = unsafe {
        SHGetPropertyStoreForWindow::<IPropertyStore>(window.raw())
            .ok()
            .and_then(|store| store.GetValue(&PKEY_AppUserModel_ID).ok())
            .and_then(|v| PropVariantToStringAlloc(&v).ok())
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            })
    };
    explicit.filter(|s| !s.is_empty()).or_else(|| {
        let process = crate::process::Process::open(window.pid()?).ok()?;
        process
            .app_user_model_id()
            .or_else(|| known_app_id(&process.image_path()?).map(str::to_string))
    })
}

/// The app id Windows itself gives a program that sets none, for the programs Kabuk
/// knows: File Explorer's windows carry none, but its jump list is Windows' own.
pub fn known_app_id(exe: &str) -> Option<&'static str> {
    /// (program file name, app id)
    const KNOWN: [(&str, &str); 1] = [("explorer.exe", "Microsoft.Windows.Explorer")];
    let name = std::path::Path::new(exe).file_name()?.to_str()?;
    KNOWN
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
}

/// Opens `file` with `program` (a recent file from an app's menu on the bar).
pub fn open_with(program: &str, file: &str) -> std::io::Result<()> {
    // SAFETY: plain strings; ShellExecuteW returns a value above 32 on success.
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(program),
            &HSTRING::from(format!("\"{file}\"")),
            None,
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
    //! Edge cases: the apps folder lists something with a name and an id, a known folder
    //! exists, a missing shortcut gives nothing, a shortcut made here resolves. Nothing
    //! is opened (that would start programs on the user's desktop).
    use super::*;

    #[test]
    fn file_explorer_goes_by_windows_own_app_id() {
        assert_eq!(
            known_app_id(r"C:\Windows\explorer.exe"),
            Some("Microsoft.Windows.Explorer")
        );
        assert_eq!(
            known_app_id(r"C:\WINDOWS\EXPLORER.EXE"),
            Some("Microsoft.Windows.Explorer")
        );
        assert_eq!(known_app_id(r"C:\Tools\explorer.exe.bak"), None);
        assert_eq!(known_app_id(r"C:\Windows\System32\notepad.exe"), None);
        assert_eq!(known_app_id(""), None);
    }

    #[test]
    fn no_window_has_no_app_id() {
        crate::system::com_init();
        assert_eq!(window_app_id(crate::window::Hwnd(0)), None);
    }

    #[test]
    fn installed_apps_have_names_and_ids() {
        crate::system::com_init();
        let apps = installed_apps().unwrap();
        assert!(!apps.is_empty());
        assert!(apps.iter().all(|a| !a.name.is_empty() && !a.id.is_empty()));
        assert!(apps[0].target().starts_with("shell:AppsFolder\\"));
    }

    #[test]
    fn known_folders_exist() {
        crate::system::com_init();
        assert!(known_folder(Folder::LocalAppData).is_some_and(|p| p.is_dir()));
        assert!(known_folder(Folder::Recent).is_some_and(|p| p.is_dir()));
        assert!(known_folder(Folder::RoamingAppData).is_some_and(|p| p.is_dir()));
    }

    #[test]
    fn shortcuts_resolve_to_their_target() {
        crate::system::com_init();
        assert_eq!(shortcut_target(Path::new("C:\\nope\\missing.lnk")), None);
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "x").unwrap();
        let lnk = dir.path().join("t.lnk");
        write_shortcut(&lnk, &target).unwrap();
        assert_eq!(shortcut_target(&lnk), Some(target));
    }
}
