//! Plain-language catalog of known events, keyed on (provider, id), never on message text (which
//! is localised). Only well-established entries are listed; everything else gets a generic entry
//! chosen by level with `known: false`.
#![allow(dead_code)]

use super::model::{Category, Event, Info, Severity};

/// Lowercase provider name without the `microsoft-windows-` prefix, so
/// `Microsoft-Windows-Kernel-Power` and `Kernel-Power` compare equal.
pub fn provider_key(provider: &str) -> String {
    let p = provider.trim().to_ascii_lowercase();
    p.strip_prefix("microsoft-windows-")
        .map(str::to_string)
        .unwrap_or(p)
}

const fn entry(
    category: Category,
    severity: Severity,
    headline: &'static str,
    hint: &'static str,
) -> Info {
    Info {
        category,
        severity,
        headline,
        hint,
        known: true,
    }
}

const DISK_HINT: &str = "A few are usually harmless; many in a short time often point to a drive, cable or controller problem.";

fn lookup(provider: &str, id: u32) -> Option<Info> {
    use Category as C;
    use Severity as S;
    let info = match (provider, id) {
        ("kernel-power", 41) => entry(
            C::Power,
            S::NeedsAttention,
            "Windows restarted without shutting down cleanly.",
            "This is a symptom. Look at the stop code or what happened just before.",
        ),
        ("eventlog", 6008) => entry(
            C::Power,
            S::WorthWatching,
            "The previous shutdown was unexpected.",
            "Windows noticed the last shutdown did not finish cleanly, for example after a crash or power loss.",
        ),
        ("wer-systemerrorreporting", 1001) => entry(
            C::System,
            S::NeedsAttention,
            "Windows recorded a bluescreen.",
            "The stop code tells you what Windows thought went wrong.",
        ),
        ("whea-logger", 17 | 19) => entry(
            C::Hardware,
            S::WorthWatching,
            "A hardware error was corrected.",
            "A few are normal; a steady stream is worth a look.",
        ),
        ("whea-logger", 18) => entry(
            C::Hardware,
            S::NeedsAttention,
            "A fatal hardware error was reported.",
            "This usually points to a hardware problem such as the processor, memory or a bus. Heat and overclocking can cause it.",
        ),
        ("disk", 7) => entry(
            C::Disk,
            S::WorthWatching,
            "A drive reported a bad block.",
            DISK_HINT,
        ),
        ("disk", 11) => entry(
            C::Disk,
            S::WorthWatching,
            "The driver found a drive error.",
            DISK_HINT,
        ),
        ("disk", 51) => entry(
            C::Disk,
            S::WorthWatching,
            "A drive reported an error while Windows was using it.",
            DISK_HINT,
        ),
        ("disk", 153) => entry(
            C::Disk,
            S::WorthWatching,
            "A drive request had to be retried.",
            DISK_HINT,
        ),
        ("storahci" | "stornvme" | "storport", 129) => entry(
            C::Disk,
            S::WorthWatching,
            "A drive did not answer and Windows reset it.",
            DISK_HINT,
        ),
        ("ntfs", 55) => entry(
            C::Filesystem,
            S::NeedsAttention,
            "Windows found damage in a drive's file system.",
            "A disk check is often worth running, and a backup of important files is a good idea.",
        ),
        ("display", 4101) => entry(
            C::Graphics,
            S::WorthWatching,
            "Your graphics driver stopped responding and recovered.",
            "Once in a while is common; repeated often can mean a driver, heat or graphics card problem.",
        ),
        ("volmgr", 46 | 161 | 162) => entry(
            C::System,
            S::WorthWatching,
            "Windows could not write a crash dump.",
            "If the computer crashes, there may be no dump file to examine.",
        ),
        ("application error", 1000) => entry(
            C::Software,
            S::WorthWatching,
            "A program crashed.",
            "One crash is usually nothing. The same program crashing repeatedly may need an update or reinstall.",
        ),
        ("application hang", 1002) => entry(
            C::Software,
            S::Info,
            "A program stopped responding.",
            "This is usually a one-off. Repeated hangs of the same program may need an update.",
        ),
        ("service control manager", 7000 | 7001 | 7023 | 7031 | 7034) => entry(
            C::Software,
            S::WorthWatching,
            "A Windows service failed.",
            "Occasional failures are common. One that repeats may be worth looking into.",
        ),
        ("resource-exhaustion-detector", 2004) => entry(
            C::Memory,
            S::WorthWatching,
            "Windows ran low on memory.",
            "Closing programs usually helps. If it repeats, a program may be using too much memory.",
        ),
        ("distributedcom", 10016) => entry(
            C::Software,
            S::Harmless,
            "A program was refused a permission it asked for.",
            "This is very common and usually harmless.",
        ),
        ("bthusb" | "bthmini" | "bthport", _) => entry(
            C::Driver,
            S::Info,
            "The Bluetooth driver logged a message.",
            "Often caused by a Bluetooth device connecting or dropping. Only worth a look if Bluetooth stops working.",
        ),
        ("system restore", 8193) => entry(
            C::System,
            S::Info,
            "Windows could not create a restore point.",
            "Restore points may be turned off or the drive is short on space. Only matters if you rely on System Restore.",
        ),
        ("windowsupdateclient", 20) => entry(
            C::Software,
            S::WorthWatching,
            "A Windows update failed to install.",
            "Windows usually retries by itself. If the same update keeps failing, run the Windows Update troubleshooter.",
        ),
        ("kernel-processor-power", 37) => entry(
            C::Power,
            S::Harmless,
            "The processor ran slower than its top speed.",
            "This is usually normal power management.",
        ),
        _ => return None,
    };
    Some(info)
}

/// Classifies one event: a catalog entry when (provider, id) is known, else a generic entry from
/// the level (`known: false`). Never reads `event.message`.
pub fn classify(event: &Event) -> Info {
    if let Some(info) = lookup(&provider_key(&event.provider), event.id) {
        return info;
    }
    let (severity, headline) = match event.level {
        1 | 2 => (Severity::WorthWatching, "Windows logged an error."),
        3 => (Severity::Info, "Windows logged a warning."),
        _ => (Severity::Info, "Windows logged an event."),
    };
    Info {
        category: Category::Other,
        severity,
        headline,
        hint: "Open the details to read Windows' own message.",
        known: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(provider: &str, id: u32, level: u8) -> Event {
        Event {
            record_id: 1,
            time_ms: 0,
            channel: "System".into(),
            provider: provider.into(),
            id,
            level,
            data: Vec::new(),
            message: None,
        }
    }

    fn check(provider: &str, id: u32, cat: Category, sev: Severity) {
        let i = classify(&ev(provider, id, 2));
        assert!(i.known, "{provider} {id}");
        assert_eq!((i.category, i.severity), (cat, sev), "{provider} {id}");
        assert!(!i.headline.is_empty());
    }

    #[test]
    fn crash_markers() {
        let i = classify(&ev("Microsoft-Windows-Kernel-Power", 41, 1));
        assert_eq!(i.severity, Severity::NeedsAttention);
        assert_eq!(
            i.headline,
            "Windows restarted without shutting down cleanly."
        );
        check("EventLog", 6008, Category::Power, Severity::WorthWatching);
        check(
            "Microsoft-Windows-WER-SystemErrorReporting",
            1001,
            Category::System,
            Severity::NeedsAttention,
        );
    }

    #[test]
    fn hardware_and_disk() {
        for id in [17, 19] {
            check(
                "Microsoft-Windows-WHEA-Logger",
                id,
                Category::Hardware,
                Severity::WorthWatching,
            );
        }
        check(
            "Microsoft-Windows-WHEA-Logger",
            18,
            Category::Hardware,
            Severity::NeedsAttention,
        );
        for id in [7, 11, 51, 153] {
            check("disk", id, Category::Disk, Severity::WorthWatching);
        }
        for p in ["storahci", "stornvme", "storport"] {
            check(p, 129, Category::Disk, Severity::WorthWatching);
        }
        check("Ntfs", 55, Category::Filesystem, Severity::NeedsAttention);
        check("Display", 4101, Category::Graphics, Severity::WorthWatching);
        for id in [46, 161, 162] {
            check("volmgr", id, Category::System, Severity::WorthWatching);
        }
    }

    #[test]
    fn software_and_harmless() {
        check(
            "Application Error",
            1000,
            Category::Software,
            Severity::WorthWatching,
        );
        check("Application Hang", 1002, Category::Software, Severity::Info);
        for id in [7000, 7001, 7023, 7031, 7034] {
            check(
                "Service Control Manager",
                id,
                Category::Software,
                Severity::WorthWatching,
            );
        }
        check(
            "Microsoft-Windows-Resource-Exhaustion-Detector",
            2004,
            Category::Memory,
            Severity::WorthWatching,
        );
        check(
            "DistributedCOM",
            10016,
            Category::Software,
            Severity::Harmless,
        );
        check(
            "Microsoft-Windows-Kernel-Processor-Power",
            37,
            Category::Power,
            Severity::Harmless,
        );
    }

    #[test]
    fn noisy_everyday_events() {
        for id in [3, 16, 99] {
            check("BTHUSB", id, Category::Driver, Severity::Info);
        }
        check("System Restore", 8193, Category::System, Severity::Info);
        check(
            "Microsoft-Windows-WindowsUpdateClient",
            20,
            Category::Software,
            Severity::WorthWatching,
        );
    }

    #[test]
    fn provider_match_is_case_insensitive() {
        assert!(classify(&ev("DISK", 153, 2)).known);
        assert!(classify(&ev("microsoft-windows-kernel-power", 41, 1)).known);
    }

    #[test]
    fn unknown_events_get_generic_entry_by_level() {
        let e = classify(&ev("Microsoft-Windows-Kernel-PnP", 219, 2));
        assert!(!e.known);
        assert_eq!(e.severity, Severity::WorthWatching);
        assert_eq!(e.headline, "Windows logged an error.");
        assert_eq!(e.hint, "Open the details to read Windows' own message.");
        assert_eq!(classify(&ev("X", 1, 1)).severity, Severity::WorthWatching);
        let w = classify(&ev("X", 1, 3));
        assert_eq!(
            (w.severity, w.headline, w.known),
            (Severity::Info, "Windows logged a warning.", false)
        );
        assert_eq!(classify(&ev("X", 1, 4)).severity, Severity::Info);
    }

    #[test]
    fn classify_ignores_message() {
        let mut a = ev("Some-Provider", 5, 3);
        let mut b = a.clone();
        a.message = Some("Kernel-Power 41 bluescreen disk error".into());
        b.message = None;
        assert_eq!(classify(&a), classify(&b));
        let mut k = ev("Microsoft-Windows-Kernel-Power", 41, 1);
        k.message = Some("harmless".into());
        assert_eq!(classify(&k).severity, Severity::NeedsAttention);
    }
}
