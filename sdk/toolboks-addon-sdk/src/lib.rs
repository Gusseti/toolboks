//! API 1 declarative packages and API 2 source-level host-linked providers.
//! This contract is not a dynamic-library ABI or arbitrary executable-code loader.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024;
pub const API_VERSION: u32 = 2;

#[cfg(feature = "native-ui")]
pub mod tutorial;

#[cfg(feature = "runtime")]
pub mod runtime;

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
}

#[cfg(feature = "native-ui")]
/// One item an add-on shows in the host's status bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusItem {
    pub text: String,
    pub tooltip: Option<String>,
}

#[cfg(feature = "native-ui")]
pub enum HostRequest {
    RestartElevated { page: String },
    ReviewUpdate,
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
            "grid", "activity", "layers", "storage", "wrench", "leaf", "macro",
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
                )
            })
        {
            return Err("This capability requires an approved native provider.".into());
        }
        if m.byline.as_ref().is_some_and(|s| !text(s, 60)) {
            return Err("Invalid add-on byline.".into());
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
    fn set_capabilities(&mut self, _capabilities: &[Capability]) {}
    /// Initializes preferences before foreground UI or background work.
    fn load_preferences(
        &mut self,
        _preferences: &std::collections::BTreeMap<String, serde_json::Value>,
    ) {
    }
    fn take_host_request(&mut self) -> Option<HostRequest> {
        None
    }
    fn navigate(&mut self, _page: &str) {}
    fn page_index(&self) -> usize {
        0
    }
    /// Returns true when the add-on changed its namespaced persistent preferences.
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool;
    fn tick(&mut self, _ctx: &egui::Context) {}
    fn suspend(&mut self) {}
    fn resume(&mut self) {}
    fn has_background_work(&self) -> bool {
        false
    }
    fn tick_background(&mut self) {}
    /// Collects completed background changes without constructing the add-on UI.
    fn save_preferences(
        &mut self,
        _preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        false
    }
    fn cancel(&mut self) {}
    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        Vec::new()
    }
    fn tutorial_location(&self) -> String {
        String::new()
    }
    fn tutorial_start(&mut self) {}
    fn tutorial_navigate(&mut self, _page: &str) {}
    fn tutorial_finish(&mut self) {}
    /// Status bar items. Only the out-of-process runtime forwards them today.
    fn status_items(&self) -> Vec<StatusItem> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> serde_json::Value {
        serde_json::from_str(include_str!("../../examples/hello-system.tbaddon")).unwrap()
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
