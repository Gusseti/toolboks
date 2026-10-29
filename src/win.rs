//! Small shared Win32 helpers.

use windows::Win32::Foundation::{CloseHandle, HANDLE};

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
