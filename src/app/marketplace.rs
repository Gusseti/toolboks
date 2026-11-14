use super::shell::{caption, muted, panel, primary, section, Screen};
use super::*;
use crate::catalog::Entry;
use crate::marketplace::{self as market, InstallStatus};
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

    pub(super) fn poll_marketplace(&mut self, ctx: &egui::Context) {
        if !self.market.checked {
            self.market.refresh(self.registry.root.clone(), ctx);
        }
        let running = crate::installer::installed(&self.registry.root)
            .into_iter()
            .filter(|entry| {
                self.supervisor
                    .as_ref()
                    .is_some_and(|supervisor| supervisor.is_running(&entry.id))
            })
            .map(|entry| entry.id)
            .collect();
        if self.market.poll(&self.registry.root, ctx, &running) {
            let config = self.registry.config.clone();
            self.registry = Registry::load(self.registry.root.clone());
            self.registry.config = config;
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
            if let Some(crate::addon_services::Effect::SaveConfig) = self.services.apply(
                &event,
                supervisor,
                &self.registry.root,
                &mut self.registry.config,
            ) {
                self.preferences_dirty = Some(std::time::Instant::now());
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
                    if self.screen == Screen::External(id.clone()) {
                        supervisor.send(
                            &id,
                            &toolboks_addon_proto::HostMessage::Navigate {
                                page: self.addon_page.to_string(),
                            },
                        );
                    }
                }
                crate::supervisor::Event::Stopped { id, reason } => {
                    if let Some(view) = self.views.get_mut(&id) {
                        view.stop(reason.text());
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
            if self.screen != Screen::External(entry.id.clone()) || self.hidden {
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
        if !self.views.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn finish_picker(&mut self) {
        self.registry.config.first_run_complete = true;
        self.persist();
        self.screen = Screen::Home;
    }

    fn catalog_notice(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "preview")]
        if std::env::var_os("TOOLBOKS_TEST_CATALOG").is_some() {
            caption(
                ui,
                "Local test catalog. These are SDK samples, not published tools.",
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
        let width = ui.available_width().min(720.0);
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.vertical(|ui| {
                ui.set_width(width);
                Self::title(
                    ui,
                    "Pick your tools",
                    "Add the tools you need. You can add or remove them later.",
                );
                self.catalog_notice(ui);
                let mut entries: Vec<_> = self
                    .market
                    .entries
                    .iter()
                    .filter(|entry| entry.verified && entry.publisher == "Toolboks")
                    .cloned()
                    .collect();
                entries.sort_by_key(market::pick_rank);
                caption(ui, "From the Toolboks team. No tools selected by default.");
                anchors::region(ui, "browser-list", |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("tool-picker")
                        .max_height((ui.available_height() - 130.0).max(60.0))
                        .show(ui, |ui| {
                            for entry in entries {
                                let installed = self.installed_version(&entry.id).is_some();
                                let busy = self
                                    .market
                                    .jobs
                                    .get(&entry.id)
                                    .is_some_and(|job| job.status.busy());
                                ui.separator();
                                ui.horizontal_top(|ui| {
                                    let mut selected = self.market.selected.contains(&entry.id);
                                    let response = ui
                                        .add_enabled(
                                            !installed && !busy && market::can_install(&entry),
                                            egui::Checkbox::new(&mut selected, ""),
                                        )
                                        .on_hover_text(format!("Select {}", entry.title));
                                    response.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::Role::CheckBox,
                                            !installed && !busy,
                                            selected,
                                            &entry.title,
                                        )
                                    });
                                    if response.changed() {
                                        self.market.select(&entry.id, selected);
                                    }
                                    ui.vertical(|ui| {
                                        ui.set_width((width - 145.0).max(130.0));
                                        ui.strong(&entry.title);
                                        muted(ui, &entry.short_description);
                                        if let Some(job) = self.market.jobs.get(&entry.id) {
                                            caption(ui, job.status.label());
                                        }
                                    });
                                    ui.vertical(|ui| {
                                        caption(ui, market::size(entry.size));
                                        if installed && ui.button("Open").clicked() {
                                            self.open_market_addon(&entry.id);
                                        }
                                    });
                                });
                            }
                            self.job_list(ui);
                        });
                });
                ui.separator();
                let selected = self.market.selected_entries();
                caption(
                    ui,
                    format!(
                        "{} selected - {} download",
                        selected.len(),
                        market::size(selected.iter().map(|entry| entry.size).sum())
                    ),
                );
                muted(ui, "Review what they ask to access before installing.");
                ui.horizontal(|ui| {
                    if ui.button("Skip").clicked() {
                        self.finish_picker();
                    }
                    if ui
                        .add_enabled_ui(!selected.is_empty() && !self.market.loading, |ui| {
                            primary(ui, "Install selected")
                        })
                        .inner
                        .clicked()
                    {
                        self.market.review = Some(selected);
                    }
                    if ui.button("Browse all add-ons").clicked() {
                        self.screen = Screen::Browser;
                    }
                });
            });
        });
    }

    pub(super) fn marketplace_ui(&mut self, ui: &mut egui::Ui) {
        if self.market.review.is_some() {
            self.install_review(ui);
            return;
        }
        if self.show_install {
            if ui.button("Back to add-ons").clicked() {
                self.show_install = false;
            }
            self.browser(ui);
            return;
        }
        if self.market.details {
            if ui.button("Back to add-ons").clicked() {
                self.market.details = false;
            }
            if let Some(entry) = self.focused_entry() {
                egui::ScrollArea::vertical()
                    .id_salt("addon-full-detail")
                    .show(ui, |ui| self.catalog_detail(ui, &entry, true));
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

    fn catalog_detail(&mut self, ui: &mut egui::Ui, entry: &Entry, full: bool) {
        Self::title(ui, &entry.title, &entry.short_description);
        caption(
            ui,
            format!(
                "Published by {}{}",
                entry.publisher,
                if entry.verified {
                    " - Toolboks verified"
                } else {
                    ""
                }
            ),
        );
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
                if installed.is_none() || update {
                    if ui
                        .add_enabled_ui(
                            accepted && market::can_install(entry) && !busy && !self.market.loading,
                            |ui| primary(ui, if update { "Update" } else { "Install" }),
                        )
                        .inner
                        .clicked()
                    {
                        self.market.review = Some(vec![entry.clone()]);
                    }
                } else if primary(ui, "Open").clicked() {
                    self.open_market_addon(&entry.id);
                }
                if installed.is_some() {
                    if update && ui.button("Open installed version").clicked() {
                        self.open_market_addon(&entry.id);
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                        self.market.remove = Some((entry.id.clone(), false));
                    }
                }
                if !full && ui.button("View details").clicked() {
                    self.market.details = true;
                }
            });
        });
        if let Some(version) = installed {
            caption(
                ui,
                format!(
                    "Installed version {version}{}",
                    if update { " - Update available" } else { "" }
                ),
            );
        }
        caption(
            ui,
            format!(
                "{} download - Version {}",
                market::size(entry.size),
                entry.version
            ),
        );
        caption(
            ui,
            "You will review its access before installing. Removing it can undo an install.",
        );
        ui.add_space(12.0);
        if entry.screenshots.is_empty() {
            muted(ui, "No screenshots supplied by the publisher.");
        }
        for url in entry.screenshots.iter().take(if full { 8 } else { 1 }) {
            self.market.image(url, ui.ctx());
            match self.market.images.get(url) {
                Some(Ok(bytes)) => {
                    ui.add(
                        egui::Image::from_bytes(format!("bytes://{url}"), bytes.clone())
                            .fit_to_exact_size(Vec2::new(ui.available_width(), 200.0))
                            .maintain_aspect_ratio(true),
                    );
                }
                Some(Err(_)) => muted(ui, "This screenshot could not be loaded."),
                None => muted(ui, "Loading screenshot..."),
            }
        }
        ui.add_space(12.0);
        muted(
            ui,
            if entry.long_description.is_empty() {
                &entry.short_description
            } else {
                &entry.long_description
            },
        );
        ui.add_space(12.0);
        section(ui, "What it asks to access");
        if entry.capabilities.is_empty() {
            muted(ui, "No extra host access requested.");
        }
        for capability in &entry.capabilities {
            muted(ui, market::access(capability));
        }
        if entry.kind == crate::catalog::Kind::Native {
            muted(
                ui,
                "This add-on runs a program on your PC. Its access list is not a security sandbox.",
            );
        }
        ui.add_space(12.0);
        section(ui, "Works with");
        muted(
            ui,
            format!(
                "Toolboks {} or newer. This build supports its add-on connection.",
                entry.min_toolboks_version
            ),
        );
        ui.add_space(12.0);
        section(ui, "What's new");
        muted(
            ui,
            if entry.release_notes.is_empty() {
                "No release notes supplied."
            } else {
                &entry.release_notes
            },
        );
        self.job_actions(ui, &entry.id);
    }

    fn install_review(&mut self, ui: &mut egui::Ui) {
        let entries = self.market.review.clone().unwrap_or_default();
        Self::title(
            ui,
            "Review installation",
            "Only the add-ons below will be installed. Nothing has been downloaded yet.",
        );
        egui::ScrollArea::vertical().id_salt("grouped-install-review").max_height((ui.available_height() - 95.0).max(100.0)).show(ui, |ui| {
            for entry in &entries {
                ui.separator();
                section(ui, &entry.title);
                caption(ui, format!("{} - Version {} - Published by {}", market::size(entry.size), entry.version, entry.publisher));
                for capability in &entry.capabilities { muted(ui, market::access(capability)); }
                if entry.capabilities.is_empty() { muted(ui, "No extra host access requested."); }
                if entry.kind == crate::catalog::Kind::Native { muted(ui, "Runs a program on your PC, not in a security sandbox. Open tools will close for the update."); }
            }
        });
        ui.separator();
        caption(
            ui,
            format!(
                "{} add-ons - {} total download",
                entries.len(),
                market::size(entries.iter().map(|entry| entry.size).sum())
            ),
        );
        muted(ui, "You can remove an add-on later. Its saved data is kept unless you choose to delete it.");
        ui.horizontal(|ui| {
            if primary(
                ui,
                if entries.len() == 1 {
                    "Install add-on"
                } else {
                    "Install these add-ons"
                },
            )
            .clicked()
            {
                for entry in &entries {
                    if let Some(supervisor) = &mut self.supervisor {
                        supervisor.stop(&entry.id, std::time::Instant::now());
                    }
                    self.views.remove(&entry.id);
                }
                self.market.review = None;
                self.market.queue(entries);
                self.registry.config.first_run_complete = true;
                self.persist();
            }
            if ui.button("Cancel").clicked() {
                self.market.review = None;
            }
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        let ids: Vec<_> = self.market.jobs.keys().cloned().collect();
        for id in ids {
            section(ui, &self.market.jobs[&id].entry.title.clone());
            self.job_actions(ui, &id);
        }
    }

    fn job_actions(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(job) = self.market.jobs.get(id) else {
            return;
        };
        let entry = job.entry.clone();
        let status = job.status.clone();
        caption(ui, status.label());
        if let InstallStatus::Failed(error) = &status {
            muted(ui, error);
        }
        ui.horizontal_wrapped(|ui| {
            if status.busy() {
                if ui.button("Cancel download").clicked() {
                    self.market.cancel(id);
                }
            } else {
                if matches!(status, InstallStatus::Ready) {
                    if ui.button("Open").clicked() {
                        self.open_market_addon(id);
                    }
                } else if self
                    .market
                    .entries
                    .iter()
                    .any(|available| available == &entry)
                    && ui.button("Try again").clicked()
                {
                    self.market.review = Some(vec![entry]);
                }
                if ui.button("Dismiss").clicked() {
                    self.market.dismiss(id);
                }
            }
        });
    }

    pub(super) fn open_market_addon(&mut self, id: &str) {
        if !self.registry.enabled(id) {
            if let Err(error) = self.registry.set_enabled(id, true) {
                self.message = Some(error);
                return;
            }
        }
        self.screen = Screen::External(id.into());
        self.addon_page = 0;
    }

    pub(super) fn market_sidebar(&mut self, ui: &mut egui::Ui, expanded: bool) {
        let mut entries = crate::installer::installed(&self.registry.root);
        for job in self.market.jobs.values() {
            if !entries.iter().any(|entry| entry.id == job.entry.id) {
                entries.push(job.entry.clone());
            }
        }
        for entry in entries {
            if self
                .registry
                .packages
                .iter()
                .any(|package| package.manifest.id == entry.id)
            {
                continue;
            }
            let status = self
                .market_status(&entry.id)
                .unwrap_or_else(|| "Installed".into());
            let response = super::shell::nav_row(
                ui,
                &entry.title,
                &entry.icon,
                self.screen == Screen::External(entry.id.clone()),
                expanded,
            );
            if response
                .on_hover_text(format!("{} - {status}", entry.title))
                .clicked()
            {
                if self.installed_version(&entry.id).is_some() {
                    self.open_market_addon(&entry.id);
                } else {
                    self.screen = Screen::Browser;
                    self.market.focused = Some(entry.id.clone());
                    self.market.details = true;
                }
            }
            if expanded {
                caption(ui, status);
            }
            if expanded && self.screen == Screen::External(entry.id.clone()) {
                for (page, title) in entry.pages.iter().enumerate() {
                    if super::shell::page_row(ui, title, self.addon_page == page) {
                        self.addon_page = page;
                        if let Some(supervisor) = &self.supervisor {
                            supervisor.send(
                                &entry.id,
                                &toolboks_addon_proto::HostMessage::Navigate {
                                    page: page.to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    pub(super) fn market_sidebar_status(&mut self, ui: &mut egui::Ui, id: &str, expanded: bool) {
        let status = self.market_status(id);
        if let Some(status) = status {
            if expanded {
                if ui.small_button(&status).clicked() {
                    self.screen = Screen::Browser;
                    self.market.focused = Some(id.into());
                    self.market.details = true;
                }
            } else {
                let center = egui::pos2(ui.max_rect().right() - 3.0, ui.cursor().top() - 12.0);
                ui.painter().circle_filled(center, 3.0, look::DATA_MEMORY);
                let response = ui
                    .interact(
                        egui::Rect::from_center_size(center, Vec2::splat(10.0)),
                        egui::Id::unique(("addon-status", id)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(&status);
                if response.clicked() {
                    self.screen = Screen::Browser;
                    self.market.focused = Some(id.into());
                    self.market.details = true;
                }
            }
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

    pub(super) fn remote_workspace(&mut self, ui: &mut egui::Ui, id: &str) -> bool {
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
            view.set_interactive(self.tour.is_none());
            view.show(ui, ui.available_rect_before_wrap());
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
        let response = egui::Modal::new(egui::Id::unique("market-remove")).frame(panel()).show(ctx, |ui| {
            ui.set_width(420.0_f32.min(ctx.content_rect().width() - 64.0));
            Self::title(ui, &format!("Remove {name}?"), "The add-on will leave your sidebar. Its saved data stays on this PC by default, ready if you install it again.");
            ui.checkbox(&mut delete_data, "Also delete its data");
            if delete_data { muted(ui, "Saved settings and data will be deleted. This cannot be undone."); }
            let running = self.supervisor.as_ref().is_some_and(|supervisor| supervisor.is_running(&id));
            if running {
                if ui.button("Close add-on before removing").clicked() { self.supervisor.as_mut().unwrap().stop(&id, std::time::Instant::now()); }
            } else if ui.button("Remove add-on").clicked() {
                let result = if self.registry.packages.iter().any(|package| package.manifest.id == id) {
                    self.registry.remove(&id, !delete_data)
                } else {
                    crate::installer::remove(&self.registry.root, &id, !delete_data).map_err(String::from).and_then(|()| {
                        if delete_data { self.registry.config.preferences.remove(&id); }
                        self.registry.save()
                    })
                };
                match result {
                    Ok(()) => {
                        if let Some(supervisor) = &mut self.supervisor { supervisor.forget(&id); }
                        self.views.remove(&id);
                        self.market.dismiss(&id);
                        self.message = Some("Add-on removed. You can install it again from Add-ons.".into());
                        self.screen = Screen::Browser;
                    }
                    Err(error) => self.message = Some(format!("Could not remove the add-on: {error}")),
                }
                self.market.remove = None;
                return;
            }
            if ui.button("Keep add-on").clicked() { self.market.remove = None; }
        });
        if response.should_close() {
            self.market.remove = None;
        } else if self.market.remove.is_some() {
            self.market.remove = Some((id, delete_data));
        }
    }
}
