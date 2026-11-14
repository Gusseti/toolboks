//! Debug-only preview of the add-on host view (#81): a window that paints a sample add-on
//! through `AddonView`. The add-on runs on a thread over in-memory pipes with the SDK
//! runtime, so it speaks the real wire protocol without a child process (that is #80).
//!
//! ```text
//! cargo run --example addon_view_demo
//! ```
//!
//! Try: click and type in the sample, Tab through it and out to the host button, drag from
//! the slider and release outside the page, copy text, open the link, change the display
//! scale, and press "Restart add-on" to see the dimmed restart state. This window is not
//! part of Toolboks; users never see it.

#[allow(dead_code)]
#[path = "../src/addon_view/mod.rs"]
mod addon_view;

use addon_view::{AddonLink, AddonView};
use eframe::egui;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use toolboks_addon_sdk::runtime::proto::memory::duplex;
use toolboks_addon_sdk::runtime::proto::{host_handshake, AddonMessage, Hello};
use toolboks_addon_sdk::runtime::run_addon_on;
use toolboks_addon_sdk::NativeAddon;

const RESTART_DELAY: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct Sample {
    count: u64,
    text: String,
    level: f32,
}

impl NativeAddon for Sample {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        ui.heading("Sample add-on");
        ui.label("Painted by the add-on runtime, shown by the host view.");
        if ui.button("Add one").clicked() {
            self.count += 1;
        }
        ui.label(format!("Count: {}", self.count));
        ui.text_edit_singleline(&mut self.text);
        ui.add(egui::Slider::new(&mut self.level, 0.0..=100.0).text("Level"));
        ui.hyperlink("https://github.com/Gusseti/toolboks");
        false
    }
}

fn connect(ctx: &egui::Context) -> (AddonLink, mpsc::Receiver<AddonMessage>) {
    let (host, addon) = duplex();
    std::thread::spawn(move || run_addon_on(addon, Sample::default(), "sample"));
    let (mut reader, mut writer) = host.split();
    host_handshake(&mut reader, &mut writer, &Hello::new("sample", "demo")).expect("handshake");
    let (side_tx, side_rx) = mpsc::channel();
    let link = AddonLink::spawn(reader, writer, ctx, side_tx).expect("reader thread");
    (link, side_rx)
}

struct Demo {
    view: AddonView,
    side: mpsc::Receiver<AddonMessage>,
    restart_at: Option<Instant>,
}

impl Demo {
    fn new(ctx: &egui::Context) -> Self {
        let mut view = AddonView::new("Sample", ctx);
        let (link, side) = connect(ctx);
        view.attach(link);
        Self {
            view,
            side,
            restart_at: None,
        }
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(message) = self.side.try_recv() {
            eprintln!("add-on: {message:?}");
        }
        if let Some(at) = self.restart_at {
            let now = Instant::now();
            if now >= at {
                self.restart_at = None;
                let (link, side) = connect(ui.ctx());
                self.side = side;
                self.view.attach(link);
            } else {
                ui.ctx().request_repaint_after(at - now);
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Restart add-on").clicked() && self.restart_at.is_none() {
                self.view.begin_restart();
                self.restart_at = Some(Instant::now() + RESTART_DELAY);
            }
            let _ = ui.button("Host button (Tab target)");
            ui.label(format!("{:?}", self.view.state()));
        });
        let rect = ui.available_rect_before_wrap();
        self.view.show(ui, rect);
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Add-on view demo",
        eframe::NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(Demo::new(&cc.egui_ctx)))),
    )
}
