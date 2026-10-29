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
    relaunch_elevated_with_args(&[])
}

/// Like [`relaunch_elevated`], passing command-line arguments to the new copy.
pub fn relaunch_elevated_with_args(args: &[&str]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let params: Vec<u16> = join_args(args).encode_utf16().chain(Some(0)).collect();
    let params_ptr = if args.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(params.as_ptr())
    };
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
    result.0 as isize > 32
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
}
