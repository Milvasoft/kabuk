//! The input thread (ARCHITECTURE §3): the low-level mouse hook for grabbing windows with
//! Alt and for double clicks on a floating window's title (`kabuk_core::grab`), and for
//! the hot corners (`kabuk_core::hot_corner`), which it tells the bar of.
//!
//! Windows calls the hook for every mouse event on the machine and waits for it, so this
//! thread does nothing else: it decides from a snapshot the window manager keeps up to
//! date, and hands everything else to the window manager's queue.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use kabuk_core::config::{Config, HotCornerAction, HotCorners};
use kabuk_core::grab::{self, Grabber, Input, Order, Target};
use kabuk_win::input::{self, MouseHook, MouseKind};
use kabuk_win::msgloop::{self, msg, Msg, Window};
use kabuk_win::window::Hwnd;

/// How long a window may take to say whether a point is on its title bar. Asked inside
/// the hook, only for the second click of a double click on a floating window.
const CAPTION_TIMEOUT: Duration = Duration::from_millis(50);
/// The smallest frame an Alt+right drag leaves, in physical pixels.
const MIN_FRAME: (i32, i32) = (160, 100);
/// Presses the mask key once the hook has returned.
const MSG_MASK: u32 = msg::APP + 1;
/// Ends the input thread.
const MSG_STOP: u32 = msg::APP + 2;
/// Removes the hook while [`Snapshot::paused`] says so (game mode, ARCHITECTURE §11), and
/// puts it back after.
const MSG_PAUSE: u32 = msg::APP + 3;
const CLASS: &str = "Kabuk.Input";

/// What the hook knows: the managed windows (and whether each floats) and the settings.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub managed: HashMap<u64, bool>,
    pub alt_drag: bool,
    pub double_click_to_tile: bool,
    /// No hook at all while a full-screen program runs (anti-cheat systems watch for
    /// global hooks); set with [`pause`].
    pub paused: bool,
    /// The input thread's window, for [`pause`].
    pub window: Option<Hwnd>,
    /// What each corner does, the monitors' rectangles, and the bar's control window,
    /// which acts on a corner reached ([`MSG_HOT_CORNER`]).
    pub corners: HotCorners,
    pub monitors: Vec<kabuk_core::state::Rect>,
    pub bar: Option<Hwnd>,
}

/// Posted to the bar's control window when the mouse reached a hot corner: `wparam` is
/// the action's place in [`HotCornerAction::ALL`].
pub const MSG_HOT_CORNER: u32 = msg::APP + 48;

/// Removes the hook (`true`) or puts it back, from any thread.
pub fn pause(snapshot: &SharedSnapshot, paused: bool) {
    let window = {
        let mut s = snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.paused == paused {
            return;
        }
        s.paused = paused;
        s.window
    };
    if let Some(w) = window {
        let _ = msgloop::post_message(
            w,
            Msg {
                id: MSG_PAUSE,
                wparam: 0,
                lparam: 0,
            },
        );
    }
}

impl Snapshot {
    pub fn set_config(&mut self, cfg: &Config) {
        self.alt_drag = cfg.windows.alt_drag;
        self.double_click_to_tile = cfg.windows.double_click_to_tile;
        self.corners = cfg.hot_corners;
    }

    /// Whether any corner does something (else moves cost the hook nothing).
    fn corners_on(&self) -> bool {
        kabuk_core::hot_corner::Corner::ALL
            .iter()
            .any(|c| c.action(&self.corners) != HotCornerAction::None)
    }
}

pub type SharedSnapshot = Arc<Mutex<Snapshot>>;

/// The running input thread; dropping it removes the hook.
pub struct InputThread {
    thread: Option<std::thread::JoinHandle<()>>,
    window: Hwnd,
}

impl InputThread {
    /// Starts the thread; `orders` receives what the window manager has to do.
    pub fn start(snapshot: SharedSnapshot, orders: impl Fn(Order) + Send + 'static) -> std::io::Result<InputThread> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new().name("input".into()).spawn(move || {
            let me: Rc<Cell<Option<Hwnd>>> = Rc::new(Cell::new(None));
            let orders: Rc<dyn Fn(Order)> = Rc::new(orders);
            let make = {
                let (snapshot, me) = (snapshot.clone(), me.clone());
                move || hook(snapshot.clone(), orders.clone(), me.clone())
            };
            let current: Rc<RefCell<Option<MouseHook>>> = Rc::new(RefCell::new(None));
            let window = {
                let (snapshot, current) = (snapshot.clone(), current.clone());
                let make = make.clone();
                Window::new(CLASS, "input", move |m: Msg| match m.id {
                    MSG_MASK => {
                        input::mask_alt();
                        Some(0)
                    }
                    MSG_PAUSE => {
                        let paused = snapshot
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .paused;
                        let mut hook = current.borrow_mut();
                        if paused && hook.is_some() {
                            *hook = None;
                            tracing::info!("mouse hook removed for a full-screen program");
                        } else if !paused && hook.is_none() {
                            match make() {
                                Ok(h) => {
                                    *hook = Some(h);
                                    tracing::info!("mouse hook back after a full-screen program");
                                }
                                Err(e) => tracing::error!(error = %e, "mouse hook not put back"),
                            }
                        }
                        Some(0)
                    }
                    MSG_STOP => {
                        msgloop::post_quit(0);
                        Some(0)
                    }
                    _ => None,
                })
            };
            let window = match window {
                Ok(w) => w,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            me.set(Some(window.hwnd()));
            match make() {
                Ok(h) => *current.borrow_mut() = Some(h),
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            }
            snapshot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .window = Some(window.hwnd());
            let _ = ready_tx.send(Ok(window.hwnd()));
            tracing::info!("mouse hook installed");
            let _ = msgloop::run_until(&[], None);
            current.borrow_mut().take();
            drop(window);
            tracing::info!("mouse hook removed");
        })?;
        match ready_rx.recv() {
            Ok(Ok(window)) => Ok(InputThread {
                thread: Some(thread),
                window,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(std::io::Error::other("input thread ended during start"))
            }
        }
    }
}

/// Installs the hook: grabs and double clicks from the snapshot, orders to `orders`, the
/// mask key through the input thread's window `me`.
fn hook(snapshot: SharedSnapshot, orders: Rc<dyn Fn(Order)>, me: Rc<Cell<Option<Hwnd>>>) -> std::io::Result<MouseHook> {
    let (double_click_ms, double_click_size) = input::double_click();
    let mut grabber = Grabber::new();
    let mut corners = kabuk_core::hot_corner::Watch::default();
    let mut buttons = 0u32;
    MouseHook::start(move |e| {
        match e.kind {
            MouseKind::Down(_) => buttons += 1,
            MouseKind::Up(_) => buttons = buttons.saturating_sub(1),
            MouseKind::Move => {
                let snap = snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if snap.corners_on() {
                    let held = buttons > 0 || grabber.holding();
                    if let Some(c) = corners.moved(e.at, &snap.monitors, held, &snap.corners) {
                        let action = c.action(&snap.corners);
                        let wparam = HotCornerAction::ALL.iter().position(|a| *a == action).unwrap_or(0);
                        if let Some(bar) = snap.bar {
                            let _ = msgloop::post_message(
                                bar,
                                Msg {
                                    id: MSG_HOT_CORNER,
                                    wparam,
                                    lparam: 0,
                                },
                            );
                        }
                    }
                }
            }
        }
        // Moves come all the time; they matter only while a window is held.
        if e.kind == MouseKind::Move && !grabber.holding() {
            return false;
        }
        let (options, target) = {
            let snap = snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let options = grab::Options {
                alt_drag: snap.alt_drag,
                double_click_to_tile: snap.double_click_to_tile,
                double_click_ms,
                double_click_size,
                min_size: MIN_FRAME,
            };
            let target = match e.kind {
                MouseKind::Down(_) => target_at(e.at, &snap.managed),
                _ => None,
            };
            (options, target)
        };
        let input = match e.kind {
            MouseKind::Down(button) => Input::Down {
                button,
                at: e.at,
                time: e.time,
                keys: input::keys(),
                target,
            },
            MouseKind::Up(button) => Input::Up { button },
            MouseKind::Move => Input::Move { at: e.at },
        };
        let step = grabber.input(input, &options, |p| {
            target.is_some_and(|t| input::on_caption(Hwnd(t.window), p, CAPTION_TIMEOUT))
        });
        for order in step.orders {
            match order {
                Order::MaskAlt => {
                    if let Some(w) = me.get() {
                        let _ = msgloop::post_message(
                            w,
                            Msg {
                                id: MSG_MASK,
                                wparam: 0,
                                lparam: 0,
                            },
                        );
                    }
                }
                other => orders(other),
            }
        }
        step.swallow
    })
}

impl Drop for InputThread {
    fn drop(&mut self) {
        let _ = msgloop::post_message(
            self.window,
            Msg {
                id: MSG_STOP,
                wparam: 0,
                lparam: 0,
            },
        );
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The managed window under the cursor, unless it is maximised or minimised (Windows
/// owns those).
fn target_at(at: grab::Point, managed: &HashMap<u64, bool>) -> Option<Target> {
    let root = input::root_window_at(at)?;
    let floating = *managed.get(&root.0)?;
    if root.is_maximized() || root.is_minimized() {
        return None;
    }
    Some(Target {
        window: root.0,
        rect: root.frame_rect()?,
        floating,
    })
}
