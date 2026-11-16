//! What Toolboks itself costs, for the line on the Settings page ("Toolboks uses about 0.3% CPU
//! and 38 MB"): CPU and memory of this process
//! (the host only; add-ons are separate processes). Read with two cheap calls on the process's own
//! handle, every [`EVERY`] while the Settings page is drawn, and never otherwise.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// How often the figure is read again while the Settings page is open.
pub const EVERY: Duration = Duration::from_secs(2);

/// One shown figure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Share of the whole PC (all logical cores), as Task Manager shows it.
    pub cpu_percent: f64,
    /// Private working set, the "Memory" column of Task Manager.
    pub memory_mb: f64,
}

#[derive(Default)]
pub struct OwnUsage {
    /// The last read: when, and the processor time used up to then (100 ns units).
    last: Option<(Instant, u64)>,
    shown: Option<Reading>,
}

impl OwnUsage {
    /// Reads again when [`EVERY`] has passed and returns the figure to show (none until two reads
    /// exist, since CPU use is a difference).
    pub fn poll(&mut self, now: Instant) -> Option<Reading> {
        let due = self
            .last
            .is_none_or(|(at, _)| now.saturating_duration_since(at) >= EVERY);
        if due {
            if let (Some(cpu), Some(memory)) = (cpu_time(), private_working_set()) {
                if let Some((at, before)) = self.last {
                    self.shown = Some(Reading {
                        cpu_percent: cpu_percent(
                            cpu.saturating_sub(before),
                            now.saturating_duration_since(at),
                            cores(),
                        ),
                        memory_mb: memory as f64 / (1024.0 * 1024.0),
                    });
                }
                self.last = Some((now, cpu));
            }
        }
        self.shown
    }

    /// The page closed: start fresh next time, so an old figure is never shown.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Processor time used (100 ns units) over `elapsed`, as a share of all `cores`.
pub fn cpu_percent(used_100ns: u64, elapsed: Duration, cores: usize) -> f64 {
    let wall = elapsed.as_secs_f64() * cores.max(1) as f64;
    if wall <= 0.0 {
        return 0.0;
    }
    (used_100ns as f64 / 10_000_000.0 / wall * 100.0).clamp(0.0, 100.0)
}

/// The Settings line, e.g. "Toolboks uses about 0.3% CPU and 38 MB (add-ons not included)".
pub fn text(reading: Option<Reading>) -> String {
    match reading {
        None => "Measuring what Toolboks uses...".into(),
        Some(r) => {
            let cpu = if r.cpu_percent < 0.1 {
                "less than 0.1%".to_string()
            } else {
                format!("about {:.1}%", r.cpu_percent)
            };
            format!(
                "Toolboks uses {cpu} CPU and {:.0} MB (add-ons not included)",
                r.memory_mb
            )
        }
    }
}

fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// Kernel plus user time of this process, in 100 ns units.
fn cpu_time() -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: the pseudo handle of this process is always valid; all four out pointers are valid.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    Some(ticks(kernel) + ticks(user))
}

/// This process's private working set in bytes.
fn private_working_set() -> Option<usize> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        ..Default::default()
    };
    // SAFETY: `counters` is a valid PROCESS_MEMORY_COUNTERS_EX2 and `cb` holds its size; the
    // function accepts the extended struct through the base pointer.
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .ok()?;
    Some(counters.PrivateWorkingSetSize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_is_a_share_of_all_cores() {
        // One full core for 2 s on an 8-core PC is 12.5% of the PC.
        let one_core_2s = 2 * 10_000_000;
        assert!((cpu_percent(one_core_2s, Duration::from_secs(2), 8) - 12.5).abs() < 1e-9);
        assert_eq!(cpu_percent(0, Duration::from_secs(2), 8), 0.0);
        // No time passed or no cores reported: never divide by zero, never above 100.
        assert_eq!(cpu_percent(5, Duration::ZERO, 8), 0.0);
        assert_eq!(cpu_percent(u64::MAX, Duration::from_secs(1), 0), 100.0);
    }

    #[test]
    fn text_is_one_plain_line() {
        assert_eq!(text(None), "Measuring what Toolboks uses...");
        let reading = Reading {
            cpu_percent: 0.04,
            memory_mb: 37.6,
        };
        assert_eq!(
            text(Some(reading)),
            "Toolboks uses less than 0.1% CPU and 38 MB (add-ons not included)"
        );
        let busy = Reading {
            cpu_percent: 1.26,
            memory_mb: 40.0,
        };
        assert!(text(Some(busy)).contains("about 1.3% CPU and 40 MB"));
    }

    #[test]
    fn reads_this_process_and_shows_a_figure_on_the_second_read() {
        let mut usage = OwnUsage::default();
        let start = Instant::now();
        assert_eq!(usage.poll(start), None);
        // Too soon: no new read, still nothing to show.
        assert_eq!(usage.poll(start + Duration::from_millis(100)), None);
        let reading = usage
            .poll(start + EVERY)
            .expect("a figure after the second read");
        assert!(reading.memory_mb > 0.0);
        assert!((0.0..=100.0).contains(&reading.cpu_percent));
        usage.reset();
        assert_eq!(usage.poll(start + EVERY * 2), None);
    }
}
