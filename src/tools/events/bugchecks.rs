//! Bug check (stop code) table: about 50 common codes. Names are checked against Microsoft's bug
//! check code reference; the sentences are plain-language summaries and name a tendency, never a
//! certainty. 0x141 is deliberately absent (not in Microsoft's master table).
#![allow(dead_code)]

/// Who is usually to blame for a stop code (a tendency, never a certainty).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Hardware,
    Driver,
    Software,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct BugCheck {
    pub code: u32,
    /// e.g. `DRIVER_IRQL_NOT_LESS_OR_EQUAL`.
    pub name: &'static str,
    /// One plain sentence.
    pub sentence: &'static str,
    pub cause: Cause,
}

const fn bc(code: u32, name: &'static str, cause: Cause, sentence: &'static str) -> BugCheck {
    BugCheck {
        code,
        name,
        sentence,
        cause,
    }
}

use Cause::{Driver, Hardware, Software, Unknown};

static TABLE: &[BugCheck] = &[
    bc(
        0x0A,
        "IRQL_NOT_LESS_OR_EQUAL",
        Driver,
        "Windows or a driver touched memory at the wrong moment, which is usually a driver bug and sometimes bad memory.",
    ),
    bc(
        0x19,
        "BAD_POOL_HEADER",
        Driver,
        "Windows found damaged bookkeeping in its memory pool, often from a misbehaving driver or faulty memory.",
    ),
    bc(
        0x1A,
        "MEMORY_MANAGEMENT",
        Hardware,
        "Windows' memory manager found something badly wrong, which often points to faulty memory but can also be a driver or disk problem.",
    ),
    bc(
        0x1E,
        "KMODE_EXCEPTION_NOT_HANDLED",
        Driver,
        "A core part of Windows hit an error that nothing handled, which is usually a driver bug.",
    ),
    bc(
        0x24,
        "NTFS_FILE_SYSTEM",
        Hardware,
        "The driver for the disk's file system hit a problem it could not recover from, often from disk errors or a damaged file system.",
    ),
    bc(
        0x3B,
        "SYSTEM_SERVICE_EXCEPTION",
        Driver,
        "An error happened while a program called into Windows' core, often tied to graphics or security software drivers.",
    ),
    bc(
        0x3F,
        "NO_MORE_SYSTEM_PTES",
        Driver,
        "Windows ran out of a core address resource, which is usually a driver leaking it.",
    ),
    bc(
        0x4E,
        "PFN_LIST_CORRUPT",
        Driver,
        "Windows' list of memory pages was damaged, which is often a bad driver or bad memory.",
    ),
    bc(
        0x50,
        "PAGE_FAULT_IN_NONPAGED_AREA",
        Unknown,
        "Windows tried to use memory that should exist but did not, which can be bad memory, a buggy driver or damaged system or security software.",
    ),
    bc(
        0x51,
        "REGISTRY_ERROR",
        Hardware,
        "Windows hit a serious problem reading its registry, which is often disk or file damage.",
    ),
    bc(
        0x5A,
        "CRITICAL_SERVICE_FAILED",
        Software,
        "A service Windows cannot run without failed to start, which is often a damaged system file or driver.",
    ),
    bc(
        0x77,
        "KERNEL_STACK_INPAGE_ERROR",
        Hardware,
        "Windows could not read part of its working memory back from disk, which often points to a failing drive, cable or controller.",
    ),
    bc(
        0x7A,
        "KERNEL_DATA_INPAGE_ERROR",
        Hardware,
        "Windows could not read needed data from the disk, which often points to a failing drive, cable, controller or memory.",
    ),
    bc(
        0x7B,
        "INACCESSIBLE_BOOT_DEVICE",
        Hardware,
        "Windows could not reach the drive it starts from, which often follows a storage setting or driver change, or a disk problem.",
    ),
    bc(
        0x7E,
        "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED",
        Driver,
        "A background part of Windows, usually inside a driver, crashed with an error nothing handled.",
    ),
    bc(
        0x7F,
        "UNEXPECTED_KERNEL_MODE_TRAP",
        Driver,
        "The processor raised a fault Windows could not handle, which is often a driver bug and sometimes hardware.",
    ),
    bc(
        0x9C,
        "MACHINE_CHECK_EXCEPTION",
        Hardware,
        "The processor reported a fatal hardware error.",
    ),
    bc(
        0x9F,
        "DRIVER_POWER_STATE_FAILURE",
        Driver,
        "A device driver did not answer a sleep, wake or shutdown request in time, which is usually a driver problem.",
    ),
    bc(
        0xA0,
        "INTERNAL_POWER_ERROR",
        Driver,
        "Windows' power manager hit a serious error while changing power state, often a driver or firmware issue.",
    ),
    bc(
        0xA5,
        "ACPI_BIOS_ERROR",
        Hardware,
        "The computer's firmware (BIOS) usually does not follow the rules Windows expects, and a BIOS update often helps.",
    ),
    bc(
        0xBE,
        "ATTEMPTED_WRITE_TO_READONLY_MEMORY",
        Driver,
        "A driver tried to write to protected memory, which is usually a driver bug.",
    ),
    bc(
        0xC1,
        "SPECIAL_POOL_DETECTED_MEMORY_CORRUPTION",
        Driver,
        "A driver wrote to memory just outside what it had been given, which is usually a driver bug.",
    ),
    bc(
        0xC2,
        "BAD_POOL_CALLER",
        Driver,
        "A driver or Windows component asked for memory in an invalid way, which is usually a driver bug.",
    ),
    bc(
        0xC4,
        "DRIVER_VERIFIER_DETECTED_VIOLATION",
        Driver,
        "Driver Verifier, a driver testing tool, caught a driver breaking the rules, which usually means that tool is switched on.",
    ),
    bc(
        0xC5,
        "DRIVER_CORRUPTED_EXPOOL",
        Driver,
        "A driver damaged Windows' memory pool, which is usually a driver bug.",
    ),
    bc(
        0xCE,
        "DRIVER_UNLOADED_WITHOUT_CANCELLING_PENDING_OPERATIONS",
        Driver,
        "A driver was removed while it still had work scheduled, which is usually a driver bug.",
    ),
    bc(
        0xD1,
        "DRIVER_IRQL_NOT_LESS_OR_EQUAL",
        Driver,
        "A driver touched memory when it was not allowed to, which is usually a bad pointer in the driver, often a network, storage or security driver.",
    ),
    bc(
        0xE2,
        "MANUALLY_INITIATED_CRASH",
        Software,
        "Someone deliberately triggered this crash with the keyboard or a tool.",
    ),
    bc(
        0xEA,
        "THREAD_STUCK_IN_DEVICE_DRIVER",
        Driver,
        "A driver, often the graphics driver, got stuck in a loop and froze the computer.",
    ),
    bc(
        0xED,
        "UNMOUNTABLE_BOOT_VOLUME",
        Hardware,
        "Windows could not open its own drive, which often means file damage or a disk problem.",
    ),
    bc(
        0xEF,
        "CRITICAL_PROCESS_DIED",
        Software,
        "A process Windows needs to run ended unexpectedly, which can be a damaged system file, a driver, the disk or memory.",
    ),
    bc(
        0xF4,
        "CRITICAL_OBJECT_TERMINATION",
        Hardware,
        "A process Windows needs ended unexpectedly, which is often storage trouble.",
    ),
    bc(
        0xFC,
        "ATTEMPTED_EXECUTE_OF_NOEXECUTE_MEMORY",
        Driver,
        "Code tried to run from memory meant only for data, which is often a driver bug or harmful software.",
    ),
    bc(
        0x101,
        "CLOCK_WATCHDOG_TIMEOUT",
        Hardware,
        "A processor core stopped responding, which is often a processor, overclocking or firmware matter.",
    ),
    bc(
        0x109,
        "CRITICAL_STRUCTURE_CORRUPTION",
        Driver,
        "Protected parts of Windows were changed, which is often a bad driver or harmful software and occasionally faulty memory.",
    ),
    bc(
        0x116,
        "VIDEO_TDR_FAILURE",
        Driver,
        "The graphics driver stopped responding and Windows could not reset it, which is a graphics driver or graphics card problem.",
    ),
    bc(
        0x117,
        "VIDEO_TDR_TIMEOUT_DETECTED",
        Driver,
        "The graphics driver stopped responding and could not recover, which is a graphics driver or graphics card problem.",
    ),
    bc(
        0x119,
        "VIDEO_SCHEDULER_INTERNAL_ERROR",
        Driver,
        "The graphics system found a serious internal error, which is usually the graphics driver or card.",
    ),
    bc(
        0x124,
        "WHEA_UNCORRECTABLE_ERROR",
        Hardware,
        "The hardware reported an error it could not fix, which usually means a hardware problem such as heat, overclocking or a failing part.",
    ),
    bc(
        0x133,
        "DPC_WATCHDOG_VIOLATION",
        Driver,
        "A driver kept the processor busy for too long, which is usually a driver problem, often storage, network or USB.",
    ),
    bc(
        0x139,
        "KERNEL_SECURITY_CHECK_FAILURE",
        Driver,
        "Windows found a damaged internal data structure, which is often caused by a driver and sometimes by memory.",
    ),
    bc(
        0x13A,
        "KERNEL_MODE_HEAP_CORRUPTION",
        Driver,
        "Windows found corrupted memory in its heap, which is usually a driver bug.",
    ),
    bc(
        0x154,
        "UNEXPECTED_STORE_EXCEPTION",
        Hardware,
        "Windows' memory compression hit an unexpected error, which often points to a failing drive.",
    ),
    bc(
        0x1C8,
        "MANUALLY_INITIATED_POWER_BUTTON_HOLD",
        Software,
        "The power button was held down to force a crash.",
    ),
    bc(
        0x1CA,
        "SYNTHETIC_WATCHDOG_TIMEOUT",
        Unknown,
        "Windows stopped making progress and a system-wide watchdog gave up waiting, which can be a driver or hardware.",
    ),
    bc(
        0x1CF,
        "HARDWARE_WATCHDOG_TIMEOUT",
        Hardware,
        "A hardware watchdog timer ran out because the computer stopped responding, which is often a hardware or firmware matter.",
    ),
    bc(
        0xC000021A,
        "WINLOGON_FATAL_ERROR",
        Software,
        "A core sign-in process of Windows failed, which is often a damaged system file or a bad update.",
    ),
    bc(
        0xDEADDEAD,
        "MANUALLY_INITIATED_CRASH1",
        Software,
        "Someone deliberately triggered this crash with a tool.",
    ),
];

pub fn lookup(code: u32) -> Option<&'static BugCheck> {
    TABLE.iter().find(|b| b.code == code)
}

/// `0x00000139` style text.
pub fn hex(code: u32) -> String {
    format!("0x{code:08X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_common_codes() {
        for (code, name) in [
            (0x139, "KERNEL_SECURITY_CHECK_FAILURE"),
            (0x124, "WHEA_UNCORRECTABLE_ERROR"),
            (0x116, "VIDEO_TDR_FAILURE"),
            (0xD1, "DRIVER_IRQL_NOT_LESS_OR_EQUAL"),
            (0x9F, "DRIVER_POWER_STATE_FAILURE"),
            (0x133, "DPC_WATCHDOG_VIOLATION"),
        ] {
            assert_eq!(lookup(code).map(|b| b.name), Some(name), "{code:#x}");
        }
        assert_eq!(lookup(0x124).unwrap().cause, Cause::Hardware);
        assert_eq!(lookup(0x133).unwrap().cause, Cause::Driver);
    }

    #[test]
    fn unknown_code_is_none() {
        assert!(lookup(0x12345678).is_none());
        assert!(lookup(0x141).is_none());
    }

    #[test]
    fn table_is_sane() {
        assert!(TABLE.len() >= 45);
        for (i, b) in TABLE.iter().enumerate() {
            assert!(!b.sentence.is_empty() && !b.name.is_empty());
            assert!(!b.sentence.to_lowercase().contains("always"), "{}", b.name);
            assert!(
                TABLE[i + 1..].iter().all(|o| o.code != b.code),
                "duplicate {:#x}",
                b.code
            );
        }
    }

    #[test]
    fn hex_format() {
        assert_eq!(hex(0x139), "0x00000139");
        assert_eq!(hex(0xC000021A), "0xC000021A");
    }
}
