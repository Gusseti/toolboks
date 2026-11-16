//! Host services for out-of-process add-ons (#87): what a built-in tool gets from the host by
//! calling it, an add-on gets through protocol messages. This module holds the host's side.
//!
//! - **Status bar items.** The add-on sends its items; they are shown while it runs and is
//!   turned on, and disappear when it stops or is turned off.
//! - **Links.** `--open "Title"` or `--open "Title/Page"` resolves against the pages the add-on
//!   declares in its catalog entry ([`resolve_link`]).
//! - **Guided tours.** The add-on sends its steps and, for the step being shown, the rectangle
//!   of the anchor; the host draws the overlay and its buttons. While a tour runs the page
//!   forwards no input to the add-on, so a tour can navigate but never run an action.
//! - **Tray.** The add-on may register entries; the host shows them and sends back the one the
//!   user chose.
//! - **Background add-ons.** An entry marked `background` is started at launch while it is
//!   turned on ([`background`]).
//! - **Preferences.** The add-on gets its namespaced preferences after it connects and sends
//!   changes back; the host writes them to `config.json`.
//! - **Health findings** (protocol 4). The add-on sends its checks for Home; they are handed
//!   to [`Services::on_findings`]. The supervisor drops them on a protocol 3 connection.
//!
//! Everything is optional for an add-on. What an add-on sends is cut to fixed limits before
//! the host keeps or shows it.

// The add-on pages call into this from the pilot (#88) on; the tests drive it today.
#![allow(dead_code)]

use crate::addons::Config;
use crate::installer;
use crate::supervisor::{Event, LaunchSpec, Supervisor};
use eframe::egui::{Pos2, Rect};
use std::collections::BTreeMap;
use std::path::Path;
use toolboks_addon_proto::{
    AddonMessage, Finding, HostMessage, HostRequest, StatusItem, TrayItem, TutorialStep,
    MAX_FINDINGS,
};
use toolboks_catalog::{Entry, Kind};

/// Capabilities that let an add-on request a separate administrator helper.
pub const ELEVATING_CAPABILITIES: [&str; 3] = ["administrator", "drive_optimize", "system_tuning"];

const MAX_STATUS_ITEMS: usize = 8;
const MAX_STATUS_CHARS: usize = 48;
const MAX_TOOLTIP_CHARS: usize = 200;
const MAX_TRAY_ITEMS: usize = 8;
const MAX_TRAY_CHARS: usize = 60;
const MAX_ID_CHARS: usize = 64;
const MAX_PREFERENCES_BYTES: usize = 256 * 1024;
/// The same limits as the SDK validator applies to a declarative package's tour.
const MAX_TOUR_STEPS: usize = 300;
const MAX_STEP_TITLE_BYTES: usize = 120;
const MAX_STEP_TEXT_BYTES: usize = 4000;

/// Cuts text from an add-on to `max` characters on one line.
fn clip(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// What the supervisor starts for an installed native add-on.
pub fn launch_spec(root: &Path, entry: &Entry) -> LaunchSpec {
    LaunchSpec {
        id: entry.id.clone(),
        name: entry.title.clone(),
        // Read from the installer's record before every start (`installed_root`).
        exe: Default::default(),
        sha256: String::new(),
        may_elevate: entry
            .capabilities
            .iter()
            .any(|name| ELEVATING_CAPABILITIES.contains(&name.as_str())),
        background: entry.background,
        installed_root: Some(root.to_path_buf()),
        // The host speaks the installed entry's protocol, so older add-ons keep working.
        protocol_version: entry.protocol_version,
    }
}

/// The installed native add-ons to start at launch: marked `background` and turned on.
pub fn background(
    installed: &[Entry],
    root: &Path,
    enabled: impl Fn(&str) -> bool,
) -> Vec<LaunchSpec> {
    installed
        .iter()
        .filter(|entry| entry.kind == Kind::Native && entry.background && enabled(&entry.id))
        .map(|entry| launch_spec(root, entry))
        .collect()
}

/// Where a link such as `--open "Disk Manager/Map"` leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Open this add-on, on this page when one was named and exists.
    Open {
        id: String,
        page: Option<String>,
    },
    /// The add-on is installed but turned off: show the overview with a short message.
    TurnedOff {
        id: String,
        title: String,
    },
    /// The add-on is in the catalog but not installed: offer to install it.
    NotInstalled {
        id: String,
        title: String,
    },
    Unknown,
}

/// Resolves `"Title"` or `"Title/Page"` (case-insensitive) against the installed add-ons, then
/// against the catalog. A page that does not exist opens the add-on's first page.
pub fn resolve_link(
    target: &str,
    installed: &[Entry],
    enabled: impl Fn(&str) -> bool,
    catalog: &[&Entry],
) -> Link {
    let (title, page) = match target.split_once('/') {
        Some((title, page)) => (title.trim(), Some(page.trim())),
        None => (target.trim(), None),
    };
    let named = |entry: &&Entry| entry.title.eq_ignore_ascii_case(title);
    if let Some(entry) = installed.iter().find(named) {
        if !enabled(&entry.id) {
            return Link::TurnedOff {
                id: entry.id.clone(),
                title: entry.title.clone(),
            };
        }
        let page = page.and_then(|wanted| {
            entry
                .pages
                .iter()
                .find(|page| page.eq_ignore_ascii_case(wanted))
                .cloned()
        });
        return Link::Open {
            id: entry.id.clone(),
            page,
        };
    }
    match catalog.iter().copied().find(named) {
        Some(entry) => Link::NotInstalled {
            id: entry.id.clone(),
            title: entry.title.clone(),
        },
        None => Link::Unknown,
    }
}

/// A tour the add-on described.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tour {
    pub steps: Vec<TutorialStep>,
    /// Where the add-on was when the tour was asked for (its route string).
    pub location: String,
}

#[derive(Default)]
struct State {
    status: Vec<StatusItem>,
    tray: Vec<TrayItem>,
    /// A tour was started and not finished: the page forwards no input meanwhile.
    touring: bool,
    tour: Option<Tour>,
    /// The anchor the host asked for.
    target: Option<String>,
    /// Its rectangle in the add-on's view, once the add-on reported it.
    anchor: Option<Rect>,
}

/// Something the caller has to do after [`Services::apply`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Preferences changed in `config`; write `config.json`.
    SaveConfig,
    /// The add-on's tour arrived; see [`Services::tour`].
    TourReady { id: String },
    /// A request the host handles itself (the package review).
    Request { id: String, request: HostRequest },
}

/// The host's record of what each running add-on has registered.
#[derive(Default)]
pub struct Services {
    addons: BTreeMap<String, State>,
}

impl Services {
    /// Applies one supervisor event. `root` is `addons::data_root()`.
    pub fn apply(
        &mut self,
        event: &Event,
        supervisor: &Supervisor,
        root: &Path,
        config: &mut Config,
    ) -> Option<Effect> {
        match event {
            Event::Connected { id } => {
                // A fresh process knows nothing: start from an empty record.
                self.addons.insert(id.clone(), State::default());
                if let Some(entry) = installer::installed_entry(root, id) {
                    supervisor.send(id, &HostMessage::Capabilities(entry.capabilities));
                }
                let preferences = config.preferences.get(id).cloned().unwrap_or_default();
                if let Ok(json) = serde_json::to_string(&preferences) {
                    supervisor.send(id, &HostMessage::LoadPreferences { json });
                }
                None
            }
            // Not running: its status items, tray entries and tour go away.
            Event::Restarting { id } | Event::Stopped { id, .. } => {
                self.addons.remove(id);
                None
            }
            Event::Stalled { .. } => None,
            Event::Message { id, message } => self.message(id, message, config),
        }
    }

    fn message(&mut self, id: &str, message: &AddonMessage, config: &mut Config) -> Option<Effect> {
        let state = self.addons.entry(id.to_string()).or_default();
        match message {
            AddonMessage::StatusItems(items) => {
                state.status = items
                    .iter()
                    .take(MAX_STATUS_ITEMS)
                    .map(|item| StatusItem {
                        text: clip(&item.text, MAX_STATUS_CHARS),
                        tooltip: item
                            .tooltip
                            .as_deref()
                            .map(|tooltip| clip(tooltip, MAX_TOOLTIP_CHARS)),
                    })
                    .filter(|item| !item.text.is_empty())
                    .collect();
                None
            }
            AddonMessage::TrayItems(items) => {
                state.tray = items
                    .iter()
                    .take(MAX_TRAY_ITEMS)
                    .filter(|item| item.id.chars().count() <= MAX_ID_CHARS)
                    .map(|item| TrayItem {
                        id: item.id.clone(),
                        label: clip(&item.label, MAX_TRAY_CHARS),
                        enabled: item.enabled,
                    })
                    .filter(|item| !item.label.is_empty())
                    .collect();
                None
            }
            AddonMessage::Preferences { json, changed } => {
                if !*changed || json.len() > MAX_PREFERENCES_BYTES {
                    return None;
                }
                // Only a JSON object is a set of preferences; anything else is ignored.
                let preferences: BTreeMap<String, serde_json::Value> =
                    serde_json::from_str(json).ok()?;
                if config.preferences.get(id) == Some(&preferences) {
                    return None;
                }
                // Namespaced: an add-on only ever writes under its own id.
                config.preferences.insert(id.to_string(), preferences);
                Some(Effect::SaveConfig)
            }
            AddonMessage::Tutorial { steps, location } => {
                let steps = steps
                    .iter()
                    .take(MAX_TOUR_STEPS)
                    .filter(|step| {
                        step.title.len() <= MAX_STEP_TITLE_BYTES
                            && step.description.len() <= MAX_STEP_TEXT_BYTES
                    })
                    .cloned()
                    .collect();
                state.tour = Some(Tour {
                    steps,
                    location: clip(location, MAX_TOOLTIP_CHARS),
                });
                Some(Effect::TourReady { id: id.to_string() })
            }
            AddonMessage::TutorialAnchor { target, rect } => {
                if state.target.as_deref() == Some(target.as_str()) {
                    state.anchor = rect.filter(|rect| rect.is_finite() && rect.is_positive());
                }
                None
            }
            AddonMessage::HostRequest(request) => Some(Effect::Request {
                id: id.to_string(),
                request: request.clone(),
            }),
            AddonMessage::Findings(findings) => {
                // Only the first ones are ever kept; the rest are not even copied.
                let findings = findings.iter().take(MAX_FINDINGS).cloned().collect();
                self.on_findings(id, findings);
                None
            }
            _ => None,
        }
    }

    /// The add-on `id` sent its full set of health findings (protocol 4 only: the supervisor
    /// drops them on a protocol 3 connection). `id` is the connection's own id, never a field
    /// of the message.
    ///
    /// TODO(#181): validate them and keep them in memory only in `health::Store` (cut to the
    /// limits in `toolboks_addon_proto`, routes checked against the entry's `pages`). Until
    /// then nothing is kept.
    pub fn on_findings(&mut self, id: &str, findings: Vec<Finding>) {
        let _ = (id, findings);
    }

    /// Status bar items of the running add-ons that are turned on, in add-on id order.
    pub fn status_items(&self, enabled: impl Fn(&str) -> bool) -> Vec<StatusItem> {
        self.addons
            .iter()
            .filter(|(id, _)| enabled(id))
            .flat_map(|(_, state)| state.status.iter().cloned())
            .collect()
    }

    /// Tray entries of the running add-ons that are turned on, with the add-on's id.
    pub fn tray_items(&self, enabled: impl Fn(&str) -> bool) -> Vec<(String, TrayItem)> {
        self.addons
            .iter()
            .filter(|(id, _)| enabled(id))
            .flat_map(|(id, state)| state.tray.iter().map(|item| (id.clone(), item.clone())))
            .collect()
    }

    /// The user chose a tray entry. Only an entry the add-on registered and enabled is sent.
    pub fn tray_action(&self, supervisor: &Supervisor, addon: &str, item: &str) -> bool {
        let offered = self.addons.get(addon).is_some_and(|state| {
            state
                .tray
                .iter()
                .any(|entry| entry.id == item && entry.enabled)
        });
        offered && supervisor.send(addon, &HostMessage::TrayAction { id: item.into() })
    }

    /// Asks the add-on for its preferences (a background add-on may change them without its
    /// page ever being shown). A change comes back as [`Effect::SaveConfig`].
    pub fn save_preferences(&self, supervisor: &Supervisor, id: &str) -> bool {
        supervisor.send(id, &HostMessage::SavePreferences)
    }

    /// Starts a tour: asks the add-on for its steps and tells it to save its UI state. The
    /// steps arrive as [`Effect::TourReady`]. Returns false when the add-on is not running.
    pub fn start_tour(&mut self, supervisor: &Supervisor, id: &str) -> bool {
        let Some(state) = self.addons.get_mut(id) else {
            return false;
        };
        if !supervisor.send(id, &HostMessage::TutorialQuery) {
            return false;
        }
        supervisor.send(id, &HostMessage::TutorialStart);
        state.touring = true;
        state.tour = None;
        true
    }

    /// A tour of this add-on is running: the page must not forward input to it
    /// (`AddonView::set_interactive(false)`), so the tour cannot run an action.
    pub fn touring(&self, id: &str) -> bool {
        self.addons.get(id).is_some_and(|state| state.touring)
    }

    pub fn tour(&self, id: &str) -> Option<&Tour> {
        self.addons.get(id)?.tour.as_ref()
    }

    /// Shows step `index` of the tour: the add-on navigates to the step's page (navigation
    /// only) and reports the anchor's rectangle, scrolled into view.
    pub fn show_step(&mut self, supervisor: &Supervisor, id: &str, index: usize) -> bool {
        let Some(state) = self.addons.get_mut(id) else {
            return false;
        };
        let Some(step) = state.tour.as_ref().and_then(|tour| tour.steps.get(index)) else {
            return false;
        };
        let (page, target) = (step.page.clone(), step.target.clone());
        state.target = Some(target.clone());
        state.anchor = None;
        supervisor.send(id, &HostMessage::TutorialNavigate { page })
            && supervisor.send(
                id,
                &HostMessage::TutorialTarget {
                    target: Some(target),
                    reveal: true,
                },
            )
    }

    /// The highlight rectangle of the step being shown, in the host window. `origin` is the
    /// top-left corner of the add-on's page.
    pub fn anchor(&self, id: &str, origin: Pos2) -> Option<Rect> {
        let rect = self.addons.get(id)?.anchor?;
        Some(rect.translate(origin.to_vec2()))
    }

    /// Ends the tour (also on Cancel): the add-on restores the UI state it saved.
    pub fn finish_tour(&mut self, supervisor: &Supervisor, id: &str) {
        if let Some(state) = self.addons.get_mut(id) {
            if std::mem::take(&mut state.touring) {
                state.target = None;
                state.anchor = None;
                supervisor.send(
                    id,
                    &HostMessage::TutorialTarget {
                        target: None,
                        reveal: false,
                    },
                );
                supervisor.send(id, &HostMessage::TutorialFinish);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::tests::{fake, supervisor, ID};
    use crate::supervisor::{StopReason, Timing};
    use eframe::egui::pos2;
    use std::time::{Duration, Instant};
    use toolboks_addon_proto::{InputBatch, Theme};

    fn entry(id: &str, title: &str, pages: &[&str]) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": title,
            "short_description": "Example",
            "publisher": "Toolboks",
            "category": "system",
            "icon": "leaf",
            "accent": "#8B5CF6",
            "kind": "native",
            "pages": pages,
            "version": "1.0.0",
            "min_toolboks_version": "0.12.0",
            "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": "https://github.com/test/addon.exe",
            "size": 1,
            "sha256": "00".repeat(32),
            "verified": true,
            "channel": "stable"
        }))
        .unwrap()
    }

    fn message(id: &str, message: AddonMessage) -> Event {
        Event::Message {
            id: id.into(),
            message,
        }
    }

    fn status(text: &str) -> StatusItem {
        StatusItem {
            text: text.into(),
            tooltip: None,
        }
    }

    /// A supervisor with nothing running, a data folder that does not exist, and a config.
    fn idle() -> (Supervisor, std::path::PathBuf, Config) {
        (
            supervisor(Timing::default()),
            std::env::temp_dir().join("toolboks-services-no-such-folder"),
            Config::default(),
        )
    }

    fn texts(items: Vec<StatusItem>) -> Vec<String> {
        items.into_iter().map(|item| item.text).collect()
    }

    #[test]
    fn status_items_show_while_running_and_on() {
        let (supervisor, root, mut config) = idle();
        let mut services = Services::default();
        let mut apply = |services: &mut Services, event: Event| {
            services.apply(&event, &supervisor, &root, &mut config)
        };
        apply(
            &mut services,
            message(
                "b.monitor",
                AddonMessage::StatusItems(vec![status("CPU 24%")]),
            ),
        );
        apply(
            &mut services,
            message(
                "a.memory",
                AddonMessage::StatusItems(vec![status("RAM 63%")]),
            ),
        );
        assert_eq!(
            texts(services.status_items(|_| true)),
            ["RAM 63%", "CPU 24%"]
        );
        // Turned off: its items are hidden, the other add-on's stay.
        assert_eq!(
            texts(services.status_items(|id| id != "a.memory")),
            ["CPU 24%"]
        );
        // A new set replaces the old one.
        apply(
            &mut services,
            message(
                "b.monitor",
                AddonMessage::StatusItems(vec![status("CPU 7%")]),
            ),
        );
        assert_eq!(
            texts(services.status_items(|_| true)),
            ["RAM 63%", "CPU 7%"]
        );
        // Not running any more: the items disappear.
        apply(
            &mut services,
            Event::Stopped {
                id: "b.monitor".into(),
                reason: StopReason::ByHost,
            },
        );
        apply(
            &mut services,
            Event::Restarting {
                id: "a.memory".into(),
            },
        );
        assert!(services.status_items(|_| true).is_empty());
    }

    #[test]
    fn what_an_add_on_sends_is_cut_to_the_limits() {
        let (supervisor, root, mut config) = idle();
        let mut services = Services::default();
        let many = (0..20).map(|i| status(&format!("Item {i}"))).collect();
        services.apply(
            &message("a", AddonMessage::StatusItems(many)),
            &supervisor,
            &root,
            &mut config,
        );
        assert_eq!(services.status_items(|_| true).len(), MAX_STATUS_ITEMS);
        let long = status(&format!("line one\nline two {}", "x".repeat(200)));
        services.apply(
            &message("a", AddonMessage::StatusItems(vec![long, status("  ")])),
            &supervisor,
            &root,
            &mut config,
        );
        let items = texts(services.status_items(|_| true));
        assert_eq!(items.len(), 1, "a blank item is dropped");
        assert_eq!(items[0].chars().count(), MAX_STATUS_CHARS);
        assert!(!items[0].contains('\n'));

        let tray = (0..20)
            .map(|i| TrayItem {
                id: format!("item-{i}"),
                label: "Do it".into(),
                enabled: true,
            })
            .collect();
        services.apply(
            &message("a", AddonMessage::TrayItems(tray)),
            &supervisor,
            &root,
            &mut config,
        );
        assert_eq!(services.tray_items(|_| true).len(), MAX_TRAY_ITEMS);
        assert!(services.tray_items(|_| false).is_empty());
        // Nothing runs here, so nothing can be sent.
        assert!(!services.tray_action(&supervisor, "a", "item-0"));
    }

    #[test]
    fn preferences_are_written_under_the_add_ons_own_id_only() {
        let (supervisor, root, mut config) = idle();
        let mut services = Services::default();
        config
            .preferences
            .entry("other".into())
            .or_default()
            .insert("kept".into(), true.into());
        let mut send = |json: &str, changed: bool| {
            services.apply(
                &message(
                    "a",
                    AddonMessage::Preferences {
                        json: json.into(),
                        changed,
                    },
                ),
                &supervisor,
                &root,
                &mut config,
            )
        };
        assert_eq!(send(r#"{"count":1}"#, true), Some(Effect::SaveConfig));
        // The same again, an unchanged report, something that is no object, and too much.
        assert_eq!(send(r#"{"count":1}"#, true), None);
        assert_eq!(send(r#"{"count":2}"#, false), None);
        assert_eq!(send("[1,2]", true), None);
        let huge = format!(r#"{{"x":"{}"}}"#, "y".repeat(MAX_PREFERENCES_BYTES));
        assert_eq!(send(&huge, true), None);
        assert_eq!(config.preferences["a"]["count"], 1);
        assert_eq!(config.preferences["other"]["kept"], true);
        assert_eq!(config.preferences.len(), 2);
    }

    #[test]
    fn links_resolve_against_declared_pages() {
        let installed = [
            entry(
                "toolboks.disk-manager",
                "Disk Manager",
                &["Map", "Largest files"],
            ),
            entry("toolboks.debloater", "Debloater", &[]),
        ];
        let monitor = entry(
            "toolboks.resource-monitor",
            "Resource Monitor",
            &["Overview"],
        );
        let catalog = [&monitor];
        let on = |id: &str| id != "toolboks.debloater";
        let resolve = |target: &str| resolve_link(target, &installed, on, &catalog);
        assert_eq!(
            resolve("disk manager/LARGEST FILES"),
            Link::Open {
                id: "toolboks.disk-manager".into(),
                page: Some("Largest files".into())
            }
        );
        let first_page = Link::Open {
            id: "toolboks.disk-manager".into(),
            page: None,
        };
        assert_eq!(resolve("Disk Manager"), first_page);
        assert_eq!(resolve("Disk Manager/Nope"), first_page);
        // Turned off: the overview with a message, as for the built-in tools today.
        assert_eq!(
            resolve("Debloater/Apps"),
            Link::TurnedOff {
                id: "toolboks.debloater".into(),
                title: "Debloater".into()
            }
        );
        // Not installed but in the catalog: offer to install it.
        assert_eq!(
            resolve("Resource Monitor/Overview"),
            Link::NotInstalled {
                id: "toolboks.resource-monitor".into(),
                title: "Resource Monitor".into()
            }
        );
        assert_eq!(resolve("Nope"), Link::Unknown);
        assert_eq!(resolve(""), Link::Unknown);
    }

    #[test]
    fn background_add_ons_start_only_while_turned_on() {
        let root = std::path::Path::new("data");
        let mut recorder = entry("toolboks.history", "History", &[]);
        recorder.background = true;
        let mut cleaner = entry("toolboks.memory-cleaner", "Memory Cleaner", &[]);
        cleaner.background = true;
        cleaner.capabilities = vec!["administrator".into()];
        let mut package = entry("example.declarative", "Package", &[]);
        package.background = true;
        package.kind = Kind::Declarative;
        let installed = [
            recorder,
            cleaner,
            package,
            entry("toolboks.disk-manager", "Disk Manager", &[]),
        ];
        let specs = background(&installed, root, |id| id != "toolboks.history");
        assert_eq!(
            specs.len(),
            1,
            "off, declarative and foreground add-ons do not start"
        );
        assert_eq!(specs[0].id, "toolboks.memory-cleaner");
        assert_eq!(specs[0].name, "Memory Cleaner");
        assert!(specs[0].may_elevate);
        assert!(specs[0].background);
        assert!(!launch_spec(root, &installed[3]).background);
        assert_eq!(specs[0].installed_root.as_deref(), Some(root));
        assert!(!launch_spec(root, &installed[3]).may_elevate);
    }

    #[test]
    fn the_launch_spec_speaks_the_installed_entrys_protocol() {
        let root = std::path::Path::new("data");
        let mut old = entry("toolboks.old", "Old", &[]);
        old.protocol_version = 3;
        assert_eq!(launch_spec(root, &old).protocol_version, 3);
        let new = entry("toolboks.new", "New", &[]);
        assert_eq!(new.protocol_version, 4);
        assert_eq!(launch_spec(root, &new).protocol_version, 4);
    }

    #[test]
    fn findings_are_accepted_and_change_nothing_else() {
        let (supervisor, root, mut config) = idle();
        let mut services = Services::default();
        let many = (0..40)
            .map(|i| Finding::ok(format!("check-{i}"), "Title", "Seen"))
            .collect();
        let effect = services.apply(
            &message("a", AddonMessage::Findings(many)),
            &supervisor,
            &root,
            &mut config,
        );
        assert_eq!(effect, None);
        assert!(services.status_items(|_| true).is_empty());
        assert!(config.preferences.is_empty());
    }

    /// Ticks the supervisor and applies every event until `done`. Returns the effects seen.
    fn run(
        supervisor: &mut Supervisor,
        services: &mut Services,
        config: &mut Config,
        mut done: impl FnMut(&Services, &Config, &[Effect]) -> bool,
    ) -> Vec<Effect> {
        let root = std::env::temp_dir().join("toolboks-services-no-such-folder");
        let give_up = Instant::now() + Duration::from_secs(30);
        let mut effects = Vec::new();
        loop {
            for event in supervisor.tick(Instant::now()) {
                effects.extend(services.apply(&event, supervisor, &root, config));
            }
            if done(services, config, &effects) {
                return effects;
            }
            assert!(Instant::now() < give_up, "timed out; effects: {effects:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_real_add_on_uses_every_service() {
        let (_dir, spec) = fake("services", "services");
        let mut supervisor = supervisor(Timing::default());
        let mut services = Services::default();
        let mut config = Config::default();
        config
            .preferences
            .entry(ID.into())
            .or_default()
            .insert("count".into(), 5.into());
        let shows = |text: &'static str| {
            move |services: &Services, _: &Config, _: &[Effect]| {
                texts(services.status_items(|_| true)) == [text]
            }
        };

        // Preferences are loaded after it connects, and its status item shows them.
        supervisor.open(spec, Instant::now());
        run(&mut supervisor, &mut services, &mut config, shows("Fake 5"));
        assert!(services.status_items(|_| false).is_empty(), "turned off");

        // Tray: only an entry it registered and enabled is passed on.
        let tray = services.tray_items(|_| true);
        let labels: Vec<_> = tray
            .iter()
            .map(|(addon, item)| (addon.as_str(), item.label.as_str(), item.enabled))
            .collect();
        assert_eq!(labels, [(ID, "Add one", true), (ID, "Not now", false)]);
        assert!(!services.tray_action(&supervisor, ID, "off"));
        assert!(!services.tray_action(&supervisor, ID, "nope"));
        assert!(services.tray_action(&supervisor, ID, "bump"));
        run(&mut supervisor, &mut services, &mut config, shows("Fake 6"));

        // Preferences: the change comes back and lands under the add-on's id.
        assert!(services.save_preferences(&supervisor, ID));
        let effects = run(
            &mut supervisor,
            &mut services,
            &mut config,
            |_, _, effects| effects.contains(&Effect::SaveConfig),
        );
        assert_eq!(effects, [Effect::SaveConfig]);
        assert_eq!(config.preferences[ID]["count"], 6);

        // Tour: steps arrive, and the anchor of the shown step is reported in the page.
        assert!(services.start_tour(&supervisor, ID));
        assert!(services.touring(ID));
        run(
            &mut supervisor,
            &mut services,
            &mut config,
            |services, _, _| services.tour(ID).is_some(),
        );
        let tour = services.tour(ID).unwrap().clone();
        assert_eq!(tour.location, "main");
        assert_eq!(tour.steps.len(), 1);
        assert_eq!(tour.steps[0].target, "fake-anchor");
        assert!(!services.show_step(&supervisor, ID, 1), "no such step");
        assert!(services.show_step(&supervisor, ID, 0));
        // The page asks for a frame after changing the step; this test has no view.
        let frame = HostMessage::Input(InputBatch {
            seq: 1,
            size: [400.0, 300.0],
            pixels_per_point: 1.0,
            theme: Theme::Dark,
            window_focused: true,
            events: Vec::new(),
        });
        assert!(supervisor.send(ID, &frame));
        let origin = pos2(100.0, 50.0);
        run(
            &mut supervisor,
            &mut services,
            &mut config,
            |services, _, _| services.anchor(ID, origin).is_some(),
        );
        let anchor = services.anchor(ID, origin).unwrap();
        assert!(anchor.min.x >= 100.0 && anchor.min.y >= 50.0, "{anchor:?}");
        assert!(anchor.max.x <= 500.0 && anchor.max.y <= 350.0, "{anchor:?}");
        services.finish_tour(&supervisor, ID);
        assert!(!services.touring(ID));
        assert_eq!(services.anchor(ID, origin), None);
        // The tour ran no action: the count is what the tray entry left.
        assert_eq!(texts(services.status_items(|_| true)), ["Fake 6"]);

        // Stopped: nothing of it is left in the status bar or the tray.
        supervisor.stop(ID, Instant::now());
        run(
            &mut supervisor,
            &mut services,
            &mut config,
            |services, _, _| services.status_items(|_| true).is_empty(),
        );
        assert!(services.tray_items(|_| true).is_empty());
        assert!(!services.tray_action(&supervisor, ID, "bump"));
    }
}
