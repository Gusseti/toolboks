//! Small shared Win32 helpers.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Local wall-clock time as `HH:MM:SS`, for log lines.
pub fn local_time_hms() -> String {
    // SAFETY: plain FFI call with no arguments.
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

/// Owns a kernel handle and closes it on every exit path.
pub struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful Win32 call and is closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
