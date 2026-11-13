use super::*;
use crate::ui::{BORDER, HOST_ACCENT as ACCENT, MUTED, PANEL_BG, SIDEBAR_BG, TEXT, WINDOW_BG};
use egui::{Color32, RichText, Vec2};
use toolboks_addon_sdk::{Manifest, Metric, Package, Widget};

#[derive(Clone, PartialEq)]
pub(super) enum Screen {
    Home,
    Browser,
    Settings,
    /// A built-in tool, by its position in `tools::all()`.
    Tool(usize),
    External(String),
}

/// Body text (12 px) in the muted token.
fn muted(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.label(RichText::new(s.into()).size(look::TEXT_BODY).color(MUTED));
}
/// A control label (12 px, full-strength text).
fn body(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(look::TEXT_BODY).color(TEXT));
}
fn caption(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.label(
        RichText::new(s.into())
            .size(look::TEXT_CAPTION)
            .color(MUTED),
    );
}
/// A section heading: 14 px, strong.
fn section(ui: &mut egui::Ui, s: &str) {
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
            &m.icon,
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
const RAIL_W: f32 = 52.0;
const SIDEBAR_W: f32 = 204.0;
const HEADER_H: f32 = 46.0;
const STATUS_H: f32 = 24.0;
const ROW_H: f32 = 32.0;
const PAGE_ROW_H: f32 = 28.0;
/// Where a sidebar row's label starts, measured from the row's left edge.
const LABEL_X: f32 = 40.0;

/// Paints the keyboard focus ring over a custom-drawn control.
fn focus_ring(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
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
fn nav_row(
    ui: &mut egui::Ui,
    label: &str,
    icon: &str,
    selected: bool,
    show_label: bool,
) -> egui::Response {
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
            ui.painter().with_clip_rect(rect).text(
                egui::pos2(rect.left() + LABEL_X, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(look::TEXT_SECTION),
                if selected { TEXT } else { MUTED },
            );
        }
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A navigation row; with the rail collapsed it shows its name as a tooltip. True when clicked.
fn nav(ui: &mut egui::Ui, label: &str, icon: &str, selected: bool, expanded: bool) -> bool {
    let response = nav_row(ui, label, icon, selected, expanded);
    if expanded {
        response.clicked()
    } else {
        response.on_hover_text(label).clicked()
    }
}

/// A page nested under the active tool in the expanded sidebar; selected = lighter fill.
fn page_row(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
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
        let expanded = self.sidebar_expanded;
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
                let response = nav_row(ui, tip, "menu", false, false);
                if expanded {
                    ui.painter().text(
                        egui::pos2(response.rect.left() + LABEL_X, response.rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        "Toolboks",
                        egui::FontId::proportional(look::TEXT_SECTION),
                        TEXT,
                    );
                }
                if response.on_hover_text(tip).clicked() {
                    self.sidebar_expanded = !expanded;
                }
                ui.add_space(look::GAP);

                let tools: Vec<_> = (0..self.registry.builtins.len())
                    .filter(|&i| self.tool_enabled(i))
                    .map(|i| {
                        let m = &self.registry.builtins[i];
                        (i, m.title.clone(), m.icon.clone())
                    })
                    .collect();
                let installed: Vec<_> = self
                    .registry
                    .packages
                    .iter()
                    .filter(|p| self.registry.enabled(&p.manifest.id))
                    .map(|p| {
                        let provider = p.manifest.native_provider.clone();
                        let pages: Vec<String> = if let Some(provider) = &provider {
                            crate::native::NativeHost::pages(provider)
                                .iter()
                                .map(|p| p.to_string())
                                .collect()
                        } else {
                            p.pages.iter().map(|page| page.title.clone()).collect()
                        };
                        (
                            p.manifest.id.clone(),
                            p.manifest.title.clone(),
                            p.manifest.icon.clone(),
                            provider,
                            pages,
                        )
                    })
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt("sidebar-nav")
                    .max_height((ui.available_height() - 2.0 * ROW_H - 8.0).max(50.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if nav(ui, "Home", "home", self.screen == Screen::Home, expanded) {
                            self.screen = Screen::Home;
                        }
                        for (i, title, icon) in tools {
                            let on = self.screen == Screen::Tool(i);
                            if nav(ui, &title, &icon, on, expanded) {
                                self.open_tool(i);
                            }
                            if on && expanded {
                                let pages = tools::WORKSPACES[i].1;
                                if pages.len() > 1 {
                                    for (p, name) in pages.iter().enumerate() {
                                        if page_row(ui, name, self.base.selected.1 == p) {
                                            self.base.selected.1 = p;
                                        }
                                    }
                                }
                            }
                        }
                        for (id, title, icon, provider, pages) in installed {
                            let on = self.screen == Screen::External(id.clone());
                            if nav(ui, &title, &icon, on, expanded) {
                                self.screen = Screen::External(id.clone());
                                self.addon_page = 0;
                            }
                            if on && expanded && pages.len() > 1 {
                                let page = provider
                                    .as_deref()
                                    .map_or(self.addon_page, |p| self.native.page_index(p));
                                for (p, name) in pages.iter().enumerate() {
                                    if page_row(ui, name, page == p) {
                                        if let Some(provider) = &provider {
                                            self.native.navigate(provider, &p.to_string());
                                        } else {
                                            self.addon_page = p;
                                        }
                                    }
                                }
                            }
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let browser_on = self.screen == Screen::Browser;
                    if nav(ui, "Add-ons", "grid", browser_on, expanded) {
                        self.screen = Screen::Browser;
                    }
                    let settings_on = self.screen == Screen::Settings;
                    if nav(ui, "Settings", "sliders", settings_on, expanded) {
                        self.screen = Screen::Settings;
                    }
                });
            });

        // The header: breadcrumb at the left, page controls at the right.
        let crumb = match self.screen.clone() {
            Screen::Home => Crumb::plain("Home"),
            Screen::Browser => Crumb::plain("Add-ons"),
            Screen::Settings => Crumb::plain("Settings"),
            Screen::Tool(i) => {
                let m = &self.registry.builtins[i];
                let pages: Vec<String> = tools::WORKSPACES[i]
                    .1
                    .iter()
                    .map(|p| p.to_string())
                    .collect();
                let page = self.base.selected.1;
                Crumb {
                    root: m.title.clone(),
                    byline: None,
                    leaf: pages.get(page).filter(|_| pages.len() > 1).cloned(),
                    pages,
                    page,
                    help: Some(m.id.clone()),
                    verified: true,
                }
            }
            Screen::External(id) => {
                let package = self.registry.packages.iter().find(|p| p.manifest.id == id);
                let provider = package.and_then(|p| p.manifest.native_provider.as_deref());
                let pages: Vec<String> = package
                    .map(|p| match provider {
                        Some(provider) => crate::native::NativeHost::pages(provider)
                            .iter()
                            .map(|page| page.to_string())
                            .collect(),
                        None => p.pages.iter().map(|page| page.title.clone()).collect(),
                    })
                    .unwrap_or_default();
                let page = provider.map_or(self.addon_page, |p| self.native.page_index(p));
                Crumb {
                    root: package.map_or_else(|| id.clone(), |p| p.manifest.title.clone()),
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
                        if let Some(id) = &crumb.help {
                            if ui.button("Help").clicked() {
                                self.help = Some(id.clone());
                            }
                        }
                        // With the rail collapsed the nested pages are out of sight, so the
                        // pages stay reachable here.
                        if !expanded && crumb.byline.is_some() && crumb.pages.len() > 1 {
                            egui::ComboBox::from_id_salt("addon-page-selector")
                                .selected_text("Pages")
                                .show_ui(ui, |ui| {
                                    for (i, name) in crumb.pages.iter().enumerate() {
                                        if ui.selectable_label(crumb.page == i, name).clicked() {
                                            chosen_page = Some(i);
                                        }
                                    }
                                });
                        } else if !expanded && crumb.pages.len() > 1 {
                            for (i, name) in crumb.pages.iter().enumerate().rev() {
                                if look::pill(ui, name, crumb.page == i).clicked() {
                                    chosen_page = Some(i);
                                }
                            }
                        }
                    });
                });
            });
        if let Some(page) = chosen_page {
            match self.screen {
                Screen::Tool(_) => self.base.selected.1 = page,
                Screen::External(ref id) => {
                    let provider = self
                        .registry
                        .packages
                        .iter()
                        .find(|p| &p.manifest.id == id)
                        .and_then(|p| p.manifest.native_provider.as_deref());
                    if let Some(provider) = provider {
                        self.native.navigate(provider, &page.to_string());
                    } else {
                        self.addon_page = page;
                    }
                }
                _ => {}
            }
        }

        self.sync_workspaces();
        let items = status_items(
            self.base
                .active
                .and_then(|i| self.base.tools[i].as_ref())
                .map(|t| t.status()),
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
                    Screen::Browser => self.browser(ui),
                    Screen::Settings => self.settings(ui),
                    Screen::Tool(i) => self.tool_workspace(ui, i),
                    Screen::External(id) => self.external_workspace(ui, &id),
                }
            });
        self.dialogs(ui.ctx());
        self.update_prompt(ui.ctx());
        self.tutorial_ui(ui.ctx());
    }
    fn title(ui: &mut egui::Ui, title: &str, description: &str) {
        ui.label(
            RichText::new(title)
                .size(look::TEXT_TITLE)
                .strong()
                .color(TEXT),
        );
        muted(ui, description);
        ui.add_space(look::SPACE_M);
    }
    /// Every add-on the person can see: the built-in tools, the Disk Manager and any package they
    /// installed, with its state.
    fn entries(&self) -> Vec<Entry> {
        let mut list = Vec::new();
        for (i, m) in self.registry.builtins.iter().enumerate() {
            list.push(Entry {
                m: m.clone(),
                verified: true,
                installed: true,
                enabled: self.tool_enabled(i),
            });
        }
        for id in addons::OFFICIAL_ADDONS {
            if let Some(package) = addons::official_package(id) {
                list.push(Entry {
                    enabled: self.registry.enabled(id),
                    installed: self.registry.packages.iter().any(|p| p.manifest.id == id),
                    verified: true,
                    m: package.manifest,
                });
            }
        }
        for p in &self.registry.packages {
            if addons::OFFICIAL_ADDONS.contains(&p.manifest.id.as_str()) {
                continue;
            }
            list.push(Entry {
                m: p.manifest.clone(),
                verified: false,
                installed: true,
                enabled: self.registry.enabled(&p.manifest.id),
            });
        }
        list
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        let entries = self.entries();
        let total_tools = self.registry.builtins.len();
        let on_tools = (0..total_tools).filter(|&i| self.tool_enabled(i)).count();
        let added = self.registry.packages.len();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Self::title(
                    ui,
                    "Your toolkit",
                    &format!(
                        "{on_tools} of {total_tools} built-in tools are on. {added} {} installed.",
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
                                if list_row(ui, &e.m, e.status(), tint, i == selected, accent(&e.m))
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
            ui.add_space(look::GAP);
            ui.label(&e.m.description);
            ui.add_space(look::SPACE_L);
            ui.horizontal_wrapped(|ui| {
                if primary(ui, e.action()).clicked() {
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
            self.pending_package = addons::official_package(&e.m.id);
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
                glyph(ui, &e.m.icon, accent(&e.m), 22.0);
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
    fn open_addon(&mut self, id: &str) {
        if let Some(tool) = self.registry.builtin_index(id) {
            self.open_tool(tool);
            return;
        }
        self.screen = Screen::External(id.into());
        self.addon_page = 0;
    }
    fn tool_enabled(&self, tool: usize) -> bool {
        self.registry
            .builtins
            .get(tool)
            .is_some_and(|m| self.registry.enabled(&m.id))
    }
    fn open_tool(&mut self, tool: usize) {
        if !self.tool_enabled(tool) {
            self.screen = Screen::Home;
            return;
        }
        if self.base.selected.0 != tool {
            self.base.selected = (tool, 0);
        }
        self.screen = Screen::Tool(tool);
    }
    /// Turns an add-on on or off and saves it. Returns false (with a message) if it could not
    /// be saved. Turning a built-in tool off releases its session.
    fn set_addon_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Err(e) = self.registry.set_enabled(id, enabled) {
            self.message = Some(e);
            return false;
        }
        if let (Some(tool), false) = (self.registry.builtin_index(id), enabled) {
            self.base.tools[tool] = None;
            if self.screen == Screen::Tool(tool) {
                self.screen = Screen::Home;
            }
            if self.tour.as_ref().is_some_and(|run| run.tool == Some(tool)) {
                self.tour = None;
            }
            if self.help.as_deref() == Some(id) {
                self.help = None;
            }
        }
        true
    }
    fn browser(&mut self, ui: &mut egui::Ui) {
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
                    match Package::parse(addons::SDK_EXAMPLE.as_bytes(), env!("CARGO_PKG_VERSION"))
                    {
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

    fn tool_workspace(&mut self, ui: &mut egui::Ui, tool: usize) {
        if !self.tool_enabled(tool) {
            self.screen = Screen::Home;
            return;
        }
        if self.base.selected.0 != tool {
            self.base.selected = (tool, 0);
        }
        let page = self.base.selected.1;
        if let Some(workspace) = &mut self.base.tools[tool] {
            workspace.ui(ui, page);
        }
    }
    fn external_workspace(&mut self, ui: &mut egui::Ui, id: &str) {
        if self.hidden {
            return;
        }
        if !self.registry.enabled(id) {
            self.screen = Screen::Home;
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
        if let Some(provider) = &package.manifest.native_provider {
            let preferences = self
                .registry
                .config
                .preferences
                .entry(id.into())
                .or_default();
            if self
                .native
                .ui(provider, ui, &package.manifest.capabilities, preferences)
            {
                self.preferences_dirty = Some(std::time::Instant::now());
                ui.request_repaint_after(Duration::from_millis(600));
            }
            if let Some(request) = self.native.take_host_request(provider) {
                match request {
                    toolboks_addon_sdk::HostRequest::ReviewUpdate => {
                        self.pending_package = addons::official_package(id)
                    }
                    toolboks_addon_sdk::HostRequest::RestartElevated { page } => {
                        if self.install_in_progress() {
                            self.message = Some(
                                "Wait for the update to finish before restarting Toolboks.".into(),
                            );
                            return;
                        }
                        if crate::elevation::relaunch_elevated_with_args(&[
                            "--open-addon",
                            id,
                            "--addon-page",
                            &page,
                        ]) {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        } else {
                            self.message = Some(
                                "The administrator restart was cancelled or could not be started."
                                    .into(),
                            );
                        }
                    }
                }
            }
            return;
        }
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
    fn persist(&mut self) {
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
                    if addons::trusted_native(&p) {
                        badge(ui, "Toolboks verified", ACCENT_TEXT, true);
                    } else {
                        badge(ui, "Local package, not verified", look::AMBER, false);
                    }
                    ui.add_space(look::GAP);
                    muted(ui, if p.manifest.id == addons::MACRO_ID {
                        "Recording captures keyboard and mouse input only while you record. Playback sends real input to your desktop. Escape stops recording or playback. Macros and settings are saved locally."
                    } else if p.manifest.native_provider.is_some() {
                        "Folder scans read metadata. Reports are saved only when you choose. This update also adds administrator-only fragmentation analysis, HDD defrag and SSD TRIM; optimization modifies storage metadata only when you request it."
                    } else {
                        "This package uses Toolboks-rendered pages. It cannot run native code, open windows, or change system settings."
                    });
                    ui.add_space(look::SPACE_M);
                    section(ui, "It asks to");
                    for c in &p.manifest.capabilities {
                        muted(ui, match c {
                            toolboks_addon_sdk::Capability::HostMetrics => "Read basic memory and uptime readings",
                            toolboks_addon_sdk::Capability::LocalPreferences => "Save its own preferences locally",
                            toolboks_addon_sdk::Capability::FilesystemRead => "Read file and folder metadata for locations you select",
                            toolboks_addon_sdk::Capability::UserExports => "Save reports to a destination you choose",
                            toolboks_addon_sdk::Capability::DriveOptimize => "Analyze fragmentation and run Windows defrag or TRIM only when you request it (administrator access required)",
                            toolboks_addon_sdk::Capability::LiveAudio => "Open the selected microphone and route live audio to outputs you enable",
                            toolboks_addon_sdk::Capability::SystemTuning => "Back up and change the Windows settings and power plan you select",
                            toolboks_addon_sdk::Capability::InputAutomation => "Record keyboard and mouse input and replay it only when you start a macro",
                        });
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
            let manifest = if let Some(tool) = self.registry.builtin_index(&id) {
                Some(self.registry.builtins[tool].clone())
            } else if let Some(package) = addons::official_package(&id) {
                Some(package.manifest)
            } else {
                self.registry
                    .packages
                    .iter()
                    .find(|p| p.manifest.id == id)
                    .map(|p| p.manifest.clone())
            };
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
                        let builtin = self.registry.builtin_index(&id).is_some();
                        let package = self.registry.packages.iter().any(|p| p.manifest.id == id);
                        if builtin || package {
                            let mut enabled = self.registry.enabled(&id);
                            if switch_row(ui, "Show in sidebar", &mut enabled) {
                                self.set_addon_enabled(&id, enabled);
                            }
                            ui.add_space(look::GAP);
                            if package {
                                if ui
                                    .button(RichText::new("Remove add-on").color(look::RED))
                                    .clicked()
                                {
                                    self.detail = None;
                                    self.remove_pending = Some(id.clone());
                                }
                            } else {
                                caption(
                                    ui,
                                    "Included with Toolboks and updated with the app. Turn it off to stop it running in the background.",
                                );
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
        if let Some(id) = self.remove_pending.clone() {
            let name = self
                .registry
                .packages
                .iter()
                .find(|p| p.manifest.id == id)
                .map(|p| p.manifest.title.as_str())
                .unwrap_or(&id)
                .to_owned();
            let response = egui::Modal::new(egui::Id::unique("remove-addon"))
                .frame(panel())
                .show(ctx, |ui| {
                    ui.set_width(modal_width(ctx, 420.0));
                    ui.label(
                        RichText::new(format!("Remove {name}?"))
                            .size(look::TEXT_TITLE)
                            .strong()
                            .color(TEXT),
                    );
                    ui.add_space(look::GAP);
                    muted(
                        ui,
                        "The add-on and its saved preferences will be removed from this PC.",
                    );
                    ui.add_space(look::SPACE_L);
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Remove add-on").color(look::RED))
                            .clicked()
                        {
                            match self.registry.uninstall(&id) {
                                Ok(()) => {
                                    if self.screen == Screen::External(id.clone()) {
                                        self.screen = Screen::Home;
                                    }
                                    self.message = Some("Add-on removed.".into());
                                }
                                Err(e) => self.message = Some(e),
                            }
                            self.remove_pending = None;
                        }
                        if ui.button("Keep add-on").clicked() {
                            self.remove_pending = None;
                        }
                    });
                });
            if response.should_close() {
                self.remove_pending = None;
            }
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
