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
pub mod startup;

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
        }
    }
}

impl Tool for Debloater {
    fn name(&self) -> &'static str {
        "Debloater"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Privacy and ads", "Apps", "Startup"]
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        self.page_ui(ui, page);
    }
}
