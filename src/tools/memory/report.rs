//! Honest run report of a memory clean (issue #41). Pure logic, no UI and no system calls, so it
//! is unit-tested. Reports live in memory only (the last `HISTORY_CAP` runs).
//!
//! A run is measured before, right after, and 30 s and 120 s later. The wording never says
//! memory was "freed": trimming moves it to the cache and it shows what came back.

use std::collections::VecDeque;
use std::time::Instant;

use crate::format::size_plain;

const MB: u64 = 1024 * 1024;
/// How many finished reports are kept in memory.
pub const HISTORY_CAP: usize = 20;
/// Less than this moved to cache is "almost nothing".
const SMALL: u64 = 50 * MB;
/// Memory that came back below this is not worth a sentence.
const GREW_MIN: u64 = 8 * MB;
const HONEST_NOTE: &str =
    "Windows already frees unused memory by itself, so cleaning rarely makes a PC faster.";

/// The one-line honest note shown under the result.
pub fn honest_note() -> &'static str {
    HONEST_NOTE
}

/// Commit moving by less than this is shown as "unchanged".
const COMMIT_UNCHANGED: u64 = 64 * MB;
/// Seconds after a run for the two settle checks.
pub const SETTLE_SECS: [u64; 2] = [30, 120];

/// System numbers at one moment of a run. Bytes, and rates per second where available.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunMeasurement {
    /// Zero plus free pages.
    pub zero_free: u64,
    pub standby: u64,
    pub modified: u64,
    /// Commit charge in use (does not change when memory is trimmed).
    pub commit_used: u64,
    /// Private working set summed over the apps that were trimmed.
    pub private_ws: u64,
    pub pages_input_per_sec: Option<f64>,
    pub page_faults_per_sec: Option<f64>,
}

impl RunMeasurement {
    fn cache_and_free(&self) -> u64 {
        self.zero_free + self.standby
    }
}

/// Reads one `RunMeasurement`. The real one is `sampler::WindowsSampler`.
pub trait MemorySampler {
    /// `trimmed_pids` are the apps whose private working set is summed. `None` if the system
    /// numbers could not be read.
    fn sample(&mut self, trimmed_pids: &[u32]) -> Option<RunMeasurement>;
}

/// The two settle checks after a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    At30s,
    At120s,
}

/// Which settle check is due `elapsed` after the run, given the readings already taken.
pub fn next_stage_due(elapsed: std::time::Duration, taken: &RunMeasurements) -> Option<Stage> {
    let secs = elapsed.as_secs();
    if taken.at_30s.is_none() && secs >= SETTLE_SECS[0] {
        Some(Stage::At30s)
    } else if taken.at_30s.is_some() && taken.at_120s.is_none() && secs >= SETTLE_SECS[1] {
        Some(Stage::At120s)
    } else {
        None
    }
}

/// The readings taken so far for one run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunMeasurements {
    pub before: RunMeasurement,
    pub after: RunMeasurement,
    pub at_30s: Option<RunMeasurement>,
    pub at_120s: Option<RunMeasurement>,
}

/// Why a run happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    /// Memory pressure held for a minute.
    Pressure,
}

impl Trigger {
    pub fn is_automatic(self) -> bool {
        self != Trigger::Manual
    }

    /// The rule that started the run, in plain words.
    pub fn text(self) -> &'static str {
        match self {
            Trigger::Manual => "You started this run.",
            Trigger::Pressure => "Memory had been under pressure for a minute.",
        }
    }
}

/// An honest account of one run: what moved, what came back, and that commit did not change.
#[derive(Clone, Debug, PartialEq)]
pub struct HonestReport {
    pub trigger: Trigger,
    pub measurements: RunMeasurements,
    pub apps_trimmed: usize,
    /// Bytes that left the apps' working sets and went to cache.
    pub moved_to_cache: u64,
    /// Bytes of that which apps loaded back by the latest check taken.
    pub came_back: u64,
    /// The latest settle check taken (30 or 120 seconds), if any.
    pub settled: Option<Stage>,
    /// Commit after minus commit before.
    pub commit_change: i64,
    /// Page reads per second rose clearly after the run (apps reloading memory).
    pub reload_signal: bool,
    /// When the run finished (for "5 min ago" in the history).
    pub finished: Instant,
}

impl HonestReport {
    /// Pure maths over the measurements.
    pub fn compute(trigger: Trigger, apps_trimmed: usize, m: RunMeasurements) -> Self {
        let ws_drop = m.before.private_ws.saturating_sub(m.after.private_ws);
        let cache_gain = m
            .after
            .cache_and_free()
            .saturating_sub(m.before.cache_and_free());
        // Both must show it: apps gave up memory AND the cache side grew by that much.
        let moved_to_cache = ws_drop.min(cache_gain);

        let (settled, latest) = match (m.at_120s, m.at_30s) {
            (Some(x), _) => (Some(Stage::At120s), Some(x)),
            (None, Some(x)) => (Some(Stage::At30s), Some(x)),
            _ => (None, None),
        };
        let came_back = latest.map_or(0, |l| {
            l.private_ws
                .saturating_sub(m.after.private_ws)
                .min(moved_to_cache)
        });
        let reload_signal =
            m.at_30s.is_some_and(
                |l| match (l.pages_input_per_sec, m.before.pages_input_per_sec) {
                    (Some(after), Some(before)) => after > 50.0 && after > before * 2.0,
                    _ => false,
                },
            );
        Self {
            trigger,
            measurements: m,
            apps_trimmed,
            moved_to_cache,
            came_back,
            settled,
            commit_change: m.after.commit_used as i64 - m.before.commit_used as i64,
            reload_signal,
            finished: Instant::now(),
        }
    }

    pub fn automatic(&self) -> bool {
        self.trigger.is_automatic()
    }

    /// Almost nothing moved to cache.
    pub fn is_small(&self) -> bool {
        self.moved_to_cache < SMALL
    }

    /// Part of the memory that moved came back.
    pub fn reverted(&self) -> bool {
        self.came_back >= GREW_MIN
    }

    /// Commit is shown as unchanged unless it really moved.
    pub fn commit_unchanged(&self) -> bool {
        self.commit_change.unsigned_abs() < COMMIT_UNCHANGED
    }

    /// Plain sentences. Never says memory was "freed".
    pub fn sentences(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.moved_to_cache < SMALL {
            out.push("Almost nothing moved to cache.".to_string());
        } else {
            out.push(format!(
                "{} moved to cache.",
                size_plain(self.moved_to_cache)
            ));
        }
        let window = match self.settled {
            Some(Stage::At120s) => Some("within two minutes"),
            Some(Stage::At30s) => Some("within 30 seconds"),
            _ => None,
        };
        match window {
            None => out.push("Checking again after 30 seconds and two minutes.".to_string()),
            Some(w) if self.moved_to_cache >= SMALL && self.reverted() => out.push(format!(
                "{} came back {w} because apps used it again.",
                size_plain(self.came_back)
            )),
            Some(w) if self.moved_to_cache >= SMALL => {
                out.push(format!("Almost none of it came back {w}."))
            }
            Some(_) => {}
        }
        if self.reload_signal {
            out.push("Memory was reloaded from disk after the run.".to_string());
        }
        if self.commit_unchanged() {
            out.push(
                "Commit stayed the same: trimming moves memory to cache, it does not give it back \
                 to Windows."
                    .to_string(),
            );
        } else {
            out.push(format!(
                "Commit {} by {} because apps asked for or gave up memory on their own, not \
                 because of the run.",
                if self.commit_change > 0 {
                    "grew"
                } else {
                    "fell"
                },
                size_plain(self.commit_change.unsigned_abs())
            ));
        }
        out
    }

    /// The sentences, plus the rule that triggered an automatic run.
    pub fn text(&self) -> String {
        let mut t = self.sentences().join(" ");
        if self.automatic() {
            t.push_str("\nWhy it ran: ");
            t.push_str(self.trigger.text());
        }
        t
    }

    /// Text for the clipboard: the sentences, the rule behind an automatic run and the note.
    pub fn copy_text(&self) -> String {
        format!(
            "toolboks memory report ({})
{}
{}",
            if self.automatic() {
                "automatic"
            } else {
                "manual"
            },
            self.text(),
            HONEST_NOTE
        )
    }

    /// One line for the history list.
    pub fn one_line(&self) -> String {
        format!(
            "{}: {} moved to cache, {} came back",
            if self.automatic() { "Auto" } else { "Manual" },
            size_plain(self.moved_to_cache),
            size_plain(self.came_back)
        )
    }
}

/// Adds a report, keeping the last `HISTORY_CAP`; newest at the back.
pub fn push_report(history: &mut VecDeque<HonestReport>, report: HonestReport) {
    history.push_back(report);
    while history.len() > HISTORY_CAP {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meas(zf: u64, standby: u64, commit: u64, ws: u64) -> RunMeasurement {
        RunMeasurement {
            zero_free: zf * MB,
            standby: standby * MB,
            modified: 0,
            commit_used: commit * MB,
            private_ws: ws * MB,
            pages_input_per_sec: None,
            page_faults_per_sec: None,
        }
    }

    struct FakeSampler(VecDeque<Option<RunMeasurement>>, Vec<Vec<u32>>);

    impl MemorySampler for FakeSampler {
        fn sample(&mut self, pids: &[u32]) -> Option<RunMeasurement> {
            self.1.push(pids.to_vec());
            self.0.pop_front().flatten()
        }
    }

    fn run(m: RunMeasurements) -> HonestReport {
        HonestReport::compute(Trigger::Manual, 4, m)
    }

    #[test]
    fn fake_sampler_feeds_a_report() {
        let mut s = FakeSampler(
            VecDeque::from([
                Some(meas(1000, 2000, 8000, 3000)),
                Some(meas(1000, 3200, 8000, 1800)),
            ]),
            Vec::new(),
        );
        let before = s.sample(&[1, 2]).unwrap();
        let after = s.sample(&[1, 2]).unwrap();
        assert_eq!(s.1, vec![vec![1, 2], vec![1, 2]]);
        assert_eq!(s.sample(&[1]), None);
        let r = run(RunMeasurements {
            before,
            after,
            ..Default::default()
        });
        assert_eq!(r.moved_to_cache, 1200 * MB);
    }

    #[test]
    fn moved_needs_both_working_set_drop_and_cache_growth() {
        let base = meas(1000, 2000, 8000, 3000);
        let a = run(RunMeasurements {
            before: base,
            after: meas(1000, 2400, 8000, 2000),
            ..Default::default()
        });
        assert_eq!(a.moved_to_cache, 400 * MB);
        let b = run(RunMeasurements {
            before: base,
            after: meas(1500, 2500, 8000, 2700),
            ..Default::default()
        });
        assert_eq!(b.moved_to_cache, 300 * MB);
        let c = run(RunMeasurements {
            before: base,
            after: meas(900, 1900, 8000, 3100),
            ..Default::default()
        });
        assert_eq!(c.moved_to_cache, 0);
    }

    fn reverted_run(ws_at_120: u64) -> HonestReport {
        run(RunMeasurements {
            before: meas(1000, 2000, 8000, 3000),
            after: meas(1000, 3200, 8000, 1800),
            at_30s: Some(meas(1000, 3000, 8000, 1900)),
            at_120s: Some(meas(1000, 2900, 8000, ws_at_120)),
        })
    }

    #[test]
    fn revert_is_detected_and_capped_at_what_moved() {
        let r = reverted_run(2100);
        assert_eq!(r.moved_to_cache, 1200 * MB);
        assert_eq!(r.came_back, 300 * MB);
        assert!(r.reverted());
        assert_eq!(r.settled, Some(Stage::At120s));
        assert_eq!(reverted_run(5000).came_back, 1200 * MB);
        assert!(!reverted_run(1805).reverted());
    }

    #[test]
    fn falls_back_to_the_30s_check() {
        let r = run(RunMeasurements {
            before: meas(1000, 2000, 8000, 3000),
            after: meas(1000, 3200, 8000, 1800),
            at_30s: Some(meas(1000, 3000, 8000, 2000)),
            at_120s: None,
        });
        assert_eq!(r.settled, Some(Stage::At30s));
        assert_eq!(r.came_back, 200 * MB);
        assert!(r.sentences()[1].contains("within 30 seconds"));
    }

    #[test]
    fn wording_matches_the_plain_style() {
        let s = reverted_run(2100).sentences();
        assert_eq!(s[0], "1.2 GB moved to cache.");
        assert_eq!(
            s[1],
            "300 MB came back within two minutes because apps used it again."
        );
        assert!(s[2].starts_with("Commit stayed the same"));
        for line in &s {
            assert!(!line.to_lowercase().contains("freed"), "{line}");
        }
    }

    #[test]
    fn wording_for_pending_small_and_no_revert() {
        let pending = run(RunMeasurements {
            before: meas(1000, 2000, 8000, 3000),
            after: meas(1000, 3200, 8000, 1800),
            ..Default::default()
        });
        assert_eq!(
            pending.sentences()[1],
            "Checking again after 30 seconds and two minutes."
        );
        let held = reverted_run(1800).sentences();
        assert_eq!(held[1], "Almost none of it came back within two minutes.");
        let tiny = run(RunMeasurements {
            before: meas(1000, 2000, 8000, 3000),
            after: meas(1000, 2010, 8000, 2990),
            at_120s: Some(meas(1000, 2010, 8000, 2990)),
            ..Default::default()
        });
        assert_eq!(tiny.sentences()[0], "Almost nothing moved to cache.");
        assert!(!tiny.sentences().iter().any(|l| l.contains("came back")));
    }

    #[test]
    fn commit_is_unchanged_unless_it_really_moved() {
        let mut m = RunMeasurements {
            before: meas(0, 0, 8000, 0),
            after: meas(0, 0, 8030, 0),
            ..Default::default()
        };
        assert!(run(m).commit_unchanged());
        m.after = meas(0, 0, 8500, 0);
        let r = run(m);
        assert!(!r.commit_unchanged());
        assert_eq!(r.commit_change, 500 * MB as i64);
        assert!(r
            .sentences()
            .last()
            .unwrap()
            .contains("Commit grew by 500 MB"));
    }

    #[test]
    fn reload_signal_needs_a_clear_rise_in_page_reads() {
        let mut m = RunMeasurements::default();
        m.before.pages_input_per_sec = Some(20.0);
        let mut later = RunMeasurement {
            pages_input_per_sec: Some(400.0),
            ..Default::default()
        };
        m.at_30s = Some(later);
        let r = run(m);
        assert!(r.reload_signal);
        assert!(r
            .sentences()
            .iter()
            .any(|l| l.contains("reloaded from disk")));
        later.pages_input_per_sec = Some(30.0);
        m.at_30s = Some(later);
        assert!(!run(m).reload_signal);
        later.pages_input_per_sec = None;
        m.at_30s = Some(later);
        assert!(!run(m).reload_signal);
    }

    #[test]
    fn automatic_runs_show_the_trigger() {
        let r = HonestReport::compute(Trigger::Pressure, 3, RunMeasurements::default());
        assert!(r.automatic());
        assert!(r
            .text()
            .ends_with("Why it ran: Memory had been under pressure for a minute."));
        assert!(!run(RunMeasurements::default())
            .text()
            .contains("Why it ran"));
        assert!(r.one_line().starts_with("Auto:"));
    }

    #[test]
    fn stages_come_due_in_order() {
        use std::time::Duration;
        let mut m = RunMeasurements::default();
        let due = |s: u64, m: &RunMeasurements| next_stage_due(Duration::from_secs(s), m);
        assert_eq!(due(29, &m), None);
        assert_eq!(due(31, &m), Some(Stage::At30s));
        assert_eq!(due(130, &m), Some(Stage::At30s));
        m.at_30s = Some(RunMeasurement::default());
        assert_eq!(due(100, &m), None);
        assert_eq!(due(120, &m), Some(Stage::At120s));
        m.at_120s = Some(RunMeasurement::default());
        assert_eq!(due(500, &m), None);
    }

    #[test]
    fn honest_history_keeps_the_last_twenty() {
        let mut h = VecDeque::new();
        for i in 0..25u64 {
            let mut m = RunMeasurements::default();
            m.before.private_ws = i * 100 * MB;
            push_report(&mut h, run(m));
        }
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h.front().unwrap().measurements.before.private_ws, 500 * MB);
    }
}
