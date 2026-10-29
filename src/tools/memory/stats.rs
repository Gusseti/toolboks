//! Memory and process sampling. No UI here, so the logic stays testable.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    /// Physical memory currently held by the process (working set).
    pub working_set: u64,
    /// Private bytes: memory committed to this process alone.
    pub private: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

impl Totals {
    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 / self.total as f64) as f32
        }
    }
}

/// Reads system memory and the process list through `sysinfo`.
pub struct Sampler {
    sys: System,
    pub totals: Totals,
    pub processes: Vec<ProcInfo>,
}

impl Sampler {
    /// `None` if the platform refuses to provide system info.
    pub fn new() -> Option<Self> {
        let mut sampler = Self {
            sys: System::new().ok()?,
            totals: Totals::default(),
            processes: Vec::new(),
        };
        sampler.refresh();
        Some(sampler)
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );

        self.totals = Totals {
            total: self.sys.total_memory(),
            used: self.sys.used_memory(),
            available: self.sys.available_memory(),
        };
        self.processes = self
            .sys
            .processes()
            .values()
            .map(|p| ProcInfo {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                working_set: p.memory(),
                private: p.virtual_memory(),
            })
            .collect();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Pid,
    WorkingSet,
    Private,
}

/// Rows matching `filter` (case-insensitive, on name or pid), ordered by `key`.
pub fn sort_and_filter<'a>(
    procs: &'a [ProcInfo],
    key: SortKey,
    descending: bool,
    filter: &str,
) -> Vec<&'a ProcInfo> {
    let needle = filter.trim().to_lowercase();
    let mut rows: Vec<&ProcInfo> = procs
        .iter()
        .filter(|p| {
            needle.is_empty()
                || p.name.to_lowercase().contains(&needle)
                || p.pid.to_string().contains(&needle)
        })
        .collect();

    rows.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::WorkingSet => a.working_set.cmp(&b.working_set),
            SortKey::Private => a.private.cmp(&b.private),
        };
        let ord = if descending { ord.reverse() } else { ord };
        ord.then(a.pid.cmp(&b.pid))
    });
    rows
}

pub fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else {
        format!("{:.1} MiB", b / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, working_set: u64, private: u64) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.to_string(),
            working_set,
            private,
        }
    }

    fn sample() -> Vec<ProcInfo> {
        vec![
            proc(10, "chrome.exe", 300, 100),
            proc(20, "Code.exe", 500, 50),
            proc(30, "chrome.exe", 100, 400),
        ]
    }

    #[test]
    fn sorts_by_working_set_descending() {
        let procs = sample();
        let pids: Vec<u32> = sort_and_filter(&procs, SortKey::WorkingSet, true, "")
            .iter()
            .map(|p| p.pid)
            .collect();
        assert_eq!(pids, vec![20, 10, 30]);
    }

    #[test]
    fn sorts_by_name_case_insensitive_with_pid_tiebreak() {
        let procs = sample();
        let pids: Vec<u32> = sort_and_filter(&procs, SortKey::Name, false, "")
            .iter()
            .map(|p| p.pid)
            .collect();
        assert_eq!(pids, vec![10, 30, 20]);
    }

    #[test]
    fn filters_on_name_or_pid() {
        let procs = sample();
        assert_eq!(
            sort_and_filter(&procs, SortKey::Pid, false, "CHROME").len(),
            2
        );
        let by_pid = sort_and_filter(&procs, SortKey::Pid, false, "20");
        assert_eq!(by_pid.len(), 1);
        assert_eq!(by_pid[0].name, "Code.exe");
        assert!(sort_and_filter(&procs, SortKey::Pid, false, "nope").is_empty());
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0.0 MiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }

    #[test]
    fn used_fraction_handles_zero_total() {
        assert_eq!(Totals::default().used_fraction(), 0.0);
        let t = Totals {
            total: 200,
            used: 50,
            available: 150,
        };
        assert_eq!(t.used_fraction(), 0.25);
    }
}
