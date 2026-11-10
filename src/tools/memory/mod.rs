mod actions;
mod activity;
mod admin;
mod advanced;
mod commit;
mod engine;
mod groups;
mod growth;
mod home;
mod memlist;
mod pressure;
mod report;
mod sampler;
mod scheduler;
mod series;
mod stats;
mod style;
mod trim;
mod workbench;

use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;

use self::actions::{Cooldowns, UndoLog};
use self::activity::Activity;
use self::admin::PurgeJob;
use self::engine::{RunJob, RunSummary, Settling};
use self::growth::GrowthTracker;
use self::memlist::Lists;
use self::pressure::{Assessment, PressureModel, Verdict, WindowsSignals};
use self::sampler::WindowsSampler;
use self::scheduler::Scheduler;
use self::stats::{Sampler, Snapshot, SortKey};
use self::workbench::Selection;
use super::Tool;
use crate::elevation;

const REFRESH: Duration = Duration::from_secs(2);

enum RefreshAction {
    Optimize(report::Trigger),
    Trim {
        selection: HashSet<String>,
        grouped: bool,
    },
}

struct RefreshWorker {
    request: Sender<Option<RefreshAction>>,
    result: Receiver<(Option<Snapshot>, Option<RefreshAction>)>,
}

/// Live RAM usage, a per-process table, manual per-process trimming, a pressure verdict, the
/// safe optimize run and an automatic mode that only acts under real memory pressure.
/// Nothing is stored anywhere: exclusions and filters reset when the app closes.
pub struct Memory {
    /// `None` if the platform refused to provide system info.
    sampler: Option<Snapshot>,
    refresh_worker: Option<RefreshWorker>,
    refresh_in_flight: usize,
    refresh_action_pending: bool,
    active: bool,
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
    /// The low-priority cache purge running on a worker thread, and its last result.
    purge: Option<PurgeJob>,
    last_purge: Option<String>,
    /// Result of the last action, shown above the table.
    status: Option<String>,
    /// The safe optimize run on a worker thread, what the last one did, and the settle
    /// readings still to come.
    run: Option<RunJob>,
    last_run: Option<RunSummary>,
    settling: Option<Settling>,
    /// Why the last automatic run started (the scheduler's text).
    run_note: Option<String>,
    /// Memory priorities the tool lowered, and when each app was last trimmed.
    undo: UndoLog,
    cooldowns: Cooldowns,
    /// When each app was last in front, for the idle checks of the safe actions.
    activity: Activity,
    /// Pressure verdict: the model, the Windows readings and the latest assessment.
    pressure: PressureModel,
    signals: Option<WindowsSignals>,
    assessment: Assessment,
    /// Automatic mode (off by default) and the advice shown instead of a run.
    scheduler: Scheduler,
    advice: Option<String>,
    advice_hidden: bool,
    /// Zero for the models' millisecond clock, when the pressure check last ran and when the
    /// page last drew a frame.
    epoch: Instant,
    last_eval: Instant,
    last_ui: Option<Instant>,
    /// Per-app growth, fed from the process refresh.
    growth: GrowthTracker,
    last_growth: Option<Instant>,
    /// Reads the numbers a run report compares.
    win_sampler: Option<WindowsSampler>,
    /// "Where your memory goes" on Home; rebuilt with every refresh. Not stored anywhere.
    breakdown: groups::Breakdown,
    /// Commit charge on Home; refreshed with the sampler. `None` if the query fails.
    commit: Option<commit::Commit>,
    /// The last 20 honest run reports, newest last. In memory only.
    reports: VecDeque<report::HonestReport>,
    /// Which resource the Home graph shows, how far back, and the measured history.
    selected: home::Metric,
    range: series::Range,
    history: series::History,
    /// Pages read from disk per second, from the latest pressure reading.
    hard_faults: Option<f64>,
    /// Home shows the Advanced tools instead of the overview.
    advanced_open: bool,
}

impl Memory {
    pub fn new() -> Self {
        let epoch = Instant::now();
        Self {
            sampler: None,
            refresh_worker: None,
            refresh_in_flight: 0,
            refresh_action_pending: false,
            active: true,
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
            purge: None,
            last_purge: None,
            status: None,
            run: None,
            last_run: None,
            settling: None,
            run_note: None,
            undo: UndoLog::new(),
            cooldowns: Cooldowns::new(),
            activity: Activity::new(0),
            pressure: PressureModel::new(),
            signals: Some(WindowsSignals::new()),
            assessment: Assessment {
                verdict: Verdict::Healthy,
                reasons: Vec::new(),
            },
            scheduler: Scheduler::default(),
            advice: None,
            advice_hidden: false,
            epoch,
            last_eval: Instant::now(),
            last_ui: None,
            growth: GrowthTracker::new(),
            last_growth: None,
            win_sampler: None,
            breakdown: Default::default(),
            commit: commit::query(),
            reports: VecDeque::new(),
            selected: home::Metric::Memory,
            range: series::Range::default(),
            history: series::History::new(),
            hard_faults: None,
            advanced_open: false,
        }
    }

    fn request_refresh(&mut self, ctx: &egui::Context, action: Option<RefreshAction>) -> bool {
        if !self.active || (action.is_none() && self.refresh_in_flight != 0) {
            return false;
        }
        if self.refresh_worker.is_none() {
            let (request, requests) = mpsc::channel();
            let (result, results) = mpsc::channel();
            let ctx = ctx.clone();
            let thread = std::thread::Builder::new()
                .name("memory-processes".into())
                .spawn(move || {
                    let mut sampler = None;
                    for action in requests {
                        if sampler.is_none() {
                            sampler = Sampler::new();
                        }
                        let snapshot = sampler.as_mut().map(Sampler::refresh);
                        if result.send((snapshot, action)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                });
            if thread.is_err() {
                self.status = Some("Could not start memory sampling.".into());
                return false;
            }
            self.refresh_worker = Some(RefreshWorker {
                request,
                result: results,
            });
        }
        let worker = self.refresh_worker.as_ref().unwrap();
        let is_action = action.is_some();
        if worker.request.send(action).is_err() {
            self.refresh_worker = None;
            self.refresh_in_flight = 0;
            self.refresh_action_pending = false;
            return false;
        }
        self.refresh_in_flight += 1;
        self.refresh_action_pending |= is_action;
        true
    }

    fn refresh_now(&mut self, ctx: &egui::Context) {
        let _ = self.request_refresh(ctx, None);
    }

    fn poll_refresh(&mut self, ctx: &egui::Context) {
        while let Some(worker) = &self.refresh_worker {
            let (snapshot, action) = match worker.result.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.refresh_worker = None;
                    self.refresh_in_flight = 0;
                    self.refresh_action_pending = false;
                    self.status = Some("Memory sampling stopped. Try again.".into());
                    break;
                }
            };
            self.refresh_in_flight -= 1;
            self.last_refresh = Instant::now();
            let available = snapshot.is_some();
            if let Some(snapshot) = snapshot.filter(|_| self.active || action.is_some()) {
                if self.active {
                    self.breakdown = groups::breakdown(&snapshot.processes, snapshot.totals.used);
                }
                self.sampler = Some(snapshot);
                if self.active {
                    self.lists = memlist::query();
                    self.commit = commit::query();
                    self.feed_growth();
                }
            }
            if let Some(action) = action {
                self.refresh_action_pending = false;
                if !available {
                    self.status = Some("Could not read a fresh process list. Try again.".into());
                } else {
                    match action {
                        RefreshAction::Optimize(trigger) => {
                            self.run_from_snapshot(ctx, trigger);
                        }
                        RefreshAction::Trim { selection, grouped } => {
                            self.trim_from_snapshot(ctx, &selection, grouped);
                        }
                    }
                }
                if !self.active {
                    self.sampler = None;
                    self.refresh_worker = None;
                    self.refresh_in_flight = 0;
                }
            }
        }
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
    fn tutorial_location(&self) -> String {
        match self.tab {
            advanced::Tab::Selected => "selected",
            advanced::Tab::System => "system",
            advanced::Tab::Rules => "rules",
        }
        .into()
    }
    fn tutorial_navigate(&mut self, section: &str) {
        self.tab = match section {
            "system" => advanced::Tab::System,
            "rules" => advanced::Tab::Rules,
            _ => advanced::Tab::Selected,
        };
    }
    fn tick(&mut self, ctx: &egui::Context) {
        self.poll_refresh(ctx);
        if self.refresh_worker.is_none() {
            self.refresh_now(ctx);
        }
        self.background(ctx);
    }

    fn suspend(&mut self) -> bool {
        self.active = false;
        self.sampler = None;
        if !self.refresh_action_pending {
            self.refresh_worker = None;
            self.refresh_in_flight = 0;
        }
        self.signals = None;
        self.last_ui = None;
        self.breakdown = Default::default();
        self.growth = GrowthTracker::new();
        self.last_growth = None;
        self.history = series::History::new();
        self.pressure = PressureModel::new();
        self.activity = Activity::new(self.now_ms());
        self.selection = Selection::default();
        if self.run.is_none() && self.settling.is_none() {
            self.win_sampler = None;
        }
        true
    }

    fn resume(&mut self) {
        self.active = true;
        self.signals = Some(WindowsSignals::new());
    }

    fn finish_pending(&mut self, ctx: &egui::Context) {
        if !self.active && self.refresh_action_pending {
            self.poll_refresh(ctx);
        }
        self.poll_purge(ctx);
        self.poll_run(ctx);
        self.poll_trim(ctx);
        self.settle_tick();
        if self.settling.is_some() {
            ctx.request_repaint_after(REFRESH);
        } else if self.sampler.is_none() && self.run.is_none() {
            self.win_sampler = None;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        if self.sampler.is_none() {
            if self.refresh_in_flight > 0 {
                ui.label("Collecting memory information…");
            } else {
                ui.label("Memory information is not available. Toolboks will try again.");
                if self.last_refresh.elapsed() >= REFRESH {
                    self.refresh_now(ui.ctx());
                }
                ui.request_repaint_after(REFRESH);
            }
            return;
        }

        self.last_ui = Some(Instant::now());
        if self.last_refresh.elapsed() >= REFRESH {
            self.refresh_now(ui.ctx());
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
