//! Admin rights: detect them, and relaunch the app with them on demand.
//! Nothing is stored; the elevated instance starts fresh.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::win::Handle;

pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out-parameter; the handle is wrapped for closing right away.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let token = Handle(token);

    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a `TOKEN_ELEVATION` and its exact size is passed.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .is_ok();
    ok && elevation.TokenIsElevated != 0
}

/// Starts a second, elevated copy of this exe through the UAC prompt.
/// Returns `false` if the user declined the prompt or the launch failed.
pub fn relaunch_elevated() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success with a value greater than 32.
    result.0 as isize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_check_does_not_fail() {
        // The answer depends on how the tests are run; it just must not panic.
        let _ = is_elevated();
    }
}
