//! Isolated PREVIEW integration check for the real catalog-installed Disk Manager.
//! Uses the normal browser/installer/supervisor/view and egui's test input hook.
use super::*;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use toolboks_addon_proto::HostMessage;

const ID: &str = "toolboks.disk-manager";
const PAGES: [&str; 5] = [
    "Overview",
    "Largest files",
    "File types",
    "Scan details",
    "Optimization",
];

pub struct Check {
    app: App,
    output: PathBuf,
    stage: usize,
    frames: usize,
    started: std::time::Instant,
    capture: Option<String>,
    saved: Arc<AtomicBool>,
    click: Option<(egui::Pos2, bool)>,
    analyze: Option<egui::Pos2>,
    opened: Option<std::time::Instant>,
    first_frame_ms: Option<f64>,
    page_started: std::time::Instant,
}

impl Check {
    pub fn new(output: PathBuf) -> Self {
        let fixture = output.join("scan-fixture");
        for (name, size) in [
            ("a.bin", 1_048_576),
            ("one/b.bin", 2_097_152),
            ("two/c.bin", 3_145_728),
            ("three/d.txt", 8192),
            ("three/e.txt", 512),
        ] {
            let path = fixture.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create_new(path)
                .unwrap()
                .set_len(size)
                .unwrap();
        }
        let mut app = App::new();
        app.screen = Screen::Browser;
        app.sidebar_expanded = true;
        app.registry.config.preferences.insert(ID.into(), BTreeMap::from([("settings".into(), serde_json::json!({
            "path":fixture.canonicalize().unwrap(), "exclude":"", "min_bytes":0, "older_days":0, "sort":"size"
        }))]));
        app.persist();
        Self {
            app,
            output,
            stage: 0,
            frames: 0,
            started: std::time::Instant::now(),
            capture: None,
            saved: Arc::new(AtomicBool::new(true)),
            click: None,
            analyze: None,
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
    fn navigate(&mut self, page: usize) {
        self.page_started = std::time::Instant::now();
        self.app.addon_page = page;
        self.app.supervisor.as_ref().unwrap().send(
            ID,
            &HostMessage::Navigate {
                page: page.to_string(),
            },
        );
        self.app.views.get_mut(ID).unwrap().request_frame();
        self.frames = 0;
    }
}

impl eframe::App for Check {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        if let Some((pos, pressed)) = self.click.take() {
            input.focused = true;
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            if pressed {
                self.click = Some((pos, false));
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        if let Some(opened) = self.opened {
            if self.first_frame_ms.is_none()
                && self.app.views.get(ID).is_some_and(|view| {
                    matches!(view.state(), crate::addon_view::ViewState::Running)
                })
            {
                self.first_frame_ms = Some(opened.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let ctx = ui.ctx();
        ctx.request_repaint_after(Duration::from_millis(100));
        assert!(
            self.started.elapsed() < Duration::from_secs(120),
            "Disk Manager check timed out at stage {}",
            self.stage
        );
        self.frames += 1;
        if self.frames < 12 || self.app.market.loading {
            return;
        }
        if let Some(name) = self.capture.take() {
            let path = self.output.join(format!("{name}.png"));
            let saved = self.saved.clone();
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
            0 if self.app.market.entries.iter().any(|entry| entry.id == ID) => {
                let entry = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == ID)
                    .unwrap();
                assert_eq!(entry.pages, PAGES);
                assert!(entry.background);
                self.app.market.focused = Some(ID.into());
                self.capture("01-browser");
            }
            1 => {
                let entry = self
                    .app
                    .market
                    .entries
                    .iter()
                    .find(|entry| entry.id == ID)
                    .unwrap()
                    .clone();
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
                    crate::installer::native_exe(&self.app.registry.root, ID).is_some(),
                    "catalog install failed"
                );
                self.capture("03-installed");
            }
            4 => {
                self.opened = Some(std::time::Instant::now());
                self.app.open_market_addon(ID);
                self.stage += 1;
                self.frames = 0;
            }
            5 if self.app.views.get(ID).is_some_and(|view| {
                matches!(view.state(), crate::addon_view::ViewState::Running)
            }) =>
            {
                self.capture("04-open-overview");
            }
            6 => {
                self.app
                    .services
                    .start_tour(self.app.supervisor.as_ref().unwrap(), ID);
                self.stage += 1;
                self.frames = 0;
            }
            7 if self.app.services.tour(ID).is_some() => {
                // Query the real control layout, without the tour's sample-data banner.
                // The SDK anchor request is still useful after restoring the real page.
                self.app
                    .supervisor
                    .as_ref()
                    .unwrap()
                    .send(ID, &HostMessage::TutorialFinish);
                let step = self
                    .app
                    .services
                    .tour(ID)
                    .unwrap()
                    .steps
                    .iter()
                    .position(|step| step.target == "disk-analyze")
                    .unwrap();
                self.app
                    .services
                    .show_step(self.app.supervisor.as_ref().unwrap(), ID, step);
                self.app.views.get_mut(ID).unwrap().request_frame();
                self.stage += 1;
                self.frames = 0;
            }
            8 => {
                let origin = ctx.data(|data| {
                    data.get_temp::<egui::Pos2>(egui::Id::unique("disk-pilot-origin"))
                });
                if let Some(anchor) = origin.and_then(|origin| self.app.services.anchor(ID, origin))
                {
                    self.analyze = Some(anchor.center());
                    self.app
                        .services
                        .finish_tour(self.app.supervisor.as_ref().unwrap(), ID);
                    self.app.views.get_mut(ID).unwrap().request_frame();
                    self.stage += 1;
                    self.frames = 0;
                }
            }
            9 => {
                self.click = Some((self.analyze.unwrap(), true));
                self.stage += 1;
                self.frames = 0;
            }
            10 if self.frames > 40 => {
                self.capture("05-scanned-overview");
            }
            11 => {
                self.navigate(1);
                self.stage += 1;
            }
            12 => {
                self.capture("06-largest-files");
            }
            13 => {
                self.navigate(2);
                self.stage += 1;
            }
            14 => {
                self.capture("07-file-types");
            }
            15 => {
                self.navigate(3);
                self.stage += 1;
            }
            16 => {
                self.capture("08-scan-details");
            }
            17 => {
                self.navigate(4);
                self.stage += 1;
            }
            18 if self.page_started.elapsed() > Duration::from_secs(8) => {
                self.capture("09-optimization-gated");
            }
            19 => {
                std::fs::write(self.output.join("native-timing.json"), serde_json::to_vec_pretty(&serde_json::json!({
                    "open_to_first_drawn_frame_ms": self.first_frame_ms.expect("first native frame"),
                    "method": "App open_market_addon to the first Running frame drawn by the native view, including normal supervisor/named-pipe startup if not already running; background process may have started after install. GPU presentation and human click latency excluded.",
                    "expected_fixture": {"files":5,"folders_including_root":4,"logical_bytes":6_300_160}
                })).unwrap()).unwrap();
                for name in PAGES {
                    let link = crate::addon_services::resolve_link(
                        &format!("Disk Manager/{name}"),
                        &crate::installer::installed(&self.app.registry.root),
                        |_| true,
                        &[],
                    );
                    assert_eq!(
                        link,
                        crate::addon_services::Link::Open {
                            id: ID.into(),
                            page: Some(name.into())
                        }
                    );
                }
                std::fs::write(self.output.join("result.txt"), "PASS: real executable installed through browser review/queue and normal installer; authenticated supervisor and view opened all five pages; all five --open page names resolve. Generated scan fixture expects 5 files, 4 folders including root, 6,300,160 bytes: scan completion/totals/ranking require visual inspection of the captures. Drive changes remain gated on #132; no elevation or maintenance was requested.\n").unwrap();
                self.app.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}
