use super::*;
use egui::{Color32, RichText, Vec2};
use toolboks_addon_sdk::{tutorial as anchors, TutorialStep};
mod catalog;

pub(super) struct Run {
    owner: String,
    steps: Vec<TutorialStep>,
    index: usize,
    applied: bool,
    reveal_frames: u8,
    original_base: (usize, usize),
    original_page: usize,
    /// Position of the built-in tool this tour belongs to, if it is not a package.
    pub(super) tool: Option<usize>,
    locations: Vec<String>,
    provider: Option<String>,
}
/// The page a step belongs to: the first part of its route (`page` or `page/section`).
fn chapter(route: &str) -> String {
    route.split('/').next().unwrap_or("").into()
}
impl App {
    fn tour_provider(&self, owner: &str) -> Option<String> {
        self.registry
            .packages
            .iter()
            .find(|p| p.manifest.id == owner)
            .and_then(|p| p.manifest.native_provider.clone())
    }
    fn tour_steps(&mut self, owner: &str) -> (Vec<TutorialStep>, String) {
        if let Some(tool) = self.registry.builtin_index(owner) {
            let page = if self.base.selected.0 == tool {
                self.base.selected.1
            } else {
                0
            };
            return (catalog::for_tool(tool), page.to_string());
        }
        if let Some(provider) = self.tour_provider(owner) {
            return (
                self.native.tutorial_steps(&provider),
                self.native.tutorial_location(&provider),
            );
        }
        let Some(p) = self
            .registry
            .packages
            .iter()
            .find(|p| p.manifest.id == owner)
        else {
            return (Vec::new(), String::new());
        };
        let page = p
            .pages
            .get(self.addon_page)
            .map(|p| p.id.clone())
            .unwrap_or_default();
        if !p.tutorial.is_empty() {
            return (p.tutorial.clone(), page);
        }
        let mut steps = Vec::new();
        for p in &p.pages {
            steps.push(TutorialStep::new(&p.id,"page",&p.title,"This is one of the add-on's pages. The tabs switch between its sections without leaving Toolboks."));
            for (i, w) in p.widgets.iter().enumerate() {
                let (title,body) = match w {
                    toolboks_addon_sdk::Widget::Heading{text} => (text.clone(),"A heading introduces the controls and information below it.".into()),
                    toolboks_addon_sdk::Widget::Text{text} => ("About this section".into(),text.clone()),
                    toolboks_addon_sdk::Widget::Note{title,text} => (title.clone(),text.clone()),
                    toolboks_addon_sdk::Widget::Metric{label,..} => (label.clone(),"This live reading is supplied by Toolboks. It updates while this page is visible.".into()),
                    toolboks_addon_sdk::Widget::Toggle{label,..} => (label.clone(),"This switch changes an add-on preference. Its value is saved locally for this add-on.".into()),
                    toolboks_addon_sdk::Widget::TextInput{label,..} => (label.clone(),"Enter a value here. It is saved locally in this add-on's own preferences.".into()),
                };
                steps.push(TutorialStep::new(
                    &p.id,
                    &format!("widget-{i}"),
                    &title,
                    &body,
                ));
            }
        }
        (steps, page)
    }
    fn start_tutorial(&mut self, owner: String, current_only: bool) {
        let (mut steps, location) = self.tour_steps(&owner);
        if current_only {
            steps.retain(|s| chapter(&s.page) == chapter(&location));
        }
        if steps.is_empty() {
            self.message = Some("This add-on has no walkthrough for this page yet.".into());
            return;
        }
        let provider = self.tour_provider(&owner);
        let tool = self.registry.builtin_index(&owner);
        let locations = if let Some(p) = &provider {
            vec![self.native.tutorial_location(p)]
        } else if let Some(t) = tool {
            vec![self.base.tools[t].tutorial_location()]
        } else {
            Vec::new()
        };
        if let Some(p) = &provider {
            self.native.tutorial_start(p);
        }
        self.tour = Some(Run {
            owner,
            steps,
            index: 0,
            applied: false,
            reveal_frames: 3,
            original_base: self.base.selected,
            original_page: self.addon_page,
            tool,
            locations,
            provider,
        });
    }
    pub(super) fn prepare_tutorial(&mut self, ctx: &egui::Context) {
        let Some(run) = &mut self.tour else {
            anchors::prepare(ctx, None, false);
            return;
        };
        let step = &run.steps[run.index];
        if !run.applied {
            if let Some(t) = run.tool {
                let mut parts = step.page.split('/');
                if let Some(p) = parts.next().and_then(|s| s.parse::<usize>().ok()) {
                    self.base.selected = (t, p);
                    self.base.tools[t].tutorial_navigate(parts.next().unwrap_or(""));
                }
            } else if let Some(provider) = &run.provider {
                self.native.tutorial_navigate(provider, &step.page);
            } else if let Some(package) = self
                .registry
                .packages
                .iter()
                .find(|p| p.manifest.id == run.owner)
            {
                if let Some(i) = package.pages.iter().position(|p| p.id == step.page) {
                    self.addon_page = i;
                }
            }
            run.applied = true;
        }
        anchors::prepare(ctx, Some(&step.target), run.reveal_frames > 0);
        run.reveal_frames = run.reveal_frames.saturating_sub(1);
    }
    fn finish_tutorial(&mut self, ctx: &egui::Context) {
        if let Some(run) = self.tour.take() {
            self.base.selected = run.original_base;
            self.addon_page = run.original_page;
            if let Some(provider) = run.provider {
                if let Some(location) = run.locations.first() {
                    self.native.tutorial_navigate(&provider, location);
                }
                self.native.tutorial_finish(&provider);
            } else if let (Some(t), Some(location)) = (run.tool, run.locations.first()) {
                self.base.tools[t].tutorial_navigate(location);
            }
        }
        anchors::prepare(ctx, None, false);
        ctx.request_repaint();
    }
    pub(super) fn tutorial_ui(&mut self, ctx: &egui::Context) {
        if let Some(owner) = self.help.clone() {
            let (steps, location) = self.tour_steps(&owner);
            let current = steps
                .iter()
                .filter(|s| chapter(&s.page) == chapter(&location))
                .count();
            let mut choice = None;
            let response=egui::Modal::new(egui::Id::unique("help-chooser")).show(ctx,|ui| {
                ui.set_width(430.0_f32.min(ctx.content_rect().width()-60.0));
                ui.label(RichText::new("A little guidance.").size(26.0));
                ui.label("Choose a walkthrough. Next and Back move between explanations; Cancel or Esc exits at any time.");
                ui.add_space(12.0);
                if ui.add_enabled(!steps.is_empty(),egui::Button::new(format!("Full add-on walkthrough · {} steps",steps.len()))).clicked(){choice=Some(false);}
                if ui.add_enabled(current>0,egui::Button::new(format!("Just this tab · {current} steps"))).clicked(){choice=Some(true);}
                ui.add_space(12.0);
                ui.label(RichText::new("Tours only navigate and explain. They do not run scans, change Windows settings, end processes or export files.").small().weak());
                if ui.button("Cancel").clicked(){self.help=None;}
            });
            if response.should_close() {
                self.help = None;
            }
            if let Some(current) = choice {
                self.help = None;
                self.start_tutorial(owner, current);
                ctx.request_repaint();
            }
        }
        let Some(run) = &mut self.tour else {
            return;
        };
        let screen = ctx.content_rect();
        let target = anchors::anchor(ctx).map(|r| r.expand(4.0).intersect(screen));
        let mut index = run.index;
        let mut finish = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if index + 1 < run.steps.len() {
                index += 1;
            } else {
                finish = true;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            index = index.saturating_sub(1);
        }
        egui::Area::new(egui::Id::unique("tour-backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, egui::Sense::click_and_drag());
                let p = ui.painter();
                let dim = Color32::from_black_alpha(165);
                if let Some(r) = target {
                    for rect in [
                        egui::Rect::from_min_max(screen.min, egui::pos2(screen.right(), r.top())),
                        egui::Rect::from_min_max(egui::pos2(screen.left(), r.bottom()), screen.max),
                        egui::Rect::from_min_max(
                            egui::pos2(screen.left(), r.top()),
                            egui::pos2(r.left(), r.bottom()),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(r.right(), r.top()),
                            egui::pos2(screen.right(), r.bottom()),
                        ),
                    ] {
                        p.rect_filled(rect, 0, dim);
                    }
                    for (width, alpha) in [(15.0, 12), (9.0, 22), (5.0, 42), (2.0, 230)] {
                        p.rect_stroke(
                            r,
                            7,
                            egui::Stroke::new(
                                width,
                                Color32::from_rgba_unmultiplied(170, 124, 225, alpha),
                            ),
                            egui::StrokeKind::Outside,
                        );
                    }
                } else {
                    p.rect_filled(screen, 0, dim);
                }
            });
        let card_width = 420.0_f32.min(screen.width() - 32.0);
        let estimate = Vec2::new(card_width + 32.0, 260.0_f32.min(screen.height() - 32.0));
        let positions = [
            screen.min + Vec2::splat(16.0),
            egui::pos2(screen.right() - estimate.x - 16.0, screen.top() + 16.0),
            egui::pos2(screen.left() + 16.0, screen.bottom() - estimate.y - 16.0),
            screen.max - estimate - Vec2::splat(16.0),
        ];
        let position = positions
            .into_iter()
            .min_by(|a, b| {
                let overlap = |p| {
                    target.map_or(0.0, |r| {
                        let overlap = r.intersect(egui::Rect::from_min_size(p, estimate));
                        if overlap.is_positive() {
                            overlap.area()
                        } else {
                            0.0
                        }
                    })
                };
                overlap(*a).total_cmp(&overlap(*b))
            })
            .unwrap();
        egui::Area::new(egui::Id::unique("tour-card")).order(egui::Order::Foreground).fixed_pos(position).show(ctx,|ui| {
            egui::Frame::new().fill(Color32::from_rgb(25,23,32)).stroke(egui::Stroke::new(1.0,look::HOST_ACCENT)).corner_radius(12).inner_margin(16).show(ui,|ui| {
                ui.set_width(card_width);
                ui.horizontal(|ui| {ui.label(RichText::new(format!("GUIDED TOUR · {} / {}",run.index+1,run.steps.len())).small().color(look::HOST_ACCENT));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.button("Cancel").clicked(){finish=true;}});});
                ui.label(RichText::new(&run.steps[run.index].title).size(21.0).strong());
                egui::ScrollArea::vertical().id_salt("tour-description").max_height((screen.height()-230.0).clamp(70.0,170.0)).show(ui,|ui|{ui.label(&run.steps[run.index].description);});
                if target.is_none(){ui.label(RichText::new("This section is not visible yet or has no data. You can continue or go back.").small().weak());}
                ui.add(egui::ProgressBar::new((run.index+1)as f32/run.steps.len()as f32).fill(look::HOST_ACCENT).desired_height(3.0));
                ui.horizontal(|ui| {
                    if ui.add_enabled(run.index>0,egui::Button::new("Back")).clicked(){index=run.index.saturating_sub(1);}
                    if ui.button(if run.index+1==run.steps.len(){"Finish"}else{"Next"}).clicked(){if run.index+1==run.steps.len(){finish=true;}else{index=run.index+1;}}
                    egui::ComboBox::from_id_salt("tour-jump").selected_text("Jump to…").width(180.0).show_ui(ui,|ui|{for (i,step) in run.steps.iter().enumerate(){ui.selectable_value(&mut index,i,format!("{}. {}",i+1,step.title));}});
                });
            });
        });

        if index != run.index {
            run.index = index;
            run.applied = false;
            run.reveal_frames = 3;
            ctx.request_repaint();
        }
        if run.reveal_frames > 0 {
            ctx.request_repaint();
        }
        if finish {
            self.finish_tutorial(ctx);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_tab_includes_its_inner_sections_only() {
        assert_eq!(chapter("1/system"), "1");
        assert_eq!(chapter("2"), "2");
        assert_eq!(chapter("overview/options"), "overview");
    }
    #[test]
    fn every_builtin_tool_has_its_own_tour_covering_every_page() {
        for (t, tool) in tools::all().iter().enumerate() {
            let steps = catalog::for_tool(t);
            assert!(!steps.is_empty(), "{} has a tour", tool.name());
            for p in 0..tool.pages().len() {
                assert!(
                    steps.iter().any(|s| chapter(&s.page) == p.to_string()),
                    "{} page {p}",
                    tool.name()
                );
            }
        }
        assert!(catalog::for_tool(tools::COUNT).is_empty());
    }
    #[test]
    fn tool_steps_resolve_real_layout_anchors() {
        let ctx = egui::Context::default();
        let mut tools = tools::all();
        for (t, tool) in tools.iter_mut().enumerate() {
            for step in catalog::for_tool(t) {
                let parts: Vec<_> = step.page.split('/').collect();
                let p: usize = parts[0].parse().unwrap();
                tool.tutorial_navigate(parts.get(1).copied().unwrap_or(""));
                for _ in 0..5 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(1500.0, 1000.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            anchors::prepare(ui.ctx(), Some(&step.target), true);
                            ui.disable();
                            tool.ui(ui, p);
                        },
                    );
                    output.textures_delta.clear();
                }
                assert!(
                    anchors::anchor(&ctx).is_some(),
                    "{}: missing {}",
                    step.title,
                    step.target
                );
            }
        }
    }
}
