//! The Processes page: a compact, selectable process table with an inspector on the right.
//!
//! Sampling runs on its own worker thread (every 2 s, only while the page is on screen), and
//! ending or re-prioritising processes runs on a short worker thread, so the UI never blocks.
//! Everything here lives in memory; nothing is written to disk.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::System::Threading::{
    GetPriorityClass, OpenProcess, SetPriorityClass, TerminateProcess, BELOW_NORMAL_PRIORITY_CLASS,
    PROCESS_CREATION_FLAGS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_TERMINATE,
};

use crate::format::{friendly_name, size_plain, uptime};
use crate::ui::{CARD_PAD, GAP, RED};
use crate::win::Handle;

const ROW_HEIGHT: f32 = 22.0;
const INSPECTOR_WIDTH: f32 = 190.0;
const INSPECTOR_PAD: f32 = 8.0;
/// Time between two process refreshes while the page is visible.
const REFRESH: Duration = Duration::from_secs(2);
/// Wait before the second refresh that makes CPU percentages meaningful.
const PRIME: Duration = Duration::from_secs(1);
/// A ping older than this means the page is no longer shown.
const VISIBLE_WINDOW_MS: u64 = 2500;
/// Result lines shown in the inspector before "and N more".
const MAX_LINES: usize = 6;
const PROTECTED_NOTE: &str = "Windows needs this to run";

/// System and shell processes that are never ended or re-prioritised. Lowercase.
const PROTECTED: &[&str] = &[
    "system",
    "system idle process",
    "registry",
    "memory compression",
    "secure system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "toolboks.exe",
];

// ---------------------------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------------------------

/// One process as sampled.
#[derive(Clone, Debug, PartialEq)]
struct Proc {
    pid: u32,
    name: String,
    /// Percent of the whole machine.
    cpu: f32,
    /// Working set in bytes.
    mem: u64,
    run_secs: u64,
    exe: Option<String>,
}

/// One table row: a process, or all processes of one name when grouped.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    key: String,
    name: String,
    /// The pid, or the lowest pid of a group.
    pid: u32,
    pids: Vec<u32>,
    cpu: f32,
    mem: u64,
    run_secs: u64,
    exe: Option<String>,
    /// True when every process in the row is protected.
    protected: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortKey {
    Name,
    Pid,
    Cpu,
    Memory,
}

/// True for processes Windows needs, this app itself, and any that refused an action before.
fn is_protected(pid: u32, name: &str, own_pid: u32, denied: &HashSet<u32>) -> bool {
    pid == own_pid
        || pid == 0
        || pid == 4
        || denied.contains(&pid)
        || PROTECTED.contains(&name.to_lowercase().as_str())
}

/// Stable key of a row: lowercase name when grouped, "pid:<n>" when not.
fn row_key(grouped: bool, p: &Proc) -> String {
    if grouped {
        p.name.to_lowercase()
    } else {
        format!("pid:{}", p.pid)
    }
}

/// The processes to show: the frozen snapshot while paused, else the live list.
fn shown<'a>(live: &'a [Proc], frozen: Option<&'a [Proc]>) -> &'a [Proc] {
    frozen.unwrap_or(live)
}

/// Rows for the table; grouped rows sum CPU and memory of every process with the same name.
fn build_rows(procs: &[Proc], grouped: bool, protected: &dyn Fn(&Proc) -> bool) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for p in procs {
        let key = row_key(grouped, p);
        let is_prot = protected(p);
        match index.get(&key) {
            Some(&i) => {
                let r = &mut rows[i];
                r.pid = r.pid.min(p.pid);
                r.pids.push(p.pid);
                r.cpu += p.cpu;
                r.mem += p.mem;
                r.run_secs = r.run_secs.max(p.run_secs);
                if r.exe.is_none() {
                    r.exe = p.exe.clone();
                }
                r.protected &= is_prot;
            }
            None => {
                index.insert(key.clone(), rows.len());
                rows.push(Row {
                    key,
                    name: p.name.clone(),
                    pid: p.pid,
                    pids: vec![p.pid],
                    cpu: p.cpu,
                    mem: p.mem,
                    run_secs: p.run_secs,
                    exe: p.exe.clone(),
                    protected: is_prot,
                });
            }
        }
    }
    rows
}

/// Orders rows by `key`; ties fall back to name, then pid, whatever the direction.
fn sort_rows(rows: &mut [Row], key: SortKey, descending: bool) {
    rows.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Pid => a.pid.cmp(&b.pid),
            SortKey::Cpu => a.cpu.total_cmp(&b.cpu),
            SortKey::Memory => a.mem.cmp(&b.mem),
        };
        let ord = if descending { ord.reverse() } else { ord };
        ord.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then(a.pid.cmp(&b.pid))
    });
}

/// Keeps rows whose name or any pid contains `filter` (case-insensitive).
fn filter_rows(rows: Vec<Row>, filter: &str) -> Vec<Row> {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| {
            r.name.to_lowercase().contains(&needle)
                || r.pids.iter().any(|p| p.to_string().contains(&needle))
        })
        .collect()
}

/// Splits the selected processes into those that may be ended and those left alone.
fn plan_end(selected: &[Proc], protected: &dyn Fn(&Proc) -> bool) -> (Vec<Proc>, Vec<Proc>) {
    let (skipped, to_end): (Vec<Proc>, Vec<Proc>) =
        selected.iter().cloned().partition(|p| protected(p));
    (to_end, skipped)
}

/// The question asked before ending: `label` is an app name or a short description.
fn confirm_text(label: &str, count: usize) -> String {
    if count <= 1 {
        format!("End {label}? Unsaved work in this app will be lost.")
    } else {
        format!("End {count} processes of {label}? Unsaved work in them will be lost.")
    }
}

/// Plain text for the clipboard: one line per row.
fn copy_text(rows: &[&Row], ready: bool) -> String {
    rows.iter()
        .map(|r| {
            let cpu = if ready {
                format!("{:.1}%", r.cpu)
            } else {
                "...".to_string()
            };
            let id = if r.pids.len() > 1 {
                format!("{} processes", r.pids.len())
            } else {
                format!("pid {}", r.pid)
            };
            format!("{}, {}, CPU {}, {}", r.name, id, cpu, size_plain(r.mem))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cpu_text(cpu: f32, ready: bool) -> String {
    if ready {
        format!("{cpu:.1}%")
    } else {
        "...".to_string()
    }
}

fn sort_arrow(active: bool, descending: bool) -> &'static str {
    match (active, descending) {
        // Plain letters: the bundled font has none of the arrow or triangle glyphs.
        (true, true) => " v",
        (true, false) => " ^",
        _ => "",
    }
}

/// Name for the screen: "Chrome", with the count after it for a group.
fn display_name(r: &Row) -> String {
    let name = friendly_name(&r.name);
    if r.pids.len() > 1 {
        format!("{name} ({})", r.pids.len())
    } else {
        name
    }
}

/// The tiny selection model: rows remembered by key so they survive the refresh.
#[derive(Default)]
struct Selection {
    keys: HashSet<String>,
    anchor: Option<String>,
}

impl Selection {
    fn only(&mut self, key: &str) {
        self.keys.clear();
        self.keys.insert(key.to_string());
        self.anchor = Some(key.to_string());
    }

    fn toggle(&mut self, key: &str) {
        if !self.keys.remove(key) {
            self.keys.insert(key.to_string());
        }
        self.anchor = Some(key.to_string());
    }

    /// Every row from the anchor to `key` in the order shown; just `key` without an anchor.
    fn range(&mut self, key: &str, visible: &[String]) {
        let pos = |k: &str| visible.iter().position(|v| v == k);
        let (Some(to), Some(from)) = (pos(key), self.anchor.as_deref().and_then(pos)) else {
            self.only(key);
            return;
        };
        self.keys = visible[from.min(to)..=from.max(to)]
            .iter()
            .cloned()
            .collect();
    }

    fn prune(&mut self, existing: &HashSet<String>) {
        self.keys.retain(|k| existing.contains(k));
        if self.anchor.as_ref().is_some_and(|a| !existing.contains(a)) {
            self.anchor = None;
        }
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.anchor = None;
    }
}

/// One result line for the inspector.
fn result_line(done: &str, name: &str, pid: u32, result: Result<(), ActionError>) -> String {
    match result {
        Ok(()) => format!("{done} {name} (pid {pid})"),
        Err(ActionError::AccessDenied) => format!("{name} (pid {pid}): Access denied"),
        Err(ActionError::Failed) => {
            format!("{name} (pid {pid}): Could not be changed, it may have closed already")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windows actions
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionError {
    AccessDenied,
    Failed,
}

fn classify(e: &windows::core::Error) -> ActionError {
    if e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        ActionError::AccessDenied
    } else {
        ActionError::Failed
    }
}

fn end_process(pid: u32) -> Result<(), ActionError> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }
        .map(Handle)
        .map_err(|e| classify(&e))?;
    // SAFETY: the handle is valid and was opened with PROCESS_TERMINATE.
    unsafe { TerminateProcess(handle.0, 1) }.map_err(|e| classify(&e))
}

/// Sets the priority class and returns the previous one.
fn set_priority(pid: u32, class: u32) -> Result<u32, ActionError> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    }
    .map(Handle)
    .map_err(|e| classify(&e))?;
    // SAFETY: the handle is valid and was opened with the needed access.
    let previous = unsafe { GetPriorityClass(handle.0) };
    if previous == 0 {
        return Err(ActionError::Failed);
    }
    // SAFETY: as above.
    unsafe { SetPriorityClass(handle.0, PROCESS_CREATION_FLAGS(class)) }
        .map_err(|e| classify(&e))?;
    Ok(previous)
}

/// Opens Explorer with the file selected.
fn open_location(path: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

enum Action {
    End(Vec<Proc>),
    Lower(Vec<Proc>),
    /// Process and the class it had before.
    Restore(Vec<(Proc, u32)>),
}

#[derive(Default)]
struct Done {
    lines: Vec<String>,
    /// Pids that refused: treated as protected from now on.
    denied: Vec<u32>,
    /// (pid, class before) for each process just lowered.
    lowered: Vec<(u32, u32)>,
    restored: Vec<u32>,
}

fn run_action(action: Action) -> Done {
    let mut done = Done::default();
    let note = |done: &mut Done, p: &Proc, ok: &str, r: Result<(), ActionError>| {
        if r == Err(ActionError::AccessDenied) {
            done.denied.push(p.pid);
        }
        done.lines.push(result_line(ok, &p.name, p.pid, r));
    };
    match action {
        Action::End(list) => {
            for p in &list {
                note(&mut done, p, "Ended", end_process(p.pid));
            }
        }
        Action::Lower(list) => {
            for p in &list {
                match set_priority(p.pid, BELOW_NORMAL_PRIORITY_CLASS.0) {
                    Ok(prev) => {
                        if prev != BELOW_NORMAL_PRIORITY_CLASS.0 {
                            done.lowered.push((p.pid, prev));
                        }
                        note(&mut done, p, "Lowered priority of", Ok(()));
                    }
                    Err(e) => note(&mut done, p, "", Err(e)),
                }
            }
        }
        Action::Restore(list) => {
            for (p, prev) in &list {
                let r = set_priority(p.pid, *prev).map(|_| ());
                if r.is_ok() {
                    done.restored.push(p.pid);
                }
                note(&mut done, p, "Restored priority of", r);
            }
        }
    }
    done
}

// ---------------------------------------------------------------------------------------------
// Sampling worker
// ---------------------------------------------------------------------------------------------

struct Snapshot {
    procs: Vec<Proc>,
    /// False until the second refresh, when CPU percentages become real.
    cpu_ready: bool,
}

struct Worker {
    rx: Receiver<Snapshot>,
    last_ping: Arc<AtomicU64>,
    epoch: Instant,
}

fn visible(last_ping_plus_one: u64, now_ms: u64) -> bool {
    last_ping_plus_one != 0 && (now_ms + 1).saturating_sub(last_ping_plus_one) < VISIBLE_WINDOW_MS
}

impl Worker {
    fn start(ctx: &egui::Context) -> Worker {
        let (tx, rx) = mpsc::channel();
        let last_ping = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        let ping = Arc::clone(&last_ping);
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("process-sampler".into())
            .spawn(move || {
                let Ok(mut sys) = System::new() else {
                    return;
                };
                let logical = std::thread::available_parallelism().map_or(1, |n| n.get()) as f32;
                let kind = ProcessRefreshKind::nothing()
                    .with_cpu()
                    .with_memory()
                    .with_exe(UpdateKind::OnlyIfNotSet);
                let mut last: Option<Instant> = None;
                let mut ready = false;
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let now_ms = epoch.elapsed().as_millis() as u64;
                    if !visible(ping.load(Ordering::Relaxed), now_ms) {
                        // The CPU baseline goes stale while hidden.
                        last = None;
                        ready = false;
                        continue;
                    }
                    let wait = if ready { REFRESH } else { PRIME };
                    if last.is_some_and(|t| t.elapsed() < wait) {
                        continue;
                    }
                    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
                    let was_primed = last.is_some();
                    last = Some(Instant::now());
                    let procs = sys
                        .processes()
                        .values()
                        .map(|p| Proc {
                            pid: p.pid().as_u32(),
                            name: p.name().to_string_lossy().into_owned(),
                            cpu: (p.cpu_usage() / logical).clamp(0.0, 100.0),
                            mem: p.memory(),
                            run_secs: p.run_time(),
                            exe: p.exe().map(|e| e.display().to_string()),
                        })
                        .collect();
                    ready = was_primed;
                    if tx
                        .send(Snapshot {
                            procs,
                            cpu_ready: ready,
                        })
                        .is_err()
                    {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        // If the thread cannot start, the channel just stays empty.
        let _ = spawned;
        Worker {
            rx,
            last_ping,
            epoch,
        }
    }

    fn ping(&self) {
        let now_ms = self.epoch.elapsed().as_millis() as u64;
        self.last_ping.store(now_ms + 1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------------------------

/// A click on the table, applied once the table is drawn.
enum Click {
    Row(String, egui::Modifiers),
    Box(String),
}

/// What the inspector asked for this frame.
enum Intent {
    AskEnd(Vec<u32>),
    ConfirmEnd,
    CancelEnd,
    Lower,
    Restore,
    Copy(String),
    Open(String),
}

pub(super) struct Page {
    worker: Option<Worker>,
    live: Vec<Proc>,
    cpu_ready: bool,
    frozen: Option<Vec<Proc>>,
    filter: String,
    grouped: bool,
    was_grouped: bool,
    sort_key: SortKey,
    descending: bool,
    selection: Selection,
    /// Pids waiting for the second click on End task.
    confirm: Option<Vec<u32>>,
    /// (pid, class before) for processes lowered in this session.
    lowered: Vec<(u32, u32)>,
    denied: HashSet<u32>,
    job: Option<Receiver<Done>>,
    lines: Vec<String>,
}

impl Page {
    pub(super) fn new() -> Self {
        Self {
            worker: None,
            live: Vec::new(),
            cpu_ready: false,
            frozen: None,
            filter: String::new(),
            grouped: true,
            was_grouped: true,
            sort_key: SortKey::Cpu,
            descending: true,
            selection: Selection::default(),
            confirm: None,
            lowered: Vec::new(),
            denied: HashSet::new(),
            job: None,
            lines: Vec::new(),
        }
    }

    /// Takes in whatever the workers have produced. Never blocks.
    pub(super) fn tick(&mut self) {
        if let Some(worker) = &self.worker {
            for snap in worker.rx.try_iter() {
                self.live = snap.procs;
                self.cpu_ready = snap.cpu_ready;
            }
            // A lowered process that has exited needs no restore entry.
            let pids: HashSet<u32> = self.live.iter().map(|p| p.pid).collect();
            if !pids.is_empty() {
                self.lowered.retain(|(pid, _)| pids.contains(pid));
            }
        }
        let finished = self.job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(done) = finished {
            self.job = None;
            self.denied.extend(done.denied);
            self.lowered.extend(done.lowered);
            self.lowered.retain(|(pid, _)| !done.restored.contains(pid));
            self.lines = done.lines;
        }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui) {
        let worker = self.worker.get_or_insert_with(|| Worker::start(ui.ctx()));
        worker.ping();

        if self.was_grouped != self.grouped {
            self.was_grouped = self.grouped;
            self.selection.clear();
            self.confirm = None;
        }
        let own_pid = std::process::id();
        let grouped = self.grouped;
        let denied = self.denied.clone();
        let protected = move |p: &Proc| is_protected(p.pid, &p.name, own_pid, &denied);
        let procs: Vec<Proc> = shown(&self.live, self.frozen.as_deref()).to_vec();
        let all_rows = build_rows(&procs, grouped, &protected);
        let existing: HashSet<String> = all_rows.iter().map(|r| r.key.clone()).collect();
        self.selection.prune(&existing);

        let inspector = egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(INSPECTOR_PAD);
        let mut intent: Option<Intent> = None;
        let panel = egui::Panel::right("processes_inspector")
            .resizable(false)
            .exact_size(INSPECTOR_WIDTH)
            .frame(inspector)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        intent = self.inspector(ui, &procs, &all_rows, &protected);
                    });
            });
        let rect = panel.response.rect;
        ui.painter().vline(
            rect.left(),
            rect.y_range(),
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        let side = egui::Frame::NONE.inner_margin(CARD_PAD);
        egui::CentralPanel::default().frame(side).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = Vec2::new(6.0, 1.0);
            let rows = self.toolbar(ui, all_rows.clone());
            ui.add_space(2.0);
            self.table(ui, rows);
        });

        if let Some(intent) = intent {
            self.apply(intent, ui.ctx(), &procs, &all_rows, &protected);
        }
    }

    fn selected_procs(&self, procs: &[Proc]) -> Vec<Proc> {
        procs
            .iter()
            .filter(|p| self.selection.keys.contains(&row_key(self.grouped, p)))
            .cloned()
            .collect()
    }

    fn apply(
        &mut self,
        intent: Intent,
        ctx: &egui::Context,
        procs: &[Proc],
        rows: &[Row],
        protected: &dyn Fn(&Proc) -> bool,
    ) {
        match intent {
            Intent::AskEnd(pids) => self.confirm = Some(pids),
            Intent::CancelEnd => self.confirm = None,
            Intent::ConfirmEnd => {
                self.confirm = None;
                let (to_end, _) = plan_end(&self.selected_procs(procs), protected);
                self.start_job(ctx, Action::End(to_end));
            }
            Intent::Lower => {
                let (list, _) = plan_end(&self.selected_procs(procs), protected);
                self.start_job(ctx, Action::Lower(list));
            }
            Intent::Restore => {
                let list: Vec<(Proc, u32)> = self
                    .selected_procs(procs)
                    .into_iter()
                    .filter_map(|p| {
                        let prev = self.lowered.iter().find(|(pid, _)| *pid == p.pid)?.1;
                        Some((p, prev))
                    })
                    .collect();
                self.start_job(ctx, Action::Restore(list));
            }
            Intent::Copy(text) => ctx.copy_text(text),
            Intent::Open(path) => open_location(&path),
        }
        let _ = rows;
    }

    fn start_job(&mut self, ctx: &egui::Context, action: Action) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("process-action".into())
            .spawn(move || {
                let _ = tx.send(run_action(action));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.job = Some(rx);
            self.lines.clear();
        } else {
            self.lines = vec!["Could not start the action".to_string()];
        }
    }

    /// Filter, group, pause and count. Returns the rows to show, filtered and sorted.
    fn toolbar(&mut self, ui: &mut egui::Ui, rows: Vec<Row>) -> Vec<Row> {
        let mut paused = self.frozen.is_some();
        let was_paused = paused;
        let procs_total: usize = rows.iter().map(|r| r.pids.len()).sum();
        let mut rows = filter_rows(rows, &self.filter);
        sort_rows(&mut rows, self.sort_key, self.descending);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by name or pid")
                    .desired_width(150.0),
            );
            ui.checkbox(&mut self.grouped, "Group by name");
            ui.toggle_value(&mut paused, if was_paused { "Paused" } else { "Pause" })
                .on_hover_text("Freeze the list so rows stop moving. Sampling carries on.");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if self.grouped {
                    format!("{} apps, {procs_total} processes", rows.len())
                } else {
                    format!("{} processes", rows.len())
                };
                ui.label(RichText::new(text).weak());
            });
        });
        if paused != was_paused {
            self.frozen = paused.then(|| self.live.clone());
        }
        rows
    }

    fn table(&mut self, ui: &mut egui::Ui, rows: Vec<Row>) {
        let mut sort_click: Option<SortKey> = None;
        let mut click: Option<Click> = None;
        let (sort_key, descending, grouped) = (self.sort_key, self.descending, self.grouped);
        let selection = &self.selection;
        let ready = self.cpu_ready;
        let modifiers = ui.input(|i| i.modifiers);

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .column(Column::exact(22.0))
            .column(Column::remainder().at_least(110.0).clip(true));
        if !grouped {
            table = table.column(Column::exact(56.0));
        }
        table
            .column(Column::exact(64.0))
            .column(Column::exact(84.0))
            .header(ROW_HEIGHT, |mut header| {
                header.col(|_| {});
                let sortable = |ui: &mut egui::Ui, label: &str, key: SortKey| {
                    let arrow = sort_arrow(sort_key == key, descending);
                    ui.add(
                        egui::Button::new(
                            RichText::new(format!("{label}{arrow}")).small().strong(),
                        )
                        .frame(false),
                    )
                    .clicked()
                    .then_some(key)
                };
                header.col(|ui| sort_click = sortable(ui, "Name", SortKey::Name).or(sort_click));
                if !grouped {
                    header.col(|ui| sort_click = sortable(ui, "PID", SortKey::Pid).or(sort_click));
                }
                header.col(|ui| sort_click = sortable(ui, "CPU", SortKey::Cpu).or(sort_click));
                header
                    .col(|ui| sort_click = sortable(ui, "Memory", SortKey::Memory).or(sort_click));
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let r = &rows[row.index()];
                    let is_selected = selection.keys.contains(&r.key);
                    row.set_selected(is_selected);
                    let cell = |ui: &mut egui::Ui, text: String| {
                        let mut t = RichText::new(text);
                        if r.protected {
                            t = t.weak();
                        }
                        let l = ui.add(egui::Label::new(t).selectable(false));
                        if r.protected {
                            l.on_hover_text(PROTECTED_NOTE);
                        }
                    };
                    row.col(|ui| {
                        let mut on = is_selected;
                        if ui.checkbox(&mut on, "").changed() {
                            click = Some(Click::Box(r.key.clone()));
                        }
                    });
                    row.col(|ui| {
                        let mut t = RichText::new(display_name(r));
                        if r.protected {
                            t = t.weak();
                        }
                        let l = ui.add(egui::Label::new(t).selectable(false).truncate());
                        if r.protected {
                            l.on_hover_text(PROTECTED_NOTE);
                        }
                    });
                    if !grouped {
                        row.col(|ui| cell(ui, r.pid.to_string()));
                    }
                    row.col(|ui| cell(ui, cpu_text(r.cpu, ready)));
                    row.col(|ui| cell(ui, size_plain(r.mem)));
                    if row.response().clicked() && click.is_none() {
                        click = Some(Click::Row(r.key.clone(), modifiers));
                    }
                });
            });

        let visible: Vec<String> = match click {
            Some(Click::Row(..)) if modifiers.shift => rows.iter().map(|r| r.key.clone()).collect(),
            _ => Vec::new(),
        };
        match click {
            Some(Click::Box(key)) => self.selection.toggle(&key),
            Some(Click::Row(key, m)) if m.shift => self.selection.range(&key, &visible),
            Some(Click::Row(key, m)) if m.command || m.ctrl => self.selection.toggle(&key),
            Some(Click::Row(key, _)) => self.selection.only(&key),
            None => {}
        }
        if let Some(key) = sort_click {
            if self.sort_key == key {
                self.descending = !self.descending;
            } else {
                self.sort_key = key;
                // Names and pids read naturally upwards, numbers biggest first.
                self.descending = matches!(key, SortKey::Cpu | SortKey::Memory);
            }
        }
    }

    fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        procs: &[Proc],
        all_rows: &[Row],
        protected: &dyn Fn(&Proc) -> bool,
    ) -> Option<Intent> {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        let selected: Vec<&Row> = all_rows
            .iter()
            .filter(|r| self.selection.keys.contains(&r.key))
            .collect();
        if selected.is_empty() {
            ui.label(RichText::new("Nothing selected.").weak());
            ui.label(
                RichText::new(
                    "Click a row. Ctrl+click adds or removes one, Shift+click selects a range.",
                )
                .weak()
                .small(),
            );
            self.result_lines(ui);
            return None;
        }

        let ready = self.cpu_ready;
        let title = match selected.as_slice() {
            [one] => display_name(one),
            many => format!("{} selected", many.len()),
        };
        ui.label(RichText::new(&title).strong());
        let proc_count: usize = selected.iter().map(|r| r.pids.len()).sum();
        let cpu: f32 = selected.iter().map(|r| r.cpu).sum();
        let mem: u64 = selected.iter().map(|r| r.mem).sum();
        if let [one] = selected.as_slice() {
            kv(ui, "Name", &one.name);
            if one.pids.len() > 1 {
                kv(ui, "Processes", &one.pids.len().to_string());
            } else {
                kv(ui, "PID", &one.pid.to_string());
            }
        } else {
            kv(ui, "Processes", &proc_count.to_string());
        }
        kv(ui, "CPU", &cpu_text(cpu, ready));
        kv(ui, "Memory", &size_plain(mem));
        if let [one] = selected.as_slice() {
            kv(ui, "Running for", &uptime(one.run_secs));
            if let Some(path) = &one.exe {
                kv(ui, "Path", path);
            }
        }

        let chosen = self.selected_procs(procs);
        let (to_end, skipped) = plan_end(&chosen, protected);
        if to_end.is_empty() {
            ui.label(RichText::new(PROTECTED_NOTE).weak().small());
        } else if !skipped.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} of {} are protected and will be left alone.",
                    skipped.len(),
                    chosen.len()
                ))
                .weak()
                .small(),
            );
        }
        ui.separator();

        let busy = self.job.is_some();
        let mut intent = None;
        let end_pids: Vec<u32> = {
            let mut v: Vec<u32> = to_end.iter().map(|p| p.pid).collect();
            v.sort_unstable();
            v
        };
        if self.confirm.as_ref().is_some_and(|c| *c != end_pids) {
            self.confirm = None;
        }
        if self.confirm.is_some() && !end_pids.is_empty() {
            let label = match selected.as_slice() {
                [one] => display_name(one)
                    .split(" (")
                    .next()
                    .unwrap_or("")
                    .to_string(),
                many => format!("{} apps", many.len()),
            };
            ui.label(RichText::new(confirm_text(&label, end_pids.len())).small());
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(RichText::new("End").color(RED)))
                    .clicked()
                {
                    intent = Some(Intent::ConfirmEnd);
                }
                if ui.button("Cancel").clicked() {
                    intent = Some(Intent::CancelEnd);
                }
            });
        } else if ui
            .add_enabled(!end_pids.is_empty() && !busy, egui::Button::new("End task"))
            .clicked()
        {
            intent = Some(Intent::AskEnd(end_pids.clone()));
        }

        let can_restore = chosen
            .iter()
            .any(|p| self.lowered.iter().any(|(pid, _)| *pid == p.pid));
        if can_restore {
            if ui
                .add_enabled(!busy, egui::Button::new("Restore priority"))
                .clicked()
            {
                intent = Some(Intent::Restore);
            }
        } else if ui
            .add_enabled(
                !to_end.is_empty() && !busy,
                egui::Button::new("Lower priority"),
            )
            .on_hover_text("Lets other apps use the processor first. Undo it here any time.")
            .clicked()
        {
            intent = Some(Intent::Lower);
        }
        if ui.button("Copy").clicked() {
            intent = Some(Intent::Copy(copy_text(&selected, ready)));
        }
        if let [one] = selected.as_slice() {
            if let Some(path) = &one.exe {
                if ui.button("Open file location").clicked() {
                    intent = Some(Intent::Open(path.clone()));
                }
            }
        }
        if busy {
            ui.label(RichText::new("Working...").weak().small());
        }
        ui.add_space(GAP - 2.0);
        self.result_lines(ui);
        intent
    }

    fn result_lines(&self, ui: &mut egui::Ui) {
        for line in self.lines.iter().take(MAX_LINES) {
            ui.label(RichText::new(line).small());
        }
        if self.lines.len() > MAX_LINES {
            ui.label(
                RichText::new(format!("and {} more", self.lines.len() - MAX_LINES))
                    .weak()
                    .small(),
            );
        }
    }
}

/// A small caption over a value.
fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.label(RichText::new(key).small().weak());
    ui.add(egui::Label::new(value).wrap());
    ui.spacing_mut().item_spacing.y = 4.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, cpu: f32, mem: u64) -> Proc {
        Proc {
            pid,
            name: name.to_string(),
            cpu,
            mem,
            run_secs: pid as u64,
            exe: None,
        }
    }

    fn none(_: &Proc) -> bool {
        false
    }

    fn sample() -> Vec<Proc> {
        vec![
            p(10, "chrome.exe", 1.0, 100),
            p(11, "Chrome.exe", 2.0, 200),
            p(20, "code.exe", 5.0, 50),
            p(4, "System", 0.5, 10),
        ]
    }

    #[test]
    fn grouping_sums_and_counts() {
        let rows = build_rows(&sample(), true, &none);
        assert_eq!(rows.len(), 3);
        let chrome = &rows[0];
        assert_eq!(chrome.pids, vec![10, 11]);
        assert_eq!(chrome.pid, 10);
        assert_eq!(chrome.mem, 300);
        assert!((chrome.cpu - 3.0).abs() < 1e-6);
        assert_eq!(display_name(chrome), "Chrome (2)");
        assert_eq!(build_rows(&sample(), false, &none).len(), 4);
    }

    #[test]
    fn grouped_row_is_protected_only_when_all_are() {
        let some = |q: &Proc| q.pid == 10;
        assert!(!build_rows(&sample(), true, &some)[0].protected);
        let all = |q: &Proc| q.name.to_lowercase() == "chrome.exe";
        assert!(build_rows(&sample(), true, &all)[0].protected);
    }

    #[test]
    fn sorts_by_every_column_with_name_ties() {
        let mut rows = build_rows(&sample(), false, &none);
        sort_rows(&mut rows, SortKey::Memory, true);
        let order: Vec<u32> = rows.iter().map(|r| r.pid).collect();
        assert_eq!(order, vec![11, 10, 20, 4]);
        sort_rows(&mut rows, SortKey::Cpu, false);
        assert_eq!(rows[0].pid, 4);
        sort_rows(&mut rows, SortKey::Pid, false);
        assert_eq!(rows[0].pid, 4);
        sort_rows(&mut rows, SortKey::Name, false);
        assert_eq!(rows[0].name, "chrome.exe");
        assert_eq!(rows[0].pid, 10);

        let ties = vec![
            p(3, "b.exe", 1.0, 5),
            p(2, "a.exe", 1.0, 5),
            p(1, "a.exe", 1.0, 5),
        ];
        let mut rows = build_rows(&ties, false, &none);
        sort_rows(&mut rows, SortKey::Cpu, true);
        let order: Vec<u32> = rows.iter().map(|r| r.pid).collect();
        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn filter_matches_name_or_pid() {
        let rows = build_rows(&sample(), false, &none);
        assert_eq!(filter_rows(rows.clone(), "CHR").len(), 2);
        assert_eq!(filter_rows(rows.clone(), "20").len(), 1);
        assert_eq!(filter_rows(rows.clone(), "  ").len(), 4);
        assert!(filter_rows(rows, "zzz").is_empty());
        let grouped = build_rows(&sample(), true, &none);
        assert_eq!(filter_rows(grouped, "11").len(), 1);
    }

    #[test]
    fn paused_view_uses_the_frozen_snapshot() {
        let live = vec![p(1, "a.exe", 9.0, 9)];
        let frozen = vec![p(1, "a.exe", 1.0, 1), p(2, "b.exe", 2.0, 2)];
        assert_eq!(shown(&live, None).len(), 1);
        assert_eq!(shown(&live, Some(&frozen)).len(), 2);
        assert!((shown(&live, Some(&frozen))[0].cpu - 1.0).abs() < 1e-6);
    }

    #[test]
    fn protected_names_and_pids() {
        let denied: HashSet<u32> = [777].into();
        for name in [
            "System",
            "Registry",
            "SVCHOST.EXE",
            "Memory Compression",
            "dwm.exe",
        ] {
            assert!(is_protected(500, name, 1, &denied), "{name}");
        }
        assert!(is_protected(0, "anything", 1, &denied));
        assert!(is_protected(1234, "whatever.exe", 1234, &denied));
        assert!(is_protected(777, "refused.exe", 1, &denied));
        assert!(!is_protected(500, "chrome.exe", 1, &denied));
    }

    #[test]
    fn plan_end_splits_protected() {
        let sel = sample();
        let prot = |q: &Proc| q.pid == 4;
        let (to_end, skipped) = plan_end(&sel, &prot);
        assert_eq!(to_end.len(), 3);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].pid, 4);
        let (to_end, skipped) = plan_end(&[], &prot);
        assert!(to_end.is_empty() && skipped.is_empty());
    }

    #[test]
    fn confirm_wording() {
        assert_eq!(
            confirm_text("Notepad", 1),
            "End Notepad? Unsaved work in this app will be lost."
        );
        assert_eq!(
            confirm_text("Chrome", 14),
            "End 14 processes of Chrome? Unsaved work in them will be lost."
        );
    }

    #[test]
    fn copy_text_lines() {
        let rows = build_rows(&sample(), true, &none);
        let refs: Vec<&Row> = rows.iter().take(2).collect();
        let text = copy_text(&refs, true);
        assert_eq!(
            text,
            "chrome.exe, 2 processes, CPU 3.0%, 0 MB\ncode.exe, pid 20, CPU 5.0%, 0 MB"
        );
        assert!(copy_text(&refs, false).contains("CPU ..."));
    }

    #[test]
    fn result_lines_are_plain() {
        assert_eq!(
            result_line("Ended", "a.exe", 5, Ok(())),
            "Ended a.exe (pid 5)"
        );
        assert_eq!(
            result_line("Ended", "a.exe", 5, Err(ActionError::AccessDenied)),
            "a.exe (pid 5): Access denied"
        );
    }

    #[test]
    fn selection_click_ctrl_shift() {
        let keys: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let mut s = Selection::default();
        s.only("b");
        s.toggle("d");
        assert_eq!(s.keys.len(), 2);
        s.toggle("d");
        assert!(!s.keys.contains("d"));
        s.only("b");
        s.range("d", &keys);
        assert_eq!(s.keys.len(), 3);
        assert!(s.keys.contains("c"));
        let existing: HashSet<String> = ["b".to_string()].into();
        s.prune(&existing);
        assert_eq!(s.keys.len(), 1);
        let mut fresh = Selection::default();
        fresh.range("c", &keys);
        assert_eq!(fresh.keys.len(), 1);
    }

    #[test]
    fn sort_arrows_use_letters() {
        assert_eq!(sort_arrow(true, true), " v");
        assert_eq!(sort_arrow(true, false), " ^");
        assert_eq!(sort_arrow(false, true), "");
    }

    #[test]
    fn visibility_window() {
        assert!(!visible(0, 100));
        assert!(visible(1001, 1500));
        assert!(!visible(1001, 1000 + VISIBLE_WINDOW_MS));
    }
}
