//! Add-on processes: the hash check before a start, the job object that ends them with the
//! host, and the two ways to start one (as the user, or elevated through a UAC prompt).

use std::fs::File;
use std::io::{self, Read};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, GetProcessId, ResumeThread, TerminateProcess,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::elevation::join_args;
use crate::win::Handle;

/// The exe is not the file that was installed.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    Missing(String),
    WrongHash,
}

/// Checks the exe against the SHA-256 recorded at install time. The returned file is open
/// without write or delete sharing: while it is held, the exe cannot be replaced, renamed or
/// deleted, so the file that was checked is the file that starts.
pub fn verify(exe: &Path, sha256: &str) -> Result<File, VerifyError> {
    let missing = |e: io::Error| VerifyError::Missing(e.to_string());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(exe)
        .map_err(missing)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(missing)?;
    if toolboks_catalog::sha256_hex(&bytes).eq_ignore_ascii_case(sha256) {
        Ok(file)
    } else {
        Err(VerifyError::WrongHash)
    }
}

/// A job object that ends every process in it when its last handle closes. The host holds the
/// only handle, so the add-ons end when Toolboks ends, also when it crashes.
pub struct Job(Handle);

// SAFETY: a job handle may be used from any thread; the kernel synchronizes the object.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> io::Result<Self> {
        // SAFETY: an unnamed job with default security; the handle is wrapped right away.
        let job = Handle(unsafe { CreateJobObjectW(None, PCWSTR::null()) }?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the buffer is the structure this information class expects, with its size.
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }?;
        Ok(Self(job))
    }
}

/// A started add-on process.
pub struct Process {
    handle: Handle,
    pid: Option<u32>,
    /// Started through the UAC prompt: not in the job, and the host may lack the rights to end
    /// it. It ends itself when the host closes the pipes.
    pub elevated: bool,
}

// SAFETY: a process handle may be used from any thread; the kernel synchronizes the object.
unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl Process {
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The exit code once the process has ended.
    pub fn exit_code(&self) -> Option<u32> {
        // SAFETY: the handle is open; a zero timeout only asks for the state.
        if unsafe { WaitForSingleObject(self.handle.0, 0) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0;
        // SAFETY: `code` is a valid out-parameter. An elevated process may refuse the query.
        match unsafe { GetExitCodeProcess(self.handle.0, &mut code) } {
            Ok(()) => Some(code),
            Err(_) => Some(0),
        }
    }

    /// Ends the process now. Best effort for an elevated one.
    pub fn kill(&self) {
        // SAFETY: the handle is open; a failure (already gone, no right) is fine.
        unsafe {
            let _ = TerminateProcess(self.handle.0, 1);
        }
    }
}

/// Starts `exe` as the current user, inside `job` before its first instruction runs.
pub fn spawn(job: &Job, exe: &Path, args: &[&str]) -> io::Result<Process> {
    let exe_text = exe.to_string_lossy();
    let mut all = vec![exe_text.as_ref()];
    all.extend_from_slice(args);
    let mut command: Vec<u16> = join_args(&all).encode_utf16().chain(Some(0)).collect();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    let directory = exe.parent().map(|d| HSTRING::from(d.as_os_str()));
    // Suspended, so the process joins the job before it can start children of its own. No
    // handles are inherited and it gets no console window.
    // SAFETY: every string is NUL-terminated and outlives the call; `command` is writable.
    unsafe {
        CreateProcessW(
            &HSTRING::from(exe.as_os_str()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            None,
            directory
                .as_ref()
                .map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
            &startup,
            &mut info,
        )
    }?;
    let thread = Handle(info.hThread);
    let process = Process {
        handle: Handle(info.hProcess),
        pid: Some(info.dwProcessId),
        elevated: false,
    };
    // SAFETY: both handles are open.
    if let Err(e) = unsafe { AssignProcessToJobObject(job.0 .0, process.handle.0) } {
        process.kill();
        return Err(e.into());
    }
    // SAFETY: the thread handle is open and the thread was created suspended.
    unsafe { ResumeThread(thread.0) };
    Ok(process)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ElevateError {
    /// The user said no at the UAC prompt.
    Declined,
    Failed(String),
}

/// Starts `exe` with administrator rights through the UAC prompt. Blocks until the user has
/// answered the prompt, so call it off the UI thread.
pub fn spawn_elevated(exe: &Path, args: &[&str]) -> Result<Process, ElevateError> {
    let file = HSTRING::from(exe.as_os_str());
    let parameters = HSTRING::from(join_args(args));
    let directory = exe.parent().map(|d| HSTRING::from(d.as_os_str()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: directory
            .as_ref()
            .map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: the structure and its strings outlive the call.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
            ElevateError::Declined
        } else {
            ElevateError::Failed(e.to_string())
        });
    }
    if info.hProcess.is_invalid() || info.hProcess == HANDLE::default() {
        return Err(ElevateError::Failed("no process was started".into()));
    }
    let handle = Handle(info.hProcess);
    // SAFETY: the handle is open. Zero means the handle lacks the right to ask.
    let pid = unsafe { GetProcessId(handle.0) };
    Ok(Process {
        handle,
        pid: (pid != 0).then_some(pid),
        elevated: true,
    })
}
