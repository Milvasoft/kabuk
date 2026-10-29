//! Shell items' pictures (an app's icon takes 10-60 ms to load) loaded off the bar
//! thread: a worker loads what it is asked for, one at a time, and tells the control
//! window when some are done; they are drawn as they come. The launcher, the app grid
//! and the bar's apps each keep their own.

use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};

use kabuk_ui::render::Images;
use kabuk_win::msgloop::{self, Msg};
use kabuk_win::window::Hwnd;

/// What the worker gives back: (source, side in pixels, pixels); `None` when it could
/// not be loaded.
type Loaded = Arc<Mutex<Vec<(String, u32, Option<Vec<u8>>)>>>;

/// Loaded pictures by source, the ones asked for, and the worker.
#[derive(Default)]
pub struct Pictures {
    /// The renderer's key of each loaded source's picture.
    pub loaded: HashMap<String, u64>,
    asked: HashSet<String>,
    /// Asked for and not back from the worker yet.
    pending: usize,
    /// To the worker: (source, side in pixels).
    worker: Option<mpsc::Sender<(String, u32)>>,
    /// From the worker.
    done: Loaded,
    /// Keeps these pictures' keys apart from other owners'.
    owner: &'static str,
}

impl Pictures {
    /// Starts the worker for `owner` (also its thread's name); it posts `message` to
    /// `control` whenever some pictures are done. Without a worker nothing is loaded.
    pub fn start(owner: &'static str, control: Hwnd, message: u32) -> Pictures {
        let done = Loaded::default();
        let (tx, rx) = mpsc::channel::<(String, u32)>();
        let out = done.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("{owner}-pictures"))
            .spawn(move || {
                kabuk_win::system::com_init();
                while let Ok((source, side)) = rx.recv() {
                    let px = kabuk_win::icon::shell_item_icon_pixels(&source, side)
                        .or_else(|| kabuk_win::icon::file_icon_pixels(std::path::Path::new(&source), side));
                    if let Ok(mut d) = out.lock() {
                        d.push((source, side, px));
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

    /// Forgets every picture, so each is asked for again (at a new side).
    pub fn clear(&mut self) {
        self.loaded.clear();
        self.asked.clear();
    }

    /// Whether `source`'s picture is loaded.
    pub fn has(&self, source: &str) -> bool {
        self.loaded.contains_key(source)
    }

    /// Asks the worker for the `sources` not loaded or asked for yet, at `side` pixels.
    pub fn ask(&mut self, sources: impl IntoIterator<Item = String>, side: u32) {
        let Some(w) = &self.worker else { return };
        for source in sources {
            if self.loaded.contains_key(&source) || self.asked.contains(&source) {
                continue;
            }
            if w.send((source.clone(), side)).is_ok() {
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
        let done: Vec<_> = self
            .done
            .lock()
            .map(|mut d| std::mem::take(&mut *d))
            .unwrap_or_default();
        let count = done.len();
        self.pending = self.pending.saturating_sub(count);
        for (source, side, px) in done {
            match px {
                Some(px) => {
                    let key = self.key(&source, side);
                    store.set(key, side, px);
                    self.loaded.insert(source, key);
                }
                // Not loadable: it keeps its kind's icon and is not asked again.
                None => tracing::debug!(owner = self.owner, source = %source, "picture unavailable"),
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a picture forgotten is asked for again; all cleared (a new side) are
    //! asked for again; one already asked for is not asked twice.
    use super::*;

    #[test]
    fn a_forgotten_picture_is_asked_for_again() {
        const MSG: u32 = msgloop::msg::APP + 904;
        let window = msgloop::MessageWindow::new("Kabuk.Test.Pictures", "", |m| (m.id == MSG).then_some(0)).unwrap();
        let store = Images::default();
        let mut p = Pictures::start("test", window.hwnd(), MSG);
        let source = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let wait = |p: &mut Pictures| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while p.pending() > 0 && std::time::Instant::now() < deadline {
                msgloop::run_until(&[], Some(std::time::Duration::from_millis(50)));
                p.arrived(&store);
            }
        };
        p.ask([source.clone()], 32);
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 1, "asked once");
        wait(&mut p);
        assert!(p.has(&source));
        assert!(store.contains(p.key(&source, 32)));
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 0, "loaded: not asked again");
        p.forget(&source);
        assert!(!p.has(&source));
        p.ask([source.clone()], 32);
        assert_eq!(p.pending(), 1, "forgotten: asked again");
        wait(&mut p);
        p.clear();
        p.ask([source.clone()], 48);
        assert_eq!(p.pending(), 1, "cleared: asked again at the new side");
        wait(&mut p);
        assert!(store.contains(p.key(&source, 48)));
    }
}
