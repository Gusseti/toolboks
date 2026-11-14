//! The Debloater tool: turn off Windows ads, suggestions and tracking toggles. State lives in
//! memory only; the page always reads the real state from Windows.

// model::Group::Apps is not constructed anywhere yet; drop this once the curated list is used.

use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use eframe::egui;

use super::Tool;

// The UI-free engine lives in its own crate; re-exported so `debloat::<module>` paths keep working.
pub use toolboks_debloat_engine::{backend, catalog, model, plan};

pub mod apps;
mod page;
pub mod restore;
pub mod services;
pub mod startup;
mod style;
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
    /// Tasks page: Windows confirmed a new or recent restore point this session.
    tasks_done: bool,
    /// Services page: same, once per session.
    services_done: bool,
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
        Result<tasks::UndoStep, ChangeError<tasks::UndoStep>>,
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

/// Result of a short worker run on the Services page.
enum ServiceJob {
    /// One service was switched off: (title, outcome, restore point result if one was made first).
    Set(
        String,
        Result<services::UndoStep, ChangeError<services::UndoStep>>,
        Option<restore::RestoreStatus>,
    ),
    /// Undo ran; the steps that failed come back with the counts.
    Undo {
        remaining: Vec<services::UndoStep>,
        done: usize,
        failed: usize,
    },
}

#[derive(Debug)]
pub(super) struct ChangeError<T> {
    message: String,
    recovery: Option<T>,
}

impl<T> From<String> for ChangeError<T> {
    fn from(message: String) -> Self {
        Self {
            message,
            recovery: None,
        }
    }
}

impl<T> std::fmt::Display for ChangeError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

fn rollback<T>(
    message: String,
    step: T,
    undo: impl FnOnce(&T) -> Result<(), String>,
) -> ChangeError<T> {
    match undo(&step) {
        Ok(()) => format!("{message}. The previous setting was restored.").into(),
        Err(error) => ChangeError {
            message: format!(
                "{message}. Could not restore the previous setting ({error}). Use Undo to retry."
            ),
            recovery: Some(step),
        },
    }
}

/// State of the Services page. Service Control Manager calls run on worker threads only.
#[derive(Default)]
struct ServicesState {
    statuses: Vec<services::ServiceStatus>,
    /// A load was started and not yet invalidated.
    started: bool,
    loading: Option<Receiver<Vec<services::ServiceStatus>>>,
    /// A change or undo is running.
    working: Option<Receiver<ServiceJob>>,
    /// The service shown in the inspector.
    focus: Option<&'static str>,
    /// A service waiting for the inline "Switch off?" confirm.
    pending: Option<&'static str>,
    /// Changes of this session, oldest first.
    steps: Vec<services::UndoStep>,
    message: Option<String>,
}

pub struct Debloater {
    env: model::Env,
    can_change_user_profile: bool,
    statuses: Vec<model::Status>,
    /// Ticked item ids.
    selected: HashSet<&'static str>,
    /// The item shown in "About this item".
    focus: Option<&'static str>,
    /// Registry undo steps grouped by apply run, oldest first.
    undo_runs: Vec<Vec<plan::UndoOp>>,
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
    services: ServicesState,
    restore: RestoreState,
}

impl Debloater {
    pub fn new() -> Self {
        Self {
            env: backend::detect_env(),
            can_change_user_profile: crate::elevation::can_change_user_profile(),
            statuses: Vec::new(),
            selected: HashSet::new(),
            focus: None,
            undo_runs: Vec::new(),
            message: None,
            run_notes: Vec::new(),
            confirming: false,
            loaded: false,
            apps: AppsState::default(),
            startup: StartupState::default(),
            tasks: TasksState::default(),
            services: ServicesState::default(),
            restore: RestoreState::default(),
        }
    }
}

impl Tool for Debloater {
    fn suspend(&mut self) -> bool {
        self.statuses = Vec::new();
        self.loaded = false;
        self.confirming = false;
        self.selected = HashSet::new();
        self.apps.rows = Vec::new();
        self.apps.loading = None;
        self.apps.started = false;
        self.apps.confirming = false;
        self.apps.selected = HashSet::new();
        self.startup.entries = Vec::new();
        self.startup.loading = None;
        self.startup.started = false;
        self.startup.pending = None;
        self.tasks.statuses = Vec::new();
        self.tasks.loading = None;
        self.tasks.started = false;
        self.tasks.pending = None;
        self.services.statuses = Vec::new();
        self.services.loading = None;
        self.services.started = false;
        self.services.pending = None;
        // Undo and results from committed actions remain available for the session.
        !self.undo_runs.is_empty()
            || !self.startup.steps.is_empty()
            || !self.tasks.steps.is_empty()
            || !self.services.steps.is_empty()
            || self.apps.removing.is_some()
            || self.restore.creating.is_some()
            || self.tasks.working.is_some()
            || self.services.working.is_some()
            || self.message.is_some()
            || self.apps.message.is_some()
            || self.startup.message.is_some()
            || self.tasks.message.is_some()
            || self.services.message.is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        self.page_ui(ui, page);
    }
}
