//! Secrets in Windows Credential Manager (ARCHITECTURE §11b): the Create tab's API keys,
//! as generic credentials of the user, never in a file or a log. Values are UTF-8.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

/// The user name shown next to Kabuk's entries in Credential Manager.
const USER: &str = "Kabuk";

fn io(e: windows::core::Error) -> std::io::Error {
    std::io::Error::other(e)
}

/// Stores `secret` under `target`, replacing what was there.
pub fn write(target: &str, secret: &str) -> std::io::Result<()> {
    let target_w = HSTRING::from(target);
    let user = HSTRING::from(USER);
    let mut blob = secret.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_ptr().cast_mut()),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: u32::try_from(blob.len()).map_err(std::io::Error::other)?,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(user.as_ptr().cast_mut()),
    };
    // SAFETY: every pointer in `cred` points into a buffer that outlives the call, which
    // copies them; Windows does not write through them.
    let r = unsafe { CredWriteW(&cred, 0) }.map_err(io);
    blob.iter_mut().for_each(|b| *b = 0);
    r
}

/// The secret stored under `target`, if any.
pub fn read(target: &str) -> Option<String> {
    let mut out: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: Windows allocates the credential and gives it to us; it is freed below with
    // CredFree after its blob is copied.
    unsafe {
        CredReadW(&HSTRING::from(target), CRED_TYPE_GENERIC, None, &mut out).ok()?;
        if out.is_null() {
            return None;
        }
        let c = &*out;
        let bytes = if c.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
        };
        CredFree(out.cast());
        String::from_utf8(bytes).ok()
    }
}

/// Whether a secret is stored under `target`.
pub fn exists(target: &str) -> bool {
    read(target).is_some_and(|s| !s.is_empty())
}

/// Removes the secret under `target`; nothing to remove is fine.
pub fn delete(target: &str) -> std::io::Result<()> {
    // SAFETY: plain call with a valid string.
    match unsafe { CredDeleteW(&HSTRING::from(target), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(io(e)),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a secret written, read back, replaced and deleted under a target only
    //! this test uses; deleting what is not there; a missing target reads nothing.
    use super::*;

    #[test]
    fn a_secret_is_written_read_replaced_and_deleted() {
        let target = format!("Kabuk/test/credentials-{}", std::process::id());
        assert_eq!(read(&target), None);
        write(&target, "sk-test-ğü").unwrap();
        assert_eq!(read(&target).as_deref(), Some("sk-test-ğü"));
        assert!(exists(&target));
        write(&target, "second").unwrap();
        assert_eq!(read(&target).as_deref(), Some("second"));
        delete(&target).unwrap();
        assert_eq!(read(&target), None);
        assert!(!exists(&target));
        delete(&target).unwrap();
    }
}
