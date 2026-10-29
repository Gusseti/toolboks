//! The Debloater pages: a centered item list and a right-hand Review inspector.

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};

use super::model::{Applicable, Edition, Group, Item, Risk, State, Status};
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
                    .show(ui, |ui| self.inspector(ui));
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
                ui.centered_and_justified(|ui| {
                    ui.weak("App removal comes in a later step.");
                });
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
                if elevation::relaunch_elevated() {
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
        // Phase 1: applying is not enabled yet.
        ui.add_enabled(
            false,
            egui::Button::new(format!(
                "Apply {}",
                plural(review.changes, "change", "changes")
            )),
        );
        ui.label(
            RichText::new("Applying changes comes in the next step")
                .small()
                .weak(),
        );

        ui.add_space(GAP);
        let can_undo = self.last_run.is_some();
        if ui
            .add_enabled(can_undo, egui::Button::new("Undo last run"))
            .clicked()
        {
            self.undo_last_run();
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

    fn undo_last_run(&mut self) {
        let Some(run) = self.last_run.take() else {
            return;
        };
        let results = plan::undo(&run, &mut backend::WinRegistry);
        let failed = results.iter().filter(|(_, r)| r.is_err()).count();
        self.message = Some(if failed == 0 {
            format!("Undone: {}.", plural(results.len(), "value", "values"))
        } else {
            format!("Undo: {failed} of {} failed.", results.len())
        });
        self.refresh();
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

    #[test]
    fn plural_and_edition_labels() {
        assert_eq!(plural(1, "change", "changes"), "1 change");
        assert_eq!(plural(3, "change", "changes"), "3 changes");
        assert_eq!(edition_label(Edition::Pro), "Windows Pro");
        assert_eq!(edition_label(Edition::Unknown), "Windows");
    }
}
