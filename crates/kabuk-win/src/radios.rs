//! The computer's radios for the quick settings panel (DECISIONS D88): Wi-Fi and
//! Bluetooth on and off through Windows' radios (the switches Windows' own quick settings
//! use), and airplane mode as every radio off. Every call blocks while Windows answers:
//! make them off the bar's thread.

use std::io;

use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

/// The radios' states: `None` for a kind the computer has none of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Radios {
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
}

impl Radios {
    /// Airplane mode as the panel shows it: radios there, and all of them off.
    pub fn airplane(&self) -> Option<bool> {
        let states: Vec<bool> = [self.wifi, self.bluetooth].into_iter().flatten().collect();
        (!states.is_empty()).then(|| states.iter().all(|on| !on))
    }
}

fn all() -> io::Result<Vec<Radio>> {
    let access = Radio::RequestAccessAsync()
        .and_then(|op| op.join())
        .map_err(crate::sync::io_error)?;
    if access != RadioAccessStatus::Allowed {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let list = Radio::GetRadiosAsync()
        .and_then(|op| op.join())
        .map_err(crate::sync::io_error)?;
    Ok(list.into_iter().collect())
}

/// The radios now.
pub fn read() -> Radios {
    let mut out = Radios::default();
    for r in all().unwrap_or_default() {
        let on = r.State().is_ok_and(|s| s == RadioState::On);
        match r.Kind() {
            Ok(k) if k == RadioKind::WiFi => out.wifi = Some(out.wifi.unwrap_or(false) || on),
            Ok(k) if k == RadioKind::Bluetooth => out.bluetooth = Some(out.bluetooth.unwrap_or(false) || on),
            _ => {}
        }
    }
    out
}

fn set(kinds: &[RadioKind], on: bool) -> io::Result<()> {
    let state = if on { RadioState::On } else { RadioState::Off };
    let mut found = false;
    for r in all()? {
        if kinds.contains(&r.Kind().map_err(crate::sync::io_error)?) {
            found = true;
            let done = r
                .SetStateAsync(state)
                .and_then(|op| op.join())
                .map_err(crate::sync::io_error)?;
            if done != RadioAccessStatus::Allowed {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
        }
    }
    if found {
        Ok(())
    } else {
        Err(io::Error::other("no such radio"))
    }
}

pub fn set_wifi(on: bool) -> io::Result<()> {
    set(&[RadioKind::WiFi], on)
}

pub fn set_bluetooth(on: bool) -> io::Result<()> {
    set(&[RadioKind::Bluetooth], on)
}

/// Airplane mode on (every radio off) or off (every radio on).
pub fn set_airplane(on: bool) -> io::Result<()> {
    set(
        &[RadioKind::WiFi, RadioKind::Bluetooth, RadioKind::MobileBroadband],
        !on,
    )
}

#[cfg(test)]
mod tests {
    //! Edge cases: airplane mode with no radios, one off, all off; reading the machine's
    //! radios never fails (none in a test machine is fine).
    use super::*;

    #[test]
    fn airplane_is_every_radio_off() {
        assert_eq!(Radios::default().airplane(), None);
        let r = |wifi, bluetooth| Radios { wifi, bluetooth };
        assert_eq!(r(Some(false), Some(true)).airplane(), Some(false));
        assert_eq!(r(Some(false), Some(false)).airplane(), Some(true));
        assert_eq!(r(None, Some(false)).airplane(), Some(true));
    }

    #[test]
    fn the_radios_read() {
        crate::system::com_init_mta();
        let _ = read();
    }
}
