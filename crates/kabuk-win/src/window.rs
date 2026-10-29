//! Top-level windows of any process: identify, enumerate, describe, show, hide, move,
//! focus, close and frame them.
//!
//! Nothing here waits on another process: showing and hiding use `ShowWindowAsync`,
//! moving uses `SWP_ASYNCWINDOWPOS` and closing posts `WM_CLOSE`, so a hung window can
//! never block Kabuk (ARCHITECTURE §1.3).

use windows::core::{BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetAncestor, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    PostMessageW, SetForegroundWindow, SetWindowPos, ShowWindowAsync, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, GW_OWNER,
    HWND_MESSAGE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOWNA, WINDOW_LONG_PTR_INDEX, WM_CLOSE, WS_CAPTION, WS_CHILD,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_THICKFRAME,
};

/// The settings app's window class (`webview`) for Kabuk instance `instance`, known
/// without that feature so the shell and its tests can find the window. One per
/// instance: a test's settings window never meets the user's.
pub fn settings_class(instance: &str) -> String {
    format!("Kabuk.Settings.{instance}")
}

/// A window's corners (Windows 11's DWM).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corners {
    Default,
    Square,
    Round,
    /// Slightly rounded (Defter's 3 px).
    RoundSmall,
}

/// What Kabuk gives other programs' frames to follow the look (DECISIONS D30): the
/// title bar's colour and its text's, and the corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameStyle {
    pub caption: (u8, u8, u8),
    pub text: (u8, u8, u8),
    pub corners: Corners,
}

/// A colour as Windows' `COLORREF` (`0x00BBGGRR`).
pub fn colorref(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

/// A window handle as a plain value. Handles are global to the session, so they can be
/// stored in the state file and passed between threads; every call re-validates them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hwnd(pub u64);

impl Hwnd {
    pub(crate) fn from_raw(h: HWND) -> Hwnd {
        Hwnd(h.0 as usize as u64)
    }

    pub(crate) fn raw(self) -> HWND {
        HWND(self.0 as usize as *mut core::ffi::c_void)
    }

    /// True while the handle names an existing window.
    pub fn exists(self) -> bool {
        // SAFETY: IsWindow accepts any value and only reports whether it is a window.
        unsafe { IsWindow(Some(self.raw())) }.as_bool()
    }

    pub fn is_visible(self) -> bool {
        // SAFETY: IsWindowVisible accepts any handle and returns false for invalid ones.
        unsafe { IsWindowVisible(self.raw()) }.as_bool()
    }

    /// The owning process id, or `None` if the window is gone.
    pub fn pid(self) -> Option<u32> {
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out pointer; an invalid handle makes the call return 0.
        let thread = unsafe { GetWindowThreadProcessId(self.raw(), Some(&mut pid)) };
        (thread != 0).then_some(pid)
    }

    pub fn class_name(self) -> String {
        let mut buf = [0u16; 256];
        // SAFETY: the buffer is valid for its length; the call writes at most that many chars.
        let n = unsafe { GetClassNameW(self.raw(), &mut buf) };
        String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
    }

    /// The window title. Reads it with `GetWindowTextW`, which does not send a message
    /// to windows of other processes, so it cannot hang.
    pub fn title(self) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { GetWindowTextW(self.raw(), &mut buf) };
        String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
    }

    /// The visible frame in physical pixels, without the invisible resize borders
    /// Windows 10/11 add around windows (`DWMWA_EXTENDED_FRAME_BOUNDS`).
    pub fn frame_rect(self) -> Option<kabuk_core::state::Rect> {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        crate::dpi::physical(|| {
            let mut r = RECT::default();
            // SAFETY: `r` is a valid out buffer of the size passed.
            unsafe {
                DwmGetWindowAttribute(
                    self.raw(),
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    (&mut r as *mut RECT).cast(),
                    u32::try_from(std::mem::size_of::<RECT>()).unwrap_or_default(),
                )
            }
            .ok()?;
            Some(kabuk_core::state::Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            })
        })
    }

    fn long(self, index: WINDOW_LONG_PTR_INDEX) -> u32 {
        // SAFETY: reads a window long; invalid handles return 0.
        let v = unsafe { GetWindowLongPtrW(self.raw(), index) };
        // Styles are 32-bit values stored in a pointer-sized slot.
        v as u32
    }

    /// The owner window (dialogs, palettes), if any.
    pub fn owner(self) -> Option<Hwnd> {
        // SAFETY: plain query; an invalid handle yields an error.
        let h = unsafe { GetWindow(self.raw(), GW_OWNER) }.ok()?;
        (!h.is_invalid()).then(|| Hwnd::from_raw(h))
    }

    /// True for a top-level window (its own root).
    pub fn is_top_level(self) -> bool {
        // SAFETY: plain query.
        let root = unsafe { GetAncestor(self.raw(), GA_ROOT) };
        root == self.raw()
    }

    /// Always on top (`WS_EX_TOPMOST`).
    pub fn is_topmost(self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST;
        self.long(GWL_EXSTYLE) & WS_EX_TOPMOST.0 != 0
    }

    /// True when the mouse goes through the window (layered and transparent to input).
    pub fn passes_input(self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{WS_EX_LAYERED, WS_EX_TRANSPARENT};
        let through = WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0;
        self.long(GWL_EXSTYLE) & through == through
    }

    /// Hidden by DWM although "visible" (another virtual desktop, a suspended UWP app).
    pub fn is_cloaked(self) -> bool {
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        let mut cloaked = 0u32;
        // SAFETY: `cloaked` is a valid out buffer of the size passed.
        let r = unsafe {
            DwmGetWindowAttribute(
                self.raw(),
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32).cast(),
                u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default(),
            )
        };
        r.is_ok() && cloaked != 0
    }

    /// Blurs what is behind this (our own) window and tints it with `tint` (red, green,
    /// blue, alpha), as Windows' taskbar does; `None` turns it off. Uses user32's
    /// `SetWindowCompositionAttribute`, which only changes how DWM composes the window
    /// (no code runs in other processes); false where Windows lacks it.
    pub fn set_blur_behind(self, tint: Option<[u8; 4]>) -> bool {
        use windows::core::s;
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        /// `WINDOWCOMPOSITIONATTRIB::WCA_ACCENT_POLICY`.
        const WCA_ACCENT_POLICY: u32 = 19;
        /// `ACCENT_STATE`: off, and the acrylic blur Windows' taskbar uses.
        const ACCENT_DISABLED: u32 = 0;
        const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
        #[repr(C)]
        struct AccentPolicy {
            state: u32,
            flags: u32,
            /// The tint as 0xAABBGGRR.
            gradient: u32,
            animation: u32,
        }
        #[repr(C)]
        struct CompositionData {
            attribute: u32,
            data: *mut core::ffi::c_void,
            size: usize,
        }
        type SetAttribute = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
        // SAFETY: user32 is loaded in every GUI process; the export, when present, has the
        // signature above (it has since Windows 7).
        let set: Option<SetAttribute> = unsafe {
            let user32 = GetModuleHandleW(windows::core::w!("user32.dll")).ok();
            user32
                .and_then(|m| GetProcAddress(m, s!("SetWindowCompositionAttribute")))
                .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, SetAttribute>(f))
        };
        let Some(set) = set else { return false };
        let mut policy = AccentPolicy {
            state: if tint.is_some() {
                ACCENT_ENABLE_ACRYLICBLURBEHIND
            } else {
                ACCENT_DISABLED
            },
            flags: 0,
            gradient: tint.map_or(0, |[r, g, b, a]| u32::from_le_bytes([r, g, b, a])),
            animation: 0,
        };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size: std::mem::size_of::<AccentPolicy>(),
        };
        // SAFETY: `data` points at a live policy of the size given, for the call only.
        unsafe { set(self.raw(), &mut data) }.as_bool()
    }

    pub fn is_minimized(self) -> bool {
        // SAFETY: plain query.
        unsafe { IsIconic(self.raw()) }.as_bool()
    }

    /// The window's size when it is not minimised (pixels): its restored size while it
    /// is minimised, else its size now.
    pub fn restored_size(self) -> Option<(i32, i32)> {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
        if !self.is_minimized() {
            let r = self.window_rect()?;
            return Some((r.right - r.left, r.bottom - r.top));
        }
        crate::dpi::physical(|| {
            let mut p = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            // SAFETY: `p` is a valid out pointer with its length set.
            unsafe { GetWindowPlacement(self.raw(), &mut p) }.ok()?;
            let r = p.rcNormalPosition;
            Some((r.right - r.left, r.bottom - r.top))
        })
    }

    /// Where Windows will put the window back (its outer rectangle, in workspace
    /// coordinates: see `kabuk_core::genie::workspace_to_screen`) and whether it comes
    /// back maximised; for a minimised window too.
    pub fn restored_place(self) -> Option<(kabuk_core::state::Rect, bool)> {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED};
        crate::dpi::physical(|| {
            let mut p = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            // SAFETY: `p` is a valid out pointer with its length set.
            unsafe { GetWindowPlacement(self.raw(), &mut p) }.ok()?;
            let r = p.rcNormalPosition;
            let maximized = (p.flags.0 & WPF_RESTORETOMAXIMIZED.0) != 0 || self.is_maximized();
            Some((
                kabuk_core::state::Rect {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                maximized,
            ))
        })
    }

    /// Moves where Windows will put a minimised window back to `normal` (its outer
    /// rectangle, in workspace coordinates), keeping it minimised and whether it comes back
    /// maximised (on `normal`'s monitor). Never blocks; false when Windows refuses.
    pub fn set_restored_place(self, normal: kabuk_core::state::Rect) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowPlacement, SetWindowPlacement, SW_SHOWMINNOACTIVE, WINDOWPLACEMENT, WPF_ASYNCWINDOWPLACEMENT,
        };
        crate::dpi::physical(|| {
            let mut p = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            // SAFETY: `p` is a valid out pointer with its length set.
            if unsafe { GetWindowPlacement(self.raw(), &mut p) }.is_err() {
                return false;
            }
            p.rcNormalPosition = windows::Win32::Foundation::RECT {
                left: normal.left,
                top: normal.top,
                right: normal.right,
                bottom: normal.bottom,
            };
            p.showCmd = SW_SHOWMINNOACTIVE.0 as u32;
            p.flags |= WPF_ASYNCWINDOWPLACEMENT;
            // SAFETY: a placement read from the window, its rectangle and show state set.
            unsafe { SetWindowPlacement(self.raw(), &p) }.is_ok()
        })
    }

    pub fn is_maximized(self) -> bool {
        // SAFETY: plain query.
        unsafe { IsZoomed(self.raw()) }.as_bool()
    }

    /// Whether Windows' taskbar would show a button for it ([`kabuk_core::wm::on_taskbar`]):
    /// a few cheap queries, no process opened.
    pub fn on_taskbar(self) -> bool {
        // Most top-level windows are hidden: nothing more is asked of them.
        if !self.is_visible() {
            return false;
        }
        let style = self.long(GWL_STYLE);
        let ex = self.long(GWL_EXSTYLE);
        let has = |bits: u32, flag: u32| bits & flag == flag;
        kabuk_core::wm::on_taskbar(&kabuk_core::wm::WindowFacts {
            class: self.class_name(),
            visible: true,
            cloaked: self.is_cloaked(),
            child: has(style, WS_CHILD.0),
            tool_window: has(ex, WS_EX_TOOLWINDOW.0),
            app_window: has(ex, WS_EX_APPWINDOW.0),
            no_activate: has(ex, WS_EX_NOACTIVATE.0),
            owned: self.owner().is_some(),
            ..Default::default()
        })
    }

    /// Everything the window manager's filter needs ([`kabuk_core::wm::classify`]).
    pub fn facts(self) -> kabuk_core::wm::WindowFacts {
        kabuk_core::wm::WindowFacts {
            elevated: self.pid().is_some_and(crate::process::runs_elevated),
            ..self.facts_but_rights()
        }
    }

    /// [`Hwnd::facts`] without asking whether it runs elevated (`elevated` false): that
    /// opens its process, and only decides between floating and tiling a window the
    /// filter does not ignore anyway.
    pub fn facts_but_rights(self) -> kabuk_core::wm::WindowFacts {
        let style = self.long(GWL_STYLE);
        let ex = self.long(GWL_EXSTYLE);
        let has = |bits: u32, flag: u32| bits & flag == flag;
        let owner = self.owner();
        // SAFETY: plain query; does not send a message.
        let title_len = unsafe { GetWindowTextLengthW(self.raw()) };
        kabuk_core::wm::WindowFacts {
            class: self.class_name(),
            has_title: title_len > 0,
            visible: self.is_visible(),
            cloaked: self.is_cloaked(),
            minimized: self.is_minimized(),
            child: has(style, WS_CHILD.0),
            tool_window: has(ex, WS_EX_TOOLWINDOW.0),
            app_window: has(ex, WS_EX_APPWINDOW.0),
            no_activate: has(ex, WS_EX_NOACTIVATE.0),
            caption: has(style, WS_CAPTION.0),
            resizable: has(style, WS_THICKFRAME.0),
            popup: has(style, WS_POPUP.0),
            owned: owner.is_some(),
            owner_visible: owner.is_some_and(Hwnd::is_visible),
            elevated: false,
            full_screen: !has(style, WS_CAPTION.0) && self.covers_its_monitor(),
        }
    }

    /// The monitor that holds most of the window (its `HMONITOR` value), if it is on one.
    pub fn monitor_handle(self) -> Option<isize> {
        use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONULL};
        // SAFETY: plain query; a gone window gives no monitor.
        let m = unsafe { MonitorFromWindow(self.raw(), MONITOR_DEFAULTTONULL) };
        (!m.is_invalid()).then_some(m.0 as isize)
    }

    /// The rectangle of the monitor the window is on, as Windows gives it now (a game in
    /// exclusive full screen may have changed its resolution), physical pixels.
    pub fn monitor_rect(self) -> Option<kabuk_core::state::Rect> {
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
        crate::dpi::physical(|| {
            // SAFETY: plain query; an invalid window yields a null monitor.
            let m = unsafe { MonitorFromWindow(self.raw(), MONITOR_DEFAULTTONULL) };
            if m.is_invalid() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
                ..Default::default()
            };
            // SAFETY: `info` is a valid MONITORINFO with its size set.
            unsafe { GetMonitorInfoW(m, &mut info) }.ok().ok()?;
            let r = info.rcMonitor;
            Some(kabuk_core::state::Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            })
        })
    }

    /// The window covers the whole monitor it is on (borders hanging over its edges
    /// included).
    pub fn covers_its_monitor(self) -> bool {
        match (self.window_rect(), self.monitor_rect()) {
            (Some(w), Some(m)) => w.left <= m.left && w.top <= m.top && w.right >= m.right && w.bottom >= m.bottom,
            _ => false,
        }
    }

    /// A full-screen program's window: no caption, covering its whole monitor (a game, a
    /// video, a presentation; DECISIONS D72).
    pub fn is_full_screen(self) -> bool {
        self.long(GWL_STYLE) & WS_CAPTION.0 != WS_CAPTION.0 && self.covers_its_monitor()
    }

    /// The desktop or a taskbar (Explorer's or Kabuk's tray window).
    pub fn is_shell(self) -> bool {
        SHELL_CLASSES.contains(&self.class_name().as_str())
    }

    /// The whole window rectangle (with the invisible resize borders), physical pixels.
    pub fn window_rect(self) -> Option<kabuk_core::state::Rect> {
        use windows::Win32::Foundation::RECT;
        crate::dpi::physical(|| {
            let mut r = RECT::default();
            // SAFETY: `r` is a valid out pointer.
            unsafe { GetWindowRect(self.raw(), &mut r) }.ok()?;
            Some(kabuk_core::state::Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            })
        })
    }

    /// Asks the window to move so that its *visible* frame becomes `frame` (physical
    /// pixels): the invisible borders Windows adds around it are measured and added
    /// back. Never blocks (`SWP_ASYNCWINDOWPOS`); does not activate or reorder.
    pub fn place_frame_async(self, frame: kabuk_core::state::Rect) -> bool {
        match self.outer_and_frame() {
            Some((outer, visible)) => self.place_frame_like_async(frame, outer, visible),
            None => false,
        }
    }

    /// The outer rectangle and the visible frame now. While a window moves the two are
    /// updated at different moments, so a moving window is placed with the pair measured
    /// before it started ([`Hwnd::place_frame_like_async`]).
    pub fn outer_and_frame(self) -> Option<(kabuk_core::state::Rect, kabuk_core::state::Rect)> {
        Some((self.window_rect()?, self.frame_rect()?))
    }

    /// [`Hwnd::place_frame_async`] with invisible borders measured earlier, as
    /// `outer` around `visible`.
    pub fn place_frame_like_async(
        self,
        frame: kabuk_core::state::Rect,
        outer: kabuk_core::state::Rect,
        visible: kabuk_core::state::Rect,
    ) -> bool {
        let target = with_invisible_borders(frame, outer, visible);
        crate::dpi::physical(|| {
            // SAFETY: plain call on any window; the async flag queues it to the window's thread.
            unsafe {
                SetWindowPos(
                    self.raw(),
                    None,
                    target.left,
                    target.top,
                    target.right - target.left,
                    target.bottom - target.top,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS | SWP_NOOWNERZORDER,
                )
            }
            .is_ok()
        })
    }

    /// Moves the window's top-left corner to (x, y), physical pixels, keeping its size,
    /// without activating it (another program's popup menu, for one).
    pub fn move_to_async(self, x: i32, y: i32) -> bool {
        crate::dpi::physical(|| {
            // SAFETY: plain call on any window; the async flag queues it to the window's thread.
            unsafe {
                SetWindowPos(
                    self.raw(),
                    None,
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS | SWP_NOOWNERZORDER,
                )
            }
            .is_ok()
        })
    }

    /// Brings the window to the foreground. Windows refuses a process that did not get
    /// the last input, and a chord Kabuk takes from its keyboard hook does not count as
    /// Kabuk's (found by the user: Win and an arrow moved no focus); then Kabuk's thread
    /// shares the front window's input for the call, as the front window could hand the
    /// focus on itself. False when Windows still refuses; the caller logs it.
    pub fn focus(self) -> bool {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        };
        // SAFETY: plain calls; invalid handles fail.
        let front = || unsafe { GetForegroundWindow() } == self.raw();
        // SAFETY: as above.
        if unsafe { SetForegroundWindow(self.raw()) }.as_bool() && front() {
            return true;
        }
        let mut fg_pid = 0u32;
        // SAFETY: plain queries and a valid out pointer.
        let (fg_thread, me) = unsafe {
            (
                GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut fg_pid)),
                GetCurrentThreadId(),
            )
        };
        // Never with a thread of Kabuk's own (found by a test: the tab strip in front, the
        // bar's thread waiting on this one, both stopped for good).
        if fg_pid == std::process::id() {
            return front();
        }
        // SAFETY: joins this thread's input with the front window's thread, undone below.
        let joined = fg_thread != 0 && fg_thread != me && unsafe { AttachThreadInput(me, fg_thread, true) }.as_bool();
        if !joined {
            // Nothing to share (no window in front): another try only stirs the windows
            // (found by a test: it handed this thread messages in the middle of its work).
            return front();
        }
        // SAFETY: plain calls on a window handle.
        unsafe {
            let _ = BringWindowToTop(self.raw());
            let _ = SetForegroundWindow(self.raw());
        }
        // SAFETY: undoes the join above.
        let _ = unsafe { AttachThreadInput(me, fg_thread, false) };
        front()
    }

    /// Asks the window to close, as its close button would. Never blocks.
    pub fn close_async(self) -> bool {
        // SAFETY: PostMessageW only queues the message.
        unsafe { PostMessageW(Some(self.raw()), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    /// Sets the DWM border colour (Windows 11), or back to the system default with `None`.
    /// Returns false where unsupported (Windows 10) or when the window is gone.
    pub fn set_border_color(self, rgb: Option<(u8, u8, u8)>) -> bool {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT};
        let value: u32 = match rgb {
            Some((r, g, b)) => colorref(r, g, b),
            None => DWMWA_COLOR_DEFAULT,
        };
        // SAFETY: `value` is a valid 4-byte buffer for the attribute.
        unsafe {
            DwmSetWindowAttribute(
                self.raw(),
                DWMWA_BORDER_COLOR,
                (&value as *const u32).cast(),
                u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default(),
            )
        }
        .is_ok()
    }

    /// Turns Windows' own transitions (minimise, restore, show and hide) for the window on
    /// or off, so the genie can play them instead. Returns false when the window is gone.
    pub fn set_transitions(self, on: bool) -> bool {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
        let disabled = BOOL::from(!on);
        // SAFETY: `disabled` is a valid BOOL buffer for the attribute.
        unsafe {
            DwmSetWindowAttribute(
                self.raw(),
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&raw const disabled).cast(),
                u32::try_from(std::mem::size_of::<BOOL>()).unwrap_or_default(),
            )
        }
        .is_ok()
    }

    /// Gives the window's frame `style` through DWM (DECISIONS D30), or Windows' defaults
    /// back with `None`. Returns false where unsupported or when the window is gone.
    pub fn set_frame_style(self, style: Option<FrameStyle>) -> bool {
        use windows::Win32::Graphics::Dwm::{
            DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_TEXT_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
            DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWCP_ROUNDSMALL,
        };
        let colorref = |c: Option<(u8, u8, u8)>| c.map_or(DWMWA_COLOR_DEFAULT, |(r, g, b)| colorref(r, g, b));
        let corners = match style.map(|s| s.corners) {
            None | Some(Corners::Default) => DWMWCP_DEFAULT,
            Some(Corners::Square) => DWMWCP_DONOTROUND,
            Some(Corners::Round) => DWMWCP_ROUND,
            Some(Corners::RoundSmall) => DWMWCP_ROUNDSMALL,
        };
        let caption = self.dwm_set(DWMWA_CAPTION_COLOR, colorref(style.map(|s| s.caption)));
        let text = self.dwm_set(DWMWA_TEXT_COLOR, colorref(style.map(|s| s.text)));
        let round = self.dwm_set(DWMWA_WINDOW_CORNER_PREFERENCE, corners.0 as u32);
        caption && text && round
    }

    /// The corner preference DWM holds for the window (`DWMWCP_*`), for tests: the only
    /// frame attribute DWM reads back (the colours answer "invalid parameter").
    pub fn corner_preference(self) -> Option<u32> {
        self.dwm_get(windows::Win32::Graphics::Dwm::DWMWA_WINDOW_CORNER_PREFERENCE)
    }

    fn dwm_set(self, attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE, value: u32) -> bool {
        use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
        // SAFETY: `value` is a valid 4-byte buffer, the size these attributes take.
        unsafe {
            DwmSetWindowAttribute(
                self.raw(),
                attribute,
                (&value as *const u32).cast(),
                u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default(),
            )
        }
        .is_ok()
    }

    fn dwm_get(self, attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE) -> Option<u32> {
        use windows::Win32::Graphics::Dwm::DwmGetWindowAttribute;
        let mut value = 0u32;
        // SAFETY: `value` is a valid 4-byte out buffer of the size passed.
        unsafe {
            DwmGetWindowAttribute(
                self.raw(),
                attribute,
                (&mut value as *mut u32).cast(),
                u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default(),
            )
        }
        .ok()
        .map(|()| value)
    }

    /// Asks the window to show without activating it. Never blocks.
    pub fn show_async(self) -> bool {
        // SAFETY: ShowWindowAsync posts the request; invalid handles make it return false.
        unsafe { ShowWindowAsync(self.raw(), SW_SHOWNA) }.as_bool()
    }

    /// Asks the window to minimise, as its minimise button would. Never blocks.
    pub fn minimize_async(self) -> bool {
        // SAFETY: as for `show_async`.
        unsafe { ShowWindowAsync(self.raw(), SW_MINIMIZE) }.as_bool()
    }

    /// Asks the window to maximise, as its maximise button would. Never blocks.
    pub fn maximize_async(self) -> bool {
        // SAFETY: as for `show_async`.
        unsafe { ShowWindowAsync(self.raw(), SW_MAXIMIZE) }.as_bool()
    }

    /// Asks a minimised or maximised window to return to its normal size. Never blocks.
    pub fn restore_async(self) -> bool {
        // SAFETY: as for `show_async`.
        unsafe { ShowWindowAsync(self.raw(), SW_RESTORE) }.as_bool()
    }

    /// Asks the window to hide. Never blocks. Callers record the window in the state
    /// file before calling this.
    pub fn hide_async(self) -> bool {
        // SAFETY: as for `show_async`.
        unsafe { ShowWindowAsync(self.raw(), SW_HIDE) }.as_bool()
    }

    /// Makes the window fully transparent (`on`), so it shows nothing even while
    /// Windows has it shown, or opaque again. Only a transparency set here is taken off:
    /// a window that was layered on its own stays so. True when the window is as asked.
    /// For Explorer's taskbars, which Explorer shows for a moment whenever a work area
    /// changes (found by the user: Windows' taskbar flashed as the auto-hiding bar came
    /// and went; DECISIONS D77).
    pub fn set_see_through(self, on: bool) -> bool {
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetLayeredWindowAttributes, SetLayeredWindowAttributes, SetWindowLongPtrW, LAYERED_WINDOW_ATTRIBUTES_FLAGS,
            LWA_ALPHA, WS_EX_LAYERED,
        };
        let ex = self.long(GWL_EXSTYLE);
        let layered = ex & WS_EX_LAYERED.0 != 0;
        let mut alpha = 0u8;
        let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
        // SAFETY: plain query with valid out pointers; fails for a window not layered.
        let read = layered
            && unsafe { GetLayeredWindowAttributes(self.raw(), None, Some(&mut alpha), Some(&mut flags)) }.is_ok();
        let ours = read && flags.contains(LWA_ALPHA) && alpha == 0;
        // SAFETY: style and layering changes on a window handle; invalid ones fail.
        unsafe {
            if on {
                if ours {
                    return true;
                }
                if layered {
                    // Layered by its owner: left alone.
                    return false;
                }
                SetWindowLongPtrW(self.raw(), GWL_EXSTYLE, (ex | WS_EX_LAYERED.0) as isize);
                SetLayeredWindowAttributes(self.raw(), COLORREF(0), 0, LWA_ALPHA).is_ok()
            } else {
                if !ours {
                    return !layered || read;
                }
                let _ = SetLayeredWindowAttributes(self.raw(), COLORREF(0), u8::MAX, LWA_ALPHA);
                SetWindowLongPtrW(self.raw(), GWL_EXSTYLE, (ex & !WS_EX_LAYERED.0) as isize);
                true
            }
        }
    }

    /// Whether a visible ordinary (not topmost) window is above this one in the z-order.
    /// A topmost window should never be, yet Kabuk's bars were found under ordinary
    /// windows on the user's machine, as was a topmost window of Explorer's (D78).
    pub fn under_ordinary_window(self) -> bool {
        let mut ordinary_above = false;
        for w in top_level_windows() {
            if w == self {
                return ordinary_above;
            }
            ordinary_above |= w.is_visible() && !w.is_topmost() && !w.is_cloaked();
        }
        // Not a window (any more).
        false
    }

    /// Whether [`Hwnd::set_see_through`] made the window transparent.
    pub fn is_see_through(self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetLayeredWindowAttributes, LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, WS_EX_LAYERED,
        };
        if self.long(GWL_EXSTYLE) & WS_EX_LAYERED.0 == 0 {
            return false;
        }
        let mut alpha = 0u8;
        let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
        // SAFETY: plain query with valid out pointers.
        unsafe { GetLayeredWindowAttributes(self.raw(), None, Some(&mut alpha), Some(&mut flags)) }.is_ok()
            && flags.contains(LWA_ALPHA)
            && alpha == 0
    }
}

/// The window rectangle that puts the visible frame at `frame`, given the window's current
/// outer rectangle and visible frame (their difference is the invisible border).
pub fn with_invisible_borders(
    frame: kabuk_core::state::Rect,
    outer: kabuk_core::state::Rect,
    visible: kabuk_core::state::Rect,
) -> kabuk_core::state::Rect {
    kabuk_core::state::Rect {
        left: frame.left - (visible.left - outer.left),
        top: frame.top - (visible.top - outer.top),
        right: frame.right + (outer.right - visible.right),
        bottom: frame.bottom + (outer.bottom - visible.bottom),
    }
}

/// All top-level windows, visible or not, in z-order.
pub fn top_level_windows() -> Vec<Hwnd> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the address of the Vec passed below, alive for the whole EnumWindows call.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<Hwnd>) };
        out.push(Hwnd::from_raw(hwnd));
        BOOL(1)
    }
    let mut out: Vec<Hwnd> = Vec::new();
    // SAFETY: the callback only touches `out`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut out as *mut Vec<Hwnd> as isize)) };
    out
}

/// Top-level windows of a class, visible or not.
pub fn windows_of_class(class: &str) -> Vec<Hwnd> {
    top_level_windows()
        .into_iter()
        .filter(|w| w.class_name() == class)
        .collect()
}

/// A popup menu is on screen (any program's: a tray icon's menu, a window's menu; Windows
/// draws them as top-level windows of the class `#32768`).
pub fn popup_menu_open() -> bool {
    windows_of_class(MENU_CLASS).into_iter().any(Hwnd::is_visible)
}

/// The class of Windows' popup menus.
const MENU_CLASS: &str = "#32768";

/// Finds a message-only window by class and title.
pub fn find_message_window(class: &str, title: &str) -> Option<Hwnd> {
    let class = HSTRING::from(class);
    let title = HSTRING::from(title);
    // SAFETY: both strings are valid and NUL-terminated for the duration of the call.
    let found = unsafe { FindWindowExW(Some(HWND_MESSAGE), None, PCWSTR(class.as_ptr()), PCWSTR(title.as_ptr())) };
    found.ok().filter(|h| !h.is_invalid()).map(Hwnd::from_raw)
}

/// The desktop and the taskbars: always there, never a program's content
/// ([`Hwnd::is_shell`]).
pub(crate) const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// As [`front_window_rects`], with whether each window is maximised (a maximised window
/// is never a full-screen program, `kabuk_core::fullscreen::is_fullscreen_front`).
pub fn front_windows(skip_pid: u32) -> Vec<(Hwnd, kabuk_core::state::Rect, bool)> {
    front_candidates(skip_pid)
        .into_iter()
        .filter_map(|w| Some((w, w.window_rect()?, w.is_maximized())))
        .collect()
}

/// The outer rectangles of the visible top-level windows that hold a program's content,
/// front first, for full-screen detection. Tool windows (tooltips, overlays' popups),
/// windows that never activate, windows clicks pass through (the overlays of graphics
/// drivers, Game Bar and voice chats over a game; DECISIONS D72), the desktop, the
/// taskbars and `skip_pid`'s windows (Kabuk's own) are left out.
pub fn front_window_rects(skip_pid: u32) -> Vec<kabuk_core::state::Rect> {
    front_candidates(skip_pid)
        .into_iter()
        .filter_map(Hwnd::window_rect)
        .collect()
}

/// The visible ordinary windows, front first, without `skip_pid`'s and the shell's.
fn front_candidates(skip_pid: u32) -> Vec<Hwnd> {
    top_level_windows()
        .into_iter()
        .filter(|w| w.is_visible() && !w.is_minimized() && !w.is_cloaked())
        .filter(|w| {
            let ex = w.long(GWL_EXSTYLE);
            ex & WS_EX_TOOLWINDOW.0 == 0 && ex & WS_EX_NOACTIVATE.0 == 0 && ex & WS_EX_TRANSPARENT.0 == 0
        })
        .filter(|w| w.pid() != Some(skip_pid))
        .filter(|w| !SHELL_CLASSES.contains(&w.class_name().as_str()))
        .collect()
}

/// Every message-only window of a class, of any process (a test picks its shell's by pid).
pub fn message_windows_of_class(class: &str) -> Vec<Hwnd> {
    let class = HSTRING::from(class);
    let mut out = Vec::new();
    let mut after: Option<HWND> = None;
    loop {
        // SAFETY: the class string outlives the call; `after` is a window found before.
        let found = unsafe { FindWindowExW(Some(HWND_MESSAGE), after, PCWSTR(class.as_ptr()), PCWSTR::null()) };
        match found.ok().filter(|h| !h.is_invalid()) {
            Some(h) => {
                out.push(Hwnd::from_raw(h));
                after = Some(h);
            }
            None => return out,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a bogus handle is reported as gone everywhere; enumeration sees at
    //! least the shell's windows; an unknown class matches nothing; message-only windows
    //! are found by class.
    use super::*;

    #[test]
    fn bogus_handle_is_gone() {
        let h = Hwnd(0x1234_5678);
        assert!(!h.exists());
        assert!(!h.is_visible());
        assert_eq!(h.pid(), None);
        assert_eq!(h.class_name(), "");
        assert_eq!(h.title(), "");
        assert!(!h.show_async());
        assert!(!h.minimize_async() && !h.restore_async());
    }

    #[test]
    fn bogus_handle_has_no_facts_and_ignores_requests() {
        let h = Hwnd(0x1234_5678);
        let f = h.facts();
        assert!(!f.visible && !f.owned && f.class.is_empty());
        assert_eq!(kabuk_core::wm::classify(&f), kabuk_core::wm::Verdict::Ignore);
        assert_eq!(h.window_rect(), None);
        assert!(!h.place_frame_async(kabuk_core::state::Rect::default()));
        assert!(!h.close_async());
        assert!(!h.set_border_color(None));
        assert_eq!(h.owner(), None);
        assert_eq!(h.restored_size(), None);
        assert!(!h.set_transitions(false));
        assert_eq!(h.restored_place(), None);
    }

    #[test]
    fn a_shown_windows_restored_size_is_its_size() {
        let rect = crate::msgloop::PxRect {
            x: -30_000,
            y: -30_000,
            w: 321,
            h: 123,
        };
        // Made and moved in physical pixels, as Kabuk's per-monitor aware programs do.
        let w = crate::dpi::physical(|| {
            let w = crate::msgloop::Window::shell_surface("Kabuk.Test.Size", "", rect, |_| None).unwrap();
            w.set_rect(rect);
            w
        });
        assert_eq!(w.hwnd().restored_size(), Some((321, 123)));
    }

    /// D77: a window made see-through is fully transparent and turned back opaque, as it
    /// was; twice is as once; a bogus handle does nothing.
    #[test]
    fn a_window_turns_see_through_and_back() {
        let rect = crate::msgloop::PxRect {
            x: -30_000,
            y: -30_000,
            w: 50,
            h: 50,
        };
        let w = crate::msgloop::Window::shell_surface("Kabuk.Test.SeeThrough", "", rect, |_| None).unwrap();
        let h = w.hwnd();
        assert!(!h.is_see_through());
        assert!(h.set_see_through(true));
        assert!(h.is_see_through());
        assert!(h.set_see_through(true), "twice");
        assert!(h.set_see_through(false));
        assert!(!h.is_see_through());
        assert_eq!(
            h.long(GWL_EXSTYLE) & windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED.0,
            0
        );
        assert!(h.set_see_through(false), "already opaque");
        assert!(!Hwnd(0x1234_5678).is_see_through());
    }

    /// D78: an ordinary window under another ordinary one is under an ordinary window; a
    /// topmost window raised to the top is not; a bogus handle is not.
    #[test]
    fn a_window_knows_an_ordinary_window_is_over_it() {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, SWP_NOMOVE, SWP_SHOWWINDOW,
        };
        let rect = crate::msgloop::PxRect {
            x: -30_000,
            y: -30_000,
            w: 20,
            h: 20,
        };
        // Shell surfaces made ordinary (desk surfaces keep to the desktop's level).
        let under = crate::msgloop::Window::shell_surface("Kabuk.Test.Under", "", rect, |_| None).unwrap();
        let over = crate::msgloop::Window::shell_surface("Kabuk.Test.Over", "", rect, |_| None).unwrap();
        let place = |w: Hwnd, after| {
            // SAFETY: our own windows, out of sight; shown without taking the focus.
            unsafe {
                let _ = SetWindowPos(
                    w.raw(),
                    Some(after),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        };
        place(under.hwnd(), HWND_NOTOPMOST);
        place(over.hwnd(), HWND_NOTOPMOST);
        place(over.hwnd(), HWND_TOP);
        assert!(!under.hwnd().is_topmost() && !over.hwnd().is_topmost());
        assert!(under.hwnd().under_ordinary_window(), "under the other");
        place(under.hwnd(), HWND_TOPMOST);
        assert!(under.hwnd().is_topmost());
        assert!(!under.hwnd().under_ordinary_window(), "raised to the top");
        assert!(!Hwnd(0x1234_5678).under_ordinary_window());
    }

    #[test]
    fn invisible_borders_are_added_back() {
        use kabuk_core::state::Rect;
        // Windows 11: 7 px invisible border left, right and bottom, none on top.
        let outer = Rect {
            left: 93,
            top: 100,
            right: 907,
            bottom: 707,
        };
        let visible = Rect {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        };
        let frame = Rect {
            left: 10,
            top: 75,
            right: 1270,
            bottom: 1370,
        };
        assert_eq!(
            with_invisible_borders(frame, outer, visible),
            Rect {
                left: 3,
                top: 75,
                right: 1277,
                bottom: 1377
            }
        );
        // No invisible border: unchanged.
        assert_eq!(with_invisible_borders(frame, visible, visible), frame);
    }

    #[test]
    fn enumerates_windows() {
        let all = top_level_windows();
        assert!(!all.is_empty());
        assert!(all.iter().any(|w| w.exists()));
        assert!(windows_of_class("Kabuk.NoSuchClass.42").is_empty());
        assert_eq!(find_message_window("Kabuk.NoSuchClass.42", "x"), None);
    }

    #[test]
    fn message_windows_are_found_by_class() {
        let w = crate::msgloop::Window::new("Kabuk.Test.FindMessage", "", |_| None).unwrap();
        let found = message_windows_of_class("Kabuk.Test.FindMessage");
        assert!(found.contains(&w.hwnd()));
        assert!(message_windows_of_class("Kabuk.Test.NoSuchClass").is_empty());
    }

    #[test]
    fn front_windows_leave_out_our_own_and_the_shell() {
        let w = crate::msgloop::Window::shell_surface(
            "Kabuk.Test.Front",
            "",
            crate::msgloop::PxRect {
                x: 10,
                y: 10,
                w: 50,
                h: 50,
            },
            |_| None,
        )
        .unwrap();
        w.show();
        assert!(
            !front_window_rects(std::process::id()).contains(&kabuk_core::state::Rect {
                left: 10,
                top: 10,
                right: 60,
                bottom: 60
            })
        );
    }
}
