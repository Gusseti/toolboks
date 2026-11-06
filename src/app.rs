use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::instance;
use crate::tools::{self, Tool};
use crate::tray::{self, Tray};
use crate::ui as look;
use crate::update::{self, CheckOutcome, Release, UpdateError, Version};
mod shell;
mod tutorial;
use crate::addons::{self, Registry, SystemAddon};
use shell::Screen;

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
    base: SystemAddon,
    native: crate::native::NativeHost,
    registry: Registry,
    screen: Screen,
    addon_page: usize,
    search: String,
    install_path: String,
    show_install: bool,
    pending_package: Option<toolboks_addon_sdk::Package>,
    detail: Option<String>,
    remove_pending: Option<String>,
    message: Option<String>,
    preferences_dirty: Option<std::time::Instant>,
    tour: Option<tutorial::Run>,
    help: Option<String>,
    update: UpdateState,
    update_rx: Option<Receiver<UpdateMsg>>,
    /// The check at launch has been started (it runs once per launch).
    launch_check_started: bool,
    /// The running check is the launch check: say nothing unless an update exists.
    silent_check: bool,
    /// Show the "update available" prompt window.
    prompt_open: bool,
    /// "Keep running in the tray": the window's X hides it instead of quitting. Memory only.
    keep_in_tray: bool,
    tray: Option<Tray>,
    /// The window is hidden in the tray.
    hidden: bool,
    /// The tray's Quit was chosen: the next close request really closes.
    quitting: bool,
}

impl Drop for App {
    fn drop(&mut self) {
        if self.preferences_dirty.is_some() {
            let _ = self.registry.save();
        }
    }
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
        let registry = Registry::load(addons::data_root());
        let mut message = None;
        if !registry.root.join("config.json").exists() {
            if let Err(e) = registry.save() {
                message = Some(format!("Could not save configuration: {e}"));
            }
        }
        let keep_in_tray = registry.config.keep_in_tray;
        let mut screen = if parse_open(&args, &tools).is_some() {
            Screen::Base
        } else {
            Screen::Home
        };
        let mut native = crate::native::NativeHost::default();
        if let Some(index) = args.iter().position(|a| a == "--open-addon") {
            if let Some(id) = args.get(index + 1) {
                if let Some(package) = registry
                    .packages
                    .iter()
                    .find(|p| p.manifest.id == *id && registry.enabled(id))
                {
                    screen = Screen::External(id.clone());
                    if let Some(provider) = &package.manifest.native_provider {
                        let page = args
                            .iter()
                            .position(|a| a == "--addon-page")
                            .and_then(|i| args.get(i + 1))
                            .map(String::as_str)
                            .unwrap_or("0");
                        native.navigate(provider, page);
                    }
                }
            }
        }
        Self {
            native,
            base: SystemAddon { tools, selected },
            registry,
            screen,
            addon_page: 0,
            search: String::new(),
            install_path: String::new(),
            show_install: false,
            pending_package: None,
            detail: None,
            remove_pending: None,
            message,
            preferences_dirty: None,
            tour: None,
            help: None,
            update: UpdateState::Idle,
            update_rx: None,
            launch_check_started: false,
            silent_check: false,
            prompt_open: false,
            keep_in_tray,
            tray: None,
            hidden: false,
            quitting: false,
        }
    }

    /// Checks for an update once, right after launch. Offline, a private repo or an unreadable
    /// answer are ignored without a word; only a newer version shows the prompt.
    fn check_on_launch(&mut self, ctx: &egui::Context) {
        if cfg!(feature = "preview") || self.launch_check_started {
            return;
        }
        self.launch_check_started = true;
        self.silent_check = true;
        self.update = UpdateState::Checking;
        self.spawn_update_job(ctx, || UpdateMsg::Checked(update::check()));
    }

    fn start_install(&mut self, ctx: &egui::Context, release: Release) {
        if cfg!(feature = "preview") {
            return;
        }
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
        if cfg!(feature = "preview") {
            ui.weak("Preview · updates disabled");
            return;
        }
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
    std::process::Command::new(std::env::current_exe()?)
        .arg(instance::RELAUNCH_ARG)
        .spawn()?;
    instance::mark_relaunching();
    Ok(())
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_on_launch(ctx);
        self.poll_update(ctx);
        self.tray_tick(ctx);
        self.native.tick(ctx, &self.registry);
        for tool in &mut self.base.tools {
            tool.tick(ctx);
        }
        if self
            .preferences_dirty
            .is_some_and(|t| t.elapsed() > Duration::from_millis(500))
        {
            self.preferences_dirty = None;
            if let Err(e) = self.registry.save() {
                self.message = Some(format!("Preferences could not be saved: {e}"));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell_ui(ui);
    }
}

impl App {
    /// Tray mode: icon, menu, hiding on close, and keeping the app ticking while hidden.
    /// eframe 0.37 calls `logic` (and repaints every ~100 ms when asked) for a hidden window,
    /// so the tools' background work keeps running without any minimize trick.
    fn tray_tick(&mut self, ctx: &egui::Context) {
        if !self.keep_in_tray {
            if let Some(tray) = &self.tray {
                tray.set_visible(false);
            }
            if self.hidden {
                self.show_window(ctx);
            }
            return;
        }
        if self.tray.is_none() {
            self.tray = Tray::new(ctx);
            if self.tray.is_none() {
                // No icon means no way back from hiding: stay a normal window.
                self.keep_in_tray = false;
                return;
            }
        }
        let Some(tray) = self.tray.as_mut() else {
            return;
        };
        tray.set_visible(true);
        tray.refresh();
        let actions = tray.actions();
        // The percentage is polled, so wake up regularly even if nothing else asks.
        ctx.request_repaint_after(Duration::from_secs(if self.hidden { 1 } else { 2 }));

        for action in actions {
            match action {
                tray::Action::Toggle if self.hidden => self.show_window(ctx),
                tray::Action::Toggle => self.hide_window(ctx),
                tray::Action::Show => self.show_window(ctx),
                tray::Action::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let close = ctx.input(|i| i.viewport().close_requested());
        if close && !self.quitting && !instance::is_relaunching() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
        // Another launch (or the user) brought the window back without the tray.
        if self.hidden && ctx.input(|i| i.viewport().focused) == Some(true) {
            self.hidden = false;
        }
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

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
                if let Some(size) = release.size {
                    ui.small(format!("Download about {}", update::download_size(size)));
                }
                let notes = update::plain_notes(&release.notes);
                if !notes.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("What's new").strong());
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .id_salt("update_notes")
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(notes).small().weak());
                        });
                }
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
