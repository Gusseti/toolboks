//! Before/after report of an optimize run. Pure logic, no UI and no system calls, so it is
//! unit-tested. Reports live in memory only (the last `HISTORY_CAP` runs).

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use super::stats::ProcInfo;
use crate::format::size_plain;

const MB: u64 = 1024 * 1024;
/// How many finished reports are kept in memory.
pub const HISTORY_CAP: usize = 20;
/// Less than this moved out, or less than this gained as available memory, is a "small" result.
const SMALL: u64 = 50 * MB;
/// Loaded-back memory below this is not worth naming apps for.
const GREW_MIN: u64 = 8 * MB;
const HONEST_NOTE: &str =
    "Windows already frees unused memory by itself, so cleaning rarely makes a PC faster.";

/// System memory and per-process working sets at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub used: u64,
    pub available: u64,
    /// (process name, working set in bytes), one entry per process.
    pub procs: Vec<(String, u64)>,
}

impl Snapshot {
    pub fn new(used: u64, available: u64, procs: &[ProcInfo]) -> Self {
        Self {
            used,
            available,
            procs: procs
                .iter()
                .map(|p| (p.name.clone(), p.working_set))
                .collect(),
        }
    }
}

/// The result of one run, as shown in "Last run" and the history.
#[derive(Clone, Debug)]
pub struct RunReport {
    pub automatic: bool,
    pub finished: Instant,
    /// Working-set memory apps gave up right after the run.
    pub moved_out: u64,
    /// Available memory after settling minus available memory before the run.
    pub available_change: i64,
    /// Working-set memory apps took back between the run and the settled check.
    pub loaded_back: u64,
    /// Up to three apps that grew most after the run: (name, growth in bytes).
    pub grew_back: Vec<(String, u64)>,
}

impl RunReport {
    /// The result was too small to matter.
    pub fn is_small(&self) -> bool {
        self.moved_out < SMALL || self.available_change < SMALL as i64
    }
}

/// Working sets summed per lowercase name, remembering one display name.
fn by_name(s: &Snapshot) -> HashMap<String, (String, u64)> {
    let mut map: HashMap<String, (String, u64)> = HashMap::new();
    for (name, ws) in &s.procs {
        let e = map
            .entry(name.to_lowercase())
            .or_insert_with(|| (name.clone(), 0));
        e.1 += ws;
    }
    map
}

/// Compares the snapshot before the run, right after it (`immediate`) and 10 s later (`settled`).
pub fn compare(
    before: &Snapshot,
    immediate: &Snapshot,
    settled: &Snapshot,
    automatic: bool,
) -> RunReport {
    let (b, i, s) = (by_name(before), by_name(immediate), by_name(settled));
    let moved_out = b
        .iter()
        .map(|(k, (_, ws))| ws.saturating_sub(i.get(k).map_or(0, |v| v.1)))
        .sum();
    let mut grew: Vec<(String, u64)> = s
        .iter()
        .filter_map(|(k, (name, ws))| {
            // Only apps that were present right after the run; new apps are not "grew back".
            let was = i.get(k)?.1;
            let g = ws.saturating_sub(was);
            (g > 0).then(|| (name.clone(), g))
        })
        .collect();
    let loaded_back = grew.iter().map(|g| g.1).sum();
    grew.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    grew.retain(|g| g.1 >= GREW_MIN);
    grew.truncate(3);
    RunReport {
        automatic,
        finished: Instant::now(),
        moved_out,
        available_change: settled.available as i64 - before.available as i64,
        loaded_back,
        grew_back: grew,
    }
}

/// Plain sentences for the result (without the honest note).
pub fn describe(r: &RunReport) -> String {
    let mut text = format!("Moved {} out of app memory.", size_plain(r.moved_out));
    let change = size_plain(r.available_change.unsigned_abs());
    let more_less = if r.available_change >= 0 {
        "more"
    } else {
        "less"
    };
    text.push_str(&format!(
        " Ten seconds later Windows had {change} {more_less} available"
    ));
    if r.loaded_back >= GREW_MIN {
        text.push_str(&format!(
            ", and {} was loaded back in.",
            size_plain(r.loaded_back)
        ));
    } else {
        text.push_str(", and almost nothing was loaded back in.");
    }
    if !r.grew_back.is_empty() {
        let names: Vec<&str> = r.grew_back.iter().map(|g| g.0.as_str()).collect();
        text.push_str(&format!(" Apps that grew back: {}.", names.join(", ")));
    }
    text
}

/// The one-line honest note shown under the result.
pub fn honest_note() -> &'static str {
    HONEST_NOTE
}

/// One line for the history list.
pub fn one_line(r: &RunReport) -> String {
    format!(
        "{}: moved {}, available {}{}",
        if r.automatic { "Auto" } else { "Manual" },
        size_plain(r.moved_out),
        if r.available_change >= 0 { "+" } else { "-" },
        size_plain(r.available_change.unsigned_abs())
    )
}

/// Text for the clipboard.
pub fn copy_text(r: &RunReport) -> String {
    format!(
        "toolboks memory report ({})\n{}\n{}",
        if r.automatic { "automatic" } else { "manual" },
        describe(r),
        HONEST_NOTE
    )
}

/// Adds a report, dropping the oldest beyond `HISTORY_CAP`. Newest is at the back.
pub fn push(history: &mut VecDeque<RunReport>, report: RunReport) {
    history.push_back(report);
    while history.len() > HISTORY_CAP {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(available_mb: u64, procs: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            used: 0,
            available: available_mb * MB,
            procs: procs.iter().map(|(n, m)| (n.to_string(), m * MB)).collect(),
        }
    }

    fn report(moved_mb: u64, avail_mb: i64, back_mb: u64) -> RunReport {
        RunReport {
            automatic: false,
            finished: Instant::now(),
            moved_out: moved_mb * MB,
            available_change: avail_mb * MB as i64,
            loaded_back: back_mb * MB,
            grew_back: Vec::new(),
        }
    }

    #[test]
    fn compare_sums_drops_and_available_change() {
        let before = snap(1000, &[("a.exe", 500), ("b.exe", 300), ("c.exe", 100)]);
        let now = snap(1500, &[("a.exe", 100), ("b.exe", 300), ("c.exe", 60)]);
        let settled = snap(1400, &[("a.exe", 100), ("b.exe", 300), ("c.exe", 60)]);
        let r = compare(&before, &now, &settled, true);
        assert_eq!(r.moved_out, 440 * MB);
        assert_eq!(r.available_change, 400 * MB as i64);
        assert_eq!(r.loaded_back, 0);
        assert!(r.grew_back.is_empty());
        assert!(r.automatic);
    }

    #[test]
    fn growth_in_before_to_after_is_not_moved_out() {
        let before = snap(0, &[("a.exe", 100)]);
        let now = snap(0, &[("a.exe", 150)]);
        assert_eq!(compare(&before, &now, &now, false).moved_out, 0);
    }

    #[test]
    fn grew_back_is_ranked_and_capped_at_three() {
        let before = snap(0, &[]);
        let now = snap(
            0,
            &[
                ("a.exe", 10),
                ("b.exe", 10),
                ("c.exe", 10),
                ("d.exe", 10),
                ("e.exe", 10),
            ],
        );
        let settled = snap(
            0,
            &[
                ("a.exe", 20),
                ("B.exe", 110),
                ("c.exe", 60),
                ("d.exe", 40),
                ("e.exe", 12),
                ("new.exe", 900),
            ],
        );
        let r = compare(&before, &now, &settled, false);
        let names: Vec<&str> = r.grew_back.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(names, vec!["B.exe", "c.exe", "d.exe"]);
        assert_eq!(r.grew_back[0].1, 100 * MB);
        // Every grower counts toward the total, tiny ones included; new apps do not.
        assert_eq!(r.loaded_back, (10 + 100 + 50 + 30 + 2) * MB);
    }

    #[test]
    fn same_name_processes_are_summed() {
        let before = snap(0, &[("x.exe", 100), ("x.exe", 100)]);
        let now = snap(0, &[("x.exe", 50), ("X.exe", 50)]);
        assert_eq!(compare(&before, &now, &now, false).moved_out, 100 * MB);
    }

    #[test]
    fn describe_matches_the_plain_wording() {
        let mut r = report(2150, 410, 640);
        r.grew_back = vec![
            ("chrome.exe".into(), 400 * MB),
            ("Teams.exe".into(), 200 * MB),
        ];
        assert_eq!(
            describe(&r),
            "Moved 2.1 GB out of app memory. Ten seconds later Windows had 410 MB more \
             available, and 640 MB was loaded back in. Apps that grew back: chrome.exe, Teams.exe."
        );
    }

    #[test]
    fn describe_handles_loss_and_no_growth() {
        let text = describe(&report(100, -200, 0));
        assert!(text.contains("200 MB less available"));
        assert!(text.contains("almost nothing was loaded back in."));
        assert!(!text.contains("Apps that grew back"));
    }

    #[test]
    fn small_results_are_detected() {
        assert!(report(10, 500, 0).is_small());
        assert!(report(500, 10, 0).is_small());
        assert!(report(500, -10, 0).is_small());
        assert!(!report(500, 500, 0).is_small());
    }

    #[test]
    fn copy_text_has_description_and_note() {
        let t = copy_text(&report(500, 500, 0));
        assert!(t.contains("Moved 500 MB out of app memory."));
        assert!(t.ends_with("so cleaning rarely makes a PC faster."));
        assert!(one_line(&report(500, -200, 0)).contains("available -200 MB"));
    }

    #[test]
    fn history_keeps_the_last_twenty() {
        let mut h = VecDeque::new();
        for i in 0..25u64 {
            push(&mut h, report(i, 0, 0));
        }
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h.front().unwrap().moved_out, 5 * MB, "oldest five dropped");
        assert_eq!(h.back().unwrap().moved_out, 24 * MB);
    }
}
