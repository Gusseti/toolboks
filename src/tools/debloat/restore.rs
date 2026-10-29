//! System Restore point before admin changes. See docs/RESEARCH-debloat.md section 10.
//! A restore point snapshots registry and system state; it does NOT bring back removed apps.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use super::backend::{Registry, WinRegistry};
use super::model::{Hive, RegValue};

/// What happened when a restore point was requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreStatus {
    /// A new restore point exists (its sequence number changed).
    Created {
        sequence: u64,
    },
    /// Windows reported success but made no new point (one exists from the last 24 hours).
    SkippedRecent {
        sequence: u64,
    },
    /// System Protection is off for the system drive, so nothing can be created.
    ProtectionOff,
    NeedsAdmin,
    Failed(String),
}

/// System Restore access behind a trait so the logic is tested with a fake.
pub trait RestoreApi {
    /// `Some(true/false)` if System Protection state could be read, `None` if unknown.
    fn protection_enabled(&self) -> Option<bool>;
    /// The `SystemRestorePointCreationFrequency` value in minutes, `None` if not set.
    fn frequency_minutes(&self) -> Option<u32>;
    /// `None` removes the value (restores the Windows default).
    fn set_frequency_minutes(&mut self, minutes: Option<u32>) -> Result<(), String>;
    /// Sequence number of the newest restore point, `None` if none or unknown.
    fn latest_sequence(&self) -> Option<u64>;
    /// SRSetRestorePointW begin (MODIFY_SETTINGS). Returns the sequence number Windows reports.
    fn begin(&mut self, description: &str) -> Result<u64, String>;
    fn end(&mut self, sequence: u64) -> Result<(), String>;
}

/// The real System Restore. srclient.dll is loaded at run time (never linked). There is no simple,
/// reliable read-only source for the newest restore point, so `latest_sequence` is the nonzero
/// number from the last successful `begin` in this process (`None` before). A skip because of the
/// 24 hour rule is therefore only seen when `begin` itself reports sequence 0.
pub struct WinRestore;

/// Nonzero sequence from the last successful `begin` (0 = none); static because callers build a
/// fresh `WinRestore` value for each call.
static LAST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const SR_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore";
const SR_POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows NT\SystemRestore";
const FREQ_VALUE: &str = "SystemRestorePointCreationFrequency";

const BEGIN_SYSTEM_CHANGE: u32 = 100;
const END_SYSTEM_CHANGE: u32 = 101;
const MODIFY_SETTINGS: u32 = 12;

/// RESTOREPOINTINFOW (Microsoft docs: 1-byte packing).
#[repr(C, packed(1))]
struct RestorePointInfoW {
    event_type: u32,
    restore_point_type: u32,
    sequence_number: i64,
    description: [u16; 64],
}

/// STATEMGRSTATUS (Microsoft docs: 1-byte packing).
#[repr(C, packed(1))]
struct StateMgrStatus {
    status: u32,
    sequence_number: i64,
}

type SetRestorePointFn =
    unsafe extern "system" fn(*const RestorePointInfoW, *mut StateMgrStatus) -> i32;

const OFF_TEXT: &str = "System Restore is turned off or not available";

fn read_dword(path: &str, name: &str) -> Option<u32> {
    match WinRegistry.read(Hive::LocalMachine, path, name) {
        Ok(Some(RegValue::Dword(v))) => Some(v),
        _ => None,
    }
}

fn call_sr(event: u32, sequence: i64, description: &str) -> Result<u64, String> {
    // SAFETY: plain LoadLibraryW/GetProcAddress with NUL-terminated names; the function pointer is
    // transmuted to the documented SRSetRestorePointW signature and used while the DLL stays loaded.
    unsafe {
        let name: Vec<u16> = "srclient.dll".encode_utf16().chain(Some(0)).collect();
        let lib = LoadLibraryW(PCWSTR(name.as_ptr())).map_err(|_| OFF_TEXT.to_string())?;
        let Some(proc) = GetProcAddress(lib, PCSTR(c"SRSetRestorePointW".as_ptr().cast())) else {
            let _ = FreeLibrary(lib);
            return Err(OFF_TEXT.to_string());
        };
        let func: SetRestorePointFn = std::mem::transmute(proc);
        let mut info = RestorePointInfoW {
            event_type: event,
            restore_point_type: MODIFY_SETTINGS,
            sequence_number: sequence,
            description: [0; 64],
        };
        for (i, ch) in description.encode_utf16().take(63).enumerate() {
            info.description[i] = ch;
        }
        let mut status = StateMgrStatus {
            status: 0,
            sequence_number: 0,
        };
        let ok = func(&info, &mut status);
        let code = status.status; // copy out of the packed struct
        let seq = status.sequence_number;
        let _ = FreeLibrary(lib);
        if ok == 0 {
            return Err(sr_error_text(code));
        }
        Ok(seq.max(0) as u64)
    }
}

fn sr_error_text(code: u32) -> String {
    match code {
        // ERROR_SERVICE_DISABLED, ERROR_SERVICE_DOES_NOT_EXIST, E_FAIL, ERROR_FILE_NOT_FOUND, none
        1058 | 1060 | 0x8000_4005 | 2 | 0 => OFF_TEXT.to_string(),
        5 => "Needs administrator rights".to_string(),
        n => format!("Windows could not make a restore point (error {n})"),
    }
}

impl RestoreApi for WinRestore {
    fn protection_enabled(&self) -> Option<bool> {
        if read_dword(SR_POLICY_KEY, "DisableSR") == Some(1) {
            return Some(false);
        }
        match read_dword(SR_KEY, "RPSessionInterval") {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        }
    }
    fn frequency_minutes(&self) -> Option<u32> {
        read_dword(SR_KEY, FREQ_VALUE)
    }
    fn set_frequency_minutes(&mut self, minutes: Option<u32>) -> Result<(), String> {
        match minutes {
            Some(m) => {
                WinRegistry.write(Hive::LocalMachine, SR_KEY, FREQ_VALUE, &RegValue::Dword(m))
            }
            None => WinRegistry.delete(Hive::LocalMachine, SR_KEY, FREQ_VALUE),
        }
    }
    fn latest_sequence(&self) -> Option<u64> {
        match LAST_SEQUENCE.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }
    fn begin(&mut self, description: &str) -> Result<u64, String> {
        let seq = call_sr(BEGIN_SYSTEM_CHANGE, 0, description)?;
        if seq != 0 {
            LAST_SEQUENCE.store(seq, Ordering::Relaxed);
        }
        Ok(seq)
    }
    fn end(&mut self, sequence: u64) -> Result<(), String> {
        call_sr(END_SYSTEM_CHANGE, sequence as i64, "").map(|_| ())
    }
}

/// Creates a restore point: checks admin and System Protection, sets the creation frequency to 0
/// for the call and puts the old value back afterwards (even on failure, tried twice), and compares
/// the latest sequence number before and after to tell `Created` from `SkippedRecent`.
pub fn create(api: &mut dyn RestoreApi, description: &str, elevated: bool) -> RestoreStatus {
    if !elevated {
        return RestoreStatus::NeedsAdmin;
    }
    if api.protection_enabled() == Some(false) {
        return RestoreStatus::ProtectionOff;
    }
    let before = api.latest_sequence();
    let old_freq = api.frequency_minutes();

    let result = match api.set_frequency_minutes(Some(0)) {
        Err(e) => Err(e),
        Ok(()) => api.begin(description).and_then(|seq| {
            // Sequence 0 means Windows made no point, so there is nothing to close.
            if seq != 0 {
                api.end(seq)?;
            }
            Ok(seq)
        }),
    };

    let mut restore_failed = false;
    if api.set_frequency_minutes(old_freq).is_err() {
        restore_failed = api.set_frequency_minutes(old_freq).is_err();
    }

    match result {
        Err(e) => {
            let mut text = friendly(&e);
            if restore_failed {
                text.push_str(
                    " The restore point setting could not be put back; check it in Windows.",
                );
            }
            RestoreStatus::Failed(text)
        }
        Ok(begun) => match (before, api.latest_sequence()) {
            (b, Some(a)) if b != Some(a) => RestoreStatus::Created { sequence: a },
            _ => RestoreStatus::SkippedRecent {
                sequence: before.unwrap_or(begun),
            },
        },
    }
}

fn friendly(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("1058") || l.contains("e_fail") || l.contains("disabled") {
        OFF_TEXT.to_string()
    } else {
        e.to_string()
    }
}

/// One plain sentence for the page.
pub fn status_text(status: &RestoreStatus) -> String {
    match status {
        RestoreStatus::Created { .. } => "Restore point created.".to_string(),
        RestoreStatus::SkippedRecent { .. } => "Windows already has a restore point from the last 24 hours, so no new one was made.".to_string(),
        RestoreStatus::ProtectionOff => "System Protection is off, so no restore point was made. You can still go on; every change can be undone in toolboks until you close it.".to_string(),
        RestoreStatus::NeedsAdmin => "Making a restore point needs administrator rights. Restart toolboks as administrator to use it.".to_string(),
        RestoreStatus::Failed(e) => format!("No restore point was made: {e}. You can still go on; every change can be undone in toolboks until you close it."),
    }
}

/// True when applying may go on (anything except NeedsAdmin is only a warning).
pub fn is_blocking(status: &RestoreStatus) -> bool {
    matches!(status, RestoreStatus::NeedsAdmin)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRestore {
        protection: Option<bool>,
        freq: Option<u32>,
        before: Option<u64>,
        after: Option<u64>,
        begin_result: Result<u64, String>,
        end_result: Result<(), String>,
        set_fails: bool,
        begun: bool,
        ended_with: Vec<u64>,
        freq_log: Vec<Option<u32>>,
    }

    impl FakeRestore {
        fn new() -> Self {
            Self {
                protection: Some(true),
                freq: Some(1440),
                before: Some(5),
                after: Some(6),
                begin_result: Ok(6),
                end_result: Ok(()),
                set_fails: false,
                begun: false,
                ended_with: vec![],
                freq_log: vec![],
            }
        }
    }

    impl RestoreApi for FakeRestore {
        fn protection_enabled(&self) -> Option<bool> {
            self.protection
        }
        fn frequency_minutes(&self) -> Option<u32> {
            self.freq
        }
        fn set_frequency_minutes(&mut self, minutes: Option<u32>) -> Result<(), String> {
            self.freq_log.push(minutes);
            if self.set_fails && minutes != Some(0) {
                return Err("denied".into());
            }
            self.freq = minutes;
            Ok(())
        }
        fn latest_sequence(&self) -> Option<u64> {
            if self.begun {
                self.after
            } else {
                self.before
            }
        }
        fn begin(&mut self, _d: &str) -> Result<u64, String> {
            self.begun = true;
            self.begin_result.clone()
        }
        fn end(&mut self, sequence: u64) -> Result<(), String> {
            self.ended_with.push(sequence);
            self.end_result.clone()
        }
    }

    #[test]
    fn needs_admin_and_protection_off() {
        let mut f = FakeRestore::new();
        assert_eq!(create(&mut f, "x", false), RestoreStatus::NeedsAdmin);
        assert!(!f.begun);
        f.protection = Some(false);
        assert_eq!(create(&mut f, "x", true), RestoreStatus::ProtectionOff);
        assert!(!f.begun);
        assert!(f.freq_log.is_empty());
    }

    #[test]
    fn created_restores_frequency_and_ends_with_sequence() {
        let mut f = FakeRestore::new();
        assert_eq!(
            create(&mut f, "x", true),
            RestoreStatus::Created { sequence: 6 }
        );
        assert_eq!(f.ended_with, vec![6]);
        assert_eq!(f.freq, Some(1440));
        assert_eq!(f.freq_log, vec![Some(0), Some(1440)]);
    }

    #[test]
    fn unknown_protection_still_tries_and_unset_frequency_is_removed_again() {
        let mut f = FakeRestore::new();
        f.protection = None;
        f.freq = None;
        f.before = None;
        assert_eq!(
            create(&mut f, "x", true),
            RestoreStatus::Created { sequence: 6 }
        );
        assert_eq!(f.freq, None);
    }

    #[test]
    fn skipped_recent_when_sequence_unchanged() {
        let mut f = FakeRestore::new();
        f.after = Some(5);
        f.begin_result = Ok(5);
        assert_eq!(
            create(&mut f, "x", true),
            RestoreStatus::SkippedRecent { sequence: 5 }
        );
        assert_eq!(f.freq, Some(1440));
    }

    #[test]
    fn begin_zero_is_skipped_and_not_ended() {
        let mut f = FakeRestore::new();
        f.after = f.before;
        f.begin_result = Ok(0);
        assert_eq!(
            create(&mut f, "x", true),
            RestoreStatus::SkippedRecent { sequence: 5 }
        );
        assert!(f.ended_with.is_empty());
    }

    #[test]
    fn failure_restores_frequency() {
        let mut f = FakeRestore::new();
        f.begin_result = Err("boom".into());
        match create(&mut f, "x", true) {
            RestoreStatus::Failed(t) => assert!(!t.is_empty()),
            other => panic!("{other:?}"),
        }
        assert_eq!(f.freq, Some(1440));

        let mut g = FakeRestore::new();
        g.freq = None;
        g.end_result = Err("1058".into());
        match create(&mut g, "x", true) {
            RestoreStatus::Failed(t) => assert!(t.contains("turned off")),
            other => panic!("{other:?}"),
        }
        assert_eq!(g.freq, None);
    }

    #[test]
    fn restore_failure_is_tried_twice_and_mentioned() {
        let mut f = FakeRestore::new();
        f.set_fails = true;
        f.begin_result = Err("boom".into());
        match create(&mut f, "x", true) {
            RestoreStatus::Failed(t) => assert!(t.contains("put back")),
            other => panic!("{other:?}"),
        }
        assert_eq!(f.freq_log, vec![Some(0), Some(1440), Some(1440)]);
    }

    #[test]
    fn texts_and_blocking() {
        let all = [
            RestoreStatus::Created { sequence: 1 },
            RestoreStatus::SkippedRecent { sequence: 1 },
            RestoreStatus::ProtectionOff,
            RestoreStatus::NeedsAdmin,
            RestoreStatus::Failed("x".into()),
        ];
        for s in &all {
            assert!(!status_text(s).is_empty());
            assert_eq!(is_blocking(s), *s == RestoreStatus::NeedsAdmin);
        }
    }

    #[test]
    fn struct_sizes_match_docs() {
        assert_eq!(std::mem::size_of::<RestorePointInfoW>(), 144);
        assert_eq!(std::mem::size_of::<StateMgrStatus>(), 12);
    }

    #[test]
    #[ignore = "reads the real registry"]
    fn smoke_read_only() {
        let w = WinRestore;
        println!(
            "protection={:?} frequency={:?}",
            w.protection_enabled(),
            w.frequency_minutes()
        );
    }
}
