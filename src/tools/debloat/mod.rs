//! The Debloater tool: turn off Windows ads, suggestions and tracking toggles. State lives in
//! memory only; the page always reads the real state from Windows.

// model::Group::Apps is not constructed anywhere yet; drop this once the curated list is used.

use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use eframe::egui;

use super::Tool;

pub mod apps;
pub mod backend;
pub mod catalog;
pub mod model;
mod page;
pub mod plan;
pub mod restore;
pub mod startup;
pub mod tasks;

/// State of the Apps page. Package calls are slow, so they run on worker threads and report
/// back over these channels.
#[derive(Default)]
struct AppsState {
    rows: Vec<apps::AppRow>,
    /// A load was started and not yet invalidated.
    started: bool,
    loading: Option<Receiver<Result<Vec<apps::AppRow>, String>>>,
    removing: Option<Receiver<apps::AppRun>>,
    error: Option<String>,
    /// Ticked curated app ids.
    selected: HashSet<&'static str>,
    focus: Option<&'static str>,
    confirming: bool,
    last_run: Option<apps::AppRun>,
    message: Option<String>,
}

/// State of the Startup page. Reading versions of every startup file is slow, so the list loads
/// on a worker thread; a single switch change runs on the UI thread.
#[derive(Default)]
struct StartupState {
    entries: Vec<startup::Entry>,
    /// A load was started and not yet invalidated.
    started: bool,
    loading: Option<Receiver<Result<Vec<startup::Entry>, String>>>,
    error: Option<String>,
    /// The row shown in the inspector, as (source, name).
    focus: Option<(startup::Source, String)>,
    /// A Careful entry waiting for the inline "Switch off?" confirm.
    pending: Option<(startup::Source, String)>,
    /// Changes of this session, oldest first.
    steps: Vec<startup::UndoStep>,
    message: Option<String>,
}

/// The "Create a restore point first" switch, shared by Privacy and ads and Tasks. Memory only.
#[derive(Default)]
struct RestoreState {
    /// The switch (off by default each session).
    on: bool,
    /// Tasks page: a restore point was already tried this session, so do not repeat it.
    tasks_done: bool,
    /// Privacy and ads: creating before an apply, with the ids to apply afterwards.
    creating: Option<Receiver<(restore::RestoreStatus, Vec<&'static str>)>>,
    /// Last result line.
    status: Option<String>,
}

/// Result of a short worker run on the Tasks page.
enum TaskJob {
    /// One task was switched: (title, outcome, restore point result if one was made first).
    Set(
        String,
        Result<tasks::UndoStep, String>,
        Option<restore::RestoreStatus>,
    ),
    /// Undo ran; the steps that failed come back with the counts.
    Undo {
        remaining: Vec<tasks::UndoStep>,
        done: usize,
        failed: usize,
    },
}

/// State of the Tasks page. Task Scheduler (COM) calls run on worker threads only.
#[derive(Default)]
struct TasksState {
    statuses: Vec<tasks::TaskStatus>,
    /// A load was started and not yet invalidated.
    started: bool,
    loading: Option<Receiver<Vec<tasks::TaskStatus>>>,
    /// A change or undo is running.
    working: Option<Receiver<TaskJob>>,
    /// The task shown in the inspector.
    focus: Option<&'static str>,
    /// A Careful task waiting for the inline "Switch off?" confirm.
    pending: Option<&'static str>,
    /// Changes of this session, oldest first.
    steps: Vec<tasks::UndoStep>,
    message: Option<String>,
}

pub struct Debloater {
    env: model::Env,
    statuses: Vec<model::Status>,
    /// Ticked item ids.
    selected: HashSet<&'static str>,
    /// The item shown in "About this item".
    focus: Option<&'static str>,
    last_run: Option<plan::Run>,
    message: Option<String>,
    /// Plain-words lines for items of the last run that did not fully work.
    run_notes: Vec<String>,
    /// The inline "Apply N changes?" confirm block is open.
    confirming: bool,
    /// The state has been read from Windows at least once.
    loaded: bool,
    apps: AppsState,
    startup: StartupState,
    tasks: TasksState,
    restore: RestoreState,
}

impl Debloater {
    pub fn new() -> Self {
        Self {
            env: backend::detect_env(),
            statuses: Vec::new(),
            selected: HashSet::new(),
            focus: None,
            last_run: None,
            message: None,
            run_notes: Vec::new(),
            confirming: false,
            loaded: false,
            apps: AppsState::default(),
            startup: StartupState::default(),
            tasks: TasksState::default(),
            restore: RestoreState::default(),
        }
    }
}

impl Tool for Debloater {
    fn name(&self) -> &'static str {
        "Debloater"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Privacy and ads", "Apps", "Startup", "Tasks"]
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        self.page_ui(ui, page);
    }
}
