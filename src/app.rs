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
    /// (tool, page) shown in the main area.
    selected: (usize, usize),
    update: UpdateState,
    update_rx: Option<Receiver<UpdateMsg>>,
    /// The check at launch has been started (it runs once per launch).
    launch_check_started: bool,
    /// The running check is the launch check: say nothing unless an update exists.
    silent_check: bool,
    /// Show the "update available" prompt window.
    prompt_open: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            tools: tools::all(),
            selected: (0, 0),
            update: UpdateState::Idle,
            update_rx: None,
            launch_check_started: false,
            silent_check: false,
            prompt_open: false,
        }
    }

    /// Checks for an update once, right after launch. Offline, a private repo or an unreadable
    /// answer are ignored without a word; only a newer version shows the prompt.
    fn check_on_launch(&mut self, ctx: &egui::Context) {
        if self.launch_check_started {
            return;
        }
        self.launch_check_started = true;
        self.silent_check = true;
        self.update = UpdateState::Checking;
        self.spawn_update_job(ctx, || UpdateMsg::Checked(update::check()));
    }

    fn start_install(&mut self, ctx: &egui::Context, release: Release) {
        self.prompt_open = false;
        self.update = UpdateState::Installing;
        self.spawn_update_job(ctx, move || UpdateMsg::Installed(update::install(&release)));
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

        let silent = std::mem::take(&mut self.silent_check);
        self.update = match msg {
            UpdateMsg::Checked(Ok(CheckOutcome::UpToDate)) if silent => UpdateState::Idle,
            UpdateMsg::Checked(Ok(CheckOutcome::UpToDate)) => UpdateState::UpToDate,
            UpdateMsg::Checked(Ok(CheckOutcome::Available(release))) => {
                self.prompt_open = true;
                UpdateState::Available(release)
            }
            UpdateMsg::Checked(Err(_)) if silent => UpdateState::Idle,
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
                    self.start_install(&ctx, release);
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
        self.check_on_launch(ctx);
        self.poll_update(ctx);
        for tool in &mut self.tools {
            tool.tick(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(170.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("toolboks").heading().strong());
                ui.add_space(8.0);

                for (ti, tool) in self.tools.iter().enumerate() {
                    let pages = tool.pages();
                    if pages.is_empty() {
                        ui.selectable_value(&mut self.selected, (ti, 0), tool.name());
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(tool.name()).small().weak());
                    for (pi, page) in pages.iter().enumerate() {
                        ui.selectable_value(&mut self.selected, (ti, pi), *page);
                    }
                }

                // Bottom-up: the first item added sits lowest.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(4.0);
                    ui.weak(format!("v{}", Version::current()));
                    self.update_ui(ui);
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(24.0))
            .show(ui, |ui| {
                let (tool, page) = self.selected;
                if let Some(tool) = self.tools.get_mut(tool) {
                    tool.ui(ui, page);
                }
            });

        self.update_prompt(&ui.ctx().clone());
    }
}

impl App {
    /// The "update available" question: shown once a newer release is found, and dismissed
    /// with "Not now" until the next launch (the sidebar keeps an Install button).
    fn update_prompt(&mut self, ctx: &egui::Context) {
        if !self.prompt_open {
            return;
        }
        let UpdateState::Available(release) = &self.update else {
            self.prompt_open = false;
            return;
        };
        let release = release.clone();

        let (mut install, mut later) = (false, false);
        egui::Window::new("Update available")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(340.0);
                ui.label(
                    egui::RichText::new(format!("toolboks {} is ready", release.version))
                        .strong()
                        .size(17.0),
                );
                ui.label(format!(
                    "You are using version {}. Do you want to update now? \
                     The app will restart.",
                    Version::current()
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Update now").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(38, 110, 214)),
                        )
                        .clicked()
                    {
                        install = true;
                    }
                    if ui.button("Not now").clicked() {
                        later = true;
                    }
                });
            });

        if install {
            self.start_install(ctx, release);
        } else if later {
            self.prompt_open = false;
        }
    }
}
