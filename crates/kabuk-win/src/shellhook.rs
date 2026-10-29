//! The shell's notices about top-level windows (`RegisterShellHookWindow`, documented and
//! without code in other processes): a window that flashes for attention, and one that
//! comes to the front. Windows' taskbar shows the first in its button (PLAN M19 U14).

use windows::Win32::UI::WindowsAndMessaging::{DeregisterShellHookWindow, RegisterShellHookWindow};

use crate::window::Hwnd;

/// `HSHELL_WINDOWACTIVATED`: a window came to the front (`lparam` its handle).
pub const ACTIVATED: usize = 4;
/// `HSHELL_RUDEAPPACTIVATED`: a full-screen window came to the front.
pub const RUDE_ACTIVATED: usize = 0x8004;
/// `HSHELL_FLASH`: a window flashes to ask for attention (`lparam` its handle).
pub const FLASH: usize = 0x8006;
/// `HSHELL_WINDOWDESTROYED`.
pub const DESTROYED: usize = 2;

/// What a shell notice says, from its `wparam`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The window asks for attention.
    Flash,
    /// The window is in front now (or gone): it has the user's attention.
    Seen,
    Other,
}

impl Notice {
    pub fn of(wparam: usize) -> Notice {
        match wparam {
            FLASH => Notice::Flash,
            ACTIVATED | RUDE_ACTIVATED | DESTROYED => Notice::Seen,
            _ => Notice::Other,
        }
    }
}

/// `window` gets the shell's notices as [`ShellHook::message`] until dropped.
pub struct ShellHook {
    window: Hwnd,
    message: u32,
}

impl ShellHook {
    pub fn register(window: Hwnd) -> std::io::Result<ShellHook> {
        let message = crate::system::register_message("SHELLHOOK");
        // SAFETY: a window of this process; deregistered in Drop.
        if !unsafe { RegisterShellHookWindow(window.raw()) }.as_bool() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(ShellHook { window, message })
    }

    /// The message id the shell's notices come with (`wparam` the notice, `lparam` the
    /// window).
    pub fn message(&self) -> u32 {
        self.message
    }
}

impl Drop for ShellHook {
    fn drop(&mut self) {
        // SAFETY: the window registered in `register`.
        let _ = unsafe { DeregisterShellHookWindow(self.window.raw()) };
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: each notice read from its value; a window registers and lets go.
    use super::*;

    #[test]
    fn the_notices_are_read_from_their_values() {
        assert_eq!(Notice::of(FLASH), Notice::Flash);
        assert_eq!(Notice::of(ACTIVATED), Notice::Seen);
        assert_eq!(Notice::of(RUDE_ACTIVATED), Notice::Seen);
        assert_eq!(Notice::of(DESTROYED), Notice::Seen);
        assert_eq!(Notice::of(1), Notice::Other);
    }

    #[test]
    fn a_window_registers_for_the_shells_notices() {
        let w = crate::msgloop::Window::shell_surface(
            "Kabuk.Test.ShellHook",
            "",
            crate::msgloop::PxRect::default(),
            |_| None,
        )
        .unwrap();
        let hook = ShellHook::register(w.hwnd()).expect("registered");
        assert!(hook.message() >= 0xC000, "a registered message");
        drop(hook);
    }
}
