//! Which shell has the desk (asked for by the user: never two shells on one desk): a
//! session-wide named block holding the state directory of the shell that has it. The
//! block lives while a shell holds its [`Claim`], so a shell that ended leaves none.

use std::path::{Path, PathBuf};

use kabuk_core::instance::{desk_owner, desk_owner_block, DESK_OWNER, DESK_OWNER_SIZE};
use windows::core::HSTRING;
use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE, PAGE_READWRITE,
};

use crate::handle::OwnedHandle;
use crate::sync::io_error;

/// This process has the desk while it keeps this.
#[derive(Debug)]
pub struct Claim {
    _block: OwnedHandle,
}

/// Names `state_dir` as the desk's shell.
pub fn claim(state_dir: &Path) -> std::io::Result<Claim> {
    let bytes = desk_owner_block(state_dir).ok_or_else(|| std::io::Error::other("state directory path too long"))?;
    let size = u32::try_from(DESK_OWNER_SIZE).unwrap_or(u32::MAX);
    // SAFETY: a named, page-file backed mapping owned by the wrapper.
    let block = OwnedHandle::new(
        unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size,
                &HSTRING::from(DESK_OWNER),
            )
        }
        .map_err(io_error)?,
    );
    // SAFETY: a writable view of the whole block, unmapped below.
    let view = unsafe { MapViewOfFile(block.raw(), FILE_MAP_WRITE, 0, 0, DESK_OWNER_SIZE) };
    if view.Value.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the view and `bytes` are both DESK_OWNER_SIZE bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), view.Value.cast::<u8>(), DESK_OWNER_SIZE) };
    // SAFETY: the view mapped above.
    let _ = unsafe { UnmapViewOfFile(view) };
    Ok(Claim { _block: block })
}

/// The state directory of the shell that has the desk; `None` when no shell has it.
pub fn current() -> Option<PathBuf> {
    // SAFETY: a named mapping opened read-only, closed when `block` drops.
    let block = OwnedHandle::new(unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, &HSTRING::from(DESK_OWNER)) }.ok()?);
    // SAFETY: a read-only view of the whole block, unmapped below.
    let view = unsafe { MapViewOfFile(block.raw(), FILE_MAP_READ, 0, 0, DESK_OWNER_SIZE) };
    if view.Value.is_null() {
        return None;
    }
    // SAFETY: the view is DESK_OWNER_SIZE readable bytes, copied before unmapping.
    let bytes = unsafe { std::slice::from_raw_parts(view.Value.cast::<u8>(), DESK_OWNER_SIZE) }.to_vec();
    // SAFETY: the view mapped above.
    let _ = unsafe { UnmapViewOfFile(view) };
    desk_owner(&bytes)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a claim names its directory while kept and a later claim takes over
    //! the name. (Only the shared name is touched; no running shell is involved, but a
    //! shell of the user's running at the same time would see the test's name for a
    //! moment, so the original owner is put back.)
    use super::*;

    #[test]
    fn a_claim_names_its_directory_and_a_later_one_takes_over() {
        let before = current();
        let a = claim(Path::new(r"C:\kabuk-test\a")).unwrap();
        assert_eq!(current().as_deref(), Some(Path::new(r"C:\kabuk-test\a")));
        let b = claim(Path::new(r"C:\kabuk-test\b")).unwrap();
        assert_eq!(current().as_deref(), Some(Path::new(r"C:\kabuk-test\b")));
        drop(a);
        drop(b);
        if let Some(dir) = before {
            // Another holder keeps the block alive: give it its name back.
            let _ = claim(&dir);
        } else {
            assert_eq!(current(), None);
        }
    }
}
