//! Design tokens and small helpers for Disk Manager, mirroring DESIGN.md.
//!
//! The add-on crate cannot import the host's `src/ui.rs`, so the token values are
//! repeated here. Keep them identical to DESIGN.md (Tokens) and `ui::apply_style`;
//! change them there first. Nothing here draws its own palette, font or icon style.
use egui::{Color32, RichText, Vec2};

pub const WINDOW: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x11);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x16, 0x1a);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x24, 0x26, 0x2c);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa8);
pub const ACCENT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
/// Text on a filled data colour (tiles, bars). Dark on every data colour, above 4.5:1.
pub const ON_DATA: Color32 = WINDOW;

/// Data colours used for ranked tiles and bars. Always shown next to a label.
pub const DATA: [Color32; 6] = [
    Color32::from_rgb(0x8b, 0x5c, 0xf6),
    Color32::from_rgb(0xb3, 0x9d, 0xfa),
    Color32::from_rgb(0x3e, 0xcf, 0x8e),
    Color32::from_rgb(0xf0, 0xa8, 0x3a),
    Color32::from_rgb(0x45, 0xb8, 0xf0),
    Color32::from_rgb(0x9a, 0x9e, 0xa8),
];

pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;

/// Card: 10 px radius, 1 px hairline, no shadow, no coloured edge.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(10)
        .inner_margin(12)
}

/// Page title: 18 px, once per page.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(18.0).strong());
}

/// Section heading inside a card: 14 px.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).strong());
}

/// Muted body text.
pub fn weak(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

/// Muted 11 px caption.
pub fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(11.0).color(MUTED));
}

/// Inline stat: value in strong text, then a small muted label.
pub fn stat(ui: &mut egui::Ui, value: impl Into<String>, label: &str) {
    ui.label(RichText::new(value.into()).strong());
    ui.label(RichText::new(label).size(11.0).color(MUTED));
    ui.add_space(SPACE_M);
}

/// Primary button: accent fill, one per view.
pub fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
            .fill(ACCENT)
            .corner_radius(6)
            .min_size(Vec2::new(75.0, 28.0)),
    )
}

/// Secondary button: outlined, no fill.
pub fn outlined(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(6)
}

/// One-line, muted, dismissible banner. Returns true when dismissed.
pub fn banner(ui: &mut egui::Ui, text: &str) -> bool {
    let mut dismissed = false;
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            weak(ui, text);
            if ui.small_button("Dismiss").clicked() {
                dismissed = true;
            }
        });
    });
    dismissed
}

/// Table header cell text: 11 px muted, sentence case.
pub fn header_cell(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}
