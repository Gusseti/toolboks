//! Commit charge: how much memory Windows has promised to programs against its limit
//! (RAM plus pagefile). Running out of commit, not low free RAM, is when apps fail.
//! Pagefile and compressed-memory sizes are not shown: no reliable source is wired in.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

const WARN_PERCENT: f32 = 85.0;
const HOT_PERCENT: f32 = 95.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub total: u64,
    pub limit: u64,
    pub peak: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Calm,
    Warn,
    Hot,
}

/// Reads the commit charge; `None` if the call fails.
pub fn query() -> Option<Commit> {
    let mut info = PERFORMANCE_INFORMATION::default();
    let size = size_of::<PERFORMANCE_INFORMATION>() as u32;
    info.cb = size;
    // SAFETY: `info` is a valid, correctly sized out-parameter for the call's duration.
    unsafe { GetPerformanceInfo(&mut info, size) }.ok()?;
    let page = info.PageSize as u64;
    Some(Commit {
        total: info.CommitTotal as u64 * page,
        limit: info.CommitLimit as u64 * page,
        peak: info.CommitPeak as u64 * page,
    })
}

pub fn percent(c: &Commit) -> f32 {
    if c.limit == 0 {
        0.0
    } else {
        (c.total as f64 / c.limit as f64 * 100.0) as f32
    }
}

pub fn headroom_bytes(c: &Commit) -> u64 {
    c.limit.saturating_sub(c.total)
}

pub fn level(c: &Commit) -> Level {
    let p = percent(c);
    if p >= HOT_PERCENT {
        Level::Hot
    } else if p >= WARN_PERCENT {
        Level::Warn
    } else {
        Level::Calm
    }
}

/// The plain sentence under the numbers; the warning replaces it when the limit is near.
pub fn sentence(c: &Commit) -> String {
    match level(c) {
        Level::Calm => format!(
            "Windows can promise {} more to programs before it runs out.",
            crate::format::size_plain(headroom_bytes(c))
        ),
        _ => "Close big apps or add pagefile space. Cleaning memory will not help here.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn commit(total_gb: f64, limit_gb: f64) -> Commit {
        Commit {
            total: (total_gb * GB as f64) as u64,
            limit: (limit_gb * GB as f64) as u64,
            peak: 0,
        }
    }

    #[test]
    fn percent_and_headroom() {
        let c = commit(16.0, 32.0);
        assert_eq!(percent(&c), 50.0);
        assert_eq!(headroom_bytes(&c), 16 * GB);
        assert_eq!(percent(&Commit::default()), 0.0);
        assert_eq!(headroom_bytes(&commit(40.0, 32.0)), 0);
    }

    #[test]
    fn level_thresholds() {
        assert_eq!(level(&commit(84.9, 100.0)), Level::Calm);
        assert_eq!(level(&commit(85.0, 100.0)), Level::Warn);
        assert_eq!(level(&commit(94.9, 100.0)), Level::Warn);
        assert_eq!(level(&commit(95.0, 100.0)), Level::Hot);
    }

    #[test]
    fn sentences() {
        assert_eq!(
            sentence(&commit(18.3, 31.9)),
            "Windows can promise 13.6 GB more to programs before it runs out."
        );
        assert_eq!(
            sentence(&commit(90.0, 100.0)),
            "Close big apps or add pagefile space. Cleaning memory will not help here."
        );
    }

    #[test]
    fn query_reads_something_sane() {
        // Read-only call; no memory command is issued.
        let c = query().expect("GetPerformanceInfo");
        assert!(c.limit >= c.total && c.total > 0);
    }
}
