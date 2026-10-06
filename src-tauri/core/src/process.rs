//! The only place that may start child processes (CLAUDE.md hard rule).
//! Every child is hidden (no console window), runs with UTF-8 I/O, and is put
//! in a Job Object so cancelling — or the app dying — ends the whole tree:
//! yt-dlp spawns ffmpeg, and the PyInstaller bootloader spawns python.

use std::ffi::OsStr;
use std::io;
use std::process::{Output, Stdio};

use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("program not found: {0}")]
    Missing(String),
    /// Access denied on spawn is almost always antivirus quarantine or blocking.
    #[error("program blocked: {0}")]
    Blocked(String),
    #[error("{0}")]
    Other(#[from] io::Error),
}

pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
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

pub fn spawn(cmd: &mut Command) -> Result<Spawned, SpawnError> {
    let label = format!("{:?}", cmd.as_std().get_program());
    let child = cmd.spawn().map_err(|e| classify(e, &label))?;
    let group = JobGroup::new()?;
    group.assign(&child)?;
    Ok(Spawned { child, group })
}

pub async fn run(cmd: &mut Command) -> Result<Output, SpawnError> {
    let spawned = spawn(cmd)?;
    // Keep the group alive until the child exits; dropping it kills the tree.
    let Spawned { child, group } = spawned;
    let output = child.wait_with_output().await?;
    drop(group);
    Ok(output)
}

fn classify(err: io::Error, label: &str) -> SpawnError {
    match err.kind() {
        io::ErrorKind::NotFound => SpawnError::Missing(label.to_owned()),
        io::ErrorKind::PermissionDenied => SpawnError::Blocked(label.to_owned()),
        _ => SpawnError::Other(err),
    }
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
        let Some(process) = child.raw_handle() else {
            return Ok(()); // already exited
        };
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
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn runs_a_hidden_child_and_captures_output() {
        let out = run(command("cmd.exe").args(["/C", "echo hi"])).await.unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[tokio::test]
    async fn missing_program_is_classified() {
        let err = run(&mut command("definitely-not-a-real-program-xyz.exe"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpawnError::Missing(_)), "{err:?}");
    }

    #[tokio::test]
    async fn kill_tree_terminates_the_job() {
        let mut spawned =
            spawn(command("cmd.exe").args(["/C", "ping -n 30 127.0.0.1 >NUL"])).unwrap();
        let started = Instant::now();
        spawned.kill_tree();
        spawned.child.wait().await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
