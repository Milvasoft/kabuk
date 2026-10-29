//! The bar's figures (CPU, RAM, network rate) read off the bar thread: listing the network
//! interfaces took seconds on a busy machine with many virtual adapters (found by M19 P3:
//! the first bars waited 2.6 s for it). A worker samples on its interval while told to,
//! keeps the latest sample and tells the control window; the bar draws it.

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use kabuk_win::msgloop::{self, Msg};
use kabuk_win::stats::{Sample, Sampler, Wants};
use kabuk_win::window::Hwnd;

/// The worker and its latest sample.
#[derive(Default)]
pub struct Figures {
    /// To the worker: what to read from now on (`None`: nothing, it waits).
    worker: Option<mpsc::Sender<Option<Wants>>>,
    latest: Arc<Mutex<Option<Sample>>>,
}

impl Figures {
    /// Starts the worker; it posts `message` to `control` after each sample, one every
    /// `every` while it reads anything. It reads nothing until [`Figures::want`].
    pub fn start(control: Hwnd, message: u32, every: Duration) -> Figures {
        let latest = Arc::<Mutex<Option<Sample>>>::default();
        let out = latest.clone();
        let (tx, rx) = mpsc::channel::<Option<Wants>>();
        let spawned = std::thread::Builder::new().name("figures".into()).spawn(move || {
            kabuk_win::system::background_thread();
            let mut sampler = Sampler::default();
            let mut wants: Option<Wants> = None;
            loop {
                let told = match wants {
                    Some(_) => rx.recv_timeout(every),
                    None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                };
                match told {
                    Ok(w) => wants = w,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                let Some(w) = wants else { continue };
                let sample = sampler.sample(w);
                if let Ok(mut l) = out.lock() {
                    *l = Some(sample);
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
                tracing::warn!(error = %e, "figures not read");
                None
            }
        };
        Figures { worker, latest }
    }

    /// Reads `wants` from now on, the first sample at once; `None` stops reading.
    pub fn want(&self, wants: Option<Wants>) {
        if let Some(w) = &self.worker {
            let _ = w.send(wants);
        }
    }

    /// The latest sample, once.
    pub fn take(&self) -> Option<Sample> {
        self.latest.lock().ok()?.take()
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: nothing is read until asked; a sample comes at once when asked and
    //! again after the interval; stopped, none comes.
    use super::*;

    #[test]
    fn figures_come_while_asked_for_and_stop_after() {
        const MSG: u32 = msgloop::msg::APP + 905;
        const EVERY: Duration = Duration::from_millis(200);
        let told = std::rc::Rc::new(std::cell::Cell::new(0));
        let t = told.clone();
        let w = msgloop::MessageWindow::new("Kabuk.Test.Figures", "", move |m| {
            (m.id == MSG).then(|| {
                t.set(t.get() + 1);
                0
            })
        })
        .unwrap();
        let f = Figures::start(w.hwnd(), MSG, EVERY);
        let pump = |d: Duration| msgloop::run_until(&[], Some(d));
        pump(Duration::from_millis(300));
        assert_eq!(told.get(), 0, "nothing read before asked");
        assert!(f.take().is_none());
        f.want(Some(Wants {
            cpu: true,
            ram: true,
            net: false,
            gpu: false,
        }));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while told.get() < 2 && std::time::Instant::now() < deadline {
            pump(Duration::from_millis(50));
        }
        assert!(told.get() >= 2, "a sample at once and one after the interval");
        assert!(f.take().is_some_and(|s| s.ram_gib.is_some()));
        f.want(None);
        pump(Duration::from_millis(300));
        let stopped = told.get();
        pump(Duration::from_millis(600));
        assert_eq!(told.get(), stopped, "none after it stopped");
    }
}
