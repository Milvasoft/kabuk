//! M4 spike (ARCHITECTURE §9): can Kabuk receive other programs' tray icons while
//! Explorer keeps running?
//!
//! **Run only by hand, with the user's go-ahead:** it makes every running program send
//! its tray icons here (it broadcasts `TaskbarCreated`). Nothing is lost if it crashes:
//! every message is forwarded to Explorer's own tray first, so Explorer keeps every icon,
//! and AppBar messages (Kabuk's bars use them) reach Explorer unchanged.
//!
//!   cargo run -p kabuk-win --example tray_spike -- [--seconds 20]
//!
//! What it does: finds Explorer's `Shell_TrayWnd`, creates a hidden topmost window of the
//! same class (programs find the topmost one), forwards every `WM_COPYDATA` to Explorer
//! and returns Explorer's answer, records the notify-icon calls, broadcasts
//! `TaskbarCreated` so programs register their icons again, prints what it saw, then
//! removes its window and broadcasts `TaskbarCreated` once more.
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

const TRAY_CLASS: &str = "Shell_TrayWnd";
/// `WM_COPYDATA` kinds sent to the tray window.
const COPYDATA_APPBAR: usize = 0;
const COPYDATA_NOTIFY_ICON: usize = 1;
/// `SHELLTRAYDATA.dwSignature`.
const TRAY_SIGNATURE: u32 = 0x3475_3423;
const NIF_TIP: u32 = 0x4;
const NIM_NAMES: [&str; 5] = ["add", "modify", "delete", "setfocus", "setversion"];
/// Programs find the topmost `Shell_TrayWnd`; Explorer may raise its own again.
const TOPMOST_TIMER: usize = 1;
const TOPMOST_EVERY_MS: u32 = 100;
const FORWARD_TIMEOUT_MS: u32 = 1000;

/// `COPYDATASTRUCT`.
#[repr(C)]
struct CopyData {
    dw_data: usize,
    cb_data: u32,
    lp_data: *const u8,
}

/// The 32-bit `NOTIFYICONDATAW` inside `SHELLTRAYDATA` (handles are 32-bit values).
#[repr(C)]
#[derive(Clone, Copy)]
struct NotifyIconData32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback_message: u32,
    hicon: u32,
    tip: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrayDataHead {
    signature: u32,
    message: u32,
    nid: NotifyIconData32,
}

#[derive(Debug, Default)]
struct Seen {
    explorer: isize,
    appbar_messages: u32,
    calls: BTreeMap<&'static str, u32>,
    /// (owner hwnd, uid) → (tooltip, owner exe, callback message)
    icons: BTreeMap<(u32, u32), (String, String, u32)>,
    forward_failures: u32,
    /// Timer ticks on which `FindWindow` found Explorer's tray instead of ours.
    lost_front: u32,
}

static mut SEEN: Option<Seen> = None;

fn seen() -> &'static mut Seen {
    // SAFETY: single-threaded spike; only the UI thread touches SEEN.
    unsafe { (*std::ptr::addr_of_mut!(SEEN)).get_or_insert_with(Seen::default) }
}

fn owner_exe(hwnd: u32) -> String {
    let h = HWND(hwnd as usize as *mut core::ffi::c_void);
    let mut pid = 0u32;
    // SAFETY: valid out pointer.
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    kabuk_win::process::Process::open(pid)
        .ok()
        .and_then(|p| p.image_name())
        .unwrap_or_else(|| format!("pid {pid}"))
}

fn record(cds: &CopyData) {
    let s = seen();
    if cds.dw_data == COPYDATA_APPBAR {
        s.appbar_messages += 1;
        return;
    }
    if cds.dw_data != COPYDATA_NOTIFY_ICON || (cds.cb_data as usize) < std::mem::size_of::<TrayDataHead>() {
        return;
    }
    // SAFETY: cb_data covers the head; read unaligned from the sender's copied buffer.
    let head: TrayDataHead = unsafe { std::ptr::read_unaligned(cds.lp_data.cast()) };
    if head.signature != TRAY_SIGNATURE {
        return;
    }
    let name = NIM_NAMES.get(head.message as usize).copied().unwrap_or("other");
    *s.calls.entry(name).or_default() += 1;
    let key = (head.nid.hwnd, head.nid.uid);
    match name {
        "delete" => {
            s.icons.remove(&key);
        }
        "add" | "modify" => {
            let entry = s
                .icons
                .entry(key)
                .or_insert_with(|| (String::new(), owner_exe(head.nid.hwnd), 0));
            if head.nid.flags & NIF_TIP != 0 {
                let end = head.nid.tip.iter().position(|c| *c == 0).unwrap_or(head.nid.tip.len());
                entry.0 = String::from_utf16_lossy(&head.nid.tip[..end]);
            }
            if head.nid.callback_message != 0 {
                entry.2 = head.nid.callback_message;
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: (whole function) parameters are used as documented for each message.
    unsafe {
        match msg {
            WM_COPYDATA => {
                let cds = &*(lparam.0 as *const CopyData);
                record(cds);
                // Explorer answers; its answer is the caller's result.
                let explorer = HWND(seen().explorer as *mut core::ffi::c_void);
                let mut result = 0usize;
                let ok = SendMessageTimeoutW(
                    explorer,
                    WM_COPYDATA,
                    wparam,
                    lparam,
                    SMTO_ABORTIFHUNG,
                    FORWARD_TIMEOUT_MS,
                    Some(&mut result),
                );
                if ok.0 == 0 {
                    seen().forward_failures += 1;
                }
                LRESULT(result as isize)
            }
            WM_TIMER if wparam.0 == TOPMOST_TIMER => {
                // How often does Explorer's tray get in front of ours?
                let class = HSTRING::from(TRAY_CLASS);
                if FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()).ok() != Some(hwnd) {
                    seen().lost_front += 1;
                }
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u64 = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let class = HSTRING::from(TRAY_CLASS);
    // SAFETY: (whole block) plain Win32 calls with valid arguments on this thread.
    unsafe {
        let explorer = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()).expect("Explorer's tray window");
        seen().explorer = explorer.0 as isize;
        let instance = GetModuleHandleW(PCWSTR::null()).expect("module");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let ours = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            &class,
            &HSTRING::new(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("spike tray window");
        SetTimer(Some(ours), TOPMOST_TIMER, TOPMOST_EVERY_MS, None);
        let found = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()).unwrap_or_default();
        println!(
            "Explorer tray {:?}, ours {:?}, FindWindow finds {}",
            explorer,
            ours,
            if found == ours { "ours" } else { "Explorer's" }
        );

        let taskbar_created = RegisterWindowMessageW(&HSTRING::from("TaskbarCreated"));
        let _ = SendNotifyMessageW(HWND_BROADCAST, taskbar_created, WPARAM(0), LPARAM(0));
        println!("TaskbarCreated broadcast; listening for {seconds} s");

        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut msg = MSG::default();
        while Instant::now() < deadline {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            MsgWaitForMultipleObjects(
                None,
                false,
                left.as_millis().min(u128::from(u32::MAX)) as u32,
                QS_ALLINPUT,
            );
        }

        let _ = DestroyWindow(ours);
        let _ = SendNotifyMessageW(HWND_BROADCAST, taskbar_created, WPARAM(0), LPARAM(0));
    }
    let s = seen();
    println!("\nAppBar messages forwarded: {}", s.appbar_messages);
    println!("Notify-icon calls: {:?}", s.calls);
    println!("Forward failures: {}", s.forward_failures);
    println!("Times Explorer's tray was in front of ours: {}", s.lost_front);
    println!("Icons ({}):", s.icons.len());
    for ((hwnd, uid), (tip, exe, cb)) in &s.icons {
        println!("  {exe:<28} hwnd={hwnd:#x} uid={uid} callback={cb:#x} tip={tip:?}");
    }
    println!("\nWindow removed and TaskbarCreated broadcast again; programs re-register with Explorer.");
}
