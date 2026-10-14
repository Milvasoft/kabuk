//! The settings app's window (DECISIONS D3): a normal application window holding a
//! WebView2 that shows pages served from memory at `https://kabuk.settings/`, and a
//! message channel between the page (`window.chrome.webview`) and Rust.
//!
//! The WebView2 runtime ships with Windows 11; its browser processes end with this
//! process, so a closed settings window costs nothing.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller, ICoreWebView2Environment,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
};
use webview2_com::{
    CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    WebMessageReceivedEventHandler, WebResourceRequestedEventHandler,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetWindowLongPtrW, LoadCursorW,
    LoadIconW, PostMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_APP, WM_DESTROY, WM_SIZE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

use crate::window::Hwnd;

/// Where the pages live.
pub const ORIGIN: &str = "https://kabuk.settings/";

/// A file the page asks for.
pub struct Resource {
    pub bytes: &'static [u8],
    pub mime: &'static str,
}

fn err(e: impl std::fmt::Debug) -> std::io::Error {
    std::io::Error::other(format!("{e:?}"))
}

struct State {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    poster: Poster,
}

/// Tells the window that [`Poster`] has messages waiting.
const WM_POST: u32 = WM_APP + 1;

/// Sends messages to the page from any thread (work that runs on its own thread, such
/// as creating a wallpaper): they are queued and the window's thread passes them on.
#[derive(Clone, Default)]
pub struct Poster(Arc<PosterInner>);

#[derive(Default)]
struct PosterInner {
    hwnd: AtomicIsize,
    queue: Mutex<VecDeque<String>>,
}

impl Poster {
    /// Queues `text` for the page; sent once the window exists.
    pub fn post(&self, text: String) {
        if let Ok(mut q) = self.0.queue.lock() {
            q.push_back(text);
        }
        let h = self.0.hwnd.load(Ordering::Acquire);
        if h != 0 {
            // SAFETY: a message to our own window; a gone window makes the call fail.
            let _ = unsafe { PostMessageW(Some(HWND(h as *mut _)), WM_POST, WPARAM(0), LPARAM(0)) };
        }
    }

    fn take(&self) -> Vec<String> {
        self.0
            .queue
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default()
    }
}

/// The window and its WebView, until the user closes it ([`Settings::run`]).
pub struct Settings {
    hwnd: HWND,
    webview: ICoreWebView2,
    _controller: ICoreWebView2Controller,
    _state: Rc<RefCell<State>>,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds 0 or the `Rc<RefCell<State>>` pointer set in `open`,
    // which lives as long as the window.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<State>;
    match msg {
        WM_SIZE if !state.is_null() => {
            crate::msgloop::contain((), || {
                // SAFETY: see above; borrowed only for the call.
                let s = unsafe { &*state };
                if let Ok(s) = s.try_borrow() {
                    if let Some(c) = &s.controller {
                        let mut r = RECT::default();
                        // SAFETY: our own window; `r` is a valid out pointer.
                        unsafe {
                            let _ = GetClientRect(hwnd, &mut r);
                            let _ = c.SetBounds(r);
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_POST if !state.is_null() => {
            crate::msgloop::contain((), || {
                // SAFETY: see above; borrowed only for the call.
                let s = unsafe { &*state };
                if let Ok(s) = s.try_borrow() {
                    if let Some(w) = &s.webview {
                        for text in s.poster.take() {
                            // SAFETY: a valid string for the call, on the WebView's thread.
                            let _ = unsafe { w.PostWebMessageAsString(&HSTRING::from(text)) };
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default processing with the arguments we received.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Creates the window: `resources` serves `https://kabuk.settings/<path>`, `on_message`
/// gets each message the page posts and may answer with one; `poster` sends more from
/// other threads. COM must be initialised (apartment-threaded) on this thread;
/// `data_dir` holds WebView2's profile.
pub fn open(
    class: &str,
    title: &str,
    size: (i32, i32),
    data_dir: &std::path::Path,
    resources: impl Fn(&str) -> Option<Resource> + 'static,
    on_message: impl Fn(&str) -> Option<String> + 'static,
    poster: &Poster,
) -> std::io::Result<Settings> {
    let class = HSTRING::from(class);
    // SAFETY: (whole block) plain window creation with a valid class and our procedure.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).map_err(crate::sync::io_error)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // The program's own icon (resource 1: Kabuk's logo).
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(std::ptr::without_provenance(1))).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &class,
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            size.0,
            size.1,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(crate::sync::io_error)?
    };

    let data = HSTRING::from(data_dir.as_os_str());
    let environment: ICoreWebView2Environment = {
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                // SAFETY: the default runtime with our profile folder; the handler outlives the call.
                unsafe { CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &data, None, &handler) }
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |code, env| {
                code?;
                let _ = tx.send(env.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                Ok(())
            }),
        )
        .map_err(err)?;
        rx.recv().map_err(err)?.map_err(crate::sync::io_error)?
    };
    let controller: ICoreWebView2Controller = {
        let (tx, rx) = mpsc::channel();
        let env = environment.clone();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| {
                // SAFETY: a live environment and our window.
                unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) }.map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |code, c| {
                code?;
                let _ = tx.send(c.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                Ok(())
            }),
        )
        .map_err(err)?;
        rx.recv().map_err(err)?.map_err(crate::sync::io_error)?
    };
    // SAFETY: (whole block) COM calls on the live controller and its WebView; the handlers
    // own what they capture and are released with the WebView.
    let webview = unsafe {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        controller.SetBounds(r).map_err(crate::sync::io_error)?;
        controller.SetIsVisible(true).map_err(crate::sync::io_error)?;
        let webview = controller.CoreWebView2().map_err(crate::sync::io_error)?;

        let filter = HSTRING::from(format!("{ORIGIN}*"));
        webview
            .AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            .map_err(crate::sync::io_error)?;
        let env = environment.clone();
        let mut token = 0i64;
        webview
            .add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    let uri = CoTaskMemPWSTR::from(uri).to_string();
                    let path = uri
                        .strip_prefix(ORIGIN)
                        .unwrap_or("")
                        .split(['?', '#'])
                        .next()
                        .unwrap_or("");
                    let (status, reason, body, mime) = match resources(path) {
                        Some(r) => (200, "OK", r.bytes, r.mime),
                        None => (404, "Not Found", &b""[..], "text/plain"),
                    };
                    let stream = SHCreateMemStream(Some(body));
                    let headers = HSTRING::from(format!("Content-Type: {mime}"));
                    let response =
                        env.CreateWebResourceResponse(stream.as_ref(), status, &HSTRING::from(reason), &headers)?;
                    args.SetResponse(&response)?;
                    Ok(())
                })),
                &mut token,
            )
            .map_err(crate::sync::io_error)?;
        let replier = webview.clone();
        webview
            .add_WebMessageReceived(
                &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut text = PWSTR::null();
                    if args.TryGetWebMessageAsString(&mut text).is_ok() {
                        let text = CoTaskMemPWSTR::from(text).to_string();
                        let answer = crate::msgloop::contain(None, || on_message(&text));
                        if let Some(a) = answer {
                            replier.PostWebMessageAsString(&HSTRING::from(a))?;
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )
            .map_err(crate::sync::io_error)?;
        webview
    };
    let state = Rc::new(RefCell::new(State {
        controller: Some(controller.clone()),
        webview: Some(webview.clone()),
        poster: poster.clone(),
    }));
    // SAFETY: the pointer stays valid while the window exists: `Settings` keeps the Rc and
    // the window is destroyed before `Settings` is dropped (run returns after WM_DESTROY).
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&state) as isize) };
    poster.0.hwnd.store(hwnd.0 as isize, Ordering::Release);
    // Anything queued before the window existed goes now.
    // SAFETY: a message to our own window.
    let _ = unsafe { PostMessageW(Some(hwnd), WM_POST, WPARAM(0), LPARAM(0)) };
    Ok(Settings {
        hwnd,
        webview,
        _controller: controller,
        _state: state,
    })
}

impl Settings {
    pub fn hwnd(&self) -> Hwnd {
        Hwnd::from_raw(self.hwnd)
    }

    /// Loads `https://kabuk.settings/<page>`.
    pub fn navigate(&self, page: &str) -> std::io::Result<()> {
        // SAFETY: a valid URL string for the call.
        unsafe { self.webview.Navigate(&HSTRING::from(format!("{ORIGIN}{page}"))) }.map_err(crate::sync::io_error)
    }

    /// Sends a message to the page (`window.chrome.webview` `message` event).
    pub fn post(&self, text: &str) -> std::io::Result<()> {
        // SAFETY: a valid string for the call.
        unsafe { self.webview.PostWebMessageAsString(&HSTRING::from(text)) }.map_err(crate::sync::io_error)
    }

    /// Shows the window and runs its message loop until the user closes it.
    pub fn run(self) -> i32 {
        // SAFETY: (whole block) our window; the standard message loop of this thread.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            i32::try_from(msg.wParam.0).unwrap_or(0)
        }
    }
}
