//! The look shared by every tool: the dark graphite style, cards on an 8 px grid, tabs, a switch,
//! stat blocks and line graphs.

use eframe::egui::{self, Color32, Mesh, Pos2, RichText, Sense, Shape, Stroke, Vec2};

/// Space between cards, and the padding inside them.
pub const GAP: f32 = 8.0;
pub const CARD_PAD: f32 = 10.0;

pub const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
pub const GREEN: Color32 = Color32::from_rgb(52, 168, 107);
pub const AMBER: Color32 = Color32::from_rgb(232, 163, 23);
pub const RED: Color32 = Color32::from_rgb(214, 69, 65);

/// Graphite surfaces and text of the dark theme.
pub const WINDOW_BG: Color32 = Color32::from_rgb(0x13, 0x14, 0x16);
pub const STATUS_BG: Color32 = Color32::from_rgb(0x0f, 0x10, 0x12);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe6, 0xea);
pub const MUTED: Color32 = Color32::from_rgb(0x8d, 0x93, 0x9e);

/// Sets fonts sizes, spacing, rounding and the dark visuals. Call once at start-up.
pub fn apply_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};

    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(10.5, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(11.5, FontFamily::Monospace),
            ),
        ]
        .into();

        let sp = &mut style.spacing;
        sp.item_spacing = Vec2::new(6.0, 4.0);
        sp.button_padding = Vec2::new(8.0, 3.0);
        sp.interact_size.y = 20.0;
        sp.slider_width = 110.0;
        sp.indent = 14.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = WINDOW_BG;
        v.window_fill = WINDOW_BG;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.extreme_bg_color = STATUS_BG;
        v.faint_bg_color = Color32::from_rgb(0x1b, 0x1d, 0x21);
        v.code_bg_color = Color32::from_rgb(0x1b, 0x1d, 0x21);
        v.hyperlink_color = ACCENT;
        v.weak_text_color = Some(MUTED);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_corner_radius = 6.0.into();
        v.menu_corner_radius = 4.0.into();

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = WINDOW_BG;
        w.noninteractive.weak_bg_fill = WINDOW_BG;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.bg_fill = Color32::from_rgb(0x2a, 0x2d, 0x33);
        w.inactive.weak_bg_fill = Color32::from_rgb(0x23, 0x26, 0x2b);
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_fill = Color32::from_rgb(0x34, 0x38, 0x40);
        w.hovered.weak_bg_fill = Color32::from_rgb(0x2c, 0x30, 0x37);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x42, 0x47, 0x50));
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.bg_fill = ACCENT;
        w.active.weak_bg_fill = ACCENT.gamma_multiply(0.6);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.open.bg_fill = Color32::from_rgb(0x2c, 0x30, 0x37);
        w.open.weak_bg_fill = Color32::from_rgb(0x2c, 0x30, 0x37);
        for wv in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            wv.corner_radius = 4.0.into();
        }
    });
}

/// A text tab of a tab strip: `height` tall, the selected one underlined 2 px in the accent,
/// a faint highlight on hover.
pub fn tab(ui: &mut egui::Ui, text: &str, selected: bool, height: f32) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = Vec2::new(galley.size().x + 24.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && !selected {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(8));
        }
        let color = if response.hovered() && !selected {
            ui.visuals().text_color()
        } else {
            color
        };
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
        if selected {
            let line = egui::Rect::from_min_max(
                Pos2::new(rect.left() + 6.0, rect.bottom() - 2.0),
                Pos2::new(rect.right() - 6.0, rect.bottom()),
            );
            painter.rect_filled(line, 1.0, ACCENT);
        }
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, text));
    response
}

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
        .corner_radius(4.0)
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
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(32.0, 18.0), Sense::click());
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
            .circle_filled(egui::pos2(x, rect.center().y), radius - 2.5, Color32::WHITE);
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
    let size = Vec2::new(galley.size().x + 18.0, 20.0);
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
        ui.label(RichText::new(value).size(16.0).strong());
        ui.label(RichText::new(label).weak().size(10.5));
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
    painter.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);

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

/// One piece of a [`stacked_bar`]: its share of the whole (0 to 1) and its color.
pub struct BarPart {
    pub fraction: f32,
    pub color: Color32,
}

/// One thick bar of the full available width. The parts sit left to right, sized by their
/// `fraction` of the whole, with 2 px gaps; the outer ends are rounded, the inner corners square
/// and tiny parts stay at least 3 px wide. Any unfilled remainder is drawn as the track color.
/// Returns the index of the part under the pointer, if any.
pub fn stacked_bar(ui: &mut egui::Ui, height: f32, parts: &[BarPart]) -> Option<usize> {
    const SPLIT: f32 = 2.0;
    const MIN_W: f32 = 3.0;
    const ROUND: u8 = 3;

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let n = parts.len();
    let usable = (rect.width() - SPLIT * n.saturating_sub(1) as f32).max(0.0);
    let widths: Vec<f32> = parts
        .iter()
        .map(|p| (p.fraction.clamp(0.0, 1.0) * usable).max(MIN_W))
        .collect();

    // (x range, color, part index); the remainder has no index.
    let mut pieces: Vec<(f32, f32, Color32, Option<usize>)> = Vec::new();
    let mut x = rect.left();
    for (i, (p, w)) in parts.iter().zip(&widths).enumerate() {
        let right = (x + w).min(rect.right());
        pieces.push((x, right, p.color, Some(i)));
        x = right + SPLIT;
    }
    if x < rect.right() - 1.0 {
        pieces.push((x, rect.right(), ui.visuals().widgets.inactive.bg_fill, None));
    }
    if let Some(last) = pieces.last_mut() {
        last.1 = rect.right();
    }

    let hovered = response.hover_pos().and_then(|pos| {
        pieces
            .iter()
            .find(|(l, r, _, _)| pos.x >= *l - SPLIT / 2.0 && pos.x <= *r + SPLIT / 2.0)
            .and_then(|piece| piece.3)
    });

    let painter = ui.painter_at(rect);
    let count = pieces.len();
    for (k, (l, r, color, _)) in pieces.iter().enumerate() {
        let (round_l, round_r) = (
            if k == 0 { ROUND } else { 0 },
            if k + 1 == count { ROUND } else { 0 },
        );
        let radius = egui::CornerRadius {
            nw: round_l,
            sw: round_l,
            ne: round_r,
            se: round_r,
        };
        let piece =
            egui::Rect::from_min_max(Pos2::new(*l, rect.top()), Pos2::new(*r, rect.bottom()));
        painter.rect_filled(piece, radius, *color);
    }
    hovered
}
