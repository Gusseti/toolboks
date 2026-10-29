//! Per-process working-set trim and the policy for which processes may be trimmed.
//!
//! `EmptyWorkingSet` needs no admin rights for processes of the same user. The trimmed pages
//! move to the standby list and fault back in when the process touches them again.

use std::collections::HashSet;

use windows::core::HRESULT;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::System::ProcessStatus::{
    EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
};

/// System and shell processes that are never trimmed. Compared case-insensitively.
const PROTECTED: &[&str] = &[
    "system",
    "system idle process",
    "registry",
    "memory compression",
    "secure system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "dwm.exe",
    "explorer.exe",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Windows or the shell: trimming gains nothing and can hurt responsiveness.
    System,
    /// This app itself.
    Own,
    /// The user excluded it.
    Excluded,
}

impl SkipReason {
    pub fn describe(self) -> &'static str {
        match self {
            SkipReason::System => "System and shell processes are never trimmed",
            SkipReason::Own => "This is toolboks itself",
            SkipReason::Excluded => "Excluded from trimming",
        }
    }
}

/// Why this process must not be trimmed, or `None` if it may be.
/// `exclusions` holds lowercase process names.
pub fn skip_reason(
    pid: u32,
    name: &str,
    own_pid: u32,
    exclusions: &HashSet<String>,
) -> Option<SkipReason> {
    let lower = name.to_lowercase();
    if pid == own_pid {
        Some(SkipReason::Own)
    } else if pid == 0 || pid == 4 || PROTECTED.contains(&lower.as_str()) {
        Some(SkipReason::System)
    } else if exclusions.contains(&lower) {
        Some(SkipReason::Excluded)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trimmed {
    /// Working set in bytes just before and just after the trim.
    pub before: u64,
    pub after: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimError {
    /// Protected process or one owned by another user; needs elevation.
    AccessDenied,
    /// Anything else, typically the process exited in the meantime.
    Failed,
}

/// Closes the process handle on every exit path.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `OpenProcess` and is closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn working_set(handle: &Handle) -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    counters.cb = size;
    // SAFETY: `counters` is a valid, correctly sized out-parameter for the call's duration.
    unsafe { GetProcessMemoryInfo(handle.0, &mut counters, size) }.ok()?;
    Some(counters.WorkingSetSize as u64)
}

pub fn trim_process(pid: u32) -> Result<Trimmed, TrimError> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    }
    .map(Handle)
    .map_err(|e| {
        if e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
            TrimError::AccessDenied
        } else {
            TrimError::Failed
        }
    })?;

    let before = working_set(&handle).ok_or(TrimError::Failed)?;
    // SAFETY: the handle is valid and was opened with PROCESS_SET_QUOTA.
    unsafe { EmptyWorkingSet(handle.0) }.map_err(|e| {
        if e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
            TrimError::AccessDenied
        } else {
            TrimError::Failed
        }
    })?;
    let after = working_set(&handle).ok_or(TrimError::Failed)?;
    Ok(Trimmed { before, after })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn protects_system_and_shell_processes_case_insensitively() {
        assert_eq!(
            skip_reason(4, "System", 999, &none()),
            Some(SkipReason::System)
        );
        assert_eq!(
            skip_reason(500, "CSRSS.exe", 999, &none()),
            Some(SkipReason::System)
        );
        assert_eq!(
            skip_reason(600, "Explorer.EXE", 999, &none()),
            Some(SkipReason::System)
        );
        assert_eq!(
            skip_reason(0, "anything", 999, &none()),
            Some(SkipReason::System)
        );
    }

    #[test]
    fn own_pid_is_skipped_even_if_excluded() {
        let ex: HashSet<String> = ["toolboks.exe".to_string()].into();
        assert_eq!(
            skip_reason(77, "toolboks.exe", 77, &ex),
            Some(SkipReason::Own)
        );
    }

    #[test]
    fn honours_exclusions_by_lowercase_name() {
        let ex: HashSet<String> = ["chrome.exe".to_string()].into();
        assert_eq!(
            skip_reason(10, "Chrome.exe", 999, &ex),
            Some(SkipReason::Excluded)
        );
        assert_eq!(skip_reason(11, "firefox.exe", 999, &ex), None);
    }

    #[test]
    fn trims_a_child_process() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 > nul"])
            .spawn()
            .expect("spawn child");
        let result = trim_process(child.id());
        let _ = child.kill();
        let _ = child.wait();

        let trimmed = result.expect("trim own child process");
        assert!(trimmed.before > 0, "child had no working set");
        assert!(
            trimmed.after <= trimmed.before,
            "working set grew: {trimmed:?}"
        );
    }

    #[test]
    fn missing_process_fails_cleanly() {
        // Pids are multiples of 4 on Windows, so this one is never valid.
        assert!(trim_process(u32::MAX - 1).is_err());
    }
}
