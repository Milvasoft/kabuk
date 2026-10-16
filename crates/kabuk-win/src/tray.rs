//! Receiving other programs' tray icons while Explorer runs (ARCHITECTURE §9, DECISIONS
//! D19): a hidden topmost window of class `Shell_TrayWnd` gets their `Shell_NotifyIcon`
//! calls. Every `WM_COPYDATA` is forwarded to Explorer's tray first and Explorer's answer
//! returned, so Explorer keeps every icon (nothing is lost if Kabuk crashes) and AppBar
//! messages still reach it.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use kabuk_core::tray::{self as model, IconCall, RectQuery};

use crate::msgloop::{self, PxRect, Window};
use crate::window::{windows_of_class, Hwnd};

const TRAY_CLASS: &str = "Shell_TrayWnd";
/// `WM_COPYDATA` kinds sent to the tray window.
const COPYDATA_NOTIFY_ICON: usize = 1;
/// `Shell_NotifyIconGetRect`: where is an icon on screen.
const COPYDATA_ICON_RECT: usize = 3;
/// How long a forwarded message may take in Explorer.
const FORWARD_TIMEOUT: Duration = Duration::from_millis(1000);

/// `COPYDATASTRUCT`.
#[repr(C)]
struct CopyData {
    dw_data: usize,
    cb_data: u32,
    lp_data: *const u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// A program called `Shell_NotifyIcon`.
    Call(IconCall),
}

thread_local! {
    /// Kabuk's tray window on this thread, while it exists.
    static OURS: Cell<Option<Hwnd>> = const { Cell::new(None) };
}

/// Runs `f` with Explorer's tray window in front of Kabuk's, so `SHAppBarMessage` calls
/// made inside reach Explorer directly: AppBar calls forwarded through Kabuk's window are
/// answered but do not change the work area. Kabuk's window is put back in front after.
pub(crate) fn explorer_first<R>(f: impl FnOnce() -> R) -> R {
    let ours = OURS.with(Cell::get);
    let explorer = ours.and_then(|o| explorer_tray(Some(o)));
    if let (Some(o), Some(e)) = (ours, explorer) {
        crate::msgloop::place_after(o, e);
        let r = f();
        crate::msgloop::raise_topmost(o);
        r
    } else {
        f()
    }
}

/// Explorer's tray window: a `Shell_TrayWnd` that is not ours.
fn explorer_tray(ours: Option<Hwnd>) -> Option<Hwnd> {
    crate::shell::primary_taskbar()
        .filter(|h| Some(*h) != ours)
        .or_else(|| windows_of_class(TRAY_CLASS).into_iter().find(|h| Some(*h) != ours))
}

/// Our tray window; dropping it gives the icons back to Explorer.
pub struct TrayHost {
    /// `None` only while dropping.
    window: Option<Window>,
    explorer: Rc<Cell<Option<Hwnd>>>,
    taskbar_created: u32,
}

impl TrayHost {
    /// Creates the window and asks every program to register its icons again. Calls
    /// arrive on this thread's message loop. `place` tells where an icon is on screen,
    /// when a program asks (`None`: not one of Kabuk's, so Explorer answers).
    pub fn start(
        sink: impl Fn(TrayEvent) + 'static,
        place: impl Fn(&RectQuery) -> Option<PxRect> + 'static,
    ) -> std::io::Result<TrayHost> {
        let explorer = Rc::new(Cell::new(explorer_tray(None)));
        if explorer.get().is_none() {
            return Err(std::io::Error::other("Explorer's tray window was not found"));
        }
        let target = explorer.clone();
        let window = Window::shell_surface(TRAY_CLASS, "", PxRect::default(), move |m| {
            if m.id != msgloop::msg::COPYDATA {
                return None;
            }
            // SAFETY: for WM_COPYDATA, lparam points to a COPYDATASTRUCT (and its data)
            // that Windows copied into this process for the duration of the message.
            let (kind, bytes) = unsafe {
                let cds = &*(m.lparam as *const CopyData);
                let len = usize::try_from(cds.cb_data).unwrap_or(0);
                let bytes = if cds.lp_data.is_null() || len == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(cds.lp_data, len)
                };
                (cds.dw_data, bytes)
            };
            // Where an icon is: Kabuk answers for the icons it draws, as Explorer's taskbar
            // is hidden and would place a menu at the screen's edge.
            if kind == COPYDATA_ICON_RECT {
                let q = model::parse_rect_query(bytes);
                if let Some((q, r)) = q.and_then(|q| place(&q).map(|r| (q, r))) {
                    tracing::debug!(asks = ?q.asks, hwnd = q.hwnd, uid = q.uid, ?r, "tray icon place answered");
                    return Some(q.answer(r.x, r.y, r.x + r.w, r.y + r.h));
                }
                tracing::debug!(
                    parsed = q.is_some(),
                    len = bytes.len(),
                    "tray icon place left to Explorer"
                );
            }
            // Explorer first, so it keeps everything; its answer is the caller's result.
            let answer = target
                .get()
                .and_then(|e| msgloop::send_message(e, m, FORWARD_TIMEOUT).ok())
                .unwrap_or(0);
            if kind == 0 {
                let message = bytes.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                tracing::trace!(?message, len = bytes.len(), answer, "AppBar message forwarded");
            }
            if kind == COPYDATA_NOTIFY_ICON {
                if let Some(call) = model::parse(bytes) {
                    sink(TrayEvent::Call(call));
                }
            }
            // Explorer may bring its own tray window forward while it handles the call
            // (seen with a full-screen video playing): ours goes back in front before the
            // program's next call (a version right after an add) looks for a tray.
            if let Some(o) = OURS.with(Cell::get) {
                crate::msgloop::raise_topmost(o);
            }
            Some(answer)
        })?;
        let host = TrayHost {
            window: Some(window),
            explorer,
            taskbar_created: crate::system::register_message(crate::system::TASKBAR_CREATED),
        };
        OURS.with(|o| o.set(host.hwnd()));
        host.raise();
        host.broadcast();
        tracing::info!("tray window up; programs asked to register their icons");
        Ok(host)
    }

    /// Keeps our window in front of Explorer's, so programs find ours.
    pub fn raise(&self) {
        if let Some(w) = &self.window {
            w.raise_topmost();
        }
    }

    /// Whether `FindWindow` would give programs our window.
    pub fn in_front(&self) -> bool {
        windows_of_class(TRAY_CLASS).first().copied() == self.window.as_ref().map(Window::hwnd)
    }

    fn broadcast(&self) {
        crate::system::broadcast_notify(self.taskbar_created);
    }

    /// Explorer announced its tray (`TaskbarCreated`): forward to its current tray window,
    /// put ours back in front (Explorer's new one is on top now) and ask programs to
    /// register with us again. The caller ignores the copy of this broadcast it receives.
    pub fn explorer_ready(&self) {
        self.explorer.set(explorer_tray(self.window.as_ref().map(Window::hwnd)));
        tracing::info!("Explorer's tray is ready; taking the tray icons again");
        self.raise();
        self.broadcast();
    }

    pub fn hwnd(&self) -> Option<Hwnd> {
        self.window.as_ref().map(Window::hwnd)
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        // Our window first, then programs are told to register with Explorer.
        OURS.with(|o| o.set(None));
        drop(self.window.take());
        self.broadcast();
        tracing::info!("tray window removed; icons back with Explorer");
    }
}
