//! Shell items' pictures (an app's icon takes 10-60 ms to load) loaded off the bar
//! thread: a worker loads what it is asked for, one at a time, and tells the control
//! window when some are done; they are drawn as they come. The launcher, the app grid
//! and the bar's apps each keep their own.
//!
//! A picture that cannot be loaded is tried again a few times, further apart each time
//! (found by the user: right after signing in, before the shell has its packaged apps
//! ready, some pinned apps' icons failed once and stayed the plain glyph for as long as
//! Kabuk ran).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use kabuk_ui::render::Images;
use kabuk_win::msgloop::{self, Msg};
use kabuk_win::window::Hwnd;

/// How long after each failed load a picture is tried again; after the last it is given
/// up until it is asked for anew. Together about nine minutes: a slow sign-in's shell.
const RETRIES: &[Duration] = &[
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
];

/// What the worker gives back: (generation, source, side in pixels, pixels); `None`
/// when it could not be loaded even after its retries.
type Loaded = Arc<Mutex<Vec<(u64, String, u32, Option<Vec<u8>>)>>>;

/// A load the worker was asked for: (generation, source, side in pixels).
type Job = (u64, String, u32);

/// Loaded pictures by source, the ones asked for, and the worker.
#[derive(Default)]
pub struct Pictures {
    /// The renderer's key of each loaded source's picture.
    pub loaded: HashMap<String, u64>,
    asked: HashSet<String>,
    /// Asked for and not back from the worker yet.
    pending: usize,
    /// To the worker.
    worker: Option<mpsc::Sender<Job>>,
    /// From the worker.
    done: Loaded,
    /// Bumped when everything is forgotten: the worker drops older loads still waiting
    /// for a retry, and what it gives back from before is not kept.
    generation: Arc<AtomicU64>,
    /// Keeps these pictures' keys apart from other owners'.
    owner: &'static str,
}

impl Pictures {
    /// Starts the worker for `owner` (also its thread's name); it posts `message` to
    /// `control` whenever some pictures are done. Without a worker nothing is loaded.
    pub fn start(owner: &'static str, control: Hwnd, message: u32) -> Pictures {
        Pictures::start_with(owner, control, message, RETRIES)
    }

    /// [`Pictures::start`] trying a failed load again after each of `retries`.
    fn start_with(owner: &'static str, control: Hwnd, message: u32, retries: &'static [Duration]) -> Pictures {
        let done = Loaded::default();
        let generation = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel::<Job>();
        let out = done.clone();
        let current = generation.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("{owner}-pictures"))
            .spawn(move || {
                kabuk_win::system::com_init();
                kabuk_win::system::background_thread();
                // Loads that failed: when they are tried again and how often they were.
                let mut waiting: Vec<(Instant, Job, usize)> = Vec::new();
                loop {
                    let due = waiting.iter().map(|w| w.0).min();
                    let fresh = match due {
                        None => match rx.recv() {
                            Ok(job) => Some(job),
                            Err(_) => break,
                        },
                        Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                            Ok(job) => Some(job),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                    };
                    let (job, tries) = match fresh {
                        Some(job) => (job, 0),
                        None => {
                            let Some(i) = waiting.iter().position(|w| Some(w.0) == due) else {
                                continue;
                            };
                            let (_, job, tries) = waiting.swap_remove(i);
                            (job, tries)
                        }
                    };
                    let (gen, source, side) = job;
                    if gen != current.load(Ordering::Relaxed) {
                        continue;
                    }
                    let px = kabuk_win::icon::shell_item_icon_pixels(&source, side)
                        .or_else(|| kabuk_win::icon::file_icon_pixels(std::path::Path::new(&source), side));
                    if px.is_none() {
                        if let Some(after) = retries.get(tries) {
                            tracing::debug!(owner, source = %source, tries, "picture not loaded; trying again");
                            waiting.push((Instant::now() + *after, (gen, source, side), tries + 1));
                            continue;
                        }
                    }
                    if let Ok(mut d) = out.lock() {
                        d.push((gen, source, side, px));
                    }
                    let _ = msgloop::post_message(
                        control,
                        Msg {
                            id: message,
                            wparam: 0,
                            lparam: 0,
                        },
                    );
                }
            });
        let worker = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                tracing::warn!(owner, error = %e, "pictures not loaded");
                None
            }
        };
        Pictures {
            loaded: HashMap::new(),
            asked: HashSet::new(),
            pending: 0,
            worker,
            done,
            generation,
            owner,
        }
    }

    /// The renderer's key of `source`'s picture at `side` pixels.
    pub fn key(&self, source: &str, side: u32) -> u64 {
        crate::bar::apps::image_key(&format!("{}|{source}|{side}", self.owner))
    }

    /// Forgets the pictures `store` let go of (a tray refresh keeps only its own), so
    /// they are asked for again.
    pub fn forget_dropped(&mut self, store: &Images) {
        let asked = &mut self.asked;
        self.loaded.retain(|source, key| {
            let kept = store.contains(*key);
            if !kept {
                asked.remove(source);
            }
            kept
        });
    }

    /// Forgets `source`'s picture, so it is asked for again (its program came back new).
    pub fn forget(&mut self, source: &str) {
        self.loaded.remove(source);
        self.asked.remove(source);
    }

    /// Forgets every picture, so each is asked for again (at a new side); loads still
    /// waiting for a retry are dropped.
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.loaded.clear();
        self.asked.clear();
        self.pending = 0;
    }

    /// Whether `source`'s picture is loaded.
    pub fn has(&self, source: &str) -> bool {
        self.loaded.contains_key(source)
    }

    /// Asks the worker for the `sources` not loaded or asked for yet, at `side` pixels.
    pub fn ask(&mut self, sources: impl IntoIterator<Item = String>, side: u32) {
        let Some(w) = &self.worker else { return };
        let gen = self.generation.load(Ordering::Relaxed);
        for source in sources {
            if self.loaded.contains_key(&source) || self.asked.contains(&source) {
                continue;
            }
            if w.send((gen, source.clone(), side)).is_ok() {
                self.asked.insert(source);
                self.pending += 1;
            }
        }
    }

    /// How many pictures asked for the worker has not given back yet.
    pub fn pending(&self) -> usize {
        self.pending
    }

    /// Puts the pictures the worker finished into `store`; how many came.
    pub fn arrived(&mut self, store: &Images) -> usize {
        let gen = self.generation.load(Ordering::Relaxed);
        let done: Vec<_> = self
            .done
            .lock()
            .map(|mut d| std::mem::take(&mut *d))
            .unwrap_or_default()
            .into_iter()
            // From before everything was forgotten: at an old side.
            .filter(|d| d.0 == gen)
            .collect();
        let count = done.len();
        self.pending = self.pending.saturating_sub(count);
        for (_, source, side, px) in done {
            match px {
                Some(px) => {
                    let key = self.key(&source, side);
                    store.set(key, side, px);
                    tracing::debug!(owner = self.owner, icon = %source, "picture loaded");
                    self.loaded.insert(source, key);
                }
                // Not loadable after its retries: it keeps its kind's icon until it is
                // asked for anew.
                None => {
                    tracing::info!(owner = self.owner, source = %source, "picture unavailable");
                    self.asked.remove(&source);
                }
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a picture forgotten is asked for again; all cleared (a new side) are
    //! asked for again; one already asked for is not asked twice; one that cannot be
    //! loaded at first is loaded once it can be; one never loadable is given up and can
    //! be asked for again.
    use super::*;

    fn wait(p: &mut Pictures, store: &Images) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while p.pending() > 0 && Instant::now() < deadline {
            msgloop::run_until(&[], Some(Duration::from_millis(50)));
            p.arrived(store);
        }
    }

    #[test]
    fn a_forgotten_picture_is_asked_for_again() {
        const MSG: u32 = msgloop::msg::APP + 904;
        let window = msgloop::MessageWindow::new("Kabuk.Test.Pictures", "", |m| (m.id == MSG).then_some(0)).unwrap();
        let store = Images::default();
        let mut p = Pictures::start("test", window.hwnd(), MSG);
        let source = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        p.ask([source.clone()], 32);
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 1, "asked once");
        wait(&mut p, &store);
        assert!(p.has(&source));
        assert!(store.contains(p.key(&source, 32)));
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 0, "loaded: not asked again");
        p.forget(&source);
        assert!(!p.has(&source));
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 1, "forgotten: asked again");
        wait(&mut p, &store);
        p.clear();
        p.ask([source.clone()], 48);
        assert_eq!(p.pending(), 1, "cleared: asked again at the new side");
        wait(&mut p, &store);
        assert!(store.contains(p.key(&source, 48)));
    }

    /// Found by the user: after signing in, some pinned apps' icons stayed the plain
    /// glyph, loaded once before the shell could give them.
    #[test]
    fn a_picture_not_loadable_at_first_is_loaded_once_it_can_be() {
        const MSG: u32 = msgloop::msg::APP + 905;
        const QUICK: &[Duration] = &[
            Duration::from_millis(100),
            Duration::from_millis(300),
            Duration::from_millis(600),
            Duration::from_secs(1),
        ];
        let window =
            msgloop::MessageWindow::new("Kabuk.Test.PicturesLater", "", |m| (m.id == MSG).then_some(0)).unwrap();
        let store = Images::default();
        let mut p = Pictures::start_with("test-later", window.hwnd(), MSG, QUICK);
        let dir = std::env::temp_dir().join(format!("kabuk-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("later.exe");
        let _ = std::fs::remove_file(&program);
        let source = program.to_string_lossy().into_owned();
        p.ask([source.clone()], 32);
        // Not there yet: the first load fails; it comes while the worker waits to retry.
        msgloop::run_until(&[], Some(Duration::from_millis(50)));
        std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
        wait(&mut p, &store);
        let loaded = p.has(&source) && store.contains(p.key(&source, 32));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(loaded, "loaded on a retry");
    }

    #[test]
    fn a_picture_never_loadable_is_given_up_and_can_be_asked_again() {
        const MSG: u32 = msgloop::msg::APP + 906;
        const QUICK: &[Duration] = &[Duration::from_millis(20), Duration::from_millis(20)];
        let window =
            msgloop::MessageWindow::new("Kabuk.Test.PicturesNever", "", |m| (m.id == MSG).then_some(0)).unwrap();
        let store = Images::default();
        let mut p = Pictures::start_with("test-never", window.hwnd(), MSG, QUICK);
        let source = std::env::temp_dir()
            .join(format!("kabuk-no-such-program-{}.exe", std::process::id()))
            .to_string_lossy()
            .into_owned();
        p.ask([source.clone()], 32);
        wait(&mut p, &store);
        assert_eq!(p.pending(), 0, "given up");
        assert!(!p.has(&source));
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 1, "asked for anew");
    }
}
