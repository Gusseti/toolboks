use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::tools::{self, Tool};
use crate::update::{self, CheckOutcome, Release, UpdateError, Version};

/// Results sent back from the update worker thread.
enum UpdateMsg {
    Checked(Result<CheckOutcome, UpdateError>),
    Installed(Result<(), UpdateError>),
}

enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Installing,
    Failed(String),
}

pub struct App {
    tools: Vec<Box<dyn Tool>>,
    selected: usize,
    update: UpdateState,
    update_rx: Option<Receiver<UpdateMsg>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            tools: tools::all(),
            selected: 0,
            update: UpdateState::Idle,
            update_rx: None,
        }
    }

    /// Runs `work` on a thread and wakes the UI when it reports back.
    fn spawn_update_job(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce() -> UpdateMsg + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        self.update_rx = Some(rx);
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.update_rx else {
            return;
        };
        let Ok(msg) = rx.try_recv() else {
            // Still working; look again shortly, even if the window is hidden.
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        };
        self.update_rx = None;

        self.update = match msg {
            UpdateMsg::Checked(Ok(CheckOutcome::UpToDate)) => UpdateState::UpToDate,
            UpdateMsg::Checked(Ok(CheckOutcome::Available(release))) => {
                UpdateState::Available(release)
            }
            UpdateMsg::Checked(Err(e)) | UpdateMsg::Installed(Err(e)) => {
                UpdateState::Failed(e.to_string())
            }
            UpdateMsg::Installed(Ok(())) => match restart() {
                Ok(()) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    UpdateState::Installing
                }
                Err(e) => UpdateState::Failed(format!(
                    "Updated, but could not restart ({e}). Start toolboks again."
                )),
            },
        };
    }

    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.update {
            UpdateState::Idle | UpdateState::UpToDate | UpdateState::Failed(_) => {
                if ui.button("Check for updates").clicked() {
                    self.update = UpdateState::Checking;
                    self.spawn_update_job(&ctx, || UpdateMsg::Checked(update::check()));
                }
                match &self.update {
                    UpdateState::UpToDate => {
                        ui.small("You have the latest version.");
                    }
                    UpdateState::Failed(message) => {
                        ui.add(egui::Label::new(egui::RichText::new(message).small()).wrap());
                    }
                    _ => {}
                }
            }
            UpdateState::Checking => {
                ui.small("Checking\u{2026}");
            }
            UpdateState::Available(release) => {
                let release = release.clone();
                ui.small(format!("Version {} is available.", release.version));
                if ui.button("Install and restart").clicked() {
                    self.update = UpdateState::Installing;
                    self.spawn_update_job(&ctx, move || {
                        UpdateMsg::Installed(update::install(&release))
                    });
                }
            }
            UpdateState::Installing => {
                ui.small("Installing\u{2026}");
            }
        }
    }
}

/// Starts the freshly installed exe. It inherits this process's rights, admin included.
fn restart() -> std::io::Result<()> {
    std::process::Command::new(std::env::current_exe()?).spawn()?;
    Ok(())
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_update(ctx);
        for tool in &mut self.tools {
            tool.tick(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(160.0)
            .show(ui, |ui| {
                ui.heading("toolboks");
                ui.separator();
                for (i, tool) in self.tools.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, i, tool.name());
                }

                // Bottom-up: the first item added sits lowest.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.weak(format!("v{}", Version::current()));
                    self.update_ui(ui);
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(tool) = self.tools.get_mut(self.selected) {
                ui.heading(tool.name());
                ui.separator();
                tool.ui(ui);
            }
        });
    }
}
