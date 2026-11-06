//! Native Disk Manager workspace, adapted from the standalone Stratum scan engine.
mod folder_picker;
mod map;
mod optimization;
mod scanner;
mod tutorial;
use egui::{Color32, RichText, Vec2};
use scanner::{Node, Progress, Query, State};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use toolboks_addon_sdk::tutorial as guide;
use toolboks_addon_sdk::NativeAddon;

pub const PROVIDER: &str = "stratum.disk-manager";
pub const PACKAGE: &str = include_str!("../disk-manager.tbaddon");
pub const LEGACY_PACKAGE: &str = include_str!("../legacy-0.1.tbaddon");
pub const ID: &str = "toolboks.disk-manager";
const ACCENT: Color32 = Color32::from_rgb(164, 138, 202);
const MUTED: Color32 = Color32::from_rgb(164, 168, 178);
const PANEL: Color32 = Color32::from_rgb(25, 29, 34);
const BORDER: Color32 = Color32::from_rgb(42, 45, 51);

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    path: String,
    exclude: String,
    min_bytes: u64,
    older_days: u64,
    sort: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            path: if cfg!(windows) {
                "C:\\".into()
            } else {
                "/".into()
            },
            exclude: String::new(),
            min_bytes: 0,
            older_days: 0,
            sort: "size".into(),
        }
    }
}
#[derive(Clone)]
struct Drive {
    path: String,
    total: u64,
    free: u64,
}
pub struct DiskManager {
    settings: Settings,
    loaded: bool,
    shared: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    drives: Vec<Drive>,
    page: usize,
    parent: usize,
    search: String,
    offset: usize,
    options: bool,
    message: Option<String>,
    export_kind: Option<bool>,
    export_path: String,
    export_job: Option<mpsc::Receiver<Result<String, String>>>,
    last_generation: u64,
    listing_cache: Option<(String, scanner::Listing)>,
    map_cache: Option<(u64, usize, Arc<map::MapLayout>)>,
    picker: Option<mpsc::Receiver<Result<Option<String>, String>>>,
    tutorial_snapshot: Option<(usize, bool, usize, usize)>,
    optimization: optimization::Panel,
}
impl Default for DiskManager {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            loaded: false,
            shared: Default::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            drives: drives(),
            page: 0,
            parent: 0,
            search: String::new(),
            offset: 0,
            options: false,
            message: None,
            export_kind: None,
            export_path: String::new(),
            export_job: None,
            last_generation: 0,
            listing_cache: None,
            map_cache: None,
            picker: None,
            tutorial_snapshot: None,
            optimization: Default::default(),
        }
    }
}
impl Drop for DiskManager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(9)
        .inner_margin(14)
}
fn weak(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}
fn button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE))
            .fill(Color32::from_rgb(105, 68, 158))
            .corner_radius(6)
            .min_size(Vec2::new(75.0, 30.0)),
    )
}
pub fn size(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", units[unit])
    }
}
fn display_path(path: &str) -> String {
    if let Some(p) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{p}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).into()
    }
}
fn elapsed(s: f64) -> String {
    if s < 60.0 {
        format!("{s:.1}s")
    } else {
        format!("{}m {}s", s as u64 / 60, s as u64 % 60)
    }
}

impl DiskManager {
    fn start(&mut self) {
        if self.shared.lock().unwrap().progress.running {
            return;
        }
        let root = match std::fs::canonicalize(self.settings.path.trim()) {
            Ok(p) if p.is_dir() => p,
            _ => {
                self.message = Some(
                    "That folder cannot be opened. Check the path and your permissions.".into(),
                );
                return;
            }
        };
        let mut state = self.shared.lock().unwrap();
        let generation = state.progress.generation + 1;
        state.progress = Progress {
            running: true,
            root: root.to_string_lossy().into(),
            generation,
            ..Default::default()
        };
        state.nodes = Arc::new(vec![]);
        drop(state);
        self.parent = 0;
        self.offset = 0;
        self.search.clear();
        self.cancel.store(false, Ordering::Relaxed);
        let shared = self.shared.clone();
        let cancel = self.cancel.clone();
        let excludes = self
            .settings
            .exclude
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        std::thread::spawn(move || {
            let state = shared.clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scanner::scan(root, excludes, shared, cancel)
            }))
            .is_err()
            {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.progress.running = false;
                s.progress.cancelled = true;
                s.progress.errors.push(
                    "The scan worker stopped unexpectedly. Start a new scan to retry.".into(),
                );
            }
        });
        self.message = None;
    }
    fn progress(&self) -> (Progress, Arc<Vec<Node>>) {
        let s = self.shared.lock().unwrap();
        (s.progress.clone(), s.nodes.clone())
    }
    fn drill(&mut self, id: usize) {
        self.parent = id;
        self.offset = 0;
        self.search.clear();
    }
    fn map_entries(&mut self, nodes: &[Node], p: &Progress) -> Arc<map::MapLayout> {
        if self
            .map_cache
            .as_ref()
            .is_none_or(|(g, parent, _)| *g != p.generation || *parent != self.parent)
        {
            let mut children = nodes
                .iter()
                .filter(|n| n.parent == Some(self.parent))
                .collect::<Vec<_>>();
            children.sort_by_key(|n| std::cmp::Reverse(n.bytes));
            self.map_cache = Some((
                p.generation,
                self.parent,
                Arc::new(map::MapLayout::new(&children)),
            ));
        }
        self.map_cache.as_ref().unwrap().2.clone()
    }
    fn export(&mut self, nodes: Arc<Vec<Node>>, progress: Progress, json: bool) {
        let path = PathBuf::from(self.export_path.trim().trim_matches('"'));
        if path.file_name().is_none() {
            self.message = Some("Enter a full report filename.".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.export_job = Some(rx);
        self.export_kind = None;
        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                use std::io::Write;
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| {
                        format!("Could not create report: {e}. Existing files are not overwritten.")
                    })?;
                let mut out = std::io::BufWriter::new(file);
                if json {
                    #[derive(Serialize)]
                    struct Report<'a> {
                        application: &'static str,
                        summary: &'a Progress,
                        measurement: &'static str,
                        entries: &'a [Node],
                    }
                    let report=Report{application:"Disk Manager by stratum",summary:&progress,measurement:"logical bytes; hard links counted per entry; links and junctions skipped",entries:&nodes};
                    serde_json::to_writer_pretty(&mut out, &report).map_err(|e| e.to_string())?;
                } else {
                    write_csv(&mut out, &nodes, progress.cancelled).map_err(|e| e.to_string())?;
                }
                out.flush().map_err(|e| e.to_string())?;
                Ok(format!("Report saved to {}", path.display()))
            })();
            let _ = tx.send(result);
        });
    }
    fn stats(&self, ui: &mut egui::Ui, p: &Progress) {
        ui.columns(4, |cols| {
            for (col, (label, value, foot)) in cols.iter_mut().zip([
                ("ANALYZED SPACE", size(p.bytes), "Logical file sizes"),
                ("FILES FOUND", p.files.to_string(), "Across this scan"),
                (
                    "FOLDERS EXPLORED",
                    p.folders.to_string(),
                    "Readable locations",
                ),
                (
                    "SCAN TIME",
                    elapsed(p.elapsed),
                    if p.running {
                        "Scanning in background"
                    } else if p.cancelled {
                        "Partial results"
                    } else {
                        "Scan complete"
                    },
                ),
            ]) {
                card().show(col, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(label).size(11.0).color(MUTED));
                    ui.add_space(7.0);
                    ui.label(RichText::new(value).size(24.0));
                    ui.label(RichText::new(foot).size(12.0).color(MUTED));
                });
            }
        });
    }
    fn list(&mut self, ui: &mut egui::Ui, nodes: &[Node], p: &Progress) {
        let mut changed = false;
        let filters = ui.horizontal_wrapped(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search scanned paths…")
                        .desired_width(210.0),
                )
                .changed();
            egui::ComboBox::from_id_salt("disk-size")
                .selected_text(match self.settings.min_bytes {
                    0 => "Any size",
                    104857600 => "Over 100 MiB",
                    1073741824 => "Over 1 GiB",
                    _ => "Over 10 GiB",
                })
                .show_ui(ui, |ui| {
                    for (v, l) in [
                        (0, "Any size"),
                        (104857600, "Over 100 MiB"),
                        (1073741824, "Over 1 GiB"),
                        (10737418240, "Over 10 GiB"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut self.settings.min_bytes, v, l)
                            .changed();
                    }
                });
            egui::ComboBox::from_id_salt("disk-age")
                .selected_text(match self.settings.older_days {
                    0 => "Any age",
                    180 => "6+ months",
                    365 => "1+ year",
                    _ => "3+ years",
                })
                .show_ui(ui, |ui| {
                    for (v, l) in [
                        (0, "Any age"),
                        (180, "6+ months"),
                        (365, "1+ year"),
                        (1095, "3+ years"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut self.settings.older_days, v, l)
                            .changed();
                    }
                });
            egui::ComboBox::from_id_salt("disk-sort")
                .selected_text(match self.settings.sort.as_str() {
                    "name" => "Name A–Z",
                    "files" => "Most files",
                    "modified" => "Recently modified",
                    _ => "Largest first",
                })
                .show_ui(ui, |ui| {
                    for (v, l) in [
                        ("size", "Largest first"),
                        ("name", "Name A–Z"),
                        ("files", "Most files"),
                        ("modified", "Recently modified"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut self.settings.sort, v.into(), l)
                            .changed();
                    }
                });
        });
        guide::mark(ui, "disk-filters", filters.response.rect);
        if changed {
            self.offset = 0;
        }
        let q = Query {
            parent: Some(self.parent),
            mode: Some(if self.page == 1 { "files" } else { "folders" }.into()),
            search: Some(self.search.clone()),
            sort: Some(self.settings.sort.clone()),
            offset: Some(self.offset),
            limit: Some(100),
            min_bytes: Some(self.settings.min_bytes),
            older_days: Some(self.settings.older_days),
        };
        let key = format!("{}:{}", p.generation, serde_json::to_string(&q).unwrap());
        if self
            .listing_cache
            .as_ref()
            .is_none_or(|(old, _)| old != &key)
        {
            self.listing_cache = Some((key, scanner::listing(nodes, &q)));
        }
        let listing = self.listing_cache.as_ref().unwrap().1.clone();
        ui.add_space(10.0);
        if self.page != 1 {
            let crumbs = egui::ScrollArea::horizontal()
                .id_salt("disk-breadcrumb")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for n in &listing.breadcrumbs {
                            if ui
                                .small_button(&n.name)
                                .on_hover_text(display_path(&n.path))
                                .clicked()
                            {
                                self.drill(n.id);
                            }
                            ui.label(RichText::new("/").color(MUTED));
                        }
                    });
                });
            guide::mark(ui, "disk-breadcrumbs", crumbs.inner_rect);
            ui.add_space(8.0);
        }
        if listing.rows.is_empty() {
            weak(ui, "No entries match these filters.");
            return;
        }
        let mut navigate = None;
        let table = egui_extras::TableBuilder::new(ui)
            .id_salt("disk-entries")
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().at_least(140.0))
            .column(egui_extras::Column::initial(95.0))
            .column(egui_extras::Column::initial(65.0))
            .column(egui_extras::Column::initial(65.0))
            .min_scrolled_height(100.0)
            .max_scroll_height(340.0);
        table
            .header(24.0, |mut h| {
                for label in ["NAME", "SIZE", "OF SCAN", "FILES"] {
                    h.col(|ui| {
                        ui.label(RichText::new(label).size(10.0).color(MUTED));
                    });
                }
            })
            .body(|body| {
                body.rows(29.0, listing.rows.len(), |mut row| {
                    let n = &listing.rows[row.index()];
                    row.col(|ui| {
                        let label = if n.dir {
                            format!("+ {}", n.name)
                        } else {
                            n.name.clone()
                        };
                        let r = ui.add(
                            egui::Button::new(RichText::new(label).color(if n.dir {
                                ACCENT
                            } else {
                                ui.visuals().text_color()
                            }))
                            .frame(false),
                        );
                        r.clone().on_hover_text(format!(
                            "{}\n{} · {} file(s)",
                            display_path(&n.path),
                            size(n.bytes),
                            n.files
                        ));
                        if n.dir && r.clicked() {
                            navigate = Some(n.id);
                        }
                        r.context_menu(|ui| {
                            if ui.button("Copy path").clicked() {
                                ui.ctx().copy_text(display_path(&n.path));
                                ui.close();
                            }
                            if ui.button("Reveal in Explorer").clicked() {
                                reveal(&n.path);
                                ui.close();
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.label(size(n.bytes));
                    });
                    row.col(|ui| {
                        weak(
                            ui,
                            format!("{:.1}%", n.bytes as f64 / p.bytes.max(1) as f64 * 100.0),
                        );
                    });
                    row.col(|ui| {
                        weak(ui, n.files.to_string());
                    });
                });
            });
        if let Some(id) = navigate {
            self.drill(id);
        }
        ui.add_space(7.0);
        ui.horizontal(|ui| {
            weak(
                ui,
                format!(
                    "{}–{} of {} entries · right-click for file actions",
                    self.offset + 1,
                    self.offset + listing.rows.len(),
                    listing.total
                ),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.offset + listing.rows.len() < listing.total,
                        egui::Button::new("Next >"),
                    )
                    .clicked()
                {
                    self.offset += 100;
                }
                if ui
                    .add_enabled(self.offset > 0, egui::Button::new("< Previous"))
                    .clicked()
                {
                    self.offset = self.offset.saturating_sub(100);
                }
            });
        });
    }
}

impl NativeAddon for DiskManager {
    fn set_capabilities(&mut self, capabilities: &[toolboks_addon_sdk::Capability]) {
        self.optimization.capabilities(capabilities);
    }
    fn take_host_request(&mut self) -> Option<toolboks_addon_sdk::HostRequest> {
        self.optimization.request()
    }
    fn navigate(&mut self, page: &str) {
        self.page = page.parse::<usize>().unwrap_or(0).min(4);
    }
    fn tutorial_steps(&self) -> Vec<toolboks_addon_sdk::TutorialStep> {
        tutorial::steps()
    }
    fn tutorial_location(&self) -> String {
        format!("{}/{}", self.page, self.options)
    }
    fn tutorial_start(&mut self) {
        self.optimization.tutorial_start();
        self.tutorial_snapshot = Some((self.page, self.options, self.parent, self.offset));
        self.parent = 0;
        self.offset = 0;
        self.map_cache = None;
        self.listing_cache = None;
    }
    fn tutorial_navigate(&mut self, route: &str) {
        let mut parts = route.split('/');
        self.page = parts
            .next()
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(0)
            .min(4);
        self.options = matches!(parts.next(), Some("options" | "true"));
        self.offset = 0;
    }
    fn tutorial_finish(&mut self) {
        self.optimization.tutorial_finish();
        if let Some((page, options, parent, offset)) = self.tutorial_snapshot.take() {
            self.page = page;
            self.options = options;
            self.parent = parent;
            self.offset = offset;
        }
        self.map_cache = None;
        self.listing_cache = None;
    }
    fn ui(&mut self, ui: &mut egui::Ui, prefs: &mut BTreeMap<String, serde_json::Value>) -> bool {
        if !self.loaded {
            if let Some(v) = prefs.get("settings") {
                self.settings = serde_json::from_value(v.clone()).unwrap_or_default();
            }
            self.loaded = true;
        }
        if let Some(job) = &self.export_job {
            match job.try_recv() {
                Ok(r) => {
                    self.message = Some(r.unwrap_or_else(|e| e));
                    self.export_job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = Some("The export worker stopped unexpectedly.".into());
                    self.export_job = None;
                }
                _ => {
                    ui.request_repaint_after(Duration::from_millis(200));
                }
            }
        }
        if let Some(picker) = &self.picker {
            match picker.try_recv() {
                Ok(Ok(path)) => {
                    if let Some(path) = path {
                        self.settings.path = path;
                    }
                    self.picker = None;
                }
                Ok(Err(error)) => {
                    self.message = Some(error);
                    self.picker = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = Some("The folder picker closed unexpectedly.".into());
                    self.picker = None;
                }
                _ => ui.request_repaint_after(Duration::from_millis(200)),
            }
        }
        let (p, nodes) = self.progress();
        if p.generation != self.last_generation {
            self.last_generation = p.generation;
            self.parent = 0;
            self.offset = 0;
        }
        let sample = self.tutorial_snapshot.is_some() && (p.generation == 0 || p.running);
        let (p, nodes) = if sample {
            tutorial::sample()
        } else {
            (p, nodes)
        };
        if sample {
            ui.label(
                RichText::new("TUTORIAL SAMPLE · Illustrative data, not a scan of your files")
                    .color(ACCENT),
            );
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Disk Manager").size(28.0));
            ui.label(
                RichText::new("by stratum")
                    .size(13.0)
                    .color(Color32::from_rgb(112, 108, 121)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = ui.add_enabled(
                    !p.running && !nodes.is_empty() && self.export_job.is_none(),
                    egui::Button::new("Export report"),
                );
                guide::mark(ui, "disk-export", export.rect);
                if export.clicked() {
                    self.export_kind = Some(false);
                    self.export_path = std::env::var("USERPROFILE")
                        .map(|p| {
                            PathBuf::from(p)
                                .join("Documents/Disk Manager Report.csv")
                                .display()
                                .to_string()
                        })
                        .unwrap_or("Disk Manager Report.csv".into());
                }
            });
        });
        weak(ui, "Find the weight. Keep what matters.");
        ui.add_space(16.0);
        if let Some(message) = self.message.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(message).color(ACCENT));
                if ui.small_button("Dismiss").clicked() {
                    self.message = None;
                }
            });
            ui.add_space(8.0);
        }
        ui.horizontal(|ui| {
            let drive = egui::ComboBox::from_id_salt("disk-drive")
                .width(85.0)
                .selected_text("Drives")
                .show_ui(ui, |ui| {
                    for d in &self.drives {
                        if ui
                            .selectable_label(
                                false,
                                format!("{} · {} free of {}", d.path, size(d.free), size(d.total)),
                            )
                            .clicked()
                        {
                            self.settings.path = d.path.clone();
                        }
                    }
                });
            guide::mark(ui, "disk-drives", drive.response.rect);
            let response = ui.add_enabled(
                !p.running && self.picker.is_none(),
                egui::TextEdit::singleline(&mut self.settings.path)
                    .hint_text("Drive or folder path")
                    .desired_width((ui.available_width() - 380.0).max(140.0)),
            );
            guide::mark(ui, "disk-path", response.rect);
            let enter = !guide::active(ui.ctx())
                && response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let browse = ui.add_enabled(
                !p.running && self.picker.is_none(),
                egui::Button::new(if self.picker.is_some() {
                    "Choosing…"
                } else {
                    "Browse to folder"
                }),
            );
            guide::mark(ui, "disk-browse", browse.rect);
            if browse.clicked() {
                self.picker = Some(folder_picker::start(
                    self.settings.path.clone(),
                    ui.ctx().clone(),
                ));
            }
            if p.running {
                let stop = button(ui, "Stop scan");
                guide::mark(ui, "disk-analyze", stop.rect);
                if stop.clicked() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            } else {
                let analyze = ui
                    .add_enabled_ui(self.picker.is_none(), |ui| button(ui, "Analyze space"))
                    .inner;
                guide::mark(ui, "disk-analyze", analyze.rect);
                if analyze.clicked() || (enter && self.picker.is_none()) {
                    self.start();
                }
            }
            if ui.button("Options").clicked() {
                self.options = !self.options;
            }
        });
        if self.options {
            ui.add_space(8.0);
            let options = card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label("Exclude folder names (separate with semicolons)");
                ui.add_enabled(
                    !p.running,
                    egui::TextEdit::singleline(&mut self.settings.exclude)
                        .hint_text("e.g. node_modules; .git; target")
                        .desired_width(ui.available_width()),
                );
                weak(
                    ui,
                    "Names match exactly, ignoring case. Leave empty for the most complete scan.",
                );
            });
            guide::mark(ui, "disk-options", options.response.rect);
        }
        ui.add_space(15.0);
        let mut optimization_changed = false;
        egui::ScrollArea::vertical().id_salt("disk-workspace").show(ui,|ui|{
            let tabs=ui.horizontal(|ui|{for(i,label)in["Overview","Largest files","File types","Scan details","Optimization"].iter().enumerate(){if ui.selectable_label(self.page==i,*label).clicked(){self.page=i;self.offset=0;}}});guide::mark(ui,"disk-tabs",tabs.response.rect);ui.add_space(14.0);
            if self.page==4{optimization_changed=self.optimization.ui(ui,prefs);return;}
            if p.generation==0{card().show(ui,|ui|{ui.set_width(ui.available_width());ui.set_min_height(190.0);ui.add_space(16.0);ui.label(RichText::new("Your storage, in perspective.").size(28.0));ui.add_space(10.0);weak(ui,"Choose a drive or enter any folder path, then analyze its space.\nExplore the biggest folders and files without leaving Toolboks.");ui.add_space(20.0);weak(ui,"Read-only scanning. Your files are never modified.");});return;}
            guide::region(ui,"disk-stats",|ui|self.stats(ui,&p));ui.add_space(16.0);
            if p.running{card().show(ui,|ui|{ui.set_width(ui.available_width());ui.horizontal(|ui|{ui.spinner();ui.label("Mapping your space…");});ui.add(egui::Label::new(RichText::new(display_path(&p.current)).small().color(MUTED)).truncate());weak(ui,format!("{} entries skipped. Stop at any time to explore partial results.",p.skipped));});return;}
            if p.cancelled{ui.label(RichText::new("Partial scan · totals cover only entries found before cancellation.").color(Color32::from_rgb(213,180,127)));ui.add_space(8.0);}
            match self.page {
                2=>{let types=card().show(ui,|ui|{ui.set_width(ui.available_width());for(i,e)in p.extensions.iter().enumerate(){ui.horizontal(|ui|{ui.label(RichText::new(&e.name).color(map::COLORS[i%map::COLORS.len()]));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(size(e.bytes));weak(ui,format!("{} files",e.files));});});ui.add(egui::ProgressBar::new(e.bytes as f32/p.bytes.max(1)as f32).fill(map::COLORS[i%map::COLORS.len()]).desired_width(ui.available_width()));ui.add_space(9.0);}});guide::mark(ui,"disk-types",types.response.rect);},
                3=>{let details=card().show(ui,|ui|{ui.set_width(ui.available_width());ui.label(RichText::new("Scan coverage").size(18.0));weak(ui,format!("{} entries skipped (links, junctions, exclusions or access errors).",p.skipped));weak(ui,"Sizes are logical file lengths. Hard links count per directory entry; sparse or cloud-placeholder files can differ from allocated space. Files may change during scanning.");ui.add_space(14.0);ui.label("Access and read errors (first 100)");if p.errors.is_empty(){weak(ui,"No errors recorded.");}for error in &p.errors{ui.label(RichText::new(error).small().color(MUTED));}});guide::mark(ui,"disk-details",details.response.rect);},
                _=>{
                    if self.page==0{let map=card().show(ui,|ui|{ui.set_width(ui.available_width());ui.horizontal(|ui|{ui.label(RichText::new("Space map").size(15.0));weak(ui,"Click a folder to explore");if self.parent!=0&&ui.small_button("Back to root").clicked(){self.drill(0);}});let entries=self.map_entries(&nodes,&p);if let Some(id)=map::show(ui,&entries){if let Some(n)=nodes.get(id){if n.dir{self.drill(id)}else{self.message=Some(format!("{} · {}",display_path(&n.path),size(n.bytes)))}}}});guide::mark(ui,"disk-map",map.response.rect);ui.add_space(16.0);}
                    guide::region(ui,"disk-list",|ui|self.list(ui,&nodes,&p));
                }
            }
            ui.add_space(10.0);ui.label(RichText::new("Logical sizes · Links and junctions excluded · Read-only analysis").size(10.0).color(MUTED));
        });
        if let Some(mut json) = self.export_kind {
            let response=egui::Modal::new(egui::Id::unique("disk-export")).frame(card()).show(ui.ctx(),|ui|{ui.set_width(450.0);ui.label(RichText::new("Take the insight with you.").size(23.0));ui.add_space(8.0);weak(ui,"Export every discovered folder and file. Existing files are not overwritten.");ui.horizontal(|ui|{if ui.selectable_value(&mut json,false,"CSV").changed(){self.export_path=PathBuf::from(&self.export_path).with_extension("csv").display().to_string();}
if ui.selectable_value(&mut json,true,"JSON").changed(){self.export_path=PathBuf::from(&self.export_path).with_extension("json").display().to_string();}});ui.add(egui::TextEdit::singleline(&mut self.export_path).desired_width(ui.available_width()));ui.add_space(15.0);ui.horizontal(|ui|{if button(ui,"Save report").clicked(){self.export(nodes.clone(),p.clone(),json);}
if ui.button("Cancel").clicked(){self.export_kind=None;}});});
            if response.should_close() {
                self.export_kind = None;
            } else if self.export_kind.is_some() {
                self.export_kind = Some(json);
            }
        }
        let value = serde_json::to_value(&self.settings).unwrap();
        if prefs.get("settings") != Some(&value) {
            prefs.insert("settings".into(), value);
            true
        } else {
            optimization_changed
        }
    }
    fn tick(&mut self, ctx: &egui::Context) {
        if self.shared.lock().unwrap().progress.running
            || self.export_job.is_some()
            || self.picker.is_some()
            || self.optimization.busy()
        {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn reveal(path: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", display_path(path)))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}
fn drives() -> Vec<Drive> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetDiskFreeSpaceExW(
                path: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let mut out = vec![];
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let path = format!("{}:\\", (b'A' + i) as char);
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let (mut available, mut total, mut free) = (0, 0, 0);
            if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) }
                != 0
            {
                out.push(Drive { path, total, free });
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}
fn csv_cell(value: &str) -> String {
    let protected = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.into()
    };
    format!("\"{}\"", protected.replace('"', "\"\""))
}
fn write_csv(out: &mut impl std::io::Write, nodes: &[Node], partial: bool) -> std::io::Result<()> {
    writeln!(
        out,
        "Path,Kind,Logical bytes,Files,Modified Unix seconds,Partial scan"
    )?;
    for n in nodes {
        writeln!(
            out,
            "{},{},{},{},{},{}",
            csv_cell(&display_path(&n.path)),
            if n.dir { "Folder" } else { "File" },
            n.bytes,
            n.files,
            n.modified,
            partial
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_protects_formulas_and_quotes() {
        assert_eq!(csv_cell("=1+1"), "\"'=1+1\"");
        assert_eq!(csv_cell("a\"b"), "\"a\"\"b\"");
    }
    #[test]
    fn pretty_paths_and_sizes() {
        assert_eq!(display_path(r"\\?\C:\data"), r"C:\data");
        assert_eq!(size(1024), "1.00 KiB");
    }
    #[test]
    fn official_package_validates() {
        let p = toolboks_addon_sdk::Package::parse(PACKAGE.as_bytes(), "0.8.2").unwrap();
        assert_eq!(p.manifest.native_provider.as_deref(), Some(PROVIDER));
    }
}
