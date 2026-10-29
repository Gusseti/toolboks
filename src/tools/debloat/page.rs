//! The Debloater pages: a centered item list and a right-hand Review inspector.

use std::sync::mpsc;

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

use super::apps::{self, AppRow, AppRun};
use super::model::{Applicable, Edition, Group, Hive, Item, RegValue, Risk, State, Status};
use super::{backend, catalog, plan, Debloater};
use crate::elevation;
use crate::ui::{AMBER, CARD_PAD, GAP, GREEN};

const INSPECTOR_WIDTH: f32 = 190.0;
const INSPECTOR_PAD: f32 = 8.0;
const ROW_HEIGHT: f32 = 22.0;

/// Text for the state column of a row.
fn state_text(status: Status) -> &'static str {
    if status.applicable == Applicable::EditionNotSupported {
        return "Not on this edition";
    }
    match (status.state, status.applicable) {
        (State::Off, _) => "Already off",
        (State::Unknown, _) => "Unknown",
        (State::Active, Applicable::NeedsAdmin) => "Needs admin",
        (State::Active, _) => "Now on",
    }
}

/// Only features that are still on, and that this edition supports, can be ticked.
fn tickable(status: Status) -> bool {
    status.state == State::Active && status.applicable != Applicable::EditionNotSupported
}

/// The privacy and ads items split by risk: (safe, careful), catalog order kept.
fn group_by_risk(items: &[Item]) -> (Vec<&Item>, Vec<&Item>) {
    let mut safe = Vec::new();
    let mut careful = Vec::new();
    for item in items.iter().filter(|i| i.group == Group::PrivacyAds) {
        match item.risk {
            Risk::Safe => safe.push(item),
            Risk::Careful => careful.push(item),
        }
    }
    (safe, careful)
}

fn edition_label(edition: Edition) -> &'static str {
    match edition {
        Edition::Unknown => "Windows",
        Edition::Home => "Windows Home",
        Edition::Pro => "Windows Pro",
        Edition::Enterprise => "Windows Enterprise",
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn reversibility(item: &Item) -> &'static str {
    if item.needs_admin() {
        "Can be switched back by undoing, or by turning the setting on again in Windows. Needs administrator rights."
    } else {
        "Can be switched back by undoing, or by turning the setting on again in Windows."
    }
}

fn risk_color(risk: Risk) -> Color32 {
    match risk {
        Risk::Safe => GREEN.gamma_multiply(0.8),
        Risk::Careful => AMBER,
    }
}

fn risk_text(risk: Risk) -> &'static str {
    match risk {
        Risk::Safe => "Safe",
        Risk::Careful => "Careful",
    }
}

impl Debloater {
    fn refresh(&mut self) {
        self.statuses = plan::statuses(catalog::items(), &backend::WinRegistry, self.env);
        self.loaded = true;
        // Drop ticks on items that can no longer be changed.
        let statuses = &self.statuses;
        self.selected.retain(|id| {
            statuses
                .iter()
                .find(|s| s.id == *id)
                .is_some_and(|s| tickable(*s))
        });
    }

    fn status_of(&self, id: &str) -> Option<Status> {
        self.statuses.iter().find(|s| s.id == id).copied()
    }

    pub(super) fn page_ui(&mut self, ui: &mut egui::Ui, page: usize) {
        if !self.loaded {
            self.refresh();
        }

        if page == 1 {
            self.apps_poll();
            if !self.apps.started {
                self.apps_refresh(ui.ctx());
            }
        }

        let inspector = egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(INSPECTOR_PAD);
        let panel = egui::Panel::right("debloat_inspector")
            .resizable(false)
            .exact_size(INSPECTOR_WIDTH)
            .frame(inspector)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if page == 0 {
                            self.inspector(ui);
                        } else {
                            self.apps_inspector(ui);
                        }
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
            if page == 0 {
                self.privacy_page(ui);
            } else {
                self.apps_page(ui);
            }
        });
    }

    fn privacy_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let admin = if self.env.elevated {
                "Running as administrator"
            } else {
                "Not running as administrator"
            };
            ui.label(
                RichText::new(format!(
                    "{} \u{b7} {admin}",
                    edition_label(self.env.edition)
                ))
                .small()
                .weak(),
            );
            if ui.link(RichText::new("Refresh").small()).clicked() {
                self.refresh();
            }
        });
        if let Some(message) = &self.message {
            ui.label(RichText::new(message).small().weak());
        }
        ui.add_space(GAP - 2.0);

        let (safe, careful) = group_by_risk(catalog::items());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if safe.is_empty() && careful.is_empty() {
                    ui.weak("No items to show.");
                    return;
                }
                for (title, list) in [("Safe", safe), ("Careful", careful)] {
                    if list.is_empty() {
                        continue;
                    }
                    ui.add_space(2.0);
                    ui.label(RichText::new(title).strong());
                    for item in list {
                        self.item_row(ui, item);
                    }
                    ui.add_space(GAP);
                }
                self.about_item(ui);
            });
    }

    fn item_row(&mut self, ui: &mut egui::Ui, item: &Item) {
        let status = self.status_of(item.id).unwrap_or(Status {
            id: item.id,
            state: State::Unknown,
            applicable: Applicable::Yes,
        });
        let can_tick = tickable(status);
        let focused = self.focus == Some(item.id);

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        if focused || response.hovered() {
            let fill = if focused {
                ui.visuals().widgets.active.weak_bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 3.0, fill);
        }
        if response.clicked() {
            self.focus = Some(item.id);
        }

        let mut ticked = self.selected.contains(item.id);
        let dim = if can_tick {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let state_color = ui.visuals().weak_text_color();
        let mut checkbox_clicked = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_enabled_ui(can_tick, |ui| {
                    checkbox_clicked = ui.add(egui::Checkbox::without_text(&mut ticked)).changed();
                });
                ui.add(egui::Label::new(RichText::new(item.title).color(dim)).selectable(false));
                ui.add(
                    egui::Label::new(
                        RichText::new(risk_text(item.risk))
                            .small()
                            .color(risk_color(item.risk)),
                    )
                    .selectable(false),
                );
            });
        });
        ui.painter().text(
            rect.right_center() - Vec2::new(4.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            state_text(status),
            egui::TextStyle::Small.resolve(ui.style()),
            state_color,
        );
        if checkbox_clicked {
            self.focus = Some(item.id);
            if ticked {
                self.selected.insert(item.id);
            } else {
                self.selected.remove(item.id);
            }
        }
    }

    fn about_item(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("About this item").strong());
        let item = self
            .focus
            .and_then(|id| catalog::items().iter().find(|i| i.id == id));
        let Some(item) = item else {
            ui.weak("Click an item to see what it changes.");
            return;
        };
        ui.label(RichText::new(item.title).strong());
        ui.label(item.what_changes);
        ui.label(RichText::new("What could break").small().weak());
        ui.label(item.could_break);
        ui.label(RichText::new(reversibility(item)).small().weak());
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new("Review").strong());

        let ids: Vec<&str> = self.selected.iter().copied().collect();
        let review = plan::review(catalog::items(), &self.statuses, &ids);
        if review.changes == 0 {
            ui.label("No changes");
        } else {
            ui.label(plural(review.changes, "change", "changes"));
            ui.label(
                RichText::new(format!("{} safe, {} careful", review.safe, review.careful))
                    .small()
                    .weak(),
            );
        }
        if review.needs_admin > 0 {
            ui.add_space(2.0);
            ui.label(format!("{} need administrator rights", review.needs_admin));
            if ui.button("Restart as admin").clicked() {
                if elevation::relaunch_elevated_with_args(&["--open", "Debloater/Privacy and ads"])
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.message = Some("Elevation was cancelled.".to_string());
                }
            }
            ui.label(
                RichText::new("Undo history is lost when restarting")
                    .small()
                    .weak(),
            );
        }

        ui.add_space(GAP);
        let applicable = review.changes - review.needs_admin;
        if applicable == 0 {
            self.confirming = false;
        }
        let apply_label = format!("Apply {}", plural(applicable, "change", "changes"));
        if self.confirming {
            ui.label(format!(
                "Apply {}? You can undo them until you close toolboks.",
                plural(applicable, "change", "changes")
            ));
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    self.apply_selected(&ids);
                }
                if ui.button("Cancel").clicked() {
                    self.confirming = false;
                }
            });
        } else if ui
            .add_enabled(applicable > 0, egui::Button::new(apply_label))
            .clicked()
        {
            self.confirming = true;
        }
        if review.needs_admin > 0 && !self.confirming {
            ui.label(
                RichText::new(format!(
                    "{} need admin and will be skipped",
                    review.needs_admin
                ))
                .small()
                .weak(),
            );
        }

        for note in &self.run_notes {
            ui.label(RichText::new(note).small().color(AMBER));
        }

        ui.add_space(GAP);
        let can_undo = self.last_run.is_some();
        if ui
            .add_enabled(can_undo, egui::Button::new("Undo last run"))
            .clicked()
        {
            self.undo_last_run();
        }
        if can_undo && ui.link(RichText::new("Copy undo steps").small()).clicked() {
            if let Some(run) = &self.last_run {
                ctx.copy_text(undo_text(run));
            }
        }
        ui.label(
            RichText::new(
                "Undo works until you close toolboks. Removed apps come back from the \
                 Microsoft Store.",
            )
            .small()
            .weak(),
        );
    }

    fn apply_selected(&mut self, ids: &[&str]) {
        self.confirming = false;
        let run = plan::apply(catalog::items(), ids, &mut backend::WinRegistry, self.env);
        self.message = Some(summarize(&run));
        self.run_notes = run_notes(&run);
        // Keep an earlier run's undo steps if this one changed nothing.
        if !run.undo.is_empty() {
            self.last_run = Some(run);
        }
        // Refreshing also clears the ticks of items that are now off.
        self.refresh();
    }

    fn undo_last_run(&mut self) {
        let Some(run) = self.last_run.take() else {
            return;
        };
        let results = plan::undo(&run, &mut backend::WinRegistry);
        let failed = results.iter().filter(|(_, r)| r.is_err()).count();
        self.message = Some(undo_summary(results.len(), failed));
        self.run_notes.clear();
        if failed > 0 {
            // Keep the steps so the user can try again or copy them.
            self.last_run = Some(run);
        }
        self.refresh();
    }
}

/// One line for the result of an apply run.
fn summarize(run: &plan::Run) -> String {
    let count =
        |f: fn(&plan::Outcome) -> bool| run.applied.iter().filter(|a| f(&a.outcome)).count();
    let done = count(|o| matches!(o, plan::Outcome::Done));
    let ineffective = count(|o| matches!(o, plan::Outcome::NotEffective));
    let failed = count(|o| matches!(o, plan::Outcome::Failed(_)));
    format!(
        "Done: {done}, not effective: {ineffective} (Windows ignores it on this edition), \
         failed: {failed}"
    )
}

fn undo_summary(total: usize, failed: usize) -> String {
    if failed == 0 {
        format!("Restored {}.", plural(total, "setting", "settings"))
    } else {
        format!(
            "Restored {} of {}, {failed} failed. Undo is still available.",
            total - failed,
            plural(total, "setting", "settings")
        )
    }
}

fn title_of(id: &str) -> String {
    catalog::items()
        .iter()
        .find(|i| i.id == id)
        .map_or_else(|| id.to_string(), |i| i.title.to_string())
}

/// Plain-words lines for every item that did not end up fully done.
fn run_notes(run: &plan::Run) -> Vec<String> {
    run.applied
        .iter()
        .filter_map(|a| match &a.outcome {
            plan::Outcome::Done => None,
            plan::Outcome::NotEffective => Some(format!(
                "{}: saved, but Windows ignores it on this edition.",
                title_of(a.id)
            )),
            plan::Outcome::Failed(e) => {
                Some(format!("{}: could not be changed ({e}).", title_of(a.id)))
            }
        })
        .collect()
}

fn hive_text(hive: Hive) -> &'static str {
    match hive {
        Hive::CurrentUser => "HKEY_CURRENT_USER",
        Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
    }
}

/// A plain text list of what to put back by hand: item, setting path, previous value.
fn undo_text(run: &plan::Run) -> String {
    let mut out = String::from("toolboks undo steps\n");
    for op in &run.undo {
        let previous = match &op.previous {
            Some(RegValue::Dword(v)) => format!("number {v}"),
            Some(RegValue::Sz(s)) => format!("text \"{s}\""),
            None => "not set (delete the value)".to_string(),
        };
        out.push_str(&format!(
            "\n{}\n  {}\\{}\n  value: {}\n  previous: {previous}\n",
            title_of(op.id),
            hive_text(op.hive),
            op.path,
            op.name
        ));
    }
    out
}

const PROTECTED_NOTE: &str = "Store, Edge, Windows security, Photos, Calculator, Xbox services, \
                              and system components are never offered.";
const REINSTALL_NOTE: &str = "Removed apps can be reinstalled from the Microsoft Store. Removal applies to your account only.";

fn app_state_text(installed: bool) -> &'static str {
    if installed {
        "Installed"
    } else {
        "Not installed"
    }
}

/// Result line for a removal run, plus the failed apps with their reason. `title` maps an app id
/// to its display title.
fn app_summary(run: &AppRun, title: impl Fn(&str) -> String) -> String {
    let mut out = format!("Removed {}, failed {}", run.removed.len(), run.failed.len());
    for (id, reason) in &run.failed {
        out.push_str(&format!("\n{}: {reason}", title(id)));
    }
    out
}

fn app_title(id: &str) -> String {
    apps::curated()
        .iter()
        .find(|c| c.id == id)
        .map_or_else(|| id.to_string(), |c| c.title.to_string())
}

/// Opens the Microsoft Store start page.
fn open_store() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", "ms-windows-store://home"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

impl Debloater {
    /// Starts reading the installed apps on a worker thread (no-op while one is running).
    fn apps_refresh(&mut self, ctx: &egui::Context) {
        self.apps.started = true;
        if self.apps.loading.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(apps::rows(&apps::WinPackages));
            ctx.request_repaint();
        });
        self.apps.loading = Some(rx);
    }

    /// Picks up finished worker results.
    fn apps_poll(&mut self) {
        if let Some(rx) = &self.apps.loading {
            match rx.try_recv() {
                Ok(result) => {
                    self.apps.loading = None;
                    match result {
                        Ok(rows) => {
                            self.apps.rows = rows;
                            self.apps.error = None;
                            let rows = &self.apps.rows;
                            self.apps.selected.retain(|id| {
                                rows.iter()
                                    .any(|r| r.entry.id == *id && r.installed.is_some())
                            });
                        }
                        Err(e) => self.apps.error = Some(e),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.apps.loading = None;
                    self.apps.error = Some("Reading the apps stopped unexpectedly.".to_string());
                }
            }
        }
        if let Some(rx) = &self.apps.removing {
            match rx.try_recv() {
                Ok(run) => {
                    self.apps.removing = None;
                    self.apps.message = Some(app_summary(&run, app_title));
                    self.apps.last_run = Some(run);
                    self.apps.selected.clear();
                    self.apps.started = false; // reload right away
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.apps.removing = None;
                    self.apps.message = Some("Removal stopped unexpectedly.".to_string());
                    self.apps.started = false;
                }
            }
        }
    }

    fn apps_start_remove(&mut self, ctx: &egui::Context) {
        self.apps.confirming = false;
        if self.apps.removing.is_some() {
            return;
        }
        let rows = self.apps.rows.clone();
        let ids: Vec<&'static str> = self.apps.selected.iter().copied().collect();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let run = apps::remove_apps(&rows, &ids, &mut apps::WinPackages);
            let _ = tx.send(run);
            ctx.request_repaint();
        });
        self.apps.removing = Some(rx);
    }

    fn apps_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Apps for your account").small().weak());
            let busy = self.apps.loading.is_some() || self.apps.removing.is_some();
            if ui
                .add_enabled(!busy, egui::Link::new(RichText::new("Refresh").small()))
                .clicked()
            {
                self.apps_refresh(ui.ctx());
            }
        });
        ui.add_space(GAP - 2.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(error) = &self.apps.error {
                    ui.label(error.as_str());
                } else if self.apps.loading.is_some() && self.apps.rows.is_empty() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Loading apps...");
                    });
                } else if self.apps.rows.is_empty() {
                    ui.weak("No apps to show.");
                }
                let rows = self.apps.rows.clone();
                for row in &rows {
                    self.app_row(ui, row);
                }
                ui.add_space(GAP);
                ui.label(RichText::new("Protected by toolboks").small().weak());
                ui.label(RichText::new(PROTECTED_NOTE).small().weak());
                ui.add_space(GAP);
                self.about_app(ui);
            });
    }

    fn app_row(&mut self, ui: &mut egui::Ui, row: &AppRow) {
        let id = row.entry.id;
        let installed = row.installed.is_some();
        let focused = self.apps.focus == Some(id);

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        if focused || response.hovered() {
            let fill = if focused {
                ui.visuals().widgets.active.weak_bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 3.0, fill);
        }
        if response.clicked() {
            self.apps.focus = Some(id);
        }

        let mut ticked = self.apps.selected.contains(id);
        let text_color = if installed {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let state_color = ui.visuals().weak_text_color();
        let mut checkbox_clicked = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_enabled_ui(installed, |ui| {
                    checkbox_clicked = ui.add(egui::Checkbox::without_text(&mut ticked)).changed();
                });
                ui.add(
                    egui::Label::new(RichText::new(row.entry.title).color(text_color))
                        .selectable(false),
                );
            });
        });
        ui.painter().text(
            rect.right_center() - Vec2::new(4.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            app_state_text(installed),
            egui::TextStyle::Small.resolve(ui.style()),
            state_color,
        );
        if checkbox_clicked {
            self.apps.focus = Some(id);
            if ticked {
                self.apps.selected.insert(id);
            } else {
                self.apps.selected.remove(id);
            }
        }
    }

    fn about_app(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("About this app").strong());
        let entry = self
            .apps
            .focus
            .and_then(|id| self.apps.rows.iter().find(|r| r.entry.id == id))
            .map(|r| r.entry);
        let Some(entry) = entry else {
            ui.weak("Click an app to see what removing it changes.");
            return;
        };
        ui.label(RichText::new(entry.title).strong());
        ui.label(entry.what_changes);
        ui.label(RichText::new("What could break").small().weak());
        ui.label(entry.could_break);
    }

    fn apps_inspector(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new("Review").strong());

        let count = self.apps.selected.len();
        ui.label(plural(count, "app selected", "apps selected"));

        ui.add_space(GAP);
        let removing = self.apps.removing.is_some();
        if count == 0 || removing {
            self.apps.confirming = false;
        }
        if self.apps.confirming {
            ui.label(format!(
                "Remove {} for your account? Removed apps can be reinstalled from the \
                 Microsoft Store.",
                plural(count, "app", "apps")
            ));
            ui.horizontal(|ui| {
                if ui.button("Remove").clicked() {
                    self.apps_start_remove(&ctx);
                }
                if ui.button("Cancel").clicked() {
                    self.apps.confirming = false;
                }
            });
        } else if ui
            .add_enabled(
                count > 0 && !removing,
                egui::Button::new(format!("Remove {}", plural(count, "app", "apps"))),
            )
            .clicked()
        {
            self.apps.confirming = true;
        }
        if removing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Removing...");
            });
        }
        if let Some(message) = &self.apps.message {
            ui.label(RichText::new(message).small().weak());
        }

        ui.add_space(GAP);
        if ui
            .link(RichText::new("Open Microsoft Store").small())
            .clicked()
        {
            open_store();
        }
        ui.label(RichText::new(REINSTALL_NOTE).small().weak());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::debloat::model::Edition;

    fn item(id: &'static str, group: Group, risk: Risk) -> Item {
        Item {
            id,
            group,
            title: id,
            what_changes: "",
            could_break: "",
            risk,
            min_edition: Edition::Home,
            settings: &[],
        }
    }

    fn status(state: State, applicable: Applicable) -> Status {
        Status {
            id: "x",
            state,
            applicable,
        }
    }

    #[test]
    fn state_text_covers_every_case() {
        assert_eq!(state_text(status(State::Active, Applicable::Yes)), "Now on");
        assert_eq!(
            state_text(status(State::Off, Applicable::Yes)),
            "Already off"
        );
        assert_eq!(
            state_text(status(State::Active, Applicable::NeedsAdmin)),
            "Needs admin"
        );
        assert_eq!(
            state_text(status(State::Active, Applicable::EditionNotSupported)),
            "Not on this edition"
        );
        assert_eq!(
            state_text(status(State::Unknown, Applicable::Yes)),
            "Unknown"
        );
    }

    #[test]
    fn only_active_supported_items_are_tickable() {
        assert!(tickable(status(State::Active, Applicable::Yes)));
        assert!(tickable(status(State::Active, Applicable::NeedsAdmin)));
        assert!(!tickable(status(State::Off, Applicable::Yes)));
        assert!(!tickable(status(
            State::Active,
            Applicable::EditionNotSupported
        )));
        assert!(!tickable(status(State::Unknown, Applicable::Yes)));
    }

    #[test]
    fn group_by_risk_splits_and_skips_apps() {
        let items = [
            item("a", Group::PrivacyAds, Risk::Careful),
            item("b", Group::PrivacyAds, Risk::Safe),
            item("c", Group::Apps, Risk::Safe),
            item("d", Group::PrivacyAds, Risk::Safe),
        ];
        let (safe, careful) = group_by_risk(&items);
        assert_eq!(safe.iter().map(|i| i.id).collect::<Vec<_>>(), ["b", "d"]);
        assert_eq!(careful.iter().map(|i| i.id).collect::<Vec<_>>(), ["a"]);
        let (s, c) = group_by_risk(&[]);
        assert!(s.is_empty() && c.is_empty());
    }

    fn run_with(outcomes: &[plan::Outcome]) -> plan::Run {
        plan::Run {
            applied: outcomes
                .iter()
                .map(|o| plan::Applied {
                    id: "x",
                    outcome: o.clone(),
                })
                .collect(),
            undo: Vec::new(),
        }
    }

    #[test]
    fn summarize_counts_outcomes() {
        use plan::Outcome::*;
        let run = run_with(&[Done, Done, NotEffective, Failed("no".into())]);
        assert_eq!(
            summarize(&run),
            "Done: 2, not effective: 1 (Windows ignores it on this edition), failed: 1"
        );
        assert_eq!(run_notes(&run).len(), 2);
        assert!(summarize(&plan::Run::default()).starts_with("Done: 0"));
    }

    #[test]
    fn undo_summary_wording() {
        assert_eq!(undo_summary(3, 0), "Restored 3 settings.");
        assert_eq!(undo_summary(1, 0), "Restored 1 setting.");
        assert!(undo_summary(3, 1).starts_with("Restored 2 of 3 settings, 1 failed"));
    }

    #[test]
    fn undo_text_lists_previous_values() {
        let run = plan::Run {
            applied: Vec::new(),
            undo: vec![
                plan::UndoOp {
                    id: "unknown-id",
                    hive: Hive::CurrentUser,
                    path: r"Software\T",
                    name: "A",
                    previous: Some(RegValue::Dword(1)),
                },
                plan::UndoOp {
                    id: "unknown-id",
                    hive: Hive::LocalMachine,
                    path: r"Software\T",
                    name: "B",
                    previous: None,
                },
            ],
        };
        let text = undo_text(&run);
        assert!(text.contains("unknown-id"));
        assert!(text.contains(r"HKEY_CURRENT_USER\Software\T"));
        assert!(text.contains("value: A"));
        assert!(text.contains("previous: number 1"));
        assert!(text.contains("HKEY_LOCAL_MACHINE"));
        assert!(text.contains("previous: not set"));
    }

    #[test]
    fn app_summary_lists_failures() {
        let run = AppRun {
            removed: vec![("a", "A_1".into()), ("b", "B_1".into())],
            failed: vec![("c", "denied".into())],
        };
        let text = app_summary(&run, |id| id.to_uppercase());
        assert_eq!(text, "Removed 2, failed 1\nC: denied");
        assert_eq!(
            app_summary(&AppRun::default(), |id| id.into()),
            "Removed 0, failed 0"
        );
        assert_eq!(app_state_text(true), "Installed");
        assert_eq!(app_state_text(false), "Not installed");
    }

    #[test]
    fn plural_and_edition_labels() {
        assert_eq!(plural(1, "change", "changes"), "1 change");
        assert_eq!(plural(3, "change", "changes"), "3 changes");
        assert_eq!(edition_label(Edition::Pro), "Windows Pro");
        assert_eq!(edition_label(Edition::Unknown), "Windows");
    }
}
