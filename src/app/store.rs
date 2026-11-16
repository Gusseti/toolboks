use super::*;
use crate::ui::{BORDER, MUTED, PANEL_BG, TEXT, WINDOW_BG};
use crate::{catalog::Entry, marketplace as market};
use egui::{Color32, FontFamily, FontId, Margin, Rect, RichText, Sense, Stroke, Vec2};
use toolboks_addon_sdk::tutorial as anchors;

const DETAIL_WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 74.0;
const ROW_GAP: f32 = 2.0;
const DIVIDER: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x22);
const ACCENT_TEXT: Color32 = Color32::from_rgb(0xb3, 0x9d, 0xfa);
const PRIMARY: Color32 = Color32::from_rgb(0x7a, 0x4b, 0xe8);

pub(crate) fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "store-400",
            include_bytes!("../../assets/fonts/SchibstedGrotesk-400.ttf").as_slice(),
        ),
        (
            "store-500",
            include_bytes!("../../assets/fonts/SchibstedGrotesk-500.ttf").as_slice(),
        ),
        (
            "store-600",
            include_bytes!("../../assets/fonts/SchibstedGrotesk-600.ttf").as_slice(),
        ),
        (
            "store-700",
            include_bytes!("../../assets/fonts/SchibstedGrotesk-700.ttf").as_slice(),
        ),
    ] {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        fonts
            .families
            .insert(FontFamily::Name(name.into()), vec![name.into()]);
    }
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts.font_data.insert(
            "segoe-ui".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "segoe-ui".into());
    }
    // 600-weight text in the system face (Home's Health card): Segoe UI Semibold, falling back
    // to the regular face when the file is missing, so the family always exists.
    let mut semibold = fonts.families[&FontFamily::Proportional].clone();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/seguisb.ttf") {
        fonts.font_data.insert(
            "segoe-ui-600".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        semibold.insert(0, "segoe-ui-600".into());
    }
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}

/// The font family for 600-weight text in the system face; see [`install_fonts`].
pub(crate) const SEMIBOLD: &str = "host-600";

pub(super) fn font(size: f32, weight: u16) -> FontId {
    FontId::new(size, FontFamily::Name(format!("store-{weight}").into()))
}

fn text(value: impl Into<String>, size: f32, weight: u16, color: Color32) -> RichText {
    RichText::new(value)
        .font(font(size, weight))
        .line_height(Some(size * 1.6))
        .color(color)
}

fn icon(painter: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    let point =
        |position: [f32; 2]| rect.min + Vec2::new(position[0], position[1]) * (rect.width() / 24.0);
    let stroke = Stroke::new(1.7 * rect.width() / 24.0, color);
    let line = |positions: &[[f32; 2]]| {
        painter.line(
            positions.iter().map(|position| point(*position)).collect(),
            stroke,
        )
    };
    match name {
        "search" => {
            painter.circle_stroke(point([11.0, 11.0]), 6.5 * rect.width() / 24.0, stroke);
            line(&[[16.0, 16.0], [20.0, 20.0]]);
        }
        "verified" => {
            line(&[
                [12.0, 3.0],
                [14.3, 4.7],
                [17.1, 4.5],
                [18.0, 7.2],
                [20.3, 8.8],
                [19.4, 11.5],
                [20.3, 14.2],
                [18.0, 15.8],
                [17.1, 18.5],
                [14.3, 18.3],
                [12.0, 21.0],
                [9.7, 19.3],
                [6.9, 19.5],
                [6.0, 16.8],
                [3.7, 15.2],
                [4.6, 12.5],
                [3.7, 9.8],
                [6.0, 8.2],
                [6.9, 5.5],
                [9.7, 5.7],
                [12.0, 3.0],
            ]);
            line(&[[8.8, 12.2], [11.0, 14.4], [15.2, 10.0]]);
        }
        "check" => {
            line(&[[5.0, 12.5], [9.5, 17.0], [19.0, 7.5]]);
        }
        "activity" | "pulse" => {
            line(&[
                [3.0, 12.0],
                [7.0, 12.0],
                [10.0, 5.0],
                [14.0, 19.0],
                [17.0, 12.0],
                [21.0, 12.0],
            ]);
        }
        "cpu" | "chip" => {
            painter.rect_stroke(
                Rect::from_min_max(point([7.0, 7.0]), point([17.0, 17.0])),
                2,
                stroke,
                egui::StrokeKind::Middle,
            );
            for value in [10.0, 14.0] {
                line(&[[value, 3.0], [value, 7.0]]);
                line(&[[value, 17.0], [value, 21.0]]);
                line(&[[3.0, value], [7.0, value]]);
                line(&[[17.0, value], [21.0, value]]);
            }
        }
        "sliders" => {
            for (height, center) in [(7.0, 15.0), (17.0, 9.0)] {
                line(&[[4.0, height], [center - 2.0, height]]);
                line(&[[center + 2.0, height], [20.0, height]]);
                painter.circle_stroke(point([center, height]), 2.0 * rect.width() / 24.0, stroke);
            }
        }
        "shield" => {
            let mut points = vec![point([12.0, 3.0]), point([19.0, 6.0]), point([19.0, 11.0])];
            for controls in [
                [[19.0, 11.0], [19.0, 15.6], [16.1, 18.9], [12.0, 21.0]],
                [[12.0, 21.0], [7.9, 18.9], [5.0, 15.6], [5.0, 11.0]],
            ] {
                for index in 1..=16 {
                    let fraction = index as f32 / 16.0;
                    let inverse = 1.0 - fraction;
                    points.push(
                        point(controls[0]) * inverse.powi(3)
                            + point(controls[1]).to_vec2() * (3.0 * inverse.powi(2) * fraction)
                            + point(controls[2]).to_vec2() * (3.0 * inverse * fraction.powi(2))
                            + point(controls[3]).to_vec2() * fraction.powi(3),
                    );
                }
            }
            points.extend([point([5.0, 6.0]), point([12.0, 3.0])]);
            painter.line(points, stroke);
            line(&[[12.0, 8.0], [12.0, 12.0]]);
            painter.circle_filled(point([12.0, 15.5]), stroke.width / 2.0, color);
        }
        "drive" | "hard-drive" => {
            line(&[[5.0, 13.0], [7.2, 6.0], [16.8, 6.0], [19.0, 13.0]]);
            painter.rect_stroke(
                Rect::from_min_max(point([3.5, 13.0]), point([20.5, 19.0])),
                2,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.circle_filled(point([16.5, 16.0]), stroke.width / 2.0, color);
        }
        "cursor" | "mouse-pointer" => {
            line(&[
                [6.0, 4.0],
                [18.0, 11.0],
                [12.5, 12.5],
                [10.0, 18.0],
                [6.0, 4.0],
            ]);
            line(&[[13.0, 13.0], [17.0, 18.0]]);
        }
        "mic" | "microphone" => {
            painter.rect_stroke(
                Rect::from_min_max(point([9.0, 3.0]), point([15.0, 14.0])),
                3,
                stroke,
                egui::StrokeKind::Middle,
            );
            let points = (0..=20)
                .map(|index| {
                    let angle = std::f32::consts::PI * index as f32 / 20.0;
                    point([12.0 - 6.5 * angle.cos(), 11.0 + 6.5 * angle.sin()])
                })
                .collect();
            painter.line(points, stroke);
            line(&[[12.0, 17.5], [12.0, 21.0]]);
        }
        "gauge" => {
            let points = (0..=32)
                .map(|index| {
                    let angle = (159.635_f32 + 220.73 * index as f32 / 32.0).to_radians();
                    point([12.0 + 8.0 * angle.cos(), 14.216 + 8.0 * angle.sin()])
                })
                .collect();
            painter.line(points, stroke);
            line(&[[12.0, 13.0], [15.5, 9.0]]);
            painter.circle_stroke(point([12.0, 13.5]), 1.3 * rect.width() / 24.0, stroke);
        }
        _ => {
            for (left, top) in [(4.0, 4.0), (13.5, 4.0), (4.0, 13.5)] {
                painter.rect_stroke(
                    Rect::from_min_max(point([left, top]), point([left + 6.5, top + 6.5])),
                    1,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
            line(&[[16.75, 14.0], [16.75, 19.5]]);
            line(&[[14.0, 16.75], [19.5, 16.75]]);
        }
    }
}

/// One add-on row. With `check` it also shows a checkbox at the right, for the first-run picker.
pub(super) fn row(
    ui: &mut egui::Ui,
    entry: &Entry,
    selected: bool,
    state: &str,
    installed: bool,
    check: Option<bool>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected || response.hovered() {
            painter.rect_filled(
                rect,
                8,
                if selected {
                    look::SELECTED_BG
                } else {
                    PANEL_BG
                },
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                8,
                Stroke::new(1.0, look::HOST_ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let tile = Rect::from_min_size(rect.min + Vec2::new(12.0, 17.0), Vec2::splat(40.0));
        paint_tile(painter, tile, &entry.icon, selected);
        let right = rect.right() - 12.0 - if check.is_some() { 30.0 } else { 0.0 };
        if let Some(on) = check {
            let boxed = Rect::from_center_size(
                egui::pos2(rect.right() - 12.0 - 9.0, rect.center().y),
                Vec2::splat(18.0),
            );
            painter.rect_filled(boxed, 5, if on { PRIMARY } else { PANEL_BG });
            painter.rect_stroke(
                boxed,
                5,
                Stroke::new(
                    1.0,
                    if on {
                        PRIMARY
                    } else {
                        Color32::from_rgb(0x4a, 0x4d, 0x57)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if on {
                icon(painter, boxed.shrink(2.0), "check", Color32::WHITE);
            }
        }
        let state_color = if installed { look::GREEN } else { MUTED };
        let state_text = painter.layout_no_wrap(state.into(), font(13.0, 400), state_color);
        let state_width = state_text.size().x + if installed { 21.0 } else { 0.0 };
        let state_left = right - state_width;
        let left = rect.left() + 66.0;
        let text_rect = Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2((state_left - 14.0).max(left), rect.bottom()),
        );
        let clipped = painter.with_clip_rect(text_rect);
        let title = clipped.layout_no_wrap(entry.title.clone(), font(15.0, 600), TEXT);
        clipped.galley(egui::pos2(left, rect.top() + 17.0), title.clone(), TEXT);
        let badge_left = left + title.size().x + 6.0;
        if entry.verified {
            let badge = Rect::from_min_size(
                egui::pos2(badge_left, rect.top() + 18.0),
                Vec2::new(69.0, 17.2),
            );
            clipped.rect_filled(
                badge,
                99,
                Color32::from_rgba_unmultiplied(0x8b, 0x5c, 0xf6, 0x33),
            );
            icon(
                &clipped,
                Rect::from_min_size(badge.min + Vec2::new(4.0, 2.0), Vec2::splat(13.0)),
                "verified",
                ACCENT_TEXT,
            );
            clipped.text(
                badge.min + Vec2::new(22.0, 8.6),
                egui::Align2::LEFT_CENTER,
                "Verified",
                font(11.0, 500),
                ACCENT_TEXT,
            );
        } else {
            clipped.text(
                egui::pos2(badge_left, rect.top() + 27.0),
                egui::Align2::LEFT_CENTER,
                &entry.publisher,
                font(13.0, 400),
                MUTED,
            );
        }
        let mut job = egui::text::LayoutJob::simple(
            entry.short_description.clone(),
            font(13.0, 400),
            MUTED,
            text_rect.width(),
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let description = ui.fonts_mut(|fonts| fonts.layout_job(job));
        clipped.galley(egui::pos2(left, rect.top() + 43.2), description, MUTED);
        if installed {
            icon(
                painter,
                Rect::from_center_size(
                    egui::pos2(state_left + 7.5, rect.center().y),
                    Vec2::splat(15.0),
                ),
                "check",
                state_color,
            );
        }
        painter.galley(
            egui::pos2(
                right - state_text.size().x,
                rect.center().y - state_text.size().y / 2.0 - 1.0,
            ),
            state_text,
            state_color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            if check.is_some() {
                egui::Role::CheckBox
            } else {
                egui::Role::Button
            },
            true,
            check.unwrap_or(selected),
            format!("{}, {}", entry.title, state),
        )
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl App {
    pub(super) fn store_ui(&mut self, ui: &mut egui::Ui, entries: &[Entry]) {
        ui.scope(|ui| {
            ui.style_mut().text_styles.insert(egui::TextStyle::Body,font(14.0,400));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button,font(14.0,400));
            ui.spacing_mut().item_spacing=Vec2::ZERO;
            let store_frame = egui::Frame::new().fill(WINDOW_BG).stroke(Stroke::new(1.0,BORDER)).corner_radius(12).show(ui,|ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new().fill(look::SIDEBAR_BG).inner_margin(Margin{left:16,right:16,top:15,bottom:14}).show(ui,|ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing=Vec2::new(17.0,12.0);
                    // One centred row when it fits; the categories wrap below the search when it does not.
                    let bar=Vec2::new(ui.available_width(),46.0);
                    let wrap=bar.x<840.0;
                    ui.allocate_ui_with_layout(bar,egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(wrap),|ui| {
                        egui::Frame::new().fill(PANEL_BG).stroke(Stroke::new(1.0,BORDER)).corner_radius(8).inner_margin(Margin::symmetric(12,0)).show(ui,|ui| {
                            ui.spacing_mut().item_spacing.x=8.0;
                            ui.allocate_ui_with_layout(Vec2::new(294.0,44.0),egui::Layout::left_to_right(egui::Align::Center),|ui| {
                                ui.set_min_size(Vec2::new(294.0,44.0));
                                let (rect,_)=ui.allocate_exact_size(Vec2::splat(17.0),Sense::hover());
                                icon(ui.painter(),rect,"search",MUTED);
                                let response=ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(269.0).hint_text(text("Search add-ons",15.0,400,MUTED)).font(font(15.0,400)).frame(egui::Frame::NONE).margin(Vec2::ZERO));
                                anchors::mark(ui,"browser-search",response.rect);
                            });
                        });
                        let mut categories=vec!["All".to_owned()];
                        for category in ["System","Storage","Audio","Automation","Developer tools"] {
                            categories.push(category.into());
                        }
                        for entry in &self.market.entries {
                            if !categories.contains(&entry.category) {categories.push(entry.category.clone());}
                        }
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(),36.0),egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(wrap),|ui| {
                            // 4 px in the source; the extra covers egui snapping glyph advances to whole pixels.
                            ui.spacing_mut().item_spacing=Vec2::new(6.0,4.0);
                            ui.spacing_mut().button_padding=Vec2::new(12.0,7.0);
                            for category in categories {
                                let selected=self.market.category==category;
                                let response=ui.add(egui::Button::new(text(&category,14.0,400,if selected {TEXT} else {MUTED})).fill(if selected {look::SELECTED_BG} else {Color32::TRANSPARENT}).stroke(Stroke::NONE).corner_radius(7).min_size(Vec2::new(0.0,37.0)));
                                if category=="All" {anchors::mark(ui,"browser-category",response.rect);}
                                if response.clicked(){self.market.category=category;}
                            }
                        });
                    });
                });
                let line=ui.allocate_exact_size(Vec2::new(ui.available_width(),1.0),Sense::hover()).0;
                ui.painter().rect_filled(line,0,DIVIDER);
                let wide=ui.available_width()>=796.0;
                // Fill the page down to the bottom edge; the list and the details scroll inside.
                let height=(ui.available_height()-2.0).max(180.0);
                // The 1 px divider sits between the list and the 340 px detail column.
                let list_width=if wide {ui.available_width()-DETAIL_WIDTH-1.0} else {ui.available_width()};
                ui.with_layout(if wide {egui::Layout::left_to_right(egui::Align::Min)} else {egui::Layout::top_down(egui::Align::Min)},|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(list_width,if wide {height} else {height*0.4}),egui::Layout::top_down(egui::Align::Min),|ui| {
                        ui.set_width(list_width);
                        egui::Frame::new().inner_margin(Margin{left:8,right:8,top:8,bottom:0}).show(ui,|ui| {
                            ui.spacing_mut().item_spacing.y=ROW_GAP;
                            egui::ScrollArea::vertical().id_salt("market-list").max_height(if wide {height-8.0} else {height*0.4-8.0}).auto_shrink([false,false]).show(ui,|ui| {
                                anchors::region(ui,"browser-list",|ui| {
                                    for entry in entries {
                                        let installed=self.installed_version(&entry.id).is_some();
                                        let status=self.market_status(&entry.id).unwrap_or_else(||if installed {"Installed".into()}else{entry.category.clone()});
                                        if row(ui,entry,self.market.focused.as_ref()==Some(&entry.id),&status,installed&&status=="Installed",None).clicked(){self.market.focused=Some(entry.id.clone());}
                                    }
                                    if entries.is_empty(){ui.label(text("No add-ons match. Clear the search or choose another category.",13.0,400,MUTED));}
                                    ui.add_space(6.0);
                                    egui::Frame::new().inner_margin(12).show(ui,|ui|{ui.label(text(format!("{} add-ons. Verified tools are listed first. {}",entries.len(),self.market.last_checked.map(|at|crate::marketplace::checked_text(at.elapsed())).unwrap_or_default()),13.0,400,MUTED));});
                                });
                            });
                        });
                    });
                    if wide {
                        let rect=ui.allocate_exact_size(Vec2::new(1.0,height),Sense::hover()).0;
                        ui.painter().rect_filled(rect,0,DIVIDER);
                    }
                    ui.allocate_ui_with_layout(Vec2::new(if wide {DETAIL_WIDTH}else{ui.available_width()},if wide {height}else{height*0.6}),egui::Layout::top_down(egui::Align::Min),|ui| {
                        egui::Frame::new().inner_margin(20).show(ui,|ui| {
                            ui.set_width(if wide {DETAIL_WIDTH-40.0}else{ui.available_width()});
                            egui::ScrollArea::vertical().id_salt("market-preview").max_height(if wide {height-40.0}else{height*0.6-40.0}).auto_shrink([false,false]).show(ui,|ui| {
                                if let Some(entry)=self.focused_entry(){self.store_detail(ui,&entry,if wide {height-40.0}else{0.0});}
                            });
                        });
                    });
                });
            });
            #[cfg(feature = "preview")]
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::unique("store-capture-rect"), store_frame.response.rect));
            #[cfg(not(feature = "preview"))]
            let _ = store_frame;
        });
    }

    fn store_detail(&mut self, ui: &mut egui::Ui, entry: &Entry, height: f32) {
        let top = ui.cursor().top();
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
        ui.add(
            egui::Label::new(text(&entry.title, 22.0, 700, TEXT).extra_letter_spacing(-0.44))
                .wrap(),
        );
        ui.add_space(4.0);
        ui.label(text(meta_line(entry), 13.0, 400, MUTED));
        ui.add_space(16.0);
        ui.label(text(&entry.short_description, 14.0, 400, MUTED));
        ui.add_space(16.0);
        trust_card(ui, entry.verified);
        ui.add_space(14.0);
        let line = ui
            .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
            .0;
        ui.painter().rect_filled(line, 0, DIVIDER);
        ui.add_space(14.0);
        ui.label(text("It can", 13.0, 600, TEXT));
        ui.add_space(10.0);
        access_lines(ui, entry);
        let installed = self.installed_version(&entry.id);
        let update = market::update_available(installed.as_deref(), entry);
        let busy = self
            .market
            .jobs
            .get(&entry.id)
            .is_some_and(|job| job.status.busy());
        let accepted = self
            .market
            .entries
            .iter()
            .any(|available| available == entry);
        let remaining = height - (ui.cursor().top() - top) - 46.0;
        ui.add_space(remaining.max(16.0));
        anchors::region(ui, "browser-review", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                let label = if update {
                    "Update"
                } else if installed.is_some() {
                    "Open"
                } else {
                    "Install"
                };
                let enabled = if installed.is_some() && !update {
                    true
                } else {
                    accepted && market::can_install(entry) && !busy && !self.market.loading
                };
                ui.spacing_mut().button_padding = Vec2::new(21.0, 15.0);
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(
                            text(label, 16.0, 600, Color32::WHITE).line_height(Some(16.0)),
                        )
                        .fill(PRIMARY)
                        .stroke(Stroke::new(1.0, PRIMARY))
                        .corner_radius(8),
                    )
                    .clicked()
                {
                    if installed.is_some() && !update {
                        self.open_market_addon(&entry.id);
                    } else {
                        self.market.review = Some(vec![entry.clone()]);
                    }
                }
            });
        });
    }
}

/// How a button looks: the one main action, a plain one, or one that removes something.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Look {
    Primary,
    Secondary,
    Danger,
}

/// A button in the landing page's style.
pub(super) fn button(ui: &mut egui::Ui, label: &str, kind: Look) -> egui::Response {
    ui.scope(|ui| {
        let (size, weight, padding, radius) = match kind {
            Look::Primary => (16.0, 600, Vec2::new(21.0, 15.0), 8),
            _ => (14.0, 400, Vec2::new(14.0, 9.0), 6),
        };
        ui.spacing_mut().button_padding = padding;
        let color = match kind {
            Look::Primary => Color32::WHITE,
            Look::Secondary => TEXT,
            Look::Danger => look::RED,
        };
        let button = egui::Button::new(text(label, size, weight, color).line_height(Some(size)))
            .corner_radius(radius)
            .min_size(Vec2::ZERO);
        let button = if kind == Look::Primary {
            button.fill(PRIMARY).stroke(Stroke::new(1.0, PRIMARY))
        } else {
            button
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, Color32::from_rgb(0x33, 0x36, 0x3e)))
        };
        ui.add(button)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

/// A page title, 22 px bold, with a sentence under it.
pub(super) fn page_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add(egui::Label::new(text(title, 22.0, 700, TEXT).extra_letter_spacing(-0.44)).wrap());
    if !subtitle.is_empty() {
        ui.add_space(4.0);
        ui.add(egui::Label::new(text(subtitle, 14.0, 400, MUTED)).wrap());
    }
    ui.add_space(16.0);
}

/// A line of text in the strong colour and weight, such as an add-on's name in a list.
pub(super) fn strong(ui: &mut egui::Ui, value: &str, size: f32) {
    ui.label(text(value, size, 600, TEXT));
}

/// A section heading: 13 px semibold.
pub(super) fn heading(ui: &mut egui::Ui, label: &str) {
    ui.label(text(label, 13.0, 600, TEXT));
}

/// Body text in the muted colour, wrapped. 14 px for reading, 13 px for notes.
pub(super) fn paragraph(ui: &mut egui::Ui, value: &str, size: f32) {
    ui.add(egui::Label::new(text(value, size, 400, MUTED)).wrap());
}

/// A one pixel divider across the available width.
pub(super) fn rule(ui: &mut egui::Ui) {
    let line = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
        .0;
    ui.painter().rect_filled(line, 0, DIVIDER);
}

/// The rounded frame the browser sits in.
pub(super) fn card<R>(ui: &mut egui::Ui, margin: i8, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(WINDOW_BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(12)
        .inner_margin(margin)
        .show(ui, add)
        .inner
}

fn paint_tile(painter: &egui::Painter, tile: Rect, name: &str, selected: bool) {
    painter.rect_filled(tile, 10, PANEL_BG);
    painter.rect_stroke(
        tile,
        10,
        Stroke::new(
            1.0,
            if selected {
                Color32::from_rgb(0x33, 0x36, 0x3e)
            } else {
                BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    icon(
        painter,
        Rect::from_center_size(tile.center(), Vec2::splat(20.0)),
        name,
        TEXT,
    );
}

/// The 40 px icon tile of an add-on.
pub(super) fn tile(ui: &mut egui::Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    paint_tile(ui.painter(), rect, name, false);
}

/// The purple "Verified" pill.
pub(super) fn verified_pill(ui: &mut egui::Ui) {
    let (badge, _) = ui.allocate_exact_size(Vec2::new(69.0, 17.2), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        badge,
        99,
        Color32::from_rgba_unmultiplied(0x8b, 0x5c, 0xf6, 0x33),
    );
    icon(
        painter,
        Rect::from_min_size(badge.min + Vec2::new(4.0, 2.0), Vec2::splat(13.0)),
        "verified",
        ACCENT_TEXT,
    );
    painter.text(
        badge.min + Vec2::new(22.0, 8.6),
        egui::Align2::LEFT_CENTER,
        "Verified",
        font(11.0, 500),
        ACCENT_TEXT,
    );
}

/// The id of an add-on's status control in the sidebar. It comes from the add-on id, so it is the
/// same every frame and keyboard focus can stay on it.
pub(super) fn status_id(addon_id: &str) -> egui::Id {
    egui::Id::unique(("addon-status", addon_id))
}

/// What a screen reader says for an add-on's status, for example "Disk Manager, update available".
pub(super) fn status_name(title: &str, label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => format!("{title}, {}{}", first.to_lowercase(), chars.as_str()),
        None => title.to_owned(),
    }
}

/// The collapsed sidebar's status dot: a small coloured dot that takes keyboard focus, shows a
/// focus ring and names the status in a tooltip.
pub(super) fn status_dot(
    ui: &mut egui::Ui,
    addon_id: &str,
    title: &str,
    label: &str,
    center: egui::Pos2,
) -> egui::Response {
    let rect = Rect::from_center_size(center, Vec2::splat(12.0));
    let response = ui.interact(rect, status_id(addon_id), Sense::click());
    let name = status_name(title, label);
    ui.painter().circle_filled(center, 3.0, look::DATA_MEMORY);
    super::shell::focus_ring(ui, rect, &response);
    super::shell::focus_tip(&response, &name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    response.on_hover_text(label)
}

/// A small state pill for the sidebar: an update, a download or a failure. `addon_id` keeps its
/// id stable between frames and `title` makes its accessible name ("Disk Manager, update available").
pub(super) fn status_pill(
    ui: &mut egui::Ui,
    addon_id: &str,
    title: &str,
    label: &str,
) -> egui::Response {
    let color = if label.starts_with("Could not") {
        look::RED
    } else if label.starts_with("Update") {
        ACCENT_TEXT
    } else {
        MUTED
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font(11.0, 500), color);
    let width = (galley.size().x + 16.0).min(ui.available_width().max(40.0));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 19.0), Sense::hover());
    let response = ui.interact(rect, status_id(addon_id), Sense::click());
    ui.painter().rect_filled(
        rect,
        99,
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0x26),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            99,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    let clipped = ui
        .painter()
        .with_clip_rect(rect.shrink2(Vec2::new(4.0, 0.0)));
    clipped.galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    let name = status_name(title, label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The "Toolboks verified" card.
pub(super) fn trust_card(ui: &mut egui::Ui, verified: bool) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
        egui::Frame::new()
            .fill(PANEL_BG)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(17.0), Sense::hover());
                    icon(
                        ui.painter(),
                        rect,
                        if verified { "verified" } else { "grid" },
                        if verified { ACCENT_TEXT } else { MUTED },
                    );
                    ui.add(
                        egui::Label::new(text(
                            if verified {
                                "Toolboks verified. Built and checked by the Toolboks team."
                            } else {
                                "Community add-on. Not checked by the Toolboks team."
                            },
                            13.0,
                            400,
                            MUTED,
                        ))
                        .wrap(),
                    );
                });
            });
    });
}

/// What an add-on can access, one line each with a check, then the data it keeps on this PC
/// when it says so. The text comes from [`market::access_text`].
pub(super) fn access_lines(ui: &mut egui::Ui, entry: &Entry) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
        for line in market::access_text(entry) {
            match &line {
                market::AccessLine::Can(_) | market::AccessLine::Data(_) => {
                    let glyph = if matches!(line, market::AccessLine::Data(_)) {
                        "drive"
                    } else {
                        "check"
                    };
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        icon(ui.painter(), rect, glyph, ACCENT_TEXT);
                        ui.add(egui::Label::new(text(line.text(), 14.0, 400, MUTED)).wrap());
                    });
                    ui.add_space(10.0);
                }
                market::AccessLine::Nothing => {
                    ui.label(text(line.text(), 14.0, 400, MUTED));
                }
                market::AccessLine::Native => {
                    ui.label(text(line.text(), 13.0, 400, MUTED));
                }
            }
        }
    });
}

/// The grey line under a title: byline, publisher and category.
pub(super) fn meta_line(entry: &Entry) -> String {
    match &entry.byline {
        Some(byline) => format!(
            "{byline} \u{b7} {} \u{b7} {}",
            entry.publisher, entry.category
        ),
        None => format!("{} \u{b7} {}", entry.publisher, entry.category),
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn run(ctx: &egui::Context, tab: bool, add: impl FnMut(&mut egui::Ui)) {
        let events = if tab {
            vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            vec![]
        };
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(300.0, 200.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, add);
        output.textures_delta.clear();
    }

    #[test]
    fn the_status_name_says_which_add_on_and_what_state() {
        assert_eq!(
            status_name("Disk Manager", "Update available"),
            "Disk Manager, update available"
        );
        assert_eq!(
            status_name("Debloater", "Could not install"),
            "Debloater, could not install"
        );
        assert_eq!(
            status_name("Audio Gainer", "Downloading 1.2 MB of 3.4 MB"),
            "Audio Gainer, downloading 1.2 MB of 3.4 MB"
        );
        assert_eq!(status_name("Debloater", ""), "Debloater");
    }

    #[test]
    fn the_status_id_comes_from_the_add_on_id_and_never_changes() {
        assert_eq!(
            status_id("toolboks.disk-manager"),
            status_id("toolboks.disk-manager")
        );
        assert_ne!(
            status_id("toolboks.disk-manager"),
            status_id("toolboks.debloater")
        );
    }

    #[test]
    fn tab_reaches_the_pill_and_keeps_focus_between_frames() {
        let ctx = egui::Context::default();
        let draw = |ui: &mut egui::Ui| {
            status_pill(
                ui,
                "toolboks.disk-manager",
                "Disk Manager",
                "Could not install",
            );
        };
        run(&ctx, false, draw);
        run(&ctx, true, draw);
        for _ in 0..3 {
            run(&ctx, false, draw);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(status_id("toolboks.disk-manager"))
            );
        }
    }

    #[test]
    fn tab_reaches_the_collapsed_dot_and_keeps_focus_between_frames() {
        let ctx = egui::Context::default();
        let draw = |ui: &mut egui::Ui| {
            status_dot(
                ui,
                "toolboks.disk-manager",
                "Disk Manager",
                "Could not install",
                egui::pos2(40.0, 40.0),
            );
        };
        run(&ctx, false, draw);
        run(&ctx, true, draw);
        for _ in 0..3 {
            run(&ctx, false, draw);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(status_id("toolboks.disk-manager"))
            );
        }
    }
}
