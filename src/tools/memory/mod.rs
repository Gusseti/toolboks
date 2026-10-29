mod admin;
mod advanced;
mod auto;
mod groups;
mod home;
mod memlist;
mod stats;
mod trim;
mod workbench;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use eframe::egui;

use self::admin::Job;
use self::auto::AutoClean;
use self::home::{Optimizing, Outcome, DEFAULT_AUTO_PERCENT, DEFAULT_MIN_APP_MB};
use self::memlist::Lists;
use self::stats::{Sampler, SortKey};
use self::workbench::Selection;
use super::Tool;
use crate::elevation;

const REFRESH: Duration = Duration::from_secs(2);
/// How often the auto-clean rules are checked, in the background too.
const AUTO_CHECK: Duration = Duration::from_secs(5);

/// Live RAM usage, a per-process table, manual per-process trimming and, with admin rights,
/// system-wide memory list commands and auto-clean (see `admin`).
/// Nothing is stored anywhere: exclusions and filters reset when the app closes.
pub struct Memory {
    /// `None` if the platform refused to provide system info.
    sampler: Option<Sampler>,
    /// Free/modified/standby sizes; `None` if the query is unavailable.
    lists: Option<Lists>,
    elevated: bool,
    last_refresh: Instant,
    sort_key: SortKey,
    descending: bool,
    filter: String,
    /// One table row per process name instead of per process.
    grouped: bool,
    /// Lowercase process names that are never trimmed.
    exclusions: HashSet<String>,
    /// Selected table rows (by stable key), and the inspector tab shown.
    selection: Selection,
    tab: advanced::Tab,
    /// Whether the table was grouped last frame, to clear the selection when it changes.
    was_grouped: bool,
    /// "Trim selected" running on a worker thread, and the result of the last one.
    trim_job: Option<advanced::TrimJob>,
    last_trim: Option<String>,
    /// A system-wide command waiting for the user's confirmation.
    pending_system: Option<memlist::Command>,
    /// The system-wide command running on a worker thread, if any.
    job: Option<Job>,
    /// Result of the last action, shown above the table.
    status: Option<String>,
    auto: AutoClean,
    last_auto_check: Instant,
    /// The one-click optimize run in progress, and the result of the last one (Home page).
    optimizing: Option<Optimizing>,
    outcome: Option<Outcome>,
    /// Home options: skip apps below this size, and also clear cached memory when elevated.
    opt_min_mb: u32,
    opt_deep: bool,
    /// Home "Clean automatically": on/off, the percentage in use that starts it, when it last
    /// started (for the pause between runs) and when the rule was last evaluated.
    auto_opt_on: bool,
    auto_opt_percent: u32,
    last_auto_opt: Option<Instant>,
    last_opt_check: Instant,
    /// "Where your memory goes" on Home; rebuilt with every refresh. Not stored anywhere.
    breakdown: groups::Breakdown,
}

impl Memory {
    pub fn new() -> Self {
        let sampler = Sampler::new();
        let breakdown = sampler
            .as_ref()
            .map(|s| groups::breakdown(&s.processes, s.totals.used))
            .unwrap_or_default();
        Self {
            sampler,
            lists: memlist::query(),
            elevated: elevation::is_elevated(),
            last_refresh: Instant::now(),
            sort_key: SortKey::WorkingSet,
            descending: true,
            filter: String::new(),
            grouped: false,
            exclusions: HashSet::new(),
            selection: Selection::default(),
            tab: advanced::Tab::Selected,
            was_grouped: false,
            trim_job: None,
            last_trim: None,
            pending_system: None,
            job: None,
            status: None,
            auto: AutoClean::new(),
            last_auto_check: Instant::now(),
            optimizing: None,
            outcome: None,
            opt_min_mb: DEFAULT_MIN_APP_MB,
            opt_deep: true,
            auto_opt_on: false,
            auto_opt_percent: DEFAULT_AUTO_PERCENT,
            last_auto_opt: None,
            last_opt_check: Instant::now(),
            breakdown,
        }
    }

    fn refresh_now(&mut self) {
        if let Some(sampler) = self.sampler.as_mut() {
            sampler.refresh();
            self.breakdown = groups::breakdown(&sampler.processes, sampler.totals.used);
        }
        self.lists = memlist::query();
        self.last_refresh = Instant::now();
    }

    /// Clicking the active column flips the order; a new column starts in its natural order.
    fn apply_sort(&mut self, key: SortKey) {
        if self.sort_key == key {
            self.descending = !self.descending;
        } else {
            self.sort_key = key;
            // Numbers read best largest-first, names and pids smallest-first.
            self.descending = matches!(key, SortKey::WorkingSet | SortKey::Private);
        }
    }

    fn toggle_exclusion(&mut self, name: String) {
        if !self.exclusions.remove(&name) {
            self.exclusions.insert(name);
        }
    }
}

impl Tool for Memory {
    fn name(&self) -> &'static str {
        "Memory Cleaner"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Home", "Advanced"]
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.background(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        if self.sampler.is_none() {
            ui.label("Memory information is not available on this system.");
            return;
        }

        if self.last_refresh.elapsed() >= REFRESH {
            self.refresh_now();
        }
        ui.request_repaint_after(REFRESH);

        if page == 0 {
            // Home lays out its own three zones and scrolls inside them.
            self.home_ui(ui);
            return;
        }
        self.advanced_ui(ui);
    }

    fn status(&self) -> Vec<String> {
        match self.sampler.as_ref() {
            Some(s) if s.totals.total > 0 => {
                vec![format!("RAM {:.0}%", s.totals.used_fraction() * 100.0)]
            }
            _ => Vec::new(),
        }
    }
}
