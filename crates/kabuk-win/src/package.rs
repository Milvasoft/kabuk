//! Kabuk installed from the Microsoft Store (DECISIONS D84): whether this process runs
//! from its MSIX package, and the package's start at sign-in (its `StartupTask`, which
//! takes the place of the `Run` value).

use windows::core::HSTRING;
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

/// The startup task's id in `packaging/AppxManifest.xml`.
pub const STARTUP_TASK: &str = "KabukStartup";

/// Whether this process runs from an MSIX package.
pub fn is_packaged() -> bool {
    let mut length = 0u32;
    // SAFETY: asks only for the name's length, with no buffer.
    let result = unsafe { GetCurrentPackageFullName(&mut length, None) };
    result != APPMODEL_ERROR_NO_PACKAGE
}

/// The package's own temporary folder as it is on disk (`…\Packages\<family>\AC\Temp`),
/// where other programs (Explorer, for a wallpaper) can read what Kabuk writes; `None`
/// outside a package (DECISIONS D89).
pub fn temp_folder() -> Option<std::path::PathBuf> {
    if !is_packaged() {
        return None;
    }
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    let mut length = 0u32;
    // SAFETY: the two-call form: the length, then a buffer that large.
    unsafe {
        let _ = GetCurrentPackageFamilyName(&mut length, None);
        let mut buf = vec![0u16; length as usize];
        if GetCurrentPackageFamilyName(&mut length, Some(windows::core::PWSTR(buf.as_mut_ptr()))).is_err() {
            return None;
        }
        let family = String::from_utf16_lossy(&buf[..(length as usize).saturating_sub(1)]);
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(
            std::path::Path::new(&local)
                .join("Packages")
                .join(family)
                .join("AC")
                .join("Temp"),
        )
    }
}

fn startup_task() -> std::io::Result<StartupTask> {
    StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK))
        .and_then(|op| op.join())
        .map_err(std::io::Error::other)
}

/// Whether the package starts at sign-in.
pub fn starts_at_sign_in() -> std::io::Result<bool> {
    let state = startup_task()?.State().map_err(std::io::Error::other)?;
    Ok(state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy)
}

/// Turns the package's start at sign-in on or off; whether it is on afterwards (the user
/// or a policy may have turned it off in Windows' settings, which a program cannot undo).
pub fn set_start_at_sign_in(on: bool) -> std::io::Result<bool> {
    let task = startup_task()?;
    if on {
        let state = task
            .RequestEnableAsync()
            .and_then(|op| op.join())
            .map_err(std::io::Error::other)?;
        Ok(state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy)
    } else {
        task.Disable().map_err(std::io::Error::other)?;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a program run outside a package (every test) is not packaged and has
    //! no startup task to read or set.
    use super::*;

    #[test]
    fn a_program_outside_a_package_is_not_packaged() {
        assert!(!is_packaged());
    }

    #[test]
    fn outside_a_package_there_is_no_startup_task() {
        assert!(starts_at_sign_in().is_err());
        assert!(set_start_at_sign_in(false).is_err());
    }
}
