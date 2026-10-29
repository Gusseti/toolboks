//! Auto-clean rules and the decision of when to run. No Win32 and no UI, so it is testable.
//! Everything lives in memory and resets when the app closes.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const LOG_CAP: usize = 50;

/// The large-standby trigger only fires while available memory is also below this.
pub const STANDBY_RULE_AVAILABLE_LIMIT_PERCENT: f64 = 20.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    pub enabled: bool,
    pub low_available: bool,
    pub low_available_percent: u32,
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
            low_available: true,
            low_available_percent: 10,
            big_standby: true,
            big_standby_gib: 4.0,
            interval: false,
            interval_minutes: 30,
            cooldown_minutes: 5,
        }
    }
}

/// What the rules look at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub total: u64,
    pub available: u64,
    /// `None` if the memory-list query is unavailable.
    pub standby: Option<u64>,
}

impl Snapshot {
    fn available_percent(&self) -> f64 {
        if self.total == 0 {
            100.0
        } else {
            self.available as f64 / self.total as f64 * 100.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    LowAvailable,
    BigStandby,
    Interval,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Reason::LowAvailable => "low available memory",
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

    let available = snap.available_percent();
    if rules.low_available && available < f64::from(rules.low_available_percent) {
        return Some(Reason::LowAvailable);
    }
    if rules.big_standby {
        if let Some(standby) = snap.standby {
            let big = standby as f64 >= f64::from(rules.big_standby_gib) * GIB;
            if big && available < STANDBY_RULE_AVAILABLE_LIMIT_PERCENT {
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

    /// 16 GiB machine with the given available and standby sizes.
    fn snap(available_gib: f64, standby_gib: Option<f64>) -> Snapshot {
        Snapshot {
            total: (16.0 * GIB) as u64,
            available: (available_gib * GIB) as u64,
            standby: standby_gib.map(|g| (g * GIB) as u64),
        }
    }

    #[test]
    fn disabled_never_runs() {
        let r = Rules {
            enabled: false,
            ..rules()
        };
        assert_eq!(decide(&r, &snap(0.1, Some(10.0)), 60 * MIN), None);
    }

    #[test]
    fn low_available_triggers_below_threshold() {
        // 10% of 16 GiB = 1.6 GiB.
        assert_eq!(
            decide(&rules(), &snap(1.0, Some(0.0)), 10 * MIN),
            Some(Reason::LowAvailable)
        );
        assert_eq!(decide(&rules(), &snap(2.0, Some(0.0)), 10 * MIN), None);
    }

    #[test]
    fn big_standby_needs_low_available_too() {
        // 3 GiB available is 18.75%: under the 20% limit but above the 10% low-memory rule.
        assert_eq!(
            decide(&rules(), &snap(3.0, Some(6.0)), 10 * MIN),
            Some(Reason::BigStandby)
        );
        // Plenty of memory available: a big standby list alone is fine.
        assert_eq!(decide(&rules(), &snap(8.0, Some(6.0)), 10 * MIN), None);
        // Small standby list: nothing to purge.
        assert_eq!(decide(&rules(), &snap(3.0, Some(1.0)), 10 * MIN), None);
    }

    #[test]
    fn missing_standby_query_disables_that_rule_only() {
        assert_eq!(decide(&rules(), &snap(3.0, None), 10 * MIN), None);
        assert_eq!(
            decide(&rules(), &snap(1.0, None), 10 * MIN),
            Some(Reason::LowAvailable)
        );
    }

    #[test]
    fn cooldown_blocks_every_trigger() {
        assert_eq!(decide(&rules(), &snap(0.5, Some(10.0)), 4 * MIN), None);
        assert!(decide(&rules(), &snap(0.5, Some(10.0)), 5 * MIN).is_some());
    }

    #[test]
    fn interval_fires_when_due_even_with_plenty_of_memory() {
        let r = Rules {
            low_available: false,
            big_standby: false,
            interval: true,
            interval_minutes: 30,
            ..rules()
        };
        assert_eq!(decide(&r, &snap(8.0, Some(1.0)), 29 * MIN), None);
        assert_eq!(
            decide(&r, &snap(8.0, Some(1.0)), 30 * MIN),
            Some(Reason::Interval)
        );
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
