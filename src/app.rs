use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::tools::{self, Tool};
use crate::ui as look;
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

/// `--open "Tool"` or `--open "Tool/Page"` (case-insensitive, also `--open=...`) picks the page
/// shown at start-up, e.g. `toolboks.exe --open "Resource Monitor/Details"`. Unknown names are
/// ignored, so a typo just opens the first page.
fn parse_open(args: &[String], tools: &[Box<dyn Tool>]) -> Option<(usize, usize)> {
    let spec = args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix("--open=")
            .map(str::to_string)
            .or_else(|| (a == "--open").then(|| args.get(i + 1).cloned()).flatten())
    })?;
    let (tool_name, page_name) = match spec.split_once('/') {
        Some((t, p)) => (t.trim(), Some(p.trim())),
        None => (spec.trim(), None),
    };
    let tool = tools
        .iter()
        .position(|t| t.name().eq_ignore_ascii_case(tool_name))?;
    let page = page_name
        .and_then(|name| {
            tools[tool]
                .pages()
                .iter()
                .position(|p| p.eq_ignore_ascii_case(name))
        })
        .unwrap_or(0);
    Some((tool, page))
}

impl App {
    pub fn new() -> Self {
        let tools = tools::all();
        let args: Vec<String> = std::env::args().skip(1).collect();
        let selected = parse_open(&args, &tools).unwrap_or((0, 0));
        Self {
            tools,
            selected,
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

    /// The update control in one row. Meant for a right-to-left layout: the first item added
    /// sits rightmost.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.update {
            UpdateState::Idle | UpdateState::UpToDate | UpdateState::Failed(_) => {
                let check = egui::Button::new(egui::RichText::new("Check for updates").size(11.0))
                    .small()
                    .frame(false);
                if ui.add(check).clicked() {
                    self.update = UpdateState::Checking;
                    self.spawn_update_job(&ctx, || UpdateMsg::Checked(update::check()));
                }
                match &self.update {
                    UpdateState::UpToDate => {
                        ui.small("You have the latest version.");
                    }
                    UpdateState::Failed(message) => {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message).small().color(look::AMBER),
                            )
                            .truncate(),
                        );
                    }
                    _ => {}
                }
            }
            UpdateState::Checking => {
                ui.small("Checking\u{2026}");
            }
            UpdateState::Available(release) => {
                let release = release.clone();
                let install = egui::Button::new(
                    egui::RichText::new("Install and restart")
                        .size(11.0)
                        .color(egui::Color32::WHITE),
                )
                .small()
                .fill(look::ACCENT);
                if ui.add(install).clicked() {
                    self.start_install(&ctx, release.clone());
                }
                ui.small(format!("Version {} is available.", release.version));
            }
            UpdateState::Installing => {
                ui.small("Installing\u{2026}");
            }
        }
    }
}

/// Flattens the status items of all tools (in tool order), trimmed, without empty ones.
fn status_items(groups: impl IntoIterator<Item = Vec<String>>) -> Vec<String> {
    groups
        .into_iter()
        .flatten()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
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
        let strip = || {
            egui::Frame::new()
                .fill(look::WINDOW_BG)
                .inner_margin(egui::Margin::symmetric(8, 0))
        };

        // Tool tabs, with the version and the update control on the right.
        egui::Panel::top("tools")
            .resizable(false)
            .frame(strip())
            .show(ui, |ui| {
                ui.set_height(28.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for ti in 0..self.tools.len() {
                        let name = self.tools[ti].name();
                        let on = self.selected.0 == ti;
                        if look::tab(ui, name, on, 28.0).clicked() && !on {
                            self.selected = (ti, 0);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.weak(format!("v{}", Version::current()));
                        self.update_ui(ui);
                    });
                });
            });

        // Pages of the selected tool; hidden when there is one page or none.
        let pages = self
            .tools
            .get(self.selected.0)
            .map_or(&[][..], |tool| tool.pages());
        if pages.len() > 1 {
            egui::Panel::top("pages")
                .resizable(false)
                .frame(strip())
                .show(ui, |ui| {
                    ui.set_height(24.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (pi, page) in pages.iter().enumerate() {
                            let on = self.selected.1 == pi;
                            if look::tab(ui, page, on, 24.0).clicked() {
                                self.selected.1 = pi;
                            }
                        }
                    });
                });
        }

        // Status bar: the status items of every tool, in tool order.
        let items = status_items(self.tools.iter().map(|tool| tool.status()));
        egui::Panel::bottom("status")
            .resizable(false)
            .exact_size(22.0)
            .frame(
                egui::Frame::new()
                    .fill(look::STATUS_BG)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for item in &items {
                        ui.label(egui::RichText::new(item).size(11.0).color(look::MUTED));
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(look::CARD_PAD))
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
    /// with "Not now" until the next launch (the top bar keeps an Install button).
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

#[cfg(test)]
mod tests {
    use super::{parse_open, status_items};
    use crate::tools;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn open_picks_a_tool_and_page_case_insensitively() {
        let tools = tools::all();
        assert_eq!(
            parse_open(&args(&["--open", "resource monitor/DETAILS"]), &tools),
            Some((1, 1))
        );
        assert_eq!(
            parse_open(&args(&["--open=Memory Cleaner/Advanced"]), &tools),
            Some((0, 1))
        );
        assert_eq!(
            parse_open(&args(&["--open", "Resource Monitor"]), &tools),
            Some((1, 0))
        );
    }

    #[test]
    fn open_ignores_unknown_names_and_missing_values() {
        let tools = tools::all();
        assert_eq!(parse_open(&args(&[]), &tools), None);
        assert_eq!(parse_open(&args(&["--open"]), &tools), None);
        assert_eq!(parse_open(&args(&["--open", "Nope"]), &tools), None);
        // A known tool with an unknown page opens that tool's first page.
        assert_eq!(
            parse_open(&args(&["--open", "Resource Monitor/Nope"]), &tools),
            Some((1, 0))
        );
    }

    #[test]
    fn status_items_keep_tool_order_and_skip_blanks() {
        let groups = vec![
            vec!["RAM 63%".to_owned(), "  ".to_owned()],
            Vec::new(),
            vec![" CPU 24% ".to_owned(), "GPU 7%".to_owned()],
        ];
        assert_eq!(status_items(groups), ["RAM 63%", "CPU 24%", "GPU 7%"]);
    }

    #[test]
    fn status_items_empty_when_no_tool_reports() {
        assert!(status_items([Vec::new(), Vec::new()]).is_empty());
    }
}
