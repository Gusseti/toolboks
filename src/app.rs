use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::instance;
use crate::tray::{self, Tray};
use crate::ui as look;
use crate::update::{self, CheckOutcome, Release, UpdateError, Version};
#[cfg(feature = "preview")]
pub mod addon_check;
mod addon_tick;
#[cfg(feature = "preview")]
pub mod disk_manager_check;
mod marketplace;
#[cfg(feature = "preview")]
pub mod marketplace_check;
#[cfg(feature = "preview")]
pub mod restore_check;
mod shell;
pub(crate) mod store;
mod tutorial;
use crate::addons::{self, Registry};
use shell::Screen;

/// The smallest window Toolboks supports, in points: width, height. `main.rs` sets it as the
/// window's minimum and DESIGN.md states it. Nothing in the shell is laid out for less.
pub const MIN_INNER_SIZE: [f32; 2] = [1060.0, 720.0];

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
    market: crate::marketplace::Marketplace,
    supervisor: Option<crate::supervisor::Supervisor>,
    views: std::collections::BTreeMap<String, crate::addon_view::AddonView>,
    services: crate::addon_services::Services,
    registry: Registry,
    screen: Screen,
    /// The sidebar shows names (204 px) instead of icons only (52 px). Memory only.
    sidebar_expanded: bool,
    addon_page: usize,
    search: String,
    install_path: String,
    show_install: bool,
    pending_package: Option<toolboks_addon_sdk::Package>,
    pending_declarative: Option<String>,
    detail: Option<String>,
    /// The add-on selected in the Home list (by id). Memory only.
    home_selected: Option<String>,
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
    /// When the add-on work last ran, and whether `logic` ran it in this pass (#176).
    addon_tick: addon_tick::AddonTick,
    /// The window lost focus after it was hidden in the tray; see [`back_without_tray`].
    focus_lost_while_hidden: bool,
    /// Start update cleanup only after the first frame has rendered successfully.
    first_frame: Option<std::time::Instant>,
    update_cleanup_done: bool,
    /// "Keep running in the tray": the window's X hides it instead of quitting.
    keep_in_tray: bool,
    tray: Option<Tray>,
    /// The window is hidden in the tray.
    hidden: bool,
    /// The tray's Quit was chosen: the next close request really closes.
    quitting: bool,
    /// Toolboks's own CPU and memory, read only while the Settings page is drawn.
    own_usage: crate::own_usage::OwnUsage,
}

impl Drop for App {
    fn drop(&mut self) {
        if self.preferences_dirty.is_some() {
            let _ = self.registry.save();
        }
    }
}

/// `--open "Add-on"` or `--open "Add-on/Page"` (case-insensitive, also `--open=...`) names the
/// installed add-on and page to show at start-up, e.g. `toolboks.exe --open "Resource
/// Monitor/Details"`. A name that matches nothing, or an add-on that is off or not installed,
/// leaves the overview with a short explanation instead.
fn open_spec(args: &[String]) -> Option<String> {
    args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix("--open=")
            .map(str::to_string)
            .or_else(|| (a == "--open").then(|| args.get(i + 1).cloned()).flatten())
    })
}

/// Where a start-up `--open` spec leads: the screen and page, or a message to show.
fn open_target(registry: &Registry, spec: &str) -> Result<(String, usize), String> {
    let installed = crate::installer::installed(&registry.root);
    match crate::addon_services::resolve_link(spec, &installed, |id| registry.enabled(id), &[]) {
        crate::addon_services::Link::Open { id, page } => {
            let index = page
                .and_then(|name| {
                    installed
                        .iter()
                        .find(|entry| entry.id == id)
                        .and_then(|entry| {
                            entry
                                .pages
                                .iter()
                                .position(|p| p.eq_ignore_ascii_case(&name))
                        })
                })
                .unwrap_or(0);
            Ok((id, index))
        }
        crate::addon_services::Link::TurnedOff { title, .. } => Err(format!(
            "{title} is turned off. You can turn it on again in the add-on browser."
        )),
        crate::addon_services::Link::NotInstalled { title, .. } => Err(format!(
            "{title} is not installed. You can add it in the add-on browser."
        )),
        crate::addon_services::Link::Unknown => Err("That add-on is not available.".into()),
    }
}

impl App {
    pub fn new() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let registry = Registry::load(addons::data_root());
        let mut message = None;
        let keep_in_tray = registry.config.keep_in_tray;
        let mut addon_page = 0;
        let mut screen = Screen::Home;
        let mut market = crate::marketplace::Marketplace::restore(&registry.root);
        if !registry.config.restore_pending.is_empty() {
            // Updating from the version that had the tools built in: offer them back, ticked.
            screen = Screen::Pick;
            for id in &registry.config.restore_pending {
                market.select(id, true);
            }
        } else if !registry.config.first_run_complete
            && registry.packages.is_empty()
            && crate::installer::installed(&registry.root).is_empty()
        {
            // A first start has no tools yet: the picker opens by itself.
            screen = Screen::Pick;
        }
        if let Some(spec) = open_spec(&args) {
            match open_target(&registry, &spec) {
                Ok((id, page)) => {
                    screen = Screen::External(id);
                    addon_page = page;
                }
                Err(text) => message = Some(text),
            }
        }
        if let Some(index) = args.iter().position(|a| a == "--open-addon") {
            if let Some(id) = args.get(index + 1) {
                let known = registry.packages.iter().any(|p| p.manifest.id == *id)
                    || crate::installer::installed_entry(&registry.root, id).is_some();
                if known && registry.enabled(id) {
                    screen = Screen::External(id.clone());
                    addon_page = args
                        .iter()
                        .position(|a| a == "--addon-page")
                        .and_then(|i| args.get(i + 1))
                        .and_then(|page| page.parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        Self {
            market,
            supervisor: None,
            views: Default::default(),
            services: Default::default(),
            registry,
            screen,
            sidebar_expanded: false,
            addon_page,
            search: String::new(),
            install_path: String::new(),
            show_install: false,
            pending_package: None,
            pending_declarative: None,
            detail: None,
            home_selected: None,
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
            addon_tick: Default::default(),
            focus_lost_while_hidden: false,
            first_frame: None,
            update_cleanup_done: cfg!(feature = "preview"),
            keep_in_tray,
            tray: None,
            hidden: false,
            quitting: false,
            own_usage: Default::default(),
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
        // This process must never clean up the backup produced by the install it is starting.
        self.update_cleanup_done = true;
        self.first_frame = None;
        self.update = UpdateState::Installing;
        update::set_installing(true);
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
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.update_rx = None;
                update::set_installing(false);
                self.update = UpdateState::Failed("The update stopped unexpectedly.".into());
                return;
            }
        };
        self.update_rx = None;
        if matches!(msg, UpdateMsg::Installed(_)) {
            update::set_installing(false);
        }

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
                    "Updated, but could not restart ({e}). Start Toolboks again."
                )),
            },
        };
    }

    /// The update control: one wrapping row of a button and a plain status line.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        if cfg!(feature = "preview") {
            ui.label(
                egui::RichText::new("This is a preview. Updates are turned off.")
                    .size(look::TEXT_CAPTION)
                    .color(look::MUTED),
            );
            return;
        }
        let ctx = ui.ctx().clone();
        let status = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(text)
                        .size(look::TEXT_CAPTION)
                        .color(color),
                )
                .wrap(),
            );
        };
        ui.horizontal_wrapped(|ui| match &self.update {
            UpdateState::Idle | UpdateState::UpToDate | UpdateState::Failed(_) => {
                if ui.button("Check for updates").clicked() {
                    self.update = UpdateState::Checking;
                    self.spawn_update_job(&ctx, || UpdateMsg::Checked(update::check()));
                }
                match &self.update {
                    UpdateState::UpToDate => {
                        status(ui, "You have the latest version.", look::MUTED)
                    }
                    UpdateState::Failed(message) => status(ui, message, look::AMBER),
                    _ => {}
                }
            }
            UpdateState::Checking => status(ui, "Checking for updates...", look::MUTED),
            UpdateState::Available(release) => {
                let release = release.clone();
                if shell::primary(ui, "Install and restart").clicked() {
                    self.start_install(&ctx, release.clone());
                }
                status(
                    ui,
                    &format!("Version {} is available.", release.version),
                    look::MUTED,
                );
            }
            UpdateState::Installing => status(ui, "Installing the update...", look::MUTED),
        });
    }
}

/// For a window hidden in the tray: true when it was brought back without the tray (another launch
/// shows and focuses it, see `instance::bring_forward`), which is when it has focus again after
/// losing it while hidden. The focus it still has in the pass it is hidden in (the X was clicked on
/// a focused window) does not count: that made the window count as shown again at once, so a page
/// add-on kept running in the tray (#176).
fn back_without_tray(focus_lost: &mut bool, focused: Option<bool>) -> bool {
    if focused == Some(true) {
        *focus_lost
    } else {
        *focus_lost = true;
        false
    }
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
        if !self.install_in_progress()
            && !instance::is_relaunching()
            && self
                .first_frame
                .is_some_and(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.first_frame = None;
            self.update_cleanup_done = true;
            update::cleanup_old_exe();
        }
        self.check_on_launch(ctx);
        if self.install_in_progress() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.message = Some("Wait for the update to finish before closing Toolboks.".into());
        }
        self.poll_update(ctx);
        self.tray_tick(ctx);
        self.addons_logic(ctx);
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
        if !self.update_cleanup_done && self.first_frame.is_none() {
            self.first_frame = Some(std::time::Instant::now());
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
    }
}

impl App {
    fn install_in_progress(&self) -> bool {
        update::is_installing()
    }

    /// The add-on work from `logic` (#176): eframe runs no `ui` for a minimized window (and may not
    /// for one hidden in the tray), so without this the supervisor would not tick there (no
    /// restart of a crashed background add-on, no hiding of a page add-on). Once a second at
    /// most, and never in the same pass as the `ui` run; see `addon_tick`.
    fn addons_logic(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        if self.addon_tick.logic_due(now) {
            self.tick_addons(ctx);
        }
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        if addon_tick::wake_for_tick(self.hidden, minimized) {
            ctx.request_repaint_after(self.addon_tick.wait(now));
        }
    }
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
        // The percentage is polled, so wake up when the next refresh is due even if nothing else
        // asks (every 2 s; clicks on the icon wake the app at once by themselves).
        ctx.request_repaint_after(tray.refresh_in(std::time::Instant::now()));
        let installing = self.install_in_progress();

        for action in actions {
            match action {
                tray::Action::Toggle if self.hidden => self.show_window(ctx),
                tray::Action::Toggle => self.hide_window(ctx),
                tray::Action::Show => self.show_window(ctx),
                tray::Action::Quit if installing => {
                    self.show_window(ctx);
                    self.message =
                        Some("Wait for the update to finish before closing Toolboks.".into());
                }
                tray::Action::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        let close = ctx.input(|i| i.viewport().close_requested());
        if close && !installing && !self.quitting && !instance::is_relaunching() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
        // Another launch (or the user) brought the window back without the tray.
        if self.hidden
            && back_without_tray(
                &mut self.focus_lost_while_hidden,
                ctx.input(|i| i.viewport().focused),
            )
        {
            self.hidden = false;
        }
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.hidden = true;
        self.focus_lost_while_hidden = false;
        // The Settings figure starts fresh when the window is back, instead of averaging over
        // the whole time it was hidden.
        self.own_usage.reset();
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

        let (mut install, mut later) = (false, false);
        egui::Window::new("Update available")
            .collapsible(false)
            .resizable(false)
            .frame(shell::panel().shadow(egui::Shadow {
                offset: [0, 4],
                blur: 16,
                spread: 0,
                color: egui::Color32::from_black_alpha(120),
            }))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(340.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
                ui.label(
                    egui::RichText::new(format!("Toolboks {} is ready", release.version))
                        .size(look::TEXT_TITLE)
                        .strong()
                        .color(look::TEXT),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "You are using version {}. Update now? Toolboks restarts to finish.",
                        Version::current()
                    ))
                    .color(look::MUTED),
                );
                if let Some(size) = release.size {
                    ui.label(
                        egui::RichText::new(format!(
                            "Download about {}",
                            update::download_size(size)
                        ))
                        .size(look::TEXT_CAPTION)
                        .color(look::MUTED),
                    );
                }
                let notes = &release.notes;
                if !notes.is_empty() {
                    ui.add_space(look::GAP);
                    ui.label(
                        egui::RichText::new("What is new")
                            .size(look::TEXT_SECTION)
                            .strong()
                            .color(look::TEXT),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .id_salt("update_notes")
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(notes)
                                    .size(look::TEXT_CAPTION)
                                    .color(look::MUTED),
                            );
                        });
                }
                ui.add_space(look::SPACE_M);
                ui.horizontal(|ui| {
                    if shell::primary(ui, "Update now").clicked() {
                        install = true;
                    }
                    if ui.button("Not now").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        later = true;
                    }
                });
            });

        if install {
            self.start_install(ctx, release.clone());
        } else if later {
            self.prompt_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{back_without_tray, open_spec, open_target};

    #[test]
    fn a_hidden_window_counts_as_back_only_when_focus_returns_after_it_was_lost() {
        // Hidden while focused (the X clicked): still hidden, however long the old focus is seen.
        let mut lost = false;
        assert!(!back_without_tray(&mut lost, Some(true)));
        assert!(!back_without_tray(&mut lost, Some(true)));
        // The focus goes, then another launch brings the window forward.
        assert!(!back_without_tray(&mut lost, Some(false)));
        assert!(back_without_tray(&mut lost, Some(true)));
        // Unknown focus counts as not focused.
        let mut lost = false;
        assert!(!back_without_tray(&mut lost, None));
        assert!(back_without_tray(&mut lost, Some(true)));
    }
    use crate::addons::Registry;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn open_reads_the_name_in_both_forms() {
        assert_eq!(
            open_spec(&args(&["--open", "resource monitor/DETAILS"])).as_deref(),
            Some("resource monitor/DETAILS")
        );
        assert_eq!(
            open_spec(&args(&["--open=Memory Cleaner/Advanced"])).as_deref(),
            Some("Memory Cleaner/Advanced")
        );
        assert_eq!(open_spec(&args(&[])), None);
        assert_eq!(open_spec(&args(&["--open"])), None);
    }

    #[test]
    fn open_of_an_add_on_that_is_not_installed_falls_back_with_a_message() {
        let root = std::env::temp_dir().join(format!("toolboks-open-{}", std::process::id()));
        let registry = Registry::load(root.clone());
        let message = open_target(&registry, "Resource Monitor/Details").unwrap_err();
        assert!(message.contains("not available"), "{message}");
        std::fs::remove_dir_all(root).ok();
    }
}
