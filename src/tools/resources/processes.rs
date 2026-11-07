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

use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};
use egui_extras::{Column, TableBuilder};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows::core::{HRESULT, PWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, FILETIME, HANDLE};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass,
    TerminateProcess, BELOW_NORMAL_PRIORITY_CLASS, PROCESS_ACCESS_RIGHTS, PROCESS_CREATION_FLAGS,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_TERMINATE,
};

use crate::format::{friendly_name, size_plain, uptime};
use crate::ui::{
    body_cell, header_cell, inspector_edge, inspector_frame, row_rule, switch_row, table_style,
    CARD_PAD, GAP, RED, SPACE_M, TEXT_SECTION,
};
use crate::win::Handle;

pub(super) const ROW_HEIGHT: f32 = 28.0;
pub(super) const INSPECTOR_WIDTH: f32 = 240.0;
/// Time between two process refreshes while the page is visible.
pub(super) const REFRESH: Duration = Duration::from_secs(2);
/// Wait before the second refresh that makes CPU percentages meaningful.
const PRIME: Duration = Duration::from_secs(1);
/// A ping older than this means the page is no longer shown.
pub(super) const VISIBLE_WINDOW_MS: u64 = 2500;
/// Result lines shown in the inspector before "and N more".
const MAX_LINES: usize = 6;
pub(super) const PROTECTED_NOTE: &str = "Windows needs this to run";

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
    identity: Option<ProcessIdentity>,
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
pub(super) fn is_protected(pid: u32, name: &str, own_pid: u32, denied: &HashSet<u32>) -> bool {
    pid == own_pid
        || pid == 0
        || pid == 4
        || denied.contains(&pid)
        || PROTECTED.contains(&name.to_lowercase().as_str())
}

/// Stable key of a row: lowercase name when grouped, process identity when not.
fn row_key(grouped: bool, p: &Proc) -> String {
    if grouped {
        p.name.to_lowercase()
    } else {
        format!("pid:{}:{}", p.pid, p.identity.map_or(0, |id| id.created))
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
    let (skipped, to_end): (Vec<Proc>, Vec<Proc>) = selected
        .iter()
        .cloned()
        .partition(|p| p.identity.is_none() || protected(p));
    (to_end, skipped)
}

fn action_identities(processes: &[Proc]) -> Vec<ProcessIdentity> {
    let mut identities: Vec<_> = processes.iter().filter_map(|p| p.identity).collect();
    identities.sort_unstable();
    identities
}

/// The question asked before ending: `label` is an app name or a short description.
pub(super) fn confirm_text(label: &str, count: usize) -> String {
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

pub(super) fn sort_arrow(active: bool, descending: bool) -> &'static str {
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
pub(super) fn result_line(
    done: &str,
    name: &str,
    pid: u32,
    result: Result<(), ActionError>,
) -> String {
    match result {
        Ok(()) => format!("{done} {name} (pid {pid})"),
        Err(ActionError::AccessDenied) => format!("{name} (pid {pid}): Access denied"),
        Err(ActionError::IdentityChanged) => {
            format!("{name} (pid {pid}): Windows could not verify the selected process")
        }
        Err(ActionError::Failed) => {
            format!("{name} (pid {pid}): Could not be changed, it may have closed already")
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windows actions
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionError {
    AccessDenied,
    IdentityChanged,
    Failed,
}

fn classify(e: &windows::core::Error) -> ActionError {
    if e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        ActionError::AccessDenied
    } else {
        ActionError::Failed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct ProcessIdentity {
    pub(super) pid: u32,
    pub(super) created: u64,
}

fn filetime(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

pub(super) fn snapshot_start() -> u64 {
    // SAFETY: this call reads the system time and takes no arguments.
    filetime(unsafe { GetSystemTimePreciseAsFileTime() })
}

fn creation_time(handle: HANDLE) -> windows::core::Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: the handle has query access and all four outputs are valid FILETIME buffers.
    unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }?;
    Ok(filetime(created))
}

pub(super) fn sampled_identity(
    process: &sysinfo::Process,
    sample_start: u64,
) -> Option<ProcessIdentity> {
    let pid = process.pid().as_u32();
    // SAFETY: Windows validates the PID. Only query access is requested.
    let handle =
        Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let created = creation_time(handle.0).ok()?;
    let unix_secs = (created / 10_000_000).checked_sub(11_644_473_600)?;
    // Reject processes born during the sample, including a reused connection-owner PID.
    (created < sample_start && unix_secs == process.start_time())
        .then_some(ProcessIdentity { pid, created })
}

fn action_handle(
    identity: ProcessIdentity,
    expected_name: &str,
    access: PROCESS_ACCESS_RIGHTS,
) -> Result<Handle, ActionError> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe {
        OpenProcess(
            access | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            identity.pid,
        )
    }
    .map(Handle)
    .map_err(|e| classify(&e))?;
    if creation_time(handle.0).ok() != Some(identity.created) {
        return Err(ActionError::IdentityChanged);
    }

    let mut image = vec![0u16; 32_768];
    let mut len = image.len() as u32;
    // SAFETY: the handle has query access and the output buffer holds `len` UTF-16 units.
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(image.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|_| ActionError::IdentityChanged)?;
    let image = String::from_utf16_lossy(&image[..len as usize]);
    let name = image.rsplit('\\').next().unwrap_or_default();
    if !name.eq_ignore_ascii_case(expected_name)
        || is_protected(identity.pid, name, std::process::id(), &HashSet::new())
    {
        return Err(ActionError::IdentityChanged);
    }
    Ok(handle)
}

pub(super) fn end_process(identity: ProcessIdentity, name: &str) -> Result<(), ActionError> {
    let handle = action_handle(identity, name, PROCESS_TERMINATE)?;
    // SAFETY: the handle is valid and was opened with PROCESS_TERMINATE.
    unsafe { TerminateProcess(handle.0, 1) }.map_err(|e| classify(&e))
}

/// Sets the priority class and returns the previous one.
fn set_priority(identity: ProcessIdentity, name: &str, class: u32) -> Result<u32, ActionError> {
    let handle = action_handle(identity, name, PROCESS_SET_INFORMATION)?;
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
    lowered: Vec<(ProcessIdentity, u32)>,
    restored: Vec<ProcessIdentity>,
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
                let result = p
                    .identity
                    .ok_or(ActionError::IdentityChanged)
                    .and_then(|id| end_process(id, &p.name));
                note(&mut done, p, "Ended", result);
            }
        }
        Action::Lower(list) => {
            for p in &list {
                let result = p
                    .identity
                    .ok_or(ActionError::IdentityChanged)
                    .and_then(|id| {
                        set_priority(id, &p.name, BELOW_NORMAL_PRIORITY_CLASS.0)
                            .map(|prev| (id, prev))
                    });
                match result {
                    Ok((id, prev)) => {
                        if prev != BELOW_NORMAL_PRIORITY_CLASS.0 {
                            done.lowered.push((id, prev));
                        }
                        note(&mut done, p, "Lowered priority of", Ok(()));
                    }
                    Err(e) => note(&mut done, p, "", Err(e)),
                }
            }
        }
        Action::Restore(list) => {
            for (p, prev) in &list {
                let r = p
                    .identity
                    .ok_or(ActionError::IdentityChanged)
                    .and_then(|id| {
                        set_priority(id, &p.name, *prev)?;
                        done.restored.push(id);
                        Ok(())
                    });
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

pub(super) fn visible(last_ping_plus_one: u64, now_ms: u64) -> bool {
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
                    if Arc::strong_count(&ping) == 1 {
                        return;
                    }
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
                    let sample_start = snapshot_start();
                    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
                    let was_primed = last.is_some();
                    last = Some(Instant::now());
                    let procs = sys
                        .processes()
                        .values()
                        .map(|p| Proc {
                            pid: p.pid().as_u32(),
                            identity: sampled_identity(p, sample_start),
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
    AskEnd(Vec<ProcessIdentity>),
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
    /// Process identities waiting for the second click on End task.
    confirm: Option<Vec<ProcessIdentity>>,
    /// Previous priority class keyed by process identity.
    lowered: Vec<(ProcessIdentity, u32)>,
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
            let identities: HashSet<_> = self.live.iter().filter_map(|p| p.identity).collect();
            if !identities.is_empty() {
                self.lowered.retain(|(id, _)| identities.contains(id));
            }
        }
        let finished = self.job.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(done) = finished {
            self.job = None;
            self.denied.extend(done.denied);
            self.lowered.extend(done.lowered);
            self.lowered.retain(|(id, _)| !done.restored.contains(id));
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

        let inspector = inspector_frame();
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
        toolboks_addon_sdk::tutorial::mark(ui, "inspector", rect);
        inspector_edge(ui, rect);

        let side = egui::Frame::NONE.inner_margin(CARD_PAD);
        egui::CentralPanel::default().frame(side).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = Vec2::new(6.0, 1.0);
            let rows = toolboks_addon_sdk::tutorial::region(ui, "toolbar", |ui| {
                self.toolbar(ui, all_rows.clone())
            });
            ui.add_space(2.0);
            toolboks_addon_sdk::tutorial::region(ui, "table", |ui| {
                ui.scope(|ui| {
                    table_style(ui);
                    self.table(ui, rows);
                });
            });
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
        if self.frozen.is_some()
            && matches!(
                intent,
                Intent::AskEnd(_) | Intent::ConfirmEnd | Intent::Lower | Intent::Restore
            )
        {
            return;
        }
        match intent {
            Intent::AskEnd(pids) => self.confirm = Some(pids),
            Intent::CancelEnd => self.confirm = None,
            Intent::ConfirmEnd => {
                let (to_end, _) = plan_end(&self.selected_procs(procs), protected);
                if self
                    .confirm
                    .take()
                    .is_some_and(|ids| ids == action_identities(&to_end))
                {
                    self.start_job(ctx, Action::End(to_end));
                }
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
                        let identity = p.identity?;
                        let prev = self.lowered.iter().find(|(id, _)| *id == identity)?.1;
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
            ui.spacing_mut().item_spacing.x = SPACE_M;
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by name or pid")
                    .desired_width(170.0),
            );
            switch_row(ui, &mut self.grouped, "Group by name");
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
            self.confirm = None;
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
            .cell_layout(Layout::left_to_right(Align::Center))
            .striped(false)
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
                let sortable = |ui: &mut egui::Ui, label: &str, key: SortKey, right: bool| {
                    let arrow = sort_arrow(sort_key == key, descending);
                    header_cell(ui, label, arrow, sort_key == key, right).then_some(key)
                };
                header.col(|ui| {
                    sort_click = sortable(ui, "Name", SortKey::Name, false).or(sort_click)
                });
                if !grouped {
                    header.col(|ui| {
                        sort_click = sortable(ui, "PID", SortKey::Pid, true).or(sort_click)
                    });
                }
                header
                    .col(|ui| sort_click = sortable(ui, "CPU", SortKey::Cpu, true).or(sort_click));
                header.col(|ui| {
                    sort_click = sortable(ui, "Memory", SortKey::Memory, true).or(sort_click)
                });
            })
            .body(|mut body| {
                let rule = body.ui_mut().painter().clone();
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let r = &rows[row.index()];
                    let is_selected = selection.keys.contains(&r.key);
                    row.set_selected(is_selected);
                    let cell = |ui: &mut egui::Ui, text: String| {
                        let l = body_cell(ui, text, true, r.protected);
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
                        let l = body_cell(ui, display_name(r), false, r.protected);
                        if r.protected {
                            l.on_hover_text(PROTECTED_NOTE);
                        }
                    });
                    if !grouped {
                        row.col(|ui| cell(ui, r.pid.to_string()));
                    }
                    row.col(|ui| cell(ui, cpu_text(r.cpu, ready)));
                    row.col(|ui| cell(ui, size_plain(r.mem)));
                    let response = row.response();
                    row_rule(&rule, response.rect);
                    if response.clicked() && click.is_none() {
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
        ui.label(RichText::new(&title).size(TEXT_SECTION).strong());
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
            let note = if chosen.iter().any(|p| p.identity.is_none() && !protected(p)) {
                "Windows could not verify the selected processes."
            } else {
                PROTECTED_NOTE
            };
            ui.label(RichText::new(note).weak().small());
        } else if !skipped.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} of {} are protected or could not be verified.",
                    skipped.len(),
                    chosen.len()
                ))
                .weak()
                .small(),
            );
        }
        ui.separator();

        let paused = self.frozen.is_some();
        if paused {
            ui.label(
                RichText::new("Resume to change running processes.")
                    .weak()
                    .small(),
            );
        }
        let busy = self.job.is_some() || paused;
        let mut intent = None;
        let end_pids = action_identities(&to_end);
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
                    .add_enabled(
                        !busy,
                        egui::Button::new(RichText::new("End task").color(RED)),
                    )
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
            .any(|p| self.lowered.iter().any(|(id, _)| Some(*id) == p.identity));
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
        if self.job.is_some() {
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
pub(super) fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
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
            identity: Some(ProcessIdentity {
                pid,
                created: u64::from(pid),
            }),
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
    fn reused_pid_changes_selection_and_confirmation_keys() {
        let original = p(10, "app.exe", 0.0, 0);
        let mut replacement = original.clone();
        replacement.identity.as_mut().unwrap().created += 1;
        assert_ne!(row_key(false, &original), row_key(false, &replacement));
        assert_ne!(
            action_identities(&[original]),
            action_identities(&[replacement])
        );
    }

    #[test]
    fn process_actions_verify_identity_on_the_handle_they_modify() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let cmd = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/cmd.exe");
        let mut child = Command::new(cmd)
            .args(["/d", "/q", "/c", "set /p toolboks_test_wait="])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let sample_start = snapshot_start();
        let pid = sysinfo::Pid::from_u32(child.id());
        let mut system = System::new().unwrap();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        let process = system.process(pid).unwrap();
        let identity = sampled_identity(process, sample_start).unwrap();
        let name = process.name().to_string_lossy();
        let stale = ProcessIdentity {
            created: identity.created - 1,
            ..identity
        };

        assert_eq!(end_process(stale, &name), Err(ActionError::IdentityChanged));
        assert_eq!(
            end_process(identity, "other.exe"),
            Err(ActionError::IdentityChanged)
        );
        assert_eq!(
            set_priority(stale, &name, BELOW_NORMAL_PRIORITY_CLASS.0),
            Err(ActionError::IdentityChanged)
        );
        assert!(child.try_wait().unwrap().is_none());

        let previous = set_priority(identity, &name, BELOW_NORMAL_PRIORITY_CLASS.0).unwrap();
        assert_eq!(
            set_priority(identity, &name, previous),
            Ok(BELOW_NORMAL_PRIORITY_CLASS.0)
        );
        end_process(identity, &name).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(1));
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
