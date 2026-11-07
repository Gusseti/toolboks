//! Small pieces shared by the memory pages, built on the tokens of `crate::ui`.

use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};

use crate::ui::{card, CONTROL_RADIUS, TEXT_SECTION};

/// A card as wide as the space left, at least `min_height` tall.
pub fn panel(ui: &mut egui::Ui, min_height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    card(ui, width, min_height, add);
}

/// A section title: 14, semibold, sentence case.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(TEXT_SECTION).strong());
}

/// A section title with a muted note at the right.
pub fn section_with_note(ui: &mut egui::Ui, text: &str, note: &str) {
    ui.horizontal(|ui| {
        section(ui, text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(note).weak());
        });
    });
}

/// A muted label at the left and its value at the right.
pub fn stat_row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).weak());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(value.into());
        });
    });
}

/// The primary button: accent fill, full width, one per view.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
        .fill(crate::ui::HOST_ACCENT_FILL)
        .min_size(Vec2::new(ui.available_width(), 28.0))
        .corner_radius(CONTROL_RADIUS);
    ui.add_enabled(enabled, button)
}
