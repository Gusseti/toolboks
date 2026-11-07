//! Small pieces shared by the memory pages, built on the tokens of `crate::ui`.

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Vec2};

use crate::ui::{card, BORDER, CARD_RADIUS, CONTROL_RADIUS, PANEL_BG, TEXT_SECTION};

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

/// A slim meter of the full available width: a track and a filled part (0 to 1).
pub fn meter(ui: &mut egui::Ui, height: f32, fraction: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let radius = height / 2.0;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, radius, ui.visuals().widgets.inactive.bg_fill);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    painter.rect_filled(fill, radius, color);
}

/// A one-line muted banner that can be dismissed. Returns true when dismissed.
pub fn banner(ui: &mut egui::Ui, text: &str) -> bool {
    let mut dismissed = false;
    egui::Frame::new()
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(CARD_RADIUS)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(text).weak());
                dismissed = ui.small_button("Got it").clicked();
            });
        });
    dismissed
}

/// The primary button: accent fill, full width, one per view.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
        .fill(crate::ui::HOST_ACCENT_FILL)
        .min_size(Vec2::new(ui.available_width(), 28.0))
        .corner_radius(CONTROL_RADIUS);
    ui.add_enabled(enabled, button)
}
