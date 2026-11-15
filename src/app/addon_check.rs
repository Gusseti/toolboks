//! Isolated PREVIEW integration check for any catalog-installed native add-on.
//! Installs the add-on from the test catalog through the normal review and queue, opens it
//! through the normal supervisor and view, visits every page its catalog entry lists and
//! saves one screenshot per step. Nothing is clicked inside the add-on and no administrator
//! action is requested.
use super::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use toolboks_addon_proto::HostMessage;

/// How long each page is shown before it is captured, so slow first readings can finish.
const PAGE_WAIT: Duration = Duration::from_secs(5);

pub struct Check {
    app: App,
    id: String,
    output: PathBuf,
    stage: usize,
    page: usize,
    frames: usize,
    started: std::time::Instant,
    capture: Option<String>,
    saved: Arc<AtomicBool>,
    opened: Option<std::time::Instant>,
    first_frame_ms: Option<f64>,
    page_started: std::time::Instant,
}

impl Check {
    pub fn new(output: PathBuf, id: String) -> Self {
        let mut app = App::new();
        app.screen = Screen::Browser;
        app.sidebar_expanded = true;
        app.persist();
        Self {
            app,
            id,
            output,
            stage: 0,
            page: 0,
            frames: 0,
            started: std::time::Instant::now(),
            capture: None,
            saved: Arc::new(AtomicBool::new(true)),
            opened: None,
            first_frame_ms: None,
            page_started: std::time::Instant::now(),
        }
    }
    fn capture(&mut self, name: &str) {
        self.capture = Some(name.into());
        self.saved.store(false, Ordering::Relaxed);
        self.stage += 1;
        self.frames = 0;
    }
    fn entry(&self) -> Option<&crate::catalog::Entry> {
        self.app.market.entries.iter().find(|e| e.id == self.id)
    }
    fn navigate(&mut self, page: usize) {
        self.page_started = std::time::Instant::now();
        self.app.addon_page = page;
        self.app.supervisor.as_ref().unwrap().send(
            &self.id,
            &HostMessage::Navigate {
                page: page.to_string(),
            },
        );
        self.app.views.get_mut(&self.id).unwrap().request_frame();
        self.frames = 0;
    }
}

impl eframe::App for Check {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        if let Some(opened) = self.opened {
            if self.first_frame_ms.is_none()
                && self.app.views.get(&self.id).is_some_and(|view| {
                    matches!(view.state(), crate::addon_view::ViewState::Running)
                })
            {
                self.first_frame_ms = Some(opened.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let ctx = ui.ctx();
        ctx.request_repaint_after(Duration::from_millis(100));
        assert!(
            self.started.elapsed() < Duration::from_secs(180),
            "{} check timed out at stage {}",
            self.id,
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
        let pages = self.entry().map(|e| e.pages.clone()).unwrap_or_default();
        match self.stage {
            0 if self.entry().is_some() => {
                let id = self.id.clone();
                self.app.market.focused = Some(id);
                self.capture("01-browser");
            }
            1 => {
                let entry = self.entry().unwrap().clone();
                self.app.market.review = Some(vec![entry]);
                self.capture("02-access-review");
            }
            2 => {
                let approved = self.app.market.review.take().unwrap();
                self.app.market.queue(approved);
                self.stage += 1;
                self.frames = 0;
            }
            3 if !self.app.market.busy() => {
                assert!(
                    crate::installer::native_exe(&self.app.registry.root, &self.id).is_some(),
                    "catalog install failed"
                );
                self.capture("03-installed");
            }
            4 => {
                self.opened = Some(std::time::Instant::now());
                let id = self.id.clone();
                self.app.open_market_addon(&id);
                self.page_started = std::time::Instant::now();
                self.stage += 1;
                self.frames = 0;
            }
            5 if self.app.views.get(&self.id).is_some_and(|view| {
                matches!(view.state(), crate::addon_view::ViewState::Running)
            }) && self.page_started.elapsed() > PAGE_WAIT =>
            {
                self.capture("04-open-page-1");
            }
            6 => {
                // Next page, or finish after the last one.
                self.page += 1;
                if self.page < pages.len() {
                    self.navigate(self.page);
                    self.stage += 1;
                } else {
                    self.stage = 8;
                }
                self.frames = 0;
            }
            7 if self.page_started.elapsed() > PAGE_WAIT => {
                let name = format!("{:02}-page-{}", 4 + self.page, self.page + 1);
                self.stage = 6 - 1; // `capture` adds one, landing back on stage 6.
                self.capture(&name);
            }
            8 => {
                let entry = self.entry().unwrap().clone();
                std::fs::write(
                    self.output.join("timing.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "id": self.id,
                        "pages": entry.pages,
                        "open_to_first_drawn_frame_ms": self.first_frame_ms,
                        "method": "App open_market_addon to the first Running frame drawn by the native view, including normal supervisor and named-pipe start-up. GPU presentation and human click latency excluded.",
                    }))
                    .unwrap(),
                )
                .unwrap();
                for name in &entry.pages {
                    let link = crate::addon_services::resolve_link(
                        &format!("{}/{name}", entry.title),
                        &crate::installer::installed(&self.app.registry.root),
                        |_| true,
                        &[],
                    );
                    assert_eq!(
                        link,
                        crate::addon_services::Link::Open {
                            id: self.id.clone(),
                            page: Some(name.clone())
                        },
                        "--open link for page {name}"
                    );
                }
                std::fs::write(
                    self.output.join("result.txt"),
                    format!(
                        "PASS: {} installed from the test catalog through browser review and the normal installer; the authenticated supervisor and view opened and drew every page ({}); every --open page name resolves. Inspect the captures for content.\n",
                        self.id,
                        entry.pages.join(", ")
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
