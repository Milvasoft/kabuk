//! `kabuk-testapp.exe`: a window that imitates troublesome third-party windows, for
//! the integration tests. Prints `hwnd=<n>` (and `dialog=<n>`) on stdout once created.
//!
//! This is a test fixture standing in for other people's programs, so it calls the
//! `windows` crate directly instead of going through `kabuk-win`.
//!
//! Options:
//!   --title <text>         window title (default "Kabuk test window")
//!   --size <W>x<H>         initial size
//!   --min-size <W>x<H>     minimum size enforced through WM_GETMINMAXINFO
//!   --hidden               create without showing
//!   --maximized            show maximised
//!   --refuse-move          ignore every move and resize request
//!   --hang-after <ms>      stop pumping messages after this long (a hung window)
//!   --resize-self <ms>     change its own size on this interval
//!   --cloak-after <ms>     cloak itself (as on another virtual desktop) after this long
//!   --tray-log <path>      add a tray icon (tooltip = title, version 4) and append each
//!                          callback message to <path> as "<wparam> <lparam>"
//!   --fullscreen           borderless window covering the primary monitor, above the
//!                          others as a game is (so a maximised window of the user's in
//!                          front does not decide the tests)
//!   --splash               small borderless popup, like a splash screen
//!   --dialog               also open an owned dialog-like window
//!   --tool                 tool window (WS_EX_TOOLWINDOW)
//!   --dpi-unaware          create the window DPI unaware (scaled by Windows), like
//!                          older programs
//!
//! Posting `FAKE_DRAG` (`WM_APP + 2`, wparam = dx, lparam = dy as signed values) makes the
//! window report a title-bar drag as Windows' move loop does: `EVENT_SYSTEM_MOVESIZESTART`,
//! a move by (dx, dy), `EVENT_SYSTEM_MOVESIZEEND`. No mouse input is involved.
//! `FAKE_RESIZE` (`WM_APP + 3`, wparam = left edge's move, lparam = right edge's move)
//! reports a resize by those edges the same way.

use std::io::Write;
use std::sync::OnceLock;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: &str = "KabukTestWindow";
const DEFAULT_TITLE: &str = "Kabuk test window";
const DEFAULT_SIZE: (i32, i32) = (640, 480);
const SPLASH_SIZE: (i32, i32) = (400, 240);
const DIALOG_SIZE: (i32, i32) = (360, 200);
const TIMER_HANG: usize = 1;
const TIMER_RESIZE: usize = 2;
const TIMER_CLOAK: usize = 3;
/// The tray icon's callback message and id.
const TRAY_CALLBACK: u32 = WM_APP + 1;
const FAKE_DRAG: u32 = WM_APP + 2;
const FAKE_RESIZE: u32 = WM_APP + 3;
const TRAY_ID: u32 = 1;
/// Sizes the self-resizing window alternates between.
const RESIZE_STEPS: [(i32, i32); 2] = [(500, 400), (700, 520)];

#[derive(Default)]
struct Options {
    title: Option<String>,
    size: Option<(i32, i32)>,
    min_size: Option<(i32, i32)>,
    hidden: bool,
    maximized: bool,
    refuse_move: bool,
    hang_after_ms: Option<u32>,
    resize_self_ms: Option<u32>,
    cloak_after_ms: Option<u32>,
    tray_log: Option<String>,
    fullscreen: bool,
    splash: bool,
    dialog: bool,
    tool: bool,
    dpi_unaware: bool,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();

fn parse_size(text: &str) -> Option<(i32, i32)> {
    let (w, h) = text.split_once(['x', 'X'])?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn parse() -> Result<Options, String> {
    let mut o = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--title" => o.title = Some(value()?),
            "--size" => o.size = Some(parse_size(&value()?).ok_or("bad --size")?),
            "--min-size" => o.min_size = Some(parse_size(&value()?).ok_or("bad --min-size")?),
            "--hidden" => o.hidden = true,
            "--maximized" => o.maximized = true,
            "--refuse-move" => o.refuse_move = true,
            "--hang-after" => o.hang_after_ms = Some(value()?.parse().map_err(|_| "bad --hang-after")?),
            "--resize-self" => o.resize_self_ms = Some(value()?.parse().map_err(|_| "bad --resize-self")?),
            "--cloak-after" => o.cloak_after_ms = Some(value()?.parse().map_err(|_| "bad --cloak-after")?),
            "--tray-log" => o.tray_log = Some(value()?),
            "--fullscreen" => o.fullscreen = true,
            "--splash" => o.splash = true,
            "--dialog" => o.dialog = true,
            "--tool" => o.tool = true,
            "--dpi-unaware" => o.dpi_unaware = true,
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(o)
}

fn main() {
    let options = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let _ = OPTIONS.set(options);
    let o = OPTIONS.get().expect("set above");
    run(o);
}

fn run(o: &Options) {
    // SAFETY: (whole function) plain Win32 calls with valid arguments; the window
    // procedure only reads the immutable OPTIONS.
    unsafe {
        let instance = GetModuleHandleW(PCWSTR::null()).expect("module handle");
        let class = HSTRING::from(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        if o.dpi_unaware {
            // A window takes the awareness of the thread that creates it.
            let _ = windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
                windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_UNAWARE,
            );
        }

        let (mut style, mut ex_style) = (WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE(0));
        let (mut x, mut y) = (CW_USEDEFAULT, CW_USEDEFAULT);
        let (mut w, mut h) = o.size.unwrap_or(DEFAULT_SIZE);
        if o.fullscreen {
            style = WS_POPUP;
            ex_style |= WS_EX_TOPMOST;
            (x, y) = (0, 0);
            (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        } else if o.splash {
            style = WS_POPUP | WS_BORDER;
            (w, h) = o.size.unwrap_or(SPLASH_SIZE);
            x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        }
        if o.tool {
            ex_style |= WS_EX_TOOLWINDOW;
        }
        let title = HSTRING::from(o.title.as_deref().unwrap_or(DEFAULT_TITLE));
        let hwnd = CreateWindowExW(
            ex_style,
            &class,
            &title,
            style,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create window");
        if o.maximized {
            let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
        } else if !o.hidden {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
        println!("hwnd={}", hwnd.0 as usize);

        if o.dialog {
            let dialog_title = HSTRING::from(format!("{} dialog", o.title.as_deref().unwrap_or(DEFAULT_TITLE)));
            let (dw, dh) = DIALOG_SIZE;
            let dialog = CreateWindowExW(
                WS_EX_DLGMODALFRAME,
                &class,
                &dialog_title,
                WS_POPUP | WS_CAPTION | WS_SYSMENU,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                dw,
                dh,
                Some(hwnd),
                None,
                Some(instance.into()),
                None,
            )
            .expect("create dialog");
            let _ = ShowWindow(dialog, SW_SHOWNA);
            println!("dialog={}", dialog.0 as usize);
        }
        let _ = std::io::stdout().flush();

        if let Some(ms) = o.hang_after_ms {
            SetTimer(Some(hwnd), TIMER_HANG, ms, None);
        }
        if o.tray_log.is_some() {
            tray_icon(hwnd, o, windows::Win32::UI::Shell::NIM_ADD);
            tray_icon(hwnd, o, windows::Win32::UI::Shell::NIM_SETVERSION);
        }
        if let Some(ms) = o.cloak_after_ms {
            SetTimer(Some(hwnd), TIMER_CLOAK, ms, None);
        }
        if let Some(ms) = o.resize_self_ms {
            SetTimer(Some(hwnd), TIMER_RESIZE, ms, None);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Adds, versions or deletes the test tray icon.
unsafe fn tray_icon(hwnd: HWND, o: &Options, op: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE) {
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NOTIFYICONDATAW, NOTIFYICONDATAW_0, NOTIFYICON_VERSION_4,
    };
    // SAFETY: (whole function) a fully initialised NOTIFYICONDATAW for our own window.
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK,
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            Anonymous: NOTIFYICONDATAW_0 {
                uVersion: NOTIFYICON_VERSION_4,
            },
            ..Default::default()
        };
        let tip: Vec<u16> = o
            .title
            .as_deref()
            .unwrap_or(DEFAULT_TITLE)
            .encode_utf16()
            .take(127)
            .collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(op, &data);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let o = OPTIONS.get().expect("options are set before any window exists");
    // SAFETY: (whole function) message parameters are interpreted as documented for each message.
    unsafe {
        match msg {
            WM_GETMINMAXINFO => {
                if let Some((w, h)) = o.min_size {
                    let info = &mut *(lparam.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize.x = w;
                    info.ptMinTrackSize.y = h;
                    return LRESULT(0);
                }
            }
            WM_WINDOWPOSCHANGING if o.refuse_move => {
                let pos = &mut *(lparam.0 as *mut WINDOWPOS);
                pos.flags |= SWP_NOMOVE | SWP_NOSIZE;
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_CLOAK => {
                let _ = KillTimer(Some(hwnd), TIMER_CLOAK);
                let cloak = windows::core::BOOL(1);
                let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                    hwnd,
                    windows::Win32::Graphics::Dwm::DWMWA_CLOAK,
                    (&cloak as *const windows::core::BOOL).cast(),
                    std::mem::size_of::<windows::core::BOOL>() as u32,
                );
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_HANG => {
                // Never return: the window stops answering, like a hung program.
                loop {
                    Sleep(u32::MAX);
                }
            }
            WM_TIMER if wparam.0 == TIMER_RESIZE => {
                static STEP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let i = STEP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % RESIZE_STEPS.len();
                let (w, h) = RESIZE_STEPS[i];
                let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                return LRESULT(0);
            }
            // Like real programs: add the icon again when a tray asks.
            m if o.tray_log.is_some() && m == RegisterWindowMessageW(&HSTRING::from("TaskbarCreated")) => {
                tray_icon(hwnd, o, windows::Win32::UI::Shell::NIM_ADD);
                tray_icon(hwnd, o, windows::Win32::UI::Shell::NIM_SETVERSION);
                return LRESULT(0);
            }
            FAKE_DRAG => {
                use windows::Win32::UI::Accessibility::NotifyWinEvent;
                NotifyWinEvent(EVENT_SYSTEM_MOVESIZESTART, hwnd, OBJID_WINDOW.0, CHILDID_SELF as i32);
                let mut r = windows::Win32::Foundation::RECT::default();
                if GetWindowRect(hwnd, &mut r).is_ok() {
                    let (dx, dy) = (wparam.0 as isize as i32, lparam.0 as i32);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left + dx,
                        r.top + dy,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                NotifyWinEvent(EVENT_SYSTEM_MOVESIZEEND, hwnd, OBJID_WINDOW.0, CHILDID_SELF as i32);
                return LRESULT(0);
            }
            FAKE_RESIZE => {
                use windows::Win32::UI::Accessibility::NotifyWinEvent;
                NotifyWinEvent(EVENT_SYSTEM_MOVESIZESTART, hwnd, OBJID_WINDOW.0, CHILDID_SELF as i32);
                let mut r = windows::Win32::Foundation::RECT::default();
                if GetWindowRect(hwnd, &mut r).is_ok() {
                    let (left, right) = (wparam.0 as isize as i32, lparam.0 as i32);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        r.left + left,
                        r.top,
                        r.right + right - r.left - left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                NotifyWinEvent(EVENT_SYSTEM_MOVESIZEEND, hwnd, OBJID_WINDOW.0, CHILDID_SELF as i32);
                return LRESULT(0);
            }
            m if m == TRAY_CALLBACK => {
                if let Some(path) = &o.tray_log {
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                        let _ = writeln!(f, "{} {}", wparam.0, lparam.0);
                    }
                }
                return LRESULT(0);
            }
            WM_DESTROY => {
                if o.tray_log.is_some() && GetWindow(hwnd, GW_OWNER).is_err() {
                    tray_icon(hwnd, o, windows::Win32::UI::Shell::NIM_DELETE);
                }
                if GetWindow(hwnd, GW_OWNER).is_err() {
                    PostQuitMessage(0);
                }
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
