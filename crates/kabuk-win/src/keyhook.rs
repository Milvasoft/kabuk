//! Kabuk's shortcuts (DECISIONS D40): a low-level keyboard hook (`WH_KEYBOARD_LL`, not
//! injected into any process, like the mouse hook) sees every key before Windows'
//! hotkeys and other programs' `RegisterHotKey`, so a chord of Kabuk's always reaches it
//! (Alt+Space was taken by another program in 13 of the user's 30 starts) and is
//! swallowed there, never reaching the program in front. A chord sends its window the
//! same `WM_HOTKEY` (`wparam` = id) as a registered hotkey would, so the code that acts
//! on it does not change.
//!
//! When the Win key or Alt took part in a chord, its release is masked (a key that does
//! nothing pressed in between), so Start or the program's menu does not open. The hook
//! runs on a thread of its own that does nothing else (a slow hook makes every key on
//! the machine wait); injected keys (the mask among them) pass untouched; while the
//! window in front is on a monitor a full-screen program covers (game mode) every key
//! passes, and on the other monitors the chords work as usual (SPEC §2.4).
//!
//! Settings' shortcut recorder takes the keys with a [`Capture`]: a hook of its own,
//! installed when the recorder waits (so it is the newest and sees keys before Kabuk's
//! and Windows'), which takes the next chord, Win chords included, from everyone and
//! hands it over, then goes.
//!
//! The volume keys are taken too while the bar asks for them ([`VolumeKeys`]): the bar
//! changes the volume and shows its own OSD, so Windows' flyout does not show beside it.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use kabuk_core::config::shortcut::{Chord, Mods};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, SetWindowsHookExW, TranslateMessage, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::msgloop::{virtual_key, MessageWindow};

/// A key that does nothing (unassigned), pressed to mask a modifier's release.
const MASK_KEY: u16 = 0xE8;

/// A chord bound to a window's `WM_HOTKEY` id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binding {
    token: u64,
    hwnd: isize,
    id: i32,
    mods: Mods,
    vk: u16,
}

/// The chords bound, as a snapshot the hook reads on every key without copying it
/// (`None`: none); binding or dropping a shortcut puts a new one in its place.
static BINDINGS: RwLock<Option<Arc<[Binding]>>> = RwLock::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);
static PAUSED: AtomicBool = AtomicBool::new(false);
/// The monitors (`HMONITOR` values) a full-screen program covers while [`PAUSED`].
static PAUSED_ON: Mutex<Vec<isize>> = Mutex::new(Vec::new());
static THREAD: OnceLock<Result<(), String>> = OnceLock::new();
/// The hook thread's id, for telling it to take the hook off or put it back.
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Whether the hook is on now (off while a full-screen program is in front: D85).
static HOOKED: AtomicBool = AtomicBool::new(false);
/// The hook's state last asked of its thread (`NOT_ASKED`, `ASKED_OFF` or `ASKED_ON`): the
/// next change is decided against it, not against `HOOKED`, which the thread sets only
/// once it read the request.
static ASKED: AtomicU8 = AtomicU8::new(NOT_ASKED);
const NOT_ASKED: u8 = 0;
const ASKED_OFF: u8 = 1;
const ASKED_ON: u8 = 2;
/// Thread messages of the hook thread: take the hook off, put it back.
const MSG_UNHOOK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;
const MSG_REHOOK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// A key event as the hook sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Down(u16),
    Up(u16),
}

/// What the hook does with an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Pass,
    /// Swallowed; with the binding to tell, if the key fired one.
    Swallow(Option<(isize, i32)>),
    /// Swallowed and sent again after the mask key (a modifier's release).
    Masked(u16),
    /// Swallowed and handed to the recorder (capturing).
    Captured(u16),
}

/// A volume key, as the bar's message's `wparam`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum VolumeKey {
    Up = 1,
    Down = 2,
    Mute = 3,
}

impl VolumeKey {
    fn of(vk: u16) -> Option<VolumeKey> {
        match VIRTUAL_KEY(vk) {
            VK_VOLUME_UP => Some(VolumeKey::Up),
            VK_VOLUME_DOWN => Some(VolumeKey::Down),
            VK_VOLUME_MUTE => Some(VolumeKey::Mute),
            _ => None,
        }
    }

    /// The key a message's `wparam` names.
    pub fn from_wparam(w: usize) -> Option<VolumeKey> {
        [VolumeKey::Up, VolumeKey::Down, VolumeKey::Mute]
            .into_iter()
            .find(|k| *k as usize == w)
    }
}

/// Where the hook tells something (a window and a message), read on every key without
/// a lock: both in one word, the window's handle in the high half. A window handle fits
/// in 32 bits (Windows keeps them so for 32- and 64-bit programs to share, sign-extending
/// the low half); 0 is none.
struct Sink(AtomicU64);

impl Sink {
    const fn new() -> Sink {
        Sink(AtomicU64::new(0))
    }

    fn set(&self, to: Option<(isize, u32)>) {
        self.0.store(pack_sink(to), Ordering::Release);
    }

    fn get(&self) -> Option<(isize, u32)> {
        unpack_sink(self.0.load(Ordering::Acquire))
    }
}

/// A [`Sink`]'s word for a window and a message. Pure.
fn pack_sink(to: Option<(isize, u32)>) -> u64 {
    // The low half of the handle: all of it that is significant.
    to.map_or(0, |(hwnd, message)| (u64::from(hwnd as u32) << 32) | u64::from(message))
}

/// The window and message in a [`Sink`]'s word. Pure.
fn unpack_sink(word: u64) -> Option<(isize, u32)> {
    // The handle's half sign-extended back, as Windows does.
    let hwnd = (word >> 32) as u32 as i32 as isize;
    (hwnd != 0).then_some((hwnd, word as u32))
}

/// Where the volume keys go while taken: (window, message).
static VOLUME_SINK: Sink = Sink::new();
/// Where a chord that switches the input language is told (window, message; D88).
static LANGUAGE_SINK: Sink = Sink::new();

/// `VK_SPACE`.
const SPACE: u16 = 0x20;
const SHIFT_KEYS: [VIRTUAL_KEY; 3] = [VK_SHIFT, VK_LSHIFT, VK_RSHIFT];
const ALT_AND_CTRL_KEYS: [VIRTUAL_KEY; 6] = [VK_MENU, VK_LMENU, VK_RMENU, VK_CONTROL, VK_LCONTROL, VK_RCONTROL];

/// Whether `event` may end one of Windows' input-language chords (the release of Space,
/// Shift, Alt or Ctrl): only those are checked, and only they need the keys held.
fn may_switch_language(event: Event) -> bool {
    let Event::Up(vk) = event else { return false };
    vk == SPACE || SHIFT_KEYS.iter().chain(&ALT_AND_CTRL_KEYS).any(|k| k.0 == vk)
}

/// Whether `event` ends one of Windows' input-language chords: Win+Space, Alt+Shift or
/// Ctrl+Shift (whichever key is let go last), with `mods` the keys held then.
fn language_switch(event: Event, mods: Mods) -> bool {
    let Event::Up(vk) = event else { return false };
    let is = |keys: &[VIRTUAL_KEY]| keys.iter().any(|k| k.0 == vk);
    (vk == SPACE && mods.win) || (is(&SHIFT_KEYS) && (mods.alt || mods.ctrl)) || (is(&ALT_AND_CTRL_KEYS) && mods.shift)
}

/// What the hook remembers between events.
#[derive(Debug, Default)]
struct KeyState {
    /// Keys whose press fired a chord: their repeats and release are swallowed too.
    held: HashSet<u16>,
    used_win: bool,
    used_alt: bool,
}

fn is_win(vk: u16) -> bool {
    vk == VK_LWIN.0 || vk == VK_RWIN.0
}

fn is_alt(vk: u16) -> bool {
    vk == VK_MENU.0 || vk == VK_LMENU.0 || vk == VK_RMENU.0
}

fn is_modifier(vk: u16) -> bool {
    is_win(vk)
        || is_alt(vk)
        || [VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_SHIFT, VK_LSHIFT, VK_RSHIFT]
            .iter()
            .any(|k| k.0 == vk)
}

/// Whether the hook takes `vk` even when a program injected it: the volume keys, which
/// keyboard software sends that way (found on the user's machine: the laptop's Fn
/// volume keys come injected, so Windows' flyout showed beside Kabuk's OSD). Kabuk
/// itself never injects them; every other injected key passes (its own masks too).
fn taken_when_injected(vk: u16) -> bool {
    VolumeKey::of(vk).is_some()
}

/// A volume key's event while the bar takes them, pure: `None` when it is not one.
/// Up and Down repeat while held (as Windows steps the volume); Mute acts once per press.
/// Every event of a volume key is swallowed (`Some(None)` tells nothing).
fn decide_volume(event: Event, state: &mut KeyState) -> Option<Option<VolumeKey>> {
    match event {
        Event::Down(vk) => {
            let key = VolumeKey::of(vk)?;
            if key == VolumeKey::Mute && !state.held.insert(vk) {
                return Some(None);
            }
            Some(Some(key))
        }
        Event::Up(vk) => {
            VolumeKey::of(vk)?;
            state.held.remove(&vk);
            Some(None)
        }
    }
}

/// [`decide_as`] for Kabuk's own chords.
fn decide(event: Event, mods: Mods, bindings: &[Binding], state: &mut KeyState) -> Outcome {
    decide_as(event, mods, bindings, state, false)
}

/// The decision for one event, pure: `mods` are the modifiers held as it happens; while
/// `capturing`, every key Kabuk's chords can have is taken for the recorder.
fn decide_as(event: Event, mods: Mods, bindings: &[Binding], state: &mut KeyState, capturing: bool) -> Outcome {
    match event {
        Event::Down(vk) if is_modifier(vk) => Outcome::Pass,
        Event::Down(vk) => {
            if state.held.contains(&vk) {
                // A repeat of a key that fired: once per press, as MOD_NOREPEAT.
                return Outcome::Swallow(None);
            }
            if capturing && crate::msgloop::key_of_virtual(vk).is_some() {
                state.held.insert(vk);
                state.used_win |= mods.win;
                state.used_alt |= mods.alt;
                return Outcome::Captured(vk);
            }
            match bindings.iter().find(|b| b.vk == vk && b.mods == mods) {
                Some(b) => {
                    state.held.insert(vk);
                    state.used_win |= mods.win;
                    state.used_alt |= mods.alt;
                    Outcome::Swallow(Some((b.hwnd, b.id)))
                }
                None => Outcome::Pass,
            }
        }
        Event::Up(vk) if is_win(vk) && state.used_win => {
            state.used_win = false;
            Outcome::Masked(vk)
        }
        Event::Up(vk) if is_alt(vk) && state.used_alt => {
            state.used_alt = false;
            Outcome::Masked(vk)
        }
        Event::Up(vk) => {
            if state.held.remove(&vk) {
                Outcome::Swallow(None)
            } else {
                Outcome::Pass
            }
        }
    }
}

fn down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: plain query.
    (unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000) != 0
}

fn mods_now() -> Mods {
    Mods {
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        shift: down(VK_SHIFT),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

/// The chords bound now: the snapshot itself, not a copy.
fn bindings() -> Option<Arc<[Binding]>> {
    BINDINGS.read().ok().and_then(|b| b.clone())
}

/// Puts a new snapshot of the chords in place, `change` made to the current one.
fn change_bindings(change: impl FnOnce(&mut Vec<Binding>)) -> std::io::Result<()> {
    let mut bound = BINDINGS
        .write()
        .map_err(|_| std::io::Error::other("shortcut table poisoned"))?;
    let mut list = bound.as_deref().map(<[Binding]>::to_vec).unwrap_or_default();
    change(&mut list);
    *bound = (!list.is_empty()).then(|| list.into());
    Ok(())
}

/// Presses and releases the mask key, then releases `vk` (all injected, so the hook
/// passes them).
fn send_masked(vk: u16) {
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    };
    let inputs = [key(MASK_KEY, false), key(MASK_KEY, true), key(vk, true)];
    // SAFETY: fully initialised keyboard inputs and their size.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        tracing::debug!(sent, "masked release not sent");
    }
}

thread_local! {
    static STATE: std::cell::RefCell<KeyState> = std::cell::RefCell::new(KeyState::default());
}

/// Whether keys pass untouched: a full-screen program covers the monitor of the window
/// in front (`None`: no window in front, as a game may be). Pure.
fn paused_for(front: Option<isize>, paused_on: &[isize]) -> bool {
    !paused_on.is_empty() && front.is_none_or(|m| paused_on.contains(&m))
}

/// The monitor of the window in front.
fn front_monitor() -> Option<isize> {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONULL};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: plain queries; no window or monitor gives a null handle.
    let m = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONULL) };
    (!m.is_invalid()).then_some(m.0 as isize)
}

/// Game mode, decided per key while the hook is on (the window in front may have just
/// moved to a full-screen monitor, before the hook is taken off).
fn paused_now() -> bool {
    if !PAUSED.load(Ordering::Relaxed) {
        return false;
    }
    let front = front_monitor();
    PAUSED_ON.lock().is_ok_and(|p| paused_for(front, &p))
}

/// Takes the hook off while a full-screen program is in front, and puts it back once it
/// is not (D85, found by the user: games stuttered). With the hook on, Windows waits for
/// Kabuk's answer to every key the game gets; off, the game's keys never reach Kabuk.
/// The emergency exit stays: it is a hotkey Windows holds, not this hook's.
fn update_hook() {
    let want = !paused_now();
    let id = THREAD_ID.load(Ordering::Relaxed);
    if id == 0 {
        return;
    }
    let asked = if want { ASKED_ON } else { ASKED_OFF };
    let before = ASKED.swap(asked, Ordering::Relaxed);
    if !must_ask(before, asked, HOOKED.load(Ordering::Relaxed)) {
        return;
    }
    use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
    let msg = if want { MSG_REHOOK } else { MSG_UNHOOK };
    // SAFETY: a message to the hook thread of this process, which reads it in its loop.
    let _ = unsafe { PostThreadMessageW(id, msg, WPARAM(0), LPARAM(0)) };
}

/// Whether the hook's thread must be asked for `asked` (found by a test: off then on in
/// quick turn, the "on" was never sent, as the thread had not yet read the "off" and the
/// hook still counted as on; it stayed off until the next change of focus).
fn must_ask(before: u8, asked: u8, hooked: bool) -> bool {
    match before {
        NOT_ASKED => (asked == ASKED_ON) != hooked,
        _ => before != asked,
    }
}

/// The window in front changed: the hook comes off over a full-screen program and back
/// elsewhere (D85).
pub fn front_changed() {
    update_hook();
}

/// Whether the keyboard hook is on now (off while a full-screen program is in front).
pub fn is_hooked() -> bool {
    HOOKED.load(Ordering::Relaxed)
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pass = || {
        // SAFETY: passes the event on with the arguments received.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    };
    if code < 0 || crate::msgloop::contain(false, paused_now) {
        return pass();
    }
    // SAFETY: for WH_KEYBOARD_LL, lparam points to the event's KBDLLHOOKSTRUCT.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let vk = u16::try_from(info.vkCode).unwrap_or(0);
    if info.flags.0 & LLKHF_INJECTED.0 != 0 && !taken_when_injected(vk) {
        return pass();
    }
    let event = match wparam.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Event::Down(vk),
        WM_KEYUP | WM_SYSKEYUP => Event::Up(vk),
        _ => return pass(),
    };
    // Windows waits for this answer: the keys held are read once, and only when needed.
    let mut held = None;
    let mut mods = || *held.get_or_insert_with(mods_now);
    // Only watched, never taken: the chord goes on to Windows, which switches.
    if let Some((hwnd, message)) = LANGUAGE_SINK.get().filter(|_| may_switch_language(event)) {
        if crate::msgloop::contain(false, || language_switch(event, mods())) {
            // SAFETY: a message to a window of Kabuk's; a gone window makes it fail.
            let _ = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0)) };
        }
    }
    if let Some((hwnd, message)) = VOLUME_SINK.get() {
        let volume = crate::msgloop::contain(None, || STATE.with(|s| decide_volume(event, &mut s.borrow_mut())));
        if let Some(told) = volume {
            if let Some(key) = told {
                // SAFETY: a message to a window of Kabuk's; a gone window makes it fail.
                let _ = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(key as usize), LPARAM(0)) };
            }
            return LRESULT(1);
        }
    }
    let outcome = crate::msgloop::contain(Outcome::Pass, || {
        let bindings = bindings();
        let bound = bindings.as_deref().unwrap_or_default();
        STATE.with(|s| decide(event, mods(), bound, &mut s.borrow_mut()))
    });
    match outcome {
        Outcome::Pass => pass(),
        Outcome::Swallow(fired) => {
            if let Some((hwnd, id)) = fired {
                // SAFETY: a message to a window of Kabuk's; a gone window makes it fail.
                let _ = unsafe {
                    PostMessageW(
                        Some(HWND(hwnd as *mut _)),
                        WM_HOTKEY,
                        WPARAM(usize::try_from(id).unwrap_or(0)),
                        LPARAM(0),
                    )
                };
            }
            LRESULT(1)
        }
        Outcome::Masked(vk) => {
            send_masked(vk);
            LRESULT(1)
        }
        // Not decided by this hook.
        Outcome::Captured(_) => pass(),
    }
}

/// Where the recorder's chords go while a [`Capture`] lasts.
type Recorder = std::sync::Arc<dyn Fn(Chord) + Send + Sync>;
static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

thread_local! {
    static CAPTURE_STATE: std::cell::RefCell<KeyState> = std::cell::RefCell::new(KeyState::default());
}

unsafe extern "system" fn on_capture_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pass = || {
        // SAFETY: passes the event on with the arguments received.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    };
    if code < 0 {
        return pass();
    }
    // SAFETY: for WH_KEYBOARD_LL, lparam points to the event's KBDLLHOOKSTRUCT.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if info.flags.0 & LLKHF_INJECTED.0 != 0 {
        return pass();
    }
    let vk = u16::try_from(info.vkCode).unwrap_or(0);
    let event = match wparam.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Event::Down(vk),
        WM_KEYUP | WM_SYSKEYUP => Event::Up(vk),
        _ => return pass(),
    };
    let mods = mods_now();
    let outcome = crate::msgloop::contain(Outcome::Pass, || {
        CAPTURE_STATE.with(|s| decide_as(event, mods, &[], &mut s.borrow_mut(), true))
    });
    match outcome {
        Outcome::Captured(vk) => {
            if let (Some(key), Ok(r)) = (crate::msgloop::key_of_virtual(vk), RECORDER.lock()) {
                if let Some(record) = r.as_ref() {
                    record(Chord { mods, key });
                }
            }
            LRESULT(1)
        }
        Outcome::Masked(vk) => {
            send_masked(vk);
            LRESULT(1)
        }
        Outcome::Swallow(_) => LRESULT(1),
        Outcome::Pass => pass(),
    }
}

/// Settings' recorder taking the keys: each chord pressed meanwhile goes to the function
/// given (on the hook's thread), and nowhere else. Ends when dropped.
pub struct Capture {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    pub fn start(record: impl Fn(Chord) + Send + Sync + 'static) -> std::io::Result<Capture> {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
        if let Ok(mut r) = RECORDER.lock() {
            *r = Some(std::sync::Arc::new(record));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("key-capture".into()).spawn(move || {
            // SAFETY: a low-level hook on this thread, which pumps messages for it until
            // told to quit, then removes it.
            unsafe {
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_capture_key), None, 0);
                let _ = tx.send(hook.as_ref().map(|_| GetCurrentThreadId()).map_err(|e| e.to_string()));
                let Ok(hook) = hook else { return };
                let mut m = MSG::default();
                while GetMessageW(&mut m, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
                let _ = UnhookWindowsHookEx(hook);
            }
        })?;
        match rx.recv() {
            Ok(Ok(thread_id)) => Ok(Capture {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(std::io::Error::other(e)),
            Err(_) => Err(std::io::Error::other("key capture thread ended")),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        // SAFETY: tells our own hook thread to end its loop.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Ok(mut r) = RECORDER.lock() {
            *r = None;
        }
    }
}

/// Starts the hook's thread once; the error when the hook could not be installed.
fn ensure_hook() -> Result<(), String> {
    THREAD
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("keyboard-hook".into()).spawn(move || {
                use windows::Win32::System::Threading::{
                    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
                };
                use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
                // Answered first: Windows waits for this hook on every key, and Kabuk runs
                // below normal while a game is in front (D85).
                // SAFETY: this thread's own pseudo-handle.
                let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
                // SAFETY: a low-level hook needs no module and runs on this thread, which
                // pumps messages for it for the life of the process.
                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0) };
                let ok = hook.is_ok();
                let mut hook = hook.ok();
                HOOKED.store(ok, Ordering::Relaxed);
                // SAFETY: plain query.
                THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
                let _ = tx.send(if ok {
                    Ok(())
                } else {
                    Err("keyboard hook not installed".to_string())
                });
                if !ok {
                    return;
                }
                let mut m = MSG::default();
                // SAFETY: the standard message loop of this thread; the hook is taken off
                // and put back only here, on the thread that owns it.
                unsafe {
                    while GetMessageW(&mut m, None, 0, 0).as_bool() {
                        if m.hwnd.is_invalid() && m.message == MSG_UNHOOK {
                            if let Some(h) = hook.take() {
                                let _ = UnhookWindowsHookEx(h);
                                HOOKED.store(false, Ordering::Relaxed);
                                tracing::info!("keyboard hook off: a full-screen program is in front");
                            }
                            continue;
                        }
                        if m.hwnd.is_invalid() && m.message == MSG_REHOOK {
                            if hook.is_none() {
                                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0) {
                                    Ok(h) => {
                                        hook = Some(h);
                                        HOOKED.store(true, Ordering::Relaxed);
                                        tracing::info!("keyboard hook back");
                                    }
                                    Err(e) => tracing::error!(error = %e, "keyboard hook not put back"),
                                }
                            }
                            continue;
                        }
                        let _ = TranslateMessage(&m);
                        DispatchMessageW(&m);
                    }
                }
            });
            match spawned {
                Ok(_) => rx.recv().unwrap_or_else(|_| Err("keyboard hook thread ended".into())),
                Err(e) => Err(e.to_string()),
            }
        })
        .clone()
}

/// A chord bound to `window` until dropped: its press sends `WM_HOTKEY` with
/// `wparam == id`, as `RegisterHotKey` would.
#[derive(Debug)]
pub struct Shortcut {
    token: u64,
}

impl Shortcut {
    pub fn bind(window: &MessageWindow, id: i32, chord: Chord) -> std::io::Result<Shortcut> {
        ensure_hook().map_err(std::io::Error::other)?;
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let binding = Binding {
            token,
            hwnd: window.hwnd().0 as isize,
            id,
            mods: chord.mods,
            vk: virtual_key(chord.key).0,
        };
        change_bindings(|b| b.push(binding))?;
        Ok(Shortcut { token })
    }
}

impl Drop for Shortcut {
    fn drop(&mut self) {
        let _ = change_bindings(|b| b.retain(|b| b.token != self.token));
    }
}

/// The volume keys taken for `window` until dropped: each press (and repeat) is posted
/// to it as `message` with a [`VolumeKey`] as `wparam`, and never reaches Windows.
#[derive(Debug)]
pub struct VolumeKeys(());

impl VolumeKeys {
    pub fn take(window: &MessageWindow, message: u32) -> std::io::Result<VolumeKeys> {
        ensure_hook().map_err(std::io::Error::other)?;
        VOLUME_SINK.set(Some((window.hwnd().0 as isize, message)));
        Ok(VolumeKeys(()))
    }
}

impl Drop for VolumeKeys {
    fn drop(&mut self) {
        VOLUME_SINK.set(None);
    }
}

/// Tells `window` with `message` each time one of Windows' input-language chords ends
/// (Win+Space, Alt+Shift, Ctrl+Shift), until dropped; the keys go on to Windows (D88:
/// the bar's keyboard language follows a switch made in another program, which Windows
/// tells nobody else of).
#[derive(Debug)]
pub struct LanguageKeys(());

impl LanguageKeys {
    pub fn watch(window: crate::window::Hwnd, message: u32) -> std::io::Result<LanguageKeys> {
        ensure_hook().map_err(std::io::Error::other)?;
        LANGUAGE_SINK.set(Some((window.0 as isize, message)));
        Ok(LanguageKeys(()))
    }
}

impl Drop for LanguageKeys {
    fn drop(&mut self) {
        LANGUAGE_SINK.set(None);
    }
}

/// A chord bound by the hook, or, when the hook cannot be installed, registered with
/// `RegisterHotKey` as before D40.
#[derive(Debug)]
pub enum Bound {
    Hook(Shortcut),
    HotKey(crate::msgloop::HotKey),
}

/// Binds `chord` for `window` with the hook, falling back to `RegisterHotKey`.
pub fn bind(window: &MessageWindow, id: i32, chord: Chord) -> std::io::Result<Bound> {
    match Shortcut::bind(window, id, chord) {
        Ok(s) => Ok(Bound::Hook(s)),
        Err(e) => {
            tracing::warn!(error = %e, "keyboard hook unavailable; shortcut registered with Windows");
            crate::msgloop::HotKey::register(window, id, chord).map(Bound::HotKey)
        }
    }
}

/// Lets every key pass while the window in front is on one of `monitors` (`HMONITOR`
/// values of the monitors full-screen programs cover: game mode, DECISIONS D40), or,
/// with none, takes the chords everywhere again. Found by the user: with a video full
/// screen on one monitor, Win+arrow on the other did Windows' snapping instead.
pub fn set_paused_on(monitors: Vec<isize>) {
    let any = !monitors.is_empty();
    if let Ok(mut p) = PAUSED_ON.lock() {
        *p = monitors;
    }
    PAUSED.store(any, Ordering::Relaxed);
    update_hook();
}

#[cfg(test)]
mod tests {
    //! Edge cases (no key is sent: tests never inject input): a chord fires once, its
    //! repeats and release are swallowed; other modifiers or keys pass; the Win key's and
    //! Alt's release after a chord is masked, without one it passes; modifiers alone
    //! pass; bindings come and go with their `Shortcut`; a sink's window with its high
    //! bit set; every key and modifier set checked against the language keys.
    use super::*;
    use kabuk_core::config::shortcut::Key;

    /// Found by a test (M19): game mode on and off in quick turn left the hook off, as
    /// "on" was weighed against the state the hook's thread had not updated yet.
    #[test]
    fn off_then_on_in_quick_turn_still_asks_for_on() {
        // The thread has not read the "off" yet: the hook still counts as on.
        assert!(must_ask(ASKED_OFF, ASKED_ON, true));
        assert!(must_ask(ASKED_ON, ASKED_OFF, true));
        // Asked the same again: nothing to send.
        assert!(!must_ask(ASKED_ON, ASKED_ON, false));
        assert!(!must_ask(ASKED_OFF, ASKED_OFF, true));
        // Never asked: only a change from what the hook is.
        assert!(!must_ask(NOT_ASKED, ASKED_ON, true));
        assert!(must_ask(NOT_ASKED, ASKED_OFF, true));
        assert!(must_ask(NOT_ASKED, ASKED_ON, false));
    }

    /// D88: the end of Win+Space, Alt+Shift or Ctrl+Shift tells of a language switch,
    /// whichever key goes last; typing a capital, a key's press, or Space alone does not.
    #[test]
    fn the_end_of_a_language_chord_is_seen() {
        let held = |ctrl, alt, shift, win| Mods { ctrl, alt, shift, win };
        assert!(language_switch(Event::Up(SPACE), held(false, false, false, true)));
        assert!(!language_switch(Event::Up(SPACE), held(false, false, false, false)));
        assert!(!language_switch(Event::Down(SPACE), held(false, false, false, true)));
        assert!(language_switch(Event::Up(VK_LSHIFT.0), held(false, true, true, false)));
        assert!(language_switch(Event::Up(VK_LMENU.0), held(false, true, true, false)));
        assert!(language_switch(
            Event::Up(VK_RCONTROL.0),
            held(true, false, true, false)
        ));
        assert!(language_switch(Event::Up(VK_SHIFT.0), held(true, false, true, false)));
        assert!(
            !language_switch(Event::Up(VK_LSHIFT.0), held(false, false, true, false)),
            "a capital"
        );
        assert!(!language_switch(
            Event::Up(u16::from(b'A')),
            held(false, true, true, false)
        ));
    }

    const WIN: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        win: true,
    };
    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };

    fn binding(mods: Mods, key: Key, id: i32) -> Binding {
        Binding {
            token: id as u64,
            hwnd: 77,
            id,
            mods,
            vk: virtual_key(key).0,
        }
    }

    #[test]
    fn a_chord_fires_once_and_its_key_never_reaches_the_program() {
        let h = virtual_key(Key::Letter('H')).0;
        let b = [binding(WIN, Key::Letter('H'), 4)];
        let mut s = KeyState::default();
        assert_eq!(decide(Event::Down(VK_LWIN.0), NONE, &b, &mut s), Outcome::Pass);
        assert_eq!(decide(Event::Down(h), WIN, &b, &mut s), Outcome::Swallow(Some((77, 4))));
        assert_eq!(
            decide(Event::Down(h), WIN, &b, &mut s),
            Outcome::Swallow(None),
            "repeat"
        );
        assert_eq!(decide(Event::Up(h), WIN, &b, &mut s), Outcome::Swallow(None));
        // Start must not open: the Win key's release is masked once.
        assert_eq!(
            decide(Event::Up(VK_LWIN.0), NONE, &b, &mut s),
            Outcome::Masked(VK_LWIN.0)
        );
        assert_eq!(decide(Event::Up(VK_LWIN.0), NONE, &b, &mut s), Outcome::Pass);
    }

    #[test]
    fn other_chords_and_keys_pass_untouched() {
        let h = virtual_key(Key::Letter('H')).0;
        let e = virtual_key(Key::Letter('E')).0;
        let b = [binding(WIN, Key::Letter('H'), 4)];
        let mut s = KeyState::default();
        let win_shift = Mods { shift: true, ..WIN };
        assert_eq!(
            decide(Event::Down(h), win_shift, &b, &mut s),
            Outcome::Pass,
            "extra modifier"
        );
        assert_eq!(decide(Event::Down(h), NONE, &b, &mut s), Outcome::Pass, "plain typing");
        assert_eq!(
            decide(Event::Down(e), WIN, &b, &mut s),
            Outcome::Pass,
            "Windows' own Win+E"
        );
        assert_eq!(decide(Event::Up(h), NONE, &b, &mut s), Outcome::Pass);
        // No chord of Kabuk's in this press: Start opens as usual.
        assert_eq!(decide(Event::Up(VK_LWIN.0), NONE, &b, &mut s), Outcome::Pass);
    }

    #[test]
    fn alts_release_after_a_chord_is_masked() {
        let space = virtual_key(Key::Space).0;
        let alt = Mods { alt: true, ..NONE };
        let b = [binding(alt, Key::Space, 9)];
        let mut s = KeyState::default();
        assert_eq!(
            decide(Event::Down(space), alt, &b, &mut s),
            Outcome::Swallow(Some((77, 9)))
        );
        assert_eq!(decide(Event::Up(space), alt, &b, &mut s), Outcome::Swallow(None));
        assert_eq!(
            decide(Event::Up(VK_LMENU.0), NONE, &b, &mut s),
            Outcome::Masked(VK_LMENU.0)
        );
        assert!(!s.used_win);
    }

    #[test]
    fn volume_keys_are_taken_and_mute_acts_once_per_press() {
        let mut s = KeyState::default();
        let (up, mute) = (VK_VOLUME_UP.0, VK_VOLUME_MUTE.0);
        assert_eq!(decide_volume(Event::Down(up), &mut s), Some(Some(VolumeKey::Up)));
        assert_eq!(
            decide_volume(Event::Down(up), &mut s),
            Some(Some(VolumeKey::Up)),
            "repeat steps"
        );
        assert_eq!(decide_volume(Event::Up(up), &mut s), Some(None));
        assert_eq!(
            decide_volume(Event::Down(VK_VOLUME_DOWN.0), &mut s),
            Some(Some(VolumeKey::Down))
        );
        assert_eq!(decide_volume(Event::Down(mute), &mut s), Some(Some(VolumeKey::Mute)));
        assert_eq!(
            decide_volume(Event::Down(mute), &mut s),
            Some(None),
            "repeat does not toggle back"
        );
        assert_eq!(decide_volume(Event::Up(mute), &mut s), Some(None));
        assert_eq!(decide_volume(Event::Down(mute), &mut s), Some(Some(VolumeKey::Mute)));
        // Other keys are not the volume's.
        assert_eq!(decide_volume(Event::Down(VK_LWIN.0), &mut s), None);
        assert_eq!(decide_volume(Event::Up(virtual_key(Key::Letter('A')).0), &mut s), None);
        for k in [VolumeKey::Up, VolumeKey::Down, VolumeKey::Mute] {
            assert_eq!(VolumeKey::from_wparam(k as usize), Some(k));
        }
        assert_eq!(VolumeKey::from_wparam(0), None);
    }

    #[test]
    fn injected_volume_keys_are_taken_and_other_injected_keys_pass() {
        for vk in [VK_VOLUME_UP.0, VK_VOLUME_DOWN.0, VK_VOLUME_MUTE.0] {
            assert!(taken_when_injected(vk));
        }
        assert!(!taken_when_injected(MASK_KEY), "Kabuk's own mask key");
        assert!(!taken_when_injected(virtual_key(Key::Letter('A')).0));
        assert!(!taken_when_injected(VK_LWIN.0));
    }

    #[test]
    fn volume_keys_are_taken_only_while_asked() {
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.VolumeKeys", "", |_| None).unwrap();
        let taken = VolumeKeys::take(&w, 0x8001).expect("the hook installs");
        assert_eq!(VOLUME_SINK.get(), Some((w.hwnd().0 as isize, 0x8001)));
        drop(taken);
        assert_eq!(VOLUME_SINK.get(), None);
    }

    /// M19: a sink is read on every key without a lock, the window and the message in
    /// one word: what was set reads back, a handle with its high bit set too (Windows
    /// sign-extends handles); none is none.
    #[test]
    fn a_sink_reads_back_what_was_set() {
        for to in [
            Some((0x0001_2345, 0x8001)),
            Some((-0x7FFF_0000, u32::MAX)),
            Some((isize::from(i16::MAX), 0)),
            None,
        ] {
            assert_eq!(unpack_sink(pack_sink(to)), to, "{to:?}");
        }
        assert_eq!(pack_sink(None), 0);
        let sink = Sink::new();
        assert_eq!(sink.get(), None);
        sink.set(Some((77, 0x8002)));
        assert_eq!(sink.get(), Some((77, 0x8002)));
        sink.set(None);
        assert_eq!(sink.get(), None);
    }

    /// M19: only the release of Space, Shift, Alt or Ctrl is checked for a language
    /// switch (and reads the keys held); no other event of any key with any keys held
    /// would have been one.
    #[test]
    fn only_the_keys_of_a_language_chord_are_checked() {
        for vk in [
            SPACE,
            VK_SHIFT.0,
            VK_RSHIFT.0,
            VK_MENU.0,
            VK_LMENU.0,
            VK_CONTROL.0,
            VK_RCONTROL.0,
        ] {
            assert!(may_switch_language(Event::Up(vk)), "{vk:#x}");
            assert!(!may_switch_language(Event::Down(vk)), "{vk:#x} pressed");
        }
        assert!(!may_switch_language(Event::Up(VK_LWIN.0)));
        assert!(!may_switch_language(Event::Up(u16::from(b'A'))));
        let all_mods = (0..16u8).map(|m| Mods {
            ctrl: m & 1 != 0,
            alt: m & 2 != 0,
            shift: m & 4 != 0,
            win: m & 8 != 0,
        });
        for mods in all_mods {
            for vk in 0..=u16::from(u8::MAX) {
                for event in [Event::Down(vk), Event::Up(vk)] {
                    assert!(
                        !language_switch(event, mods) || may_switch_language(event),
                        "{event:?} with {mods:?} is a switch not checked"
                    );
                }
            }
        }
    }

    /// The game mode's state is the process's: tests that change it take turns.
    static PAUSE_TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn bindings_come_and_go_with_their_shortcut() {
        let _turn = PAUSE_TURN.lock().unwrap_or_else(|e| e.into_inner());
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Keyhook", "", |_| None).unwrap();
        let chord = Chord {
            mods: WIN,
            key: Key::Function(24),
        };
        let count = || bindings().map_or(0, |b| b.len());
        let before = count();
        let s = Shortcut::bind(&w, 3, chord).expect("the hook installs");
        assert_eq!(count(), before + 1);
        // M19: the hook reads the snapshot itself, not a copy of it.
        assert!(Arc::ptr_eq(&bindings().unwrap(), &bindings().unwrap()));
        drop(s);
        assert_eq!(count(), before);
        set_paused_on(vec![7]);
        assert!(PAUSED.load(Ordering::Relaxed));
        set_paused_on(Vec::new());
        assert!(!PAUSED.load(Ordering::Relaxed) && !paused_now());
    }

    /// Game mode lets keys pass only where the full-screen program is: the window in
    /// front on its monitor (or no window in front); on another monitor the chords work.
    #[test]
    fn keys_pass_only_while_the_front_window_is_on_a_full_screen_monitor() {
        assert!(!paused_for(Some(1), &[]), "no full-screen program");
        assert!(!paused_for(None, &[]));
        assert!(paused_for(Some(1), &[1]), "in front on its monitor");
        assert!(paused_for(Some(2), &[1, 2]));
        assert!(!paused_for(Some(2), &[1]), "the other monitor works as usual");
        assert!(paused_for(None, &[1]), "nothing in front: the game's");
    }

    /// D85, found by the user (games stuttered): while a full-screen program is in front
    /// the hook is off, so the game's keys never wait for Kabuk; it comes back after.
    #[test]
    fn the_hook_comes_off_over_a_full_screen_program_and_back_after() {
        let _turn = PAUSE_TURN.lock().unwrap_or_else(|e| e.into_inner());
        if ensure_hook().is_err() {
            println!("no keyboard hook on this desktop");
            return;
        }
        let wait = |want: bool| {
            let started = std::time::Instant::now();
            while is_hooked() != want && started.elapsed() < std::time::Duration::from_secs(2) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            is_hooked() == want
        };
        assert!(wait(true), "the hook is on to begin with");
        // The monitor in front (or, with no window in front, any: then it counts as the
        // game's) is full screen.
        set_paused_on(vec![front_monitor().unwrap_or(1)]);
        assert!(wait(false), "the hook stayed on over a full-screen program");
        set_paused_on(Vec::new());
        assert!(wait(true), "the hook did not come back");
    }

    /// The recorder takes the next chord, Win chords included, and its key's repeats
    /// and release; modifiers pass; the Win key's release is masked so Start stays shut;
    /// a key no chord can have passes.
    #[test]
    fn the_recorder_takes_any_chord() {
        let mut s = KeyState::default();
        let r = virtual_key(Key::Letter('R')).0;
        let cap = |e, m, s: &mut KeyState| decide_as(e, m, &[], s, true);
        assert_eq!(cap(Event::Down(VK_LWIN.0), NONE, &mut s), Outcome::Pass);
        assert_eq!(cap(Event::Down(r), WIN, &mut s), Outcome::Captured(r));
        assert_eq!(cap(Event::Down(r), WIN, &mut s), Outcome::Swallow(None), "repeat");
        assert_eq!(cap(Event::Up(r), WIN, &mut s), Outcome::Swallow(None));
        assert_eq!(cap(Event::Up(VK_LWIN.0), NONE, &mut s), Outcome::Masked(VK_LWIN.0));
        assert_eq!(cap(Event::Down(0xBC), NONE, &mut s), Outcome::Pass, "a comma");
        // Not capturing: the same chord unbound passes.
        let mut t = KeyState::default();
        assert_eq!(decide(Event::Down(r), WIN, &[], &mut t), Outcome::Pass);
    }
}
