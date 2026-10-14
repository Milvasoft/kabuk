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
//! the machine wait); injected keys (the mask among them) pass untouched; while paused
//! (game mode) every key passes.
//!
//! Settings' shortcut recorder takes the keys with a [`Capture`]: a hook of its own,
//! installed when the recorder waits (so it is the newest and sees keys before Kabuk's
//! and Windows'), which takes the next chord, Win chords included, from everyone and
//! hands it over, then goes.
//!
//! The volume keys are taken too while the bar asks for them ([`VolumeKeys`]): the bar
//! changes the volume and shows its own OSD, so Windows' flyout does not show beside it.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

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

static BINDINGS: Mutex<Vec<Binding>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
static PAUSED: AtomicBool = AtomicBool::new(false);
static THREAD: OnceLock<Result<(), String>> = OnceLock::new();

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

/// Where the volume keys go while taken: (window, message).
static VOLUME_SINK: Mutex<Option<(isize, u32)>> = Mutex::new(None);

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

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pass = || {
        // SAFETY: passes the event on with the arguments received.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    };
    if code < 0 || PAUSED.load(Ordering::Relaxed) {
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
    let sink = VOLUME_SINK.lock().ok().and_then(|s| *s);
    if let Some((hwnd, message)) = sink {
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
        let bindings = BINDINGS.lock().map(|b| b.clone()).unwrap_or_default();
        STATE.with(|s| decide(event, mods_now(), &bindings, &mut s.borrow_mut()))
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
                // SAFETY: a low-level hook needs no module and runs on this thread, which
                // pumps messages for it for the life of the process.
                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0) };
                let ok = hook.is_ok();
                let _ = tx.send(hook.map(|_| ()).map_err(|e| e.to_string()));
                if !ok {
                    return;
                }
                let mut m = MSG::default();
                // SAFETY: the standard message loop of this thread.
                unsafe {
                    while GetMessageW(&mut m, None, 0, 0).as_bool() {
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
        BINDINGS
            .lock()
            .map_err(|_| std::io::Error::other("shortcut table poisoned"))?
            .push(binding);
        Ok(Shortcut { token })
    }
}

impl Drop for Shortcut {
    fn drop(&mut self) {
        if let Ok(mut b) = BINDINGS.lock() {
            b.retain(|b| b.token != self.token);
        }
    }
}

/// The volume keys taken for `window` until dropped: each press (and repeat) is posted
/// to it as `message` with a [`VolumeKey`] as `wparam`, and never reaches Windows.
#[derive(Debug)]
pub struct VolumeKeys(());

impl VolumeKeys {
    pub fn take(window: &MessageWindow, message: u32) -> std::io::Result<VolumeKeys> {
        ensure_hook().map_err(std::io::Error::other)?;
        let mut sink = VOLUME_SINK
            .lock()
            .map_err(|_| std::io::Error::other("volume key sink poisoned"))?;
        *sink = Some((window.hwnd().0 as isize, message));
        Ok(VolumeKeys(()))
    }
}

impl Drop for VolumeKeys {
    fn drop(&mut self) {
        if let Ok(mut s) = VOLUME_SINK.lock() {
            *s = None;
        }
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

/// Lets every key pass (game mode, DECISIONS D40) or takes the chords again.
pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    //! Edge cases (no key is sent: tests never inject input): a chord fires once, its
    //! repeats and release are swallowed; other modifiers or keys pass; the Win key's and
    //! Alt's release after a chord is masked, without one it passes; modifiers alone
    //! pass; bindings come and go with their `Shortcut`.
    use super::*;
    use kabuk_core::config::shortcut::Key;

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
    fn volume_keys_are_taken_only_while_asked() {
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.VolumeKeys", "", |_| None).unwrap();
        let taken = VolumeKeys::take(&w, 0x8001).expect("the hook installs");
        assert!(VOLUME_SINK.lock().unwrap().is_some());
        drop(taken);
        assert!(VOLUME_SINK.lock().unwrap().is_none());
    }

    #[test]
    fn bindings_come_and_go_with_their_shortcut() {
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Keyhook", "", |_| None).unwrap();
        let chord = Chord {
            mods: WIN,
            key: Key::Function(24),
        };
        let before = BINDINGS.lock().unwrap().len();
        let s = Shortcut::bind(&w, 3, chord).expect("the hook installs");
        assert_eq!(BINDINGS.lock().unwrap().len(), before + 1);
        drop(s);
        assert_eq!(BINDINGS.lock().unwrap().len(), before);
        set_paused(true);
        assert!(PAUSED.load(Ordering::Relaxed));
        set_paused(false);
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
