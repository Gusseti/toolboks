//! Single instance: a named mutex held for the process lifetime. A plain second launch brings
//! the first window forward and exits; a launch started by the app itself (update restart,
//! admin relaunch) carries `--relaunch` and waits for the old process to finish instead.
//! Nothing is stored on disk or in the registry.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HWND, LPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, SW_RESTORE, SW_SHOW,
};

use crate::win::Handle;

/// Marker argument added by the app's own restarts.
pub const RELAUNCH_ARG: &str = "--relaunch";
const MUTEX_NAME: PCWSTR = w!("Local\\toolboks-single-instance");
const WAIT: Duration = Duration::from_secs(5);

static RELAUNCHING: AtomicBool = AtomicBool::new(false);

/// Called right after this process started its replacement: a close request must then really
/// close the window, even in "Keep running in the tray" mode.
pub fn mark_relaunching() {
    RELAUNCHING.store(true, Ordering::SeqCst);
}

pub fn is_relaunching() -> bool {
    RELAUNCHING.load(Ordering::SeqCst)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Nobody else runs: start.
    Run,
    /// Our own restart while the old process is still closing: wait for it, then start.
    WaitThenRun,
    /// A normal launch while another one runs: show that one and exit.
    HandOver,
}

pub fn decide(is_relaunch: bool, already_held: bool) -> Action {
    match (already_held, is_relaunch) {
        (false, _) => Action::Run,
        (true, true) => Action::WaitThenRun,
        (true, false) => Action::HandOver,
    }
}

/// Keeps the mutex alive; dropping it (process exit) frees it.
#[allow(dead_code)]
pub struct Guard(Option<Handle>);

/// Tries to create the mutex. `Ok(handle)` if we are the first, `Err(())` if another process
/// holds it. If the mutex exists but cannot be opened (an elevated instance owns it), that
/// counts as held.
fn try_acquire() -> Result<Handle, ()> {
    // SAFETY: the name is a valid NUL-terminated wide string; the handle is wrapped at once.
    let created = unsafe { CreateMutexW(None, false, MUTEX_NAME) };
    // SAFETY: reads the thread's last error right after the call above.
    let err = unsafe { GetLastError() };
    match created {
        Ok(h) if err == ERROR_ALREADY_EXISTS => {
            drop(Handle(h));
            Err(())
        }
        Ok(h) => Ok(Handle(h)),
        Err(_) if err == ERROR_ACCESS_DENIED => Err(()),
        // Any other failure: do not block the app over a missing nicety.
        Err(_) => Ok(Handle(Default::default())),
    }
}

/// Decides what this launch does. Returns the guard to keep for the whole run, or `None` when
/// this process should exit now (it already told the other instance to show itself).
pub fn acquire(args: &[String]) -> Option<Guard> {
    let is_relaunch = args.iter().any(|a| a == RELAUNCH_ARG);
    let mut result = try_acquire();
    match decide(is_relaunch, result.is_err()) {
        Action::Run => result.ok().map(|h| Guard(Some(h))),
        Action::WaitThenRun => {
            let start = Instant::now();
            while result.is_err() && start.elapsed() < WAIT {
                sleep(Duration::from_millis(100));
                result = try_acquire();
            }
            // After the wait, start anyway: better two windows than none.
            Some(Guard(result.ok()))
        }
        Action::HandOver => {
            bring_forward();
            None
        }
    }
}

/// Finds the other toolboks window (winit class, title starts with "toolboks ") and shows it.
fn bring_forward() {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 32];
        let mut title = [0u16; 64];
        // SAFETY: the buffers are valid for the lengths passed.
        let (cl, tl) = unsafe {
            (
                GetClassNameW(hwnd, &mut class),
                GetWindowTextW(hwnd, &mut title),
            )
        };
        let class = String::from_utf16_lossy(&class[..cl.max(0) as usize]);
        let title = String::from_utf16_lossy(&title[..tl.max(0) as usize]);
        if class == "Window Class" && title.starts_with("toolboks ") {
            // SAFETY: `found` points to the `HWND` slot owned by `bring_forward`.
            unsafe { *(found.0 as *mut HWND) = hwnd };
            return false.into();
        }
        true.into()
    }

    let mut found = HWND::default();
    // SAFETY: the callback only writes to `found`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut found as *mut HWND as isize)) };
    if found.0.is_null() {
        return;
    }
    // SAFETY: `found` is a window handle from EnumWindows; all calls tolerate stale handles.
    unsafe {
        if IsIconic(found).as_bool() {
            let _ = ShowWindow(found, SW_RESTORE);
        } else if !IsWindowVisible(found).as_bool() {
            let _ = ShowWindow(found, SW_SHOW);
        }
        let _ = SetForegroundWindow(found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_else_means_run() {
        assert_eq!(decide(false, false), Action::Run);
        assert_eq!(decide(true, false), Action::Run);
    }

    #[test]
    fn restarts_wait_and_plain_launches_hand_over() {
        assert_eq!(decide(true, true), Action::WaitThenRun);
        assert_eq!(decide(false, true), Action::HandOver);
    }
}
