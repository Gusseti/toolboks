//! System-wide memory lists through `NtQuerySystemInformation` / `NtSetSystemInformation`
//! (information class 80, `SystemMemoryListInformation`).
//!
//! These are undocumented ntdll exports, so they are resolved at runtime and every failure is
//! reported instead of assumed away. Layout and command values were checked against
//! System Informer's `phnt` headers and ntdoc.m417z.com.
//!
//! Reading the lists is read-only. Commands need admin plus `SeProfileSingleProcessPrivilege`.

use std::ffi::c_void;

use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::win::Handle;

const SYSTEM_MEMORY_LIST_INFORMATION_CLASS: u32 = 80;
const STATUS_PRIVILEGE_NOT_HELD: i32 = 0xC000_0061_u32 as i32;

/// `SYSTEM_MEMORY_LIST_COMMAND` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Command {
    /// Trim the working set of every process.
    EmptyWorkingSets = 2,
    /// Write the modified (dirty) pages to disk so they become cached pages. Used by the Home
    /// page's deeper clean only; not part of `ALL`.
    FlushModifiedList = 3,
    /// Drop all cached (standby) pages.
    PurgeStandbyList = 4,
    /// Drop only the lowest-priority cached pages.
    PurgeLowPriorityStandbyList = 5,
}

impl Command {
    pub const ALL: [Command; 3] = [
        Command::PurgeLowPriorityStandbyList,
        Command::PurgeStandbyList,
        Command::EmptyWorkingSets,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Command::EmptyWorkingSets => "Trim all working sets",
            Command::FlushModifiedList => "Flush modified pages",
            Command::PurgeStandbyList => "Purge standby list",
            Command::PurgeLowPriorityStandbyList => "Purge low-priority standby",
        }
    }

    pub fn warning(self) -> &'static str {
        match self {
            Command::EmptyWorkingSets => {
                "Trims every process. Their memory moves to the standby list and is read back \
                 when used. This rarely helps in practice."
            }
            Command::FlushModifiedList => {
                "Writes changed memory pages to disk so they can be reused right away. \
                 Safe, but it can cause a short burst of disk activity."
            }
            Command::PurgeStandbyList => {
                "Drops all cached file data from RAM. Programs and games read from disk again, \
                 so things can feel slower for a while."
            }
            Command::PurgeLowPriorityStandbyList => {
                "Drops only the least valuable cached pages. Gentler than a full purge."
            }
        }
    }
}

/// `SYSTEM_MEMORY_LIST_INFORMATION`; every field is pointer-sized, counted in pages.
#[repr(C)]
#[derive(Default)]
struct RawLists {
    zero_page_count: usize,
    free_page_count: usize,
    modified_page_count: usize,
    modified_no_write_page_count: usize,
    bad_page_count: usize,
    page_count_by_priority: [usize; 8],
    repurposed_pages_by_priority: [usize; 8],
    modified_page_count_page_file: usize,
}

/// Memory list sizes in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lists {
    pub free: u64,
    pub modified: u64,
    pub standby: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemListError {
    /// Not elevated, or the privilege could not be enabled.
    NotElevated,
    /// The ntdll export is missing on this Windows version.
    Unavailable,
    /// Windows rejected the call with this NTSTATUS.
    Status(i32),
}

type NtQuery = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
type NtSet = unsafe extern "system" fn(u32, *mut c_void, u32) -> i32;

fn ntdll_export(name: windows::core::PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    // SAFETY: ntdll is always loaded; the returned pointer is only used with the matching type.
    unsafe {
        let module = GetModuleHandleW(w!("ntdll.dll")).ok()?;
        GetProcAddress(module, name)
    }
}

fn page_size() -> u64 {
    let mut info = SYSTEM_INFO::default();
    // SAFETY: `info` is a valid out-parameter.
    unsafe { GetSystemInfo(&mut info) };
    u64::from(info.dwPageSize)
}

/// Current free, modified and standby memory, or `None` if the query is unavailable.
pub fn query() -> Option<Lists> {
    let export = ntdll_export(s!("NtQuerySystemInformation"))?;
    // SAFETY: `NtQuerySystemInformation` has exactly this signature.
    let nt_query =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQuery>(export) };

    let mut raw = RawLists::default();
    let mut returned = 0u32;
    // SAFETY: `raw` matches SYSTEM_MEMORY_LIST_INFORMATION and its exact size is passed.
    let status = unsafe {
        nt_query(
            SYSTEM_MEMORY_LIST_INFORMATION_CLASS,
            &mut raw as *mut RawLists as *mut c_void,
            size_of::<RawLists>() as u32,
            &mut returned,
        )
    };
    if status < 0 {
        return None;
    }

    let page = page_size();
    let standby_pages: usize = raw.page_count_by_priority.iter().sum();
    Some(Lists {
        free: (raw.free_page_count + raw.zero_page_count) as u64 * page,
        modified: (raw.modified_page_count + raw.modified_no_write_page_count) as u64 * page,
        standby: standby_pages as u64 * page,
    })
}

fn enable_privilege(name: PCWSTR) -> bool {
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out-parameter; the handle is wrapped for closing right away.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    }
    .is_err()
    {
        return false;
    }
    let token = Handle(token);

    let mut luid = LUID::default();
    // SAFETY: `name` is a valid NUL-terminated wide string and `luid` a valid out-parameter.
    if unsafe { LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid) }.is_err() {
        return false;
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: `privileges` is a valid single-entry TOKEN_PRIVILEGES for the call's duration.
    // AdjustTokenPrivileges succeeds even when the privilege is not held, so the last error
    // has to be checked as well.
    unsafe {
        AdjustTokenPrivileges(token.0, false, Some(&privileges), 0, None, None).is_ok()
            && GetLastError() != ERROR_NOT_ALL_ASSIGNED
    }
}

fn classify(status: i32) -> Result<(), MemListError> {
    match status {
        s if s >= 0 => Ok(()),
        STATUS_PRIVILEGE_NOT_HELD => Err(MemListError::NotElevated),
        s => Err(MemListError::Status(s)),
    }
}

/// Runs one memory-list command. Needs admin; see the module docs.
pub fn run(command: Command) -> Result<(), MemListError> {
    if !enable_privilege(w!("SeProfileSingleProcessPrivilege")) {
        return Err(MemListError::NotElevated);
    }
    let export = ntdll_export(s!("NtSetSystemInformation")).ok_or(MemListError::Unavailable)?;
    // SAFETY: `NtSetSystemInformation` has exactly this signature.
    let nt_set =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, NtSet>(export) };

    let mut value = command as u32;
    // SAFETY: the buffer is the 4-byte command value the information class expects.
    let status = unsafe {
        nt_set(
            SYSTEM_MEMORY_LIST_INFORMATION_CLASS,
            &mut value as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    classify(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_values_match_the_nt_enum() {
        // SYSTEM_MEMORY_LIST_COMMAND: ... MemoryEmptyWorkingSets = 2, MemoryFlushModifiedList = 3,
        // MemoryPurgeStandbyList = 4, MemoryPurgeLowPriorityStandbyList = 5.
        assert_eq!(Command::EmptyWorkingSets as u32, 2);
        assert_eq!(Command::FlushModifiedList as u32, 3);
        assert_eq!(Command::PurgeStandbyList as u32, 4);
        assert_eq!(Command::PurgeLowPriorityStandbyList as u32, 5);
    }

    #[test]
    fn raw_layout_has_the_documented_size() {
        // 5 counters + 8 + 8 priority arrays + 1 page-file counter, all pointer-sized.
        assert_eq!(size_of::<RawLists>(), (5 + 8 + 8 + 1) * size_of::<usize>());
    }

    #[test]
    fn classifies_statuses() {
        assert_eq!(classify(0), Ok(()));
        assert_eq!(
            classify(STATUS_PRIVILEGE_NOT_HELD),
            Err(MemListError::NotElevated)
        );
        assert_eq!(
            classify(0xC000_0022_u32 as i32),
            Err(MemListError::Status(0xC000_0022_u32 as i32))
        );
    }

    #[test]
    fn reads_the_memory_lists() {
        // Read-only. Also answers whether the query works without admin rights.
        let lists = query().expect("SystemMemoryListInformation query failed");
        assert!(lists.standby > 0 || lists.free > 0, "{lists:?}");
    }
}
