//! Per-app memory growth over time and plain-language hints. State is kept in memory only.
//!
//! Each refresh the caller turns the process list into one sample per app group (`collect`) and
//! hands it to `GrowthTracker::record` with a timestamp in ms. `GrowthTracker::info` then gives
//! the change per hour and a `Hint` for the Biggest apps table.
// wired in by the integration slice
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use super::stats::{group_by_name, ProcInfo};

// ---- Thresholds, all in one place ------------------------------------------------------------

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const MB: i64 = 1024 * 1024;

/// How much history is kept per group.
pub const KEEP_MS: u64 = 24 * HOUR_MS;
/// The slope is computed over this window.
pub const SLOPE_WINDOW_MS: u64 = HOUR_MS;
/// No number is reported before the group has at least this much data.
pub const MIN_SPAN_MS: u64 = 10 * MINUTE_MS;
/// A group counts as growing at or above this change per hour (bytes).
pub const GROWING_BYTES_PER_HOUR: i64 = 150 * MB;
/// ...and it must stay above the threshold for at least this long.
pub const GROWING_SUSTAIN_MS: u64 = 30 * MINUTE_MS;
/// Allowed shortfall when checking that the sustain window is covered by data.
const SUSTAIN_SLACK_MS: u64 = MINUTE_MS;
/// A browser group with at least this many processes gets the Many tabs hint.
pub const MANY_TABS_PROCESSES: usize = 15;
/// Names (lowercase, without .exe) of browsers that get the Many tabs hint.
const BROWSERS: &[&str] = &["chrome", "msedge", "firefox", "brave", "opera", "vivaldi"];

/// Samples closer together than this (per group) are ignored, which bounds memory use.
const MIN_SPACING_MS: u64 = 10_000;
/// Points older than this are thinned to one per `OLD_SPACING_MS`.
const FULL_RES_MS: u64 = HOUR_MS;
const OLD_SPACING_MS: u64 = 5 * MINUTE_MS;
/// A group not seen for this long is dropped (the app exited).
pub const GONE_AFTER_MS: u64 = 10 * MINUTE_MS;

// ---- Hint ------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    Fine,
    Growing,
    ManyTabs,
}

impl Hint {
    pub fn text(self) -> &'static str {
        match self {
            Hint::Fine => "Fine.",
            Hint::Growing => "Growing. Restart it if things feel slow.",
            Hint::ManyTabs => "Many tabs. Close the ones you are done with.",
        }
    }
}

/// What the table shows for one app group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrowthInfo {
    /// Latest memory of the group in bytes.
    pub bytes: u64,
    /// Signed change per hour in bytes; `None` until there is enough data.
    pub per_hour: Option<i64>,
    pub hint: Hint,
}

// ---- Reading private working set -------------------------------------------------------------

/// Source of the private working set of a process. `None` means "not available for this pid".
pub trait PrivateMemory {
    fn private_working_set(&self, pid: u32) -> Option<u64>;
}

/// One sample per app group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSample {
    /// Lowercase process name, the key of the group.
    pub key: String,
    /// Name as shown (the first one seen).
    pub name: String,
    pub bytes: u64,
    pub count: usize,
}

/// Turns the process list into per-group samples. Per process the private working set is used
/// where `src` has it, else the working set. Grouping reuses `stats::group_by_name`.
pub fn collect(procs: &[ProcInfo], src: &dyn PrivateMemory) -> Vec<GroupSample> {
    let adjusted: Vec<ProcInfo> = procs
        .iter()
        .map(|p| {
            let mut q = p.clone();
            q.working_set = src.private_working_set(p.pid).unwrap_or(p.working_set);
            q
        })
        .collect();
    group_by_name(&adjusted)
        .into_iter()
        .map(|g| GroupSample {
            key: g.name.to_lowercase(),
            name: g.name,
            bytes: g.working_set,
            count: g.count,
        })
        .collect()
}

/// Real reader using `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`. Where the OS does not
/// support the EX2 structure, or the process cannot be opened, it returns `None` and `collect`
/// falls back to the working set.
#[derive(Default)]
pub struct WindowsPrivateMemory;

impl PrivateMemory for WindowsPrivateMemory {
    fn private_working_set(&self, pid: u32) -> Option<u64> {
        use std::mem::size_of;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
        };
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

        struct Guard(HANDLE);
        impl Drop for Guard {
            fn drop(&mut self) {
                // SAFETY: the handle came from OpenProcess and is closed exactly once.
                let _ = unsafe { CloseHandle(self.0) };
            }
        }

        // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let handle = Guard(handle);

        let mut c = PROCESS_MEMORY_COUNTERS_EX2::default();
        let size = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
        c.cb = size;
        // SAFETY: `c` is a valid out-parameter of `size` bytes; the EX2 layout starts with the
        // PROCESS_MEMORY_COUNTERS layout, which is what the API's pointer type expects.
        let ok = unsafe {
            GetProcessMemoryInfo(
                handle.0,
                (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(),
                size,
            )
        }
        .is_ok();
        // Older builds reject the larger struct or leave the field zero: report "not available".
        if ok && c.PrivateWorkingSetSize > 0 {
            Some(c.PrivateWorkingSetSize as u64)
        } else {
            None
        }
    }
}

// ---- Tracker ---------------------------------------------------------------------------------

struct Series {
    count: usize,
    last_seen: u64,
    /// (timestamp ms, bytes), oldest first.
    points: VecDeque<(u64, u64)>,
}

/// Keeps bounded per-group history and computes growth.
#[derive(Default)]
pub struct GrowthTracker {
    groups: HashMap<String, Series>,
}

impl GrowthTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one round of samples taken at `now_ms`. Drops groups that have not been seen for
    /// `GONE_AFTER_MS`, history older than `KEEP_MS`, and thins points older than an hour.
    pub fn record(&mut self, now_ms: u64, samples: &[GroupSample]) {
        for s in samples {
            let series = self.groups.entry(s.key.clone()).or_insert_with(|| Series {
                count: 0,
                last_seen: now_ms,
                points: VecDeque::new(),
            });
            series.count = s.count;
            series.last_seen = now_ms;
            let too_close = series
                .points
                .back()
                .is_some_and(|&(t, _)| now_ms >= t && now_ms - t < MIN_SPACING_MS);
            if !too_close {
                series.points.push_back((now_ms, s.bytes));
            }
        }
        self.groups
            .retain(|_, g| now_ms.saturating_sub(g.last_seen) < GONE_AFTER_MS);
        for g in self.groups.values_mut() {
            prune(&mut g.points, now_ms);
        }
    }

    /// Number of groups currently tracked.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Total stored points (for bounds tests).
    #[cfg(test)]
    fn point_count(&self) -> usize {
        self.groups.values().map(|g| g.points.len()).sum()
    }

    /// Growth and hint for the group with this lowercase process name, as of `now_ms`.
    pub fn info(&self, key: &str, now_ms: u64) -> Option<GrowthInfo> {
        let g = self.groups.get(&key.to_lowercase())?;
        let bytes = g.points.back()?.1;
        let per_hour = slope_per_hour(&g.points, now_ms, SLOPE_WINDOW_MS, MIN_SPAN_MS);
        let hint = if is_browser(key) && g.count >= MANY_TABS_PROCESSES {
            Hint::ManyTabs
        } else if is_growing(&g.points, now_ms) {
            Hint::Growing
        } else {
            Hint::Fine
        };
        Some(GrowthInfo {
            bytes,
            per_hour,
            hint,
        })
    }
}

fn is_browser(key: &str) -> bool {
    let k = key.to_lowercase();
    let k = k.strip_suffix(".exe").unwrap_or(&k);
    BROWSERS.contains(&k)
}

/// Keeps `KEEP_MS` of history; points older than `FULL_RES_MS` are thinned to `OLD_SPACING_MS`.
fn prune(points: &mut VecDeque<(u64, u64)>, now_ms: u64) {
    while points
        .front()
        .is_some_and(|&(t, _)| now_ms.saturating_sub(t) > KEEP_MS)
    {
        points.pop_front();
    }
    let mut kept: VecDeque<(u64, u64)> = VecDeque::with_capacity(points.len());
    let mut last_old: Option<u64> = None;
    for &(t, b) in points.iter() {
        if now_ms.saturating_sub(t) > FULL_RES_MS {
            if last_old.is_none_or(|lt| t.saturating_sub(lt) >= OLD_SPACING_MS) {
                kept.push_back((t, b));
                last_old = Some(t);
            }
        } else {
            kept.push_back((t, b));
        }
    }
    *points = kept;
}

/// Least-squares slope in bytes per hour over the points in `[now - window, now]`.
/// `None` if the points cover less than `min_span`.
fn slope_per_hour(
    points: &VecDeque<(u64, u64)>,
    now_ms: u64,
    window: u64,
    min_span: u64,
) -> Option<i64> {
    let sel: Vec<(u64, u64)> = points
        .iter()
        .copied()
        .filter(|&(t, _)| t <= now_ms && now_ms - t <= window)
        .collect();
    if sel.len() < 2 || sel[sel.len() - 1].0 - sel[0].0 < min_span {
        return None;
    }
    let pts: Vec<(f64, f64)> = sel
        .iter()
        .map(|&(t, b)| (t as f64 / HOUR_MS as f64, b as f64))
        .collect();
    let n = pts.len() as f64;
    let mean_x = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let sxy: f64 = pts.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    Some((sxy / sxx).round() as i64)
}

/// Growing: at least the threshold per hour over the last hour AND over the last sustain window,
/// with data covering that sustain window.
fn is_growing(points: &VecDeque<(u64, u64)>, now_ms: u64) -> bool {
    let long = slope_per_hour(points, now_ms, SLOPE_WINDOW_MS, MIN_SPAN_MS);
    let sustained = slope_per_hour(
        points,
        now_ms,
        GROWING_SUSTAIN_MS,
        GROWING_SUSTAIN_MS - SUSTAIN_SLACK_MS,
    );
    matches!((long, sustained), (Some(a), Some(b))
        if a >= GROWING_BYTES_PER_HOUR && b >= GROWING_BYTES_PER_HOUR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(key: &str, bytes: u64, count: usize) -> GroupSample {
        GroupSample {
            key: key.to_string(),
            name: key.to_string(),
            bytes,
            count,
        }
    }

    /// Feeds `minutes` of one sample per minute, memory = start + rate_mb_per_hour * t.
    fn feed(t: &mut GrowthTracker, key: &str, count: usize, minutes: u64, rate_mb_h: i64) -> u64 {
        let start: i64 = 2000 * MB;
        let mut now = 0;
        for m in 0..=minutes {
            now = m * MINUTE_MS;
            let bytes = start + rate_mb_h * MB * m as i64 / 60;
            t.record(now, &[sample(key, bytes as u64, count)]);
        }
        now
    }

    #[test]
    fn slope_of_linear_growth() {
        let mut t = GrowthTracker::new();
        let now = feed(&mut t, "app", 1, 60, 300);
        let info = t.info("app", now).unwrap();
        let per_hour = info.per_hour.unwrap();
        assert!((per_hour - 300 * MB).abs() < MB / 100, "{per_hour}");
        assert_eq!(info.hint, Hint::Growing);
    }

    #[test]
    fn slope_negative_when_shrinking() {
        let mut t = GrowthTracker::new();
        let now = feed(&mut t, "app", 1, 30, -120);
        let per_hour = t.info("app", now).unwrap().per_hour.unwrap();
        assert!((per_hour + 120 * MB).abs() < MB / 100);
        assert_eq!(t.info("app", now).unwrap().hint, Hint::Fine);
    }

    #[test]
    fn needs_ten_minutes_of_data() {
        let mut t = GrowthTracker::new();
        let now = feed(&mut t, "app", 1, 9, 500);
        assert_eq!(t.info("app", now).unwrap().per_hour, None);
        let mut t2 = GrowthTracker::new();
        let now = feed(&mut t2, "app", 1, 10, 500);
        assert!(t2.info("app", now).unwrap().per_hour.is_some());
    }

    #[test]
    fn single_sample_has_no_slope() {
        let mut t = GrowthTracker::new();
        t.record(0, &[sample("app", 100, 1)]);
        let info = t.info("app", 0).unwrap();
        assert_eq!(info.per_hour, None);
        assert_eq!(info.bytes, 100);
    }

    #[test]
    fn growth_must_be_sustained_for_thirty_minutes() {
        let mut t = GrowthTracker::new();
        let now = feed(&mut t, "app", 1, 20, 600);
        // Fast, but only 20 minutes of data.
        assert!(t.info("app", now).unwrap().per_hour.unwrap() > GROWING_BYTES_PER_HOUR);
        assert_eq!(t.info("app", now).unwrap().hint, Hint::Fine);
    }

    #[test]
    fn slow_growth_is_fine() {
        let mut t = GrowthTracker::new();
        let now = feed(&mut t, "app", 1, 90, 100);
        assert_eq!(t.info("app", now).unwrap().hint, Hint::Fine);
    }

    #[test]
    fn old_growth_that_stopped_is_fine() {
        let mut t = GrowthTracker::new();
        // Climb for 30 minutes, then flat for 30 minutes.
        for m in 0..=60u64 {
            let bytes = 1000 * MB + 400 * MB * m.min(30) as i64 / 30;
            t.record(m * MINUTE_MS, &[sample("app", bytes as u64, 1)]);
        }
        let info = t.info("app", 60 * MINUTE_MS).unwrap();
        assert_eq!(info.hint, Hint::Fine);
    }

    #[test]
    fn many_tabs_only_for_browsers() {
        let mut t = GrowthTracker::new();
        t.record(
            0,
            &[
                sample("chrome", 1, MANY_TABS_PROCESSES),
                sample("code", 1, 40),
                sample("firefox.exe", 1, 3),
            ],
        );
        assert_eq!(t.info("chrome", 0).unwrap().hint, Hint::ManyTabs);
        assert_eq!(t.info("code", 0).unwrap().hint, Hint::Fine);
        assert_eq!(t.info("firefox.exe", 0).unwrap().hint, Hint::Fine);
        let mut t = GrowthTracker::new();
        t.record(0, &[sample("chrome", 1, MANY_TABS_PROCESSES - 1)]);
        assert_eq!(t.info("chrome", 0).unwrap().hint, Hint::Fine);
    }

    #[test]
    fn hint_texts() {
        assert_eq!(
            Hint::Growing.text(),
            "Growing. Restart it if things feel slow."
        );
        assert_eq!(
            Hint::ManyTabs.text(),
            "Many tabs. Close the ones you are done with."
        );
    }

    #[test]
    fn memory_is_bounded_over_two_days() {
        let mut t = GrowthTracker::new();
        // A sample every 2 seconds for 48 hours.
        let mut now = 0;
        while now <= 48 * HOUR_MS {
            t.record(now, &[sample("app", 1000 + now / 1000, 1)]);
            now += 2_000;
        }
        let full = (FULL_RES_MS / MIN_SPACING_MS) as usize + 2;
        let old = ((KEEP_MS - FULL_RES_MS) / OLD_SPACING_MS) as usize + 2;
        assert!(t.point_count() <= full + old, "{}", t.point_count());
        let g = &t.groups["app"];
        assert!(now - g.points.front().unwrap().0 <= KEEP_MS + 2_000 + MIN_SPACING_MS);
    }

    #[test]
    fn exited_apps_are_dropped_after_a_while() {
        let mut t = GrowthTracker::new();
        t.record(0, &[sample("a", 1, 1), sample("b", 1, 1)]);
        t.record(MINUTE_MS, &[sample("a", 1, 1)]);
        assert_eq!(t.group_count(), 2, "b is kept for a short while");
        t.record(GONE_AFTER_MS + MINUTE_MS, &[sample("a", 1, 1)]);
        assert_eq!(t.group_count(), 1);
        assert!(t.info("b", GONE_AFTER_MS + MINUTE_MS).is_none());
        assert!(t.info("a", GONE_AFTER_MS + MINUTE_MS).is_some());
    }

    struct Fake;
    impl PrivateMemory for Fake {
        fn private_working_set(&self, pid: u32) -> Option<u64> {
            (pid != 3).then_some(u64::from(pid) * 10)
        }
    }

    fn proc(pid: u32, name: &str, working_set: u64) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.to_string(),
            working_set,
            private: 0,
            count: 1,
        }
    }

    #[test]
    fn collect_groups_and_falls_back_to_working_set() {
        let procs = vec![
            proc(1, "Chrome.exe", 999),
            proc(2, "chrome.exe", 999),
            proc(3, "chrome.exe", 77), // no private figure: working set
            proc(4, "code.exe", 5),
        ];
        let mut out = collect(&procs, &Fake);
        out.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "chrome.exe");
        assert_eq!(out[0].count, 3);
        assert_eq!(out[0].bytes, 10 + 20 + 77);
        assert_eq!(out[1].bytes, 40);
    }

    #[test]
    fn real_reader_does_not_panic_on_own_and_bad_pid() {
        let r = WindowsPrivateMemory;
        let _ = r.private_working_set(std::process::id());
        assert_eq!(r.private_working_set(u32::MAX - 1), None);
    }
}
