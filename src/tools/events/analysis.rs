//! Pure analysis of events: grouping, crash detection, digest. No Windows calls.
#![allow(dead_code)]

use std::collections::HashMap;

use super::catalog::{self, provider_key};
use super::model::{Category, Crash, CrashKind, DigestLine, Event, Group, Severity};

pub const DEFAULT_WINDOW_MS: i64 = 10 * 60 * 1000;

const SECOND_MS: i64 = 1000;
const MINUTE_MS: i64 = 60 * SECOND_MS;

/// How far after a Kernel-Power 41 a WER 1001 may be logged and still belong to it (tolerates
/// 1001 landing a few seconds before the 41 too).
const PAIR_AFTER_MS: i64 = 15 * MINUTE_MS;
const PAIR_BEFORE_MS: i64 = MINUTE_MS;
/// Kernel-Power 41 events closer together than this are one crash.
const DEDUPE_MS: i64 = 2 * MINUTE_MS;
/// Records closer together than this belong to one burst. The boot burst after a crash is the
/// chain of records ending at the 41; the first record before a longer gap is the best guess for
/// "last thing the old session logged".
const BURST_GAP_MS: i64 = 30 * SECOND_MS;

// Digest thresholds. PROPOSALS, documented in docs/PLAN-event-watch.md; adjust after real use.
/// A disk group with at least this many events in the period is reported as needing attention.
const DISK_RETRY_MIN: usize = 5;
/// Corrected WHEA hardware errors (17/19) in the period before they are worth mentioning.
const WHEA_CORRECTED_MIN: usize = 3;
/// Recovered graphics driver resets (Display 4101) in the period before they are worth mentioning.
const DISPLAY_RECOVERY_MIN: usize = 3;

type KindRow<'a> = (&'a dyn Fn(&CrashKind) -> bool, Severity, &'a str, &'a str);

fn is_disk_provider(key: &str) -> bool {
    matches!(key, "disk" | "storahci" | "stornvme" | "storport" | "ntfs")
}

/// Subject of an event for grouping: the first present of DeviceName, Device, DiskNumber, and
/// (only for disk-like providers, where the device is often the first unnamed value) Data1.
/// Blank values are skipped. Other providers get no subject so unrelated events fold together.
fn subject_of(event: &Event, key: &str) -> Option<String> {
    let mut names = vec!["DeviceName", "Device", "DiskNumber"];
    if is_disk_provider(key) {
        names.push("Data1");
    }
    names
        .into_iter()
        .filter_map(|n| event.value(n))
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(str::to_string)
}

/// Folds repeated events (same provider + id + subject such as "Disk 0") into groups, newest group
/// first. `subject` comes from event data (e.g. DeviceName/Disk number) when present.
pub fn group(events: &[Event]) -> Vec<Group> {
    let mut map: HashMap<(String, u32, Option<String>), Group> = HashMap::new();
    for e in events {
        let key = provider_key(&e.provider);
        let subject = subject_of(e, &key);
        match map.get_mut(&(key.clone(), e.id, subject.clone())) {
            Some(g) => {
                g.count += 1;
                g.first_ms = g.first_ms.min(e.time_ms);
                if e.time_ms >= g.last_ms {
                    g.last_ms = e.time_ms;
                    g.sample = e.clone();
                }
            }
            None => {
                map.insert(
                    (key, e.id, subject.clone()),
                    Group {
                        provider: e.provider.clone(),
                        id: e.id,
                        subject,
                        count: 1,
                        first_ms: e.time_ms,
                        last_ms: e.time_ms,
                        sample: e.clone(),
                        info: catalog::classify(e),
                    },
                );
            }
        }
    }
    let mut groups: Vec<Group> = map.into_values().collect();
    for g in &mut groups {
        g.info = catalog::classify(&g.sample);
    }
    groups.sort_by(|a, b| {
        b.last_ms
            .cmp(&a.last_ms)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.subject.cmp(&b.subject))
    });
    groups
}

/// `0x...` hex or plain decimal.
fn parse_num(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

fn is_crash_marker(e: &Event) -> bool {
    let k = provider_key(&e.provider);
    matches!(
        (k.as_str(), e.id),
        ("kernel-power", 41) | ("eventlog", 6008) | ("wer-systemerrorreporting", 1001)
    )
}

fn kind_from_41(e: &Event) -> CrashKind {
    let code = e.value("BugcheckCode").map(parse_num);
    let button = e.value("PowerButtonTimestamp").map(parse_num);
    let nonzero = code
        .flatten()
        .and_then(|c| u32::try_from(c).ok())
        .filter(|&c| c != 0);
    if let Some(c) = nonzero {
        let mut params = [0u64; 4];
        for (i, p) in params.iter_mut().enumerate() {
            *p = e
                .value(&format!("BugcheckParameter{}", i + 1))
                .and_then(parse_num)
                .unwrap_or(0);
        }
        return CrashKind::BlueScreen { code: c, params };
    }
    match (code, button) {
        (_, Some(Some(b))) if b != 0 => CrashKind::ForcedRestart,
        (Some(Some(_)), Some(Some(_))) | (Some(Some(_)), None) => CrashKind::PowerLossOrHang,
        _ => CrashKind::Unknown,
    }
}

/// Parses `0x00000139 (0x..., 0x..., 0x..., 0x...)` from the first value of a WER 1001 event.
fn parse_1001_code(text: &str) -> Option<(u32, [u64; 4])> {
    let text = text.trim();
    let end = text
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(text.len());
    let code = u32::try_from(parse_num(&text[..end])?).ok()?;
    let mut params = [0u64; 4];
    if let (Some(a), Some(b)) = (text.find('('), text.find(')')) {
        if a < b {
            for (slot, part) in params.iter_mut().zip(text[a + 1..b].split(',')) {
                *slot = parse_num(part).unwrap_or(0);
            }
        }
    }
    Some((code, params))
}

/// Finds restarts that were not clean (Kernel-Power 41, with WER 1001 pairing) and attaches the
/// warnings/errors in the `window_ms` before each one. Newest crash first.
/// `events` may be unsorted and may include several channels.
///
/// `stopped_ms` rule (EventLog 6008 carries only localised text times, so it is not parsed): from
/// the 41, walk back while the previous record is at most 30 s earlier (that chain is the boot
/// burst of the new session); `stopped_ms` is the time of the first record before that gap, or
/// `noticed_ms` when there is none.
pub fn detect_crashes(events: &[Event], window_ms: i64) -> Vec<Crash> {
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by_key(|e| (e.time_ms, e.record_id));

    let mut crashes: Vec<Crash> = Vec::new();
    let mut last_kept: Option<i64> = None;
    for (idx, e) in sorted.iter().enumerate() {
        if !(provider_key(&e.provider) == "kernel-power" && e.id == 41) {
            continue;
        }
        if last_kept.is_some_and(|t| e.time_ms - t < DEDUPE_MS) {
            continue;
        }
        last_kept = Some(e.time_ms);
        let noticed_ms = e.time_ms;
        let mut kind = kind_from_41(e);

        // Nearest WER 1001.
        let wer = sorted
            .iter()
            .filter(|w| {
                provider_key(&w.provider) == "wer-systemerrorreporting"
                    && w.id == 1001
                    && w.time_ms >= noticed_ms - PAIR_BEFORE_MS
                    && w.time_ms <= noticed_ms + PAIR_AFTER_MS
            })
            .min_by_key(|w| (w.time_ms - noticed_ms).abs());
        let mut dump_path = None;
        if let Some(w) = wer {
            dump_path = w
                .data
                .iter()
                .map(|(_, v)| v)
                .find(|v| v.to_ascii_lowercase().contains(".dmp"))
                .cloned();
            if !matches!(kind, CrashKind::BlueScreen { .. }) {
                let parsed = w.data.first().and_then(|(_, v)| parse_1001_code(v));
                if let Some((code, params)) = parsed.filter(|(c, _)| *c != 0) {
                    kind = CrashKind::BlueScreen { code, params };
                }
            }
        }

        let mut i = idx;
        while i > 0 && sorted[i].time_ms - sorted[i - 1].time_ms <= BURST_GAP_MS {
            i -= 1;
        }
        let stopped_ms = if i > 0 {
            sorted[i - 1].time_ms
        } else {
            noticed_ms
        };

        let before: Vec<Event> = sorted
            .iter()
            .filter(|b| {
                (1..=3).contains(&b.level)
                    && b.time_ms >= stopped_ms - window_ms
                    && b.time_ms <= stopped_ms
                    && !is_crash_marker(b)
            })
            .map(|b| (*b).clone())
            .collect();

        crashes.push(Crash {
            noticed_ms,
            stopped_ms,
            kind,
            dump_path,
            before,
        });
    }
    crashes.reverse();
    crashes
}

/// `YYYY-MM-DD HH:MM` in UTC from milliseconds since the Unix epoch (civil-from-days, no chrono).
pub fn ms_to_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// Plain-language summary lines for the Summary page (worst first), built from the groups and the
/// crashes of the period. Tier thresholds are proposals (see the consts at the top).
pub fn digest(groups: &[Group], crashes: &[Crash]) -> Vec<DigestLine> {
    let mut lines: Vec<DigestLine> = Vec::new();

    let latest = |f: &dyn Fn(&CrashKind) -> bool| -> (usize, i64) {
        crashes
            .iter()
            .filter(|c| f(&c.kind))
            .fold((0, i64::MIN), |(n, t), c| (n + 1, t.max(c.noticed_ms)))
    };
    let kinds: [KindRow; 4] = [
        (
            &|k: &CrashKind| matches!(k, CrashKind::BlueScreen { .. }),
            Severity::NeedsAttention,
            "bluescreen",
            "bluescreens",
        ),
        (
            &|k: &CrashKind| matches!(k, CrashKind::ForcedRestart),
            Severity::WorthWatching,
            "restart forced with the power button",
            "restarts forced with the power button",
        ),
        (
            &|k: &CrashKind| matches!(k, CrashKind::PowerLossOrHang),
            Severity::WorthWatching,
            "restart after a power loss or freeze",
            "restarts after a power loss or freeze",
        ),
        (
            &|k: &CrashKind| matches!(k, CrashKind::Unknown),
            Severity::WorthWatching,
            "restart without a clean shutdown and an unclear cause",
            "restarts without a clean shutdown and an unclear cause",
        ),
    ];
    for (f, severity, one, many) in kinds {
        let (n, t) = latest(f);
        if n > 0 {
            lines.push(DigestLine {
                severity,
                text: format!(
                    "{n} {} in this period, the latest on {}.",
                    plural(n, one, many),
                    ms_to_text(t)
                ),
            });
        }
    }

    for g in groups {
        if g.info.category == Category::Disk && g.count >= DISK_RETRY_MIN {
            let what = g
                .subject
                .as_deref()
                .map_or("Disk".to_string(), |s| format!("Disk {s}"));
            lines.push(DigestLine {
                severity: Severity::NeedsAttention,
                text: format!("{what} had {} retried or reset requests.", g.count),
            });
        }
    }

    let sum = |provider: &str, ids: &[u32]| -> usize {
        groups
            .iter()
            .filter(|g| provider_key(&g.provider) == provider && ids.contains(&g.id))
            .map(|g| g.count)
            .sum()
    };
    let fatal = sum("whea-logger", &[18]);
    if fatal > 0 {
        lines.push(DigestLine {
            severity: Severity::NeedsAttention,
            text: format!(
                "A fatal hardware error was reported {fatal} {}.",
                plural(fatal, "time", "times")
            ),
        });
    }
    let corrected = sum("whea-logger", &[17, 19]);
    if corrected >= WHEA_CORRECTED_MIN {
        lines.push(DigestLine {
            severity: Severity::WorthWatching,
            text: format!(
                "{corrected} corrected hardware errors were logged. A few are normal; a steady stream is worth a look."
            ),
        });
    }
    let display = sum("display", &[4101]);
    if display >= DISPLAY_RECOVERY_MIN {
        lines.push(DigestLine {
            severity: Severity::WorthWatching,
            text: format!("Your graphics driver stopped responding and recovered {display} times."),
        });
    }

    if crashes.is_empty() {
        lines.push(DigestLine {
            severity: Severity::Info,
            text: "No crashes found in the log.".to_string(),
        });
    }

    lines.sort_by_key(|l| l.severity);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const T0: i64 = 1_800_000_000_000;

    fn ev(provider: &str, id: u32, level: u8, t: i64, data: &[(&str, &str)]) -> Event {
        Event {
            record_id: (t / 1000) as u64 + u64::from(id),
            time_ms: t,
            channel: "System".into(),
            provider: provider.into(),
            id,
            level,
            data: data
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            message: None,
        }
    }

    fn kp41(t: i64, data: &[(&str, &str)]) -> Event {
        ev("Microsoft-Windows-Kernel-Power", 41, 1, t, data)
    }

    #[test]
    fn ms_to_text_values() {
        assert_eq!(ms_to_text(0), "1970-01-01 00:00");
        assert_eq!(ms_to_text(1_793_310_300_000), "2026-10-29 21:45");
        assert_eq!(
            ms_to_text(951_782_400_000 + 3_600_000 + 60_000),
            "2000-02-29 01:01"
        );
        assert_eq!(ms_to_text(-1), "1969-12-31 23:59");
    }

    #[test]
    fn groups_many_disk_events_into_one() {
        let mut events: Vec<Event> = (0..143)
            .map(|i| {
                ev(
                    "disk",
                    153,
                    3,
                    T0 + i * 1000,
                    &[("DeviceName", "\\Device\\Harddisk0\\DR0")],
                )
            })
            .collect();
        events.push(ev(
            "disk",
            153,
            3,
            T0,
            &[("DeviceName", "\\Device\\Harddisk1\\DR1")],
        ));
        events.push(ev("Display", 4101, 3, T0 + 500_000, &[]));
        let g = group(&events);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].id, 4101);
        let disk0 = g.iter().find(|x| x.count == 143).unwrap();
        assert_eq!(disk0.first_ms, T0);
        assert_eq!(disk0.last_ms, T0 + 142_000);
        assert_eq!(disk0.sample.time_ms, T0 + 142_000);
        assert!(disk0.info.known);
        assert_eq!(disk0.subject.as_deref(), Some("\\Device\\Harddisk0\\DR0"));
    }

    #[test]
    fn subject_data1_only_for_disk_providers() {
        let d = ev("Disk", 11, 2, T0, &[("Data1", "\\Device\\Harddisk2\\DR2")]);
        assert_eq!(
            group(&[d])[0].subject.as_deref(),
            Some("\\Device\\Harddisk2\\DR2")
        );
        let o = ev("Other", 1, 2, T0, &[("Data1", "x")]);
        assert_eq!(group(&[o])[0].subject, None);
    }

    #[test]
    fn group_input_order_does_not_matter() {
        let a = ev("A", 1, 2, T0 + 10, &[]);
        let b = ev("B", 1, 2, T0 + 20, &[]);
        let c = ev("A", 1, 2, T0 + 30, &[]);
        let g1 = group(&[a.clone(), b.clone(), c.clone()]);
        let g2 = group(&[c, b, a]);
        assert_eq!(g1, g2);
        assert_eq!(g1[0].provider, "A");
        assert_eq!(g1[0].count, 2);
    }

    #[test]
    fn bluescreen_with_dump_and_window() {
        let boot = T0 + 100 * MIN;
        let events = vec![
            // Unsorted on purpose.
            kp41(
                boot + 20_000,
                &[
                    ("BugcheckCode", "313"),
                    ("BugcheckParameter1", "0x3"),
                    ("BugcheckParameter2", "0xfffff8017ffbe2c0"),
                ],
            ),
            ev("Application Error", 1000, 2, T0 + 95 * MIN, &[]), // inside window
            ev("Disk", 153, 3, T0 + 85 * MIN, &[]),               // before window
            ev("Foo", 1, 4, T0 + 99 * MIN, &[]),                  // info level, skipped
            ev("Boot", 1, 2, boot + 5_000, &[]),                  // boot burst
            ev("Boot", 2, 4, boot + 10_000, &[]),
            ev(
                "Microsoft-Windows-WER-SystemErrorReporting",
                1001,
                2,
                boot + 25_000,
                &[
                    ("param1", "0x00000139 (0x1, 0x2, 0x3, 0x4)"),
                    ("param2", "C:\\WINDOWS\\Minidump\\101426-7109-01.dmp"),
                ],
            ),
            ev("EventLog", 6008, 2, boot + 12_000, &[]),
            ev("Service Control Manager", 7034, 2, T0 + 99 * MIN, &[]),
        ];
        let crashes = detect_crashes(&events, 10 * MIN);
        assert_eq!(crashes.len(), 1);
        let c = &crashes[0];
        assert_eq!(c.noticed_ms, boot + 20_000);
        assert_eq!(c.stopped_ms, T0 + 99 * MIN);
        assert_eq!(
            c.kind,
            CrashKind::BlueScreen {
                code: 313,
                params: [3, 0xfffff8017ffbe2c0, 0, 0]
            }
        );
        assert_eq!(
            c.dump_path.as_deref(),
            Some("C:\\WINDOWS\\Minidump\\101426-7109-01.dmp")
        );
        let ids: Vec<u32> = c.before.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1000, 7034]);
    }

    #[test]
    fn hex_bugcheck_code_and_1001_fallback() {
        let t = T0 + 50 * MIN;
        let a = kp41(t, &[("BugcheckCode", "0x139")]);
        assert!(matches!(
            detect_crashes(&[a], MIN)[0].kind,
            CrashKind::BlueScreen { code: 0x139, .. }
        ));
        let z = kp41(t, &[("BugcheckCode", "0")]);
        let w = ev(
            "Microsoft-Windows-WER-SystemErrorReporting",
            1001,
            2,
            t + 3 * MIN,
            &[("x", "0x000000D1 (0x1, 0x2, 0x0, 0x4)")],
        );
        let c = &detect_crashes(&[z, w], MIN)[0];
        assert_eq!(
            c.kind,
            CrashKind::BlueScreen {
                code: 0xD1,
                params: [1, 2, 0, 4]
            }
        );
        assert_eq!(c.dump_path, None);
    }

    #[test]
    fn forced_restart_power_loss_unknown() {
        let t = T0;
        let forced = kp41(
            t,
            &[
                ("BugcheckCode", "0"),
                ("PowerButtonTimestamp", "133800000000000000"),
            ],
        );
        let loss = kp41(
            t + 60 * MIN,
            &[("BugcheckCode", "0"), ("PowerButtonTimestamp", "0")],
        );
        let unknown = kp41(t + 120 * MIN, &[("BugcheckCode", "junk")]);
        let c = detect_crashes(&[forced, loss, unknown], MIN);
        assert_eq!(c.len(), 3);
        // Newest first.
        assert_eq!(c[0].kind, CrashKind::Unknown);
        assert_eq!(c[1].kind, CrashKind::PowerLossOrHang);
        assert_eq!(c[2].kind, CrashKind::ForcedRestart);
        // No earlier records: stopped falls back to noticed.
        assert_eq!(c[2].stopped_ms, t);
    }

    #[test]
    fn dedupes_close_crashes_and_stops_before_boot_burst() {
        let t = T0;
        let events = vec![
            ev("Last", 1, 2, t - 10 * MIN, &[]),
            kp41(t, &[("BugcheckCode", "0")]),
            kp41(t + MIN, &[("BugcheckCode", "0")]),
            kp41(t + 5 * MIN, &[("BugcheckCode", "0")]),
        ];
        let c = detect_crashes(&events, MIN);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].stopped_ms, t - 10 * MIN);
        assert_eq!(c[1].before.len(), 1);
    }

    #[test]
    fn digest_thresholds() {
        let mk = |provider: &str, id: u32, n: i64, subj: Option<&str>| -> Vec<Event> {
            (0..n)
                .map(|i| {
                    let d: Vec<(&str, &str)> =
                        subj.map(|s| vec![("DeviceName", s)]).unwrap_or_default();
                    ev(provider, id, 3, T0 + i * 1000, &d)
                })
                .collect()
        };
        let mut events = mk("disk", 153, 5, Some("0"));
        events.extend(mk("disk", 11, 4, Some("1")));
        events.extend(mk("Microsoft-Windows-WHEA-Logger", 17, 3, None));
        events.extend(mk("Display", 4101, 2, None));
        let crash = detect_crashes(&[kp41(T0 + 10 * MIN, &[("BugcheckCode", "313")])], MIN);
        let lines = digest(&group(&events), &crash);
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert!(texts[0].starts_with("1 bluescreen in this period, the latest on "));
        assert_eq!(lines[0].severity, Severity::NeedsAttention);
        assert!(texts.contains(&"Disk 0 had 5 retried or reset requests."));
        assert!(!texts.iter().any(|t| t.starts_with("Disk 1")));
        assert!(texts
            .iter()
            .any(|t| t.starts_with("3 corrected hardware errors")));
        assert!(!texts.iter().any(|t| t.contains("graphics")));
        assert!(!texts.iter().any(|t| t.contains("No crashes")));
        assert!(lines.windows(2).all(|w| w[0].severity <= w[1].severity));
    }

    #[test]
    fn digest_no_crashes_and_display() {
        let events: Vec<Event> = (0..3)
            .map(|i| ev("Display", 4101, 3, T0 + i, &[]))
            .collect();
        let lines = digest(&group(&events), &[]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].severity, Severity::WorthWatching);
        assert_eq!(lines[1].text, "No crashes found in the log.");
        assert_eq!(lines[1].severity, Severity::Info);
    }
}
