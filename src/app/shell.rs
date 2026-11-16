use super::*;
use crate::ui::{BORDER, HOST_ACCENT as ACCENT, MUTED, PANEL_BG, SIDEBAR_BG, TEXT, WINDOW_BG};
use egui::{Color32, RichText, Vec2};
use toolboks_addon_sdk::{Manifest, Metric, Widget};

#[derive(Clone, PartialEq)]
pub(super) enum Screen {
    Home,
    Pick,
    Browser,
    Settings,
    External(String),
}

/// The manifest shown for an add-on installed from the catalog, built from its catalog entry.
pub(super) fn manifest_of(entry: &crate::catalog::Entry) -> Manifest {
    Manifest {
        id: entry.id.clone(),
        title: entry.title.clone(),
        byline: entry.byline.clone(),
        native_provider: None,
        description: entry.short_description.clone(),
        long_description: if entry.long_description.is_empty() {
            entry.short_description.clone()
        } else {
            entry.long_description.clone()
        },
        version: entry.version.clone(),
        author: entry.publisher.clone(),
        publisher: entry.publisher.clone(),
        category: entry.category.clone(),
        icon: entry.icon.clone(),
        accent: entry.accent.clone(),
        api_version: 2,
        min_toolboks_version: entry.min_toolboks_version.clone(),
        website: None,
        support_url: None,
        license: None,
        screenshots: entry.screenshots.clone(),
        capabilities: Vec::new(),
        data_note: entry.data_note.clone(),
    }
}

/// Body text (12 px) in the muted token.
pub(super) fn muted(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.label(RichText::new(s.into()).size(look::TEXT_BODY).color(MUTED));
}
/// A control label (12 px, full-strength text).
fn body(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(look::TEXT_BODY).color(TEXT));
}
pub(super) fn caption(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.label(
        RichText::new(s.into())
            .size(look::TEXT_CAPTION)
            .color(MUTED),
    );
}
/// A section heading: 14 px, strong.
pub(super) fn section(ui: &mut egui::Ui, s: &str) {
    ui.label(
        RichText::new(s)
            .size(look::TEXT_SECTION)
            .strong()
            .color(TEXT),
    );
}
pub(super) fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(look::CARD_RADIUS)
        .inner_margin(look::SPACE_M)
}
/// The one primary button of a view: accent fill, white text (6:1 on the fill).
pub(super) fn primary(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(s).color(Color32::WHITE))
            .fill(look::HOST_ACCENT_FILL)
            .corner_radius(look::CONTROL_RADIUS)
            .min_size(Vec2::new(80.0, 28.0)),
    )
}
/// A small status chip. Always carries a word; `verified` adds the verified mark before it.
fn badge(ui: &mut egui::Ui, s: &str, color: Color32, verified: bool) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(look::CONTROL_RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = look::SPACE_XS;
                if verified {
                    verified_mark(ui);
                }
                ui.label(RichText::new(s).size(look::TEXT_CAPTION).color(color));
            });
        });
}
/// Text-sized accent that reaches 4.5:1 on a card (the plain accent is 4.3:1).
const ACCENT_TEXT: Color32 = look::DATA_MEMORY;
/// A label with an on/off switch at the right and an accessible name.
fn switch_row(ui: &mut egui::Ui, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        body(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = look::toggle_switch(ui, on);
            let value = *on;
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, value, label));
            changed = response.changed();
        });
    });
    changed
}
/// Width of a modal: `want`, but never wider than the window allows.
fn modal_width(ctx: &egui::Context, want: f32) -> f32 {
    want.min((ctx.content_rect().width() - 64.0).max(240.0))
}

/// One add-on as the person sees it, with its state.
struct Entry {
    m: Manifest,
    verified: bool,
    installed: bool,
    enabled: bool,
}

impl Entry {
    fn status(&self) -> &'static str {
        if !self.installed {
            "Not installed"
        } else if self.enabled {
            "On"
        } else {
            "Off"
        }
    }
    fn action(&self) -> &'static str {
        if !self.installed {
            "Install add-on"
        } else if self.enabled {
            "Open add-on"
        } else {
            "Turn on"
        }
    }
}

/// One selectable row of the Home list: small icon, name, one plain detail line, state at the
/// right. Selected = lighter fill.
fn list_row(
    ui: &mut egui::Ui,
    m: &Manifest,
    status: &str,
    status_color: Color32,
    selected: bool,
    icon_color: Color32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            look::SELECTED_BG
        } else if response.hovered() {
            look::BAR_DIVIDER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, look::CONTROL_RADIUS, fill);
        focus_ring(ui, rect, &response);
        paint_glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 20.0, rect.center().y),
                Vec2::splat(18.0),
            ),
            sidebar_icon(&m.id, &m.icon),
            icon_color,
            18.0,
        );
        let small = egui::FontId::proportional(look::TEXT_CAPTION);
        let state = ui
            .painter()
            .layout_no_wrap(status.to_owned(), small.clone(), status_color);
        let text_clip = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right() - state.size().x - 16.0, rect.bottom()),
        );
        let p = ui.painter().with_clip_rect(text_clip);
        p.text(
            egui::pos2(rect.left() + 38.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            &m.title,
            egui::FontId::proportional(look::TEXT_BODY),
            TEXT,
        );
        p.text(
            egui::pos2(rect.left() + 38.0, rect.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            &m.category,
            small,
            MUTED,
        );
        ui.painter().galley(
            egui::pos2(
                rect.right() - state.size().x - 10.0,
                rect.center().y - state.size().y / 2.0,
            ),
            state,
            status_color,
        );
    }
    let name = format!("{}, {}", m.title, status);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn accent(m: &Manifest) -> Color32 {
    let rgb = u32::from_str_radix(&m.accent[1..], 16).unwrap_or(0xa48aca);
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}
fn glyph(ui: &mut egui::Ui, name: &str, color: Color32, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_glyph(ui.painter(), r, name, color, size);
}
fn paint_glyph(p: &egui::Painter, r: egui::Rect, name: &str, color: Color32, size: f32) {
    let stroke = egui::Stroke::new(1.6, color);
    let pt = |x: f32, y: f32| egui::pos2(r.left() + x * size, r.top() + y * size);
    if name == "menu" {
        for y in [0.25, 0.5, 0.75] {
            p.line_segment([pt(0.12, y), pt(0.88, y)], stroke);
        }
    } else if name == "home" {
        p.line(vec![pt(0.1, 0.48), pt(0.5, 0.12), pt(0.9, 0.48)], stroke);
        p.line(
            vec![
                pt(0.22, 0.42),
                pt(0.22, 0.88),
                pt(0.78, 0.88),
                pt(0.78, 0.42),
            ],
            stroke,
        );
        p.line(
            vec![
                pt(0.42, 0.88),
                pt(0.42, 0.62),
                pt(0.58, 0.62),
                pt(0.58, 0.88),
            ],
            stroke,
        );
    } else if name == "sliders" {
        for (y, x) in [(0.22, 0.65), (0.5, 0.35), (0.78, 0.6)] {
            p.line_segment([pt(0.1, y), pt(0.9, y)], stroke);
            p.circle_filled(pt(x, y), size * 0.1, color);
        }
    } else if name == "layers" {
        for y in [0.26, 0.5, 0.74] {
            p.line_segment([pt(0.1, y), pt(0.5, y + 0.18)], stroke);
            p.line_segment([pt(0.5, y + 0.18), pt(0.9, y)], stroke);
        }
        p.line_segment([pt(0.1, 0.26), pt(0.5, 0.08)], stroke);
        p.line_segment([pt(0.5, 0.08), pt(0.9, 0.26)], stroke);
    } else if name == "activity" {
        p.line(
            vec![
                pt(0.05, 0.55),
                pt(0.25, 0.55),
                pt(0.4, 0.2),
                pt(0.57, 0.85),
                pt(0.72, 0.4),
                pt(0.95, 0.4),
            ],
            stroke,
        );
    } else if name == "macro" {
        p.line(
            vec![
                pt(0.18, 0.12),
                pt(0.85, 0.5),
                pt(0.18, 0.88),
                pt(0.18, 0.12),
            ],
            stroke,
        );
        p.circle_filled(pt(0.84, 0.85), size * 0.07, color);
    } else if name == "storage" {
        let body = egui::Rect::from_min_max(pt(0.12, 0.14), pt(0.88, 0.86));
        p.rect_stroke(body, 3, stroke, egui::StrokeKind::Inside);
        p.line_segment([pt(0.12, 0.60), pt(0.88, 0.60)], stroke);
        p.circle_filled(pt(0.28, 0.73), size * 0.035, color);
        p.circle_filled(pt(0.42, 0.73), size * 0.035, color);
    } else if name == "leaf" {
        p.line(
            vec![
                pt(0.15, 0.85),
                pt(0.28, 0.38),
                pt(0.85, 0.12),
                pt(0.77, 0.68),
                pt(0.15, 0.85),
            ],
            stroke,
        );
        p.line_segment([pt(0.15, 0.85), pt(0.65, 0.35)], stroke);
    } else if name == "wrench" {
        p.line(
            vec![
                pt(0.7, 0.12),
                pt(0.58, 0.34),
                pt(0.76, 0.5),
                pt(0.93, 0.32),
                pt(0.87, 0.62),
                pt(0.64, 0.65),
                pt(0.28, 0.94),
                pt(0.08, 0.76),
                pt(0.41, 0.43),
                pt(0.4, 0.2),
                pt(0.7, 0.12),
            ],
            stroke,
        );
    } else if name == "chip" {
        p.rect_stroke(
            egui::Rect::from_min_max(pt(0.28, 0.28), pt(0.72, 0.72)),
            2,
            stroke,
            egui::StrokeKind::Inside,
        );
        for t in [0.38, 0.5, 0.62] {
            p.line_segment([pt(t, 0.28), pt(t, 0.12)], stroke);
            p.line_segment([pt(t, 0.72), pt(t, 0.88)], stroke);
            p.line_segment([pt(0.28, t), pt(0.12, t)], stroke);
            p.line_segment([pt(0.72, t), pt(0.88, t)], stroke);
        }
    } else if name == "shield" {
        p.line(
            vec![
                pt(0.5, 0.08),
                pt(0.86, 0.22),
                pt(0.86, 0.52),
                pt(0.5, 0.92),
                pt(0.14, 0.52),
                pt(0.14, 0.22),
                pt(0.5, 0.08),
            ],
            stroke,
        );
    } else if name == "mic" {
        p.rect_stroke(
            egui::Rect::from_min_max(pt(0.38, 0.08), pt(0.62, 0.52)),
            6,
            stroke,
            egui::StrokeKind::Inside,
        );
        p.line(
            vec![
                pt(0.22, 0.42),
                pt(0.25, 0.6),
                pt(0.36, 0.72),
                pt(0.5, 0.76),
                pt(0.64, 0.72),
                pt(0.75, 0.6),
                pt(0.78, 0.42),
            ],
            stroke,
        );
        p.line_segment([pt(0.5, 0.76), pt(0.5, 0.92)], stroke);
        p.line_segment([pt(0.36, 0.92), pt(0.64, 0.92)], stroke);
    } else if name == "gauge" {
        let arc: Vec<egui::Pos2> = (0..=14)
            .map(|i| {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 14.0);
                pt(0.5 + 0.4 * a.cos(), 0.72 + 0.4 * a.sin())
            })
            .collect();
        p.line(arc, stroke);
        p.line_segment([pt(0.1, 0.72), pt(0.9, 0.72)], stroke);
        p.line_segment([pt(0.5, 0.72), pt(0.68, 0.42)], stroke);
        p.circle_filled(pt(0.5, 0.72), size * 0.06, color);
    } else if name == "gear" {
        p.circle_stroke(pt(0.5, 0.5), size * 0.27, stroke);
        p.circle_stroke(pt(0.5, 0.5), size * 0.1, stroke);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            p.line_segment(
                [
                    pt(0.5 + 0.27 * a.cos(), 0.5 + 0.27 * a.sin()),
                    pt(0.5 + 0.4 * a.cos(), 0.5 + 0.4 * a.sin()),
                ],
                stroke,
            );
        }
    } else {
        for (x, y) in [(0.12, 0.12), (0.57, 0.12), (0.12, 0.57), (0.57, 0.57)] {
            p.rect_stroke(
                egui::Rect::from_min_max(pt(x, y), pt(x + 0.3, y + 0.3)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
}
fn paint_verified(p: &egui::Painter, r: egui::Rect) {
    p.circle_filled(r.center(), r.width() / 2.0, ACCENT);
    let at = |x: f32, y: f32| egui::pos2(r.left() + x * r.width(), r.top() + y * r.height());
    p.line(
        vec![at(0.23, 0.5), at(0.43, 0.7), at(0.78, 0.3)],
        egui::Stroke::new(1.4, Color32::from_rgb(27, 19, 38)),
    );
}
fn verified_mark(ui: &mut egui::Ui) {
    let (r, response) = ui.allocate_exact_size(Vec2::splat(13.0), egui::Sense::hover());
    paint_verified(ui.painter(), r);
    response.on_hover_text("Toolboks verified - bundled publisher");
}
/// The sidebar icon of an add-on. The tools Toolboks ships each have their own, so no two rows look
/// alike; anything else uses the icon its manifest names.
pub(super) fn sidebar_icon<'a>(id: &str, declared: &'a str) -> &'a str {
    match id {
        "toolboks.memory-cleaner" => "chip",
        "toolboks.resource-monitor" => "activity",
        "toolboks.debloater" => "sliders",
        "toolboks.event-watch" => "shield",
        "toolboks.disk-manager" => "storage",
        "toolboks.aural" => "mic",
        "toolboks.system-profiles" => "gauge",
        "toolboks.macro-manager" => "macro",
        _ => declared,
    }
}
const RAIL_W: f32 = 52.0;
const SIDEBAR_W: f32 = 240.0;
const HEADER_H: f32 = 46.0;
const STATUS_H: f32 = 24.0;
const ROW_H: f32 = 32.0;
const PAGE_ROW_H: f32 = 28.0;
/// Where a sidebar row's label starts, measured from the row's left edge.
const LABEL_X: f32 = 40.0;

/// Paints the keyboard focus ring over a custom-drawn control.
pub(super) fn focus_ring(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            look::CONTROL_RADIUS,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
}

/// One row of the sidebar: an icon centred in the 36 px rail column and, when `show_label`, the
/// name next to it. `label` is always the accessible name.
/// A mark at the right end of a sidebar row. An update is more urgent than "New".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tag {
    None,
    New,
    Update,
}

pub(super) fn nav_row(
    ui: &mut egui::Ui,
    label: &str,
    icon: &str,
    selected: bool,
    show_label: bool,
    tag: Tag,
) -> egui::Response {
    let name = match tag {
        Tag::None => label.to_owned(),
        Tag::New => format!("{label}, new"),
        Tag::Update => format!("{label}, update available"),
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            look::ACCENT_WASH
        } else if response.hovered() {
            look::SELECTED_BG
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, look::CONTROL_RADIUS, fill);
        focus_ring(ui, rect, &response);
        let icon_color = if selected { ACCENT } else { MUTED };
        paint_glyph(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 18.0, rect.center().y),
                Vec2::splat(18.0),
            ),
            icon,
            icon_color,
            18.0,
        );
        if show_label {
            let galley = ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(look::TEXT_SECTION),
                if selected { TEXT } else { MUTED },
            );
            let label_width = galley.size().x;
            ui.painter().with_clip_rect(rect).galley(
                egui::pos2(
                    rect.left() + LABEL_X,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                if selected { TEXT } else { MUTED },
            );
            if tag != Tag::None {
                // The word sits at the right end of the row. "New" goes once the add-on has been
                // opened; "Update" goes once the update is installed.
                let (word, color, wash) = match tag {
                    Tag::Update => (
                        "Update",
                        look::AMBER,
                        Color32::from_rgba_unmultiplied(240, 168, 58, 38),
                    ),
                    _ => ("New", ACCENT_TEXT, look::ACCENT_WASH),
                };
                let text = ui.painter().layout_no_wrap(
                    word.to_owned(),
                    egui::FontId::proportional(look::TEXT_CAPTION),
                    color,
                );
                let x = (rect.right() - text.size().x - 18.0)
                    .max(rect.left() + LABEL_X + label_width + 8.0);
                let tag_rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.center().y - text.size().y / 2.0 - 2.0),
                    text.size() + Vec2::new(10.0, 4.0),
                );
                ui.painter().rect_filled(tag_rect, 9.0, wash);
                ui.painter()
                    .galley(tag_rect.min + Vec2::new(5.0, 2.0), text, color);
            }
        } else if tag != Tag::None {
            // The collapsed rail has no room for a word: a dot on the icon, named in the tooltip.
            // Amber for an update, purple for "new".
            ui.painter().circle_filled(
                egui::pos2(rect.left() + 28.0, rect.center().y - 8.0),
                3.5,
                if tag == Tag::Update {
                    look::AMBER
                } else {
                    ACCENT
                },
            );
        }
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A navigation row; with the rail collapsed it shows its name as a tooltip. True when clicked.
fn nav(
    ui: &mut egui::Ui,
    label: &str,
    icon: &str,
    selected: bool,
    expanded: bool,
    tag: Tag,
) -> bool {
    let response = nav_row(ui, label, icon, selected, expanded, tag);
    let label = match tag {
        Tag::New if !expanded => format!("{label} (new)"),
        Tag::Update if !expanded => format!("{label} (update available)"),
        _ => label.to_owned(),
    };
    let label = label.as_str();
    if expanded {
        response.clicked()
    } else {
        focus_tip(&response, label);
        response.on_hover_text(label).clicked()
    }
}

/// With the rail collapsed a row has no visible name. Hover already shows it as a tooltip; this
/// shows the same tooltip while the row has keyboard focus, so Tab users can read it too.
pub(super) fn focus_tip(response: &egui::Response, text: &str) {
    if response.has_focus() && !response.hovered() {
        egui::Tooltip::for_widget(response).show(|ui| ui.label(text));
    }
}

/// A page nested under the active tool in the expanded sidebar; selected = lighter fill.
pub(super) fn page_row(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), PAGE_ROW_H),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            look::SELECTED_BG
        } else if response.hovered() {
            look::BAR_DIVIDER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, look::CONTROL_RADIUS, fill);
        focus_ring(ui, rect, &response);
        ui.painter().with_clip_rect(rect).text(
            egui::pos2(rect.left() + LABEL_X, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(look::TEXT_BODY),
            if selected { TEXT } else { MUTED },
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Space between sidebar rows.
const ROW_GAP: f32 = 2.0;
/// The hairline above the fixed entries at the foot of the sidebar.
const BOTTOM_RULE_H: f32 = 9.0;

/// Height of the group at the foot of the sidebar: the hairline, Settings, Add-ons and, while the
/// Add-ons entry is open in the wide sidebar, its "Pick your tools" page row.
fn bottom_group_height(show_pick: bool) -> f32 {
    let (rows, gaps) = if show_pick {
        (BOTTOM_RULE_H + 2.0 * ROW_H + PAGE_ROW_H, 3.0)
    } else {
        (BOTTOM_RULE_H + 2.0 * ROW_H, 2.0)
    };
    rows + gaps * ROW_GAP
}

/// The fixed entries at the foot of the sidebar. They are created top to bottom, in the order they
/// appear (hairline, Settings, Add-ons, then the Add-ons page row), because that creation order is
/// the Tab order. The group is placed against the bottom edge by its known height.
fn bottom_group(ui: &mut egui::Ui, screen: &mut Screen, expanded: bool) {
    let browser_on = matches!(*screen, Screen::Browser | Screen::Pick);
    let show_pick = expanded && browser_on;
    let area = ui.max_rect();
    let top = (area.bottom() - bottom_group_height(show_pick)).max(area.top());
    let rect = egui::Rect::from_min_max(egui::pos2(area.left(), top), area.right_bottom());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            // A hairline between the tools and the fixed entries at the bottom.
            let (line, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), BOTTOM_RULE_H),
                egui::Sense::hover(),
            );
            ui.painter().hline(
                line.x_range(),
                line.center().y,
                egui::Stroke::new(1.0, look::BORDER),
            );
            if nav(
                ui,
                "Settings",
                "gear",
                *screen == Screen::Settings,
                expanded,
                Tag::None,
            ) {
                *screen = Screen::Settings;
            }
            if toolboks_addon_sdk::tutorial::region(ui, "browser-sidebar", |ui| {
                nav(ui, "Add-ons", "grid", browser_on, expanded, Tag::None)
            }) {
                *screen = Screen::Browser;
            }
            // The one extra page of the Add-ons entry, shown only while that entry is open, like
            // the pages of any other entry.
            if show_pick && page_row(ui, "Pick your tools", *screen == Screen::Pick) {
                *screen = Screen::Pick;
            }
        },
    );
}

/// What the header shows for the current screen.
struct Crumb {
    root: String,
    byline: Option<String>,
    leaf: Option<String>,
    pages: Vec<String>,
    page: usize,
    help: Option<String>,
    verified: bool,
}

/// Width of a row of header buttons with these labels, in points. Slightly generous, so the
/// header switches to a menu before anything can touch or be cut off.
fn buttons_width(ui: &egui::Ui, labels: &[&str], extra_each: f32) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let spacing = ui.spacing();
    let text: f32 = labels
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), TEXT)
                .size()
                .x
                + 2.0 * spacing.button_padding.x
                + extra_each
        })
        .sum();
    text + spacing.item_spacing.x * labels.len().saturating_sub(1) as f32 + 8.0
}

/// Width of the add-on browser's header actions (see `market_header`), in points.
fn market_actions_width(ui: &egui::Ui, browser: bool) -> f32 {
    if browser {
        let spacing = ui.spacing();
        let box_w = spacing.icon_width + spacing.icon_spacing;
        buttons_width(
            ui,
            &[
                "Show me around",
                "View details",
                "Refresh list",
                "Install from file",
                "Pick your tools",
            ],
            0.0,
        ) + buttons_width(ui, &["Installed only"], box_w)
            + spacing.item_spacing.x
    } else {
        buttons_width(ui, &["Show me around"], 0.0)
    }
}

/// Width of one page pill per name, in points (matches `ui::pill`: text plus 18).
fn pills_width(ui: &egui::Ui, names: &[String]) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text: f32 = names
        .iter()
        .map(|name| {
            ui.painter()
                .layout_no_wrap(name.clone(), font.clone(), TEXT)
                .size()
                .x
                + 18.0
        })
        .sum();
    text + ui.spacing().item_spacing.x * names.len().saturating_sub(1) as f32 + 8.0
}

impl Crumb {
    fn plain(root: &str) -> Self {
        Self {
            root: root.to_owned(),
            byline: None,
            leaf: None,
            pages: Vec::new(),
            page: 0,
            help: None,
            verified: false,
        }
    }
}

impl App {
    pub(super) fn shell_ui(&mut self, ui: &mut egui::Ui) {
        self.poll_marketplace(ui.ctx());
        if self.screen != Screen::Settings {
            self.own_usage.reset();
        }
        self.prepare_tutorial(ui.ctx());
        if self.tour.is_some() {
            ui.disable();
        }
        let dropped = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        if let Some(path) = dropped.first() {
            self.review_package(path);
        }
        if let Screen::External(id) = &self.screen {
            if self.registry.config.new_addons.contains(id) {
                self.registry.config.new_addons.retain(|n| n != id);
                self.persist();
            }
        }
        let expanded = self.sidebar_expanded && ui.ctx().content_rect().width() >= 760.0;
        egui::Panel::left("host-sidebar")
            .resizable(false)
            .exact_size(if expanded { SIDEBAR_W } else { RAIL_W })
            .frame(
                egui::Frame::new()
                    .fill(SIDEBAR_BG)
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let tip = if expanded {
                    "Close sidebar"
                } else {
                    "Open sidebar"
                };
                let response = nav_row(ui, tip, "menu", false, false, Tag::None);
                if expanded {
                    // The sidebar's title: the app name in bold at title size.
                    ui.painter().text(
                        egui::pos2(response.rect.left() + LABEL_X, response.rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "Toolboks",
                        super::store::font(look::TEXT_TITLE - 1.0, 700),
                        TEXT,
                    );
                }
                if !expanded {
                    focus_tip(&response, tip);
                }
                if response.on_hover_text(tip).clicked() {
                    self.sidebar_expanded = !expanded;
                }
                ui.add_space(look::GAP);

                let browser_open = matches!(self.screen, Screen::Browser | Screen::Pick);
                let installed: Vec<_> = self
                    .registry
                    .packages
                    .iter()
                    .filter(|p| self.registry.enabled(&p.manifest.id))
                    .map(|p| {
                        let pages: Vec<String> =
                            p.pages.iter().map(|page| page.title.clone()).collect();
                        (
                            p.manifest.id.clone(),
                            p.manifest.title.clone(),
                            p.manifest.icon.clone(),
                            pages,
                        )
                    })
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt("sidebar-nav")
                    .max_height(
                        (ui.available_height()
                            - bottom_group_height(expanded && browser_open)
                            - 4.0)
                            .max(50.0),
                    )
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if nav(
                            ui,
                            "Home",
                            "home",
                            self.screen == Screen::Home,
                            expanded,
                            Tag::None,
                        ) {
                            self.screen = Screen::Home;
                        }
                        for (id, title, icon, pages) in installed {
                            let on = self.screen == Screen::External(id.clone());
                            let tag = self.addon_tag(&id);
                            if nav(ui, &title, sidebar_icon(&id, &icon), on, expanded, tag) {
                                self.open_addon(&id);
                            }
                            self.market_sidebar_status(ui, &id, &title, expanded);
                            if on && expanded && pages.len() > 1 {
                                for (p, name) in pages.iter().enumerate() {
                                    if page_row(ui, name, self.addon_page == p) {
                                        self.addon_page = p;
                                    }
                                }
                            }
                        }
                        toolboks_addon_sdk::tutorial::region(ui, "browser-sidebar", |ui| {
                            self.market_sidebar(ui, expanded)
                        });
                    });
                bottom_group(ui, &mut self.screen, expanded);
            });

        // The header: breadcrumb at the left, page controls at the right.
        let crumb = match self.screen.clone() {
            Screen::Home => Crumb::plain("Home"),
            Screen::Pick => Crumb::plain("Add-ons / Pick your tools"),
            Screen::Browser => Crumb::plain("Add-ons"),
            Screen::Settings => Crumb::plain("Settings"),
            Screen::External(id) => {
                let package = self.registry.packages.iter().find(|p| p.manifest.id == id);
                let pages: Vec<String> = package
                    .map(|p| p.pages.iter().map(|page| page.title.clone()).collect())
                    .unwrap_or_else(|| {
                        crate::installer::installed_entry(&self.registry.root, &id)
                            .map(|entry| entry.pages)
                            .unwrap_or_default()
                    });
                let page = self.addon_page;
                Crumb {
                    root: package.map_or_else(
                        || {
                            crate::installer::installed_entry(&self.registry.root, &id)
                                .map_or_else(|| id.clone(), |entry| entry.title)
                        },
                        |p| p.manifest.title.clone(),
                    ),
                    byline: package.and_then(|p| p.manifest.byline.clone()),
                    leaf: pages.get(page).filter(|_| pages.len() > 1).cloned(),
                    pages,
                    page,
                    verified: self.registry.verified(&id),
                    help: Some(id),
                }
            }
        };
        let mut chosen_page = None;
        egui::Panel::top("host-header")
            .exact_size(HEADER_H)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(WINDOW_BG)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let size = look::TEXT_SECTION;
                    match &crumb.leaf {
                        Some(leaf) => {
                            ui.label(RichText::new(&crumb.root).size(size).color(MUTED));
                            if let Some(byline) = &crumb.byline {
                                ui.label(
                                    RichText::new(byline).size(look::TEXT_CAPTION).color(MUTED),
                                );
                            }
                            ui.label(RichText::new("/").size(size).color(MUTED));
                            ui.label(RichText::new(leaf).size(size).strong().color(TEXT));
                        }
                        None => {
                            ui.label(RichText::new(&crumb.root).size(size).strong().color(TEXT));
                            if let Some(byline) = &crumb.byline {
                                caption(ui, byline);
                            }
                        }
                    }
                    if crumb.verified {
                        verified_mark(ui);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let room = ui.available_width();
                        if matches!(self.screen, Screen::Browser | Screen::Pick) {
                            let need =
                                market_actions_width(ui, matches!(self.screen, Screen::Browser));
                            if room < need {
                                ui.menu_button("Add-on actions", |ui| self.market_header(ui));
                            } else {
                                self.market_header(ui);
                            }
                        }
                        if let Some(id) = &crumb.help {
                            if ui.button("Help").clicked() {
                                self.help = Some(id.clone());
                            }
                        }
                        // With the rail collapsed the nested pages are out of sight, so the
                        // pages stay reachable here: one pill per page when they all fit beside
                        // Help, otherwise a menu.
                        if !expanded && crumb.pages.len() > 1 {
                            let help = if crumb.help.is_some() {
                                buttons_width(ui, &["Help"], 0.0)
                            } else {
                                0.0
                            };
                            if crumb.byline.is_some() || pills_width(ui, &crumb.pages) + help > room
                            {
                                egui::ComboBox::from_id_salt("addon-page-selector")
                                    .selected_text("Pages")
                                    .show_ui(ui, |ui| {
                                        for (i, name) in crumb.pages.iter().enumerate() {
                                            if ui.selectable_label(crumb.page == i, name).clicked()
                                            {
                                                chosen_page = Some(i);
                                            }
                                        }
                                    });
                            } else {
                                for (i, name) in crumb.pages.iter().enumerate().rev() {
                                    if look::pill(ui, name, crumb.page == i).clicked() {
                                        chosen_page = Some(i);
                                    }
                                }
                            }
                        }
                    });
                });
            });
        if let Some(page) = chosen_page {
            if let Screen::External(ref id) = self.screen {
                self.addon_page = page;
                if let Some(supervisor) = &self.supervisor {
                    supervisor.send(
                        id,
                        &toolboks_addon_proto::HostMessage::Navigate {
                            page: page.to_string(),
                        },
                    );
                }
            }
        }

        let mut items: Vec<String> = Vec::new();
        items.extend(
            self.services
                .status_items(|id| self.registry.enabled(id))
                .into_iter()
                .take(3)
                .map(|item| item.text),
        );
        egui::Panel::bottom("host-status")
            .exact_size(STATUS_H)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(look::STATUS_BG)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    for item in &items {
                        ui.label(RichText::new(item).size(look::TEXT_CAPTION).color(MUTED));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let version = if cfg!(feature = "preview") {
                            format!("v{} local preview", Version::current())
                        } else {
                            format!("v{}", Version::current())
                        };
                        ui.label(RichText::new(version).size(look::TEXT_CAPTION).color(MUTED));
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(WINDOW_BG)
                    .inner_margin(egui::Margin::symmetric(16, 16)),
            )
            .show(ui, |ui| {
                if let Some(message) = self.message.clone() {
                    panel().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(message).color(TEXT));
                            if ui.small_button("Dismiss").clicked() {
                                self.message = None;
                            }
                        });
                    });
                    ui.add_space(look::GAP);
                }
                match self.screen.clone() {
                    Screen::Home => self.home(ui),
                    Screen::Pick => self.pick_tools(ui),
                    Screen::Browser => self.marketplace_ui(ui),
                    Screen::Settings => self.settings(ui),
                    Screen::External(id) => self.external_workspace(ui, &id),
                }
            });
        self.market_remove(ui.ctx());
        self.dialogs(ui.ctx());
        self.update_prompt(ui.ctx());
        self.tutorial_ui(ui.ctx());
    }
    pub(super) fn title(ui: &mut egui::Ui, title: &str, description: &str) {
        ui.label(
            RichText::new(title)
                .size(look::TEXT_TITLE)
                .strong()
                .color(TEXT),
        );
        muted(ui, description);
        ui.add_space(look::SPACE_M);
    }
    /// Every add-on the person can see: the built-in tools and any package they
    /// installed, with its state.
    fn entries(&self) -> Vec<Entry> {
        let mut list = Vec::new();
        for p in &self.registry.packages {
            list.push(Entry {
                m: p.manifest.clone(),
                verified: self.registry.verified(&p.manifest.id),
                installed: true,
                enabled: self.registry.enabled(&p.manifest.id),
            });
        }
        // Add-ons installed from the catalog (their own program, drawn inside Toolboks).
        for entry in crate::installer::installed(&self.registry.root) {
            if list.iter().any(|e| e.m.id == entry.id) {
                continue;
            }
            list.push(Entry {
                verified: entry.verified,
                installed: true,
                enabled: self.registry.enabled(&entry.id),
                m: manifest_of(&entry),
            });
        }
        list.sort_by_key(|e| e.m.title.to_lowercase());
        list
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        if self.restore_pending() {
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Some of the tools you had before are not installed yet.")
                            .color(TEXT),
                    );
                    if ui.button("Restore your tools").clicked() {
                        let ids = self.registry.config.restore_pending.clone();
                        for id in &ids {
                            self.market.select(id, true);
                        }
                        self.screen = Screen::Pick;
                    }
                });
            });
            ui.add_space(look::GAP);
        }
        let updates = self.pending_updates();
        if !updates.is_empty() && self.market.review.is_none() {
            // Updates are found from the list the app fetches; say so here, not only in the browser.
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let titles: Vec<String> = updates.iter().map(|u| u.title.clone()).collect();
                    ui.label(
                        RichText::new(crate::marketplace::update_line(&titles))
                            .color(look::AMBER)
                            .strong(),
                    );
                    if primary(ui, "Update").clicked() {
                        self.review_updates(updates.clone());
                    }
                });
            });
            ui.add_space(look::GAP);
        }
        if self.registry.packages.is_empty()
            && crate::installer::installed(&self.registry.root).is_empty()
        {
            ui.horizontal(|ui| {
                muted(ui, "Pick the tools you want to add to your sidebar.");
                if ui.button("Pick your tools").clicked() {
                    self.screen = Screen::Pick;
                }
            });
            ui.add_space(look::GAP);
        }
        let entries = self.entries();
        let added = entries.len();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Self::title(
                    ui,
                    "Your toolkit",
                    &format!(
                        "{added} {} installed.",
                        if added == 1 { "add-on" } else { "add-ons" }
                    ),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                if ui.button("Browse add-ons").clicked() {
                    self.screen = Screen::Browser;
                }
            });
        });
        let selected = entries
            .iter()
            .position(|e| Some(&e.m.id) == self.home_selected.as_ref())
            .unwrap_or(0);
        // Narrow window or large text scale: stack the list above the detail instead of
        // squeezing two columns.
        let narrow = ui.available_width() < 520.0;
        let (list_w, height, layout) = if narrow {
            (
                ui.available_width(),
                (ui.available_height() * 0.4).clamp(132.0, 220.0),
                egui::Layout::top_down(egui::Align::Min),
            )
        } else {
            (
                280.0_f32.min(ui.available_width() * 0.45),
                ui.available_height(),
                egui::Layout::left_to_right(egui::Align::Min),
            )
        };
        ui.with_layout(layout, |ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(list_w, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(list_w);
                    egui::ScrollArea::vertical()
                        .id_salt("home-list")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for (i, e) in entries.iter().enumerate() {
                                let tint = if e.enabled && e.installed {
                                    look::GREEN
                                } else {
                                    MUTED
                                };
                                // An add-on with an update says "Update" where it said "On".
                                let (status, tint) = if updates.iter().any(|u| u.id == e.m.id) {
                                    ("Update", look::AMBER)
                                } else {
                                    (e.status(), tint)
                                };
                                if list_row(ui, &e.m, status, tint, i == selected, accent(&e.m))
                                    .clicked()
                                {
                                    self.home_selected = Some(e.m.id.clone());
                                }
                            }
                        });
                },
            );
            ui.add_space(look::GAP);
            let Some(e) = entries.get(selected) else {
                return;
            };
            ui.vertical(|ui| {
                self.addon_detail_card(ui, e);
            });
        });
    }
    /// The right side of Home: the chosen add-on, what it does, and the one thing to do next.
    fn addon_detail_card(&mut self, ui: &mut egui::Ui, e: &Entry) {
        let update = self
            .pending_updates()
            .into_iter()
            .find(|entry| entry.id == e.m.id);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&e.m.title)
                        .size(look::TEXT_TITLE)
                        .strong()
                        .color(TEXT),
                );
                if e.verified {
                    verified_mark(ui);
                }
            });
            let mut line = format!("{} · {} · v{}", e.m.publisher, e.m.category, e.m.version);
            if let Some(byline) = &e.m.byline {
                line = format!("{byline} · {line}");
            }
            caption(ui, line);
            ui.add_space(look::GAP);
            badge(
                ui,
                e.status(),
                if e.enabled && e.installed {
                    ACCENT_TEXT
                } else {
                    MUTED
                },
                false,
            );
            if let Some(update) = &update {
                ui.add_space(look::SPACE_XS);
                badge(
                    ui,
                    &format!("Update available: v{}", update.version),
                    look::AMBER,
                    false,
                );
            }
            ui.add_space(look::GAP);
            ui.label(&e.m.description);
            ui.add_space(look::SPACE_L);
            ui.horizontal_wrapped(|ui| {
                if let Some(update) = &update {
                    if primary(ui, "Update").clicked() {
                        self.review_updates(vec![update.clone()]);
                    }
                    if ui.button(e.action()).clicked() {
                        self.run_action(e);
                    }
                } else if primary(ui, e.action()).clicked() {
                    self.run_action(e);
                }
                if ui.button("View details").clicked() {
                    self.detail = Some(e.m.id.clone());
                }
            });
        });
    }
    /// The primary action of an add-on: install it, turn it on, or open it.
    fn run_action(&mut self, e: &Entry) {
        if !e.installed {
            self.screen = Screen::Browser;
            self.market.focused = Some(e.m.id.clone());
            return;
        }
        if !e.enabled && !self.set_addon_enabled(&e.m.id, true) {
            return;
        }
        self.open_addon(&e.m.id);
    }
    /// One add-on in the browser: what it is, its state, and its actions.
    fn browser_row(&mut self, ui: &mut egui::Ui, e: &Entry) {
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                glyph(ui, sidebar_icon(&e.m.id, &e.m.icon), accent(&e.m), 22.0);
                let side = 190.0_f32.min(ui.available_width() * 0.4);
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - side - look::GAP).max(120.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&e.m.title)
                                .size(look::TEXT_SECTION)
                                .strong()
                                .color(TEXT),
                        );
                        if e.verified {
                            verified_mark(ui);
                        }
                        if let Some(byline) = &e.m.byline {
                            caption(ui, byline);
                        }
                    });
                    caption(
                        ui,
                        format!("{} · v{} · {}", e.m.category, e.m.version, e.status()),
                    );
                    muted(ui, &e.m.description);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.button("Details").clicked() {
                        self.detail = Some(e.m.id.clone());
                    }
                    if ui.button(e.action()).clicked() {
                        self.run_action(e);
                    }
                });
            });
        });
    }
    pub(super) fn open_addon(&mut self, id: &str) {
        self.open_market_addon(id);
    }
    /// Turns an add-on on or off and saves it. Returns false (with a message) if it could not
    /// be saved.
    fn set_addon_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Err(e) = self.registry.set_enabled(id, enabled) {
            self.message = Some(e);
            return false;
        }
        if !enabled && self.help.as_deref() == Some(id) {
            self.help = None;
        }
        true
    }
    pub(super) fn browser(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "Add-ons",
            "Turn tools on or off, or bring a new add-on into Toolboks.",
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search add-ons")
                    .desired_width((ui.available_width() - 150.0).max(120.0)),
            );
            if primary(ui, "Install package").clicked() {
                self.show_install = !self.show_install;
            }
        });
        ui.add_space(look::GAP);
        if self.show_install {
            panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                section(ui, "Install a package");
                muted(
                    ui,
                    "Drop a .tbaddon file onto this window, or type where it is saved. You review it before anything is installed.",
                );
                ui.add_space(look::GAP);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.install_path)
                            .hint_text("C:\\Downloads\\my-addon.tbaddon")
                            .desired_width((ui.available_width() - 170.0).max(100.0)),
                    );
                    if ui.button("Review").clicked() {
                        let path =
                            std::path::PathBuf::from(self.install_path.trim().trim_matches('"'));
                        self.review_package(&path);
                    }
                });
                ui.add_space(look::GAP);
                if ui.button("Try the example package").clicked() {
                    match toolboks_addon_sdk::Package::parse(
                        addons::SDK_EXAMPLE.as_bytes(),
                        env!("CARGO_PKG_VERSION"),
                    ) {
                        Ok(p) => self.pending_package = Some(p),
                        Err(e) => self.message = Some(e),
                    }
                }
            });
            ui.add_space(look::GAP);
        }
        let needle = self.search.to_lowercase();
        let entries: Vec<Entry> = self
            .entries()
            .into_iter()
            .filter(|e| {
                format!(
                    "{} {} {} {}",
                    e.m.title, e.m.description, e.m.author, e.m.category
                )
                .to_lowercase()
                .contains(&needle)
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("browser-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = look::GAP;
                if entries.is_empty() {
                    muted(
                        ui,
                        "No add-ons match your search. Clear the search to see all of them.",
                    );
                    return;
                }
                let (official, other): (Vec<_>, Vec<_>) = entries.iter().partition(|e| e.verified);
                for (heading, group) in [
                    ("Included with Toolboks", official),
                    ("Added by you", other),
                ] {
                    if group.is_empty() {
                        continue;
                    }
                    ui.add_space(look::SPACE_XS);
                    section(ui, heading);
                    for e in group {
                        self.browser_row(ui, e);
                    }
                }
            });
    }

    fn external_workspace(&mut self, ui: &mut egui::Ui, id: &str) {
        if self.hidden {
            return;
        }
        if !self.registry.enabled(id) {
            self.screen = Screen::Home;
            return;
        }
        if self.remote_workspace(ui, id) {
            return;
        }
        let Some(package) = self
            .registry
            .packages
            .iter()
            .find(|p| p.manifest.id == id)
            .cloned()
        else {
            self.screen = Screen::Home;
            return;
        };
        let color = accent(&package.manifest);
        let page = self.addon_page.min(package.pages.len() - 1);
        self.addon_page = page;
        ui.scope(|ui| {
            ui.visuals_mut().hyperlink_color = color;
            ui.visuals_mut().selection.bg_fill = color.gamma_multiply(0.25);
            Self::title(ui, &package.manifest.title, &package.manifest.description);
            toolboks_addon_sdk::tutorial::mark(ui, "page", ui.available_rect_before_wrap());
            egui::ScrollArea::vertical()
                .id_salt(("addon", id, page))
                .show(ui, |ui| {
                    for (index, w) in package.pages[page].widgets.iter().enumerate() {
                        let is_heading = matches!(w, Widget::Heading { .. });
                        let widget = ui.push_id((id, page, index), |ui| {
                            // More space above a heading than below it.
                            if is_heading && index > 0 {
                                ui.add_space(look::SPACE_XS);
                            }
                            match w {
                                Widget::Heading { text } => {
                                    section(ui, text);
                                }
                                Widget::Text { text } => {
                                    muted(ui, text);
                                }
                                Widget::Note { title, text } => {
                                    panel().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        section(ui, title);
                                        ui.add_space(look::SPACE_XS);
                                        muted(ui, text);
                                    });
                                }
                                Widget::Metric { label, binding } => {
                                    panel().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        // A stat: value in 600, small muted label beside it.
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(metric(*binding))
                                                    .size(look::TEXT_SECTION)
                                                    .strong()
                                                    .color(TEXT),
                                            );
                                            caption(ui, label);
                                        });
                                    });
                                    ui.request_repaint_after(Duration::from_secs(2));
                                }
                                Widget::Toggle {
                                    key,
                                    label,
                                    default,
                                } => {
                                    let prefs = self
                                        .registry
                                        .config
                                        .preferences
                                        .entry(id.into())
                                        .or_default();
                                    let mut value = prefs
                                        .get(key)
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(*default);
                                    if switch_row(ui, label, &mut value) {
                                        prefs.insert(key.clone(), serde_json::json!(value));
                                        self.preferences_dirty = Some(std::time::Instant::now());
                                        ui.request_repaint_after(Duration::from_millis(600));
                                    }
                                }
                                Widget::TextInput {
                                    key,
                                    label,
                                    default,
                                } => {
                                    body(ui, label);
                                    let prefs = self
                                        .registry
                                        .config
                                        .preferences
                                        .entry(id.into())
                                        .or_default();
                                    let mut value = prefs
                                        .get(key)
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(default)
                                        .to_owned();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut value)
                                                .char_limit(4000)
                                                .desired_width(400.0_f32.min(ui.available_width())),
                                        )
                                        .changed()
                                    {
                                        prefs.insert(key.clone(), serde_json::json!(value));
                                        self.preferences_dirty = Some(std::time::Instant::now());
                                        ui.request_repaint_after(Duration::from_millis(600));
                                    }
                                }
                            }
                            ui.add_space(if is_heading {
                                look::SPACE_XS
                            } else {
                                look::SPACE_M
                            });
                        });
                        toolboks_addon_sdk::tutorial::mark(
                            ui,
                            &format!("widget-{index}"),
                            widget.response.rect,
                        );
                    }
                });
        });
        if self.pending_declarative.as_deref() == Some(id) {
            self.pending_declarative = None;
            if let Err(error) = crate::installer::started(&self.registry.root, id) {
                self.message = Some(format!(
                    "The add-on opened, but its version could not be confirmed: {error}"
                ));
            }
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        Self::title(
            ui,
            "Settings",
            "Your preferences stay on this PC, separate from your add-ons.",
        );
        egui::ScrollArea::vertical()
            .id_salt("settings-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = look::GAP;
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(ui, "When I close the window");
                    if switch_row(
                        ui,
                        "Keep Toolboks running in the tray",
                        &mut self.keep_in_tray,
                    ) {
                        self.registry.config.keep_in_tray = self.keep_in_tray;
                        self.persist();
                    }
                    caption(
                        ui,
                        "Background tools keep working. Choose Quit from the tray menu to close the app completely.",
                    );
                });
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(ui, "Where things are saved");
                    muted(
                        ui,
                        "Installed add-ons and preferences are saved in this folder.",
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(self.registry.root.display().to_string())
                                .size(look::TEXT_CAPTION)
                                .color(TEXT),
                        )
                        .wrap()
                        .selectable(true),
                    );
                    caption(
                        ui,
                        if cfg!(feature = "preview") {
                            "This preview has its own folder. Your stable installation is separate."
                        } else {
                            "No account or cloud sync is needed."
                        },
                    );
                });
                panel().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section(ui, "About Toolboks");
                    caption(
                        ui,
                        format!("Version {} · Add-on API 1", env!("CARGO_PKG_VERSION")),
                    );
                    // Read only while this page is drawn, and again every few seconds.
                    let usage = self.own_usage.poll(std::time::Instant::now());
                    caption(ui, crate::own_usage::text(usage));
                    ui.request_repaint_after(crate::own_usage::EVERY);
                    ui.add_space(look::SPACE_XS);
                    self.update_ui(ui);
                });
                if !self.registry.errors.is_empty() {
                    panel().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("Some items need attention")
                                .strong()
                                .color(look::AMBER),
                        );
                        for e in &self.registry.errors {
                            ui.label(e);
                        }
                    });
                }
            });
    }
    pub(super) fn persist(&mut self) {
        if let Err(e) = self.registry.save() {
            self.message = Some(format!("Could not save preferences: {e}"));
        }
    }
    fn review_package(&mut self, path: &std::path::Path) {
        match self.registry.inspect(path) {
            Ok(p) => self.pending_package = Some(p),
            Err(e) => self.message = Some(e),
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.pending_package.clone() {
            let response = egui::Modal::new(egui::Id::unique("install-review"))
                .frame(panel())
                .show(ctx, |ui| {
                    ui.set_width(modal_width(ctx, 480.0));
                    section(ui, "Review before installing");
                    ui.add_space(look::SPACE_XS);
                    ui.label(
                        RichText::new(&p.manifest.title)
                            .size(look::TEXT_TITLE)
                            .strong()
                            .color(TEXT),
                    );
                    caption(
                        ui,
                        format!("{} · Version {}", p.manifest.publisher, p.manifest.version),
                    );
                    ui.add_space(look::GAP);
                    ui.label(&p.manifest.description);
                    ui.add_space(look::GAP);
                    badge(ui, "Local package, not verified", look::AMBER, false);
                    ui.add_space(look::GAP);
                    muted(ui, "This package uses Toolboks-rendered pages. It cannot run native code, open windows, or change system settings.");
                    ui.add_space(look::SPACE_M);
                    section(ui, "It asks to");
                    for line in crate::marketplace::package_access_text(&p.manifest) {
                        match line {
                            crate::marketplace::AccessLine::Data(note) => {
                                ui.add_space(look::GAP);
                                muted(ui, "Data on this PC");
                                ui.label(note);
                            }
                            line => muted(ui, line.text()),
                        }
                    }
                    ui.add_space(look::SPACE_L);
                    ui.horizontal(|ui| {
                        let updating = self
                            .registry
                            .packages
                            .iter()
                            .any(|a| a.manifest.id == p.manifest.id);
                        if primary(ui, if updating { "Install update" } else { "Install add-on" })
                            .clicked()
                        {
                            self.pending_package = None;
                            match self.registry.install(p.clone()) {
                                Ok(()) => {
                                    self.message =
                                        Some(format!("{} is ready in your sidebar.", p.manifest.title))
                                }
                                Err(e) => self.message = Some(e),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_package = None;
                        }
                    });
                });
            if response.should_close() {
                self.pending_package = None;
            }
            return;
        }
        if let Some(id) = self.detail.clone() {
            let manifest = self
                .registry
                .packages
                .iter()
                .find(|p| p.manifest.id == id)
                .map(|p| p.manifest.clone())
                .or_else(|| {
                    crate::installer::installed_entry(&self.registry.root, &id)
                        .map(|entry| manifest_of(&entry))
                });
            if let Some(m) = manifest {
                let response = egui::Modal::new(egui::Id::unique("addon-detail"))
                    .frame(panel())
                    .show(ctx, |ui| {
                        ui.set_width(modal_width(ctx, 500.0));
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&m.title)
                                    .size(look::TEXT_TITLE)
                                    .strong()
                                    .color(TEXT),
                            );
                            if self.registry.verified(&id) {
                                badge(ui, "Toolboks verified", ACCENT_TEXT, true);
                            }
                            if let Some(byline) = &m.byline {
                                caption(ui, byline);
                            }
                        });
                        ui.add_space(look::GAP);
                        egui::ScrollArea::vertical()
                            .max_height((ctx.content_rect().height() - 260.0).clamp(120.0, 380.0))
                            .show(ui, |ui| {
                                ui.label(&m.long_description);
                                if let Some(note) =
                                    crate::marketplace::shown_data_note(m.data_note.as_deref())
                                {
                                    ui.add_space(look::SPACE_M);
                                    muted(ui, "Data on this PC");
                                    ui.add_space(look::SPACE_XS);
                                    ui.label(note);
                                }
                                ui.add_space(look::SPACE_M);
                                egui::Grid::new("addon-metadata")
                                    .spacing([look::SPACE_L, look::SPACE_XS])
                                    .show(ui, |ui| {
                                        for (label, value) in [
                                            ("Author", m.author.as_str()),
                                            ("Publisher", m.publisher.as_str()),
                                            ("Version", m.version.as_str()),
                                            ("Category", m.category.as_str()),
                                            (
                                                "License",
                                                m.license.as_deref().unwrap_or("Not specified"),
                                            ),
                                        ] {
                                            muted(ui, label);
                                            ui.label(value);
                                            ui.end_row();
                                        }
                                    });
                                ui.add_space(look::GAP);
                                if let Some(url) = &m.website {
                                    ui.hyperlink_to("Website", url);
                                }
                                if let Some(url) = &m.support_url {
                                    ui.hyperlink_to("Support", url);
                                }
                                for (i, url) in m.screenshots.iter().enumerate() {
                                    ui.hyperlink_to(format!("Screenshot {}", i + 1), url);
                                }
                            });
                        ui.add_space(look::SPACE_M);
                        let installed = self.registry.packages.iter().any(|p| p.manifest.id == id)
                            || crate::installer::installed_entry(&self.registry.root, &id)
                                .is_some();
                        if installed {
                            let mut enabled = self.registry.enabled(&id);
                            if switch_row(ui, "Show in sidebar", &mut enabled) {
                                self.set_addon_enabled(&id, enabled);
                            }
                            ui.add_space(look::GAP);
                            if ui
                                .button(RichText::new("Remove add-on").color(look::RED))
                                .clicked()
                            {
                                self.detail = None;
                                self.remove_pending = Some(id.clone());
                            }
                        } else {
                            muted(ui, "Not installed yet. Install it from the add-ons page.");
                        }
                        ui.add_space(look::GAP);
                        if ui.button("Close").clicked() {
                            self.detail = None;
                        }
                    });
                if response.should_close() {
                    self.detail = None;
                }
            } else {
                self.detail = None;
            }
            return;
        }
        if let Some(id) = self.remove_pending.take() {
            self.market.remove = Some((id, false));
        }
    }
}
fn metric(binding: Metric) -> String {
    use windows::Win32::System::SystemInformation::{
        GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };
    if matches!(binding, Metric::Uptime) {
        let s = unsafe { GetTickCount64() } / 1000;
        return format!("{}h {}m", s / 3600, s % 3600 / 60);
    }
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return "Unavailable".into();
    }
    match binding {
        Metric::MemoryPercent => format!("{}%", m.dwMemoryLoad),
        Metric::MemoryAvailable => format!("{:.1} GiB", m.ullAvailPhys as f64 / 1073741824.0),
        Metric::MemoryTotal => format!("{:.1} GiB", m.ullTotalPhys as f64 / 1073741824.0),
        Metric::Uptime => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut egui::Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(400.0, 600.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, add);
        output.textures_delta.clear();
    }

    fn tab() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    /// Draws the sidebar's foot the way the shell does: in a panel the width of the sidebar.
    fn draw_bottom(ui: &mut egui::Ui, screen: &mut Screen, expanded: bool) {
        egui::Panel::left("test-sidebar")
            .exact_size(if expanded { SIDEBAR_W } else { RAIL_W })
            .show(ui, |ui| bottom_group(ui, screen, expanded));
    }

    /// The top edge of the widget that has keyboard focus.
    fn focused_top(ctx: &egui::Context) -> Option<f32> {
        let id = ctx.memory(|m| m.focused())?;
        Some(ctx.read_response(id)?.rect.top())
    }

    #[test]
    fn tab_goes_down_the_foot_of_the_sidebar_in_the_order_shown() {
        for (expanded, screen) in [
            (true, Screen::Browser),
            (true, Screen::Home),
            (false, Screen::Browser),
        ] {
            let ctx = egui::Context::default();
            let mut screen = screen;
            run(&ctx, vec![], |ui| draw_bottom(ui, &mut screen, expanded));
            let mut tops = Vec::new();
            for _ in 0..4 {
                run(&ctx, tab(), |ui| draw_bottom(ui, &mut screen, expanded));
                run(&ctx, vec![], |ui| draw_bottom(ui, &mut screen, expanded));
                tops.extend(focused_top(&ctx));
            }
            let rows = if expanded && screen == Screen::Browser {
                3
            } else {
                2
            };
            assert!(tops.len() >= rows, "{tops:?}");
            let tops = &tops[..rows];
            assert!(
                tops.windows(2).all(|pair| pair[0] < pair[1]),
                "Tab must go top to bottom, got {tops:?} (expanded {expanded})"
            );
        }
    }

    #[test]
    fn bottom_group_sits_on_the_bottom_edge_and_matches_its_height() {
        for show_pick in [false, true] {
            let ctx = egui::Context::default();
            let mut screen = if show_pick {
                Screen::Pick
            } else {
                Screen::Home
            };
            let mut seen = None;
            run(&ctx, vec![], |ui| {
                egui::Panel::left("test-sidebar")
                    .exact_size(SIDEBAR_W)
                    .show(ui, |ui| {
                        let full = ui.max_rect();
                        let before = ui.min_rect().bottom();
                        bottom_group(ui, &mut screen, true);
                        let used = ui.min_rect().bottom();
                        seen = Some((full.bottom(), before, used));
                    });
            });
            let (edge, _, used) = seen.unwrap();
            assert!(
                (used - edge).abs() < 0.5,
                "the group ends at the bottom edge: {used} vs {edge}"
            );
        }
        assert_eq!(bottom_group_height(false), 77.0);
        assert_eq!(bottom_group_height(true), 77.0 + PAGE_ROW_H + ROW_GAP);
    }

    #[test]
    fn collapsed_rail_row_shows_its_name_while_it_has_keyboard_focus() {
        let ctx = egui::Context::default();
        let draw = |ui: &mut egui::Ui| {
            egui::Panel::left("test-sidebar")
                .exact_size(RAIL_W)
                .show(ui, |ui| {
                    nav(ui, "Home", "home", false, false, Tag::None);
                });
        };
        let tooltip_open = |ctx: &egui::Context| {
            ctx.memory(|m| {
                m.areas()
                    .visible_layer_ids()
                    .iter()
                    .any(|layer| layer.order == egui::Order::Tooltip)
            })
        };
        crate::ui::apply_style(&ctx);
        let away = vec![egui::Event::PointerMoved(egui::pos2(300.0, 300.0))];
        run(&ctx, away.clone(), draw);
        run(&ctx, vec![], draw);
        assert!(!tooltip_open(&ctx), "no tooltip without focus or hover");
        run(&ctx, tab(), draw);
        run(&ctx, vec![], draw);
        run(&ctx, vec![], draw);
        assert!(ctx.memory(|m| m.focused()).is_some(), "the row took focus");
        assert!(tooltip_open(&ctx), "the focused row shows its name");
        let rect = ctx.memory(|m| {
            m.areas()
                .visible_layer_ids()
                .iter()
                .find(|layer| layer.order == egui::Order::Tooltip)
                .and_then(|layer| m.area_rect(layer.id))
        });
        assert!(rect.is_some_and(|r| r.width() > 10.0 && r.height() > 10.0));
    }

    #[test]
    fn shell_fits_the_minimum_window() {
        let [width, height] = MIN_INNER_SIZE;
        // The content next to the widest sidebar keeps room for the header and the stacked Home.
        assert!(width - SIDEBAR_W >= 760.0, "{}", width - SIDEBAR_W);
        // The sidebar's own column: menu row, a few rows of the scrolling list, and the foot with
        // its page row.
        let column = 16.0 + ROW_H + look::GAP + 4.0 * ROW_H + bottom_group_height(true) + 4.0;
        assert!(column <= height - STATUS_H, "{column}");
    }

    /// The estimates behind the header's switch to a menu must never be smaller than the real
    /// widgets, or the header could still overlap just before it switches.
    #[test]
    fn header_estimates_are_not_smaller_than_the_widgets() {
        let ctx = egui::Context::default();
        let mut checked = false;
        run(&ctx, vec![], |ui| {
            let labels = [
                "Show me around",
                "View details",
                "Refresh list",
                "Install from file",
                "Pick your tools",
            ];
            let real = ui
                .horizontal(|ui| {
                    for label in labels {
                        let _ = ui.button(label);
                    }
                    let _ = ui.checkbox(&mut false, "Installed only");
                })
                .response
                .rect
                .width();
            let need = market_actions_width(ui, true);
            assert!(need >= real, "{need} < {real}");
            let names: Vec<String> = ["Overview", "Processes", "Connections", "Details"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let real = ui
                .horizontal(|ui| {
                    for name in &names {
                        let _ = look::pill(ui, name, false);
                    }
                })
                .response
                .rect
                .width();
            let need = pills_width(ui, &names);
            assert!(need >= real, "pills {need} < {real}");
            let real = ui.button("Help").rect.width();
            assert!(buttons_width(ui, &["Help"], 0.0) >= real);
            checked = true;
        });
        assert!(checked);
    }
}
