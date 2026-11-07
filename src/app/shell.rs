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

fn eyebrow(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s.to_uppercase()).size(10.0).color(MUTED));
}
fn muted(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.label(RichText::new(s.into()).color(MUTED));
}
fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(12)
        .inner_margin(20)
}
fn primary(ui: &mut egui::Ui, s: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(s.replace('→', ">")).color(Color32::from_rgb(245, 240, 251)),
        )
        .fill(look::HOST_ACCENT_FILL)
        .corner_radius(7)
        .min_size(Vec2::new(90.0, 34.0)),
    )
}
fn badge(ui: &mut egui::Ui, s: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if s.starts_with('✓') {
                    verified_mark(ui);
                }
                ui.label(
                    RichText::new(s.trim_start_matches('✓').trim())
                        .size(10.0)
                        .color(color),
                );
            });
        });
}
fn accent(m: &Manifest) -> Color32 {
    let rgb = u32::from_str_radix(&m.accent[1..], 16).unwrap_or(0xa48aca);
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}
fn glyph(ui: &mut egui::Ui, name: &str, color: Color32, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_glyph(ui.painter(), r, name, color, size);
}
fn brand_logo(ui: &mut egui::Ui, size: f32) {
    let id = egui::Id::unique("toolboks-original-logo");
    let existing = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = existing.unwrap_or_else(|| {
        let icon =
            eframe::icon_data::from_png_bytes(include_bytes!("../../assets/toolboks-logo.png"))
                .expect("bundled Toolboks logo");
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        );
        let texture = ui
            .ctx()
            .load_texture("toolboks-logo", image, egui::TextureOptions::LINEAR);
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    ui.add(egui::Image::new((texture.id(), Vec2::splat(size))));
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
                        let pages: Vec<String> = if p.manifest.native_provider.is_some() {
                            Vec::new()
                        } else {
                            p.pages.iter().map(|page| page.title.clone()).collect()
                        };
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
                                let pages = self.base.tools[i].pages();
                                if pages.len() > 1 {
                                    for (p, name) in pages.iter().enumerate() {
                                        if page_row(ui, name, self.base.selected.1 == p) {
                                            self.base.selected.1 = p;
                                        }
                                    }
                                }
                            }
                        }
                        for (id, title, icon, pages) in installed {
                            let on = self.screen == Screen::External(id.clone());
                            if nav(ui, &title, &icon, on, expanded) {
                                self.screen = Screen::External(id.clone());
                                self.addon_page = 0;
                            }
                            if on && expanded && pages.len() > 1 {
                                for (p, name) in pages.iter().enumerate() {
                                    if page_row(ui, name, self.addon_page == p) {
                                        self.addon_page = p;
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
                let pages: Vec<String> = self.base.tools[i]
                    .pages()
                    .iter()
                    .map(|p| p.to_string())
                    .collect();
                let page = self.base.selected.1;
                Crumb {
                    root: m.title.clone(),
                    leaf: pages.get(page).filter(|_| pages.len() > 1).cloned(),
                    pages,
                    page,
                    help: Some(m.id.clone()),
                    verified: true,
                }
            }
            Screen::External(id) => {
                let package = self.registry.packages.iter().find(|p| p.manifest.id == id);
                let pages: Vec<String> = package
                    .filter(|p| p.manifest.native_provider.is_none())
                    .map(|p| p.pages.iter().map(|page| page.title.clone()).collect())
                    .unwrap_or_default();
                let page = self.addon_page;
                Crumb {
                    root: package.map_or_else(|| id.clone(), |p| p.manifest.title.clone()),
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
                            ui.label(RichText::new("/").size(size).color(MUTED));
                            ui.label(RichText::new(leaf).size(size).strong().color(TEXT));
                        }
                        None => {
                            ui.label(RichText::new(&crumb.root).size(size).strong().color(TEXT));
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
                        if !expanded && crumb.pages.len() > 1 {
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
                Screen::External(_) => self.addon_page = page,
                _ => {}
            }
        }

        let items = status_items(
            self.base
                .tools
                .iter()
                .enumerate()
                .filter(|(i, _)| self.tool_enabled(*i))
                .map(|(_, t)| t.status()),
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
                    egui::Frame::new()
                        .fill(Color32::from_rgb(38, 43, 34))
                        .corner_radius(7)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(message).color(ACCENT));
                                if ui.small_button("Dismiss").clicked() {
                                    self.message = None;
                                }
                            });
                        });
                    ui.add_space(12.0);
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
    fn title(ui: &mut egui::Ui, eye: &str, title: &str, description: &str) {
        eyebrow(ui, eye);
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(30.0).color(TEXT));
        ui.add_space(3.0);
        muted(ui, description);
        ui.add_space(24.0);
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("home-scroll").show(ui,|ui|{
            Self::title(ui,"See the bigger picture","A little clarity. A lot more control.","One workspace for the tools that make your system yours.");
            panel().show(ui,|ui|{ui.set_width(ui.available_width());ui.set_min_height(188.0);ui.horizontal(|ui|{ui.vertical(|ui|{badge(ui,"MADE FOR THE CURIOUS",ACCENT);ui.add_space(16.0);ui.label(RichText::new("Know your PC.\nMake it work for you.").size(32.0));ui.add_space(10.0);muted(ui,"Understand what is happening. Find the right tool. Stay in control.");ui.add_space(18.0);if primary(ui,"Get started  →").clicked(){match (0..self.registry.builtins.len()).find(|&i|self.tool_enabled(i)){Some(i)=>self.open_tool(i),None=>self.screen=Screen::Browser}}});ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.add_space(32.0);brand_logo(ui,112.0);});});});
            ui.add_space(17.0);
            ui.columns(3, |cols| {
                let total_tools=self.registry.builtins.len();let on_tools=(0..total_tools).filter(|&i|self.tool_enabled(i)).count();let stats = [("Installed add-ons",format!("{:02}",total_tools+self.registry.packages.len()),"Your personal toolkit".to_owned()),("Included system tools",format!("{on_tools:02}"),format!("{on_tools} of {total_tools} turned on")),("Your workspace","Local".into(),"Preferences saved on this PC".to_owned())];
                for (col,(label,value,desc)) in cols.iter_mut().zip(stats) {
                    panel().show(col,|ui| {ui.set_width(ui.available_width());eyebrow(ui,label);ui.add_space(11.0);ui.label(RichText::new(value).size(28.0));ui.add_space(6.0);muted(ui,desc);});
                }
            });
            ui.add_space(26.0);ui.horizontal(|ui|{ui.label(RichText::new("Your starting point").size(16.0));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.link("Explore add-ons >").clicked(){self.screen=Screen::Browser;}});});ui.add_space(12.0);
            let builtins=self.registry.builtins.clone();for (i,m) in builtins.iter().enumerate(){let on=self.tool_enabled(i);self.addon_card(ui,m,true,on);ui.add_space(12.0);}
            ui.add_space(12.0);muted(ui,"Build a toolkit around the way you use your PC. Add-ons live right here, inside Toolboks.");
        });
    }
    fn addon_card(&mut self, ui: &mut egui::Ui, m: &Manifest, verified: bool, enabled: bool) {
        let installed = self.registry.builtin_index(&m.id).is_some()
            || self.registry.packages.iter().any(|p| p.manifest.id == m.id);
        panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(accent(m).gamma_multiply(0.10))
                    .corner_radius(10)
                    .inner_margin(13)
                    .show(ui, |ui| glyph(ui, &m.icon, accent(m), 28.0));
                ui.add_space(9.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&m.title).size(19.0));
                        if let Some(byline) = &m.byline {
                            ui.label(
                                RichText::new(byline)
                                    .size(11.0)
                                    .color(Color32::from_rgb(112, 108, 121)),
                            );
                        }
                        if verified {
                            verified_mark(ui);
                        }
                    });
                    muted(
                        ui,
                        format!("{}  ·  {}  ·  v{}", m.publisher, m.category, m.version),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    badge(
                        ui,
                        if !installed {
                            "Available"
                        } else if enabled {
                            "Installed"
                        } else {
                            "Disabled"
                        },
                        if enabled { ACCENT } else { MUTED },
                    );
                });
            });
            ui.add_space(14.0);
            muted(ui, &m.description);
            ui.add_space(17.0);
            ui.horizontal(|ui| {
                if primary(
                    ui,
                    if !installed {
                        "Install add-on"
                    } else if enabled {
                        "Open add-on  →"
                    } else {
                        "Enable add-on"
                    },
                )
                .clicked()
                {
                    if !installed {
                        self.pending_package = Some(addons::official_disk_package());
                        return;
                    }
                    if !enabled && !self.set_addon_enabled(&m.id, true) {
                        return;
                    }
                    self.open_addon(&m.id);
                }
                if ui.button("View details").clicked() {
                    self.detail = Some(m.id.clone());
                }
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
    /// be saved. Turning a built-in tool off stops its background work for good: the tool is
    /// replaced by a fresh idle one, which a later turn-on starts from scratch.
    fn set_addon_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Err(e) = self.registry.set_enabled(id, enabled) {
            self.message = Some(e);
            return false;
        }
        if let (Some(tool), false) = (self.registry.builtin_index(id), enabled) {
            self.base.tools[tool] = tools::build(tool);
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
            "Make room for more",
            "Tools that belong in your toolkit.",
            "Discover your installed add-ons, or bring a new one into your workspace.",
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search add-ons…")
                    .desired_width((ui.available_width() - 180.0).max(200.0)),
            );
            if primary(ui, "+ Install package").clicked() {
                self.show_install = !self.show_install;
            }
        });
        ui.add_space(13.0);
        if self.show_install {
            panel().show(ui, |ui| {
                ui.label(RichText::new("Bring an add-on into Toolboks").size(16.0));
                muted(
                    ui,
                    "Drop a .tbaddon package onto this window, or enter its full path below.",
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.install_path)
                            .hint_text("C:\\Downloads\\my-addon.tbaddon")
                            .desired_width((ui.available_width() - 100.0).max(100.0)),
                    );
                    if ui.button("Review").clicked() {
                        let path =
                            std::path::PathBuf::from(self.install_path.trim().trim_matches('"'));
                        self.review_package(&path);
                    }
                });
            });
            ui.add_space(15.0);
        }
        let needle = self.search.to_lowercase();
        let matches = |m: &Manifest| {
            format!("{} {} {} {}", m.title, m.description, m.author, m.category)
                .to_lowercase()
                .contains(&needle)
        };
        let builtins = self.registry.builtins.clone();
        let packages: Vec<_> = self
            .registry
            .packages
            .iter()
            .map(|p| p.manifest.clone())
            .filter(&matches)
            .collect();
        let disk = addons::official_disk_package().manifest;
        egui::ScrollArea::vertical().id_salt("browser-scroll").show(ui,|ui|{
            eyebrow(ui,"From Toolboks");ui.add_space(10.0);
            for (i,m) in builtins.iter().enumerate(){if matches(m){let on=self.tool_enabled(i);self.addon_card(ui,m,true,on);ui.add_space(14.0);}}
            if matches(&disk){let enabled=self.registry.enabled(&disk.id);self.addon_card(ui,&disk,true,enabled);ui.add_space(20.0);}
            let third_party:Vec<_>=packages.into_iter().filter(|m|m.id!=toolboks_disk_manager::ID).collect();
            if !third_party.is_empty(){eyebrow(ui,"Your installed add-ons");ui.add_space(10.0);for m in third_party{let enabled=self.registry.enabled(&m.id);self.addon_card(ui,&m,false,enabled);ui.add_space(12.0);}}
            if !builtins.iter().any(&matches)&&!matches(&disk)&&!self.registry.packages.iter().any(|p|matches(&p.manifest)){muted(ui,"No add-ons match your search.");}
            ui.add_space(8.0);panel().show(ui,|ui|{ui.set_width(ui.available_width());ui.label(RichText::new("A platform for your next tool.").size(18.0));ui.add_space(6.0);muted(ui,"Install local packages or try the SDK starter. Verified first-party add-ons are listed first.");ui.add_space(10.0);if ui.button("Install SDK example").clicked(){match Package::parse(addons::SDK_EXAMPLE.as_bytes(),env!("CARGO_PKG_VERSION")){Ok(p)=>self.pending_package=Some(p),Err(e)=>self.message=Some(e)}}});
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
        self.base.tools[tool].ui(ui, page);
    }
    fn external_workspace(&mut self, ui: &mut egui::Ui, id: &str) {
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
                        self.pending_package = Some(addons::official_disk_package())
                    }
                    toolboks_addon_sdk::HostRequest::RestartElevated { page } => {
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
            eyebrow(ui, &package.manifest.category);
            ui.label(RichText::new(&package.manifest.title).size(28.0));
            muted(ui, &package.manifest.description);
            ui.add_space(look::SPACE_L);
            toolboks_addon_sdk::tutorial::mark(ui, "page", ui.available_rect_before_wrap());
            egui::ScrollArea::vertical()
                .id_salt(("addon", id, page))
                .show(ui, |ui| {
                    for (index, w) in package.pages[page].widgets.iter().enumerate() {
                        let widget = ui.push_id((id, page, index), |ui| {
                            match w {
                                Widget::Heading { text } => {
                                    ui.label(RichText::new(text).size(25.0).color(color));
                                }
                                Widget::Text { text } => {
                                    muted(ui, text);
                                }
                                Widget::Note { title, text } => {
                                    panel().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new(title).strong());
                                        ui.add_space(6.0);
                                        muted(ui, text);
                                    });
                                }
                                Widget::Metric { label, binding } => {
                                    panel().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        eyebrow(ui, label);
                                        ui.label(
                                            RichText::new(metric(*binding)).size(28.0).color(color),
                                        );
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
                                    if ui.checkbox(&mut value, label).changed() {
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
                                    ui.label(label);
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
                                                .desired_width(400.0),
                                        )
                                        .changed()
                                    {
                                        prefs.insert(key.clone(), serde_json::json!(value));
                                        self.preferences_dirty = Some(std::time::Instant::now());
                                        ui.request_repaint_after(Duration::from_millis(600));
                                    }
                                }
                            }
                            ui.add_space(15.0);
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
            "A workspace that fits",
            "Make yourself at home.",
            "Your preferences stay on this PC, separate from your add-on packages.",
        );
        egui::ScrollArea::vertical().id_salt("settings-scroll").show(ui,|ui|{
            panel().show(ui,|ui|{ui.set_width(ui.available_width());ui.label(RichText::new("App behavior").size(18.0));ui.add_space(15.0);if ui.checkbox(&mut self.keep_in_tray,"Keep Toolboks running in the tray when I close the window").changed(){self.registry.config.keep_in_tray=self.keep_in_tray;self.persist();}ui.add_space(6.0);muted(ui,"Background tools can keep working. Quit from the tray menu to close the app completely.");});ui.add_space(16.0);
            panel().show(ui,|ui|{ui.set_width(ui.available_width());ui.label(RichText::new("Local storage").size(18.0));ui.add_space(9.0);muted(ui,"Installed packages and preferences are saved here:");ui.add_space(8.0);ui.label(RichText::new(self.registry.root.display().to_string()).monospace().size(11.0));ui.add_space(12.0);muted(ui,if cfg!(feature="preview"){"This preview has its own configuration folder. Your stable installation is separate."}else{"No account or cloud sync is required."});});ui.add_space(16.0);
            panel().show(ui,|ui|{ui.set_width(ui.available_width());ui.label(RichText::new("About Toolboks").size(18.0));muted(ui,format!("Version {} · Add-on API 1",env!("CARGO_PKG_VERSION")));ui.add_space(10.0);self.update_ui(ui);});
            if !self.registry.errors.is_empty(){ui.add_space(16.0);panel().show(ui,|ui|{ui.label(RichText::new("Items needing attention").color(look::AMBER));for e in &self.registry.errors{ui.label(e);}});}
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
                .frame(panel()).show(ctx, |ui| {
                    ui.set_width(480.0);
                    eyebrow(ui, "Review your add-on");
                    ui.add_space(8.0);
                    ui.label(RichText::new(&p.manifest.title).size(26.0));
                    muted(ui, format!("{} · Version {}",p.manifest.publisher,p.manifest.version));
                    ui.add_space(16.0);
                    ui.label(&p.manifest.description);
                    ui.add_space(14.0);
                    if addons::trusted_native(&p){badge(ui,"✓ TOOLBOKS VERIFIED",ACCENT);}else{badge(ui,"LOCAL PACKAGE · NOT VERIFIED",look::AMBER);}
                    ui.add_space(14.0);
                    muted(ui,if p.manifest.native_provider.is_some(){"Folder scans read metadata. Reports are saved only when you choose. This update also adds administrator-only fragmentation analysis, HDD defrag and SSD TRIM; optimization modifies storage metadata only when you request it."}else{"This package uses Toolboks-rendered pages. It cannot run native code, open windows, or change system settings."});
                    ui.add_space(12.0);
                    ui.label("Requested access");
                    for c in &p.manifest.capabilities {
                        muted(ui,match c {
                            toolboks_addon_sdk::Capability::HostMetrics => "Read basic memory and uptime readings",
                            toolboks_addon_sdk::Capability::LocalPreferences => "Save its own preferences locally",
                            toolboks_addon_sdk::Capability::FilesystemRead => "Read file and folder metadata for locations you select",
                            toolboks_addon_sdk::Capability::UserExports => "Save reports to a destination you choose",
                            toolboks_addon_sdk::Capability::DriveOptimize => "Analyze fragmentation and run Windows defrag or TRIM only when you request it (administrator access required)",
                        });
                    }
                    ui.add_space(22.0);
                    ui.horizontal(|ui| {
                        let updating=self.registry.packages.iter().any(|a|a.manifest.id==p.manifest.id);
                        if primary(ui,if updating {"Install update"}else{"Install add-on"}).clicked() {
                            self.pending_package=None;
                            match self.registry.install(p.clone()) {
                                Ok(())=>self.message=Some(format!("{} is ready in your sidebar.",p.manifest.title)),
                                Err(e)=>self.message=Some(e),
                            }
                        }
                        if ui.button("Cancel").clicked(){self.pending_package=None;}
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
            } else if id == toolboks_disk_manager::ID {
                Some(addons::official_disk_package().manifest)
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
                        ui.set_width(500.0);
                        ui.label(RichText::new(&m.title).size(26.0));
                        if self.registry.verified(&id) {
                            badge(ui, "✓ TOOLBOKS VERIFIED", ACCENT);
                        }
                        ui.add_space(15.0);
                        egui::ScrollArea::vertical()
                            .max_height(380.0)
                            .show(ui, |ui| {
                                ui.label(&m.long_description);
                                ui.add_space(18.0);
                                egui::Grid::new("addon-metadata").spacing([25.0, 8.0]).show(
                                    ui,
                                    |ui| {
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
                                    },
                                );
                                ui.add_space(15.0);
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
                        ui.add_space(18.0);
                        let builtin = self.registry.builtin_index(&id).is_some();
                        let package = self.registry.packages.iter().any(|p| p.manifest.id == id);
                        if builtin || package {
                            let mut enabled = self.registry.enabled(&id);
                            if ui.checkbox(&mut enabled, "Show in sidebar").changed() {
                                self.set_addon_enabled(&id, enabled);
                            }
                            ui.add_space(14.0);
                            if package {
                                if ui
                                    .button(RichText::new("Remove add-on").color(look::RED))
                                    .clicked()
                                {
                                    self.detail = None;
                                    self.remove_pending = Some(id.clone());
                                }
                            } else {
                                muted(
                                    ui,
                                    "Included with Toolboks. Updated together with the app. \
                                     Turn it off to stop it running in the background.",
                                );
                            }
                        } else {
                            muted(ui, "Available in the add-on browser.");
                        }
                        ui.add_space(12.0);
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
                    ui.set_width(420.0);
                    ui.label(RichText::new(format!("Remove {name}?")).size(23.0));
                    ui.add_space(12.0);
                    muted(
                        ui,
                        "The installed package and its saved preferences will be removed.",
                    );
                    ui.add_space(20.0);
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
