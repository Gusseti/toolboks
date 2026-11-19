use super::addon_tick;
use super::shell::{muted, Screen, Tag};
use super::store;
use super::*;
use crate::catalog::Entry;
use crate::marketplace::{self as market, InstallStatus};
use crate::ui::{MUTED, TEXT};
use egui::Vec2;
use toolboks_addon_sdk::tutorial as anchors;

impl App {
    pub(super) fn market_header(&mut self, ui: &mut egui::Ui) {
        if ui.button("Show me around").clicked() {
            self.help = Some("toolboks.addon-browser".into());
        }
        if self.screen == Screen::Browser {
            if ui.button("View details").clicked() {
                self.market.details = true;
            }
            ui.checkbox(&mut self.market.installed_only, "Installed only");
            if ui.button("Refresh list").clicked() {
                self.market.refresh(self.registry.root.clone(), ui.ctx());
            }
            if ui.button("Install from file").clicked() {
                self.show_install = !self.show_install;
            }
            if ui.button("Pick your tools").clicked() {
                self.screen = Screen::Pick;
            }
        }
    }

    pub(super) fn installed_version(&self, id: &str) -> Option<String> {
        crate::installer::installed_version(&self.registry.root, id).or_else(|| {
            self.registry
                .packages
                .iter()
                .find(|package| package.manifest.id == id)
                .map(|package| package.manifest.version.clone())
        })
    }

    /// From `ui`: the add-on work, unless the `logic` of this pass already ran it (#176), then
    /// the repaints a drawn window needs for its add-on views.
    pub(super) fn poll_marketplace(&mut self, ctx: &egui::Context) {
        if self.addon_tick.ui_due(std::time::Instant::now()) {
            self.tick_addons(ctx);
        }
        self.poll_views(ctx);
    }

    /// The add-on work that does not draw: the add-on list timer, install jobs, the supervisor
    /// tick, the hide rule and starting background add-ons. It runs from `ui` while the window is
    /// drawn and from `logic` once a second while it is not (hidden in the tray or minimized);
    /// see `addon_tick`.
    pub(super) fn tick_addons(&mut self, ctx: &egui::Context) {
        let busy = self.market.busy() || self.market.review.is_some();
        let due = self.market.refresh_due(std::time::Instant::now());
        match addon_tick::list_fetch(self.market.checked, self.hidden, due, busy) {
            addon_tick::ListFetch::First => self.market.refresh(self.registry.root.clone(), ctx),
            // Fetch the list again every few minutes while the app is open, so a new release is
            // noticed without pressing Refresh. Nothing on screen changes while it runs.
            addon_tick::ListFetch::Quiet => {
                self.market.refresh_quietly(self.registry.root.clone(), ctx);
            }
            addon_tick::ListFetch::Nothing => {}
        }
        // Wake up now and then so the timer above also runs in a window nobody is touching.
        ctx.request_repaint_after(Duration::from_secs(30));
        let running = crate::installer::installed(&self.registry.root)
            .into_iter()
            .filter(|entry| {
                self.supervisor
                    .as_ref()
                    .is_some_and(|supervisor| supervisor.is_running(&entry.id))
            })
            .map(|entry| entry.id)
            .collect();
        let ready_before: Vec<String> = self
            .market
            .jobs
            .iter()
            .filter(|(_, job)| job.status == InstallStatus::Ready)
            .map(|(id, _)| id.clone())
            .collect();
        if self.market.poll(&self.registry.root, ctx, &running) {
            let config = self.registry.config.clone();
            self.registry = Registry::load(self.registry.root.clone());
            self.registry.config = config;
        }
        // An add-on that has just finished installing or updating is "New" until it is opened.
        let finished: Vec<String> = self
            .market
            .jobs
            .iter()
            .filter(|(id, job)| job.status == InstallStatus::Ready && !ready_before.contains(id))
            .map(|(id, _)| id.clone())
            .collect();
        if !finished.is_empty() {
            for id in finished {
                if !self.registry.config.new_addons.contains(&id) {
                    self.registry.config.new_addons.push(id);
                }
            }
            self.persist();
        }
        if self.supervisor.is_none() {
            match crate::supervisor::Supervisor::new(ctx) {
                Ok(supervisor) => self.supervisor = Some(supervisor),
                Err(error) => {
                    self.message = Some(format!("Could not start add-ons: {error}"));
                    return;
                }
            }
        }
        let supervisor = self.supervisor.as_mut().unwrap();
        for event in supervisor.tick(std::time::Instant::now()) {
            match self.services.apply(
                &event,
                supervisor,
                &self.registry.root,
                &mut self.registry.config,
            ) {
                Some(crate::addon_services::Effect::SaveConfig) => {
                    self.preferences_dirty = Some(std::time::Instant::now());
                }
                Some(crate::addon_services::Effect::FindingsDropped { id, lines }) => {
                    for line in lines {
                        supervisor.note(&id, line);
                    }
                }
                _ => {}
            }
            match event {
                crate::supervisor::Event::Connected { id } => {
                    let view = self
                        .views
                        .entry(id.clone())
                        .or_insert_with(|| crate::addon_view::AddonView::new(&id, ctx));
                    if let Some(link) = supervisor.take_link(&id) {
                        view.attach(link);
                    }
                    // A route from a Home finding wins over the page index, once.
                    let route = match self.pending_route.take() {
                        Some((for_id, route)) if for_id == id => Some(route),
                        other => {
                            self.pending_route = other;
                            None
                        }
                    };
                    let page = match self.detached.get(&id) {
                        Some(window) => Some(window.page),
                        None => {
                            (self.screen == Screen::External(id.clone())).then_some(self.addon_page)
                        }
                    };
                    if let Some(page) = page {
                        supervisor.send(
                            &id,
                            &toolboks_addon_proto::HostMessage::Navigate {
                                page: route.unwrap_or_else(|| page.to_string()),
                            },
                        );
                    }
                }
                crate::supervisor::Event::Stopped { id, reason } => {
                    if let Some(view) = self.views.get_mut(&id) {
                        view.stop(reason.text());
                    }
                    // A stop that lasts puts a detached add-on back in the sidebar.
                    if shell::detach::back_on_stop(&reason) {
                        self.detached.remove(&id);
                    }
                }
                crate::supervisor::Event::Restarting { id } => {
                    if let Some(view) = self.views.get_mut(&id) {
                        view.begin_restart();
                    }
                }
                crate::supervisor::Event::Stalled { id, stalled } => {
                    if let Some(view) = self.views.get_mut(&id) {
                        view.set_stalled(stalled);
                    }
                }
                _ => {}
            }
        }
        for entry in crate::installer::installed(&self.registry.root) {
            if entry.kind != crate::catalog::Kind::Native {
                continue;
            }
            let on_screen = shell::detach::on_screen(&self.screen, &self.detached, &entry.id);
            if addon_tick::hide_addon(on_screen, self.hidden) {
                supervisor.hide(&entry.id);
            }
            if entry.background
                && self.registry.enabled(&entry.id)
                && !self
                    .market
                    .jobs
                    .get(&entry.id)
                    .is_some_and(|job| job.status.busy())
                && self
                    .market
                    .remove
                    .as_ref()
                    .is_none_or(|(id, _)| id != &entry.id)
            {
                supervisor.start_background(
                    crate::addon_services::launch_spec(&self.registry.root, &entry),
                    std::time::Instant::now(),
                );
            }
        }
    }

    /// Only from `ui`: the repaint an add-on view on screen needs. Not while hidden in the tray:
    /// eframe still runs `ui` for a window hidden with `Visible(false)` on Windows.
    fn poll_views(&mut self, ctx: &egui::Context) {
        if self.hidden {
            return;
        }
        // The add-on in the main window and every add-on in a window of its own.
        let main = match &self.screen {
            Screen::External(id) => Some(id),
            _ => None,
        };
        for id in main.into_iter().chain(self.detached.keys()) {
            let shown = self.views.get(id).map(|view| view.state());
            if let Some(after) = crate::addon_view::poll_after(shown) {
                ctx.request_repaint_after(after);
            }
        }
    }

    fn finish_picker(&mut self) {
        self.registry.config.first_run_complete = true;
        self.persist();
        self.screen = Screen::Home;
    }

    /// Drops the tools that are installed now from the list of tools to restore. Returns true
    /// while some are still waiting.
    pub(super) fn restore_pending(&mut self) -> bool {
        let installed: Vec<String> = self
            .registry
            .config
            .restore_pending
            .iter()
            .filter(|id| self.installed_version(id).is_some())
            .cloned()
            .collect();
        if !installed.is_empty() {
            self.registry
                .config
                .restore_pending
                .retain(|id| !installed.contains(id));
            self.persist();
        }
        !self.registry.config.restore_pending.is_empty()
    }

    fn catalog_notice(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "preview")]
        if std::env::var_os("TOOLBOKS_TEST_CATALOG").is_some() {
            store::paragraph(
                ui,
                "Local test catalog. These are SDK samples, not published tools.",
                13.0,
            );
        }
        if self.market.loading {
            muted(ui, "Checking for add-ons...");
        }
        if let Some(error) = self.market.error.clone() {
            muted(ui, error);
            ui.horizontal(|ui| {
                if ui.button("Try again").clicked() {
                    self.market.refresh(self.registry.root.clone(), ui.ctx());
                }
                if ui.button("Show installed").clicked() {
                    self.market.installed_only = true;
                    self.screen = Screen::Browser;
                }
            });
        }
    }

    pub(super) fn pick_tools(&mut self, ui: &mut egui::Ui) {
        if self.market.review.is_some() {
            self.install_review(ui);
            return;
        }
        let restoring = self.restore_pending();
        // Everything chosen is installed and nothing is waiting or failed: the picker has no job
        // left, so go to the toolkit. A failed install keeps the page so it can be retried.
        if !restoring
            && !self.market.jobs.is_empty()
            && self.market.selected.is_empty()
            && self
                .market
                .jobs
                .values()
                .all(|job| job.status == InstallStatus::Ready)
        {
            let ids: Vec<String> = self.market.jobs.keys().cloned().collect();
            for id in ids {
                self.market.dismiss(&id);
            }
            self.finish_picker();
            return;
        }
        let width = ui.available_width().min(760.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                if restoring {
                    store::page_title(
                        ui,
                        "Restore your tools",
                        "Toolboks now downloads each tool on its own. The tools you had are ticked below; install them to get them back with your settings.",
                    );
                } else {
                    store::page_title(
                        ui,
                        "Pick your tools",
                        "Add the tools you need. You can add or remove them later.",
                    );
                }
                self.catalog_notice(ui);
                let mut entries: Vec<_> = self
                    .market
                    .entries
                    .iter()
                    .filter(|entry| entry.verified && entry.publisher == "Toolboks")
                    .cloned()
                    .collect();
                entries.sort_by_key(market::pick_rank);
                store::paragraph(
                    ui,
                    if restoring {
                        "From the Toolboks team. Your earlier tools are ticked."
                    } else {
                        "From the Toolboks team. No tools selected by default."
                    },
                    13.0,
                );
                ui.add_space(12.0);
                let list_height = (ui.available_height() - 150.0).max(120.0);
                anchors::region(ui, "browser-list", |ui| {
                    store::card(ui, 8, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("tool-picker")
                            .max_height(list_height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                for entry in &entries {
                                    self.pick_row(ui, entry);
                                }
                                self.job_list(ui);
                            });
                    });
                });
                ui.add_space(16.0);
                let selected = self.market.selected_entries();
                store::paragraph(
                    ui,
                    &format!(
                        "{} selected \u{b7} {} download",
                        selected.len(),
                        market::size(selected.iter().map(|entry| entry.size).sum())
                    ),
                    13.0,
                );
                store::paragraph(
                    ui,
                    "Review what they ask to access before installing.",
                    13.0,
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let enabled = !selected.is_empty() && !self.market.loading;
                    if ui
                        .add_enabled_ui(enabled, |ui| {
                            store::button(ui, "Install selected", store::Look::Primary)
                        })
                        .inner
                        .clicked()
                    {
                        self.market.review = Some(selected);
                    }
                    if restoring {
                        // Not now keeps the list, so the offer is made again at the next start.
                        if store::button(ui, "Not now", store::Look::Secondary).clicked() {
                            self.finish_picker();
                        }
                        if store::button(ui, "Forget these tools", store::Look::Secondary)
                            .clicked()
                        {
                            self.registry.config.restore_pending.clear();
                            self.finish_picker();
                        }
                    } else if store::button(ui, "Skip", store::Look::Secondary).clicked() {
                        self.finish_picker();
                    }
                    if store::button(ui, "Browse all add-ons", store::Look::Secondary).clicked() {
                        self.screen = Screen::Browser;
                    }
                });
            });
        });
    }

    /// One row of the first-run picker: click it to tick or untick the add-on.
    fn pick_row(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let installed = self.installed_version(&entry.id).is_some();
        let job = self
            .market
            .jobs
            .get(&entry.id)
            .map(|job| job.status.clone());
        let busy = job.as_ref().is_some_and(InstallStatus::busy);
        let selectable = !installed && !busy && market::can_install(entry);
        let on = self.market.selected.contains(&entry.id);
        let state = match &job {
            Some(status) if !installed => status.label(),
            _ if installed => "Installed".to_owned(),
            _ => market::size(entry.size),
        };
        let response = store::row(
            ui,
            entry,
            on,
            &state,
            installed && job.is_none(),
            selectable.then_some(on),
        );
        if response.clicked() {
            if installed {
                self.open_market_addon(&entry.id);
            } else if selectable {
                self.market.select(&entry.id, !on);
            }
        }
    }

    pub(super) fn marketplace_ui(&mut self, ui: &mut egui::Ui) {
        if self.market.review.is_some() {
            self.install_review(ui);
            return;
        }
        if self.show_install {
            if store::button(ui, "Back to add-ons", store::Look::Secondary).clicked() {
                self.show_install = false;
            }
            ui.add_space(12.0);
            self.browser(ui);
            return;
        }
        if self.market.details {
            if store::button(ui, "Back to add-ons", store::Look::Secondary).clicked() {
                self.market.details = false;
            }
            ui.add_space(12.0);
            if let Some(entry) = self.focused_entry() {
                egui::ScrollArea::vertical()
                    .id_salt("addon-full-detail")
                    .show(ui, |ui| self.catalog_detail(ui, &entry));
            }
            return;
        }
        self.catalog_notice(ui);
        let needle = self.search.to_lowercase();
        let mut entries = self.market.entries.clone();
        for entry in crate::installer::installed(&self.registry.root) {
            if !entries.iter().any(|available| available.id == entry.id) {
                entries.push(entry);
            }
        }
        entries.retain(|entry| {
            (self.market.category == "All" || self.market.category == entry.category)
                && (!self.market.installed_only || self.installed_version(&entry.id).is_some())
                && format!(
                    "{} {} {}",
                    entry.title, entry.short_description, entry.publisher
                )
                .to_lowercase()
                .contains(&needle)
        });
        entries.sort_by_key(|entry| !entry.verified);
        if self
            .market
            .focused
            .as_ref()
            .is_none_or(|id| !entries.iter().any(|entry| &entry.id == id))
        {
            self.market.focused = entries.first().map(|entry| entry.id.clone());
        }
        self.store_ui(ui, &entries);
    }

    pub(super) fn focused_entry(&self) -> Option<Entry> {
        let id = self.market.focused.as_ref()?;
        self.market
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .cloned()
            .or_else(|| crate::installer::installed_entry(&self.registry.root, id))
            .or_else(|| self.market.jobs.get(id).map(|job| job.entry.clone()))
    }

    fn catalog_detail(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let width = ui.available_width().min(720.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                store::page_title(ui, &entry.title, "");
                ui.add_space(-12.0);
                ui.label(
                    egui::RichText::new(store::meta_line(entry))
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(16.0);
                store::paragraph(ui, &entry.short_description, 14.0);
                ui.add_space(16.0);
                store::trust_card(ui, entry.verified);
                ui.add_space(16.0);
                self.detail_actions(ui, entry);
                // A download in progress, a failure with Try again, or a finished install.
                if self.market.jobs.contains_key(&entry.id) {
                    ui.add_space(12.0);
                    self.job_actions(ui, &entry.id);
                }
                ui.add_space(16.0);
                store::rule(ui);
                ui.add_space(14.0);
                for url in entry.screenshots.iter().take(8) {
                    self.market.image(url, ui.ctx());
                    match self.market.images.get(url) {
                        Some(Ok(bytes)) => {
                            ui.add(
                                egui::Image::from_bytes(format!("bytes://{url}"), bytes.clone())
                                    .fit_to_exact_size(Vec2::new(ui.available_width(), 220.0))
                                    .maintain_aspect_ratio(true)
                                    .corner_radius(8),
                            );
                        }
                        Some(Err(_)) => {
                            store::paragraph(ui, "This screenshot could not be loaded.", 13.0)
                        }
                        None => store::paragraph(ui, "Loading screenshot...", 13.0),
                    }
                    ui.add_space(10.0);
                }
                if entry.screenshots.is_empty() {
                    store::paragraph(ui, "No screenshots supplied by the publisher.", 13.0);
                    ui.add_space(10.0);
                }
                store::paragraph(
                    ui,
                    if entry.long_description.is_empty() {
                        &entry.short_description
                    } else {
                        &entry.long_description
                    },
                    14.0,
                );
                ui.add_space(16.0);
                store::rule(ui);
                ui.add_space(14.0);
                store::heading(ui, "It can");
                ui.add_space(10.0);
                store::access_lines(ui, entry);
                ui.add_space(6.0);
                store::heading(ui, "Works with");
                ui.add_space(6.0);
                store::paragraph(
                    ui,
                    &format!(
                        "Toolboks {} or newer. This build supports its add-on connection.",
                        entry.min_toolboks_version
                    ),
                    14.0,
                );
                ui.add_space(16.0);
                store::heading(ui, "What's new");
                ui.add_space(6.0);
                store::paragraph(
                    ui,
                    if entry.release_notes.is_empty() {
                        "No release notes supplied."
                    } else {
                        &entry.release_notes
                    },
                    14.0,
                );
            });
        });
    }

    /// The Install, Update, Open and Remove buttons with the version and size under them.
    fn detail_actions(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let installed = self.installed_version(&entry.id);
        let update = market::update_available(installed.as_deref(), entry);
        let accepted = self
            .market
            .entries
            .iter()
            .any(|available| available == entry);
        let busy = self
            .market
            .jobs
            .get(&entry.id)
            .is_some_and(|job| job.status.busy());
        anchors::region(ui, "browser-review", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                if installed.is_none() || update {
                    let enabled =
                        accepted && market::can_install(entry) && !busy && !self.market.loading;
                    let label = if update { "Update" } else { "Install" };
                    if ui
                        .add_enabled_ui(enabled, |ui| {
                            store::button(ui, label, store::Look::Primary)
                        })
                        .inner
                        .clicked()
                    {
                        self.market.review = Some(vec![entry.clone()]);
                    }
                } else if store::button(ui, "Open", store::Look::Primary).clicked() {
                    self.open_market_addon(&entry.id);
                }
                if installed.is_some() {
                    if update
                        && store::button(ui, "Open installed version", store::Look::Secondary)
                            .clicked()
                    {
                        self.open_market_addon(&entry.id);
                    }
                    if ui
                        .add_enabled_ui(!busy, |ui| {
                            store::button(ui, "Remove", store::Look::Danger)
                        })
                        .inner
                        .clicked()
                    {
                        self.market.remove = Some((entry.id.clone(), false));
                    }
                }
            });
        });
        ui.add_space(10.0);
        if let Some(version) = installed {
            store::paragraph(
                ui,
                &format!(
                    "Installed version {version}{}",
                    if update {
                        " \u{b7} Update available"
                    } else {
                        ""
                    }
                ),
                13.0,
            );
        }
        store::paragraph(
            ui,
            &format!(
                "{} download \u{b7} Version {}",
                market::size(entry.size),
                entry.version
            ),
            13.0,
        );
        store::paragraph(
            ui,
            "You will review its access before installing. Removing it can undo an install.",
            13.0,
        );
    }

    fn install_review(&mut self, ui: &mut egui::Ui) {
        let entries = self.market.review.clone().unwrap_or_default();
        let width = ui.available_width().min(760.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                store::page_title(
                    ui,
                    "Review installation",
                    "Only the add-ons below will be installed. Nothing has been downloaded yet.",
                );
                let list_height = (ui.available_height() - 190.0).max(120.0);
                store::card(ui, 16, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("grouped-install-review")
                        .max_height(list_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (index, entry) in entries.iter().enumerate() {
                                if index > 0 {
                                    ui.add_space(16.0);
                                    store::rule(ui);
                                    ui.add_space(16.0);
                                }
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 14.0;
                                    store::tile(ui, &entry.icon);
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 2.0);
                                        ui.horizontal(|ui| {
                                            store::strong(ui, &entry.title, 15.0);
                                            if entry.verified {
                                                store::verified_pill(ui);
                                            }
                                        });
                                        store::paragraph(
                                            ui,
                                            &format!(
                                                "{} \u{b7} Version {} \u{b7} Published by {}",
                                                market::size(entry.size),
                                                entry.version,
                                                entry.publisher
                                            ),
                                            13.0,
                                        );
                                    });
                                });
                                ui.add_space(14.0);
                                store::heading(ui, "It can");
                                ui.add_space(10.0);
                                store::access_lines(ui, entry);
                                if entry.kind == crate::catalog::Kind::Native {
                                    ui.add_space(6.0);
                                    store::paragraph(
                                        ui,
                                        "Open tools will close for the update.",
                                        13.0,
                                    );
                                }
                            }
                        });
                });
                ui.add_space(16.0);
                store::paragraph(
                    ui,
                    &format!(
                        "{} add-on{} \u{b7} {} total download",
                        entries.len(),
                        if entries.len() == 1 { "" } else { "s" },
                        market::size(entries.iter().map(|entry| entry.size).sum())
                    ),
                    13.0,
                );
                store::paragraph(
                    ui,
                    "You can remove an add-on later. Its saved data is kept unless you choose to delete it.",
                    13.0,
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let label = if entries.len() == 1 {
                        "Install add-on"
                    } else {
                        "Install these add-ons"
                    };
                    if store::button(ui, label, store::Look::Primary).clicked() {
                        for entry in &entries {
                            if let Some(supervisor) = &mut self.supervisor {
                                supervisor.stop(&entry.id, std::time::Instant::now());
                            }
                            self.views.remove(&entry.id);
                            // Updated or reinstalled: it comes back to the sidebar.
                            self.detached.remove(&entry.id);
                        }
                        self.market.review = None;
                        self.market.queue(entries.clone());
                        self.registry.config.first_run_complete = true;
                        self.persist();
                    }
                    if store::button(ui, "Cancel", store::Look::Secondary).clicked() {
                        self.market.review = None;
                    }
                });
            });
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        // A finished install is already marked "Installed" in the list above; only downloads in
        // progress and installs that failed or were cancelled need a block of their own.
        let ids: Vec<_> = self
            .market
            .jobs
            .iter()
            .filter(|(_, job)| job.status != InstallStatus::Ready)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            ui.add_space(8.0);
            store::heading(ui, &self.market.jobs[&id].entry.title.clone());
            ui.add_space(4.0);
            self.job_actions(ui, &id);
        }
    }

    fn job_actions(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(job) = self.market.jobs.get(id) else {
            return;
        };
        let entry = job.entry.clone();
        let status = job.status.clone();
        store::paragraph(ui, &status.label(), 13.0);
        if let InstallStatus::Failed(error) = &status {
            store::paragraph(ui, error, 13.0);
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
            if status.busy() {
                if store::button(ui, "Cancel download", store::Look::Secondary).clicked() {
                    self.market.cancel(id);
                }
            } else {
                if matches!(status, InstallStatus::Ready) {
                    if store::button(ui, "Open", store::Look::Secondary).clicked() {
                        self.open_market_addon(id);
                    }
                } else if self
                    .market
                    .entries
                    .iter()
                    .any(|available| available == &entry)
                    && store::button(ui, "Try again", store::Look::Secondary).clicked()
                {
                    self.market.review = Some(vec![entry]);
                }
                if store::button(ui, "Dismiss", store::Look::Secondary).clicked() {
                    self.market.dismiss(id);
                }
            }
        });
    }

    pub(super) fn open_market_addon(&mut self, id: &str) {
        // In a window of its own: that window comes to the front instead.
        if let Some(window) = self.detached.get_mut(id) {
            window.focus = true;
            return;
        }
        if !self.registry.enabled(id) {
            if let Err(error) = self.registry.set_enabled(id, true) {
                self.message = Some(error);
                return;
            }
        }
        if crate::installer::installed_entry(&self.registry.root, id)
            .is_some_and(|entry| entry.kind == crate::catalog::Kind::Declarative)
        {
            match crate::installer::launching(&self.registry.root, id) {
                Ok(crate::installer::Launch::RolledBack { .. }) => {
                    self.registry = Registry::load(self.registry.root.clone());
                    self.message = Some("The previous version is back because the update did not open successfully.".into());
                }
                Ok(_) => {}
                Err(error) => {
                    self.message = Some(error.to_string());
                    return;
                }
            }
            self.pending_declarative = Some(id.into());
        }
        self.screen = Screen::External(id.into());
        self.addon_page = 0;
    }

    pub(super) fn market_sidebar_status(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        title: &str,
        expanded: bool,
    ) {
        let status = self
            .market_status(id)
            .filter(|status| status != "Update available");
        if let Some(status) = status {
            if expanded {
                let pill = ui.horizontal(|ui| {
                    ui.add_space(44.0);
                    store::status_pill(ui, id, title, &status)
                });
                if pill.inner.clicked() {
                    self.screen = Screen::Browser;
                    self.market.focused = Some(id.into());
                    self.market.details = true;
                }
            } else {
                let center = egui::pos2(ui.max_rect().right() - 3.0, ui.cursor().top() - 12.0);
                if store::status_dot(ui, id, title, &status, center).clicked() {
                    self.screen = Screen::Browser;
                    self.market.focused = Some(id.into());
                    self.market.details = true;
                }
            }
        }
    }

    /// The listed add-ons that have an update waiting, as catalog entries ready for the review.
    pub(super) fn pending_updates(&self) -> Vec<Entry> {
        market::pending_updates(&self.market.entries, |id| self.installed_version(id))
    }

    /// Opens the normal access review for these updates in the Add-ons browser.
    pub(super) fn review_updates(&mut self, updates: Vec<Entry>) {
        if updates.is_empty() || self.market.busy() {
            return;
        }
        self.market.review = Some(updates);
        self.screen = Screen::Browser;
    }

    /// What the sidebar row of an add-on says at its right end: an update first, else "New".
    pub(super) fn addon_tag(&self, id: &str) -> Tag {
        if self.market_status(id).as_deref() == Some("Update available") {
            Tag::Update
        } else if self.registry.config.new_addons.iter().any(|n| n == id) {
            Tag::New
        } else {
            Tag::None
        }
    }

    pub(super) fn market_status(&self, id: &str) -> Option<String> {
        if let Some(job) = self.market.jobs.get(id) {
            if job.status != InstallStatus::Ready {
                return Some(job.status.label());
            }
        }
        let version = self.installed_version(id)?;
        self.market
            .entries
            .iter()
            .any(|entry| entry.id == id && market::update_available(Some(&version), entry))
            .then(|| "Update available".into())
    }

    pub(super) fn remote_workspace(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        page: &mut usize,
    ) -> bool {
        let Some(entry) = crate::installer::installed_entry(&self.registry.root, id)
            .filter(|entry| entry.kind == crate::catalog::Kind::Native)
        else {
            return false;
        };
        if self
            .market
            .remove
            .as_ref()
            .is_some_and(|(removing, _)| removing == id)
        {
            muted(ui, "Close the add-on before removing it. Your saved data stays unless you choose to delete it.");
            return true;
        }
        let Some(supervisor) = &mut self.supervisor else {
            muted(ui, "The add-on could not start. Close and reopen Toolboks.");
            return true;
        };
        supervisor.open(
            crate::addon_services::launch_spec(&self.registry.root, &entry),
            std::time::Instant::now(),
        );
        if let Some(view) = self.views.get_mut(id) {
            #[cfg(feature = "preview")]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::unique("disk-pilot-origin"),
                    ui.available_rect_before_wrap().min,
                )
            });
            view.set_interactive(self.tour.is_none());
            view.show(ui, ui.available_rect_before_wrap());
            // The add-on opened one of its own pages (Resource Monitor's "Open in history"):
            // the sidebar and the header show it too.
            if let Some(own) = view.take_own_page() {
                if (own as usize) < entry.pages.len() && own as usize != *page {
                    *page = own as usize;
                    ui.ctx().request_repaint();
                }
            }
        } else {
            muted(ui, "Opening add-on...");
        }
        if let Some(crate::supervisor::Status::Stopped(reason)) = supervisor.status(id) {
            muted(ui, reason.text());
            if ui.button("Restart add-on").clicked() {
                supervisor.restart(id, std::time::Instant::now());
            }
        }
        true
    }

    pub(super) fn market_remove(&mut self, ctx: &egui::Context) {
        let Some((id, mut delete_data)) = self.market.remove.clone() else {
            return;
        };
        let name = crate::installer::installed_entry(&self.registry.root, &id)
            .map_or_else(|| id.clone(), |entry| entry.title);
        let frame = egui::Frame::new()
            .fill(crate::ui::WINDOW_BG)
            .stroke(egui::Stroke::new(1.0, crate::ui::BORDER))
            .corner_radius(12)
            .inner_margin(24);
        let response = egui::Modal::new(egui::Id::unique("market-remove"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(440.0_f32.min(ctx.content_rect().width() - 96.0));
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                store::page_title(
                    ui,
                    &format!("Remove {name}?"),
                    "The add-on will leave your sidebar. Its saved data stays on this PC by default, ready if you install it again.",
                );
                ui.checkbox(
                    &mut delete_data,
                    egui::RichText::new("Also delete its data").size(14.0).color(TEXT),
                );
                if delete_data {
                    ui.add_space(8.0);
                    store::paragraph(
                        ui,
                        "Saved settings and data will be deleted. This cannot be undone.",
                        13.0,
                    );
                }
                ui.add_space(20.0);
                let running = self
                    .supervisor
                    .as_ref()
                    .is_some_and(|supervisor| supervisor.is_running(&id));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if running {
                        if store::button(ui, "Close add-on before removing", store::Look::Secondary)
                            .clicked()
                        {
                            self.supervisor
                                .as_mut()
                                .unwrap()
                                .stop(&id, std::time::Instant::now());
                        }
                    } else if store::button(ui, "Remove add-on", store::Look::Danger).clicked() {
                        let result = if self
                            .registry
                            .packages
                            .iter()
                            .any(|package| package.manifest.id == id)
                        {
                            self.registry.remove(&id, !delete_data)
                        } else {
                            crate::installer::remove(&self.registry.root, &id, !delete_data)
                                .map_err(String::from)
                                .and_then(|()| {
                                    if delete_data {
                                        self.registry.config.preferences.remove(&id);
                                    }
                                    self.registry.save()
                                })
                        };
                        match result {
                            Ok(()) => {
                                if let Some(supervisor) = &mut self.supervisor {
                                    supervisor.forget(&id);
                                }
                                self.services.health.remove(&id);
                                self.views.remove(&id);
                                self.detached.remove(&id);
                                self.market.dismiss(&id);
                                self.message =
                                    Some("Add-on removed. You can install it again from Add-ons.".into());
                                self.screen = Screen::Browser;
                            }
                            Err(error) => {
                                self.message = Some(format!("Could not remove the add-on: {error}"))
                            }
                        }
                        self.market.remove = None;
                        return;
                    }
                    if store::button(ui, "Keep add-on", store::Look::Secondary).clicked() {
                        self.market.remove = None;
                    }
                });
            });
        if response.should_close() {
            self.market.remove = None;
        } else if self.market.remove.is_some() {
            self.market.remove = Some((id, delete_data));
        }
    }
}
