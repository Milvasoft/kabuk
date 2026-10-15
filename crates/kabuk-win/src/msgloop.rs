//! Message-only windows, global hotkeys and a message loop that also waits on
//! kernel objects (process exit, emergency event).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use kabuk_core::config::shortcut::{Chord, Key};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW, KillTimer,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, GWLP_USERDATA, HWND_MESSAGE, HWND_TOPMOST, MSG,
    MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    USER_TIMER_MINIMUM, WINDOW_EX_STYLE, WINDOW_STYLE, WM_HOTKEY, WM_QUIT, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::handle::{timeout_ms, Waitable};
use crate::sync::io_error;
use crate::window::Hwnd;

/// `WM_HOTKEY`, for handlers and tests.
pub const MSG_HOTKEY: u32 = WM_HOTKEY;

/// A window message delivered to a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    pub id: u32,
    pub wparam: usize,
    pub lparam: isize,
}

enum Handler {
    /// Runs once at a time; a nested message while it runs (e.g. from a modal box) falls
    /// through to the default processing.
    Exclusive(RefCell<Box<dyn FnMut(Msg) -> Option<isize>>>),
    /// Also runs for nested messages, so none is lost: for shell surfaces, which receive
    /// system notifications while their own code waits on another process.
    Reentrant(Box<dyn Fn(Msg) -> Option<isize>>),
}

/// A window whose messages go to a closure. Lives on the creating thread.
pub struct Window {
    hwnd: Hwnd,
    handler: *mut Handler,
}

/// The message-only flavour, for windows that only receive messages.
pub type MessageWindow = Window;

/// A screen rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Window {
    /// Keeps getting the mouse while a button is held outside the window (a drag), or
    /// lets it go.
    pub fn capture_mouse(&self, on: bool) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
        // SAFETY: plain calls for our own window on its thread.
        unsafe {
            if on {
                SetCapture(self.hwnd().raw());
            } else {
                let _ = ReleaseCapture();
            }
        }
    }

    /// Creates a message-only window. `class` is registered on first use; `title` lets
    /// other processes find it (see [`crate::window::find_message_window`]).
    pub fn new(
        class: &str,
        title: &str,
        handler: impl FnMut(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        create(
            class,
            title,
            WINDOW_EX_STYLE(0),
            WINDOW_STYLE(0),
            PxRect::default(),
            Some(HWND_MESSAGE),
            Handler::Exclusive(RefCell::new(Box::new(handler))),
        )
    }

    /// Creates a hidden shell surface: a borderless popup that never takes focus, is not
    /// in the task switcher and draws only through DirectComposition. The handler is
    /// called for nested messages too, so it must guard its own state (e.g. `try_borrow`).
    pub fn shell_surface(
        class: &str,
        title: &str,
        rect: PxRect,
        handler: impl Fn(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        let ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST;
        create(
            class,
            title,
            ex,
            WS_POPUP,
            rect,
            None,
            Handler::Reentrant(Box::new(handler)),
        )
    }

    /// Like [`Window::shell_surface`], but among ordinary windows rather than above them
    /// (a tabbed group's strip, which sits with its group). Move it with
    /// [`Window::place_on_top`].
    pub fn desk_surface(
        class: &str,
        title: &str,
        rect: PxRect,
        handler: impl Fn(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        let ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP;
        create(
            class,
            title,
            ex,
            WS_POPUP,
            rect,
            None,
            Handler::Reentrant(Box::new(handler)),
        )
    }

    /// A child of another program's window drawn through DirectComposition (the screen
    /// saver's preview in Windows' screen saver settings), covering `rect` in the
    /// parent's client pixels.
    pub fn child_surface(
        class: &str,
        parent: Hwnd,
        rect: PxRect,
        handler: impl Fn(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        use windows::Win32::UI::WindowsAndMessaging::{WS_CHILD, WS_VISIBLE};
        create(
            class,
            "",
            WS_EX_NOREDIRECTIONBITMAP,
            WS_CHILD | WS_VISIBLE,
            rect,
            Some(parent.raw()),
            Handler::Reentrant(Box::new(handler)),
        )
    }

    /// A layered child of Explorer's desktop window drawn through DirectComposition: the
    /// animated wallpaper behind the icons ([`crate::desktop`]). Layered, because 24H2's
    /// `Progman` draws only layered children (the M12 spike).
    pub fn layered_child(
        class: &str,
        parent: Hwnd,
        rect: PxRect,
        handler: impl Fn(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetLayeredWindowAttributes, LWA_ALPHA, WS_CHILD, WS_EX_LAYERED, WS_VISIBLE,
        };
        let w = create(
            class,
            "",
            WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
            WS_CHILD | WS_VISIBLE,
            rect,
            Some(parent.raw()),
            Handler::Reentrant(Box::new(handler)),
        )?;
        // SAFETY: our own window, made fully opaque.
        let _ = unsafe { SetLayeredWindowAttributes(w.hwnd.raw(), COLORREF(0), u8::MAX, LWA_ALPHA) };
        Ok(w)
    }

    /// Like [`Window::shell_surface`], but it can take the keyboard (the launcher): it
    /// becomes active when [`Window::activate`] is called after the user's input.
    pub fn input_surface(
        class: &str,
        title: &str,
        rect: PxRect,
        handler: impl Fn(Msg) -> Option<isize> + 'static,
    ) -> std::io::Result<Window> {
        let ex = WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST;
        create(
            class,
            title,
            ex,
            WS_POPUP,
            rect,
            None,
            Handler::Reentrant(Box::new(handler)),
        )
    }

    /// Shows the window and gives it the keyboard. Windows allows this while Kabuk is
    /// handling the user's input (a shortcut); otherwise the window shows without focus
    /// and this returns false.
    pub fn activate(&self) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, SW_SHOW};
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(self.hwnd.raw(), SW_SHOW);
            let front = SetForegroundWindow(self.hwnd.raw()).as_bool();
            let _ = SetFocus(Some(self.hwnd.raw()));
            front
        }
    }

    pub fn hwnd(&self) -> Hwnd {
        self.hwnd
    }

    /// Moves and resizes without activating or changing the z-order.
    pub fn set_rect(&self, r: PxRect) {
        // SAFETY: our own window; flags avoid activation.
        let _ = unsafe { SetWindowPos(self.hwnd.raw(), Some(HWND_TOPMOST), r.x, r.y, r.w, r.h, SWP_NOACTIVATE) };
    }

    /// Moves and resizes, and puts it above the other ordinary windows, without
    /// activating it.
    pub fn place_on_top(&self, r: PxRect) {
        use windows::Win32::UI::WindowsAndMessaging::HWND_TOP;
        // SAFETY: our own window; flags avoid activation.
        let _ = unsafe { SetWindowPos(self.hwnd.raw(), Some(HWND_TOP), r.x, r.y, r.w, r.h, SWP_NOACTIVATE) };
    }

    pub fn show(&self) {
        // SAFETY: our own window.
        let _ = unsafe { ShowWindow(self.hwnd.raw(), SW_SHOWNOACTIVATE) };
    }

    pub fn hide(&self) {
        // SAFETY: our own window.
        let _ = unsafe { ShowWindow(self.hwnd.raw(), SW_HIDE) };
    }

    /// Starts (or restarts) a window timer that delivers `WM_TIMER` with `wparam == id`.
    pub fn set_timer(&self, id: usize, after: Duration) {
        set_timer(self.hwnd, id, after);
    }

    /// Puts the window at the top of the topmost band, without activating, moving or
    /// showing it.
    pub fn raise_topmost(&self) {
        raise_topmost(self.hwnd);
    }

    /// Lets the mouse through to whatever is under the window (a popup fading out). The
    /// window stays layered at full opacity, so its composition content shows as before.
    pub fn pass_input(&self) {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetLayeredWindowAttributes, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED, WS_EX_TRANSPARENT,
        };
        // SAFETY: our own window; only extended style bits are added.
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd.raw(), GWL_EXSTYLE);
            SetWindowLongPtrW(
                self.hwnd.raw(),
                GWL_EXSTYLE,
                ex | (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize,
            );
            let _ = SetLayeredWindowAttributes(self.hwnd.raw(), COLORREF(0), u8::MAX, LWA_ALPHA);
        }
    }

    pub fn kill_timer(&self, id: usize) {
        // SAFETY: our own window.
        let _ = unsafe { KillTimer(Some(self.hwnd.raw()), id) };
    }
}

/// Asks for one `WM_MOUSELEAVE` when the mouse leaves `window` (a window of this thread).
pub fn track_mouse_leave(window: Hwnd) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    let mut t = TRACKMOUSEEVENT {
        cbSize: u32::try_from(std::mem::size_of::<TRACKMOUSEEVENT>()).unwrap_or_default(),
        dwFlags: TME_LEAVE,
        hwndTrack: window.raw(),
        dwHoverTime: 0,
    };
    // SAFETY: `t` is a valid, correctly sized structure for the call.
    let _ = unsafe { TrackMouseEvent(&mut t) };
}

/// Puts `window` right behind `behind` in the z-order (both topmost windows stay topmost),
/// without activating, moving or showing it.
pub fn place_after(window: Hwnd, behind: Hwnd) {
    // SAFETY: plain call; the flags leave position, size and activation alone.
    let _ = unsafe {
        SetWindowPos(
            window.raw(),
            Some(behind.raw()),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
}

/// [`Window::raise_topmost`] for code that holds only the handle of a window of this thread.
pub fn raise_topmost(window: Hwnd) {
    // SAFETY: a window of this thread; the flags leave position, size and activation alone.
    let _ = unsafe {
        SetWindowPos(
            window.raw(),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
}

fn create(
    class: &str,
    title: &str,
    ex_style: WINDOW_EX_STYLE,
    style: WINDOW_STYLE,
    rect: PxRect,
    parent: Option<HWND>,
    handler: Handler,
) -> std::io::Result<Window> {
    let class_name = HSTRING::from(class);
    // SAFETY: a null module name returns the current executable's handle.
    let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(io_error)?;
    // The arrow over Kabuk's surfaces; without a class cursor Windows keeps whatever it
    // showed last, often the busy one.
    // SAFETY: loads a shared system cursor; it is never destroyed.
    let arrow = unsafe {
        windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, windows::Win32::UI::WindowsAndMessaging::IDC_ARROW)
    }
    .unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or_default(),
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        hCursor: arrow,
        // Double clicks arrive as WM_LBUTTONDBLCLK (tray icons that open on a double click).
        style: windows::Win32::UI::WindowsAndMessaging::CS_DBLCLKS,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `wc` is fully initialised and its strings outlive the call.
    if unsafe { RegisterClassExW(&wc) } == 0 {
        // SAFETY: reads the error left by RegisterClassExW.
        let err = unsafe { GetLastError() };
        if err != ERROR_CLASS_ALREADY_EXISTS {
            return Err(std::io::Error::from_raw_os_error(i32::try_from(err.0).unwrap_or(-1)));
        }
    }
    // SAFETY: the class was registered above; the window is created hidden.
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            &class_name,
            &HSTRING::from(title),
            style,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            parent,
            None,
            Some(instance.into()),
            None,
        )
    }
    .map_err(io_error)?;
    let handler = Box::into_raw(Box::new(handler));
    // SAFETY: the window belongs to this thread; the pointer stays valid until Drop clears it.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, handler as isize) };
    Ok(Window {
        hwnd: Hwnd::from_raw(hwnd),
        handler,
    })
}

impl Drop for Window {
    fn drop(&mut self) {
        let hwnd = self.hwnd.raw();
        // SAFETY: detach the handler first so no message reaches it after it is freed.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        // SAFETY: the window was created by this object on this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
        // SAFETY: the pointer came from Box::into_raw in `new` and is freed exactly once.
        drop(unsafe { Box::from_raw(self.handler) });
    }
}

thread_local! {
    /// A panic caught in a callback from Windows, which cannot unwind through Windows'
    /// frames; [`run_until`] raises it again on this thread.
    static CAUGHT: RefCell<Option<Box<dyn std::any::Any + Send>>> = const { RefCell::new(None) };
}

/// Runs `f`, called by Windows (a window procedure, a WinEvent callback). A panic is kept
/// for [`run_until`] to raise again, and `fallback` is returned to Windows.
pub(crate) fn contain<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            CAUGHT.with(|c| {
                let mut c = c.borrow_mut();
                if c.is_none() {
                    *c = Some(payload);
                }
            });
            fallback
        }
    }
}

/// Raises a panic [`contain`] caught on this thread.
fn raise_caught() {
    if let Some(payload) = CAUGHT.with(|c| c.borrow_mut().take()) {
        std::panic::resume_unwind(payload);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds either 0 or the live handler pointer set in `create`.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Handler;
    if !ptr.is_null() {
        // SAFETY: non-null means the Window is alive (Drop clears the pointer first).
        let handler = unsafe { &*ptr };
        let m = Msg {
            id: msg,
            wparam: wparam.0,
            lparam: lparam.0,
        };
        let result = contain(None, || match handler {
            Handler::Exclusive(cell) => cell.try_borrow_mut().ok().and_then(|mut f| f(m)),
            Handler::Reentrant(f) => f(m),
        });
        if let Some(result) = result {
            return LRESULT(result);
        }
    }
    // SAFETY: default processing with the arguments we received.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Virtual-key codes the launcher and the app grid read from `WM_KEYDOWN`.
pub mod vk {
    use windows::Win32::UI::Input::KeyboardAndMouse as k;

    pub const UP: usize = k::VK_UP.0 as usize;
    pub const DOWN: usize = k::VK_DOWN.0 as usize;
    pub const LEFT: usize = k::VK_LEFT.0 as usize;
    pub const RIGHT: usize = k::VK_RIGHT.0 as usize;
    /// Page Up and Page Down.
    pub const PRIOR: usize = k::VK_PRIOR.0 as usize;
    pub const NEXT: usize = k::VK_NEXT.0 as usize;
    pub const HOME: usize = k::VK_HOME.0 as usize;
    pub const END: usize = k::VK_END.0 as usize;
    pub const RETURN: usize = k::VK_RETURN.0 as usize;
    pub const ESCAPE: usize = k::VK_ESCAPE.0 as usize;
    pub const BACK: usize = k::VK_BACK.0 as usize;
    pub const V: usize = b'V' as usize;
}

/// Whether Shift was down when the message being handled was sent (the queue's key state).
pub fn shift_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
    // SAFETY: reads this thread's key state.
    let state = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) };
    state < 0
}

/// Whether Ctrl was down when the message being handled was sent (the queue's key state).
pub fn ctrl_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
    // SAFETY: reads this thread's key state.
    let state = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) };
    state < 0
}

/// Shows the text cursor (I-beam) until the pointer moves on, in answer to
/// `WM_SETCURSOR` over a text field.
pub fn show_text_cursor() {
    use windows::Win32::UI::WindowsAndMessaging::{LoadCursorW, SetCursor, IDC_IBEAM};
    // SAFETY: a shared system cursor, never destroyed.
    if let Ok(c) = unsafe { LoadCursorW(None, IDC_IBEAM) } {
        // SAFETY: as above; this thread's cursor.
        unsafe { SetCursor(Some(c)) };
    }
}

/// Posts a message to a window of any process on this desktop.
pub fn post_message(to: Hwnd, msg: Msg) -> std::io::Result<()> {
    // SAFETY: PostMessageW only queues the message; invalid handles fail cleanly.
    unsafe { PostMessageW(Some(to.raw()), msg.id, WPARAM(msg.wparam), LPARAM(msg.lparam)) }.map_err(io_error)
}

/// Sends a message to a window of any process and waits at most `timeout` for it to be
/// handled; returns the handler's result. A hung or missing window gives an error.
pub fn send_message(to: Hwnd, msg: Msg, timeout: Duration) -> std::io::Result<isize> {
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, SMTO_ABORTIFHUNG};
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let mut result = 0usize;
    // SAFETY: plain values only (no pointers in wparam/lparam are dereferenced here);
    // `result` is a valid out pointer.
    let r = unsafe {
        SendMessageTimeoutW(
            to.raw(),
            msg.id,
            WPARAM(msg.wparam),
            LPARAM(msg.lparam),
            SMTO_ABORTIFHUNG,
            ms,
            Some(&mut result),
        )
    };
    if r.0 == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result as isize)
    }
}

/// Starts (or restarts) timer `id` on a window of this thread, for code that holds only
/// the handle (e.g. a window procedure that cannot reach the [`Window`]).
pub fn set_timer(window: Hwnd, id: usize, after: Duration) {
    let ms = u32::try_from(after.as_millis())
        .unwrap_or(u32::MAX)
        .max(USER_TIMER_MINIMUM);
    // SAFETY: a timer on a window handle; a timer with the same id is replaced, and a
    // destroyed window makes the call fail without effect.
    unsafe { SetTimer(Some(window.raw()), id, ms, None) };
}

/// Ends the current thread's message loop with `code`.
pub fn post_quit(code: i32) {
    // SAFETY: plain call affecting only this thread's queue.
    unsafe { PostQuitMessage(code) };
}

/// Why [`run_until`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// The waitable at this index is signalled.
    Signalled(usize),
    Timeout,
    /// `WM_QUIT` arrived with this exit code.
    Quit(i32),
}

/// Dispatches this thread's messages until one of `waits` is signalled, `WM_QUIT`
/// arrives or `timeout` passes. Sleeps in the kernel between events (no polling).
pub fn run_until(waits: &[&dyn Waitable], timeout: Option<Duration>) -> Wake {
    let deadline = timeout.map(|t| Instant::now() + t);
    let handles: Vec<HANDLE> = waits.iter().map(|w| w.wait_handle().raw()).collect();
    loop {
        let mut msg = MSG::default();
        // SAFETY: `msg` is a valid out pointer; messages for this thread only.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            // WinEvent callbacks and sent messages run inside PeekMessageW.
            raise_caught();
            if msg.message == WM_QUIT {
                return Wake::Quit(i32::try_from(msg.wParam.0).unwrap_or(0));
            }
            // SAFETY: standard dispatch of a message we just removed from the queue.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            raise_caught();
        }
        raise_caught();
        let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if remaining == Some(Duration::ZERO) {
            return Wake::Timeout;
        }
        // SAFETY: every handle belongs to a live Waitable borrowed for this call.
        let r = unsafe {
            MsgWaitForMultipleObjectsEx(
                (!handles.is_empty()).then_some(&handles[..]),
                timeout_ms(remaining),
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if r == WAIT_TIMEOUT {
            return Wake::Timeout;
        }
        let index = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if index < handles.len() {
            return Wake::Signalled(index);
        }
        // index == len: messages arrived; loop to dispatch them.
    }
}

/// A registered global hotkey; unregistered on drop.
#[derive(Debug)]
pub struct HotKey {
    hwnd: Hwnd,
    id: i32,
}

impl HotKey {
    /// Registers `chord` for `window`; it receives `WM_HOTKEY` with `wparam == id`.
    pub fn register(window: &MessageWindow, id: i32, chord: Chord) -> std::io::Result<HotKey> {
        let mut mods = MOD_NOREPEAT;
        if chord.mods.ctrl {
            mods |= MOD_CONTROL;
        }
        if chord.mods.alt {
            mods |= MOD_ALT;
        }
        if chord.mods.shift {
            mods |= MOD_SHIFT;
        }
        if chord.mods.win {
            mods |= MOD_WIN;
        }
        let vk = virtual_key(chord.key);
        // SAFETY: the window belongs to this thread and outlives the registration (see Drop).
        unsafe { RegisterHotKey(Some(window.hwnd.raw()), id, mods, u32::from(vk.0)) }.map_err(io_error)?;
        Ok(HotKey { hwnd: window.hwnd, id })
    }
}

impl Drop for HotKey {
    fn drop(&mut self) {
        // SAFETY: unregisters the id registered in `register` for the same window.
        let _ = unsafe { UnregisterHotKey(Some(self.hwnd.raw()), self.id) };
    }
}

/// The Windows virtual-key code of a key.
/// The key a virtual-key code is, when Kabuk's chords have it (the inverse of
/// [`virtual_key`]).
pub fn key_of_virtual(vk: u16) -> Option<Key> {
    use kabuk_core::config::shortcut::Key as K;
    let named = [
        K::Space,
        K::Enter,
        K::Tab,
        K::Escape,
        K::Backspace,
        K::Delete,
        K::Insert,
        K::Home,
        K::End,
        K::PageUp,
        K::PageDown,
        K::Left,
        K::Right,
        K::Up,
        K::Down,
    ];
    let letters = (b'A'..=b'Z').map(|c| K::Letter(char::from(c)));
    let digits = (0..=9).map(K::Digit);
    let functions = (1..=24).map(K::Function);
    named
        .into_iter()
        .chain(letters)
        .chain(digits)
        .chain(functions)
        .find(|k| virtual_key(*k).0 == vk)
}

pub fn virtual_key(key: Key) -> VIRTUAL_KEY {
    match key {
        // 'A'..'Z' and '0'..'9' share their ASCII codes with VK_A.. and VK_0..
        Key::Letter(c) => VIRTUAL_KEY(VK_A.0 + (c as u16 - u16::from(b'A'))),
        Key::Digit(d) => VIRTUAL_KEY(VK_0.0 + u16::from(d)),
        Key::Function(n) => VIRTUAL_KEY(VK_F1.0 + u16::from(n.saturating_sub(1))),
        Key::Space => VK_SPACE,
        Key::Enter => VK_RETURN,
        Key::Tab => VK_TAB,
        Key::Escape => VK_ESCAPE,
        Key::Backspace => VK_BACK,
        Key::Delete => VK_DELETE,
        Key::Insert => VK_INSERT,
        Key::Home => VK_HOME,
        Key::End => VK_END,
        Key::PageUp => VK_PRIOR,
        Key::PageDown => VK_NEXT,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: messages reach the handler; a posted WM_QUIT ends the loop; a
    //! signalled event wakes the loop; timeout with nothing to do; the window is
    //! findable by title and gone after drop; key code mapping at the range ends; a
    //! surface let through keeps its other styles; a desk surface stays out of the
    //! topmost band.
    use super::*;
    use crate::sync::NamedEvent;
    use crate::window::find_message_window;
    use std::rc::Rc;

    const TEST_MSG: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 7;

    #[test]
    fn virtual_keys_map_back_to_their_keys() {
        use kabuk_core::config::shortcut::Key;
        for key in [
            Key::Letter('A'),
            Key::Letter('Z'),
            Key::Digit(0),
            Key::Digit(9),
            Key::Function(1),
            Key::Function(24),
            Key::Escape,
            Key::Down,
        ] {
            assert_eq!(key_of_virtual(virtual_key(key).0), Some(key), "{key:?}");
        }
        // Shift, and a key Kabuk's chords do not have (the OEM comma).
        assert_eq!(key_of_virtual(0x10), None);
        assert_eq!(key_of_virtual(0xBC), None);
    }

    #[test]
    fn handler_receives_posted_messages_and_quit_ends_loop() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let w = MessageWindow::new("Kabuk.Test.MsgLoop", "loop-test", move |m| {
            if m.id == TEST_MSG {
                s.borrow_mut().push(m.wparam);
                post_quit(5);
                return Some(0);
            }
            None
        })
        .unwrap();
        post_message(
            w.hwnd(),
            Msg {
                id: TEST_MSG,
                wparam: 42,
                lparam: 0,
            },
        )
        .unwrap();
        assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(5));
        assert_eq!(*seen.borrow(), vec![42]);
    }

    #[test]
    fn a_desk_surface_is_not_topmost_and_never_takes_focus() {
        use windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
        let w = Window::desk_surface("Kabuk.Test.Desk", "", PxRect::default(), |_| None).unwrap();
        // SAFETY: reading our own window's style.
        let ex = unsafe { GetWindowLongPtrW(w.hwnd().raw(), GWL_EXSTYLE) as u32 };
        assert_eq!(ex & WS_EX_TOPMOST.0, 0);
        assert!(ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_NOREDIRECTIONBITMAP.0 != 0);
        w.place_on_top(PxRect {
            x: -32000,
            y: -32000,
            w: 10,
            h: 10,
        });
        // SAFETY: reading our own window's style again.
        let ex = unsafe { GetWindowLongPtrW(w.hwnd().raw(), GWL_EXSTYLE) as u32 };
        assert_eq!(ex & WS_EX_TOPMOST.0, 0, "placing it made it topmost");
    }

    #[test]
    fn a_surface_that_passes_input_is_click_through() {
        use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TRANSPARENT};
        let w = Window::shell_surface("Kabuk.Test.Pass", "", PxRect::default(), |_| None).unwrap();
        let ex = || {
            // SAFETY: reading our own window's style.
            unsafe { GetWindowLongPtrW(w.hwnd().raw(), GWL_EXSTYLE) as u32 }
        };
        let through = WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0;
        assert_eq!(ex() & through, 0);
        w.pass_input();
        assert_eq!(ex() & through, through);
        // Its other bits stay.
        assert!(ex() & WS_EX_NOREDIRECTIONBITMAP.0 != 0 && ex() & WS_EX_NOACTIVATE.0 != 0);
    }

    /// Regression: a panic in a handler ran inside the window procedure, which cannot
    /// unwind, so the process aborted and the window manager's own failure handling
    /// never ran. It now comes out of `run_until` as an ordinary panic.
    #[test]
    fn a_panic_in_a_handler_unwinds_out_of_the_loop() {
        let w = MessageWindow::new("Kabuk.Test.MsgLoop", "panic-test", move |m| {
            if m.id == TEST_MSG {
                panic!("handler failed");
            }
            None
        })
        .unwrap();
        post_message(
            w.hwnd(),
            Msg {
                id: TEST_MSG,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap();
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_until(&[], Some(Duration::from_secs(5)))
        }));
        let payload = ran.expect_err("the panic reaches the caller");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"handler failed"));
        // The loop works again afterwards.
        post_quit(3);
        assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(3));
    }

    #[test]
    fn event_wakes_the_loop_and_timeout_returns() {
        let name = format!("Local\\kabuk-test-loop-{}", std::process::id());
        let e = NamedEvent::create_or_open(&name).unwrap();
        assert_eq!(run_until(&[&e], Some(Duration::from_millis(20))), Wake::Timeout);
        e.set().unwrap();
        assert_eq!(run_until(&[&e], Some(Duration::from_secs(5))), Wake::Signalled(0));
    }

    /// Regression: the classes had no cursor, so over the bar Windows kept showing the
    /// busy (spinning) cursor.
    #[test]
    fn shell_surfaces_show_the_arrow_cursor() {
        use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, LoadCursorW, GCLP_HCURSOR, IDC_ARROW};
        let w = Window::shell_surface("Kabuk.Test.Cursor", "", PxRect::default(), |_| None).unwrap();
        // SAFETY: plain queries on our own window class and a shared system cursor.
        let (cursor, arrow) = unsafe {
            (
                GetClassLongPtrW(w.hwnd().raw(), GCLP_HCURSOR),
                LoadCursorW(None, IDC_ARROW).unwrap().0 as usize,
            )
        };
        assert_eq!(cursor, arrow);
    }

    #[test]
    fn window_classes_take_double_clicks() {
        use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, CS_DBLCLKS, GCL_STYLE};
        let w = Window::shell_surface("Kabuk.Test.DoubleClick", "", PxRect::default(), |_| None).unwrap();
        // SAFETY: plain query on our own window class.
        let style = unsafe { GetClassLongPtrW(w.hwnd().raw(), GCL_STYLE) } as u32;
        assert_ne!(style & CS_DBLCLKS.0, 0);
    }

    #[test]
    fn window_is_findable_until_dropped() {
        let title = format!("find-test-{}", std::process::id());
        let w = MessageWindow::new("Kabuk.Test.Find", &title, |_| None).unwrap();
        assert_eq!(find_message_window("Kabuk.Test.Find", &title), Some(w.hwnd()));
        drop(w);
        assert_eq!(find_message_window("Kabuk.Test.Find", &title), None);
    }

    /// A message sent to a window from inside its own handler: a shell surface gets it, a
    /// message window lets it fall through.
    #[test]
    fn shell_surfaces_receive_nested_messages() {
        use std::cell::Cell;
        fn nested_count(make: impl FnOnce(Rc<Cell<Option<Hwnd>>>, Rc<Cell<u32>>) -> Window) -> u32 {
            let me = Rc::new(Cell::new(None));
            let nested = Rc::new(Cell::new(0));
            let w = make(me.clone(), nested.clone());
            me.set(Some(w.hwnd()));
            post_message(
                w.hwnd(),
                Msg {
                    id: TEST_MSG,
                    wparam: 0,
                    lparam: 0,
                },
            )
            .unwrap();
            assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(0));
            nested.get()
        }
        let handler = |me: Rc<Cell<Option<Hwnd>>>, nested: Rc<Cell<u32>>| {
            move |m: Msg| {
                if m.id == TEST_MSG {
                    let hwnd = me.get().unwrap();
                    crate::system::send_setting_change(hwnd, "KabukTest", Duration::from_secs(1)).unwrap();
                    post_quit(0);
                    return Some(0);
                }
                if m.id == msg::SETTINGCHANGE
                    && crate::system::setting_change_area(m.lparam).as_deref() == Some("KabukTest")
                {
                    nested.set(nested.get() + 1);
                    return Some(0);
                }
                None
            }
        };
        let surface = nested_count(|me, nested| {
            // Never shown: a hidden popup, nothing appears on screen.
            Window::shell_surface("Kabuk.Test.Nested", "", PxRect::default(), handler(me, nested)).unwrap()
        });
        assert_eq!(surface, 1);
        let exclusive =
            nested_count(|me, nested| MessageWindow::new("Kabuk.Test.NestedMsg", "", handler(me, nested)).unwrap());
        assert_eq!(exclusive, 0);
    }

    #[test]
    fn virtual_keys() {
        assert_eq!(virtual_key(Key::Letter('A')), VK_A);
        assert_eq!(virtual_key(Key::Letter('Z')), VK_Z);
        assert_eq!(virtual_key(Key::Digit(0)), VK_0);
        assert_eq!(virtual_key(Key::Digit(9)), VK_9);
        assert_eq!(virtual_key(Key::Function(1)), VK_F1);
        assert_eq!(virtual_key(Key::Function(24)), VK_F24);
        assert_eq!(virtual_key(Key::End), VK_END);
    }
}

/// Window message ids the application handles, so it never needs the `windows` crate.
pub mod msg {
    use windows::Win32::UI::WindowsAndMessaging as w;

    pub const TIMER: u32 = w::WM_TIMER;
    pub const LBUTTONUP: u32 = w::WM_LBUTTONUP;
    pub const RBUTTONUP: u32 = w::WM_RBUTTONUP;
    pub const MOUSEWHEEL: u32 = w::WM_MOUSEWHEEL;
    /// The wheel tilted sideways (or a touchpad's sideways swipe).
    pub const MOUSEHWHEEL: u32 = w::WM_MOUSEHWHEEL;
    /// In a mouse message's `wparam`: the left button is down.
    pub const MK_LBUTTON: usize = 0x0001;
    pub const MOUSEMOVE: u32 = w::WM_MOUSEMOVE;
    /// `WM_MOUSELEAVE` (defined in the `Win32_UI_Controls` feature, not enabled here).
    pub const MOUSELEAVE: u32 = 0x02A3;
    pub const LBUTTONDBLCLK: u32 = w::WM_LBUTTONDBLCLK;
    pub const KEYDOWN: u32 = w::WM_KEYDOWN;
    pub const SYSKEYDOWN: u32 = w::WM_SYSKEYDOWN;
    pub const LBUTTONDOWN: u32 = w::WM_LBUTTONDOWN;
    pub const RBUTTONDOWN: u32 = w::WM_RBUTTONDOWN;
    pub const MBUTTONDOWN: u32 = w::WM_MBUTTONDOWN;
    pub const MBUTTONUP: u32 = w::WM_MBUTTONUP;
    pub const CHAR: u32 = w::WM_CHAR;
    /// The window may choose the cursor: [`super::show_text_cursor`] and `Some(1)`, or
    /// `None` for the class's arrow.
    pub const SETCURSOR: u32 = w::WM_SETCURSOR;
    /// `WM_ACTIVATE`'s `wparam` low word when the window loses activation.
    pub const WA_INACTIVE: usize = w::WA_INACTIVE as usize;
    pub const MOUSEACTIVATE: u32 = w::WM_MOUSEACTIVATE;
    pub const DPICHANGED: u32 = w::WM_DPICHANGED;
    pub const DISPLAYCHANGE: u32 = w::WM_DISPLAYCHANGE;
    pub const SETTINGCHANGE: u32 = w::WM_SETTINGCHANGE;
    pub const WINDOWPOSCHANGED: u32 = w::WM_WINDOWPOSCHANGED;
    pub const ACTIVATE: u32 = w::WM_ACTIVATE;
    pub const QUERYENDSESSION: u32 = w::WM_QUERYENDSESSION;
    pub const ENDSESSION: u32 = w::WM_ENDSESSION;
    pub const POWERBROADCAST: u32 = w::WM_POWERBROADCAST;
    pub const COPYDATA: u32 = w::WM_COPYDATA;
    /// First id free for application messages.
    pub const APP: u32 = w::WM_APP;
    /// `WM_MOUSEACTIVATE` result: do not activate.
    pub const MA_NOACTIVATE: isize = w::MA_NOACTIVATE as isize;
    /// One notch of the mouse wheel.
    pub const WHEEL_DELTA: i32 = w::WHEEL_DELTA as i32;

    /// The point packed in a mouse message's `lparam` (signed 16-bit x and y).
    pub fn point(lparam: isize) -> (i32, i32) {
        let x = i32::from((lparam & 0xffff) as u16 as i16);
        let y = i32::from(((lparam >> 16) & 0xffff) as u16 as i16);
        (x, y)
    }

    /// The signed wheel rotation in a `WM_MOUSEWHEEL` `wparam`.
    pub fn wheel(wparam: usize) -> i32 {
        i32::from(((wparam >> 16) & 0xffff) as u16 as i16)
    }

    #[cfg(test)]
    mod tests {
        //! Edge cases: negative coordinates (left monitor), wheel up and down.
        use super::*;

        #[test]
        fn unpacking() {
            let lp = ((-20i32 as u16 as isize) << 16) | (-2560i32 as u16 as isize);
            assert_eq!(point(lp), (-2560, -20));
            assert_eq!(point((40 << 16) | 300), (300, 40));
            assert_eq!(wheel((120usize) << 16), 120);
            assert_eq!(wheel(((-240i32 as u16) as usize) << 16), -240);
        }
    }
}
