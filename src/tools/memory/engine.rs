//! The running parts of the memory tool wired together: the pressure check and the automatic
//! scheduler, growth tracking, the safe optimize run (priority first, then targeted trim) and
//! the honest measurements that follow it.
//!
//! The decisions are in `pressure`, `scheduler`, `growth`, `actions` and `report`. This file only
//! feeds them and applies their answers. The pure helpers here are unit-tested.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

use super::actions::{
    self, ActionResult, Candidate, Cooldowns, Identity, Kind, Policy, UndoLog, WindowsOps,
};
use super::activity::{self, Activity};
use super::growth::{self, WindowsPrivateMemory};
use super::pressure::{Assessment, SignalSource, Verdict};
use super::report::{self, HonestReport, MemorySampler, RunMeasurement, RunMeasurements, Trigger};
use super::sampler::WindowsSampler;
use super::scheduler::Decision;
use super::stats::ProcInfo;
use super::Memory;
use crate::format::size_plain;

/// How often the pressure check runs while the memory page is open and while it is not.
const EVAL_OPEN: Duration = Duration::from_secs(1);
const EVAL_BACKGROUND: Duration = Duration::from_secs(10);
/// The page counts as open if it drew a frame this recently.
const OPEN_GRACE: Duration = Duration::from_secs(3);
/// The growth tracker is fed from the process refresh at most this often.
const GROWTH_EVERY: Duration = Duration::from_secs(10);

pub fn eval_interval(page_open: bool) -> Duration {
    if page_open {
        EVAL_OPEN
    } else {
        EVAL_BACKGROUND
    }
}

/// A run in progress: the actions execute on a worker thread.
pub struct RunJob {
    rx: Receiver<RunDone>,
    trigger: Trigger,
    before: RunMeasurement,
    trim_pids: Vec<u32>,
}

struct RunDone {
    results: Vec<ActionResult>,
    undo: UndoLog,
    cooldowns: Cooldowns,
}

/// What the safe actions did, in numbers, for the Last run card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub lowered: usize,
    pub trimmed: usize,
    pub failed: usize,
}

/// Said when a run found nothing to act on.
pub const NOTHING_TO_TRIM: &str = "Nothing to trim right now. Your idle apps are already small.";

fn apps(n: usize) -> String {
    format!("{n} {}", if n == 1 { "app" } else { "apps" })
}

impl RunSummary {
    pub fn from_results(results: &[ActionResult]) -> Self {
        let mut s = Self::default();
        for r in results {
            match (r.ok(), r.kind) {
                (false, _) => s.failed += 1,
                (true, Kind::LowerPriority) => s.lowered += 1,
                (true, Kind::Trim) => s.trimmed += 1,
            }
        }
        s
    }

    /// Plain words for what happened. Never says memory was freed.
    pub fn text(&self) -> String {
        if *self == Self::default() {
            return NOTHING_TO_TRIM.to_string();
        }
        let mut parts = Vec::new();
        if self.lowered > 0 {
            parts.push(format!(
                "Lowered the memory priority of {}.",
                apps(self.lowered)
            ));
        }
        if self.trimmed > 0 {
            parts.push(format!(
                "Asked {} to move unused memory to cache.",
                apps(self.trimmed)
            ));
        }
        if self.failed > 0 {
            parts.push(format!("{} could not be changed.", apps(self.failed)));
        }
        parts.join(" ")
    }
}

/// The settle readings (30 s and 120 s) of a finished run.
pub struct Settling {
    trigger: Trigger,
    apps_trimmed: usize,
    trim_pids: Vec<u32>,
    started: Instant,
    m: RunMeasurements,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettleStep {
    Waiting,
    /// A reading was taken; `last` if it was the final one.
    Took {
        last: bool,
    },
    /// The reading could not be taken; the run stays as reported so far.
    Failed,
}

impl Settling {
    pub fn new(
        trigger: Trigger,
        apps_trimmed: usize,
        trim_pids: Vec<u32>,
        m: RunMeasurements,
    ) -> Self {
        Self {
            trigger,
            apps_trimmed,
            trim_pids,
            started: Instant::now(),
            m,
        }
    }

    pub fn report(&self) -> HonestReport {
        HonestReport::compute(self.trigger, self.apps_trimmed, self.m)
    }

    /// Takes the reading that is due `elapsed` after the run, if any.
    pub fn poll(&mut self, elapsed: Duration, sampler: &mut dyn MemorySampler) -> SettleStep {
        let Some(stage) = report::next_stage_due(elapsed, &self.m) else {
            return SettleStep::Waiting;
        };
        let Some(reading) = sampler.sample(&self.trim_pids) else {
            return SettleStep::Failed;
        };
        match stage {
            report::Stage::At30s => self.m.at_30s = Some(reading),
            report::Stage::At120s => self.m.at_120s = Some(reading),
        }
        SettleStep::Took {
            last: stage == report::Stage::At120s,
        }
    }
}

/// Everything the candidate list needs from the outside world.
pub struct Inputs<'a> {
    pub procs: &'a [ProcInfo],
    pub parents: &'a HashMap<u32, u32>,
    pub activity: &'a Activity,
    pub now_ms: u64,
    pub foreground_pid: Option<u32>,
    pub fullscreen: bool,
    pub system_idle: Option<Duration>,
    /// Apps with a smaller working set are not looked at (they are too small to matter).
    pub min_bytes: u64,
}

/// One candidate per process that is big enough and can be identified.
pub fn candidates(
    inp: &Inputs,
    identity_of: &dyn Fn(u32, &str) -> Option<Identity>,
    plays_audio: &dyn Fn(u32) -> bool,
) -> Vec<Candidate> {
    let name_of = |pid: u32| inp.procs.iter().find(|p| p.pid == pid).map(|p| &p.name);
    let foreground_name = inp.foreground_pid.and_then(name_of);
    inp.procs
        .iter()
        .filter(|p| p.working_set >= inp.min_bytes)
        .filter_map(|p| {
            let identity = identity_of(p.pid, &p.name)?;
            let in_front_app = foreground_name.is_some_and(|f| f.eq_ignore_ascii_case(&p.name));
            Some(Candidate {
                identity,
                working_set: p.working_set,
                idle_for: inp.activity.idle_for(
                    &p.name,
                    inp.now_ms,
                    foreground_name.map(String::as_str),
                    inp.system_idle,
                ),
                plays_audio: plays_audio(p.pid),
                is_fullscreen: inp.fullscreen && in_front_app,
                parent_name: inp
                    .parents
                    .get(&p.pid)
                    .and_then(|parent| name_of(*parent))
                    .cloned(),
            })
        })
        .collect()
}

/// A manual run uses the looser policy, an automatic run the strict one.
pub fn policy_for(trigger: Trigger, own_pid: u32, foreground_pid: Option<u32>) -> Policy {
    if trigger == Trigger::Manual {
        Policy::manual(own_pid, foreground_pid)
    } else {
        Policy::new(own_pid, foreground_pid)
    }
}

/// "+120 MB/h", "-80 MB/h", or a note while there is not enough data yet.
pub fn per_hour_text(per_hour: Option<i64>) -> String {
    match per_hour {
        None => "collecting".to_string(),
        Some(b) if b.unsigned_abs() < 1024 * 1024 => "steady".to_string(),
        Some(b) => format!(
            "{}{}/h",
            if b > 0 { "+" } else { "-" },
            size_plain(b.unsigned_abs())
        ),
    }
}

/// A short line about the pressure verdict, or `None` while memory is healthy.
pub fn verdict_line(a: &Assessment) -> Option<String> {
    let head = match a.verdict {
        Verdict::Healthy => return None,
        Verdict::Busy => "Busy",
        Verdict::UnderPressure => "Under pressure",
    };
    if a.reasons.is_empty() {
        Some(head.to_string())
    } else {
        Some(format!("{head}: {}.", a.reasons.join("; ")))
    }
}

impl Memory {
    /// Milliseconds on the tool's own monotonic clock (what the models expect).
    pub(super) fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// The memory page drew a frame a moment ago.
    pub(super) fn page_open(&self) -> bool {
        self.last_ui.is_some_and(|t| t.elapsed() < OPEN_GRACE)
    }

    /// Feeds the growth tracker from the latest process list, at most every 10 s.
    pub(super) fn feed_growth(&mut self) {
        if self.last_growth.is_some_and(|t| t.elapsed() < GROWTH_EVERY) {
            return;
        }
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let samples = growth::collect(&sampler.processes, &WindowsPrivateMemory);
        let now = self.now_ms();
        self.growth.record(now, &samples);
        self.last_growth = Some(Instant::now());
    }

    /// Sampling and automatic checks run while the Memory Cleaner workspace is active.
    pub(super) fn background(&mut self, ctx: &egui::Context) {
        self.record_history();

        let open = self.page_open();
        let every = eval_interval(open);
        if self.last_eval.elapsed() >= every {
            self.last_eval = Instant::now();
            if !open {
                // The page refreshes itself while it is open.
                self.refresh_now();
            }
            self.note_foreground();
            self.evaluate(ctx);
        }
        ctx.request_repaint_after(if self.settling.is_some() {
            EVAL_OPEN
        } else {
            every
        });
    }

    fn note_foreground(&mut self) {
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let name = actions::foreground_pid()
            .and_then(|pid| sampler.processes.iter().find(|p| p.pid == pid))
            .map(|p| p.name.clone());
        let now = self.now_ms();
        self.activity.note_foreground(now, name.as_deref());
    }

    /// One pressure reading, then the scheduler's answer to it.
    fn evaluate(&mut self, ctx: &egui::Context) {
        let Some(signals) = self.signals.as_mut().and_then(SignalSource::read) else {
            return;
        };
        let now = self.now_ms();
        let assessment = self.pressure.evaluate(now, &signals);
        let commit_fraction = if signals.commit_limit > 0 {
            signals.commit_used as f64 / signals.commit_limit as f64
        } else {
            0.0
        };
        self.hard_faults = signals.hard_faults_per_sec;
        let decision = self.scheduler.tick(now, &assessment, commit_fraction);
        self.assessment = assessment;
        match decision {
            Decision::Idle => {
                self.advice = None;
                self.advice_hidden = false;
            }
            Decision::Advise(text) => self.advice = Some(text),
            Decision::Run { trigger } => {
                self.advice = None;
                if self.start_run(ctx, Trigger::Pressure) {
                    self.scheduler.record_run(now);
                    self.run_note = Some(trigger);
                }
            }
        }
    }

    /// Starts the safe plan (priority lowering, then targeted trim). Returns whether a run was
    /// attempted: `false` only while another job is busy, so an automatic run is retried.
    pub(super) fn start_run(&mut self, ctx: &egui::Context, trigger: Trigger) -> bool {
        if self.busy() {
            return false;
        }
        self.settling = None;
        if trigger == Trigger::Manual {
            self.run_note = None;
        }
        self.refresh_now();
        let Some(sampler) = self.sampler.as_ref() else {
            return false;
        };

        let now = Instant::now();
        let mut policy = policy_for(trigger, std::process::id(), actions::foreground_pid());
        policy.exclusions = self.exclusions.clone();
        // Keep entries as long as the strictest (automatic) cooldown needs them.
        self.cooldowns.prune(now, actions::AUTO_COOLDOWN);
        let inputs = Inputs {
            procs: &sampler.processes,
            parents: &sampler.parents,
            activity: &self.activity,
            now_ms: self.now_ms(),
            foreground_pid: policy.foreground_pid,
            fullscreen: activity::foreground_is_fullscreen(),
            system_idle: activity::system_idle(),
            min_bytes: policy.min_priority_bytes,
        };
        let cands = candidates(&inputs, &actions::identity_of, &actions::plays_audio);
        let plan = actions::plan(&cands, &policy, &self.cooldowns, now);

        if plan.is_empty() {
            self.last_run = Some(RunSummary::default());
            self.status = None;
            return true;
        }
        let trim_pids: Vec<u32> = plan
            .iter()
            .filter(|a| a.kind == Kind::Trim)
            .map(|a| a.identity.pid)
            .collect();
        let Some(before) = self
            .win_sampler
            .get_or_insert_with(WindowsSampler::new)
            .sample(&trim_pids)
        else {
            self.status = Some(
                "Could not read the memory numbers, so nothing was changed. Try again.".to_string(),
            );
            return true;
        };

        let mut undo = std::mem::take(&mut self.undo);
        let mut cooldowns = std::mem::take(&mut self.cooldowns);
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let results = actions::run(
                &plan,
                &WindowsOps,
                &mut undo,
                &mut cooldowns,
                Instant::now(),
            );
            let _ = tx.send(RunDone {
                results,
                undo,
                cooldowns,
            });
            wake.request_repaint();
        });
        self.run = Some(RunJob {
            rx,
            trigger,
            before,
            trim_pids,
        });
        self.last_run = None;
        self.status = None;
        true
    }

    /// Picks up a finished run and starts the settle readings.
    pub(super) fn poll_run(&mut self, ctx: &egui::Context) {
        let outcome = match self.run.as_ref() {
            None => return,
            Some(job) => job.rx.try_recv(),
        };
        let done = match outcome {
            Ok(done) => done,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => {
                self.run = None;
                self.status = Some("The run stopped unexpectedly.".to_string());
                return;
            }
        };
        let Some(job) = self.run.take() else {
            return;
        };
        self.undo = done.undo;
        self.cooldowns = done.cooldowns;
        let summary = RunSummary::from_results(&done.results);
        self.last_run = Some(summary);
        self.refresh_now();

        let Some(after) = self
            .win_sampler
            .get_or_insert_with(WindowsSampler::new)
            .sample(&job.trim_pids)
        else {
            self.status = Some("Could not read the memory numbers after the run.".to_string());
            return;
        };
        let settling = Settling::new(
            job.trigger,
            summary.trimmed,
            job.trim_pids,
            RunMeasurements {
                before: job.before,
                after,
                at_30s: None,
                at_120s: None,
            },
        );
        report::push_report(&mut self.reports, settling.report());
        self.settling = Some(settling);
        if job.trigger.is_automatic() {
            // Flash the taskbar button so a minimized window still tells you.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

    /// Takes the 30 s and 120 s readings when they are due and refreshes the newest report.
    pub(super) fn settle_tick(&mut self) {
        let Some(settling) = self.settling.as_mut() else {
            return;
        };
        let elapsed = settling.started.elapsed();
        match settling.poll(
            elapsed,
            self.win_sampler.get_or_insert_with(WindowsSampler::new),
        ) {
            SettleStep::Waiting => {}
            SettleStep::Took { last } => {
                if let Some(newest) = self.reports.back_mut() {
                    let finished = newest.finished;
                    *newest = settling.report();
                    newest.finished = finished;
                }
                if last {
                    self.settling = None;
                }
            }
            SettleStep::Failed => self.settling = None,
        }
    }

    /// Puts the memory priority of every app the tool lowered back where it was.
    pub(super) fn restore_priorities(&mut self) {
        let restored = self.undo.restore_all(&WindowsOps);
        let ok = restored.iter().filter(|r| r.result.is_ok()).count();
        self.status = Some(format!("Put the memory priority back for {}.", apps(ok)));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    const MB: u64 = 1024 * 1024;

    fn proc(pid: u32, name: &str, mb: u64) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.to_string(),
            working_set: mb * MB,
            private: 0,
            count: 1,
        }
    }

    fn ident(pid: u32, name: &str) -> Option<Identity> {
        Some(Identity {
            pid,
            created: 1,
            name: name.to_string(),
        })
    }

    fn reading(zero_free: u64, standby: u64, ws: u64) -> RunMeasurement {
        RunMeasurement {
            zero_free: zero_free * MB,
            standby: standby * MB,
            modified: 0,
            commit_used: 1000 * MB,
            private_ws: ws * MB,
            pages_input_per_sec: None,
            page_faults_per_sec: None,
        }
    }

    struct Fake(VecDeque<Option<RunMeasurement>>);

    impl MemorySampler for Fake {
        fn sample(&mut self, _pids: &[u32]) -> Option<RunMeasurement> {
            self.0.pop_front().flatten()
        }
    }

    fn settling() -> Settling {
        Settling::new(
            Trigger::Manual,
            2,
            vec![10, 11],
            RunMeasurements {
                before: reading(100, 100, 1000),
                after: reading(600, 100, 500),
                at_30s: None,
                at_120s: None,
            },
        )
    }

    #[test]
    fn intervals_follow_the_page() {
        assert_eq!(eval_interval(true), Duration::from_secs(1));
        assert_eq!(eval_interval(false), Duration::from_secs(10));
    }

    #[test]
    fn settling_waits_then_takes_30s_and_120s_readings() {
        let mut s = settling();
        let mut sampler = Fake(VecDeque::from([
            Some(reading(500, 100, 650)),
            Some(reading(450, 100, 700)),
        ]));
        assert_eq!(
            s.poll(Duration::from_secs(29), &mut sampler),
            SettleStep::Waiting
        );
        assert_eq!(
            s.poll(Duration::from_secs(31), &mut sampler),
            SettleStep::Took { last: false }
        );
        assert_eq!(
            s.poll(Duration::from_secs(60), &mut sampler),
            SettleStep::Waiting
        );
        assert_eq!(
            s.poll(Duration::from_secs(121), &mut sampler),
            SettleStep::Took { last: true }
        );
        let r = s.report();
        assert_eq!(r.moved_to_cache, 500 * MB);
        assert_eq!(r.came_back, 200 * MB);
        assert!(r.reverted());
    }

    #[test]
    fn a_failed_reading_gives_up_without_changing_the_report() {
        let mut s = settling();
        let before = s.report();
        let mut sampler = Fake(VecDeque::from([None]));
        assert_eq!(
            s.poll(Duration::from_secs(40), &mut sampler),
            SettleStep::Failed
        );
        assert_eq!(s.report().came_back, before.came_back);
    }

    #[test]
    fn candidates_use_activity_foreground_and_parents() {
        let procs = vec![
            proc(1, "chrome.exe", 800),
            proc(2, "slack.exe", 300),
            proc(3, "tiny.exe", 10),
            proc(4, "child.exe", 200),
            proc(5, "hidden.exe", 100),
        ];
        let parents: HashMap<u32, u32> = [(4, 2)].into();
        let mut activity = Activity::new(0);
        activity.note_foreground(100_000, Some("slack.exe"));
        let inp = Inputs {
            procs: &procs,
            parents: &parents,
            activity: &activity,
            now_ms: 700_000,
            foreground_pid: Some(1),
            fullscreen: true,
            system_idle: Some(Duration::from_secs(20)),
            min_bytes: 50 * MB,
        };
        let cands = candidates(
            &inp,
            &|pid, name| if pid == 5 { None } else { ident(pid, name) },
            &|pid| pid == 2,
        );
        let names: Vec<&str> = cands.iter().map(|c| c.identity.name.as_str()).collect();
        assert_eq!(names, vec!["chrome.exe", "slack.exe", "child.exe"]);

        let chrome = &cands[0];
        assert!(chrome.is_fullscreen, "the app in front is full screen");
        assert_eq!(chrome.idle_for, Some(Duration::from_secs(20)));
        let slack = &cands[1];
        assert!(!slack.is_fullscreen);
        assert!(slack.plays_audio);
        assert_eq!(slack.idle_for, Some(Duration::from_secs(600)));
        assert_eq!(cands[2].parent_name.as_deref(), Some("slack.exe"));
    }

    #[test]
    fn run_summary_wording_never_says_freed() {
        assert!(RunSummary::default()
            .text()
            .contains("Nothing to trim right now. Your idle apps are already small."));
        let s = RunSummary {
            lowered: 3,
            trimmed: 1,
            failed: 2,
        };
        let text = s.text();
        assert_eq!(
            text,
            "Lowered the memory priority of 3 apps. Asked 1 app to move unused memory to cache. \
             2 apps could not be changed."
        );
        assert!(!text.to_lowercase().contains("freed"));
    }

    #[test]
    fn manual_runs_use_the_looser_policy_and_automatic_runs_the_strict_one() {
        let manual = policy_for(Trigger::Manual, 1, None);
        let auto = policy_for(Trigger::Pressure, 1, None);
        assert_eq!(manual.min_idle, Duration::from_secs(60));
        assert_eq!(manual.min_trim_bytes, 100 * MB);
        assert_eq!(manual.min_priority_bytes, 25 * MB);
        assert_eq!(manual.trim_cooldown, Duration::from_secs(600));
        assert_eq!(auto.min_idle, Duration::from_secs(300));
        assert_eq!(auto.min_trim_bytes, 200 * MB);
        assert_eq!(auto.min_priority_bytes, 50 * MB);
        assert_eq!(auto.trim_cooldown, Duration::from_secs(45 * 60));
    }

    #[test]
    fn a_manual_run_finds_apps_the_automatic_one_skips() {
        let procs = vec![proc(1, "idle.exe", 150), proc(2, "small.exe", 40)];
        let parents = HashMap::new();
        let activity = Activity::new(0);
        // Both apps were last in front when the tool started: idle for 90 s.
        let inputs = |min_bytes| Inputs {
            procs: &procs,
            parents: &parents,
            activity: &activity,
            now_ms: 90_000,
            foreground_pid: None,
            fullscreen: false,
            system_idle: None,
            min_bytes,
        };
        let now = Instant::now();
        let plan_with = |p: &Policy| {
            let cands = candidates(
                &inputs(p.min_priority_bytes),
                &|pid, n| ident(pid, n),
                &|_| false,
            );
            actions::plan(&cands, p, &Cooldowns::new(), now)
        };
        let manual = plan_with(&policy_for(Trigger::Manual, 99, None));
        let auto = plan_with(&policy_for(Trigger::Pressure, 99, None));
        assert!(manual
            .iter()
            .any(|a| a.kind == Kind::Trim && a.identity.pid == 1));
        assert!(manual
            .iter()
            .any(|a| a.kind == Kind::LowerPriority && a.identity.pid == 2));
        assert!(auto.is_empty(), "{auto:?}");
    }

    #[test]
    fn summary_counts_results() {
        let id = Identity {
            pid: 1,
            created: 1,
            name: "a.exe".into(),
        };
        let mk = |kind, error| ActionResult {
            identity: id.clone(),
            kind,
            before: None,
            after: None,
            previous_priority: None,
            error,
        };
        let results = vec![
            mk(Kind::LowerPriority, None),
            mk(Kind::Trim, None),
            mk(Kind::Trim, Some(actions::ActionError::Failed)),
        ];
        assert_eq!(
            RunSummary::from_results(&results),
            RunSummary {
                lowered: 1,
                trimmed: 1,
                failed: 1
            }
        );
    }

    #[test]
    fn growth_text_covers_the_cases() {
        assert_eq!(per_hour_text(None), "collecting");
        assert_eq!(per_hour_text(Some(10_000)), "steady");
        assert_eq!(per_hour_text(Some(200 * MB as i64)), "+200 MB/h");
        assert_eq!(per_hour_text(Some(-(80 * MB as i64))), "-80 MB/h");
    }

    #[test]
    fn verdict_line_is_quiet_when_healthy() {
        let mut a = Assessment {
            verdict: Verdict::Healthy,
            reasons: Vec::new(),
        };
        assert_eq!(verdict_line(&a), None);
        a.verdict = Verdict::UnderPressure;
        a.reasons = vec![
            "Only 1.0 GB is ready to use".into(),
            "Windows reports that memory is low".into(),
        ];
        assert_eq!(
            verdict_line(&a).unwrap(),
            "Under pressure: Only 1.0 GB is ready to use; Windows reports that memory is low."
        );
        a.verdict = Verdict::Busy;
        a.reasons.clear();
        assert_eq!(verdict_line(&a).unwrap(), "Busy");
    }
}
