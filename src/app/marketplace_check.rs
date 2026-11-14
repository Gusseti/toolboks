use super::*;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct Check {
    app: App,
    output: PathBuf,
    updates: PathBuf,
    stage: usize,
    frames: usize,
    saved: Arc<AtomicBool>,
    started: std::time::Instant,
    pending: Option<(String, usize)>,
}

impl Check {
    pub fn new(output: PathBuf, updates: PathBuf) -> Self {
        Self {
            app: App::new(),
            output,
            updates,
            stage: 0,
            frames: 0,
            saved: Arc::new(AtomicBool::new(true)),
            started: std::time::Instant::now(),
            pending: None,
        }
    }

    fn capture(&mut self, _ctx: &egui::Context, name: &str) {
        self.pending = Some((name.into(), 4));
        self.saved.store(false, Ordering::Relaxed);
        self.stage += 1;
        self.frames = 0;
    }
}

impl eframe::App for Check {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        let ctx = ui.ctx();
        if let Some((name, remaining)) = self.pending.take() {
            if remaining > 0 {
                self.pending = Some((name, remaining - 1));
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            let path = self.output.join(format!("{name}.png"));
            let saved = self.saved.clone();
            let store = if name == "07-browser" {
                ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("store-capture-rect")))
            } else {
                None
            };
            let scale = ctx.pixels_per_point();
            let crop_path = self.output.join("browser-crop.png");
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    let pixels: Vec<u8> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    image::save_buffer(
                        &path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .expect("save native screenshot");
                    if let Some(rect) = store {
                        let buffer = image::RgbaImage::from_raw(
                            image.size[0] as u32,
                            image.size[1] as u32,
                            pixels.clone(),
                        )
                        .unwrap();
                        image::imageops::crop_imm(
                            &buffer,
                            (rect.left() * scale).round() as u32,
                            (rect.top() * scale).round() as u32,
                            (rect.width() * scale).round() as u32,
                            (rect.height() * scale).round() as u32,
                        )
                        .to_image()
                        .save(&crop_path)
                        .unwrap();
                    }
                    saved.store(true, Ordering::Relaxed);
                }),
            ));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        if self.started.elapsed() > Duration::from_secs(120) {
            panic!("Native marketplace check timed out at stage {}", self.stage);
        }
        self.frames += 1;
        if self.frames < 6 || !self.saved.load(Ordering::Relaxed) || self.app.market.loading {
            return;
        }
        match self.stage {
            0 if !self.app.market.entries.is_empty() => {
                assert!(self.app.market.selected.is_empty());
                assert!(self.app.screen == Screen::Pick);
                self.app.sidebar_expanded = true;
                self.capture(ctx, "01-first-run");
            }
            1 => {
                self.app.market.select("example.resource-monitor", true);
                self.app.market.select("example.memory-cleaner", true);
                self.capture(ctx, "02-two-selected");
            }
            2 => {
                let selected = self.app.market.selected_entries();
                assert_eq!(selected.len(), 2);
                self.app.market.review = Some(selected);
                self.capture(ctx, "03-grouped-review");
            }
            3 => {
                let approved = self.app.market.review.take().unwrap();
                self.app.market.queue(approved);
                self.app.registry.config.first_run_complete = true;
                self.app.persist();
                self.stage += 1;
                self.frames = 0;
            }
            4 if !self.app.market.busy() => {
                assert_eq!(
                    crate::installer::installed(&self.app.registry.root).len(),
                    2
                );
                assert!(self
                    .app
                    .market
                    .jobs
                    .values()
                    .all(|job| job.status == crate::marketplace::InstallStatus::Ready));
                self.capture(ctx, "04-installed");
            }
            5 => {
                self.app.screen = Screen::External("example.resource-monitor".into());
                self.capture(ctx, "05-open-resource-monitor");
            }
            6 => {
                self.app.screen = Screen::External("example.memory-cleaner".into());
                self.capture(ctx, "06-open-memory-cleaner");
            }
            7 => {
                self.app.screen = Screen::Browser;
                self.app.market.focused = Some("example.disk-manager".into());
                self.capture(ctx, "07-browser");
            }
            8 => {
                std::env::set_var("TOOLBOKS_TEST_CATALOG", &self.updates);
                self.app.market.refresh(self.app.registry.root.clone(), ctx);
                self.stage += 1;
                self.frames = 0;
            }
            9 => {
                let entry = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == "example.memory-cleaner")
                    .unwrap();
                assert!(crate::marketplace::update_available(Some("1.0.0"), entry));
                self.app.market.focused = Some(entry.id.clone());
                self.app.market.details = true;
                self.capture(ctx, "08-update-details");
            }
            10 => {
                self.app.market.remove = Some(("example.memory-cleaner".into(), false));
                self.capture(ctx, "09-remove-keep-data");
            }
            11 => {
                self.app.market.remove = Some(("example.memory-cleaner".into(), true));
                self.capture(ctx, "10-remove-delete-data");
            }
            12 => {
                self.app.market.remove = None;
                self.app.market.details = false;
                let failed = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == "example.macro-manager")
                    .unwrap()
                    .clone();
                std::fs::write(
                    self.updates.join("macro-manager.tbaddon"),
                    b"corrupt test download",
                )
                .unwrap();
                self.app.market.queue(vec![failed]);
                self.stage += 1;
                self.frames = 0;
            }
            13 if !self.app.market.busy() => {
                assert!(matches!(
                    self.app.market.jobs["example.macro-manager"].status,
                    crate::marketplace::InstallStatus::Failed(_)
                ));
                self.app.market.focused = Some("example.macro-manager".into());
                self.app.market.details = true;
                self.capture(ctx, "11-failed-sidebar");
            }
            14 => {
                self.app.market.details = false;
                self.app
                    .start_tutorial("toolboks.addon-browser".into(), false);
                self.capture(ctx, "12-browser-tour");
            }
            15 => {
                self.app.tour = None;
                let entry = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == "example.hello")
                    .unwrap()
                    .clone();
                self.app.market.review = Some(vec![entry]);
                self.capture(ctx, "13-native-review");
            }
            16 => {
                let approved = self.app.market.review.take().unwrap();
                self.app.market.queue(approved);
                self.stage += 1;
                self.frames = 0;
            }
            17 if !self.app.market.busy() => {
                assert!(
                    crate::installer::native_exe(&self.app.registry.root, "example.hello")
                        .is_some()
                );
                self.app.screen = Screen::External("example.hello".into());
                self.stage += 1;
                self.frames = 0;
            }
            18 if self.app.views.get("example.hello").is_some_and(|view| {
                matches!(view.state(), crate::addon_view::ViewState::Running)
            }) =>
            {
                self.capture(ctx, "14-native-open");
            }
            19 => {
                self.app.screen = Screen::Browser;
                let entry = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == "example.memory-cleaner")
                    .unwrap()
                    .clone();
                self.app.market.review = Some(vec![entry]);
                self.capture(ctx, "15-update-review");
            }
            20 => {
                let approved = self.app.market.review.take().unwrap();
                self.app.market.queue(approved);
                self.stage += 1;
                self.frames = 0;
            }
            21 if !self.app.market.busy() => {
                assert_eq!(
                    crate::installer::installed_version(
                        &self.app.registry.root,
                        "example.memory-cleaner"
                    )
                    .as_deref(),
                    Some("1.1.0")
                );
                self.app.screen = Screen::External("example.memory-cleaner".into());
                self.capture(ctx, "16-updated-open");
            }
            22 => {
                std::fs::write(self.output.join("result.txt"), "PASS: fresh first-run; no preselection; two tools reviewed together, installed from signed local catalog, and opened in the same real native window. Update, remove choices, real failed download and tour captured. State tests cover removal effects.\n").unwrap();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}
