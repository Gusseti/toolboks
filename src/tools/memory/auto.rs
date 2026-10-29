//! Auto-clean rules and the decision of when to run. No Win32 and no UI, so it is testable.
//! Everything lives in memory and resets when the app closes.
//!
//! Windows counts the standby list as *available* memory, so a standby purge never changes
//! "available" (it only turns standby pages into free ones). The rules therefore look at the
//! free and standby sizes, which are what a purge actually moves.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const LOG_CAP: usize = 50;

/// The large-standby trigger only fires while free memory is below this share of RAM.
pub const STANDBY_RULE_FREE_LIMIT_PERCENT: f64 = 20.0;

/// With less standby memory than this there is nothing worth purging, so nothing runs.
pub const MIN_USEFUL_STANDBY: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub enabled: bool,
    pub big_standby: bool,
    pub big_standby_gib: f32,
    pub interval: bool,
    pub interval_minutes: u32,
    /// Minimum time between two runs, whatever the trigger.
    pub cooldown_minutes: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            enabled: false,
            big_standby: true,
            big_standby_gib: 4.0,
            interval: false,
            interval_minutes: 30,
            cooldown_minutes: 5,
        }
    }
}

/// What the rules look at. `free` and `standby` are `None` if the memory-list query failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub total: u64,
    /// Free plus zeroed pages.
    pub free: Option<u64>,
    pub standby: Option<u64>,
}

impl Snapshot {
    fn free_percent(&self) -> Option<f64> {
        let free = self.free?;
        (self.total > 0).then(|| free as f64 / self.total as f64 * 100.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    BigStandby,
    Interval,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Reason::BigStandby => "large standby list",
            Reason::Interval => "interval",
        }
    }
}

/// Which rule, if any, says to run now. `since_last_run` is the time since the previous run,
/// or since auto-clean was switched on.
pub fn decide(rules: &Rules, snap: &Snapshot, since_last_run: Duration) -> Option<Reason> {
    if !rules.enabled {
        return None;
    }
    if since_last_run < Duration::from_secs(u64::from(rules.cooldown_minutes) * 60) {
        return None;
    }
    // Nothing to purge: skip every trigger, otherwise a run would change nothing.
    if snap.standby.is_some_and(|s| s < MIN_USEFUL_STANDBY) {
        return None;
    }

    if rules.big_standby {
        if let (Some(standby), Some(free_percent)) = (snap.standby, snap.free_percent()) {
            let big = standby as f64 >= f64::from(rules.big_standby_gib) * GIB;
            if big && free_percent < STANDBY_RULE_FREE_LIMIT_PERCENT {
                return Some(Reason::BigStandby);
            }
        }
    }
    if rules.interval
        && since_last_run >= Duration::from_secs(u64::from(rules.interval_minutes) * 60)
    {
        return Some(Reason::Interval);
    }
    None
}

/// Rules plus the run clock and a short activity log.
pub struct AutoClean {
    pub rules: Rules,
    last_run: Instant,
    pub log: VecDeque<String>,
}

impl AutoClean {
    pub fn new() -> Self {
        Self {
            rules: Rules::default(),
            last_run: Instant::now(),
            log: VecDeque::new(),
        }
    }

    pub fn since_last_run(&self) -> Duration {
        self.last_run.elapsed()
    }

    /// Restart the clock, so the cooldown and interval count from now.
    pub fn mark_run(&mut self) {
        self.last_run = Instant::now();
    }

    /// Newest entries first when read from the front.
    pub fn push_log(&mut self, entry: String) {
        self.log.push_front(entry);
        self.log.truncate(LOG_CAP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn rules() -> Rules {
        Rules {
            enabled: true,
            ..Rules::default()
        }
    }

    /// A machine with `total_gib` of RAM; free and standby must fit inside it.
    fn snap(total_gib: f64, free_gib: f64, standby_gib: Option<f64>) -> Snapshot {
        assert!(
            free_gib + standby_gib.unwrap_or(0.0) <= total_gib,
            "impossible fixture"
        );
        Snapshot {
            total: (total_gib * GIB) as u64,
            free: Some((free_gib * GIB) as u64),
            standby: standby_gib.map(|g| (g * GIB) as u64),
        }
    }

    #[test]
    fn disabled_never_runs() {
        let r = Rules {
            enabled: false,
            ..rules()
        };
        assert_eq!(decide(&r, &snap(16.0, 0.1, Some(10.0)), 60 * MIN), None);
    }

    #[test]
    fn big_standby_fires_on_16_gib_when_free_is_low() {
        // 16 GiB: 20% free limit is 3.2 GiB. 6 GiB standby with 1 GiB free fires.
        assert_eq!(
            decide(&rules(), &snap(16.0, 1.0, Some(6.0)), 10 * MIN),
            Some(Reason::BigStandby)
        );
    }

    #[test]
    fn big_standby_is_reachable_on_8_gib_machines_too() {
        // The reviewed bug: with `available` this could never fire below 20 GiB.
        assert_eq!(
            decide(&rules(), &snap(8.0, 0.5, Some(4.5)), 10 * MIN),
            Some(Reason::BigStandby)
        );
    }

    #[test]
    fn big_standby_stays_quiet_when_free_memory_is_fine() {
        // 8 GiB free of 16 is 50%: no reason to drop the cache.
        assert_eq!(
            decide(&rules(), &snap(16.0, 8.0, Some(6.0)), 10 * MIN),
            None
        );
    }

    #[test]
    fn small_standby_never_runs_any_trigger() {
        let r = Rules {
            interval: true,
            interval_minutes: 1,
            ..rules()
        };
        // 100 MiB standby is under the 256 MiB floor: nothing worth purging.
        assert_eq!(decide(&r, &snap(16.0, 0.2, Some(0.1)), 60 * MIN), None);
    }

    #[test]
    fn unknown_standby_disables_the_standby_rule_only() {
        let no_standby = Snapshot {
            total: (16.0 * GIB) as u64,
            free: Some(GIB as u64),
            standby: None,
        };
        assert_eq!(decide(&rules(), &no_standby, 10 * MIN), None);
        let r = Rules {
            big_standby: false,
            interval: true,
            interval_minutes: 30,
            ..rules()
        };
        assert_eq!(decide(&r, &no_standby, 30 * MIN), Some(Reason::Interval));
    }

    #[test]
    fn cooldown_blocks_every_trigger() {
        let s = snap(16.0, 1.0, Some(6.0));
        assert_eq!(decide(&rules(), &s, 4 * MIN), None);
        assert!(decide(&rules(), &s, 5 * MIN).is_some());
    }

    #[test]
    fn interval_fires_when_due_with_a_useful_standby() {
        let r = Rules {
            big_standby: false,
            interval: true,
            interval_minutes: 30,
            ..rules()
        };
        let s = snap(16.0, 8.0, Some(2.0));
        assert_eq!(decide(&r, &s, 29 * MIN), None);
        assert_eq!(decide(&r, &s, 30 * MIN), Some(Reason::Interval));
    }

    #[test]
    fn log_keeps_newest_first_and_is_capped() {
        let mut auto = AutoClean::new();
        for i in 0..(LOG_CAP + 10) {
            auto.push_log(format!("entry {i}"));
        }
        assert_eq!(auto.log.len(), LOG_CAP);
        assert_eq!(auto.log.front().unwrap(), &format!("entry {}", LOG_CAP + 9));
    }
}
