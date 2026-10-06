//! The only place that may start child processes (CLAUDE.md hard rule).
//! Every child is hidden (no console window), runs with UTF-8 I/O, and is put
//! in a Job Object so cancelling — or the app dying — ends the whole tree:
//! yt-dlp spawns ffmpeg, and the PyInstaller bootloader spawns python.

use std::ffi::OsStr;
use std::io;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_SUSPENDED: u32 = 0x0000_0004;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("program not found: {0}")]
    Missing(String),
    /// Access denied or a virus-scan error code on spawn: antivirus blocked or quarantined it.
    #[error("program blocked: {0}")]
    Blocked(String),
    #[error("timed out: {0}")]
    TimedOut(String),
    /// Our own Win32 plumbing failed (Job Object, resuming the thread) — not the user's machine.
    #[error("internal: {0}")]
    Internal(io::Error),
    #[error("{0}")]
    Other(#[from] io::Error),
}

/// Call once at startup. Children inherit the error mode, so a missing DLL in a
/// tool fails fast instead of opening a hidden "system error" box that hangs it.
pub fn init() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            SetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX,
        };
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

pub struct Spawned {
    pub child: Child,
    group: JobGroup,
}

impl Spawned {
    pub fn kill_tree(&self) {
        self.group.terminate();
    }
}

/// The child starts suspended and only runs after it is in the Job Object, so
/// nothing it spawns can escape the job.
pub fn spawn(cmd: &mut Command) -> Result<Spawned, SpawnError> {
    let label = format!("{:?}", cmd.as_std().get_program());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    let mut child = cmd.spawn().map_err(|e| classify(e, &label))?;
    let started = JobGroup::new().and_then(|group| {
        group.assign(&child)?;
        resume(&child)?;
        Ok(group)
    });
    match started {
        Ok(group) => Ok(Spawned { child, group }),
        Err(err) => {
            let _ = child.start_kill();
            Err(SpawnError::Internal(err))
        }
    }
}

/// Runs to completion, killing the whole tree if it takes longer than `limit`.
pub async fn run(cmd: &mut Command, limit: Duration) -> Result<Output, SpawnError> {
    let label = format!("{:?}", cmd.as_std().get_program());
    let Spawned { mut child, group } = spawn(cmd)?;
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let status = match tokio::time::timeout(limit, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            group.terminate();
            let _ = child.wait().await;
            return Err(SpawnError::TimedOut(label));
        }
    };
    Ok(Output {
        status,
        stdout: stdout.await.unwrap_or_default(),
        stderr: stderr.await.unwrap_or_default(),
    })
}

fn read_all<R>(pipe: Option<R>) -> tokio::task::JoinHandle<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf).await;
        }
        buf
    })
}

fn classify(err: io::Error, label: &str) -> SpawnError {
    // ERROR_FILE_NOT_FOUND 2, ERROR_PATH_NOT_FOUND 3, ERROR_ACCESS_DENIED 5,
    // ERROR_VIRUS_INFECTED 225, ERROR_VIRUS_DELETED 226
    match err.raw_os_error() {
        Some(2 | 3) => SpawnError::Missing(label.to_owned()),
        Some(5 | 225 | 226) => SpawnError::Blocked(label.to_owned()),
        _ => match err.kind() {
            io::ErrorKind::NotFound => SpawnError::Missing(label.to_owned()),
            io::ErrorKind::PermissionDenied => SpawnError::Blocked(label.to_owned()),
            _ => SpawnError::Other(err),
        },
    }
}

#[cfg(windows)]
fn resume(child: &Child) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let pid = child.id().ok_or_else(|| io::Error::other("child already exited"))?;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut ok = Thread32First(snapshot, &mut entry);
        while ok != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    if ResumeThread(thread) != u32::MAX {
                        resumed += 1;
                    }
                    CloseHandle(thread);
                }
            }
            ok = Thread32Next(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        if resumed == 0 {
            return Err(io::Error::other("no thread resumed"));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn resume(_child: &Child) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
struct JobGroup(windows_sys::Win32::Foundation::HANDLE);

// The job handle is only used through thread-safe Win32 calls.
#[cfg(windows)]
unsafe impl Send for JobGroup {}
#[cfg(windows)]
unsafe impl Sync for JobGroup {}

#[cfg(windows)]
impl JobGroup {
    fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let group = Self(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(group)
        }
    }

    fn assign(&self, child: &Child) -> io::Result<()> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("child already exited"))?;
        if unsafe { AssignProcessToJobObject(self.0, process as _) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for JobGroup {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct JobGroup;

#[cfg(not(windows))]
impl JobGroup {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }
    fn assign(&self, _child: &Child) -> io::Result<()> {
        Ok(())
    }
    fn terminate(&self) {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Instant;

    const LIMIT: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn runs_a_hidden_child_and_captures_output() {
        let out = run(command("cmd.exe").args(["/C", "echo hi"]), LIMIT).await.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[tokio::test]
    async fn missing_program_is_classified() {
        let err = run(&mut command("definitely-not-a-real-program-xyz.exe"), LIMIT)
            .await
            .unwrap_err();
        assert!(matches!(err, SpawnError::Missing(_)), "{err:?}");
    }

    #[test]
    fn virus_scan_errors_count_as_blocked() {
        for code in [5, 225, 226] {
            let err = classify(io::Error::from_raw_os_error(code), "x");
            assert!(matches!(err, SpawnError::Blocked(_)), "{code}: {err:?}");
        }
    }

    #[tokio::test]
    async fn kill_tree_terminates_the_job() {
        let mut spawned = spawn(command("cmd.exe").args(["/C", "ping -n 30 127.0.0.1 >NUL"])).unwrap();
        let started = Instant::now();
        spawned.kill_tree();
        spawned.child.wait().await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn hung_child_times_out_and_is_killed() {
        let started = Instant::now();
        let err = run(
            command("cmd.exe").args(["/C", "ping -n 30 127.0.0.1 >NUL"]),
            Duration::from_millis(500),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, SpawnError::TimedOut(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
