mod admin;
mod auto;
mod memlist;
mod stats;
mod trim;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use self::admin::Job;
use self::auto::AutoClean;
use self::memlist::Lists;
use self::stats::{format_bytes, group_by_name, sort_and_filter, ProcInfo, Sampler, SortKey};
use self::trim::{skip_reason, trim_process, SkipReason, TrimError};
use super::Tool;
use crate::elevation;

const REFRESH: Duration = Duration::from_secs(2);
/// How often the auto-clean rules are checked, in the background too.
const AUTO_CHECK: Duration = Duration::from_secs(5);
const ROW_HEIGHT: f32 = 22.0;

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
    /// A trim waiting for the user's confirmation: (pid, name).
    pending: Option<(u32, String)>,
    /// A system-wide command waiting for the user's confirmation.
    pending_system: Option<memlist::Command>,
    /// The system-wide command running on a worker thread, if any.
    job: Option<Job>,
    /// Result of the last action, shown above the table.
    status: Option<String>,
    auto: AutoClean,
    last_auto_check: Instant,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            sampler: Sampler::new(),
            lists: memlist::query(),
            elevated: elevation::is_elevated(),
            last_refresh: Instant::now(),
            sort_key: SortKey::WorkingSet,
            descending: true,
            filter: String::new(),
            grouped: false,
            exclusions: HashSet::new(),
            pending: None,
            pending_system: None,
            job: None,
            status: None,
            auto: AutoClean::new(),
            last_auto_check: Instant::now(),
        }
    }

    fn refresh_now(&mut self) {
        if let Some(sampler) = self.sampler.as_mut() {
            sampler.refresh();
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

    fn run_trim(&mut self, pid: u32, name: &str) {
        // The list may have changed since the click: check again that this pid is still the
        // same, still trimmable process.
        let still_there = self.sampler.as_ref().is_some_and(|s| {
            s.processes
                .iter()
                .any(|p| p.pid == pid && p.name.eq_ignore_ascii_case(name))
        });
        if !still_there {
            self.status = Some(format!(
                "Not trimmed: {name} (pid {pid}) is no longer running under that name."
            ));
            return;
        }
        if let Some(reason) = skip_reason(pid, name, std::process::id(), &self.exclusions) {
            self.status = Some(format!("Not trimmed: {}.", reason.describe()));
            return;
        }

        self.status = Some(match trim_process(pid) {
            Ok(t) => format!(
                "Trimmed {name} (pid {pid}): {} \u{2192} {} right after the trim. \
                 Pages stay cached and fault back when the process uses them.",
                format_bytes(t.before),
                format_bytes(t.after)
            ),
            Err(TrimError::AccessDenied) => {
                format!(
                    "Could not trim {name} (pid {pid}): access denied. It may need admin rights."
                )
            }
            Err(TrimError::Failed) => {
                format!("Could not trim {name} (pid {pid}): the process may have exited.")
            }
        });
        // Show the new numbers on the next frame instead of waiting for the timer.
        self.last_refresh = Instant::now() - REFRESH;
    }

    fn confirm_bar(&mut self, ui: &mut egui::Ui) {
        let Some((pid, name)) = self.pending.clone() else {
            return;
        };
        ui.group(|ui| {
            ui.label(format!(
                "Trim {name} (pid {pid})? Its memory moves to the standby list and is read back \
                 when needed, so it can be briefly slower."
            ));
            ui.horizontal(|ui| {
                if ui.button("Trim").clicked() {
                    self.run_trim(pid, &name);
                    self.pending = None;
                }
                if ui.button("Cancel").clicked() {
                    self.pending = None;
                }
            });
        });
        ui.add_space(4.0);
    }

    fn exclusions_list(&mut self, ui: &mut egui::Ui) {
        let mut names: Vec<String> = self.exclusions.iter().cloned().collect();
        names.sort();
        egui::CollapsingHeader::new(format!("Exclusions ({})", names.len())).show(ui, |ui| {
            if names.is_empty() {
                ui.weak("None. Use Exclude on a process to protect it from trimming.");
            }
            for name in &names {
                ui.horizontal(|ui| {
                    ui.label(name);
                    if ui.small_button("Remove").clicked() {
                        self.exclusions.remove(name);
                    }
                });
            }
        });
    }
}

fn sort_arrow(active: bool, descending: bool) -> &'static str {
    match (active, descending) {
        (true, true) => " \u{25BC}",
        (true, false) => " \u{25B2}",
        _ => "",
    }
}

impl Tool for Memory {
    fn name(&self) -> &'static str {
        "Memory"
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.background(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.sampler.is_none() {
            ui.label("Memory information is not available on this system.");
            return;
        }

        if self.last_refresh.elapsed() >= REFRESH {
            self.refresh_now();
        }
        ui.request_repaint_after(REFRESH);

        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        ui.add(egui::ProgressBar::new(totals.used_fraction()).text(format!(
            "{} used of {} ({:.0}%)",
            format_bytes(totals.used),
            format_bytes(totals.total),
            totals.used_fraction() * 100.0
        )));
        let mut summary = format!("Available: {}", format_bytes(totals.available));
        if let Some(lists) = self.lists {
            summary += &format!(
                "  \u{b7}  Standby (cached): {}  \u{b7}  Free: {}  \u{b7}  Modified: {}",
                format_bytes(lists.standby),
                format_bytes(lists.free),
                format_bytes(lists.modified)
            );
        }
        ui.label(summary).on_hover_text(
            "Available = Free + Standby. Windows keeps recently used data in the standby list \
             and hands it out whenever a program needs memory.",
        );
        ui.add_space(8.0);

        self.confirm_bar(ui);
        if let Some(status) = &self.status {
            ui.label(status);
            ui.add_space(4.0);
        }
        self.system_panel(ui);
        self.auto_panel(ui);
        self.exclusions_list(ui);

        let process_count = self.sampler.as_ref().map_or(0, |s| s.processes.len());
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.filter);
            ui.checkbox(&mut self.grouped, "Group by name");
            ui.label(format!("{process_count} processes"));
        });
        ui.add_space(4.0);

        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let grouped_rows: Vec<ProcInfo>;
        let source: &[ProcInfo] = if self.grouped {
            grouped_rows = group_by_name(&sampler.processes);
            &grouped_rows
        } else {
            &sampler.processes
        };
        let rows = sort_and_filter(source, self.sort_key, self.descending, &self.filter);

        // The table borrows the sampler and exclusions, so record what was clicked
        // and apply it to `self` once the table is done.
        let mut sort_click: Option<SortKey> = None;
        let mut trim_click: Option<(u32, String)> = None;
        let mut exclude_click: Option<String> = None;
        let (sort_key, descending) = (self.sort_key, self.descending);
        let exclusions = &self.exclusions;
        let own_pid = std::process::id();

        TableBuilder::new(ui)
            .striped(true)
            .column(Column::remainder().at_least(120.0))
            .column(Column::exact(64.0))
            .column(Column::exact(100.0))
            .column(Column::exact(100.0))
            .column(Column::exact(120.0))
            .header(ROW_HEIGHT + 4.0, |mut header| {
                for (label, key) in [
                    ("Name", SortKey::Name),
                    ("PID", SortKey::Pid),
                    ("Working set", SortKey::WorkingSet),
                    ("Private", SortKey::Private),
                ] {
                    header.col(|ui| {
                        let arrow = sort_arrow(sort_key == key, descending);
                        if ui
                            .button(egui::RichText::new(format!("{label}{arrow}")).strong())
                            .clicked()
                        {
                            sort_click = Some(key);
                        }
                    });
                }
                header.col(|_| {});
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let p = rows[row.index()];
                    let reason = skip_reason(p.pid, &p.name, own_pid, exclusions);
                    let several = p.count > 1;
                    row.col(|ui| {
                        ui.label(&p.name);
                    });
                    row.col(|ui| {
                        // A group has no single pid to show.
                        ui.label(if several {
                            format!("\u{d7}{}", p.count)
                        } else {
                            p.pid.to_string()
                        });
                    });
                    row.col(|ui| {
                        ui.label(format_bytes(p.working_set));
                    });
                    row.col(|ui| {
                        ui.label(format_bytes(p.private));
                    });
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            let can_trim = reason.is_none() && !several;
                            let mut trim_btn =
                                ui.add_enabled(can_trim, egui::Button::new("Trim").small());
                            if several {
                                trim_btn = trim_btn.on_disabled_hover_text(
                                    "Turn off grouping to trim one process",
                                );
                            } else if let Some(reason) = reason {
                                trim_btn = trim_btn.on_disabled_hover_text(reason.describe());
                            }
                            if trim_btn.clicked() {
                                trim_click = Some((p.pid, p.name.clone()));
                            }

                            let excluded = reason == Some(SkipReason::Excluded);
                            let can_toggle = matches!(reason, None | Some(SkipReason::Excluded));
                            let label = if excluded { "Include" } else { "Exclude" };
                            if ui
                                .add_enabled(can_toggle, egui::Button::new(label).small())
                                .clicked()
                            {
                                exclude_click = Some(p.name.to_lowercase());
                            }
                        });
                    });
                });
            });

        if let Some(key) = sort_click {
            self.apply_sort(key);
        }
        if let Some(request) = trim_click {
            self.pending = Some(request);
        }
        if let Some(name) = exclude_click {
            self.toggle_exclusion(name);
        }
    }
}
