//! Commit charge: how much memory Windows has promised to programs against its limit
//! (RAM plus pagefile). Running out of commit, not low free RAM, is when apps fail.
//! Pagefile and compressed-memory sizes are not shown: no reliable source is wired in.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub total: u64,
    pub limit: u64,
    pub peak: u64,
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
    fn percent_of_the_limit() {
        let c = commit(16.0, 32.0);
        assert_eq!(percent(&c), 50.0);
        assert_eq!(percent(&Commit::default()), 0.0);
    }

    #[test]
    fn query_reads_something_sane() {
        // Read-only call; no memory command is issued.
        let c = query().expect("GetPerformanceInfo");
        assert!(c.limit >= c.total && c.total > 0);
    }
}
