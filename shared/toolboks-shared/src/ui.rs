//! The look shared by every tool: the dark graphite tokens of DESIGN.md, cards on an 8 px grid,
//! a segmented switch, a toggle, stat bars and line graphs.

use egui::{self, Color32, Mesh, Pos2, RichText, Sense, Shape, Stroke, Vec2};

/// Spacing scale (DESIGN.md: 4, 8, 12, 16). `GAP` is the space between cards.
pub const SPACE_XS: f32 = 4.0;
pub const GAP: f32 = 8.0;
#[allow(dead_code)] // used by the next slices
pub const SPACE_M: f32 = 12.0;
pub const SPACE_L: f32 = 16.0;
pub const CARD_PAD: f32 = 10.0;

/// Radii: cards 10, controls 6.
pub const CARD_RADIUS: u8 = 10;
pub const CONTROL_RADIUS: u8 = 6;

/// Type scale: page title, section, body, caption.
#[allow(dead_code)] // used by the next slices
pub const TEXT_TITLE: f32 = 18.0;
pub const TEXT_SECTION: f32 = 14.0;
pub const TEXT_BODY: f32 = 12.0;
pub const TEXT_CAPTION: f32 = 11.0;

/// Surfaces and text.
pub const WINDOW_BG: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x11);
/// Sidebar, title bar and status bar.
pub const BAR_BG: Color32 = Color32::from_rgb(0x0a, 0x0b, 0x0c);
pub const SIDEBAR_BG: Color32 = BAR_BG;
pub const STATUS_BG: Color32 = BAR_BG;
/// Cards and panels.
pub const PANEL_BG: Color32 = Color32::from_rgb(0x15, 0x16, 0x1a);
pub const SELECTED_BG: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x21);
/// Card borders and dividers.
pub const BORDER: Color32 = Color32::from_rgb(0x24, 0x26, 0x2c);
/// Divider between a bar and the content.
pub const BAR_DIVIDER: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x22);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
/// 6.2:1 on a card; never go darker.
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa8);

/// The one accent: selection and the CPU. `ACCENT_WASH` is 20 % of it (premultiplied).
pub const ACCENT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
pub const ACCENT_WASH: Color32 = Color32::from_rgba_premultiplied(28, 18, 49, 51);
pub const HOST_ACCENT: Color32 = ACCENT;
pub const HOST_ACCENT_FILL: Color32 = Color32::from_rgb(0x6d, 0x45, 0xc4);

/// Data colours, one per resource; always shown next to a label.
#[allow(dead_code)] // used by the next slices
pub const DATA_CPU: Color32 = ACCENT;
#[allow(dead_code)] // used by the next slices
pub const DATA_MEMORY: Color32 = Color32::from_rgb(0xb3, 0x9d, 0xfa);
#[allow(dead_code)] // used by the next slices
pub const DATA_GPU: Color32 = Color32::from_rgb(0x3e, 0xcf, 0x8e);
#[allow(dead_code)] // used by the next slices
pub const DATA_DISK: Color32 = Color32::from_rgb(0xf0, 0xa8, 0x3a);
#[allow(dead_code)] // used by the next slices
pub const DATA_NETWORK: Color32 = Color32::from_rgb(0x45, 0xb8, 0xf0);
/// A second line (write, up, estimate).
#[allow(dead_code)] // used by the next slices
pub const DATA_SECOND: Color32 = MUTED;

/// Status colours; always with a word.
pub const GREEN: Color32 = Color32::from_rgb(0x3e, 0xcf, 0x8e);
pub const AMBER: Color32 = Color32::from_rgb(0xf0, 0xa8, 0x3a);
pub const RED: Color32 = Color32::from_rgb(0xe5, 0x53, 0x4b);

/// Puts the Windows UI font first if it can be read from the system font folder (read only,
/// nothing is written). Otherwise egui's bundled font stays.
fn install_system_font(ctx: &egui::Context) {
    let Some(dir) = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")) else {
        return;
    };
    let fonts_dir = std::path::Path::new(&dir).join("Fonts");
    // Static Segoe UI: the variable font is not worth the risk of a parse failure at start-up.
    let Ok(bytes) = std::fs::read(fonts_dir.join("segoeui.ttf")) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "segoe-ui".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "segoe-ui".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Sets fonts sizes, spacing, rounding and the dark visuals. Call once at start-up.
pub fn apply_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};

    install_system_font(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(TEXT_CAPTION, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(TEXT_BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(TEXT_BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(TEXT_SECTION, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(TEXT_BODY, FontFamily::Monospace),
            ),
        ]
        .into();

        let sp = &mut style.spacing;
        sp.item_spacing = Vec2::new(GAP, SPACE_XS);
        sp.button_padding = Vec2::new(GAP, SPACE_XS);
        sp.interact_size.y = 24.0;
        sp.slider_width = 110.0;
        sp.indent = 14.0;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = WINDOW_BG;
        v.window_fill = PANEL_BG;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.extreme_bg_color = BAR_BG;
        v.faint_bg_color = PANEL_BG;
        v.code_bg_color = SELECTED_BG;
        v.hyperlink_color = ACCENT;
        v.weak_text_color = Some(MUTED);
        v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        // Also the keyboard focus ring.
        v.selection.stroke = Stroke::new(1.5, ACCENT);
        v.window_corner_radius = CARD_RADIUS.into();
        v.menu_corner_radius = 8.0.into();

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = WINDOW_BG;
        w.noninteractive.weak_bg_fill = WINDOW_BG;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.bg_fill = BORDER;
        w.inactive.weak_bg_fill = SELECTED_BG;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.bg_fill = Color32::from_rgb(0x2c, 0x2f, 0x36);
        w.hovered.weak_bg_fill = BORDER;
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x45));
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.bg_fill = ACCENT;
        w.active.weak_bg_fill = ACCENT.gamma_multiply(0.6);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.open.bg_fill = BORDER;
        w.open.weak_bg_fill = BORDER;
        for wv in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            wv.corner_radius = CONTROL_RADIUS.into();
        }
    });
}

/// A rounded card of exactly `width` (including its border), at least `min_height` tall.
pub fn card(ui: &mut egui::Ui, width: f32, min_height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let stroke = Stroke::new(1.0, BORDER);
    let fill = PANEL_BG;
    let inset = 2.0 * (CARD_PAD + stroke.width);

    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CARD_RADIUS)
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

/// One choice of a segmented group; `selected` gets the lighter fill and full-strength text.
pub fn pill(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if selected { TEXT } else { MUTED };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = Vec2::new(galley.size().x + 18.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            SELECTED_BG
        } else if response.hovered() {
            BAR_DIVIDER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CONTROL_RADIUS, fill);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                CONTROL_RADIUS,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(pos, galley, color);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, text));
    response
}

/// One line of a graph: values oldest first.
pub struct Line<'a> {
    pub values: &'a [f32],
    pub color: Color32,
    /// Tint the area under the line.
    pub fill: bool,
    /// Name and value formatter shown in the hover read-out.
    pub label: &'a str,
    pub fmt: fn(f32) -> String,
}

/// Space reserved at the top of a [`graph_card`] for its title, and the height of the info bar at
/// the bottom.
const TITLE_H: f32 = 22.0;
const BAR_H: f32 = 26.0;

/// A card that is one graph: the plot fills the whole card, the title sits over its top edge and
/// a slim bar along the bottom carries the numbers.
pub struct GraphCard<'a> {
    pub title: &'a str,
    pub detail: &'a str,
    pub lines: &'a [Line<'a>],
    /// How many points a full graph shows.
    pub capacity: usize,
    /// The value at the top of the plot.
    pub max: f32,
    /// Seconds between two points, for the hover read-out.
    pub step_secs: f32,
    /// `(value, label)` pairs in the bottom bar; the label may be empty.
    pub stats: &'a [(String, &'a str)],
    /// Shown instead of the lines when there is nothing to plot.
    pub note: Option<&'a str>,
}

/// "Now", "12 s ago", "2 min 10 s ago", "1 h 5 min ago".
pub fn ago_text(secs: f32) -> String {
    let s = secs.max(0.0).round() as u64;
    match s {
        0 => "Now".to_string(),
        1..=59 => format!("{s} s ago"),
        60..=3599 => match s % 60 {
            0 => format!("{} min ago", s / 60),
            r => format!("{} min {r} s ago", s / 60),
        },
        _ => match (s % 3600) / 60 {
            0 => format!("{} h ago", s / 3600),
            m => format!("{} h {m} min ago", s / 3600),
        },
    }
}

/// How many points back from the newest the pointer is at, or `None` when it is left of the data.
/// `right` is the x of the newest point and `step` the distance between points.
pub fn hover_offset(pointer_x: f32, right: f32, step: f32, points: usize) -> Option<usize> {
    if points < 2 || step <= 0.0 {
        return None;
    }
    let k = ((right - pointer_x) / step).round().max(0.0) as usize;
    (k < points).then_some(k)
}

fn paint_grid(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let grid = Stroke::new(1.0, color);
    for i in 1..4 {
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            grid,
        );
    }
}

fn point_y(rect: egui::Rect, v: f32, max: f32) -> f32 {
    rect.bottom() - (v / max).clamp(0.0, 1.0) * (rect.height() - 2.0) - 1.0
}

/// The lines of a graph inside `rect`; the newest point is at the right edge.
fn paint_lines(
    painter: &egui::Painter,
    rect: egui::Rect,
    lines: &[Line],
    capacity: usize,
    max: f32,
) {
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
                Pos2::new(x, point_y(rect, *v, max))
            })
            .collect();

        if line.fill {
            // The area under a line is not convex, so build it from one quad per segment.
            let tint = line.color.gamma_multiply(0.22);
            let mut mesh = Mesh::default();
            for pair in points
                .windows(2)
                .filter(|pair| pair.iter().all(|p| p.y.is_finite()))
            {
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
        for segment in points.split(|point| !point.y.is_finite()) {
            match segment {
                [] => {}
                [point] => {
                    painter.circle_filled(*point, 1.0, line.color);
                }
                _ => {
                    painter.add(Shape::line(segment.to_vec(), Stroke::new(1.8, line.color)));
                }
            }
        }
    }
}

/// Draws a [`GraphCard`] of exactly `width` x `height`, with a hover read-out over the plot.
pub fn graph_card(ui: &mut egui::Ui, width: f32, height: f32, g: &GraphCard) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    toolboks_addon_sdk::tutorial::mark(ui, &format!("graph-{}", g.title.to_lowercase()), rect);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let stroke = Stroke::new(1.0, BORDER);
    let grid_color = BAR_DIVIDER;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CARD_RADIUS, PANEL_BG);
    painter.rect_stroke(rect, CARD_RADIUS, stroke, egui::StrokeKind::Inside);

    let inner = rect.shrink(stroke.width);
    let bar = egui::Rect::from_min_max(
        Pos2::new(inner.left(), inner.bottom() - BAR_H),
        inner.right_bottom(),
    );
    let plot = egui::Rect::from_min_max(inner.left_top(), Pos2::new(inner.right(), bar.top()));
    let data = egui::Rect::from_min_max(
        Pos2::new(plot.left(), plot.top() + TITLE_H),
        plot.right_bottom(),
    );

    paint_grid(&painter, data, grid_color);
    if let Some(note) = g.note {
        painter.text(
            data.center(),
            egui::Align2::CENTER_CENTER,
            note,
            egui::TextStyle::Body.resolve(ui.style()),
            MUTED,
        );
    } else {
        paint_lines(&painter, data, g.lines, g.capacity, g.max);
    }

    // The bar along the bottom.
    painter.rect_filled(
        bar,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: CARD_RADIUS - 1,
            se: CARD_RADIUS - 1,
        },
        BAR_BG,
    );
    painter.line_segment(
        [bar.left_top(), bar.right_top()],
        Stroke::new(1.0, stroke.color),
    );
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut x = bar.left() + 10.0;
    for (value, label) in g.stats {
        let v = painter.layout_no_wrap(value.clone(), body.clone(), TEXT);
        let l = painter.layout_no_wrap((*label).to_string(), small.clone(), MUTED);
        let w = v.size().x
            + if label.is_empty() {
                0.0
            } else {
                4.0 + l.size().x
            };
        if x + w > bar.right() - 6.0 {
            break;
        }
        let mid = bar.center().y;
        let value_w = v.size().x;
        painter.galley(Pos2::new(x, mid - v.size().y / 2.0), v, TEXT);
        if !label.is_empty() {
            painter.galley(
                Pos2::new(x + value_w + 4.0, mid - l.size().y / 2.0 + 1.0),
                l,
                MUTED,
            );
        }
        x += w + 16.0;
    }

    // Title over the top edge, and the detail at the right.
    let mut title_right = plot.left() + 10.0;
    if !g.title.is_empty() {
        let t = painter.layout_no_wrap(g.title.to_string(), body.clone(), TEXT);
        title_right += t.size().x;
        painter.galley(Pos2::new(plot.left() + 10.0, plot.top() + 5.0), t, TEXT);
    }
    if !g.detail.is_empty() {
        let clip = egui::Rect::from_min_max(
            Pos2::new(title_right + 12.0, plot.top()),
            Pos2::new(plot.right() - 10.0, plot.top() + TITLE_H),
        );
        painter.with_clip_rect(clip).text(
            Pos2::new(plot.right() - 10.0, plot.top() + 5.0),
            egui::Align2::RIGHT_TOP,
            g.detail,
            body,
            MUTED,
        );
    }

    // Hover: a guide line, a dot on every line and a read-out.
    if g.note.is_some() {
        return;
    }
    let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) else {
        return;
    };
    let points = g.lines.iter().map(|l| l.values.len()).max().unwrap_or(0);
    let step = data.width() / g.capacity.saturating_sub(1).max(1) as f32;
    let Some(k) = hover_offset(pos.x, data.right(), step, points) else {
        return;
    };
    let x = data.right() - k as f32 * step;
    painter.line_segment(
        [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
        Stroke::new(1.0, Color32::from_white_alpha(70)),
    );
    let max = g.max.max(f32::EPSILON);
    let mut rows: Vec<(Color32, &str, String)> = Vec::new();
    for line in g.lines {
        let n = line.values.len();
        if k >= n {
            continue;
        }
        let v = line.values[n - 1 - k];
        if !v.is_finite() {
            continue;
        }
        painter.circle_filled(Pos2::new(x, point_y(data, v, max)), 3.0, line.color);
        rows.push((line.color, line.label, (line.fmt)(v)));
    }
    let ago = ago_text(k as f32 * g.step_secs);
    response.on_hover_ui_at_pointer(|ui| {
        ui.label(RichText::new(ago).weak().small());
        for (color, label, value) in &rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, *color);
                if !label.is_empty() {
                    ui.label(*label);
                }
                ui.label(RichText::new(value).strong());
            });
        }
    });
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

// Shared table, inspector and banner pieces (Resource Monitor lists, Debloater, Memory Cleaner).

/// The inspector panel's frame: a card-coloured side panel.
pub fn inspector_frame() -> egui::Frame {
    egui::Frame::NONE.fill(PANEL_BG).inner_margin(SPACE_M)
}

/// The hairline between the table and the inspector.
pub fn inspector_edge(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter()
        .vline(rect.left(), rect.y_range(), Stroke::new(1.0, BORDER));
}

/// Table look for the scope it is called in: a soft selection fill and a quiet hover fill.
pub fn table_style(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.selection.bg_fill = SELECTED_BG;
    v.widgets.hovered.bg_fill = BAR_DIVIDER;
}

/// Hairline under a table row, drawn across the whole row.
pub fn row_rule(painter: &egui::Painter, rect: egui::Rect) {
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, BAR_DIVIDER));
}

/// A sortable column header: muted caption, full-strength text on the sorted column.
pub fn header_cell(ui: &mut egui::Ui, label: &str, arrow: &str, active: bool, right: bool) -> bool {
    let color = if active { TEXT } else { MUTED };
    let button = |ui: &mut egui::Ui| {
        ui.add(
            egui::Button::new(
                RichText::new(format!("{label}{arrow}"))
                    .small()
                    .color(color),
            )
            .frame(false),
        )
        .clicked()
    };
    if right {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), button)
            .inner
    } else {
        button(ui)
    }
}

/// A table cell; numbers are right-aligned and muted when `weak`.
pub fn body_cell(ui: &mut egui::Ui, text: String, right: bool, weak: bool) -> egui::Response {
    let mut t = RichText::new(text);
    if weak {
        t = t.weak();
    }
    let label = egui::Label::new(t).selectable(false).truncate();
    if right {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(label)
        })
        .inner
    } else {
        ui.add(label)
    }
}

/// A switch followed by its label, for on/off filters.
pub fn switch_row(ui: &mut egui::Ui, on: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        toggle_switch(ui, on);
        ui.label(label);
    });
}

/// A one-line, muted, dismissible note. Returns true when dismissed.
pub fn banner(ui: &mut egui::Ui, text: &str) -> bool {
    let mut dismissed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let w = (ui.available_width() - 70.0).max(0.0);
        ui.add_sized(
            [w, 16.0],
            egui::Label::new(RichText::new(text).small().weak()).truncate(),
        )
        .on_hover_text(text);
        dismissed = ui
            .add(egui::Button::new(RichText::new("Dismiss").small().color(MUTED)).frame(false))
            .clicked();
    });
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_text_units() {
        assert_eq!(ago_text(0.0), "Now");
        assert_eq!(ago_text(12.0), "12 s ago");
        assert_eq!(ago_text(60.0), "1 min ago");
        assert_eq!(ago_text(130.0), "2 min 10 s ago");
        assert_eq!(ago_text(3600.0), "1 h ago");
        assert_eq!(ago_text(3900.0), "1 h 5 min ago");
        assert_eq!(ago_text(-3.0), "Now");
    }

    #[test]
    fn hover_offset_counts_back_from_the_right() {
        // 10 px between points, newest at x = 100.
        assert_eq!(hover_offset(100.0, 100.0, 10.0, 5), Some(0));
        assert_eq!(hover_offset(73.0, 100.0, 10.0, 5), Some(3));
        assert_eq!(hover_offset(60.0, 100.0, 10.0, 5), Some(4));
        // Left of the oldest point, or right of the newest.
        assert_eq!(hover_offset(40.0, 100.0, 10.0, 5), None);
        assert_eq!(hover_offset(120.0, 100.0, 10.0, 5), Some(0));
        // Not enough data to draw a line.
        assert_eq!(hover_offset(100.0, 100.0, 10.0, 1), None);
        assert_eq!(hover_offset(100.0, 100.0, 0.0, 5), None);
    }
}
