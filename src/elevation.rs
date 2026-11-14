//! Admin rights: detect them, and relaunch the app with them on demand.
//! Nothing is stored; the elevated instance starts fresh.
//!
//! Detection lives in `toolboks_shared::elevation` and is re-exported here; the relaunch stays in
//! the host because it depends on the updater and the single-instance guard.

use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub use toolboks_shared::elevation::*;

/// Starts a second, elevated copy of this exe through the UAC prompt.
/// Returns `false` if the user declined the prompt or the launch failed.
pub fn relaunch_elevated() -> bool {
    relaunch_elevated_with_args(&[])
}

/// Like [`relaunch_elevated`], passing command-line arguments to the new copy.
pub fn relaunch_elevated_with_args(args: &[&str]) -> bool {
    if crate::update::is_installing() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    // `--relaunch` tells the new copy to wait for this one to close instead of handing over.
    let mut all = vec![crate::instance::RELAUNCH_ARG];
    all.extend_from_slice(args);
    let params: Vec<u16> = join_args(&all).encode_utf16().chain(Some(0)).collect();
    let params_ptr = PCWSTR(params.as_ptr());
    // SAFETY: `wide` and `params` are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(wide.as_ptr()),
            params_ptr,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success with a value greater than 32.
    let started = result.0 as isize > 32;
    if started {
        crate::instance::mark_relaunching();
    }
    started
}
