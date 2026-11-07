//! Admin rights: detect them, and relaunch the app with them on demand.
//! Nothing is stored; the elevated instance starts fresh.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    EqualSid, GetTokenInformation, TokenElevation, TokenUser, SECURITY_MAX_SID_SIZE,
    TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowThreadProcessId, SW_SHOWNORMAL,
};

use crate::win::Handle;

pub fn is_elevated() -> bool {
    token_elevated().unwrap_or(false)
}

fn process_token(process: HANDLE) -> Option<Handle> {
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out-parameter; the handle is wrapped for closing right away.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    Some(Handle(token))
}

fn token_elevated() -> Option<bool> {
    // SAFETY: GetCurrentProcess returns the pseudo-handle for this process.
    let token = process_token(unsafe { GetCurrentProcess() })?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is a `TOKEN_ELEVATION` and its exact size is passed.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .ok()?;
    Some(elevation.TokenIsElevated != 0)
}

#[repr(C)]
struct TokenUserBuffer {
    user: TOKEN_USER,
    sid: [u8; SECURITY_MAX_SID_SIZE as usize],
}

impl TokenUserBuffer {
    fn new() -> Self {
        Self {
            user: TOKEN_USER::default(),
            sid: [0; SECURITY_MAX_SID_SIZE as usize],
        }
    }

    fn read(&mut self, token: &Handle) -> Option<()> {
        let mut returned = 0;
        // SAFETY: the aligned buffer has room for TOKEN_USER and the maximum SID length.
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some((self as *mut Self).cast()),
                size_of::<Self>() as u32,
                &mut returned,
            )
        }
        .ok()
    }
}

fn same_token_user(first: &Handle, second: &Handle) -> Option<bool> {
    let (mut first_user, mut second_user) = (TokenUserBuffer::new(), TokenUserBuffer::new());
    first_user.read(first)?;
    second_user.read(second)?;
    // SAFETY: both SIDs point into their respective live buffers filled by GetTokenInformation.
    Some(unsafe { EqualSid(first_user.user.User.Sid, second_user.user.User.Sid) }.is_ok())
}

fn same_shell_user() -> Option<bool> {
    // SAFETY: these calls query the shell window on the current desktop without modifying it.
    let shell = unsafe { GetShellWindow() };
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(shell, Some(&mut pid)) } == 0 {
        return None;
    }
    // SAFETY: OpenProcess validates the PID. Only query access is requested.
    let process =
        Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let shell_token = process_token(process.0)?;
    // SAFETY: GetCurrentProcess returns the pseudo-handle for this process.
    let current_token = process_token(unsafe { GetCurrentProcess() })?;
    same_token_user(&current_token, &shell_token)
}

fn allows_profile_changes(
    elevated: Option<bool>,
    same_user: impl FnOnce() -> Option<bool>,
) -> bool {
    elevated == Some(false) || same_user() == Some(true)
}

pub fn can_change_user_profile() -> bool {
    allows_profile_changes(token_elevated(), same_shell_user)
}

pub const USER_PROFILE_BLOCKED: &str =
    "Toolboks could not verify that it is running under your signed-in account. Reopen it without administrator rights to change your profile.";

pub fn require_user_profile() -> Result<(), String> {
    can_change_user_profile()
        .then_some(())
        .ok_or_else(|| USER_PROFILE_BLOCKED.into())
}

/// Starts a second, elevated copy of this exe through the UAC prompt.
/// Returns `false` if the user declined the prompt or the launch failed.
pub fn relaunch_elevated() -> bool {
    relaunch_elevated_with_args(&[])
}

/// Like [`relaunch_elevated`], passing command-line arguments to the new copy.
pub fn relaunch_elevated_with_args(args: &[&str]) -> bool {
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

/// Quotes one argument by the Windows command-line rules (`CommandLineToArgvW`).
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

fn join_args(args: &[&str]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_are_quoted() {
        assert_eq!(quote_arg("--open"), "--open");
        assert_eq!(
            quote_arg("Debloater/Privacy and ads"),
            "\"Debloater/Privacy and ads\""
        );
        assert_eq!(quote_arg(r"a b\"), r#""a b\\""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(join_args(&["--open", "x y"]), "--open \"x y\"");
    }

    #[test]
    fn elevation_check_does_not_fail() {
        // The answer depends on how the tests are run; it just must not panic.
        let _ = is_elevated();
    }

    #[test]
    fn elevated_profile_changes_require_a_matching_shell_user() {
        assert!(allows_profile_changes(Some(false), || None));
        assert!(allows_profile_changes(Some(true), || Some(true)));
        assert!(!allows_profile_changes(Some(true), || Some(false)));
        assert!(!allows_profile_changes(Some(true), || None));
        assert!(!allows_profile_changes(None, || None));
    }

    #[test]
    fn token_user_matches_itself() {
        let token = process_token(unsafe { GetCurrentProcess() }).unwrap();
        assert_eq!(same_token_user(&token, &token), Some(true));
    }
}
