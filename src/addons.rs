//! Installed data-only packages, host configuration, and the built-in system collection.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use toolboks_addon_sdk::{Package, MAX_PACKAGE_BYTES};

/// The single "System Essentials" entry that older versions stored. It is gone now; saved
/// settings that mention it are dropped when the configuration is loaded.
pub const LEGACY_BASE_ID: &str = "toolboks.system-essentials";
pub const SDK_EXAMPLE: &str = include_str!("../sdk/examples/hello-system.tbaddon");

/// The tools that used to be part of Toolboks itself (or were host-linked add-ons). They are
/// separate add-ons now. Their ids are kept so a user who updates from an earlier version gets
/// the same tools back, with the preferences already saved under these ids.
pub const FORMER_BUILTIN_IDS: [&str; 8] = [
    "toolboks.memory-cleaner",
    "toolboks.resource-monitor",
    "toolboks.debloater",
    "toolboks.event-watch",
    "toolboks.disk-manager",
    "toolboks.aural",
    "toolboks.system-profiles",
    "toolboks.macro-manager",
];

/// Native providers are no longer linked into Toolboks, so a package that names one cannot run.
fn validate_provider(p: &Package) -> Result<(), String> {
    if p.manifest.native_provider.is_some() {
        Err("This package needs a tool that is no longer built into Toolboks. Install it from the add-on browser.".into())
    } else {
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub first_run_complete: bool,
    pub keep_in_tray: bool,
    pub sidebar_collapsed: bool,
    pub disabled: Vec<String>,
    pub preferences: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// Set once the update from the version that had the tools built in has been looked at.
    pub platform_migrated: bool,
    /// Tools the user had before the update that are not installed yet: offered once at the
    /// first start as "Restore your tools" and kept until they are installed or skipped.
    pub restore_pending: Vec<String>,
}
pub struct Registry {
    pub packages: Vec<Package>,
    pub config: Config,
    pub root: PathBuf,
    pub errors: Vec<String>,
}
// Shared with add-ons, so they find and write their data the same way as the host.
pub use toolboks_shared::storage::{data_root, write_atomic};
fn reserved(id: &str) -> bool {
    id == LEGACY_BASE_ID
}
fn read_package(path: &Path) -> Result<Package, String> {
    let file = fs::File::open(path).map_err(|e| format!("Could not open package: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_PACKAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let p = Package::parse(&bytes, env!("CARGO_PKG_VERSION"))?;
    validate_provider(&p)?;
    if reserved(&p.manifest.id) {
        return Err("A bundled add-on cannot be replaced by a local package.".into());
    }
    Ok(p)
}
/// The id of a package file left by a version that linked this tool into Toolboks, if it is one.
fn obsolete_provider_package(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let manifest = value.get("manifest")?;
    let id = manifest.get("id")?.as_str()?;
    (manifest
        .get("native_provider")
        .is_some_and(|p| !p.is_null())
        && FORMER_BUILTIN_IDS.contains(&id))
    .then(|| id.to_owned())
}

impl Registry {
    pub fn load(root: PathBuf) -> Self {
        let mut r = Self {
            packages: vec![],
            config: Config::default(),
            root,
            errors: vec![],
        };
        let config = r.root.join("config.json");
        let existing_install = config.exists() || r.root.join("addons").is_dir();
        if config.exists() {
            match fs::read(&config)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
            {
                Ok(c) => r.config = c,
                Err(e) => r.errors.push(format!(
                    "Configuration could not be read. Defaults are in use: {e}"
                )),
            }
        }
        // The old combined entry no longer exists; forget anything saved for it.
        r.config.disabled.retain(|s| s != LEGACY_BASE_ID);
        r.config.preferences.remove(LEGACY_BASE_ID);
        let folder = r.root.join("addons");
        let mut former_installed: Vec<String> = Vec::new();
        match fs::read_dir(&folder) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let mut path = entry.path();
                    // A catalog install: `addons/<id>/<active version>/package.tbaddon`.
                    let mut expected = path.file_stem().map(|f| f.to_owned());
                    if path.is_dir() {
                        let Some(active) = crate::installer::active_package(&path) else {
                            continue;
                        };
                        expected = path.file_name().map(|f| f.to_owned());
                        path = active;
                    }
                    if path.extension().is_some_and(|x| x == "tbaddon") {
                        // A host-linked tool installed by the previous version: its code is not
                        // in Toolboks any more, so the file is obsolete. It is counted for the
                        // restore offer below and removed quietly.
                        if let Some(id) = obsolete_provider_package(&path) {
                            former_installed.push(id);
                            let _ = fs::remove_file(&path);
                            continue;
                        }
                        match read_package(&path) {
                            Ok(p) => {
                                if expected
                                    .as_deref()
                                    .is_some_and(|f| f == p.manifest.id.as_str())
                                    && !r.packages.iter().any(|a| a.manifest.id == p.manifest.id)
                                {
                                    r.packages.push(p)
                                } else {
                                    r.errors.push(format!(
                                        "Ignored conflicting add-on: {}",
                                        path.display()
                                    ));
                                }
                            }
                            Err(e) => r.errors.push(format!("{}: {e}", path.display())),
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => r.errors.push(format!("Add-ons could not be read: {e}")),
        }
        r.packages.sort_by_key(|p| p.manifest.title.to_lowercase());
        r.plan_restore(existing_install, former_installed);
        r
    }
    /// Works out, once, which tools the user had before the update and does not have yet. A new
    /// install has nothing to restore. Nothing is lost if the catalog cannot be reached now: the
    /// list stays in the configuration until the tools are installed or skipped.
    fn plan_restore(&mut self, existing_install: bool, former_installed: Vec<String>) {
        if self.config.platform_migrated {
            return;
        }
        self.config.platform_migrated = true;
        if !existing_install {
            return;
        }
        let mut wanted: Vec<String> = FORMER_BUILTIN_IDS
            .iter()
            .filter(|id| {
                // The first four were built in and on unless turned off; the others were
                // installed add-ons, so only those that were actually installed come back.
                let built_in = FORMER_BUILTIN_IDS[..4].contains(id);
                self.enabled(id) && (built_in || former_installed.iter().any(|f| f == *id))
            })
            .map(|id| (*id).to_owned())
            .collect();
        wanted.retain(|id| crate::installer::installed_entry(&self.root, id).is_none());
        self.config.restore_pending = wanted;
        let _ = self.save();
    }
    pub fn save(&self) -> Result<(), String> {
        write_atomic(
            &self.root.join("config.json"),
            &serde_json::to_vec_pretty(&self.config).map_err(|e| e.to_string())?,
        )
    }
    pub fn enabled(&self, id: &str) -> bool {
        !self.config.disabled.iter().any(|s| s == id)
    }
    pub fn verified(&self, id: &str) -> bool {
        crate::installer::installed_entry(&self.root, id).is_some_and(|entry| entry.verified)
    }
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let before = self.config.clone();
        self.config.disabled.retain(|s| s != id);
        if !enabled {
            self.config.disabled.push(id.into());
        }
        if let Err(e) = self.save() {
            self.config = before;
            return Err(e);
        }
        Ok(())
    }
    pub fn inspect(&self, path: &Path) -> Result<Package, String> {
        read_package(path)
    }
    pub fn install(&mut self, p: Package) -> Result<(), String> {
        p.validate(env!("CARGO_PKG_VERSION"))?;
        validate_provider(&p)?;
        if reserved(&p.manifest.id) {
            return Err("Cannot replace the bundled add-on.".into());
        }
        if let Some(old) = self
            .packages
            .iter()
            .find(|a| a.manifest.id == p.manifest.id)
        {
            if toolboks_addon_sdk::version(&p.manifest.version)
                <= toolboks_addon_sdk::version(&old.manifest.version)
            {
                return Err(
                    "This version is already installed, or is older than the installed version."
                        .into(),
                );
            }
        }
        write_atomic(
            &self
                .root
                .join("addons")
                .join(format!("{}.tbaddon", p.manifest.id)),
            &serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?,
        )?;
        self.packages.retain(|a| a.manifest.id != p.manifest.id);
        self.packages.push(p);
        self.packages
            .sort_by_key(|a| a.manifest.title.to_lowercase());
        Ok(())
    }
    #[cfg(test)]
    pub fn uninstall(&mut self, id: &str) -> Result<(), String> {
        self.remove(id, false)
    }
    /// Removes an add-on. With `keep_data` its preferences and data folder stay, so a later
    /// install picks them up again.
    pub fn remove(&mut self, id: &str, keep_data: bool) -> Result<(), String> {
        if reserved(id) || !self.packages.iter().any(|p| p.manifest.id == id) {
            return Err("This add-on cannot be removed.".into());
        }
        let flat = self.root.join("addons").join(format!("{id}.tbaddon"));
        if flat.exists() {
            fs::remove_file(flat).map_err(|e| e.to_string())?;
        } else {
            // Installed from the catalog: drop every version folder.
            crate::installer::remove(&self.root, id, keep_data)?;
        }
        if !keep_data {
            crate::installer::remove_data(&self.root, id)?;
            self.config.preferences.remove(id);
        }
        self.packages.retain(|p| p.manifest.id != id);
        self.config.disabled.retain(|s| s != id);
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("toolboks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn installation_persistence_and_removal() {
        let root = temp("addons");
        let mut r = Registry::load(root.clone());
        let p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
        r.install(p.clone()).unwrap();
        assert!(r.install(p).is_err());
        r.set_enabled("com.example.hello-system", false).unwrap();
        let mut reloaded = Registry::load(root.clone());
        assert_eq!(reloaded.packages.len(), 1);
        assert!(!reloaded.enabled("com.example.hello-system"));
        assert!(reloaded.uninstall("toolboks.memory-cleaner").is_err());
        reloaded.uninstall("com.example.hello-system").unwrap();
        assert!(Registry::load(root.clone()).packages.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_update_preserves_preferences_and_ignores_invalid_files() {
        let root = temp("upgrade");
        let mut r = Registry::load(root.clone());
        let mut p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
        let id = p.manifest.id.clone();
        r.install(p.clone()).unwrap();
        r.config
            .preferences
            .entry(id.clone())
            .or_default()
            .insert("label".into(), serde_json::json!("Custom name"));
        r.save().unwrap();
        p.manifest.version = "1.1.0".into();
        r.install(p).unwrap();
        fs::write(root.join("addons/invalid.tbaddon"), b"not JSON").unwrap();
        let reloaded = Registry::load(root.clone());
        assert_eq!(reloaded.packages.len(), 1);
        assert_eq!(reloaded.packages[0].manifest.version, "1.1.0");
        assert_eq!(reloaded.config.preferences[&id]["label"], "Custom name");
        assert_eq!(reloaded.errors.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_package_that_names_a_host_linked_tool_is_refused() {
        let mut r = Registry::load(temp("provider"));
        let mut p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
        p.manifest.native_provider = Some("toolboks.aural".into());
        assert!(r.install(p).is_err());
    }

    #[test]
    fn old_system_essentials_settings_are_dropped_without_error() {
        let root = temp("legacy");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("config.json"),
            br#"{"keep_in_tray":true,"disabled":["toolboks.system-essentials","com.example.x"],
            "preferences":{"toolboks.system-essentials":{"a":1}}}"#,
        )
        .unwrap();
        let r = Registry::load(root.clone());
        assert!(r.errors.is_empty());
        assert!(r.config.keep_in_tray);
        assert_eq!(r.config.disabled, ["com.example.x"]);
        assert!(r.config.preferences.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_packages_cannot_take_the_old_combined_id() {
        let mut r = Registry::load(temp("reserved"));
        let mut p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
        p.manifest.id = LEGACY_BASE_ID.into();
        assert!(r.install(p).is_err());
    }

    // ---- the update from the version that had the tools built in -----------------------

    #[test]
    fn a_new_install_has_nothing_to_restore() {
        let root = temp("fresh");
        let r = Registry::load(root.clone());
        assert!(r.config.platform_migrated);
        assert!(r.config.restore_pending.is_empty());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_old_install_gets_its_four_built_in_tools_back_unless_they_were_turned_off() {
        let root = temp("old");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("config.json"),
            br#"{"first_run_complete":true,"disabled":["toolboks.debloater"],
            "preferences":{"toolboks.resource-monitor":{"x":1}}}"#,
        )
        .unwrap();
        let r = Registry::load(root.clone());
        assert_eq!(
            r.config.restore_pending,
            [
                "toolboks.memory-cleaner",
                "toolboks.resource-monitor",
                "toolboks.event-watch"
            ]
        );
        // Their saved settings stay where the add-ons will look for them.
        assert_eq!(r.config.preferences["toolboks.resource-monitor"]["x"], 1);
        // The offer is made once: a later start does not make it again.
        let again = Registry::load(root.clone());
        assert_eq!(again.config.restore_pending, r.config.restore_pending);
        assert!(again.config.platform_migrated);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn host_linked_add_ons_installed_before_are_offered_and_their_old_files_removed() {
        let root = temp("linked");
        fs::create_dir_all(root.join("addons")).unwrap();
        fs::write(root.join("config.json"), br#"{"first_run_complete":true}"#).unwrap();
        let old = |id: &str| {
            format!(
                r##"{{"schema_version":1,"manifest":{{"id":"{id}","title":"x","native_provider":"{id}","description":"d","long_description":"l","version":"0.1.0","author":"a","publisher":"p","category":"c","icon":"i","accent":"#000000","api_version":2,"min_toolboks_version":"0.12.0","license":"MIT","capabilities":[]}},"pages":[]}}"##
            )
        };
        fs::write(
            root.join("addons/toolboks.aural.tbaddon"),
            old("toolboks.aural"),
        )
        .unwrap();
        let r = Registry::load(root.clone());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.packages.is_empty());
        assert!(r
            .config
            .restore_pending
            .contains(&"toolboks.aural".to_string()));
        assert!(!r
            .config
            .restore_pending
            .contains(&"toolboks.macro-manager".to_string()));
        assert!(!root.join("addons/toolboks.aural.tbaddon").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
