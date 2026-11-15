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
        // The first panicking thread owns the dialog. A second one must not abort the process
        // while the user is reading it. Rust aborts recursive panics on the same thread itself.
        if IN_HOOK.swap(true, Ordering::SeqCst) {
            loop {
                std::thread::park();
            }
        }
        // Any failure in here is swallowed; the process aborts right after anyway.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(info)));
        std::process::abort();
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

    let text = wide(concat!(
        "Toolboks hit an unexpected error and has to close.\n\n",
        "Nothing was sent anywhere and nothing was saved to your disk.\n\n",
        "Do you want to copy a short report to the clipboard? Choose Yes to copy it, so you can ",
        "paste it into a bug report. Choose No to close without copying anything.",
    ));
    let title = wide("Toolboks has stopped");
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

/// Plain-text report about an add-on that stopped, for the clipboard. Like the crash report,
/// it is only copied when the user asks; nothing is written to disk or sent anywhere.
#[allow(clippy::too_many_arguments)]
pub fn format_addon_report<'a>(
    version: &str,
    name: &str,
    id: &str,
    addon_version: Option<&str>,
    state: &str,
    exit_code: Option<u32>,
    starts: u32,
    log: impl Iterator<Item = &'a str>,
) -> String {
    let mut report = format!(
        "toolboks {version} add-on report\n\
         Add-on: {name} ({id})\n\
         Add-on SDK: {}\n\
         State: {state}\n\
         Last exit code: {}\n\
         Starts this session: {starts}\n\
         \n\
         Last log lines:\n",
        addon_version.unwrap_or("unknown"),
        exit_code.map_or("none".to_string(), |code| format!("{code} (0x{code:08X})")),
    );
    let before = report.len();
    for line in log {
        report.push_str(line);
        report.push('\n');
    }
    if report.len() == before {
        report.push_str("(none)\n");
    }
    report
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

pub(crate) fn copy_to_clipboard(text: &str) {
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
    use super::{format_addon_report, format_report};

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

    #[test]
    fn addon_report_has_all_parts() {
        let r = format_addon_report(
            "0.12.5",
            "Disk Manager",
            "toolboks.disk-manager",
            Some("0.4.0"),
            "stopped: Crashed",
            Some(3),
            4,
            ["12:00:00 it ended with exit code 3"].into_iter(),
        );
        assert!(r.starts_with("toolboks 0.12.5 add-on report\n"));
        assert!(r.contains("Add-on: Disk Manager (toolboks.disk-manager)\n"));
        assert!(r.contains("Add-on SDK: 0.4.0\n"));
        assert!(r.contains("Last exit code: 3 (0x00000003)\n"));
        assert!(r.contains("Starts this session: 4\n"));
        assert!(r.ends_with("12:00:00 it ended with exit code 3\n"));
        let empty = format_addon_report("1", "A", "a", None, "running", None, 1, [].into_iter());
        assert!(empty.contains("Add-on SDK: unknown\n"));
        assert!(empty.ends_with("Last log lines:\n(none)\n"));
    }
}
