//! Windows' mobile hotspot for the quick settings panel (DECISIONS D88): sharing the
//! internet connection over Wi-Fi, through the tethering manager of the connection in use.
//! Calls block while Windows answers: make them off the bar's thread.

use std::io;

use windows::Networking::Connectivity::NetworkInformation;
use windows::Networking::NetworkOperators::{
    NetworkOperatorTetheringManager, TetheringCapability, TetheringOperationStatus, TetheringOperationalState,
};

fn manager() -> io::Result<NetworkOperatorTetheringManager> {
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(crate::sync::io_error)?;
    let able = NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile(&profile)
        .map_err(crate::sync::io_error)?;
    if able != TetheringCapability::Enabled {
        return Err(io::Error::other("the connection cannot be shared"));
    }
    NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(crate::sync::io_error)
}

/// Whether the hotspot is on; `None` where the connection in use cannot be shared.
pub fn state() -> Option<bool> {
    let m = manager().ok()?;
    let s = m.TetheringOperationalState().ok()?;
    Some(s == TetheringOperationalState::On || s == TetheringOperationalState::InTransition)
}

pub fn set(on: bool) -> io::Result<()> {
    let m = manager()?;
    let op = if on {
        m.StartTetheringAsync()
    } else {
        m.StopTetheringAsync()
    };
    let result = op.and_then(|o| o.join()).map_err(crate::sync::io_error)?;
    match result.Status().map_err(crate::sync::io_error)? {
        TetheringOperationStatus::Success => Ok(()),
        other => Err(io::Error::other(format!("hotspot not switched ({})", other.0))),
    }
}

#[cfg(test)]
mod tests {
    //! On the machine running the tests: the hotspot's state reads (none where the
    //! connection cannot be shared, as in the test VM).
    #[test]
    fn the_hotspot_state_reads() {
        crate::system::com_init_mta();
        let _ = super::state();
    }
}
