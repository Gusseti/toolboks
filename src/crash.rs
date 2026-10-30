//! Crash dialog. A panic hook that tells the user in plain words, and offers to copy a report
//! to the clipboard. Nothing is written to disk and nothing is sent anywhere.
//!
//! The hook runs at the panic site, before unwinding or the abort that `panic = "abort"`
//! performs, so it works in release builds too.

use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u32 = 13;

static IN_HOOK: AtomicBool = AtomicBool::new(false);

/// Installs the hook. Call first thing in `main`.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        // A panic while handling a panic (or a second thread panicking meanwhile) must not
        // recurse; the default abort follows either way.
        if IN_HOOK.swap(true, Ordering::SeqCst) {
            return;
        }
        // Any failure in here is swallowed; the process aborts right after anyway.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(info)));
    }));
}

fn handle(info: &PanicHookInfo<'_>) {
    let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message)".to_string()
    };
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_else(|| "unknown".to_string());
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let report = format_report(
        env!("CARGO_PKG_VERSION"),
        &message,
        &location,
        &thread,
        &backtrace,
    );

    let text = wide("toolboks ran into a problem and has to close. Nothing was saved or sent. Copy the details so you can report it?");
    let title = wide("toolboks stopped");
    // SAFETY: both buffers are NUL-terminated and outlive the call.
    let answer = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    if answer == IDYES {
        copy_to_clipboard(&report);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Plain-text report for the clipboard.
pub fn format_report(
    version: &str,
    message: &str,
    location: &str,
    thread: &str,
    backtrace: &str,
) -> String {
    format!(
        "toolboks {version} crashed\n\
         Message: {message}\n\
         Location: {location}\n\
         Thread: {thread}\n\
         \n\
         Backtrace (symbols are stripped in release builds, so these are addresses only):\n\
         {backtrace}\n"
    )
}

/// Closes the clipboard on drop.
struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: only constructed after a successful OpenClipboard on this thread.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn copy_to_clipboard(text: &str) {
    let data = wide(text);
    let bytes = data.len() * std::mem::size_of::<u16>();
    // SAFETY: standard clipboard sequence. The global block is freed here unless the clipboard
    // accepted it, in which case the system owns it.
    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return;
        }
        let _guard = ClipboardGuard;
        if EmptyClipboard().is_err() {
            return;
        }
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
            return;
        };
        let dst = GlobalLock(mem) as *mut u16;
        if dst.is_null() {
            let _ = GlobalFree(Some(mem));
            return;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        let _ = GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_err() {
            let _ = GlobalFree(Some(mem));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format_report;

    #[test]
    fn report_has_all_parts() {
        let r = format_report("0.7.0", "boom", "src/x.rs:10", "main", "0: 0xdead");
        assert!(r.starts_with("toolboks 0.7.0 crashed\n"));
        assert!(r.contains("Message: boom\n"));
        assert!(r.contains("Location: src/x.rs:10\n"));
        assert!(r.contains("Thread: main\n"));
        assert!(r.contains("addresses only"));
        assert!(r.contains("0: 0xdead"));
    }
}
