//! The real [`MemorySampler`]: one reading of the system memory numbers a run report compares.
//!
//! Zero plus free, standby and modified come from the memory lists, commit from
//! `GetPerformanceInfo`, the private working set of the trimmed apps from
//! `GetProcessMemoryInfo`, and the two page rates from PDH. The rates need a baseline read, so
//! one sampler is created early and kept; a rate is `None` until PDH has two readings.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

use super::growth::{PrivateMemory, WindowsPrivateMemory};
use super::memlist;
use super::pressure::{RateCounter, PAGES_INPUT_PATH, PAGE_FAULTS_PATH};
use super::report::{MemorySampler, RunMeasurement};

pub struct WindowsSampler {
    pages_input: Option<RateCounter>,
    page_faults: Option<RateCounter>,
}

impl WindowsSampler {
    pub fn new() -> Self {
        Self {
            pages_input: RateCounter::new(PAGES_INPUT_PATH),
            page_faults: RateCounter::new(PAGE_FAULTS_PATH),
        }
    }
}

/// Commit charge in use, in bytes.
fn commit_used() -> Option<u64> {
    let mut info = PERFORMANCE_INFORMATION::default();
    let size = size_of::<PERFORMANCE_INFORMATION>() as u32;
    info.cb = size;
    // SAFETY: `info` is a valid, correctly sized out parameter.
    unsafe { GetPerformanceInfo(&mut info, size) }.ok()?;
    Some(info.CommitTotal as u64 * info.PageSize as u64)
}

impl MemorySampler for WindowsSampler {
    fn sample(&mut self, trimmed_pids: &[u32]) -> Option<RunMeasurement> {
        let lists = memlist::query()?;
        let commit_used = commit_used()?;
        let private = WindowsPrivateMemory;
        let private_ws = trimmed_pids
            .iter()
            .filter_map(|pid| private.private_working_set(*pid))
            .sum();
        Some(RunMeasurement {
            zero_free: lists.free,
            standby: lists.standby,
            modified: lists.modified,
            commit_used,
            private_ws,
            pages_input_per_sec: self.pages_input.as_mut().and_then(|c| c.read()),
            page_faults_per_sec: self.page_faults.as_mut().and_then(|c| c.read()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_sampler_reads_the_basics() {
        // Read-only system calls. Sums the private working set of this very process.
        let mut sampler = WindowsSampler::new();
        let m = sampler
            .sample(&[std::process::id()])
            .expect("memory numbers readable");
        assert!(m.commit_used > 0);
        assert!(m.zero_free + m.standby > 0);
        assert!(m.private_ws > 0, "own private working set");
    }
}
