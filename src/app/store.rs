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
    ctx.set_fonts(fonts);
}

fn font(size: f32, weight: u16) -> FontId {
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

fn row(
    ui: &mut egui::Ui,
    entry: &Entry,
    selected: bool,
    state: &str,
    installed: bool,
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
            &entry.icon,
            TEXT,
        );
        let state_color = if installed { look::GREEN } else { MUTED };
        let state_text = painter.layout_no_wrap(state.into(), font(13.0, 400), state_color);
        let state_width = state_text.size().x + if installed { 21.0 } else { 0.0 };
        let state_left = rect.right() - 12.0 - state_width;
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
                rect.right() - 12.0 - state_text.size().x,
                rect.center().y - state_text.size().y / 2.0 - 1.0,
            ),
            state_text,
            state_color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
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
                                        if row(ui,entry,self.market.focused.as_ref()==Some(&entry.id),&status,installed&&status=="Installed").clicked(){self.market.focused=Some(entry.id.clone());}
                                    }
                                    if entries.is_empty(){ui.label(text("No add-ons match. Clear the search or choose another category.",13.0,400,MUTED));}
                                    ui.add_space(6.0);
                                    egui::Frame::new().inner_margin(12).show(ui,|ui|{ui.label(text(format!("{} add-ons. Verified tools are listed first.",entries.len()),13.0,400,MUTED));});
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
        ui.label(text(
            match &entry.byline {
                Some(byline) => {
                    format!(
                        "{byline} \u{b7} {} \u{b7} {}",
                        entry.publisher, entry.category
                    )
                }
                None => format!("{} \u{b7} {}", entry.publisher, entry.category),
            },
            13.0,
            400,
            MUTED,
        ));
        ui.add_space(16.0);
        ui.label(text(&entry.short_description, 14.0, 400, MUTED));
        ui.add_space(16.0);
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
                        if entry.verified { "verified" } else { "grid" },
                        if entry.verified { ACCENT_TEXT } else { MUTED },
                    );
                    ui.add(
                        egui::Label::new(text(
                            if entry.verified {
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
        ui.add_space(14.0);
        let line = ui
            .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
            .0;
        ui.painter().rect_filled(line, 0, DIVIDER);
        ui.add_space(14.0);
        ui.label(text("It can", 13.0, 600, TEXT));
        ui.add_space(10.0);
        if entry.capabilities.is_empty() {
            ui.label(text("No extra host access requested.", 14.0, 400, MUTED));
        }
        for capability in &entry.capabilities {
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icon(ui.painter(), rect, "check", ACCENT_TEXT);
                ui.add(egui::Label::new(text(market::access(capability), 14.0, 400, MUTED)).wrap());
            });
            ui.add_space(10.0);
        }
        if entry.kind == crate::catalog::Kind::Native {
            ui.label(text(
                "Runs a program on your PC, not in a security sandbox.",
                13.0,
                400,
                MUTED,
            ));
        }
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
