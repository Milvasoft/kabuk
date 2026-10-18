//! Session-wide helpers: UI language, message boxes, console output.

use kabuk_core::config::{Config, LanguageChoice};
use kabuk_i18n::{I18n, Lang};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    MB_YESNO,
};

/// Translations in the configured language, or Windows' display language when the
/// configuration says "system" or is unavailable.
pub fn i18n(config: Option<&Config>) -> I18n {
    let lang = match config.map(|c| c.language.language) {
        Some(LanguageChoice::Tr) => Lang::Tr,
        Some(LanguageChoice::En) => Lang::En,
        Some(LanguageChoice::System) | None => Lang::from_system_tag(&ui_language_tag()),
    };
    I18n::new(lang)
}

/// Initialises COM for this thread (single-threaded apartment, as UI threads need).
/// Repeated calls are harmless.
pub fn com_init() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    // SAFETY: plain call; an "already initialised" result is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

/// COM for a worker thread that only waits on WinRT calls (multi-threaded apartment).
pub fn com_init_mta() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    // SAFETY: plain call; an "already initialised" result is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// Windows' app mode ("Choose your default app mode"): true for light. Read-only;
/// defaults to dark when the value is missing.
pub fn apps_use_light_theme() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 0u32;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or_default();
    // SAFETY: valid strings and an out buffer of the size passed.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            &HSTRING::from("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    r.is_ok() && value != 0
}

/// Kabuk steps back for a full-screen program (D85, found by the user: games stuttered):
/// below-normal priority for its whole process while one is in front, so the game wins
/// every contest for the processor; normal again after. Its keyboard hook's thread stays
/// high within it (keyhook), so keys typed on another monitor meanwhile are not delayed.
pub fn yield_to_full_screen(on: bool) -> std::io::Result<()> {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    };
    let class = if on {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        NORMAL_PRIORITY_CLASS
    };
    // SAFETY: this process's own pseudo-handle and a valid priority class.
    unsafe { SetPriorityClass(GetCurrentProcess(), class) }.map_err(crate::sync::io_error)
}

/// Windows' "Animation effects" (Settings → Accessibility → Visual effects). Read-only;
/// on when it cannot be read, as Windows' default is.
pub fn animations_on() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut on = windows::core::BOOL(1);
    // SAFETY: the out parameter is a BOOL, as this action expects.
    let r = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut on as *mut windows::core::BOOL).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    r.is_err() || on.as_bool()
}

/// The setting area named by a `WM_SETTINGCHANGE` (`lparam` points to a string such as
/// `ImmersiveColorSet` when the app mode changes).
///
/// The caller passes the `lparam` of a `WM_SETTINGCHANGE` it is handling, which is
/// either null or a valid NUL-terminated wide string for the duration of the message.
pub fn setting_change_area(lparam: isize) -> Option<String> {
    if lparam == 0 {
        return None;
    }
    let p = windows::core::PCWSTR(lparam as *const u16);
    // SAFETY: per the contract above, `p` is a valid NUL-terminated string while the message is handled.
    unsafe { p.to_string() }.ok()
}

/// Sends `message` to every top-level window without waiting for any of them.
pub fn broadcast_notify(message: u32) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendNotifyMessageW, HWND_BROADCAST};
    // SAFETY: plain values; SendNotifyMessage returns at once for windows of other threads.
    let _ = unsafe { SendNotifyMessageW(HWND_BROADCAST, message, WPARAM(0), LPARAM(0)) };
}

/// `WM_SETTINGCHANGE` area sent when the light/dark app mode or accent changes.
pub const IMMERSIVE_COLOR_SET: &str = "ImmersiveColorSet";

/// Sends `WM_SETTINGCHANGE` naming `area` to one window (not a broadcast), waiting at most
/// `timeout`. Windows copies the string into the receiving process.
pub fn send_setting_change(to: crate::window::Hwnd, area: &str, timeout: std::time::Duration) -> std::io::Result<()> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    let text = HSTRING::from(area);
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `text` outlives the call, which returns only after the message was handled
    // or timed out; invalid handles fail cleanly.
    let r = unsafe {
        SendMessageTimeoutW(
            to.raw(),
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(text.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            ms,
            None,
        )
    };
    if r.0 == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// The local time.
pub fn local_time() -> kabuk_core::clock::WallTime {
    // SAFETY: plain call returning a value.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    kabuk_core::clock::WallTime {
        year: t.wYear,
        month: u8::try_from(t.wMonth).unwrap_or(1),
        day: u8::try_from(t.wDay).unwrap_or(1),
        weekday: u8::try_from(t.wDayOfWeek).unwrap_or(0),
        hour: u8::try_from(t.wHour).unwrap_or(0),
        minute: u8::try_from(t.wMinute).unwrap_or(0),
        second: u8::try_from(t.wSecond).unwrap_or(0),
        millisecond: t.wMilliseconds,
    }
}

/// A message id shared by every process that registers `name` (e.g. `TaskbarCreated`).
pub fn register_message(name: &str) -> u32 {
    // SAFETY: valid string for the call.
    unsafe { windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(&HSTRING::from(name)) }
}

/// Broadcast by Explorer when the taskbar is (re)created, e.g. after Explorer restarts.
pub const TASKBAR_CREATED: &str = "TaskbarCreated";

/// The user's preferred Windows display language, e.g. `tr-TR`. Empty if unknown.
pub fn ui_language_tag() -> String {
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: first call asks only for the buffer size.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) }.is_err() || len == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` holds `len` characters as the first call requested.
    if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(PWSTR(buf.as_mut_ptr())), &mut len) }
        .is_err()
    {
        return String::new();
    }
    // The buffer is a double-NUL-terminated list; the first entry is the preferred one.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Shows a modal warning box. Blocks the calling thread until dismissed.
pub fn warning_box(title: &str, text: &str) {
    // SAFETY: both strings are valid for the call; no owner window.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
}

/// Shows a modal information box. Blocks the calling thread until dismissed.
pub fn info_box(title: &str, text: &str) {
    // SAFETY: both strings are valid for the call; no owner window.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        )
    };
}

/// Asks a yes-or-no question in a modal box; true for yes.
pub fn question_box(title: &str, text: &str) -> bool {
    // SAFETY: both strings are valid for the call; no owner window.
    let answer = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND,
        )
    };
    answer == IDYES
}

/// Makes `println!` reach the terminal that started this GUI-subsystem program.
/// Does nothing when output is already redirected (pipes, files) or there is no
/// parent console.
pub fn attach_parent_console() {
    // SAFETY: reads the process's standard output handle.
    let has_stdout = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
    if !has_stdout {
        // SAFETY: plain call; failure just means there is no parent console.
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the language tag looks like a BCP-47 tag; the process steps back for a
    //! game and comes back.
    use super::*;

    /// D85: the process steps back for a game and comes back after.
    #[test]
    fn kabuk_steps_back_for_a_game_and_comes_back() {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        };
        // SAFETY: plain query of this process.
        let class = || unsafe { GetPriorityClass(GetCurrentProcess()) };
        yield_to_full_screen(true).expect("lowered");
        assert_eq!(class(), BELOW_NORMAL_PRIORITY_CLASS.0);
        yield_to_full_screen(false).expect("back");
        assert_eq!(class(), NORMAL_PRIORITY_CLASS.0);
    }

    #[test]
    fn the_animation_setting_reads() {
        // Either answer is the user's; reading it twice gives the same one.
        assert_eq!(animations_on(), animations_on());
    }

    #[test]
    fn configured_language_wins() {
        let mut c = Config::default();
        c.language.language = LanguageChoice::Tr;
        assert_eq!(i18n(Some(&c)).lang(), Lang::Tr);
        c.language.language = LanguageChoice::En;
        assert_eq!(i18n(Some(&c)).lang(), Lang::En);
        let system = Lang::from_system_tag(&ui_language_tag());
        assert_eq!(i18n(None).lang(), system);
    }

    #[test]
    fn local_time_is_plausible_and_messages_register() {
        let t = local_time();
        assert!(t.year >= 2024 && (1..=12).contains(&t.month) && t.weekday <= 6 && t.hour < 24);
        let a = register_message(TASKBAR_CREATED);
        assert!(a >= 0xC000, "registered messages live in 0xC000..0xFFFF");
        assert_eq!(a, register_message(TASKBAR_CREATED));
    }

    #[test]
    fn app_mode_and_setting_strings() {
        // Reads without failing whatever the user's setting is.
        let _ = apps_use_light_theme();
        assert_eq!(setting_change_area(0), None);
        let wide: Vec<u16> = "ImmersiveColorSet".encode_utf16().chain([0]).collect();
        assert_eq!(
            setting_change_area(wide.as_ptr() as isize).as_deref(),
            Some(IMMERSIVE_COLOR_SET)
        );
    }

    #[test]
    fn language_tag_shape() {
        let tag = ui_language_tag();
        assert!(tag.len() >= 2, "{tag}");
        assert!(tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{tag}");
    }
}
