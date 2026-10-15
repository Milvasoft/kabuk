//! Other processes: wait for, query and stop them.

use std::time::Duration;

use windows::core::PWSTR;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use crate::handle::{OwnedHandle, Waitable};
use crate::sync::io_error;

#[derive(Debug)]
pub struct Process {
    pid: u32,
    handle: OwnedHandle,
}

impl Process {
    /// Opens a process of the current user for waiting, querying and terminating.
    pub fn open(pid: u32) -> std::io::Result<Process> {
        let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        // SAFETY: plain call; the returned handle is owned by the wrapper.
        let handle = unsafe { OpenProcess(access, false, pid) }.map_err(io_error)?;
        Ok(Process {
            pid,
            handle: OwnedHandle::new(handle),
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Waits for the process to exit. Returns true if it exited within `timeout`.
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        self.handle.wait(timeout)
    }

    /// The exit code, or `None` while the process runs.
    pub fn exit_code(&self) -> Option<u32> {
        if !self.handle.is_signalled() {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: valid handle with query rights; `code` is a valid out pointer.
        unsafe { GetExitCodeProcess(self.handle.raw(), &mut code) }.ok()?;
        // Checked signalled first, so an exit code of 259 (STILL_ACTIVE) is not mistaken for running.
        Some(code)
    }

    pub fn terminate(&self, exit_code: u32) -> std::io::Result<()> {
        // SAFETY: valid handle with terminate rights.
        unsafe { TerminateProcess(self.handle.raw(), exit_code) }.map_err(io_error)
    }

    /// Kernel plus user CPU time used so far.
    pub fn cpu_time(&self) -> Option<Duration> {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetProcessTimes;
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: valid handle with query rights and valid out pointers.
        unsafe { GetProcessTimes(self.handle.raw(), &mut c, &mut e, &mut k, &mut u) }.ok()?;
        let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        // FILETIME counts 100 ns units.
        Some(Duration::from_nanos((t(k) + t(u)) * 100))
    }

    /// CPU cycles used so far by all threads: exact, unlike the tick-sampled
    /// [`Process::cpu_time`], which is coarse (15.6 ms steps) for nearly idle processes.
    pub fn cycles(&self) -> Option<u64> {
        let mut c = 0u64;
        // SAFETY: valid handle with query rights; `c` is a valid out pointer.
        unsafe { windows::Win32::System::WindowsProgramming::QueryProcessCycleTime(self.handle.raw(), &mut c) }.ok()?;
        Some(c)
    }

    /// Private (committed, not shared) memory in bytes.
    pub fn private_bytes(&self) -> Option<usize> {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        };
        let mut m = PROCESS_MEMORY_COUNTERS_EX {
            cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>()).unwrap_or_default(),
            ..Default::default()
        };
        // SAFETY: the EX structure starts with PROCESS_MEMORY_COUNTERS and `cb` gives its size.
        unsafe {
            GetProcessMemoryInfo(
                self.handle.raw(),
                (&mut m as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                m.cb,
            )
        }
        .ok()?;
        Some(m.PrivateUsage)
    }

    /// File name of the executable, e.g. `kabuk.exe`.
    pub fn image_name(&self) -> Option<String> {
        let path = self.image_path()?;
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }

    /// The full path of the program's executable.
    pub fn image_path(&self) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let mut len = u32::try_from(buf.len()).unwrap_or_default();
        // SAFETY: `buf` holds `len` characters; the call updates `len` to the written length.
        unsafe { QueryFullProcessImageNameW(self.handle.raw(), PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }
            .ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }

    /// The app's identity when it is a packaged (Store) app, as `shell:AppsFolder` names
    /// it (e.g. `5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App`); `None` for other programs.
    pub fn app_user_model_id(&self) -> Option<String> {
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
        /// `APPLICATION_USER_MODEL_ID_MAX_LENGTH` with its terminating zero.
        const MAX: usize = 131;
        let mut buf = vec![0u16; MAX];
        let mut len = u32::try_from(buf.len()).unwrap_or_default();
        // SAFETY: `buf` holds `len` characters; the call writes at most that many, the
        // terminating zero included, and updates `len`.
        let r = unsafe { GetApplicationUserModelId(self.handle.raw(), &mut len, Some(PWSTR(buf.as_mut_ptr()))) };
        if r != ERROR_SUCCESS {
            return None;
        }
        let id = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1).min(MAX)]);
        (!id.is_empty()).then_some(id)
    }

    /// Whether the process's top-level windows of `classes` each answer within `wait`
    /// (DECISIONS D55). A hung window's thread is not waited for at all.
    pub fn answers(&self, classes: &[&str], wait: Duration) -> kabuk_core::hang::Probe {
        use kabuk_core::hang::Probe;
        use windows::Win32::UI::WindowsAndMessaging::WM_NULL;
        let asked: Vec<_> = crate::window::top_level_windows()
            .into_iter()
            .filter(|w| w.pid() == Some(self.pid) && classes.contains(&w.class_name().as_str()))
            .collect();
        if asked.is_empty() {
            return Probe::NoWindows;
        }
        let nothing = crate::msgloop::Msg {
            id: WM_NULL,
            wparam: 0,
            lparam: 0,
        };
        if asked
            .iter()
            .all(|w| crate::msgloop::send_message(*w, nothing, wait).is_ok())
        {
            Probe::Answered
        } else {
            Probe::Silent
        }
    }

    /// Waits up to `grace` for the process to exit, then terminates it with `code`.
    /// Returns true if it exited by itself.
    pub fn stop(&self, grace: Duration, code: u32) -> bool {
        if self.wait(Some(grace)) {
            return true;
        }
        if let Err(e) = self.terminate(code) {
            tracing::error!(pid = self.pid, error = %e, "terminate failed");
        }
        self.wait(Some(grace));
        false
    }
}

/// Cycles per second of one core at its current clock, measured by spinning on the
/// calling thread for `sample` (turbo and power saving make the nominal speed unreliable).
/// Whether process `pid` runs with higher rights than Kabuk (an elevated, "Run as
/// administrator" process), whose windows Kabuk cannot move (UIPI). A process whose
/// token Kabuk may not read counts as elevated; a process that cannot be opened at all
/// (gone) does not.
pub fn runs_elevated(pid: u32) -> bool {
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::OpenProcessToken;
    // SAFETY: plain call; the returned handle is owned by the wrapper below.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let process = OwnedHandle::new(process);
    let mut token = HANDLE::default();
    // SAFETY: valid process handle with query rights; `token` is a valid out pointer.
    if let Err(e) = unsafe { OpenProcessToken(process.raw(), TOKEN_QUERY, &mut token) } {
        return e.code() == ERROR_ACCESS_DENIED.to_hresult();
    }
    let token = OwnedHandle::new(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut size = 0u32;
    // SAFETY: `elevation` is a valid buffer of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).unwrap_or_default(),
            &mut size,
        )
    };
    ok.is_ok() && elevation.TokenIsElevated != 0
}

/// Lets process `pid` take the foreground (a tray icon's program opening its menu after
/// Kabuk forwarded the click). Works while Kabuk handles the user's input.
pub fn allow_foreground(pid: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    // SAFETY: plain call with a process id.
    unsafe { AllowSetForegroundWindow(pid) }.is_ok()
}

/// Posts `message` to `window` when process `pid` ends. Waits on its own thread without
/// waking up in between; dropping the watch stops it.
pub struct ExitWatch {
    stop: OwnedHandle,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ExitWatch {
    pub fn start(pid: u32, window: crate::window::Hwnd, message: u32) -> std::io::Result<ExitWatch> {
        use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
        // SAFETY: plain call; the handle is owned by the wrapper.
        let process = OwnedHandle::new(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.map_err(io_error)?);
        // SAFETY: an unnamed manual-reset event, owned by the wrapper.
        let stop = OwnedHandle::new(unsafe { CreateEventW(None, true, false, None) }.map_err(io_error)?);
        let stop_raw = stop.raw().0 as usize;
        let thread = std::thread::Builder::new().name("exit-watch".into()).spawn(move || {
            let handles = [
                process.raw(),
                windows::Win32::Foundation::HANDLE(stop_raw as *mut core::ffi::c_void),
            ];
            // SAFETY: both handles stay open for the wait: `process` lives in this thread,
            // `stop` until Drop has joined it.
            let r = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            if r.0 == 0 {
                let _ = crate::msgloop::post_message(
                    window,
                    crate::msgloop::Msg {
                        id: message,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            }
        })?;
        Ok(ExitWatch {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for ExitWatch {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::SetEvent;
        // SAFETY: the event handle is valid until this wrapper is dropped.
        let _ = unsafe { SetEvent(self.stop.raw()) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn cycles_per_second(sample: Duration) -> f64 {
    use windows::Win32::System::Threading::GetCurrentThread;
    use windows::Win32::System::WindowsProgramming::QueryThreadCycleTime;
    let read = || {
        let mut c = 0u64;
        // SAFETY: pseudo-handle of the current thread; valid out pointer.
        let _ = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
        c
    };
    let c0 = read();
    let t0 = std::time::Instant::now();
    while t0.elapsed() < sample {
        std::hint::black_box(0u64.wrapping_add(1));
    }
    let used = read().saturating_sub(c0) as f64;
    used / t0.elapsed().as_secs_f64()
}

impl Waitable for Process {
    fn wait_handle(&self) -> &OwnedHandle {
        &self.handle
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a running child has no exit code, terminate sets the requested code,
    //! a process that exits by itself reports its code, a bogus pid fails to open.
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn an_ordinary_program_has_no_app_id() {
        let me = Process::open(std::process::id()).unwrap();
        assert_eq!(me.app_user_model_id(), None);
    }

    #[test]
    fn exit_watch_posts_when_the_process_ends_and_stops_quietly() {
        use crate::msgloop::{run_until, MessageWindow, Wake};
        const ENDED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 9;
        let window = MessageWindow::new("Kabuk.Test.ExitWatch", "", |m| {
            if m.id == ENDED {
                crate::msgloop::post_quit(7);
                return Some(0);
            }
            None
        })
        .unwrap();
        let mut child = sleeper();
        let watch = ExitWatch::start(child.id(), window.hwnd(), ENDED).unwrap();
        child.kill().unwrap();
        let _ = child.wait();
        assert_eq!(run_until(&[], Some(Duration::from_secs(5))), Wake::Quit(7));
        drop(watch);
        // Dropped while the process still runs: no message, and drop does not hang.
        let mut other = sleeper();
        let watch = ExitWatch::start(other.id(), window.hwnd(), ENDED).unwrap();
        drop(watch);
        let _ = other.kill();
        let _ = other.wait();
        assert_eq!(run_until(&[], Some(Duration::from_millis(300))), Wake::Timeout);
        assert!(ExitWatch::start(0x7fff_fff0, window.hwnd(), ENDED).is_err());
    }

    fn sleeper() -> std::process::Child {
        // `ping` waits about a second per echo and exists on every Windows install.
        Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn terminate_sets_exit_code() {
        let mut child = sleeper();
        let p = Process::open(child.id()).unwrap();
        assert_eq!(p.pid(), child.id());
        assert_eq!(p.exit_code(), None);
        assert!(!p.wait(Some(Duration::ZERO)));
        p.terminate(77).unwrap();
        assert!(p.wait(Some(Duration::from_secs(5))));
        assert_eq!(p.exit_code(), Some(77));
        let _ = child.wait();
    }

    #[test]
    fn natural_exit_code() {
        let mut child = Command::new("cmd").args(["/c", "exit 3"]).spawn().unwrap();
        let p = Process::open(child.id()).unwrap();
        assert!(p.wait(Some(Duration::from_secs(10))));
        assert_eq!(p.exit_code(), Some(3));
        let _ = child.wait();
    }

    #[test]
    fn stop_terminates_after_grace_and_names_the_image() {
        let mut child = sleeper();
        let p = Process::open(child.id()).unwrap();
        assert_eq!(
            p.image_name().as_deref().map(str::to_ascii_lowercase).as_deref(),
            Some("ping.exe")
        );
        assert!(!p.stop(Duration::from_millis(50), 9));
        assert_eq!(p.exit_code(), Some(9));
        let _ = child.wait();
    }

    #[test]
    fn cpu_time_and_memory_of_ourselves() {
        let me = Process::open(std::process::id()).unwrap();
        let t0 = me.cpu_time().unwrap();
        let until = std::time::Instant::now() + Duration::from_millis(60);
        while std::time::Instant::now() < until {
            std::hint::black_box(1u64 + 1);
        }
        assert!(me.cpu_time().unwrap() > t0);
        assert!(me.private_bytes().is_some_and(|b| b > 100_000));
        let c0 = me.cycles().unwrap();
        let hz = cycles_per_second(Duration::from_millis(50));
        assert!(hz > 1e8 && hz < 1e11, "{hz}");
        assert!(me.cycles().unwrap() > c0);
    }

    #[test]
    fn bogus_pid_fails() {
        // Pids are multiples of four; 1 never names a process.
        assert!(Process::open(1).is_err());
    }
}
