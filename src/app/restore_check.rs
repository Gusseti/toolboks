//! Isolated PREVIEW check of the update from the version that had the tools built in. It starts the
//! real app on a copy of an old data folder, expects the "Restore your tools" offer with the old
//! tools ticked, installs them from the test catalog through the normal review and installer, and
//! checks that the saved settings are still under the same ids and the offer is gone.
use super::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub struct Check {
    app: App,
    output: PathBuf,
    expected: Vec<String>,
    stage: usize,
    frames: usize,
    started: std::time::Instant,
    capture: Option<String>,
    saved: Arc<AtomicBool>,
}

impl Check {
    /// `expected` are the ids the old data folder should make Toolboks offer.
    pub fn new(output: PathBuf, expected: Vec<String>) -> Self {
        Self {
            app: App::new(),
            output,
            expected,
            stage: 0,
            frames: 0,
            started: std::time::Instant::now(),
            capture: None,
            saved: Arc::new(AtomicBool::new(true)),
        }
    }
    fn capture(&mut self, name: &str) {
        self.capture = Some(name.into());
        self.saved.store(false, Ordering::Relaxed);
        self.stage += 1;
        self.frames = 0;
    }
}

impl eframe::App for Check {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        let ctx = ui.ctx();
        ctx.request_repaint_after(Duration::from_millis(100));
        assert!(
            self.started.elapsed() < Duration::from_secs(240),
            "restore check timed out at stage {}",
            self.stage
        );
        self.frames += 1;
        if self.frames < 12 || self.app.market.loading {
            return;
        }
        if let Some(name) = self.capture.take() {
            let path = self.output.join(format!("{name}.png"));
            let saved = self.saved.clone();
            let _ = std::fs::create_dir_all(&self.output);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    let pixels: Vec<_> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    image::save_buffer(
                        path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    saved.store(true, Ordering::Relaxed);
                }),
            ));
            self.frames = 0;
            return;
        }
        if !self.saved.load(Ordering::Relaxed) {
            return;
        }
        match self.stage {
            // The offer: the picker opens by itself with the old tools ticked.
            0 if !self.app.market.entries.is_empty() => {
                assert!(
                    matches!(self.app.screen, Screen::Pick),
                    "the offer opens by itself"
                );
                let mut pending = self.app.registry.config.restore_pending.clone();
                pending.sort();
                let mut expected = self.expected.clone();
                expected.sort();
                assert_eq!(pending, expected, "the tools offered");
                let mut selected: Vec<String> = self.app.market.selected.iter().cloned().collect();
                selected.sort();
                assert_eq!(selected, expected, "the offered tools are ticked");
                self.capture("01-restore-offer");
            }
            1 => {
                let selected = self.app.market.selected_entries();
                assert_eq!(
                    selected.len(),
                    self.expected.len(),
                    "every offered tool is in the catalog"
                );
                self.app.market.review = Some(selected);
                self.capture("02-access-review");
            }
            2 => {
                let approved = self.app.market.review.take().unwrap();
                self.app.market.queue(approved);
                self.stage += 1;
                self.frames = 0;
            }
            3 if !self.app.market.busy() => {
                for id in &self.expected {
                    assert!(
                        crate::installer::native_exe(&self.app.registry.root, id).is_some(),
                        "{id} was not installed"
                    );
                }
                self.capture("03-installed");
            }
            4 => {
                assert!(
                    !self.app.restore_pending(),
                    "the offer is gone once everything is installed"
                );
                assert_eq!(
                    self.app.registry.config.preferences["toolboks.resource-monitor"]
                        ["graph_range"],
                    "5m",
                    "saved settings stay under the same id"
                );
                self.app.screen = Screen::Home;
                self.capture("04-home");
            }
            5 => {
                std::fs::write(
                    self.output.join("result.txt"),
                    format!(
                        "PASS: updating from an old data folder offered {} tools, installed them from the catalog through the normal review and installer, kept the saved settings under the same ids, and the offer is gone.\n",
                        self.expected.len()
                    ),
                )
                .unwrap();
                self.app.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}
