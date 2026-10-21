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
//!   --minimized            show minimised (without activating)
//!   --title-after <ms>     open without a title and take --title after this long
//!   --refuse-move          ignore every move and resize request
//!   --hang-after <ms>      stop pumping messages after this long (a hung window)
//!   --resize-self <ms>     change its own size on this interval
//!   --cloak-after <ms>     cloak itself (as on another virtual desktop) after this long
//!   --tray-log <path>      add a tray icon (tooltip = title, version 4) and append each
//!                          callback message to <path> as "<wparam> <lparam>"; after a
//!                          click asks the tray where its icon is and appends
//!                          "rect <left> <top> <right> <bottom>"
//!   --tray-menu-at-corner  with --tray-log, opens its menu at the screen's corner (0, 0)
//!                          after a click, as Explorer's Bluetooth icon does under Kabuk
//!   --fullscreen-after <ms> open as an ordinary window and go full screen (borderless,
//!                          covering the primary monitor) after this long, as a game does
//!   --fullscreen           borderless window covering the primary monitor, above the
//!                          others as a game is (so a maximised window of the user's in
//!                          front does not decide the tests)
//!   --overlay              a small see-through window that clicks pass through, above
//!                          the others in the middle of the primary monitor, as a graphics
//!                          driver's or a voice chat's overlay over a game
//!   --splash               small borderless popup, like a splash screen
//!   --dialog               also open an owned dialog-like window
//!   --tool                 tool window (WS_EX_TOOLWINDOW)
//!   --topmost              an ordinary window above the others, as a launcher or an app
//!                          keeping itself on top comes up over a game
//!   --app-window           asks for a taskbar button (WS_EX_APPWINDOW), as many games do
//!   --dpi-unaware          create the window DPI unaware (scaled by Windows), like
//!                          older programs
//!
//! Posting `FAKE_DRAG` (`WM_APP + 2`, wparam = dx, lparam = dy as signed values) makes the
//! window report a title-bar drag as Windows' move loop does: `EVENT_SYSTEM_MOVESIZESTART`,
//! a move by (dx, dy), `EVENT_SYSTEM_MOVESIZEEND`. No mouse input is involved.
//! `FAKE_RESIZE` (`WM_APP + 3`, wparam = left edge's move, lparam = right edge's move)
//! reports a resize by those edges the same way.
//! `OPEN_MENU` (`WM_APP + 4`) opens a popup menu at the window's corner, as a program's
//! tray icon menu (`TrackPopupMenu`), and `CLOSE_MENU` (`WM_APP + 5`) closes it.

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
/// How opaque the overlay is (it shows, and clicks still pass through).
const OVERLAY_ALPHA: u8 = 160;
const DIALOG_SIZE: (i32, i32) = (360, 200);
const TIMER_HANG: usize = 1;
const TIMER_RESIZE: usize = 2;
const TIMER_CLOAK: usize = 3;
const TIMER_TITLE: usize = 4;
const TIMER_FULLSCREEN: usize = 5;
/// The tray icon's callback message and id.
const TRAY_CALLBACK: u32 = WM_APP + 1;
const FAKE_DRAG: u32 = WM_APP + 2;
const FAKE_RESIZE: u32 = WM_APP + 3;
const OPEN_MENU: u32 = WM_APP + 4;
const CLOSE_MENU: u32 = WM_APP + 5;
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
    minimized: bool,
    title_after_ms: Option<u32>,
    refuse_move: bool,
    hang_after_ms: Option<u32>,
    resize_self_ms: Option<u32>,
    cloak_after_ms: Option<u32>,
    tray_log: Option<String>,
    tray_menu_at_corner: bool,
    fullscreen: bool,
    fullscreen_after_ms: Option<u32>,
    overlay: bool,
    splash: bool,
    dialog: bool,
    tool: bool,
    topmost: bool,
    app_window: bool,
    dpi_unaware: bool,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();

/// Opens a popup menu at screen point (x, y), as a tray icon's program does: in front,
/// then the menu until it is chosen from or closed (`CLOSE_MENU`). `hwnd` is this
/// program's window, on this thread.
fn open_menu(hwnd: HWND, x: i32, y: i32) {
    // SAFETY: the menu is created, shown and destroyed here; `item` outlives the append.
    unsafe {
        if let Ok(menu) = CreatePopupMenu() {
            let item = HSTRING::from("Test item");
            let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(item.as_ptr()));
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, x, y, None, hwnd, None);
            let _ = DestroyMenu(menu);
        }
    }
}

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
            "--minimized" => o.minimized = true,
            "--title-after" => o.title_after_ms = Some(value()?.parse().map_err(|_| "bad --title-after")?),
            "--refuse-move" => o.refuse_move = true,
            "--hang-after" => o.hang_after_ms = Some(value()?.parse().map_err(|_| "bad --hang-after")?),
            "--resize-self" => o.resize_self_ms = Some(value()?.parse().map_err(|_| "bad --resize-self")?),
            "--cloak-after" => o.cloak_after_ms = Some(value()?.parse().map_err(|_| "bad --cloak-after")?),
            "--tray-log" => o.tray_log = Some(value()?),
            "--tray-menu-at-corner" => o.tray_menu_at_corner = true,
            "--fullscreen" => o.fullscreen = true,
            "--fullscreen-after" => {
                o.fullscreen_after_ms = Some(value()?.parse().map_err(|_| "bad --fullscreen-after")?)
            }
            "--overlay" => o.overlay = true,
            "--splash" => o.splash = true,
            "--dialog" => o.dialog = true,
            "--tool" => o.tool = true,
            "--topmost" => o.topmost = true,
            "--app-window" => o.app_window = true,
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
        } else if o.overlay {
            style = WS_POPUP;
            ex_style |= WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT;
            (w, h) = o.size.unwrap_or(SPLASH_SIZE);
            x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        } else if o.splash {
            style = WS_POPUP | WS_BORDER;
            (w, h) = o.size.unwrap_or(SPLASH_SIZE);
            x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        }
        if o.tool {
            ex_style |= WS_EX_TOOLWINDOW;
        }
        if o.topmost {
            ex_style |= WS_EX_TOPMOST;
        }
        if o.app_window {
            ex_style |= WS_EX_APPWINDOW;
        }
        let title = if o.title_after_ms.is_some() {
            HSTRING::new()
        } else {
            HSTRING::from(o.title.as_deref().unwrap_or(DEFAULT_TITLE))
        };
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
        if o.overlay {
            let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), OVERLAY_ALPHA, LWA_ALPHA);
        }
        if o.maximized {
            let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
        } else if o.minimized {
            let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
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
        if let Some(ms) = o.title_after_ms {
            SetTimer(Some(hwnd), TIMER_TITLE, ms, None);
        }
        if let Some(ms) = o.fullscreen_after_ms {
            SetTimer(Some(hwnd), TIMER_FULLSCREEN, ms, None);
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
            WM_TIMER if wparam.0 == TIMER_TITLE => {
                let _ = KillTimer(Some(hwnd), TIMER_TITLE);
                let _ = SetWindowTextW(hwnd, &HSTRING::from(o.title.as_deref().unwrap_or(DEFAULT_TITLE)));
                return LRESULT(0);
            }
            WM_TIMER if wparam.0 == TIMER_FULLSCREEN => {
                // As a game going full screen: borderless, over the whole primary monitor.
                let _ = KillTimer(Some(hwnd), TIMER_FULLSCREEN);
                SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    0,
                    0,
                    GetSystemMetrics(SM_CXSCREEN),
                    GetSystemMetrics(SM_CYSCREEN),
                    SWP_FRAMECHANGED | SWP_NOACTIVATE,
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
            OPEN_MENU => {
                let mut r = windows::Win32::Foundation::RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                open_menu(hwnd, r.left + 20, r.top + 20);
                return LRESULT(0);
            }
            CLOSE_MENU => {
                let _ = EndMenu();
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
                        // Like a program opening its menu by its icon: ask the tray where
                        // the icon is, after the click's last message.
                        let msg = (lparam.0 & 0xFFFF) as u32;
                        if msg == windows::Win32::UI::Shell::NIN_SELECT || msg == WM_CONTEXTMENU {
                            use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
                            let id = NOTIFYICONIDENTIFIER {
                                cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
                                hWnd: hwnd,
                                uID: TRAY_ID,
                                ..Default::default()
                            };
                            if let Ok(r) = Shell_NotifyIconGetRect(&id) {
                                let _ = writeln!(f, "rect {} {} {} {}", r.left, r.top, r.right, r.bottom);
                            }
                            drop(f);
                            if o.tray_menu_at_corner {
                                open_menu(hwnd, 0, 0);
                            }
                        }
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
