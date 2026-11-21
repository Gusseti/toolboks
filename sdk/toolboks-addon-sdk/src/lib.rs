//! The Toolboks add-on SDK: the manifest and package format, and the [`NativeAddon`] trait an
//! add-on implements.
//!
//! Making an add-on? Start with `docs/ADDON-AUTHOR-GUIDE.md` in the Toolboks repository: it
//! creates one from the `hello` add-on, draws a page, saves a setting and tests it. In short, an
//! add-on is a struct that implements [`NativeAddon`] (only [`NativeAddon::ui`] is required) and
//! is run by `runtime::run_addon` (feature `runtime`).
//!
//! API 1 declarative packages and API 2 source-level host-linked providers.
//! This contract is not a dynamic-library ABI or arbitrary executable-code loader.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024;
pub const API_VERSION: u32 = 2;

#[cfg(feature = "native-ui")]
pub mod copy_hover;
#[cfg(feature = "native-ui")]
pub mod tutorial;

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(feature = "native-ui")]
pub mod admin;
#[cfg(feature = "native-ui")]
pub use toolboks_addon_proto::{AdminOutcome, AdminRequest};
/// Health findings (protocol 4; `why` and `evidence` protocol 5): see
/// [`NativeAddon::findings`].
#[cfg(feature = "native-ui")]
pub use toolboks_addon_proto::{Evidence, Finding, FindingAction, FindingStatus, Severity};

/// Declarative tours only describe and navigate UI; they never execute actions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialStep {
    pub page: String,
    pub target: String,
    pub title: String,
    pub description: String,
}
impl TutorialStep {
    pub fn new(page: &str, target: &str, title: &str, description: &str) -> Self {
        Self {
            page: page.into(),
            target: target.into(),
            title: title.into(),
            description: description.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub byline: Option<String>,
    #[serde(default)]
    pub native_provider: Option<String>,
    pub description: String,
    pub long_description: String,
    pub version: String,
    pub author: String,
    pub publisher: String,
    pub category: String,
    pub icon: String,
    pub accent: String,
    pub api_version: u32,
    pub min_toolboks_version: String,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub support_url: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// One plain-language line about data the add-on keeps on this PC, shown in the install
    /// review and the details. See [`valid_data_note`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_note: Option<String>,
    /// The names of the health checks this add-on can report to Home (see
    /// [`valid_check_names`]). Optional; the catalog entry carries the same list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    HostMetrics,
    LocalPreferences,
    FilesystemRead,
    UserExports,
    DriveOptimize,
    LiveAudio,
    SystemTuning,
    InputAutomation,
    /// A local job queue and Claude Code integration (the Testdeck add-on). Grants no
    /// administrator helper, so it is not in the native-only list in `validate`.
    DeveloperIntegration,
    Administrator,
}

#[cfg(feature = "native-ui")]
/// One item an add-on shows in the host's status bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusItem {
    pub text: String,
    pub tooltip: Option<String>,
}

#[cfg(feature = "native-ui")]
/// One entry an add-on adds to the host's tray menu. `id` comes back in
/// [`NativeAddon::tray_action`] when the user chooses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[cfg(feature = "native-ui")]
pub enum HostRequest {
    /// Legacy source contract; protocol-3 hosts refuse whole-interface elevation.
    RestartElevated {
        page: String,
    },
    ReviewUpdate,
    AdminOperation(AdminRequest),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub schema_version: u32,
    pub manifest: Manifest,
    pub pages: Vec<Page>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tutorial: Vec<TutorialStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub widgets: Vec<Widget>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Widget {
    Heading {
        text: String,
    },
    Text {
        text: String,
    },
    Note {
        title: String,
        text: String,
    },
    Metric {
        label: String,
        binding: Metric,
    },
    Toggle {
        key: String,
        label: String,
        #[serde(default)]
        default: bool,
    },
    TextInput {
        key: String,
        label: String,
        #[serde(default)]
        default: String,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    MemoryPercent,
    MemoryAvailable,
    MemoryTotal,
    Uptime,
}

pub fn version(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<_> = s.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || !p.bytes().all(|c| c.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
        })
    {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}
pub fn valid_id(s: &str) -> bool {
    (3..=100).contains(&s.len())
        && s.split('.').all(|p| {
            !p.is_empty()
                && p.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
fn text(s: &str, max: usize) -> bool {
    !s.trim().is_empty()
        && s.len() <= max
        && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}
/// The longest data note, in characters.
pub const MAX_DATA_NOTE_CHARS: usize = 160;
/// A data note is one line of plain text: 1 to [`MAX_DATA_NOTE_CHARS`] characters, no whitespace
/// at either end, no control characters (so no line breaks or tabs) and no [`hidden_char`].
/// The SDK and the catalog both check notes with this one rule.
pub fn valid_data_note(s: &str) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= MAX_DATA_NOTE_CHARS
        && !s.chars().any(|c| c.is_control() || hidden_char(c))
}
/// The most health check names an add-on can declare.
pub const MAX_CHECKS: usize = 6;
/// The longest health check name, in characters.
pub const MAX_CHECK_NAME_CHARS: usize = 40;
/// A health check name is plain text like a data note (no control, hidden or bidi characters, no
/// whitespace at either end, so never empty after trimming) of at most
/// [`MAX_CHECK_NAME_CHARS`] characters.
pub fn valid_check_name(s: &str) -> bool {
    valid_data_note(s) && s.chars().count() <= MAX_CHECK_NAME_CHARS
}
/// A list of health check names: at most [`MAX_CHECKS`], each a [`valid_check_name`], and no two
/// the same when upper and lower case are ignored. The SDK and the catalog use this one rule.
pub fn valid_check_names(names: &[String]) -> bool {
    names.len() <= MAX_CHECKS
        && names.iter().all(|n| valid_check_name(n))
        && names.iter().enumerate().all(|(i, n)| {
            !names[..i]
                .iter()
                .any(|earlier| earlier.to_lowercase() == n.to_lowercase())
        })
}
/// Unicode format characters (general category Cf, as of Unicode 15) and the line and paragraph
/// separators (U+2028, U+2029). They are invisible or change how the text around them is shown:
/// bidi overrides and isolates such as U+202E can make a note read differently from what it
/// says, and zero-width characters hide text.
pub fn hidden_char(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}
fn key(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn url(s: &str) -> bool {
    s.starts_with("https://")
        && s.len() <= 2048
        && !s.chars().any(char::is_whitespace)
        && s.len() > 8
}

impl Package {
    pub fn parse(bytes: &[u8], host_version: &str) -> Result<Self, String> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err("The package exceeds the 2 MiB limit.".into());
        }
        let package: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid add-on package: {e}"))?;
        package.validate(host_version)?;
        Ok(package)
    }
    pub fn validate(&self, host_version: &str) -> Result<(), String> {
        let m = &self.manifest;
        if self.schema_version != 1 || m.api_version == 0 || m.api_version > API_VERSION {
            return Err("This package needs an unsupported Toolboks add-on API.".into());
        }
        if !valid_id(&m.id) {
            return Err(
                "Add-on ID must use lowercase dot-separated names, digits and hyphens.".into(),
            );
        }
        if version(&m.version).is_none() {
            return Err("Add-on version must use major.minor.patch.".into());
        }
        let required =
            version(&m.min_toolboks_version).ok_or("Invalid minimum Toolboks version.")?;
        if version(host_version).is_none_or(|v| v < required) {
            return Err(format!(
                "This add-on needs Toolboks {} or later.",
                m.min_toolboks_version
            ));
        }
        if !text(&m.title, 64)
            || !text(&m.description, 220)
            || !text(&m.long_description, 8000)
            || !text(&m.author, 100)
            || !text(&m.publisher, 100)
            || !text(&m.category, 60)
        {
            return Err("Missing or oversized add-on metadata.".into());
        }
        if ![
            "grid", "activity", "layers", "storage", "wrench", "leaf", "macro", "sliders", "chip",
            "shield", "mic", "gauge", "custom",
        ]
        .contains(&m.icon.as_str())
        {
            return Err("Unknown sidebar icon.".into());
        }
        if m.accent.len() != 7
            || !m.accent.starts_with('#')
            || !m.accent[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Accent must be a #RRGGBB color.".into());
        }
        if m.website
            .iter()
            .chain(&m.support_url)
            .chain(&m.screenshots)
            .any(|s| !url(s))
            || m.screenshots.len() > 8
        {
            return Err(
                "Public links and screenshots must use HTTPS (maximum 8 screenshots).".into(),
            );
        }
        if m.license.as_ref().is_some_and(|s| !text(s, 100)) {
            return Err("Invalid license.".into());
        }
        if let Some(provider) = &m.native_provider {
            if m.api_version < 2 || !valid_id(provider) || !self.pages.is_empty() {
                return Err("Native providers require API 2, a valid provider ID, and no declarative pages.".into());
            }
        } else if self.pages.is_empty() || self.pages.len() > 12 {
            return Err("An add-on must provide 1–12 pages.".into());
        }
        if m.native_provider.is_none()
            && m.capabilities.iter().any(|c| {
                matches!(
                    c,
                    Capability::FilesystemRead
                        | Capability::UserExports
                        | Capability::DriveOptimize
                        | Capability::LiveAudio
                        | Capability::SystemTuning
                        | Capability::InputAutomation
                        | Capability::Administrator
                )
            })
        {
            return Err("This capability requires an approved native provider.".into());
        }
        if m.byline.as_ref().is_some_and(|s| !text(s, 60)) {
            return Err("Invalid add-on byline.".into());
        }
        if m.data_note.as_deref().is_some_and(|s| !valid_data_note(s)) {
            return Err(format!(
                "The data note must be one line of plain text, 1 to {MAX_DATA_NOTE_CHARS} characters, with no spaces at either end."
            ));
        }
        if !valid_check_names(&m.checks) {
            return Err(format!(
                "The health checks must be at most {MAX_CHECKS} different names of 1 to {MAX_CHECK_NAME_CHARS} characters of plain text, with no spaces at either end."
            ));
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for page in &self.pages {
            if !key(&page.id)
                || !ids.insert(&page.id)
                || !text(&page.title, 60)
                || page.widgets.len() > 100
            {
                return Err("Invalid page ID, title or widget count.".into());
            }
            for w in &page.widgets {
                let good = match w {
                    Widget::Heading { text: s } => text(s, 180),
                    Widget::Text { text: s } => text(s, 8000),
                    Widget::Note { title, text: s } => text(title, 120) && text(s, 4000),
                    Widget::Metric { label, .. } => {
                        text(label, 100) && m.capabilities.contains(&Capability::HostMetrics)
                    }
                    Widget::Toggle { key: k, label, .. } => {
                        key(k)
                            && keys.insert(k)
                            && text(label, 180)
                            && m.capabilities.contains(&Capability::LocalPreferences)
                    }
                    Widget::TextInput {
                        key: k,
                        label,
                        default,
                    } => {
                        key(k)
                            && keys.insert(k)
                            && text(label, 180)
                            && default.len() <= 4000
                            && m.capabilities.contains(&Capability::LocalPreferences)
                    }
                };
                if !good {
                    return Err(
                        "Invalid widget, duplicate preference key, or undeclared capability."
                            .into(),
                    );
                }
            }
        }
        if self.tutorial.len() > 300 {
            return Err("A tutorial can have at most 300 steps.".into());
        }
        for step in &self.tutorial {
            let page = self.pages.iter().find(|p| p.id == step.page);
            let target_valid = page.is_some_and(|p| {
                step.target == "page"
                    || step
                        .target
                        .strip_prefix("widget-")
                        .and_then(|s| s.parse::<usize>().ok())
                        .is_some_and(|i| i < p.widgets.len())
            });
            if !text(&step.title, 120) || !text(&step.description, 4000) || !target_valid {
                return Err(
                    "Tutorial steps need a valid page and widget target, title and description."
                        .into(),
                );
            }
        }
        Ok(())
    }
}

/// A host-linked provider renders inside the existing workspace. This Rust trait is
/// a source-level contract, not a dynamic-library ABI. The host resolves trusted providers.
#[cfg(feature = "native-ui")]
pub trait NativeAddon {
    /// Tells the add-on which capabilities the user granted, after the host connected. Do
    /// nothing that was not granted; the manifest alone grants nothing.
    fn set_capabilities(&mut self, _capabilities: &[Capability]) {}
    /// Initializes preferences before foreground UI or background work.
    fn load_preferences(
        &mut self,
        _preferences: &std::collections::BTreeMap<String, serde_json::Value>,
    ) {
    }
    /// Read-only preparation in the dedicated helper, never in the drawing runtime.
    fn prepare_admin(
        &mut self,
        _request: &AdminRequest,
    ) -> Result<Box<dyn admin::AdminChange>, String> {
        Err("This add-on does not support administrator actions.".into())
    }
    /// Informational result in the unelevated UI; never use this to authorize a write.
    fn admin_result(&mut self, _id: u64, _outcome: AdminOutcome) {}
    /// Asked after every message. Return `HostRequest::AdminOperation` for one administrator
    /// action the user then confirms in a Confirm dialog, once per user action, then `None`.
    /// (The host currently takes no action on `HostRequest::ReviewUpdate`.)
    fn take_host_request(&mut self) -> Option<HostRequest> {
        None
    }
    /// A link or the sidebar chose the page named `_page` (one of the names in the add-on's
    /// `host-services.json`). Only needed by add-ons with more than one page.
    fn navigate(&mut self, _page: &str) {}
    /// The index of the page being shown, in the order of the `host-services.json` names.
    fn page_index(&self) -> usize {
        0
    }
    /// Draws the page. Called once per frame while the page is shown, when the host sent input
    /// or a repaint was requested (`ui.ctx().request_repaint()`).
    ///
    /// Returns true when the add-on changed its namespaced persistent preferences: write the
    /// new values into `preferences` first, and the host saves them under the add-on's id.
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool;
    /// Per-frame housekeeping with the egui context, before `ui`. Not called while suspended.
    fn tick(&mut self, _ctx: &egui::Context) {}
    /// The page was left. Release memory you only need while it is visible.
    fn suspend(&mut self) {}
    /// The page is shown again after `suspend`.
    fn resume(&mut self) {}
    /// True while there is work to do with the page hidden. Only an add-on declared
    /// `"background": true` in `host-services.json` keeps its process while hidden.
    fn has_background_work(&self) -> bool {
        false
    }
    /// Called about every 200 ms while the page is hidden and `has_background_work` is true.
    fn tick_background(&mut self) {}
    /// Collects completed background changes without constructing the add-on UI.
    fn save_preferences(
        &mut self,
        _preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        false
    }
    /// Called before the add-on exits (Toolboks closed, or the user turned it off). Stop work
    /// safely; the process can be ended at any time, so never rely on this for consistency.
    fn cancel(&mut self) {}
    /// The steps of the guided tour. Default: none, and the host supplies no tour.
    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        Vec::new()
    }
    /// The route of the page being shown, matching a step's page.
    fn tutorial_location(&self) -> String {
        String::new()
    }
    /// A tour started. Save transient UI state (open tabs, selections) to restore at the end.
    fn tutorial_start(&mut self) {}
    /// The tour moved to `_page`. Navigate only; never run an action.
    fn tutorial_navigate(&mut self, _page: &str) {}
    /// The tour ended or was cancelled. Restore the state saved in `tutorial_start`.
    fn tutorial_finish(&mut self) {}
    /// Status bar items. Only the out-of-process runtime forwards them today.
    fn status_items(&self) -> Vec<StatusItem> {
        Vec::new()
    }
    /// This add-on's current health findings for Home. Called after every frame and
    /// background tick (also while suspended); sent to the host only when the set changed,
    /// and only to a host that speaks protocol 4 or later. At most 16 are kept. Default: none.
    /// A finding's `why` and `evidence` reach a host at protocol 5 or later; a protocol 4 host
    /// gets the finding without them.
    ///
    /// Build a finding when you measure, keep it, and return a copy here. The builders
    /// ([`Finding::ok`] and the others) stamp the current time, so a finding built inside this
    /// method changes every second and is sent again each time.
    fn findings(&self) -> Vec<Finding> {
        Vec::new()
    }
    /// The user asked Home to check again. Optional: check again when you can, and the new
    /// [`Self::findings`] are sent. Default: nothing.
    fn check_now(&mut self) {}
    /// Entries for the host's tray menu. Only the out-of-process runtime forwards them.
    fn tray_items(&self) -> Vec<TrayItem> {
        Vec::new()
    }
    /// The user chose the tray entry `id`. Only entries from [`Self::tray_items`] arrive.
    fn tray_action(&mut self, _id: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> serde_json::Value {
        serde_json::from_str(include_str!("../../examples/hello-system.tbaddon")).unwrap()
    }
    #[test]
    fn every_icon_the_host_draws_is_accepted_and_others_are_not() {
        for icon in [
            "grid", "activity", "layers", "storage", "wrench", "leaf", "macro", "sliders", "chip",
            "shield", "mic", "gauge",
            // The add-on's own picture, `icon.png` beside the manifest (see the author guide).
            "custom",
        ] {
            let mut v = example();
            v["manifest"]["icon"] = serde_json::json!(icon);
            assert!(
                Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_ok(),
                "{icon}"
            );
        }
        let mut v = example();
        v["manifest"]["icon"] = serde_json::json!("no-such-icon");
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
    }
    #[test]
    fn starter_is_compatible() {
        Package::parse(&serde_json::to_vec(&example()).unwrap(), "0.8.2").unwrap();
    }
    #[test]
    fn rejects_spoofed_verification_and_unknown_code() {
        for field in ["verified", "entrypoint"] {
            let mut v = example();
            v["manifest"][field] = serde_json::json!(true);
            assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
        }
    }
    #[test]
    fn rejects_traversal_incompatible_api_and_future_host() {
        for (field, value) in [
            ("id", serde_json::json!("../../bad")),
            ("api_version", serde_json::json!(99)),
            ("min_toolboks_version", serde_json::json!("99.0.0")),
        ] {
            let mut v = example();
            v["manifest"][field] = value;
            assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
        }
    }
    #[test]
    fn capabilities_are_enforced() {
        let mut v = example();
        v["manifest"]["capabilities"] = serde_json::json!([]);
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
        for capability in [
            "administrator",
            "drive_optimize",
            "live_audio",
            "system_tuning",
            "input_automation",
        ] {
            let mut v = example();
            v["manifest"]["capabilities"] =
                serde_json::json!(["host_metrics", "local_preferences", capability]);
            assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
        }
    }
    #[test]
    fn developer_integration_is_a_known_capability() {
        let mut v = example();
        v["manifest"]["capabilities"] =
            serde_json::json!(["host_metrics", "local_preferences", "developer_integration"]);
        let package = Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").unwrap();
        assert!(package
            .manifest
            .capabilities
            .contains(&Capability::DeveloperIntegration));
        assert_eq!(
            serde_json::to_value(Capability::DeveloperIntegration).unwrap(),
            "developer_integration"
        );
    }
    #[test]
    fn repeated_page_ids_and_preference_keys_are_rejected() {
        let mut v = example();
        v["pages"][1]["id"] = v["pages"][0]["id"].clone();
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
        let mut v = example();
        v["pages"][1]["widgets"][2]["key"] = v["pages"][1]["widgets"][1]["key"].clone();
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
    }
    #[test]
    fn oversized_packages_and_unrecognized_widgets_are_rejected() {
        assert!(Package::parse(&vec![b' '; MAX_PACKAGE_BYTES + 1], "0.8.2").is_err());
        let mut v = example();
        v["pages"][0]["widgets"][0]["kind"] = serde_json::json!("execute");
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
    }
    #[test]
    fn version_and_id_rules() {
        assert!(valid_id("com.example.hello"));
        assert!(!valid_id("com..bad"));
        assert!(!valid_id("C:/escape"));
        assert_eq!(version("1.2.3"), Some((1, 2, 3)));
        assert!(version("01.2.3").is_none());
    }
    #[test]
    fn data_note_is_optional_and_checked() {
        let parse = |note: Option<serde_json::Value>| {
            let mut v = example();
            if let Some(note) = note {
                v["manifest"]["data_note"] = note;
            }
            Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2")
        };
        // Absent: every existing manifest keeps parsing, and it is not written back out.
        let package = parse(None).unwrap();
        assert_eq!(package.manifest.data_note, None);
        assert!(!serde_json::to_string(&package)
            .unwrap()
            .contains("data_note"));
        // An explicit null is the same as absent.
        assert_eq!(
            parse(Some(serde_json::Value::Null))
                .unwrap()
                .manifest
                .data_note,
            None
        );
        let note = "Keeps your last scan of each drive on this PC (file and folder names).";
        assert_eq!(
            parse(Some(serde_json::json!(note)))
                .unwrap()
                .manifest
                .data_note
                .as_deref(),
            Some(note)
        );
        // The limit counts characters, not bytes.
        assert!(parse(Some(serde_json::json!("a".repeat(160)))).is_ok());
        assert!(parse(Some(serde_json::json!("\u{e5}".repeat(160)))).is_ok());
        for bad in [
            "a".repeat(161),
            "\u{e5}".repeat(161),
            String::new(),
            "   ".into(),
            " Keeps scans.".into(),
            "Keeps scans. ".into(),
            "Keeps\nscans.".into(),
            "Keeps\tscans.".into(),
            "Keeps\u{7}scans.".into(),
            "Keeps\u{85}scans.".into(),
        ] {
            assert!(parse(Some(serde_json::json!(bad))).is_err(), "{bad:?}");
        }
        // Format characters (Cf) and line/paragraph separators, anywhere in the note.
        for hidden in [
            '\u{202e}',  // right-to-left override
            '\u{202a}',  // left-to-right embedding
            '\u{2066}',  // left-to-right isolate
            '\u{2069}',  // pop directional isolate
            '\u{200b}',  // zero width space
            '\u{200d}',  // zero width joiner
            '\u{200e}',  // left-to-right mark
            '\u{2060}',  // word joiner
            '\u{feff}',  // zero width no-break space
            '\u{ad}',    // soft hyphen
            '\u{e0041}', // tag latin capital letter a
            '\u{2028}',  // line separator
            '\u{2029}',  // paragraph separator
        ] {
            assert!(hidden_char(hidden), "{hidden:?}");
            for bad in [
                format!("Keeps{hidden}scans."),
                format!("{hidden}Keeps scans."),
                format!("Keeps scans.{hidden}"),
            ] {
                assert!(!valid_data_note(&bad), "{bad:?}");
                assert!(parse(Some(serde_json::json!(bad))).is_err(), "{bad:?}");
            }
        }
        // Ordinary punctuation and letters are not hidden.
        for shown in [
            'a', '\u{e5}', '(', '.', '\u{2013}', '\u{2019}', '\u{20ac}', ' ',
        ] {
            assert!(!hidden_char(shown), "{shown:?}");
        }
        assert!(valid_data_note(
            "Keeps scans \u{2013} file names, \u{e5}\u{f8}\u{e6}."
        ));
    }
    #[test]
    fn checks_are_optional_short_unique_plain_names() {
        let parse = |checks: Option<serde_json::Value>| {
            let mut v = example();
            if let Some(checks) = checks {
                v["manifest"]["checks"] = checks;
            }
            Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2")
        };
        // Absent: every existing manifest keeps parsing, and it is not written back out.
        let package = parse(None).unwrap();
        assert!(package.manifest.checks.is_empty());
        assert!(!serde_json::to_string(&package).unwrap().contains("checks"));
        let names: Vec<String> = vec![
            "Recent crashes".into(),
            "Disk health".into(),
            "\u{e5}".repeat(40),
        ];
        assert_eq!(
            parse(Some(serde_json::json!(names)))
                .unwrap()
                .manifest
                .checks,
            names
        );
        let six: Vec<String> = (0..6).map(|i| format!("Check {i}")).collect();
        assert!(parse(Some(serde_json::json!(six))).is_ok());
        let seven: Vec<String> = (0..7).map(|i| format!("Check {i}")).collect();
        let bad_lists: Vec<Vec<String>> = vec![
            seven,
            vec!["a".repeat(41)],
            vec!["\u{e5}".repeat(41)],
            vec![String::new()],
            vec!["  ".into()],
            vec![" Disk".into()],
            vec!["Disk ".into()],
            vec!["Dis\nk".into()],
            vec!["Dis\u{202e}k".into()],
            vec!["Dis\u{200b}k".into()],
            vec!["Disk".into(), "Disk".into()],
            vec!["Disk".into(), "disk".into()],
        ];
        for bad in bad_lists {
            assert!(parse(Some(serde_json::json!(bad))).is_err(), "{bad:?}");
            assert!(!valid_check_names(&bad), "{bad:?}");
        }
        assert!(valid_check_names(&[]));
    }
    #[test]
    fn tutorial_targets_are_validated_without_executable_actions() {
        let mut v = example();
        v["tutorial"] = serde_json::json!([{"page":"overview","target":"widget-2","title":"Memory reading","description":"A live reading from Toolboks."}]);
        Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").unwrap();
        for (field, value) in [
            ("page", "missing"),
            ("target", "widget-999"),
            ("target", "execute"),
            ("description", ""),
        ] {
            let mut invalid = v.clone();
            invalid["tutorial"][0][field] = serde_json::json!(value);
            assert!(Package::parse(&serde_json::to_vec(&invalid).unwrap(), "0.8.2").is_err());
        }
        v["tutorial"][0]["action"] = serde_json::json!("launch");
        assert!(Package::parse(&serde_json::to_vec(&v).unwrap(), "0.8.2").is_err());
    }
}
