//! Automatic-mode scheduler: decides when an automatic clean may run. Pure logic: the clock
//! is injected as `now_ms` and nothing sleeps. Off by default.
//!
//! Rules (issue #42, docs/RESEARCH-memory-cleaning.md):
//! - only when enabled;
//! - only after Under pressure has held for `PRESSURE_HOLD_MS` (the pressure model already
//!   holds; this is enforced again here);
//! - at most one automatic run per `COOLDOWN_MS`;
//! - after a run, re-arm only once the verdict has been Healthy or Busy at least once;
//! - never run when commit is at or above `COMMIT_NEAR_LIMIT`; advise closing apps instead.
//!
//! The scheduler never runs anything itself. `Decision::Run` repeats on every tick until the
//! caller reports the run with `record_run`. A manual run never calls `record_run`, so it
//! never consumes the automatic cooldown.

#![allow(dead_code)] // wired in by the integration slice

use super::pressure::{Assessment, Verdict};

/// Every tunable number in one place.
pub mod limits {
    /// Under pressure must have held this long before an automatic run.
    pub const PRESSURE_HOLD_MS: u64 = 60_000;
    /// At most one automatic run per this long.
    pub const COOLDOWN_MS: u64 = 60 * 60_000;
    /// Commit used fraction at or above this never runs; advice is shown instead.
    pub const COMMIT_NEAR_LIMIT: f64 = 0.95;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Idle,
    /// Commit is near the limit: trimming will not help. Show this text; never run.
    Advise(String),
    /// Run the safe actions now; `trigger` is plain text for the report.
    Run {
        trigger: String,
    },
}

#[derive(Debug)]
pub struct Scheduler {
    enabled: bool,
    pressure_since: Option<u64>,
    last_run_ms: Option<u64>,
    /// False after a run until the verdict has been Healthy or Busy.
    armed: bool,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Scheduler {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            pressure_since: None,
            last_run_ms: None,
            armed: true,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Turning it off or on forgets any pressure being timed; the cooldown is kept so
    /// toggling the switch cannot be used to beat the once-per-hour limit.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.pressure_since = None;
        }
        self.enabled = enabled;
    }

    /// Call after an automatic run (never after a manual one).
    pub fn record_run(&mut self, now_ms: u64) {
        self.last_run_ms = Some(now_ms);
        self.armed = false;
        self.pressure_since = None;
    }

    fn cooldown_left_ms(&self, now_ms: u64) -> u64 {
        match self.last_run_ms {
            Some(t) => limits::COOLDOWN_MS.saturating_sub(now_ms.saturating_sub(t)),
            None => 0,
        }
    }

    /// Feed one assessment taken at `now_ms`. `commit_used_fraction` is 0.0 to 1.0.
    pub fn tick(
        &mut self,
        now_ms: u64,
        assessment: &Assessment,
        commit_used_fraction: f64,
    ) -> Decision {
        if !self.enabled {
            return Decision::Idle;
        }
        if assessment.verdict != Verdict::UnderPressure {
            self.pressure_since = None;
            self.armed = true;
            return Decision::Idle;
        }
        let since = *self.pressure_since.get_or_insert(now_ms);
        if now_ms.saturating_sub(since) < limits::PRESSURE_HOLD_MS {
            return Decision::Idle;
        }
        if commit_used_fraction >= limits::COMMIT_NEAR_LIMIT {
            return Decision::Advise(
                "Programs have reserved almost all the memory Windows can promise. Trimming \
                 will not help here. Close some apps or browser tabs instead."
                    .to_string(),
            );
        }
        if !self.armed || self.cooldown_left_ms(now_ms) > 0 {
            return Decision::Idle;
        }
        let mut trigger = "Memory had been under pressure for a minute".to_string();
        if !assessment.reasons.is_empty() {
            trigger.push_str(": ");
            trigger.push_str(&assessment.reasons.join("; "));
        }
        Decision::Run { trigger }
    }

    /// Short text for the UI.
    pub fn status(&self, now_ms: u64) -> String {
        if !self.enabled {
            return "Off".to_string();
        }
        let left = self.cooldown_left_ms(now_ms);
        if left > 0 {
            let mins = left.div_ceil(60_000);
            let unit = if mins == 1 { "minute" } else { "minutes" };
            return format!("Cooling down for {mins} more {unit}");
        }
        if !self.armed {
            return "Waiting for memory to settle".to_string();
        }
        "Waiting for pressure".to_string()
    }
}

/// Shown in place of the old "start when in use reaches N%" slider.
pub fn legacy_threshold_note() -> &'static str {
    "Automatic cleaning no longer starts at a fixed percentage of memory in use. A high \
     percentage alone is normal, because Windows keeps spare memory busy on purpose. It now \
     waits until Windows is really short of memory for about a minute, runs at most once an \
     hour, and runs again only after memory has settled. If programs have reserved almost all \
     the memory Windows can promise, it tells you to close apps instead."
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn a(v: Verdict) -> Assessment {
        Assessment {
            verdict: v,
            reasons: vec!["Windows reports that memory is low".to_string()],
        }
    }

    fn is_run(d: &Decision) -> bool {
        matches!(d, Decision::Run { .. })
    }

    /// Ticks once a second over `[from, to]` and returns the last decision.
    fn feed(s: &mut Scheduler, from: u64, to: u64, v: Verdict, commit: f64) -> Decision {
        let mut t = from;
        let mut last = Decision::Idle;
        while t <= to {
            last = s.tick(t, &a(v), commit);
            t += 1_000;
        }
        last
    }

    #[test]
    fn off_by_default_and_never_runs() {
        let mut s = Scheduler::default();
        assert!(!s.is_enabled());
        assert_eq!(s.status(0), "Off");
        assert_eq!(
            feed(&mut s, 0, 10 * MIN, Verdict::UnderPressure, 0.5),
            Decision::Idle
        );
    }

    #[test]
    fn runs_only_after_hold() {
        let mut s = Scheduler::new(true);
        assert_eq!(
            feed(&mut s, 0, 59_000, Verdict::UnderPressure, 0.5),
            Decision::Idle
        );
        match s.tick(60_000, &a(Verdict::UnderPressure), 0.5) {
            Decision::Run { trigger } => {
                assert!(trigger.starts_with("Memory had been under pressure for a minute"));
                assert!(trigger.contains("memory is low"));
            }
            d => panic!("expected run, got {d:?}"),
        }
    }

    #[test]
    fn healthy_or_busy_never_runs() {
        let mut s = Scheduler::new(true);
        assert_eq!(
            feed(&mut s, 0, 10 * MIN, Verdict::Busy, 0.5),
            Decision::Idle
        );
        assert_eq!(
            feed(&mut s, 11 * MIN, 20 * MIN, Verdict::Healthy, 0.5),
            Decision::Idle
        );
    }

    #[test]
    fn a_gap_restarts_the_hold() {
        let mut s = Scheduler::new(true);
        feed(&mut s, 0, 50_000, Verdict::UnderPressure, 0.5);
        s.tick(51_000, &a(Verdict::Busy), 0.5);
        assert_eq!(
            feed(&mut s, 52_000, 100_000, Verdict::UnderPressure, 0.5),
            Decision::Idle
        );
        assert!(is_run(&s.tick(112_000, &a(Verdict::UnderPressure), 0.5)));
    }

    #[test]
    fn cooldown_blocks_second_run_for_an_hour() {
        let mut s = Scheduler::new(true);
        assert!(is_run(&feed(&mut s, 0, MIN, Verdict::UnderPressure, 0.5)));
        s.record_run(MIN);
        // Pressure clears, then returns: armed again but still cooling down.
        s.tick(2 * MIN, &a(Verdict::Healthy), 0.5);
        let start = 3 * MIN;
        let d = feed(&mut s, start, start + 10 * MIN, Verdict::UnderPressure, 0.5);
        assert_eq!(d, Decision::Idle);
        assert_eq!(s.status(18 * MIN), "Cooling down for 43 more minutes");
        // Hour after the run (61 min) the next run is allowed.
        s.tick(60 * MIN, &a(Verdict::Busy), 0.5);
        let d = feed(&mut s, 61 * MIN, 62 * MIN, Verdict::UnderPressure, 0.5);
        assert!(is_run(&d));
    }

    #[test]
    fn needs_rearm_after_run_even_when_cooldown_over() {
        let mut s = Scheduler::new(true);
        s.record_run(0);
        // Pressure never clears for two hours: no run, status explains.
        let d = feed(&mut s, 0, 2 * 60 * MIN, Verdict::UnderPressure, 0.5);
        assert_eq!(d, Decision::Idle);
        assert_eq!(s.status(2 * 60 * MIN), "Waiting for memory to settle");
        // One Busy tick re-arms; a fresh hold then runs.
        s.tick(2 * 60 * MIN + 1_000, &a(Verdict::Busy), 0.5);
        let t = 2 * 60 * MIN + 2_000;
        assert!(is_run(&feed(
            &mut s,
            t,
            t + MIN,
            Verdict::UnderPressure,
            0.5
        )));
    }

    #[test]
    fn run_repeats_until_recorded() {
        let mut s = Scheduler::new(true);
        assert!(is_run(&feed(&mut s, 0, MIN, Verdict::UnderPressure, 0.5)));
        assert!(is_run(&s.tick(
            MIN + 1_000,
            &a(Verdict::UnderPressure),
            0.5
        )));
        s.record_run(MIN + 1_000);
        assert_eq!(
            s.tick(MIN + 2_000, &a(Verdict::UnderPressure), 0.5),
            Decision::Idle
        );
    }

    #[test]
    fn manual_run_does_not_consume_cooldown() {
        // A manual run never reaches the scheduler, so the next automatic run is allowed.
        let mut s = Scheduler::new(true);
        assert_eq!(s.status(0), "Waiting for pressure");
        assert!(is_run(&feed(&mut s, 0, MIN, Verdict::UnderPressure, 0.5)));
    }

    #[test]
    fn near_commit_limit_advises_and_never_runs() {
        let mut s = Scheduler::new(true);
        assert_eq!(
            feed(&mut s, 0, 59_000, Verdict::UnderPressure, 0.97),
            Decision::Idle
        );
        match s.tick(60_000, &a(Verdict::UnderPressure), 0.95) {
            Decision::Advise(text) => {
                assert!(text.contains("Trimming will not help"));
                assert!(text.contains("Close"));
            }
            d => panic!("expected advice, got {d:?}"),
        }
        // Below 95% runs normally.
        assert!(is_run(&s.tick(61_000, &a(Verdict::UnderPressure), 0.949)));
        // Advice does not start the cooldown.
        assert_eq!(s.status(62_000), "Waiting for pressure");
    }

    #[test]
    fn disabling_stops_runs_and_enabling_restarts_the_hold() {
        let mut s = Scheduler::new(true);
        feed(&mut s, 0, 50_000, Verdict::UnderPressure, 0.5);
        s.set_enabled(false);
        assert_eq!(s.status(51_000), "Off");
        assert_eq!(
            feed(&mut s, 51_000, 200_000, Verdict::UnderPressure, 0.5),
            Decision::Idle
        );
        s.set_enabled(true);
        assert_eq!(
            s.tick(201_000, &a(Verdict::UnderPressure), 0.5),
            Decision::Idle
        );
        assert!(is_run(&s.tick(
            201_000 + MIN,
            &a(Verdict::UnderPressure),
            0.5
        )));
    }

    #[test]
    fn toggling_does_not_reset_cooldown() {
        let mut s = Scheduler::new(true);
        s.record_run(0);
        s.set_enabled(false);
        s.set_enabled(true);
        assert_eq!(s.status(MIN), "Cooling down for 59 more minutes");
        assert_eq!(
            s.status(59 * MIN + 30_000),
            "Cooling down for 1 more minute"
        );
    }

    #[test]
    fn legacy_note_explains() {
        let n = legacy_threshold_note();
        assert!(n.contains("once an hour") && n.contains("no longer"));
    }
}
