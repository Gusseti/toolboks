//! The Advanced page of the memory tool, a workbench: the memory lists on top, a filterable and
//! selectable process table in the middle and an inspector with three tabs on the right.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};

use super::groups::color_for;
use super::stats::{format_bytes, group_by_name, sort_and_filter, ProcInfo, SortKey};
use super::trim::{skip_reason, trim_process, SkipReason};
use super::workbench::{
    legend, list_bar_parts, list_segments, plan_trim, row_key, share_of, trim_summary,
};
use super::Memory;
use crate::format::size_plain;
use crate::ui::{pill, stacked_bar, ACCENT, CARD_PAD, GAP};

const ROW_HEIGHT: f32 = 22.0;
const INSPECTOR_WIDTH: f32 = 200.0;
const INSPECTOR_PAD: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Selected,
    System,
    Rules,
}

/// What a finished "Trim selected" run did.
struct TrimResult {
    trimmed: usize,
    groups: usize,
    before: u64,
    after: u64,
    skipped: usize,
    failed: usize,
}

/// A "Trim selected" run on a worker thread.
pub struct TrimJob {
    rx: Receiver<TrimResult>,
}

fn sort_arrow(active: bool, descending: bool) -> &'static str {
    match (active, descending) {
        // Plain letters: the bundled font has none of the arrow or triangle glyphs.
        (true, true) => " v",
        (true, false) => " ^",
        _ => "",
    }
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A label that does not take clicks, so the row underneath still gets them.
fn cell(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(text.into()).selectable(false));
}

/// A click on the table, applied once the table is drawn.
enum Click {
    /// A click on the row itself, with the modifier keys held.
    Row(String, egui::Modifiers),
    /// A click on the select box.
    Box(String),
}

impl Memory {
    pub(super) fn advanced_ui(&mut self, ui: &mut egui::Ui) {
        if self.was_grouped != self.grouped {
            self.was_grouped = self.grouped;
            self.selection.clear();
        }
        self.prune_selection();

        let inspector = egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(INSPECTOR_PAD);
        let panel = egui::Panel::right("memory_workbench_inspector")
            .resizable(false)
            .exact_size(INSPECTOR_WIDTH)
            .frame(inspector)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.workbench_inspector(ui));
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
            self.lists_strip(ui);
            ui.add_space(GAP - 2.0);
            self.toolbar(ui);
            if let Some(status) = &self.status {
                ui.label(RichText::new(status).small().weak());
            }
            ui.add_space(2.0);
            self.table(ui);
        });
    }

    /// Forget selected rows that no longer exist (the process ended).
    fn prune_selection(&mut self) {
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        if self.selection.len() == 0 {
            return;
        }
        let existing: HashSet<String> = sampler
            .processes
            .iter()
            .map(|p| row_key(self.grouped, p))
            .collect();
        self.selection.prune(&existing);
    }

    fn lists_strip(&self, ui: &mut egui::Ui) {
        let total = self.sampler.as_ref().map_or(0, |s| s.totals.total);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Memory lists").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} total", size_plain(total))).weak());
            });
        });
        let parts = list_bar_parts(total, self.lists);
        let Some(lists) = self.lists.filter(|_| !parts.is_empty()) else {
            ui.label(RichText::new("Memory lists are not available").weak());
            return;
        };
        stacked_bar(ui, 14.0, &parts);
        let entries = legend(list_segments(total, lists));
        ui.horizontal_wrapped(|ui| {
            for (name, color, bytes) in entries {
                dot(ui, color);
                ui.label(RichText::new(format!("{name} {}", size_plain(bytes))).small());
                ui.add_space(4.0);
            }
        })
        .response
        .on_hover_text(
            "Active is memory in use by apps. Cached (standby) memory is counted as available: \
             Windows hands it out whenever a program needs memory.",
        );
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let count = self.sampler.as_ref().map_or(0, |s| s.processes.len());
        let selected = self.selection.len();
        let mut trim = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by name or pid")
                    .desired_width(150.0),
            );
            ui.checkbox(&mut self.grouped, "Group by name");
            trim = ui
                .add_enabled(
                    selected > 0 && !self.busy(),
                    egui::Button::new(format!("Trim selected ({selected})")),
                )
                .clicked();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{count} processes")).weak());
            });
        });
        if trim {
            let ctx = ui.ctx().clone();
            self.start_trim(&ctx);
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let used = sampler.totals.used;
        let grouped_rows: Vec<ProcInfo>;
        let source: &[ProcInfo] = if self.grouped {
            grouped_rows = group_by_name(&sampler.processes);
            &grouped_rows
        } else {
            &sampler.processes
        };
        let rows = sort_and_filter(source, self.sort_key, self.descending, &self.filter);

        // The table borrows the sampler, selection and exclusions, so record what was clicked
        // and apply it to `self` once the table is done.
        let mut sort_click: Option<SortKey> = None;
        let mut click: Option<Click> = None;
        let mut exclude_click: Option<String> = None;
        let (sort_key, descending, grouped) = (self.sort_key, self.descending, self.grouped);
        let (selection, exclusions) = (&self.selection, &self.exclusions);
        let own_pid = std::process::id();
        let modifiers = ui.input(|i| i.modifiers);

        TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .column(Column::exact(22.0))
            .column(Column::remainder().at_least(110.0).clip(true))
            .column(Column::exact(44.0))
            .column(Column::exact(78.0))
            .column(Column::exact(78.0))
            .column(Column::exact(116.0))
            .column(Column::exact(36.0))
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
                header.col(|ui| {
                    ui.label(RichText::new("Procs").small().strong());
                });
                header.col(|ui| {
                    sort_click = sortable(ui, "Working", SortKey::WorkingSet).or(sort_click)
                });
                header.col(|ui| {
                    sort_click = sortable(ui, "Private", SortKey::Private).or(sort_click)
                });
                header.col(|ui| {
                    ui.label(RichText::new("Share of used").small().strong())
                        .on_hover_text(
                            "Working set as a share of the memory in use (sort by Working)",
                        );
                });
                header.col(|ui| {
                    ui.label(RichText::new("Lock").small().strong())
                        .on_hover_text("Tick to keep this app from ever being trimmed");
                });
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let p = rows[row.index()];
                    let key = row_key(grouped, p);
                    let is_selected = selection.contains(&key);
                    row.set_selected(is_selected);
                    let reason = skip_reason(p.pid, &p.name, own_pid, exclusions);
                    row.col(|ui| {
                        let mut on = is_selected;
                        if ui.checkbox(&mut on, "").changed() {
                            click = Some(Click::Box(key.clone()));
                        }
                    });
                    row.col(|ui| {
                        let [r, g, b] = color_for(&p.name);
                        dot(ui, Color32::from_rgb(r, g, b));
                        let label = ui.add(egui::Label::new(&p.name).selectable(false).truncate());
                        if !grouped {
                            label.on_hover_text(format!("pid {}", p.pid));
                        }
                    });
                    row.col(|ui| cell(ui, p.count.to_string()));
                    row.col(|ui| cell(ui, format_bytes(p.working_set)));
                    row.col(|ui| cell(ui, format_bytes(p.private)));
                    row.col(|ui| {
                        let share = share_of(p.working_set, used);
                        let w = (ui.available_width() - 38.0).max(10.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 6.0), Sense::hover());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 3.0, ui.visuals().widgets.inactive.bg_fill);
                        let mut fill = rect;
                        fill.set_width(w * share.clamp(0.0, 1.0));
                        painter.rect_filled(fill, 3.0, ACCENT);
                        cell(ui, format!("{:.1}%", share * 100.0));
                    });
                    row.col(|ui| {
                        // System and shell processes are always protected, so they show as
                        // ticked and locked.
                        let locked = matches!(reason, Some(SkipReason::System | SkipReason::Own));
                        let mut protected = reason.is_some();
                        let tick = ui
                            .add_enabled(!locked, egui::Checkbox::without_text(&mut protected))
                            .on_hover_text("Tick to keep this app from ever being trimmed");
                        if tick.changed() {
                            exclude_click = Some(p.name.to_lowercase());
                        }
                    });
                    if row.response().clicked() && click.is_none() {
                        click = Some(Click::Row(key, modifiers));
                    }
                });
            });

        let visible: Vec<String> = match click {
            Some(Click::Row(..)) if modifiers.shift => {
                rows.iter().map(|p| row_key(grouped, p)).collect()
            }
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
            self.apply_sort(key);
        }
        if let Some(name) = exclude_click {
            self.toggle_exclusion(name);
        }
    }

    fn workbench_inspector(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = Vec2::new(2.0, 4.0);
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Selected, "Selected"),
                (Tab::System, "System"),
                (Tab::Rules, "Rules"),
            ] {
                if pill(ui, label, self.tab == tab).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(GAP - 2.0);
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        match self.tab {
            Tab::Selected => self.selected_tab(ui),
            Tab::System => {
                self.system_panel(ui);
                if let Some(status) = &self.status {
                    ui.separator();
                    ui.label(RichText::new(status).small());
                }
            }
            Tab::Rules => {
                self.auto_panel(ui);
                ui.separator();
                self.exclusions_list(ui);
            }
        }
    }

    fn selected_tab(&mut self, ui: &mut egui::Ui) {
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let grouped = self.grouped;
        let own_pid = std::process::id();
        let used = sampler.totals.used;

        let mut rows: HashSet<String> = HashSet::new();
        let (mut procs, mut ws, mut private) = (0usize, 0u64, 0u64);
        let mut first: Option<&ProcInfo> = None;
        let mut names: HashSet<String> = HashSet::new();
        for p in &sampler.processes {
            let key = row_key(grouped, p);
            if !self.selection.contains(&key) {
                continue;
            }
            rows.insert(key);
            procs += 1;
            ws += p.working_set;
            private += p.private;
            first.get_or_insert(p);
            if !matches!(
                skip_reason(p.pid, &p.name, own_pid, &self.exclusions),
                Some(SkipReason::System | SkipReason::Own)
            ) {
                names.insert(p.name.to_lowercase());
            }
        }
        let Some(first) = first else {
            ui.label(RichText::new("Nothing selected.").weak());
            ui.label(
                RichText::new(
                    "Click a row. Ctrl+click adds or removes one, Shift+click selects a range.",
                )
                .weak()
                .small(),
            );
            self.last_trim_line(ui);
            return;
        };
        let plan = plan_trim(
            &sampler.processes,
            self.selection.keys(),
            grouped,
            own_pid,
            &self.exclusions,
        );
        let title = if rows.len() == 1 {
            first.name.clone()
        } else {
            format!("{} items", rows.len())
        };
        let single_reason = (rows.len() == 1 && plan.targets.is_empty())
            .then(|| skip_reason(first.pid, &first.name, own_pid, &self.exclusions))
            .flatten();
        let all_protected = !names.is_empty() && names.iter().all(|n| self.exclusions.contains(n));

        ui.label(RichText::new(title).strong());
        for (label, value) in [
            ("Processes", procs.to_string()),
            ("Working set", format_bytes(ws)),
            ("Private", format_bytes(private)),
            (
                "Share of used",
                format!("{:.1}%", share_of(ws, used) * 100.0),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(value);
                });
            });
        }
        if let Some(reason) = single_reason {
            ui.label(RichText::new(reason.describe()).small().weak());
        } else if plan.skipped > 0 {
            ui.label(
                RichText::new(format!(
                    "{} protected {} will be skipped.",
                    plan.skipped,
                    if plan.skipped == 1 {
                        "process"
                    } else {
                        "processes"
                    }
                ))
                .small()
                .weak(),
            );
        }

        ui.add_space(GAP - 2.0);
        let busy = self.busy();
        let can_trim = !plan.targets.is_empty() && !busy;
        let mut trim = false;
        let mut protect: Option<bool> = None;
        ui.horizontal(|ui| {
            trim = ui
                .add_enabled(can_trim, egui::Button::new("Trim"))
                .on_hover_text("Moves the working set to the standby list; pages fault back in")
                .clicked();
            let label = if all_protected {
                "Unprotect"
            } else {
                "Protect"
            };
            if ui
                .add_enabled(!names.is_empty(), egui::Button::new(label))
                .clicked()
            {
                protect = Some(!all_protected);
            }
        });
        if busy && self.trim_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Trimming\u{2026}");
            });
        }
        self.last_trim_line(ui);

        if let Some(on) = protect {
            for name in names {
                if on {
                    self.exclusions.insert(name);
                } else {
                    self.exclusions.remove(&name);
                }
            }
        }
        if trim {
            let ctx = ui.ctx().clone();
            self.start_trim(&ctx);
        }
    }

    fn last_trim_line(&self, ui: &mut egui::Ui) {
        if let Some(text) = &self.last_trim {
            ui.add_space(2.0);
            ui.label(RichText::new("Last trim").strong());
            ui.label(RichText::new(text).small());
        }
    }

    /// Protected apps with a Remove button each.
    pub(super) fn exclusions_list(&mut self, ui: &mut egui::Ui) {
        let mut names: Vec<String> = self.exclusions.iter().cloned().collect();
        names.sort();
        ui.label(RichText::new(format!("Protected apps ({})", names.len())).strong());
        if names.is_empty() {
            ui.label(
                RichText::new(
                    "None. Tick Lock on an app in the table to keep it from being trimmed.",
                )
                .weak()
                .small(),
            );
        }
        for name in &names {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(name).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        self.exclusions.remove(name);
                    }
                });
            });
        }
    }

    /// Trims every trimmable process of the selected rows on a worker thread.
    fn start_trim(&mut self, ctx: &egui::Context) {
        if self.busy() || self.selection.len() == 0 {
            return;
        }
        // Plan from fresh numbers, then re-check each process again at execution time.
        self.refresh_now();
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let own_pid = std::process::id();
        let plan = plan_trim(
            &sampler.processes,
            self.selection.keys(),
            self.grouped,
            own_pid,
            &self.exclusions,
        );
        if plan.targets.is_empty() {
            self.last_trim = Some(format!(
                "Nothing to trim: skipped {} protected.",
                plan.skipped
            ));
            return;
        }
        let exclusions = self.exclusions.clone();
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let mut result = TrimResult {
                trimmed: 0,
                groups: plan.groups,
                before: 0,
                after: 0,
                skipped: plan.skipped,
                failed: 0,
            };
            for (pid, name) in &plan.targets {
                if skip_reason(*pid, name, own_pid, &exclusions).is_some() {
                    result.skipped += 1;
                    continue;
                }
                match trim_process(*pid) {
                    Ok(t) => {
                        result.trimmed += 1;
                        result.before += t.before;
                        result.after += t.after;
                    }
                    Err(_) => result.failed += 1,
                }
            }
            let _ = tx.send(result);
            wake.request_repaint();
        });
        self.trim_job = Some(TrimJob { rx });
    }

    /// Picks up a finished trim run. Runs every frame via `background`.
    pub(super) fn poll_trim(&mut self, ctx: &egui::Context) {
        let outcome = match self.trim_job.as_ref() {
            None => return,
            Some(job) => job.rx.try_recv(),
        };
        let text = match outcome {
            Ok(r) => {
                let mut text = trim_summary(r.trimmed, r.groups, r.before, r.after, r.skipped);
                if r.failed > 0 {
                    text += &format!(", {} could not be trimmed", r.failed);
                }
                text
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => "Trimming stopped unexpectedly.".to_string(),
        };
        self.trim_job = None;
        self.last_trim = Some(text);
        self.refresh_now();
    }
}
