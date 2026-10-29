//! The look shared by every tool: cards on a 12 px grid, a switch, stat blocks and line graphs.

use eframe::egui::{self, Color32, Mesh, Pos2, RichText, Sense, Shape, Stroke, Vec2};

/// Space between cards, and the padding inside them.
pub const GAP: f32 = 12.0;
pub const CARD_PAD: f32 = 16.0;

pub const ACCENT: Color32 = Color32::from_rgb(38, 110, 214);
pub const GREEN: Color32 = Color32::from_rgb(52, 168, 107);
pub const AMBER: Color32 = Color32::from_rgb(232, 163, 23);
pub const RED: Color32 = Color32::from_rgb(214, 69, 65);

/// A rounded card of exactly `width` (including its border), at least `min_height` tall.
pub fn card(ui: &mut egui::Ui, width: f32, min_height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let visuals = ui.visuals();
    let fill = visuals
        .panel_fill
        .lerp_to_gamma(visuals.strong_text_color(), 0.05);
    let stroke = visuals.widgets.noninteractive.bg_stroke;
    let inset = 2.0 * (CARD_PAD + stroke.width);

    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(12.0)
        .inner_margin(CARD_PAD)
        .show(ui, |ui| {
            ui.set_width(width - inset);
            ui.set_min_height((min_height - inset).max(0.0));
            // Tighter than the gap between cards, so the cards' heights are predictable.
            ui.spacing_mut().item_spacing.y = 6.0;
            add(ui);
        });
}

/// An on/off switch.
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(38.0, 22.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    if ui.is_rect_visible(rect) {
        let radius = rect.height() / 2.0;
        let fill = ui
            .visuals()
            .widgets
            .inactive
            .bg_fill
            .lerp_to_gamma(ACCENT, t);
        ui.painter().rect_filled(rect, radius, fill);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        ui.painter()
            .circle_filled(egui::pos2(x, rect.center().y), radius - 3.0, Color32::WHITE);
    }
    response
}

/// A fully rounded chip that acts as one choice of a group; `selected` tints it with the accent.
pub fn pill(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = Vec2::new(galley.size().x + 24.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 70)
        } else if response.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(pos, galley, color);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, text));
    response
}

/// A big number with a small label under it.
pub fn stat(ui: &mut egui::Ui, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(value).size(20.0).strong());
        ui.label(RichText::new(label).weak().small());
    });
}

/// One line of a graph: values oldest first.
pub struct Line<'a> {
    pub values: &'a [f32],
    pub color: Color32,
    /// Tint the area under the line.
    pub fill: bool,
}

/// A line graph. The full width shows `capacity` points with the newest at the right, so a
/// graph that is still filling up grows in from the right. `max` is the value at the top.
pub fn graph(ui: &mut egui::Ui, height: f32, lines: &[Line], capacity: usize, max: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, ui.visuals().extreme_bg_color);

    let grid = Stroke::new(
        1.0,
        ui.visuals()
            .widgets
            .noninteractive
            .bg_stroke
            .color
            .gamma_multiply(0.6),
    );
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            grid,
        );
    }

    let step = rect.width() / capacity.saturating_sub(1).max(1) as f32;
    let max = max.max(f32::EPSILON);
    for line in lines {
        let n = line.values.len();
        if n < 2 {
            continue;
        }
        let points: Vec<Pos2> = line
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = rect.right() - (n - 1 - i) as f32 * step;
                let y = rect.bottom() - (v / max).clamp(0.0, 1.0) * (rect.height() - 2.0) - 1.0;
                Pos2::new(x, y)
            })
            .collect();

        if line.fill {
            // The area under a line is not convex, so build it from one quad per segment.
            let tint = line.color.gamma_multiply(0.22);
            let mut mesh = Mesh::default();
            for pair in points.windows(2) {
                let base = mesh.vertices.len() as u32;
                mesh.colored_vertex(pair[0], tint);
                mesh.colored_vertex(pair[1], tint);
                mesh.colored_vertex(Pos2::new(pair[0].x, rect.bottom()), tint);
                mesh.colored_vertex(Pos2::new(pair[1].x, rect.bottom()), tint);
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base + 1, base + 3, base + 2);
            }
            painter.add(Shape::mesh(mesh));
        }
        painter.add(Shape::line(points, Stroke::new(1.8, line.color)));
    }
}
