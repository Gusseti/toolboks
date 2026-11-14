//! Test host for add-on authors: a small window that starts an add-on exe and shows its page
//! the way Toolboks does, with the same view code (`src/addon_view`) and the Toolboks style.
//! It is a development tool, not part of Toolboks; users never see it.
//!
//! ```text
//! cargo build -p hello-addon
//! cargo run --example addon_host -- target\debug\hello-addon.exe example.hello
//! ```
//!
//! The first argument is the add-on exe, the second its id (default `example.hello`).
//!
//! What it does and does not do:
//! - It talks to the add-on over stdin and stdout, which `run_addon` uses when it gets no pipe
//!   arguments. The real host uses named pipes, a launch token and a file check (see
//!   "Connection" and "Supervisor" in `docs/ADDON-PROTOCOL.md`); those are not exercised here.
//! - Status bar items are shown at the bottom, log lines and other messages go to the terminal.
//! - Preferences are kept in memory and given back after "Restart add-on", so you can see that
//!   your add-on restores them.

#[allow(dead_code)]
#[path = "../src/addon_view/mod.rs"]
mod addon_view;

use addon_view::{AddonLink, AddonView};
use eframe::egui;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use toolboks_addon_proto::{
    host_handshake, AddonMessage, FrameReader, FrameWriter, Hello, HostMessage,
};

struct Running {
    child: Child,
    side: mpsc::Receiver<AddonMessage>,
}

/// Starts the exe, does the handshake and hands the connection to the view.
fn start(
    ctx: &egui::Context,
    exe: &str,
    id: &str,
    preferences: &str,
    view: &mut AddonView,
) -> Result<Running, String> {
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start {exe}: {e}"))?;
    let mut reader = FrameReader::new(child.stdout.take().expect("piped stdout"));
    let mut writer = FrameWriter::new(child.stdin.take().expect("piped stdin"));
    if let Err(e) = host_handshake(&mut reader, &mut writer, &Hello::new(id, "addon_host")) {
        let _ = child.kill();
        return Err(format!("The add-on did not answer as \"{id}\": {e}"));
    }
    let (side_tx, side) = mpsc::channel();
    let link = AddonLink::spawn(reader, writer, ctx, side_tx).map_err(|e| e.to_string())?;
    let sender = link.sender();
    view.attach(link);
    if let Ok(mut sender) = sender.lock() {
        let json = preferences.to_string();
        let _ = sender.send(&HostMessage::LoadPreferences { json });
    }
    Ok(Running { child, side })
}

struct Host {
    exe: String,
    id: String,
    view: AddonView,
    running: Option<Running>,
    error: Option<String>,
    status: Vec<String>,
    /// The add-on's preferences as it last sent them (a JSON object).
    preferences: String,
}

impl Host {
    fn new(ctx: &egui::Context, exe: String, id: String) -> Self {
        let mut host = Self {
            view: AddonView::new(id.clone(), ctx),
            exe,
            id,
            running: None,
            error: None,
            status: Vec::new(),
            preferences: "{}".into(),
        };
        host.connect(ctx);
        host
    }

    fn connect(&mut self, ctx: &egui::Context) {
        match start(ctx, &self.exe, &self.id, &self.preferences, &mut self.view) {
            Ok(running) => {
                self.running = Some(running);
                self.error = None;
            }
            Err(error) => {
                self.view.stop(error.clone());
                self.error = Some(error);
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
        self.status.clear();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(running) = &self.running {
            while let Ok(message) = running.side.try_recv() {
                match message {
                    AddonMessage::StatusItems(items) => {
                        self.status = items.into_iter().map(|item| item.text).collect();
                    }
                    AddonMessage::Preferences { json, changed } => {
                        if changed {
                            eprintln!("preferences: {json}");
                            self.preferences = json;
                        }
                    }
                    AddonMessage::Log { level, line } => eprintln!("log {level:?}: {line}"),
                    AddonMessage::Pong { .. } => {}
                    other => eprintln!("add-on: {other:?}"),
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(format!("{}  ({})", self.id, self.exe));
            if ui.button("Restart add-on").clicked() {
                self.stop();
                self.view.begin_restart();
                self.connect(ui.ctx());
            }
            ui.label(format!("{:?}", self.view.state()));
        });
        if let Some(error) = &self.error {
            ui.label(error);
        }
        ui.separator();
        let mut rect = ui.available_rect_before_wrap();
        // The bottom line stands in for the Toolboks status bar.
        rect.max.y -= 28.0;
        self.view.show(ui, rect);
        ui.advance_cursor_after_rect(rect);
        ui.separator();
        ui.label(if self.status.is_empty() {
            "Status bar: nothing".to_string()
        } else {
            format!("Status bar: {}", self.status.join("  ·  "))
        });
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let Some(exe) = args.next() else {
        eprintln!("usage: cargo run --example addon_host -- <add-on exe> [add-on id]");
        std::process::exit(2);
    };
    let id = args.next().unwrap_or_else(|| "example.hello".into());
    eframe::run_native(
        "Toolboks add-on test host",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            toolboks_shared::ui::apply_style(&cc.egui_ctx);
            Ok(Box::new(Host::new(&cc.egui_ctx, exe, id)))
        }),
    )
}
