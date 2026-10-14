//! The command pipe (ARCHITECTURE §12): a named pipe only the current user can open,
//! one request line in, one answer line out per connection.
//!
//! The pipe is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a program that created a
//! pipe of the same name first (with a looser ACL) makes the server fail instead of
//! receiving the user's commands; remote clients are rejected.

use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{
    LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_SEM_TIMEOUT, GENERIC_READ,
    GENERIC_WRITE, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    AclSizeInformation, GetAce, GetAclInformation, GetTokenInformation, TokenUser, ACCESS_ALLOWED_ACE, ACL,
    ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, WaitNamedPipeW,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::handle::OwnedHandle;
use crate::sync::io_error;

/// The largest request or answer (a `query windows` of a few hundred windows fits).
pub const MAX_MESSAGE: usize = 1 << 20;
/// Pipe buffer sizes.
const BUFFER: u32 = 64 * 1024;

/// The current user's SID as text (`S-1-5-21-…`).
pub fn user_sid() -> std::io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: our own process; `token` is a valid out pointer, owned below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(io_error)?;
    let token = OwnedHandle::new(token);
    let mut size = 0u32;
    // SAFETY: a size query; it fails with "insufficient buffer" and sets `size`.
    let _ = unsafe { GetTokenInformation(token.raw(), TokenUser, None, 0, &mut size) };
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` has the size the first call asked for.
    unsafe { GetTokenInformation(token.raw(), TokenUser, Some(buf.as_mut_ptr().cast()), size, &mut size) }
        .map_err(io_error)?;
    // SAFETY: the buffer holds a TOKEN_USER (followed by the SID it points into).
    let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
    sid_text(sid)
}

fn sid_text(sid: PSID) -> std::io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: a valid SID; the string is freed with LocalFree below.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(io_error)?;
    // SAFETY: `text` is the NUL-terminated string just allocated.
    let s = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc.
    let _ = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Ok(s)
}

/// A security descriptor allowing only `sid`, freed on drop.
struct OnlyUser(PSECURITY_DESCRIPTOR);

impl OnlyUser {
    fn new(sid: &str) -> std::io::Result<OnlyUser> {
        // Protected DACL with one entry: full access for the user. Nobody else, not even
        // administrators or SYSTEM, can open the pipe.
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a valid SDDL string; the descriptor is freed in Drop.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }
            .map_err(io_error)?;
        Ok(OnlyUser(sd))
    }
}

// SAFETY: the descriptor is a heap block only read once built; it moves with the server
// thread that uses it and is freed once.
unsafe impl Send for OnlyUser {}

impl Drop for OnlyUser {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0 .0))) };
    }
}

/// The server's answer to one request, and what to do once it has been sent (the
/// `restore` command stops the shell only after answering).
pub struct Reply {
    pub line: String,
    pub after: Option<Box<dyn FnOnce() + Send>>,
}

/// Serves the pipe on its own thread until dropped.
pub struct PipeServer {
    name: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn create(name: &str, sd: &OnlyUser) -> std::io::Result<OwnedHandle> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    // SAFETY: valid name and security attributes for the duration of the call.
    let h = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(name),
            FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX.0 | FILE_FLAG_FIRST_PIPE_INSTANCE.0),
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            BUFFER,
            BUFFER,
            0,
            Some(&sa),
        )
    };
    if h.is_invalid() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(OwnedHandle::new(h))
}

/// Reads one line (up to the newline) or until the other side stops writing.
fn read_line(file: &mut std::fs::File) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.contains(&b'\n') || buf.len() > MAX_MESSAGE {
            break;
        }
    }
    let end = buf.iter().position(|b| *b == b'\n').unwrap_or(buf.len());
    Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// A pipe handle as a file for reading and writing; the handle stays owned by `h`.
fn as_file(h: &OwnedHandle) -> std::mem::ManuallyDrop<std::fs::File> {
    // SAFETY: the handle is valid while `h` lives; ManuallyDrop keeps File from closing it.
    std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(h.raw().0) })
}

impl PipeServer {
    /// Creates the pipe now (so a name already taken is reported) and serves it.
    pub fn start(name: &str, handler: impl Fn(&str) -> Reply + Send + 'static) -> std::io::Result<PipeServer> {
        let sd = OnlyUser::new(&user_sid()?)?;
        let first = create(name, &sd)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stop_t, name_t) = (stop.clone(), name.to_string());
        let thread = std::thread::Builder::new().name("ipc".into()).spawn(move || {
            let mut pipe = Some(first);
            loop {
                // Also after a connection: the wake-up of Drop may be the one just served.
                if stop_t.load(Ordering::SeqCst) {
                    return;
                }
                let h = match pipe.take() {
                    Some(h) => h,
                    None => match create(&name_t, &sd) {
                        Ok(h) => h,
                        Err(e) => {
                            tracing::error!(error = %e, "command pipe lost");
                            return;
                        }
                    },
                };
                // SAFETY: a pipe handle we own; blocking connect.
                let connected = unsafe { ConnectNamedPipe(h.raw(), None) };
                let ok = match connected {
                    Ok(()) => true,
                    Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                };
                if stop_t.load(Ordering::SeqCst) {
                    return;
                }
                if !ok {
                    continue;
                }
                let mut file = as_file(&h);
                let reply = match read_line(&mut file) {
                    Ok(line) => Some(handler(&line)),
                    Err(e) => {
                        tracing::debug!(error = %e, "command not read");
                        None
                    }
                };
                if let Some(r) = &reply {
                    let _ = file.write_all(r.line.as_bytes());
                    // SAFETY: our pipe handle; waits until the client has read the answer.
                    let _ = unsafe { FlushFileBuffers(h.raw()) };
                }
                // SAFETY: our pipe handle.
                let _ = unsafe { DisconnectNamedPipe(h.raw()) };
                drop(h);
                if let Some(after) = reply.and_then(|r| r.after) {
                    after();
                }
            }
        })?;
        Ok(PipeServer {
            name: name.to_string(),
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the blocking connect.
        let _ = open(&self.name, Duration::from_millis(200));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Opens the pipe, waiting up to `timeout` while the server is between connections (its
/// one instance is busy or being made again). No server: an error of kind `NotFound`.
fn open(name: &str, timeout: Duration) -> std::io::Result<OwnedHandle> {
    let wide = HSTRING::from(name);
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).max(1);
        // SAFETY: a valid name; waits at most `ms` for a free pipe instance.
        let attempt = if unsafe { WaitNamedPipeW(&wide, ms) }.as_bool() {
            // SAFETY: opening an existing pipe for reading and writing.
            unsafe {
                CreateFileW(
                    &wide,
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    None,
                )
            }
            .map_err(io_error)
        } else {
            Err(std::io::Error::last_os_error())
        };
        let error = match attempt {
            Ok(h) => return Ok(OwnedHandle::new(h)),
            Err(e) => e,
        };
        // Busy (another client took the instance) or missing for a moment (the server
        // is making the next one): try again until the time is up.
        let passing = [ERROR_PIPE_BUSY.0, ERROR_SEM_TIMEOUT.0, ERROR_FILE_NOT_FOUND.0]
            .iter()
            .any(|c| error.raw_os_error() == Some(*c as i32));
        if !passing || std::time::Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(RETRY);
    }
}

/// How long a client waits before trying a busy or briefly missing pipe again.
const RETRY: Duration = Duration::from_millis(5);

/// Sends one request line and returns the answer line. `before` gets the server's
/// process id first (the client lets it take the foreground for `launcher`). No server
/// gives an error of kind `NotFound`.
pub fn request(name: &str, line: &str, timeout: Duration, before: impl FnOnce(u32)) -> std::io::Result<String> {
    let h = open(name, timeout)?;
    let mut pid = 0u32;
    // SAFETY: our client handle; `pid` is a valid out pointer.
    if unsafe { GetNamedPipeServerProcessId(h.raw(), &mut pid) }.is_ok() {
        before(pid);
    }
    let mut file = as_file(&h);
    file.write_all(line.as_bytes())?;
    read_line(&mut file)
}

/// The SIDs the pipe's DACL allows, for the ACL test.
pub fn allowed_sids(name: &str) -> std::io::Result<Vec<String>> {
    let h = open(name, Duration::from_secs(2))?;
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: our handle; the descriptor holding the DACL is freed below.
    let r = unsafe {
        GetSecurityInfo(
            h.raw(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            Some(&mut sd),
        )
    };
    if r.is_err() {
        return Err(std::io::Error::from_raw_os_error(r.0 as i32));
    }
    let mut out = Vec::new();
    let mut info = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` points into `sd`, valid until it is freed; `info` has the right size.
    unsafe {
        if !dacl.is_null()
            && GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
            .is_ok()
        {
            for i in 0..info.AceCount {
                let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                if GetAce(dacl, i, &mut ace).is_ok() {
                    // Only access-allowed entries are in the descriptor we create.
                    let ace = ace as *const ACCESS_ALLOWED_ACE;
                    let sid = PSID(std::ptr::addr_of!((*ace).SidStart) as *mut _);
                    out.push(sid_text(sid)?);
                }
            }
        }
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Edge cases: a request answered and the after-action run; several requests in a
    //! row; a missing server; a second server on the same name refused; the DACL holds
    //! only the current user.
    use super::*;
    use std::sync::mpsc;

    fn name(test: &str) -> String {
        format!(r"\\.\pipe\kabuk-test-{test}-{}", std::process::id())
    }

    fn echo(tx: mpsc::Sender<String>) -> impl Fn(&str) -> Reply + Send + 'static {
        move |line| {
            let tx = tx.clone();
            let got = line.to_string();
            Reply {
                line: format!("re: {line}\n"),
                after: Some(Box::new(move || {
                    let _ = tx.send(got);
                })),
            }
        }
    }

    #[test]
    fn requests_are_answered_one_after_another() {
        let n = name("echo");
        let (tx, rx) = mpsc::channel();
        let _server = PipeServer::start(&n, echo(tx)).unwrap();
        let mut server_pid = 0;
        let a = request(&n, "ışık 1\n", Duration::from_secs(2), |pid| server_pid = pid).unwrap();
        assert_eq!(a, "re: ışık 1");
        assert_eq!(server_pid, std::process::id());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "ışık 1");
        assert_eq!(request(&n, "2\n", Duration::from_secs(2), |_| {}).unwrap(), "re: 2");
    }

    #[test]
    fn no_server_and_a_taken_name_are_errors() {
        let e = request(&name("none"), "x\n", Duration::from_millis(50), |_| {}).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        let n = name("twice");
        let (tx, _rx) = mpsc::channel();
        let _first = PipeServer::start(&n, echo(tx.clone())).unwrap();
        assert!(
            PipeServer::start(&n, echo(tx)).is_err(),
            "a second server on the same name"
        );
    }

    /// PLAN M7: the pipe is open only to the current user.
    #[test]
    fn only_the_current_user_may_open_the_pipe() {
        let n = name("acl");
        let (tx, _rx) = mpsc::channel();
        let _server = PipeServer::start(&n, echo(tx)).unwrap();
        let me = user_sid().unwrap();
        assert!(me.starts_with("S-1-"), "{me}");
        assert_eq!(allowed_sids(&n).unwrap(), vec![me]);
    }
}
