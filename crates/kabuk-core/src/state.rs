//! `state.json`: everything Kabuk changed in Windows that must be undone after a
//! crash. Each change is written here *before* it is made (CONTRIBUTING.md safety rule 3),
//! so the watchdog or `kabuk --restore` can always put the desktop back. The records that
//! outlast Windows stopping (the user's settings) also go to `state.lasting.json`, and
//! only their changes wait for the disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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

    /// Records windows about to be hidden (a window recorded already moves to the end).
    pub fn add_hidden(&mut self, windows: impl IntoIterator<Item = HiddenWindow>) {
        for window in windows {
            self.hidden_windows.retain(|w| w.hwnd != window.hwnd);
            self.hidden_windows.push(window);
        }
    }

    /// Forgets windows shown again.
    pub fn remove_hidden(&mut self, hwnds: &[u64]) {
        self.hidden_windows.retain(|w| !hwnds.contains(&w.hwnd));
    }

    /// Records a window about to get a coloured border; once per window.
    pub fn add_bordered(&mut self, window: HiddenWindow) {
        add_once(&mut self.bordered_windows, window);
    }

    /// Forgets a window whose border went back to the default.
    pub fn remove_bordered(&mut self, hwnd: u64) {
        self.bordered_windows.retain(|w| w.hwnd != hwnd);
    }

    /// Records a window before its frame is styled (DECISIONS D30); once per window.
    pub fn add_styled(&mut self, window: HiddenWindow) {
        add_once(&mut self.styled_windows, window);
    }

    pub fn remove_styled(&mut self, hwnd: u64) {
        self.styled_windows.retain(|w| w.hwnd != hwnd);
    }

    /// Records a window before its own transitions are turned off (the genie); once per
    /// window.
    pub fn add_genie(&mut self, window: HiddenWindow) {
        add_once(&mut self.genie_windows, window);
    }

    pub fn remove_genie(&mut self, hwnd: u64) {
        self.genie_windows.retain(|w| w.hwnd != hwnd);
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

/// Adds `window` to `windows` unless its HWND is there already.
fn add_once(windows: &mut Vec<HiddenWindow>, window: HiddenWindow) {
    if !windows.iter().any(|w| w.hwnd == window.hwnd) {
        windows.push(window);
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

/// The state file on disk. Every mutation is saved before it returns; one that leaves the
/// records as they were writes nothing.
#[derive(Debug)]
pub struct StateFile {
    path: PathBuf,
    state: State,
    /// `state` serialised as last staged: a change that leaves it the same writes nothing.
    json: Vec<u8>,
    /// The number of the last change staged ([`StateFile::stage`]); the file holds it
    /// once `disk` says so. It starts unwritten: what the file holds is not known.
    seq: u64,
    /// The lasting records as their copy ([`LASTING_FILE_NAME`]) holds them, flushed:
    /// read when the file is opened, then as last staged.
    lasting_saved: Option<State>,
    /// The change that last changed the lasting records (0: their copy as it was read).
    lasting_seq: u64,
    disk: Arc<Disk>,
}

/// What a [`StateFile`]'s two files hold, shared with the writes it staged, which may
/// end in another order than they were staged in (a [`SharedStateFile`]'s).
#[derive(Debug, Default)]
struct Disk {
    /// Held while writing, so writes do not overlap.
    writing: Mutex<()>,
    /// The newest change the state file holds.
    state: AtomicU64,
    /// The newest change of the lasting records their copy holds.
    lasting: AtomicU64,
}

/// A change made in memory and serialised, to be written ([`Pending::write`]).
#[derive(Debug)]
struct Pending {
    seq: u64,
    path: PathBuf,
    json: Vec<u8>,
    /// The lasting records' copy and their change, while the copy may not hold them.
    lasting: Option<(u64, Vec<u8>)>,
    /// The records before this change, to go back to when the write fails; `None` when
    /// it changed nothing and only waits for an earlier change to be written.
    before: Option<Vec<u8>>,
    disk: Arc<Disk>,
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
        Ok(StateFile::with(path, StateFile::read(path)?))
    }

    /// Starts from an empty state, ignoring what the file holds (used after a corrupt
    /// file was reported and the desktop restored as far as possible).
    pub fn fresh(path: &Path) -> StateFile {
        StateFile::with(path, State::default())
    }

    fn with(path: &Path, state: State) -> StateFile {
        StateFile {
            path: path.to_path_buf(),
            json: to_json(&state),
            state,
            seq: 1,
            lasting_saved: StateFile::read_lasting(path).ok().flatten(),
            lasting_seq: 0,
            disk: Arc::default(),
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
        let Some(pending) = self.stage(change) else {
            return Ok(());
        };
        let written = pending.write();
        if written.is_err() {
            self.failed(pending);
        }
        written
    }

    /// Records a window about to be hidden. Call before hiding it.
    pub fn add_hidden(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        self.add_hidden_all(vec![window])
    }

    /// Records windows about to be hidden, with one write (a workspace switch hides many:
    /// one write each made it slow on a slow disk, found in the test VM). Call before
    /// hiding them.
    pub fn add_hidden_all(&mut self, windows: Vec<HiddenWindow>) -> Result<(), StateError> {
        self.update(|s| s.add_hidden(windows))
    }

    /// Forgets a window after it was shown again.
    pub fn remove_hidden(&mut self, hwnd: u64) -> Result<(), StateError> {
        self.remove_hidden_all(&[hwnd])
    }

    /// Forgets windows after they were shown again, with one write (none when none of
    /// them was recorded).
    pub fn remove_hidden_all(&mut self, hwnds: &[u64]) -> Result<(), StateError> {
        self.update(|s| s.remove_hidden(hwnds))
    }

    /// Records a window about to get a coloured border. Call before colouring it.
    pub fn add_bordered(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        self.update(|s| s.add_bordered(window))
    }

    /// Forgets a window after its border went back to the default.
    pub fn remove_bordered(&mut self, hwnd: u64) -> Result<(), StateError> {
        self.update(|s| s.remove_bordered(hwnd))
    }

    /// Records a window before its frame is styled (DECISIONS D30); once per window.
    pub fn add_styled(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        self.update(|s| s.add_styled(window))
    }

    pub fn remove_styled(&mut self, hwnd: u64) -> Result<(), StateError> {
        self.update(|s| s.remove_styled(hwnd))
    }

    /// Records a window before its own transitions are turned off (the genie); once per
    /// window.
    pub fn add_genie(&mut self, window: HiddenWindow) -> Result<(), StateError> {
        self.update(|s| s.add_genie(window))
    }

    pub fn remove_genie(&mut self, hwnd: u64) -> Result<(), StateError> {
        self.update(|s| s.remove_genie(hwnd))
    }

    /// Applies `change` in memory and serialises the records for [`Pending::write`];
    /// `None` when there is nothing to write: the records stayed the same and the files
    /// hold them already. A change that left them the same still waits for an earlier
    /// one not written yet, so whatever the caller saw recorded is on disk once it is.
    fn stage(&mut self, change: impl FnOnce(&mut State)) -> Option<Pending> {
        change(&mut self.state);
        let json = to_json(&self.state);
        let before = if json == self.json {
            None
        } else {
            self.seq += 1;
            let lasting = self.state.lasting();
            if self.lasting_saved.as_ref() != Some(&lasting) {
                self.lasting_saved = Some(lasting);
                self.lasting_seq = self.seq;
            }
            Some(std::mem::replace(&mut self.json, json))
        };
        let state_written = self.disk.state.load(Ordering::Acquire) >= self.seq;
        let lasting_written = self.disk.lasting.load(Ordering::Acquire) >= self.lasting_seq;
        if before.is_none() && state_written && lasting_written {
            return None;
        }
        let lasting = (!lasting_written)
            .then(|| self.lasting_saved.as_ref().map(|l| (self.lasting_seq, to_json(l))))
            .flatten();
        Some(Pending {
            seq: self.seq,
            path: self.path.clone(),
            json: self.json.clone(),
            lasting,
            before,
            disk: Arc::clone(&self.disk),
        })
    }

    /// Writing `pending` failed: its change is undone in memory, unless a later change
    /// was staged since (which holds it, and is written with it). Going back is a change
    /// of its own, written by the next one, whatever part of the files the failed write
    /// replaced.
    fn failed(&mut self, pending: Pending) {
        let Some(before) = pending.before else { return };
        if pending.seq != self.seq {
            return;
        }
        // Serialised from a `State` above, so it reads back.
        if let Ok(state) = serde_json::from_slice(&before) {
            self.state = state;
            self.json = before;
            self.seq += 1;
            self.lasting_saved = Some(self.state.lasting());
            self.lasting_seq = self.seq;
        }
    }
}

impl Pending {
    /// Writes the file, unless a later change was written already. Only a change of the
    /// lasting records waits for the disk, and writes their copy too; the window records
    /// are written without (found in the test VM, where each flush took 20 to 90 ms: a
    /// workspace switch, recording the windows it hid and the frame it moved, took over
    /// 100 ms). Every program reads them at once and they stay after Kabuk ends in any
    /// way; only Windows stopping can lose them, and with them the windows they are about.
    fn write(&self) -> Result<(), StateError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| StateError::Io { path, source }
        };
        let _writing = self.disk.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let lasting = self
            .lasting
            .as_ref()
            .filter(|(seq, _)| *seq > self.disk.lasting.load(Ordering::Acquire));
        if self.seq > self.disk.state.load(Ordering::Acquire) {
            // The state file first: those reading it (the shell's pid) wait no longer
            // than before the copy was kept.
            if lasting.is_some() {
                crate::fsutil::write_atomic(&self.path, &self.json).map_err(io(&self.path))?;
            } else {
                crate::fsutil::write_atomic_cached(&self.path, &self.json).map_err(io(&self.path))?;
            }
            self.disk.state.store(self.seq, Ordering::Release);
        }
        if let Some((seq, json)) = lasting {
            let copy = lasting_path(&self.path);
            crate::fsutil::write_atomic(&copy, json).map_err(io(&copy))?;
            self.disk.lasting.store(*seq, Ordering::Release);
        }
        Ok(())
    }
}

/// A [`StateFile`] shared by threads (the bar's and the window manager's). A change is
/// made and serialised under its lock but written after it, so a thread that only reads
/// the records, or records something else, does not wait for another's disk write; the
/// change is still on disk when [`SharedStateFile::update`] returns (CONTRIBUTING.md
/// safety rule 3).
#[derive(Debug)]
pub struct SharedStateFile {
    path: PathBuf,
    file: Mutex<StateFile>,
}

impl SharedStateFile {
    pub fn new(file: StateFile) -> SharedStateFile {
        SharedStateFile {
            path: file.path.clone(),
            file: Mutex::new(file),
        }
    }

    /// The state file's path, without waiting for the lock.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Locks the file; a panic elsewhere must not stop the safety net. A change made
    /// through the guard is written under the lock: [`SharedStateFile::update`] is not.
    pub fn lock(&self) -> MutexGuard<'_, StateFile> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`StateFile::update`], written after the lock is let go.
    pub fn update(&self, change: impl FnOnce(&mut State)) -> Result<(), StateError> {
        let pending = self.lock().stage(change);
        let Some(pending) = pending else {
            return Ok(());
        };
        let written = pending.write();
        if written.is_err() {
            self.lock().failed(pending);
        }
        written
    }
}

/// The records as the state file holds them. Serialising this plain data cannot fail.
fn to_json(state: &State) -> Vec<u8> {
    serde_json::to_vec_pretty(state).unwrap_or_default()
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
    //! corrupt or missing copy, nothing written when nothing changed, threads recording at
    //! once, the lock free during a write, writes ending out of order.
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
        let shared = SharedStateFile::new(f);
        assert!(shared.update(|s| s.add_hidden([window(2)])).is_err());
        assert!(shared.lock().state().hidden_windows.is_empty());
        // Once the disk takes it, the records as they went back are written, though the
        // next call changes nothing.
        std::fs::remove_dir(&p).unwrap();
        shared.update(|_| {}).unwrap();
        assert_eq!(StateFile::read(&p).unwrap(), State::default());
    }

    fn hidden_on_disk(p: &Path) -> Vec<u64> {
        StateFile::read(p)
            .unwrap()
            .hidden_windows
            .iter()
            .map(|w| w.hwnd)
            .collect()
    }

    /// M19: a call that leaves the records as they were writes nothing (the window
    /// manager wrote at each focus change, the bar each time it found its work area
    /// backup already there).
    #[test]
    fn an_unchanged_state_is_not_written_again() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        f.update(|s| s.kabuk_pid = Some(1)).unwrap();
        f.add_bordered(window(4)).unwrap();
        std::fs::remove_file(&p).unwrap();
        f.update(|s| s.kabuk_pid = Some(1)).unwrap();
        f.add_bordered(window(4)).unwrap();
        f.remove_styled(9).unwrap();
        f.update(|_| {}).unwrap();
        assert!(!p.exists(), "written although nothing changed");
        f.update(|s| s.kabuk_pid = Some(2)).unwrap();
        assert_eq!(StateFile::read(&p).unwrap().kabuk_pid, Some(2));
        let shared = SharedStateFile::new(f);
        std::fs::remove_file(&p).unwrap();
        shared.update(|s| s.kabuk_pid = Some(2)).unwrap();
        assert!(!p.exists(), "written although nothing changed");
        shared.update(|s| s.add_hidden([window(5)])).unwrap();
        assert_eq!(hidden_on_disk(&p), vec![5]);
    }

    /// Threads recording at once each find their record on disk when their call
    /// returns, whichever write ends last (CONTRIBUTING.md safety rule 3).
    #[test]
    fn a_shared_change_is_on_disk_when_its_call_returns() {
        const THREADS: u64 = 8;
        const EACH: u64 = 10;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let shared = Arc::new(SharedStateFile::new(StateFile::open(&p).unwrap()));
        let threads: Vec<_> = (1..=THREADS)
            .map(|t| {
                let shared = Arc::clone(&shared);
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..EACH {
                        let hwnd = t * 100 + i;
                        shared.update(|s| s.add_hidden([window(hwnd)])).unwrap();
                        assert!(hidden_on_disk(&p).contains(&hwnd), "{hwnd} not on disk");
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(hidden_on_disk(&p).len() as u64, THREADS * EACH);
    }

    /// M19: the file's lock is free while a change waits for the disk, so the bar
    /// reading the records does not wait for the window manager's write; the change is
    /// on disk only when its call returns.
    #[test]
    fn the_lock_is_free_while_a_change_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let shared = Arc::new(SharedStateFile::new(StateFile::open(&p).unwrap()));
        let disk = Arc::clone(&shared.lock().disk);
        let writing = disk.writing.lock().unwrap();
        let recorder = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || shared.update(|s| s.add_hidden([window(1)])))
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        // `try_lock`: with the lock held through the write, waiting for it would hang.
        while !shared
            .file
            .try_lock()
            .is_ok_and(|f| f.state().hidden_windows == vec![window(1)])
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the lock was held while waiting for the disk"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!p.exists(), "written while the disk was busy");
        assert!(!recorder.is_finished(), "returned before its change was written");
        drop(writing);
        recorder.join().unwrap().unwrap();
        assert_eq!(hidden_on_disk(&p), vec![1]);
    }

    /// Writes that end in another order than their changes were made leave the newest
    /// on disk; a call that changes nothing still writes an earlier change not written
    /// yet, so what it saw recorded is on disk when it returns.
    #[test]
    fn the_newest_change_stays_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut f = StateFile::open(&p).unwrap();
        let first = f.stage(|s| s.add_hidden([window(1)])).unwrap();
        let second = f.stage(|s| s.add_hidden([window(2)])).unwrap();
        second.write().unwrap();
        first.write().unwrap();
        assert_eq!(hidden_on_disk(&p), vec![1, 2]);
        let third = f.stage(|s| s.remove_hidden(&[1])).unwrap();
        let waiting = f.stage(|_| {}).expect("the earlier change is not on disk yet");
        waiting.write().unwrap();
        assert_eq!(hidden_on_disk(&p), vec![2]);
        third.write().unwrap();
        assert_eq!(hidden_on_disk(&p), vec![2]);
        assert!(f.stage(|_| {}).is_none(), "everything is on disk");
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
