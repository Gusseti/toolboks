//! Memory pressure monitor: turns raw memory signals into a plain verdict (Healthy, Busy,
//! Under pressure) plus the reasons behind it. The model is pure: time is passed in as
//! `now_ms`, so tests never sleep. The Windows readers sit behind `SignalSource`.
//!
//! Rule (see docs/RESEARCH-memory-cleaning.md): Under pressure needs at least two signals
//! that have each held continuously for `HOLD_MS`. Thresholds are a synthesis, not measured
//! facts, so they all live in `limits` and are easy to change.

use std::time::Instant;

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    CreateMemoryResourceNotification, LowMemoryResourceNotification,
    QueryMemoryResourceNotification,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// PDH path of the Pages Input/sec counter.
pub(super) const PAGES_INPUT_PATH: &str = "\\Memory\\Pages Input/sec";
/// PDH path of the Page Faults/sec counter (all page faults, soft and hard).
pub(super) const PAGE_FAULTS_PATH: &str = "\\Memory\\Page Faults/sec";

/// Every tunable number in one place.
pub mod limits {
    pub const GIB: u64 = 1024 * 1024 * 1024;
    pub const MIB: u64 = 1024 * 1024;
    /// Available memory is low below max(this share of RAM, `AVAILABLE_FLOOR_BYTES`).
    pub const AVAILABLE_PERCENT: u64 = 10;
    pub const AVAILABLE_FLOOR_BYTES: u64 = GIB + GIB / 2;
    /// Commit at or above this share of the limit marks the PC as busy.
    pub const COMMIT_BUSY_PERCENT: u64 = 85;
    /// Commit at or above this share of the limit counts as a pressure signal.
    pub const COMMIT_PRESSURE_PERCENT: u64 = 90;
    /// Pages Input/sec (pages read from disk to resolve hard faults) at or above this is high.
    pub const HARD_FAULTS_PER_SEC: f64 = 300.0;
    /// Compressed memory must grow by at least this much within the window.
    pub const COMPRESSED_GROWTH_BYTES: u64 = 256 * MIB;
    /// Length of the compressed-growth window.
    pub const COMPRESSED_WINDOW_MS: u64 = 5 * 60_000;
    /// The window must span at least this long before growth is judged.
    pub const COMPRESSED_MIN_SPAN_MS: u64 = 60_000;
    /// A signal must stay active this long to count towards Under pressure.
    pub const HOLD_MS: u64 = 60_000;
    /// Signals needed (each held for `HOLD_MS`) to enter Under pressure.
    pub const SIGNALS_NEEDED: usize = 2;
    /// The low-memory handle is read at most this often (it stays signalled, so never spin).
    pub const LOW_MEMORY_COOLDOWN_MS: u64 = 1_000;
}

/// One snapshot of the system. `None` means the source could not provide the value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Signals {
    pub available: u64,
    pub total_ram: u64,
    pub commit_used: u64,
    pub commit_limit: u64,
    pub compressed: Option<u64>,
    pub hard_faults_per_sec: Option<f64>,
    pub low_memory_signalled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Healthy,
    Busy,
    UnderPressure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub verdict: Verdict,
    /// Plain-language reasons for the UI; empty when healthy.
    pub reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sig {
    LowAvailable,
    CommitHigh,
    LowMemoryNotice,
    CompressedGrowing,
    HardFaults,
}

const ALL_SIGS: [Sig; 5] = [
    Sig::LowAvailable,
    Sig::CommitHigh,
    Sig::LowMemoryNotice,
    Sig::CompressedGrowing,
    Sig::HardFaults,
];

impl Sig {
    fn index(self) -> usize {
        self as usize
    }
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / limits::GIB as f64)
}

/// The Available threshold for a machine with `total_ram` bytes.
pub fn available_threshold(total_ram: u64) -> u64 {
    (total_ram / 100 * limits::AVAILABLE_PERCENT).max(limits::AVAILABLE_FLOOR_BYTES)
}

fn commit_percent(s: &Signals) -> u64 {
    if s.commit_limit == 0 {
        0
    } else {
        (s.commit_used as u128 * 100 / s.commit_limit as u128) as u64
    }
}

/// Stateful evaluator: tracks how long each signal has held, the compressed-size history
/// and the pressure latch.
#[derive(Debug, Default)]
pub struct PressureModel {
    since: [Option<u64>; 5],
    compressed: Vec<(u64, u64)>,
    latched: bool,
}

impl PressureModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates one snapshot taken at `now_ms` (any monotonic millisecond clock).
    pub fn evaluate(&mut self, now_ms: u64, s: &Signals) -> Assessment {
        let growth = self.track_compressed(now_ms, s.compressed);
        let commit_pct = commit_percent(s);
        let threshold = available_threshold(s.total_ram);

        let mut active = [false; 5];
        active[Sig::LowAvailable.index()] = s.total_ram > 0 && s.available < threshold;
        active[Sig::CommitHigh.index()] =
            s.commit_limit > 0 && commit_pct >= limits::COMMIT_PRESSURE_PERCENT;
        active[Sig::LowMemoryNotice.index()] = s.low_memory_signalled;
        active[Sig::CompressedGrowing.index()] = growth.is_some();
        active[Sig::HardFaults.index()] = s
            .hard_faults_per_sec
            .is_some_and(|f| f >= limits::HARD_FAULTS_PER_SEC);

        // A gap resets a signal's hold; a continuing signal keeps its start time.
        for sig in ALL_SIGS {
            let i = sig.index();
            self.since[i] = match (active[i], self.since[i]) {
                (true, None) => Some(now_ms),
                (true, Some(t)) => Some(t),
                (false, _) => None,
            };
        }

        let any_active = active.iter().any(|a| *a);
        let held = ALL_SIGS
            .iter()
            .filter(|sig| {
                self.since[sig.index()].is_some_and(|t| now_ms.saturating_sub(t) >= limits::HOLD_MS)
            })
            .count();

        if self.latched {
            // Hysteresis: stay under pressure until every signal has cleared.
            if !any_active {
                self.latched = false;
            }
        } else if held >= limits::SIGNALS_NEEDED {
            self.latched = true;
        }

        let commit_busy = s.commit_limit > 0 && commit_pct >= limits::COMMIT_BUSY_PERCENT;
        let verdict = if self.latched {
            Verdict::UnderPressure
        } else if any_active || commit_busy {
            Verdict::Busy
        } else {
            Verdict::Healthy
        };

        let mut reasons = Vec::new();
        if active[Sig::LowAvailable.index()] {
            reasons.push(format!("Only {} is ready to use", gb(s.available)));
        }
        if active[Sig::CommitHigh.index()] || commit_busy {
            reasons.push(format!(
                "Programs have reserved {commit_pct}% of the memory Windows can promise"
            ));
        }
        if active[Sig::LowMemoryNotice.index()] {
            reasons.push("Windows reports that memory is low".to_string());
        }
        if let Some(bytes) = growth {
            reasons.push(format!(
                "Compressed memory grew by {} in the last few minutes",
                gb(bytes)
            ));
        }
        if active[Sig::HardFaults.index()] {
            reasons.push("Windows is reading memory back from disk a lot".to_string());
        }
        Assessment { verdict, reasons }
    }

    /// Records a compressed-size sample; returns the growth if it passes the threshold.
    fn track_compressed(&mut self, now_ms: u64, sample: Option<u64>) -> Option<u64> {
        let bytes = sample?;
        self.compressed.push((now_ms, bytes));
        let cutoff = now_ms.saturating_sub(limits::COMPRESSED_WINDOW_MS);
        self.compressed.retain(|(t, _)| *t >= cutoff);
        let (oldest_t, oldest) = *self.compressed.first()?;
        if now_ms - oldest_t < limits::COMPRESSED_MIN_SPAN_MS {
            return None;
        }
        let growth = bytes.saturating_sub(oldest);
        (growth >= limits::COMPRESSED_GROWTH_BYTES).then_some(growth)
    }
}

/// Anything that can produce a `Signals` snapshot; fakes implement it in tests.
pub trait SignalSource {
    /// `None` if the basics (RAM and commit) cannot be read.
    fn read(&mut self) -> Option<Signals>;
}

/// Watches the system low-memory notification. The handle stays signalled while memory is
/// low, so it is polled (never waited on in a loop) and the answer is cached for
/// `LOW_MEMORY_COOLDOWN_MS`.
pub struct LowMemoryWatcher {
    handle: HANDLE,
    last_poll: Option<Instant>,
    last: bool,
}

impl LowMemoryWatcher {
    /// `None` if Windows refuses to create the notification.
    pub fn new() -> Option<Self> {
        // SAFETY: plain call with a valid enum; returns an owned handle closed in `Drop`.
        let handle =
            unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }.ok()?;
        Some(Self {
            handle,
            last_poll: None,
            last: false,
        })
    }

    /// True while Windows says memory is low. Cheap: re-queries at most once per cooldown.
    pub fn is_signalled(&mut self) -> bool {
        let due = self
            .last_poll
            .is_none_or(|t| t.elapsed().as_millis() as u64 >= limits::LOW_MEMORY_COOLDOWN_MS);
        if due {
            let mut state = BOOL(0);
            // SAFETY: `self.handle` is a valid notification handle and `state` a valid out pointer.
            let ok = unsafe { QueryMemoryResourceNotification(self.handle, &mut state) }.is_ok();
            self.last = ok && state.as_bool();
            self.last_poll = Some(Instant::now());
        }
        self.last
    }
}

impl Drop for LowMemoryWatcher {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by `self` and closed exactly once.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// A per-second rate counter from the English PDH namespace; if it cannot be added the value
/// is simply `None`. Used for Pages Input/sec (hard faults) here and in the run sampler.
pub(super) struct RateCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl RateCounter {
    pub(super) fn new(counter_path: &str) -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: valid out pointer; a null data source means live data.
        if unsafe { PdhOpenQueryW(None, 0, &mut query) } != 0 {
            return None;
        }
        let path: Vec<u16> = counter_path.encode_utf16().chain(Some(0)).collect();
        let mut counter = PDH_HCOUNTER::default();
        // SAFETY: `query` is open, `path` is NUL-terminated and outlives the call.
        let status =
            unsafe { PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) };
        if status != 0 {
            // SAFETY: `query` is a valid handle nothing else owns.
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        // Rate counters need a baseline sample before the first real read.
        // SAFETY: `query` is open.
        unsafe { PdhCollectQueryData(query) };
        Some(Self { query, counter })
    }

    pub(super) fn read(&mut self) -> Option<f64> {
        // SAFETY: `self.query` is open for the life of `self`.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }
        let mut value = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: `self.counter` belongs to the open query; `value` is a valid out pointer.
        let status =
            unsafe { PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut value) };
        if status != 0 || value.CStatus != 0 {
            return None;
        }
        // SAFETY: PDH_FMT_DOUBLE was requested, so `doubleValue` is the active union field.
        Some(unsafe { value.Anonymous.doubleValue })
    }
}

impl Drop for RateCounter {
    fn drop(&mut self) {
        // SAFETY: the query is owned by `self` and closed exactly once.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// The real Windows source. Compressed size has no simple documented API, so it is always
/// `None` here and the compressed-growth signal stays off until a reliable source exists.
/// Hard faults come from PDH and are `None` on the first read or if PDH is unavailable.
pub struct WindowsSignals {
    watcher: Option<LowMemoryWatcher>,
    hard_faults: Option<RateCounter>,
}

impl WindowsSignals {
    pub fn new() -> Self {
        Self {
            watcher: LowMemoryWatcher::new(),
            hard_faults: RateCounter::new(PAGES_INPUT_PATH),
        }
    }
}

impl Default for WindowsSignals {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalSource for WindowsSignals {
    fn read(&mut self) -> Option<Signals> {
        let mut mem = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `mem` is a valid out parameter with `dwLength` set.
        unsafe { GlobalMemoryStatusEx(&mut mem) }.ok()?;

        let mut info = PERFORMANCE_INFORMATION::default();
        let size = size_of::<PERFORMANCE_INFORMATION>() as u32;
        info.cb = size;
        // SAFETY: `info` is a valid, correctly sized out parameter.
        unsafe { GetPerformanceInfo(&mut info, size) }.ok()?;
        let page = info.PageSize as u64;

        Some(Signals {
            available: mem.ullAvailPhys,
            total_ram: mem.ullTotalPhys,
            commit_used: info.CommitTotal as u64 * page,
            commit_limit: info.CommitLimit as u64 * page,
            compressed: None,
            hard_faults_per_sec: self.hard_faults.as_mut().and_then(|c| c.read()),
            low_memory_signalled: self.watcher.as_mut().is_some_and(|w| w.is_signalled()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::limits::*;
    use super::*;

    const RAM: u64 = 16 * GIB;

    fn calm() -> Signals {
        Signals {
            available: 8 * GIB,
            total_ram: RAM,
            commit_used: 10 * GIB,
            commit_limit: 40 * GIB,
            compressed: Some(GIB),
            hard_faults_per_sec: Some(5.0),
            low_memory_signalled: false,
        }
    }

    fn low_avail() -> Signals {
        Signals {
            available: GIB,
            ..calm()
        }
    }

    fn commit_pct(p: u64) -> Signals {
        Signals {
            commit_used: 100 * p,
            commit_limit: 10_000,
            ..calm()
        }
    }

    /// Feeds `s` once a second from `from` to `to` ms inclusive; returns the last assessment.
    fn feed(m: &mut PressureModel, from: u64, to: u64, s: &Signals) -> Assessment {
        let mut last = None;
        let mut t = from;
        while t <= to {
            last = Some(m.evaluate(t, s));
            t += 1_000;
        }
        last.unwrap()
    }

    #[test]
    fn healthy_when_calm() {
        let a = PressureModel::new().evaluate(0, &calm());
        assert_eq!(a.verdict, Verdict::Healthy);
        assert!(a.reasons.is_empty());
    }

    #[test]
    fn available_threshold_is_larger_of_percent_and_floor() {
        assert_eq!(available_threshold(8 * GIB), GIB + GIB / 2);
        assert_eq!(available_threshold(64 * GIB) / MIB, 6553);
    }

    #[test]
    fn low_available_alone_is_busy_with_reason() {
        let mut m = PressureModel::new();
        let a = feed(&mut m, 0, 120_000, &low_avail());
        assert_eq!(a.verdict, Verdict::Busy);
        assert_eq!(a.reasons, vec!["Only 1.0 GB is ready to use".to_string()]);
    }

    #[test]
    fn reason_text_one_point_four() {
        let s = Signals {
            available: (1.4 * GIB as f64) as u64,
            ..calm()
        };
        let a = PressureModel::new().evaluate(0, &s);
        assert_eq!(a.reasons[0], "Only 1.4 GB is ready to use");
    }

    #[test]
    fn commit_levels() {
        let mut m = PressureModel::new();
        assert_eq!(m.evaluate(0, &commit_pct(84)).verdict, Verdict::Healthy);
        let a = m.evaluate(1_000, &commit_pct(85));
        assert_eq!(a.verdict, Verdict::Busy);
        assert!(a.reasons[0].contains("85%"));
        // 90% counts as a pressure signal, 85% does not.
        let mut m = PressureModel::new();
        feed(&mut m, 0, 120_000, &commit_pct(86));
        let a = feed(
            &mut m,
            121_000,
            200_000,
            &Signals {
                low_memory_signalled: true,
                ..commit_pct(86)
            },
        );
        assert_eq!(a.verdict, Verdict::Busy);
    }

    #[test]
    fn commit_high_plus_low_memory_enters_pressure() {
        let s = Signals {
            low_memory_signalled: true,
            ..commit_pct(90)
        };
        let mut m = PressureModel::new();
        assert_eq!(feed(&mut m, 0, 59_000, &s).verdict, Verdict::Busy);
        assert_eq!(m.evaluate(60_000, &s).verdict, Verdict::UnderPressure);
    }

    #[test]
    fn hard_faults_signal() {
        let s = Signals {
            hard_faults_per_sec: Some(HARD_FAULTS_PER_SEC),
            ..low_avail()
        };
        let mut m = PressureModel::new();
        let a = feed(&mut m, 0, 60_000, &s);
        assert_eq!(a.verdict, Verdict::UnderPressure);
        assert!(a
            .reasons
            .contains(&"Windows is reading memory back from disk a lot".to_string()));
        // Unknown hard faults never count.
        let none = Signals {
            hard_faults_per_sec: None,
            ..calm()
        };
        assert_eq!(
            PressureModel::new().evaluate(0, &none).verdict,
            Verdict::Healthy
        );
    }

    #[test]
    fn compressed_growth_signal() {
        let mut m = PressureModel::new();
        let mut verdict = Verdict::Healthy;
        // Grows 64 MiB every 10 s while memory is also low.
        for i in 0..=18u64 {
            let s = Signals {
                compressed: Some(GIB + i * 64 * MIB),
                ..low_avail()
            };
            verdict = m.evaluate(i * 10_000, &s).verdict;
        }
        assert_eq!(verdict, Verdict::UnderPressure);
        // Flat compressed size never counts.
        let mut m = PressureModel::new();
        let a = feed(&mut m, 0, 300_000, &calm());
        assert_eq!(a.verdict, Verdict::Healthy);
    }

    #[test]
    fn compressed_growth_needs_a_minimum_span() {
        let mut m = PressureModel::new();
        m.evaluate(
            0,
            &Signals {
                compressed: Some(GIB),
                ..calm()
            },
        );
        let a = m.evaluate(
            10_000,
            &Signals {
                compressed: Some(3 * GIB),
                ..calm()
            },
        );
        assert_eq!(a.verdict, Verdict::Healthy);
    }

    #[test]
    fn hold_time_not_met_stays_busy() {
        let s = Signals {
            low_memory_signalled: true,
            ..low_avail()
        };
        let mut m = PressureModel::new();
        assert_eq!(feed(&mut m, 0, 59_000, &s).verdict, Verdict::Busy);
    }

    #[test]
    fn one_signal_never_enters_pressure() {
        let mut m = PressureModel::new();
        assert_eq!(
            feed(&mut m, 0, 600_000, &low_avail()).verdict,
            Verdict::Busy
        );
    }

    #[test]
    fn a_gap_resets_the_hold() {
        let both = Signals {
            low_memory_signalled: true,
            ..low_avail()
        };
        let mut m = PressureModel::new();
        feed(&mut m, 0, 50_000, &both);
        // Low-memory clears for one tick, so its 60 s hold restarts.
        m.evaluate(51_000, &low_avail());
        assert_eq!(feed(&mut m, 52_000, 100_000, &both).verdict, Verdict::Busy);
        assert_eq!(
            feed(&mut m, 101_000, 112_000, &both).verdict,
            Verdict::UnderPressure
        );
    }

    #[test]
    fn hysteresis_holds_until_all_signals_clear() {
        let both = Signals {
            low_memory_signalled: true,
            ..low_avail()
        };
        let mut m = PressureModel::new();
        assert_eq!(
            feed(&mut m, 0, 60_000, &both).verdict,
            Verdict::UnderPressure
        );
        // One signal clears: still under pressure.
        assert_eq!(
            feed(&mut m, 61_000, 200_000, &low_avail()).verdict,
            Verdict::UnderPressure
        );
        // All clear: back to healthy, armed again.
        assert_eq!(m.evaluate(201_000, &calm()).verdict, Verdict::Healthy);
        // Re-entering needs a fresh two-signal 60 s hold.
        assert_eq!(feed(&mut m, 202_000, 260_000, &both).verdict, Verdict::Busy);
        assert_eq!(m.evaluate(262_000, &both).verdict, Verdict::UnderPressure);
    }

    #[test]
    fn low_memory_reason_text() {
        let s = Signals {
            low_memory_signalled: true,
            ..calm()
        };
        let a = PressureModel::new().evaluate(0, &s);
        assert_eq!(a.verdict, Verdict::Busy);
        assert_eq!(
            a.reasons,
            vec!["Windows reports that memory is low".to_string()]
        );
    }

    struct Fake(Option<Signals>);
    impl SignalSource for Fake {
        fn read(&mut self) -> Option<Signals> {
            self.0
        }
    }

    #[test]
    fn works_through_the_source_trait() {
        let mut src = Fake(Some(low_avail()));
        let mut m = PressureModel::new();
        let s = src.read().unwrap();
        assert_eq!(m.evaluate(0, &s).verdict, Verdict::Busy);
        assert!(Fake(None).read().is_none());
    }

    #[test]
    fn real_source_reads_basics() {
        let s = WindowsSignals::new().read().expect("memory status");
        assert!(s.total_ram > 0 && s.commit_limit > 0);
    }
}
