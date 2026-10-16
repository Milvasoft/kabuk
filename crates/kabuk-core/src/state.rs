//! `state.json`: everything Kabuk changed in Windows that must be undone after a
//! crash. Each change is written here *before* it is made (CONTRIBUTING.md safety rule 3),
//! so the watchdog or `kabuk --restore` can always put the desktop back. The records that
//! outlast Windows stopping (the user's settings) also go to `state.lasting.json`, and
//! only their changes wait for the disk.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const STATE_VERSION: u32 = 1;
pub const STATE_FILE_NAME: &str = "state.json";
/// A copy of the state file's lasting records (everything but the windows'), flushed
/// to disk with each change of theirs: read when Windows stopped before it wrote the
/// state file out ([`StateFile::read_lasting`]).
pub const LASTING_FILE_NAME: &str = "state.lasting.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// A window Kabuk changed (hid, or gave a coloured border). `pid` and `class` guard
/// against HWND reuse: a restore only touches the window if both still match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenWindow {
    pub hwnd: u64,
    pub pid: u32,
    pub class: String,
    /// For logs only.
    #[serde(default)]
    pub title: String,
}

/// The taskbar's state before Kabuk hid it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskbarBackup {
    /// The taskbar's own auto-hide setting.
    pub autohide: bool,
}

/// The user's mouse cursors before Kabuk set its own (DECISIONS D20): every value of
/// `HKCU\Control Panel\Cursors` it writes (`""` is the scheme's name), `None` where the
/// value was not set, and the scheme source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorBackup {
    pub values: Vec<(String, Option<String>)>,
    pub scheme_source: Option<u32>,
    /// Windows' pointer size (`CursorBaseSize`) as it was, once it was recorded (D41);
    /// a backup from before sizes were chosen has not recorded it and leaves it alone.
    #[serde(default)]
    pub base_size_recorded: bool,
    #[serde(default)]
    pub base_size: Option<u32>,
}

impl CursorBackup {
    /// What the pointer size goes back to: `None` to leave it, `Some(None)` to remove it.
    pub fn base_size_to_restore(&self) -> Option<Option<u32>> {
        self.base_size_recorded.then_some(self.base_size)
    }
}

/// A registry value under `HKCU` as it was before Kabuk wrote it: its key, its name and
/// its data (`None`: it was not set).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryValue {
    pub key: String,
    pub name: String,
    pub data: Option<RegistryData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistryData {
    Dword(u32),
    Binary(Vec<u8>),
    /// Text, `REG_EXPAND_SZ` when `expand`.
    Text {
        value: String,
        expand: bool,
    },
}

/// The screen saver settings before Kabuk set its own (SPEC §2.7, DECISIONS D12): the
/// values of `HKCU\Control Panel\Desktop` it writes (`None` where not set) and
/// OledSaver's theme.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaverBackup {
    pub values: Vec<(String, Option<String>)>,
    pub theme: Option<u32>,
}

/// The wallpaper before Kabuk set its own (SPEC §2.8): each monitor's picture by
/// Windows' monitor id (`None`: no picture), the position and the background colour.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperBackup {
    pub monitors: Vec<(String, Option<String>)>,
    pub position: i32,
    pub color: u32,
}

/// A monitor's work area before Kabuk changed it, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAreaBackup {
    pub monitor: Rect,
    pub work_area: Rect,
}

impl Rect {
    pub fn contains(&self, inner: &Rect) -> bool {
        inner.left >= self.left && inner.top >= self.top && inner.right <= self.right && inner.bottom <= self.bottom
    }

    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

impl WorkAreaBackup {
    /// Safe to re-apply only when its monitor still exists with the same bounds and
    /// the work area is a non-empty part of it. Otherwise the monitor layout changed
    /// (or the record is wrong) and Explorer recomputes the work area by itself.
    pub fn applies_to(&self, monitors: &[Rect]) -> bool {
        monitors.contains(&self.monitor) && !self.work_area.is_empty() && self.monitor.contains(&self.work_area)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    /// The running shell, so `--restore` can stop it first.
    pub kabuk_pid: Option<u32>,
    pub hidden_windows: Vec<HiddenWindow>,
    /// Windows whose DWM border colour Kabuk set (the active-window frame).
    pub bordered_windows: Vec<HiddenWindow>,
    pub taskbar: Option<TaskbarBackup>,
    pub work_areas: Vec<WorkAreaBackup>,
    pub cursors: Option<CursorBackup>,
    /// Windows whose title bar colours and corners Kabuk set (DECISIONS D30).
    pub styled_windows: Vec<HiddenWindow>,
    pub saver: Option<SaverBackup>,
    pub wallpaper: Option<WallpaperBackup>,
    /// Windows whose own minimise and restore transitions Kabuk turned off for the genie.
    pub genie_windows: Vec<HiddenWindow>,
    /// Windows' mode and accent colour before the system theme's colour layer set them
    /// (SPEC §2.10, DECISIONS D42).
    pub system_colors: Option<Vec<RegistryValue>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: STATE_VERSION,
            kabuk_pid: None,
            hidden_windows: Vec::new(),
            bordered_windows: Vec::new(),
            taskbar: None,
            work_areas: Vec::new(),
            cursors: None,
            styled_windows: Vec::new(),
            saver: None,
            wallpaper: None,
            genie_windows: Vec::new(),
            system_colors: None,
        }
    }
}

impl State {
    /// True when something must be undone.
    pub fn has_changes(&self) -> bool {
        !self.hidden_windows.is_empty()
            || !self.bordered_windows.is_empty()
            || self.taskbar.is_some()
            || !self.work_areas.is_empty()
            || self.cursors.is_some()
            || !self.styled_windows.is_empty()
            || self.saver.is_some()
            || self.wallpaper.is_some()
            || !self.genie_windows.is_empty()
            || self.system_colors.is_some()
    }

    /// The records that outlast Windows stopping: all but the windows' and the shell's
    /// pid, which mean nothing once it starts again.
    fn lasting(&self) -> State {
        State {
            kabuk_pid: None,
            hidden_windows: Vec::new(),
            bordered_windows: Vec::new(),
            styled_windows: Vec::new(),
            genie_windows: Vec::new(),
            ..self.clone()
        }
    }

    /// Forgets every change (after they were undone).
    pub fn clear_changes(&mut self) {
        self.clear_changes_except(Unrestored::default());
    }

    /// Forgets the changes that were undone. The user's settings that could not be given
    /// back stay recorded, so the next restore gives them back (and Kabuk's next start
    /// keeps them as the user's instead of recording its own in their place).
    pub fn clear_changes_except(&mut self, kept: Unrestored) {
        self.hidden_windows.clear();
        self.bordered_windows.clear();
        self.taskbar = None;
        self.work_areas.clear();
        self.styled_windows.clear();
        self.genie_windows.clear();
        if !kept.cursors {
            self.cursors = None;
        }
        if !kept.saver {
            self.saver = None;
        }
        if !kept.wallpaper {
            self.wallpaper = None;
        }
        if !kept.system_colors {
            self.system_colors = None;
        }
    }
}

/// Which of the user's recorded settings an undo could not give back (Windows refuses
/// some while the machine saves power or is locked).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unrestored {
    pub cursors: bool,
    pub saver: bool,
    pub wallpaper: bool,
    pub system_colors: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a valid state file: {detail}")]
    Corrupt { path: PathBuf, detail: String },
}

/// The state file on disk. Every mutation is saved before it returns.
#[derive(Debug)]
pub struct StateFile {
    path: PathBuf,
    state: State,
    /// The lasting records as their copy ([`LASTING_FILE_NAME`]) holds them, flushed:
    /// read when the file is opened, then as last written.
    lasting_saved: Option<State>,
}

impl StateFile {
    pub fn path_in(state_dir: &Path) -> PathBuf {
        state_dir.join(STATE_FILE_NAME)
    }

    /// Reads the file; a missing file is an empty state.
    pub fn read(path: &Path) -> Result<State, StateError> {
        read_json(path).map(Option::unwrap_or_default)
    }

    /// The copy of the lasting records kept beside the state file at `path` (`None`:
    /// there is none). What is left to undo when the state file is unreadable because
    /// Windows stopped (a power cut) before it wrote the file out.
    pub fn read_lasting(path: &Path) -> Result<Option<State>, StateError> {
        read_json(&lasting_path(path))
    }

    pub fn open(path: &Path) -> Result<StateFile, StateError> {
        Ok(StateFile {
            path: path.to_path_buf(),
            state: StateFile::read(path)?,
            lasting_saved: StateFile::read_lasting(path).ok().flatten(),
        })
    }

    /// Starts from an empty state, ignoring what the file holds (used after a corrupt
    /// file was reported and the desktop restored as far as possible).
    pub fn fresh(path: &Path) -> StateFile {
        StateFile {
            path: path.to_path_buf(),
            state: State::default(),
            lasting_saved: StateFile::read_lasting(path).ok().flatten(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Applies `change` and saves. If saving fails the in-memory state is rolled back,
    /// so the caller must not go on to make the Windows change.
    pub fn update(&mut self, change: impl FnOnce(&mut State)) -> Result<(), StateError> {
        let before = self.state.clone();
        change(&mut self.state);
        if let Err(e) = self.save() {
            self.state = before;
            return Err(e);
        }
        Ok(())
    }

    /// Records a window about to be hidden. Call before hiding it.
    pub fn add_hidden(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        self.add_hidden_all(vec![window])
    }

    /// Records windows about to be hidden, with one write (a workspace switch hides many:
    /// one write each made it slow on a slow disk, found in the test VM). Call before
    /// hiding them.
    pub fn add_hidden_all(&mut self, windows: Vec<HiddenWindow>) -> Result<(), StateError> {
        if windows.is_empty() {
            return Ok(());
        }
        self.update(|s| {
            for window in windows {
                s.hidden_windows.retain(|w| w.hwnd != window.hwnd);
                s.hidden_windows.push(window);
            }
        })
    }

    /// Forgets a window after it was shown again.
    pub fn remove_hidden(&mut self, hwnd: u64) -> Result<(), StateError> {
        self.remove_hidden_all(&[hwnd])
    }

    /// Forgets windows after they were shown again, with one write (none when none of
    /// them was recorded).
    pub fn remove_hidden_all(&mut self, hwnds: &[u64]) -> Result<(), StateError> {
        if !self.state.hidden_windows.iter().any(|w| hwnds.contains(&w.hwnd)) {
            return Ok(());
        }
        self.update(|s| s.hidden_windows.retain(|w| !hwnds.contains(&w.hwnd)))
    }

    /// Records a window about to get a coloured border. Call before colouring it.
    pub fn add_bordered(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        if self.state.bordered_windows.iter().any(|w| w.hwnd == window.hwnd) {
            return Ok(());
        }
        self.update(|s| s.bordered_windows.push(window))
    }

    /// Forgets a window after its border went back to the default.
    /// Records a window before its frame is styled (DECISIONS D30); once per window.
    pub fn add_styled(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        if self.state.styled_windows.iter().any(|w| w.hwnd == window.hwnd) {
            return Ok(());
        }
        self.update(|s| s.styled_windows.push(window))
    }

    pub fn remove_styled(&mut self, hwnd: u64) -> Result<(), StateError> {
        if !self.state.styled_windows.iter().any(|w| w.hwnd == hwnd) {
            return Ok(());
        }
        self.update(|s| s.styled_windows.retain(|w| w.hwnd != hwnd))
    }

    /// Records a window before its own transitions are turned off (the genie); once per
    /// window.
    pub fn add_genie(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        if self.state.genie_windows.iter().any(|w| w.hwnd == window.hwnd) {
            return Ok(());
        }
        self.update(|s| s.genie_windows.push(window))
    }

    pub fn remove_genie(&mut self, hwnd: u64) -> Result<(), StateError> {
        if !self.state.genie_windows.iter().any(|w| w.hwnd == hwnd) {
            return Ok(());
        }
        self.update(|s| s.genie_windows.retain(|w| w.hwnd != hwnd))
    }

    pub fn remove_bordered(&mut self, hwnd: u64) -> Result<(), StateError> {
        if !self.state.bordered_windows.iter().any(|w| w.hwnd == hwnd) {
            return Ok(());
        }
        self.update(|s| s.bordered_windows.retain(|w| w.hwnd != hwnd))
    }

    /// Writes the file. Only a change of the lasting records waits for the disk, and
    /// writes their copy too; the window records are written without (found in the test
    /// VM, where each flush took 20 to 90 ms: a workspace switch, recording the windows
    /// it hid and the frame it moved, took over 100 ms). Every program reads them at
    /// once and they stay after Kabuk ends in any way; only Windows stopping can lose
    /// them, and with them the windows they are about.
    fn save(&mut self) -> Result<(), StateError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| StateError::Io { path, source }
        };
        // Serializing this plain data cannot fail.
        let json = serde_json::to_vec_pretty(&self.state).unwrap_or_default();
        let lasting = self.state.lasting();
        if self.lasting_saved.as_ref() == Some(&lasting) {
            return crate::fsutil::write_atomic_cached(&self.path, &json).map_err(io(&self.path));
        }
        // The state file first: those reading it (the shell's pid) wait no longer than
        // before the copy was kept.
        crate::fsutil::write_atomic(&self.path, &json).map_err(io(&self.path))?;
        let copy = lasting_path(&self.path);
        let lasting_json = serde_json::to_vec_pretty(&lasting).unwrap_or_default();
        crate::fsutil::write_atomic(&copy, &lasting_json).map_err(io(&copy))?;
        self.lasting_saved = Some(lasting);
        Ok(())
    }
}

/// The copy of the lasting records next to the state file at `path`.
fn lasting_path(path: &Path) -> PathBuf {
    path.with_file_name(LASTING_FILE_NAME)
}

/// A state file's content; `None` when there is no file.
fn read_json(path: &Path) -> Result<Option<State>, StateError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StateError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    serde_json::from_str(&text).map(Some).map_err(|e| StateError::Corrupt {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    //! Edge cases: missing file, empty object, unknown future fields, corrupt JSON,
    //! save-before-change ordering, duplicate HWND, removing an unknown HWND,
    //! rollback when the save fails, clearing changes, has_changes for each kind, window
    //! records leaving the lasting copy alone, a zero-filled file read from the copy, a
    //! corrupt or missing copy.
    use super::*;

    fn window(hwnd: u64) -> HiddenWindow {
        HiddenWindow {
            hwnd,
            pid: 42,
            class: "KabukTestWindow".into(),
            title: "t".into(),
        }
    }

    #[test]
    fn missing_file_is_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let s = StateFile::read(&dir.path().join("state.json")).unwrap();
        assert_eq!(s, State::default());
        assert!(!s.has_changes());
    }

    #[test]
    fn tolerant_of_missing_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        std::fs::write(
            &p,
            r#"{"hidden_windows":[{"hwnd":7,"pid":1,"class":"C"}],"future":true}"#,
        )
        .unwrap();
        let s = StateFile::read(&p).unwrap();
        assert_eq!(s.hidden_windows[0].hwnd, 7);
        assert_eq!(s.hidden_windows[0].title, "");
        assert_eq!(s.version, STATE_VERSION);
    }

    #[test]
    fn corrupt_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(matches!(StateFile::read(&p), Err(StateError::Corrupt { .. })));
    }

    #[test]
    fn hidden_window_is_on_disk_when_add_returns() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.add_hidden(window(1)).unwrap();
        f.add_hidden(window(2)).unwrap();
        f.add_hidden(window(1)).unwrap();
        let on_disk = StateFile::read(&p).unwrap();
        let hwnds: Vec<u64> = on_disk.hidden_windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(hwnds, vec![2, 1]);
        f.remove_hidden(2).unwrap();
        f.remove_hidden(99).unwrap();
        assert_eq!(StateFile::read(&p).unwrap().hidden_windows, vec![window(1)]);
    }

    /// Many windows hidden at once are on disk together, in one write; shown again,
    /// they go together; none at all writes nothing.
    #[test]
    fn windows_hidden_together_are_recorded_in_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.add_hidden_all(vec![window(1), window(2), window(3), window(2)])
            .unwrap();
        let hwnds = |p: &std::path::Path| -> Vec<u64> {
            StateFile::read(p)
                .unwrap()
                .hidden_windows
                .iter()
                .map(|w| w.hwnd)
                .collect()
        };
        assert_eq!(hwnds(&p), vec![1, 3, 2]);
        let written = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        f.add_hidden_all(Vec::new()).unwrap();
        f.remove_hidden_all(&[98, 99]).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().modified().unwrap(),
            written,
            "nothing to write"
        );
        f.remove_hidden_all(&[1, 2, 99]).unwrap();
        assert_eq!(hwnds(&p), vec![3]);
    }

    /// DECISIONS D20: the user's cursors are on disk before Kabuk sets its own, a value
    /// that was not set stays "not set", and clearing the changes forgets them.
    #[test]
    fn the_users_cursors_are_kept_until_restored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        let backup = CursorBackup {
            values: vec![
                (String::new(), Some("Windows Default".into())),
                ("Arrow".into(), Some(r"%SystemRoot%\cursors\aero_arrow.cur".into())),
                ("Pin".into(), None),
            ],
            scheme_source: Some(2),
            base_size_recorded: true,
            base_size: Some(48),
        };
        f.update(|s| s.cursors = Some(backup.clone())).unwrap();
        let on_disk = StateFile::read(&p).unwrap();
        assert_eq!(on_disk.cursors, Some(backup));
        assert!(on_disk.has_changes());
        let mut s = on_disk;
        s.clear_changes();
        assert!(s.cursors.is_none() && !s.has_changes());
    }

    /// D41: the pointer size goes back only when it was recorded; a backup written
    /// before sizes existed leaves it alone, and one that found it unset removes it.
    #[test]
    fn the_pointer_size_goes_back_only_when_recorded() {
        let old: CursorBackup = serde_json::from_str(r#"{"values":[],"scheme_source":1}"#).unwrap();
        assert_eq!(old.base_size_to_restore(), None);
        let unset = CursorBackup {
            base_size_recorded: true,
            ..old.clone()
        };
        assert_eq!(unset.base_size_to_restore(), Some(None));
        let set = CursorBackup {
            base_size: Some(32),
            ..unset
        };
        assert_eq!(set.base_size_to_restore(), Some(Some(32)));
    }

    #[test]
    fn genie_windows_are_recorded_once_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.add_genie(window(7)).unwrap();
        f.add_genie(window(7)).unwrap();
        let on_disk = StateFile::read(&p).unwrap();
        assert_eq!(on_disk.genie_windows, vec![window(7)]);
        assert!(on_disk.has_changes());
        f.remove_genie(7).unwrap();
        f.remove_genie(7).unwrap();
        assert!(!StateFile::read(&p).unwrap().has_changes());
        // A state file from before the genie reads without it.
        let old: State = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(old.genie_windows.is_empty());
    }

    #[test]
    fn styled_windows_are_recorded_once_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.add_styled(window(5)).unwrap();
        f.add_styled(window(5)).unwrap();
        let on_disk = StateFile::read(&p).unwrap();
        assert_eq!(on_disk.styled_windows, vec![window(5)]);
        assert!(on_disk.has_changes());
        f.remove_styled(5).unwrap();
        f.remove_styled(5).unwrap();
        assert!(!StateFile::read(&p).unwrap().has_changes());
    }

    #[test]
    fn bordered_windows_are_recorded_once_and_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.add_bordered(window(4)).unwrap();
        f.add_bordered(window(4)).unwrap();
        assert_eq!(StateFile::read(&p).unwrap().bordered_windows, vec![window(4)]);
        f.remove_bordered(4).unwrap();
        f.remove_bordered(4).unwrap();
        assert!(StateFile::read(&p).unwrap().bordered_windows.is_empty());
        // Files written before borders existed still load.
        std::fs::write(&p, r#"{"version":1,"hidden_windows":[]}"#).unwrap();
        assert!(StateFile::read(&p).unwrap().bordered_windows.is_empty());
    }

    /// Found in the test VM: every record waited for the disk (`FlushFileBuffers`, 20 to
    /// 90 ms there and more at times), so switching workspaces, which records the windows
    /// it hides and the frame of the one it focuses, took over 100 ms. Window records are
    /// only written now; the lasting records' copy, written and flushed with every change
    /// of theirs, is left alone by them.
    #[test]
    fn window_records_do_not_wait_for_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let copy = dir.path().join(LASTING_FILE_NAME);
        let mut f = StateFile::open(&p).unwrap();
        f.update(|s| s.taskbar = Some(TaskbarBackup { autohide: false }))
            .unwrap();
        let lasting = StateFile::read_lasting(&p).unwrap().unwrap();
        assert_eq!(lasting.taskbar, Some(TaskbarBackup { autohide: false }));
        std::fs::remove_file(&copy).unwrap();
        f.add_hidden_all(vec![window(1), window(2)]).unwrap();
        f.add_bordered(window(3)).unwrap();
        f.add_styled(window(3)).unwrap();
        f.add_genie(window(3)).unwrap();
        f.remove_hidden_all(&[1, 2]).unwrap();
        f.remove_bordered(3).unwrap();
        // The shell's pid means nothing after Windows starts again either.
        f.update(|s| s.kabuk_pid = Some(9)).unwrap();
        assert!(!copy.exists(), "a window record waited for the disk");
        let on_disk = StateFile::read(&p).unwrap();
        assert_eq!(on_disk.styled_windows, vec![window(3)]);
        assert_eq!(on_disk.taskbar, Some(TaskbarBackup { autohide: false }));
        assert_eq!(on_disk.kabuk_pid, Some(9));
        // A lasting record is flushed again, with its copy.
        f.update(|s| s.cursors = Some(CursorBackup::default())).unwrap();
        let lasting = StateFile::read_lasting(&p).unwrap().unwrap();
        assert!(lasting.cursors.is_some() && lasting.taskbar.is_some());
        assert_eq!(lasting.kabuk_pid, None);
        assert!(lasting.styled_windows.is_empty() && lasting.genie_windows.is_empty());
    }

    /// A file opened again (the restore at every start, the watchdog's) knows the copy
    /// already holds its lasting records: clearing the pid or the window records does
    /// not wait for the disk, which kept a starting shell's pid out of the file longer
    /// (`kabuk --restore` cannot stop a shell whose pid is not there yet).
    #[test]
    fn a_file_opened_again_knows_its_lasting_copy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let copy = dir.path().join(LASTING_FILE_NAME);
        let mut f = StateFile::open(&p).unwrap();
        f.update(|s| s.taskbar = Some(TaskbarBackup { autohide: true }))
            .unwrap();
        f.add_hidden(window(1)).unwrap();
        f.update(|s| s.kabuk_pid = Some(9)).unwrap();
        drop(f);
        let mut again = StateFile::open(&p).unwrap();
        std::fs::remove_file(&copy).unwrap();
        again.add_hidden(window(2)).unwrap();
        again.update(|s| s.kabuk_pid = None).unwrap();
        assert!(!copy.exists(), "the copy written again");
        let mut fresh = StateFile::fresh(&p);
        fresh.update(|s| s.kabuk_pid = Some(10)).unwrap();
        assert!(copy.exists(), "the copy no longer holds what a fresh file has");
        assert_eq!(StateFile::read_lasting(&p).unwrap().unwrap(), State::default());
        let mut later = StateFile::fresh(&p);
        std::fs::remove_file(&copy).unwrap();
        later.update(|s| s.kabuk_pid = Some(11)).unwrap();
        assert!(!copy.exists(), "a fresh file wrote the same copy again");
    }

    /// Windows stopping (a power cut) before it wrote an unflushed `state.json` out
    /// leaves the file's place filled with zeros: the user's settings recorded before
    /// are in their copy; the window records went with the windows.
    #[test]
    fn the_users_settings_outlast_a_state_file_windows_did_not_write_out() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        assert_eq!(StateFile::read_lasting(&p).unwrap(), None);
        let mut f = StateFile::open(&p).unwrap();
        let saver = SaverBackup {
            values: vec![("ScreenSaveActive".into(), Some("1".into()))],
            theme: Some(2),
        };
        f.update(|s| s.saver = Some(saver.clone())).unwrap();
        f.add_hidden(window(1)).unwrap();
        let len = std::fs::metadata(&p).unwrap().len();
        std::fs::write(&p, vec![0u8; usize::try_from(len).unwrap()]).unwrap();
        assert!(matches!(StateFile::read(&p), Err(StateError::Corrupt { .. })));
        let lasting = StateFile::read_lasting(&p).unwrap().unwrap();
        assert_eq!(lasting.saver, Some(saver));
        assert!(lasting.hidden_windows.is_empty());
        std::fs::write(dir.path().join(LASTING_FILE_NAME), "{ not json").unwrap();
        assert!(matches!(StateFile::read_lasting(&p), Err(StateError::Corrupt { .. })));
    }

    #[test]
    fn failed_save_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        // The state "file" is a directory, so every save fails.
        let p = dir.path().join("state.json");
        std::fs::create_dir(&p).unwrap();
        let mut f = StateFile::fresh(&p);
        assert!(f.add_hidden(window(1)).is_err());
        assert!(f.state().hidden_windows.is_empty());
    }

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn work_area_backup_applies_only_to_an_unchanged_monitor() {
        let monitors = [r(0, 0, 2560, 1440), r(-2560, 0, 0, 1440)];
        let ok = WorkAreaBackup {
            monitor: r(0, 0, 2560, 1440),
            work_area: r(0, 0, 2560, 1380),
        };
        assert!(ok.applies_to(&monitors));
        let negative = WorkAreaBackup {
            monitor: r(-2560, 0, 0, 1440),
            work_area: r(-2560, 40, 0, 1440),
        };
        assert!(negative.applies_to(&monitors));
        // Regression: a record in scaled (125 %) coordinates must not be applied as physical ones.
        let scaled = WorkAreaBackup {
            monitor: r(0, 0, 2048, 1152),
            work_area: r(0, 0, 2048, 1104),
        };
        assert!(!scaled.applies_to(&monitors));
        let outside = WorkAreaBackup {
            monitor: r(0, 0, 2560, 1440),
            work_area: r(0, 0, 2600, 1380),
        };
        assert!(!outside.applies_to(&monitors));
        let empty = WorkAreaBackup {
            monitor: r(0, 0, 2560, 1440),
            work_area: r(10, 10, 10, 1380),
        };
        assert!(!empty.applies_to(&monitors));
        assert!(!ok.applies_to(&[]));
    }

    #[test]
    fn has_changes_and_clear() {
        let mut s = State {
            kabuk_pid: Some(5),
            ..State::default()
        };
        assert!(!s.has_changes());
        s.taskbar = Some(TaskbarBackup { autohide: false });
        assert!(s.has_changes());
        s.taskbar = None;
        s.work_areas.push(WorkAreaBackup {
            monitor: Rect::default(),
            work_area: Rect::default(),
        });
        assert!(s.has_changes());
        s.hidden_windows.push(window(3));
        s.clear_changes();
        assert!(!s.has_changes());
        s.bordered_windows.push(window(6));
        assert!(s.has_changes());
        s.clear_changes();
        assert!(!s.has_changes());
        assert_eq!(s.kabuk_pid, Some(5));
    }

    #[test]
    fn settings_not_given_back_stay_recorded() {
        let saver = SaverBackup {
            values: vec![("SCRNSAVE.EXE".into(), Some("theirs.scr".into()))],
            theme: None,
        };
        let wallpaper = WallpaperBackup {
            monitors: vec![("m1".into(), None)],
            position: 0,
            color: 0,
        };
        let mut s = State {
            taskbar: Some(TaskbarBackup { autohide: false }),
            saver: Some(saver.clone()),
            wallpaper: Some(wallpaper),
            ..State::default()
        };
        s.hidden_windows.push(window(3));
        s.clear_changes_except(Unrestored {
            saver: true,
            ..Unrestored::default()
        });
        assert_eq!(s.saver, Some(saver), "the screen saver not given back");
        assert!(s.wallpaper.is_none() && s.taskbar.is_none() && s.hidden_windows.is_empty());
        assert!(s.has_changes());
    }

    /// D42: Windows' colours are kept until restored, an absent value too; a state file
    /// from before the layer reads without them.
    #[test]
    fn the_system_colours_are_kept_until_restored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        let values = vec![
            RegistryValue {
                key: "K".into(),
                name: "AppsUseLightTheme".into(),
                data: Some(RegistryData::Dword(0)),
            },
            RegistryValue {
                key: "K".into(),
                name: "AccentPalette".into(),
                data: Some(RegistryData::Binary(vec![1, 2, 3])),
            },
            RegistryValue {
                key: "K".into(),
                name: "Gone".into(),
                data: None,
            },
        ];
        f.update(|s| s.system_colors = Some(values.clone())).unwrap();
        let on_disk = StateFile::read(&p).unwrap();
        assert_eq!(on_disk.system_colors, Some(values));
        assert!(on_disk.has_changes());
        let mut s = on_disk;
        s.clear_changes_except(Unrestored {
            system_colors: true,
            ..Unrestored::default()
        });
        assert!(s.system_colors.is_some(), "kept when not given back");
        s.clear_changes();
        assert!(!s.has_changes());
        let old: State = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(old.system_colors.is_none());
    }
}
