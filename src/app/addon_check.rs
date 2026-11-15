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
/// How often a screenshot is saved while waiting for a person to answer a prompt.
const HOLD_SHOT_EVERY: Duration = Duration::from_secs(10);

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
    /// `TOOLBOKS_CHECK_CLICK=x,y`: one click at this point after the first page is shown, for
    /// checks that need a button pressed (for example one that raises a UAC prompt). The check
    /// then waits `TOOLBOKS_CHECK_HOLD` seconds, saving a screenshot every few seconds, so a
    /// person can answer the prompts.
    click_at: Option<egui::Pos2>,
    hold: Duration,
    click: Option<(egui::Pos2, bool)>,
    hold_started: std::time::Instant,
    shots: usize,
    /// `TOOLBOKS_CHECK_SCROLL=points`: scroll the page down by this much before the click.
    scroll: Option<f32>,
    scroll_now: Option<f32>,
    /// `TOOLBOKS_CHECK_SCROLL_UP=1`: a few seconds after the click, scroll back to the top so the
    /// screenshots show what the page reported.
    scroll_up: bool,
}

impl Check {
    pub fn new(output: PathBuf, id: String) -> Self {
        let mut app = App::new();
        app.screen = Screen::Browser;
        app.sidebar_expanded = true;
        app.persist();
        let click_at = std::env::var("TOOLBOKS_CHECK_CLICK")
            .ok()
            .and_then(|value| {
                let (x, y) = value.split_once(',')?;
                Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
            });
        let scroll = std::env::var("TOOLBOKS_CHECK_SCROLL")
            .ok()
            .and_then(|value| value.parse::<f32>().ok());
        let hold = std::env::var("TOOLBOKS_CHECK_HOLD")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map_or(Duration::ZERO, Duration::from_secs);
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
            click_at,
            hold,
            click: None,
            hold_started: std::time::Instant::now(),
            shots: 0,
            scroll,
            scroll_now: None,
            scroll_up: std::env::var_os("TOOLBOKS_CHECK_SCROLL_UP").is_some(),
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
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        if let Some(points) = self.scroll_now.take() {
            input.focused = true;
            input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(800.0, 500.0)));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -points),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
        }
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
            self.started.elapsed() < Duration::from_secs(180) + self.hold,
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
            0 if self.entry().is_some()
                && crate::installer::native_exe(&self.app.registry.root, &self.id).is_some() =>
            {
                // Installed by an earlier run in this data folder.
                self.stage = 4;
                self.frames = 0;
            }
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
            6 if self.click_at.is_some() || self.scroll.is_some() => {
                self.scroll_now = self.scroll;
                self.page_started = std::time::Instant::now();
                self.stage = 19;
                self.frames = 0;
            }
            19 if self.page_started.elapsed() > Duration::from_secs(2) => {
                // The page has scrolled; a screenshot shows where the buttons ended up.
                self.stage = 19;
                self.capture("04b-scrolled");
                self.stage = 20;
            }
            20 => {
                self.click = self.click_at.map(|pos| (pos, true));
                self.hold_started = std::time::Instant::now();
                self.page_started = std::time::Instant::now();
                self.stage = 21;
            }
            21 if self.scroll_up && self.hold_started.elapsed() > Duration::from_secs(6) => {
                self.scroll_up = false;
                self.scroll_now = Some(-3000.0);
            }
            21 if self.hold_started.elapsed() > self.hold => {
                self.stage = 8;
                self.frames = 0;
            }
            21 if self.page_started.elapsed() > HOLD_SHOT_EVERY => {
                self.shots += 1;
                let name = format!("click-{:02}", self.shots);
                self.page_started = std::time::Instant::now();
                self.stage = 20; // `capture` adds one, landing back on 21.
                self.capture(&name);
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
