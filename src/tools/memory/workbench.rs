//! Pure logic of the Advanced workbench: memory-list segments, row selection and the
//! "trim selected" plan. No UI here, so it stays testable.

use std::collections::HashSet;

use eframe::egui::Color32;

use super::memlist::Lists;
use super::stats::ProcInfo;
use super::trim::skip_reason;
use crate::ui::{BarPart, ACCENT, AMBER, GREEN};

/// Physical memory split into the four memory lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListSegments {
    pub active: u64,
    pub modified: u64,
    /// Cached (standby) memory.
    pub cached: u64,
    pub free: u64,
}

/// Active is whatever is left after free, standby and modified; never negative.
pub fn list_segments(total: u64, lists: Lists) -> ListSegments {
    let active = total
        .saturating_sub(lists.free)
        .saturating_sub(lists.standby)
        .saturating_sub(lists.modified);
    ListSegments {
        active,
        modified: lists.modified,
        cached: lists.standby,
        free: lists.free,
    }
}

const CACHED_GREY: Color32 = Color32::from_rgb(0x7a, 0x80, 0x8a);

/// Legend entries in bar order: label, color, bytes.
pub fn legend(seg: ListSegments) -> [(&'static str, Color32, u64); 4] {
    [
        ("Active", ACCENT, seg.active),
        ("Modified", AMBER, seg.modified),
        ("Cached", CACHED_GREY, seg.cached),
        ("Free", GREEN, seg.free),
    ]
}

/// Bar parts as fractions of `total`; empty when the lists are unknown or `total` is 0.
pub fn list_bar_parts(total: u64, lists: Option<Lists>) -> Vec<BarPart> {
    let Some(lists) = lists else {
        return Vec::new();
    };
    if total == 0 {
        return Vec::new();
    }
    legend(list_segments(total, lists))
        .iter()
        .map(|&(_, color, bytes)| BarPart {
            fraction: (bytes as f64 / total as f64) as f32,
            color,
        })
        .collect()
}

/// Share of memory in use held by `bytes`, 0 to 1 (more than 1 only if sampling disagrees).
pub fn share_of(bytes: u64, used: u64) -> f32 {
    if used == 0 {
        0.0
    } else {
        (bytes as f64 / used as f64) as f32
    }
}

/// Stable key of a table row: lowercase name when grouped, "pid:<n>" when not.
pub fn row_key(grouped: bool, p: &ProcInfo) -> String {
    if grouped {
        p.name.to_lowercase()
    } else {
        format!("pid:{}", p.pid)
    }
}

/// The selected rows, remembered by key so they survive the refresh.
#[derive(Default)]
pub struct Selection {
    keys: HashSet<String>,
    /// The row last clicked, where a Shift range starts.
    anchor: Option<String>,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    pub fn keys(&self) -> &HashSet<String> {
        &self.keys
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.anchor = None;
    }

    /// Only this row.
    pub fn only(&mut self, key: &str) {
        self.keys.clear();
        self.keys.insert(key.to_string());
        self.anchor = Some(key.to_string());
    }

    /// Adds the row, or removes it if it was selected.
    pub fn toggle(&mut self, key: &str) {
        if !self.keys.remove(key) {
            self.keys.insert(key.to_string());
        }
        self.anchor = Some(key.to_string());
    }

    /// Selects every row from the anchor to `key` in the order shown (`visible`), replacing the
    /// selection. Without a usable anchor it selects just `key`.
    pub fn range(&mut self, key: &str, visible: &[String]) {
        let pos = |k: &str| visible.iter().position(|v| v == k);
        let (Some(to), Some(from)) = (pos(key), self.anchor.as_deref().and_then(pos)) else {
            self.only(key);
            return;
        };
        let (lo, hi) = (from.min(to), from.max(to));
        self.keys = visible[lo..=hi].iter().cloned().collect();
        // The anchor stays, so Shift-clicking again re-ranges from the same row.
    }

    /// Drops keys that no longer exist.
    pub fn prune(&mut self, existing: &HashSet<String>) {
        self.keys.retain(|k| existing.contains(k));
        if self.anchor.as_ref().is_some_and(|a| !existing.contains(a)) {
            self.anchor = None;
        }
    }
}

/// What "Trim selected" will do.
#[derive(Debug, PartialEq, Eq)]
pub struct TrimPlan {
    /// Trimmable processes: (pid, name).
    pub targets: Vec<(u32, String)>,
    /// Number of selected rows that contribute at least one target.
    pub groups: usize,
    /// Selected processes that are protected (system, own, excluded).
    pub skipped: usize,
}

pub fn plan_trim(
    procs: &[ProcInfo],
    selected: &HashSet<String>,
    grouped: bool,
    own_pid: u32,
    exclusions: &HashSet<String>,
) -> TrimPlan {
    let mut targets = Vec::new();
    let mut skipped = 0;
    let mut used_keys: HashSet<String> = HashSet::new();
    for p in procs {
        let key = row_key(grouped, p);
        if !selected.contains(&key) {
            continue;
        }
        if skip_reason(p.pid, &p.name, own_pid, exclusions).is_some() {
            skipped += 1;
        } else {
            targets.push((p.pid, p.name.clone()));
            used_keys.insert(key);
        }
    }
    TrimPlan {
        targets,
        groups: used_keys.len(),
        skipped,
    }
}

/// The one-line result of a trim run.
pub fn trim_summary(
    trimmed: usize,
    groups: usize,
    before: u64,
    after: u64,
    skipped: usize,
) -> String {
    format!(
        "Trimmed {trimmed} {} from {groups} {}: {} \u{2192} {} (working set), skipped {skipped} protected",
        if trimmed == 1 { "process" } else { "processes" },
        if groups == 1 { "group" } else { "groups" },
        super::stats::format_bytes(before),
        super::stats::format_bytes(after),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(free: u64, modified: u64, standby: u64) -> Lists {
        Lists {
            free,
            modified,
            standby,
        }
    }

    fn proc(pid: u32, name: &str) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.to_string(),
            working_set: 10,
            private: 10,
            count: 1,
        }
    }

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn segments_add_up_and_never_go_negative() {
        let s = list_segments(100, lists(20, 5, 30));
        assert_eq!((s.active, s.modified, s.cached, s.free), (45, 5, 30, 20));
        assert_eq!(list_segments(10, lists(20, 5, 30)).active, 0);
    }

    #[test]
    fn bar_parts_empty_when_unknown_or_no_total() {
        assert!(list_bar_parts(100, None).is_empty());
        assert!(list_bar_parts(0, Some(lists(1, 1, 1))).is_empty());
        let parts = list_bar_parts(100, Some(lists(20, 5, 30)));
        assert_eq!(parts.len(), 4);
        assert!((parts[0].fraction - 0.45).abs() < 1e-6);
        assert!((parts[3].fraction - 0.20).abs() < 1e-6);
    }

    #[test]
    fn share_handles_zero_used() {
        assert_eq!(share_of(5, 0), 0.0);
        assert!((share_of(25, 100) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn row_keys_differ_by_mode() {
        let p = proc(7, "Chrome.EXE");
        assert_eq!(row_key(true, &p), "chrome.exe");
        assert_eq!(row_key(false, &p), "pid:7");
    }

    #[test]
    fn selection_toggle_only_clear() {
        let mut s = Selection::default();
        s.toggle("a");
        s.toggle("b");
        assert_eq!(s.len(), 2);
        s.toggle("a");
        assert!(!s.contains("a") && s.contains("b"));
        s.only("c");
        assert_eq!(s.len(), 1);
        assert!(s.contains("c"));
        s.clear();
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn selection_range_from_anchor_both_directions() {
        let visible = keys(&["a", "b", "c", "d", "e"]);
        let mut s = Selection::default();
        s.only("b");
        s.range("d", &visible);
        assert_eq!(s.len(), 3);
        assert!(s.contains("b") && s.contains("c") && s.contains("d"));
        s.range("a", &visible);
        assert_eq!(s.len(), 2);
        assert!(s.contains("a") && s.contains("b"));
    }

    #[test]
    fn selection_range_without_anchor_selects_one() {
        let visible = keys(&["a", "b", "c"]);
        let mut s = Selection::default();
        s.range("c", &visible);
        assert_eq!(s.len(), 1);
        assert!(s.contains("c"));
    }

    #[test]
    fn selection_prune_drops_missing_keys() {
        let mut s = Selection::default();
        s.toggle("a");
        s.toggle("b");
        let existing: HashSet<String> = ["b".to_string()].into_iter().collect();
        s.prune(&existing);
        assert_eq!(s.len(), 1);
        assert!(s.contains("b"));
    }

    #[test]
    fn plan_skips_protected_and_counts_groups() {
        let procs = vec![
            proc(100, "app.exe"),
            proc(101, "app.exe"),
            proc(102, "keep.exe"),
            proc(103, "lsass.exe"),
            proc(104, "other.exe"),
        ];
        let excl: HashSet<String> = ["keep.exe".to_string()].into_iter().collect();
        let sel: HashSet<String> = ["app.exe", "keep.exe", "lsass.exe"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let plan = plan_trim(&procs, &sel, true, 1, &excl);
        assert_eq!(plan.targets.len(), 2);
        assert_eq!(plan.groups, 1);
        assert_eq!(plan.skipped, 2);
    }

    #[test]
    fn plan_ungrouped_uses_pid_keys() {
        let procs = vec![proc(100, "app.exe"), proc(101, "app.exe")];
        let sel: HashSet<String> = ["pid:101".to_string()].into_iter().collect();
        let plan = plan_trim(&procs, &sel, false, 1, &HashSet::new());
        assert_eq!(plan.targets, vec![(101, "app.exe".to_string())]);
        assert_eq!(plan.groups, 1);
    }

    #[test]
    fn summary_text_is_one_line() {
        let t = trim_summary(3, 2, 1024 * 1024 * 100, 1024 * 1024 * 40, 1);
        assert!(t.starts_with("Trimmed 3 processes from 2 groups: 100.0 MiB"));
        assert!(t.ends_with("skipped 1 protected"));
    }
}
