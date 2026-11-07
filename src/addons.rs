//! Installed data-only packages, host configuration, and the built-in system collection.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use toolboks_addon_sdk::{Package, MAX_PACKAGE_BYTES};

/// The built-in tools, in the same order as `tools::all()`: one manifest each.
pub const BUILTIN_MANIFESTS: [&str; 4] = [
    include_str!("../assets/memory-cleaner.json"),
    include_str!("../assets/resource-monitor.json"),
    include_str!("../assets/debloater.json"),
    include_str!("../assets/event-watch.json"),
];
/// The IDs of [`BUILTIN_MANIFESTS`], same order (a test keeps the two in step).
pub const BUILTIN_IDS: [&str; 4] = [
    "toolboks.memory-cleaner",
    "toolboks.resource-monitor",
    "toolboks.debloater",
    "toolboks.event-watch",
];
/// The single "System Essentials" entry that older versions stored. It is gone now; saved
/// settings that mention it are dropped when the configuration is loaded.
pub const LEGACY_BASE_ID: &str = "toolboks.system-essentials";
pub fn builtin_manifests() -> Vec<toolboks_addon_sdk::Manifest> {
    BUILTIN_MANIFESTS
        .iter()
        .map(|m| serde_json::from_str(m).expect("bundled manifest"))
        .collect()
}
pub const SDK_EXAMPLE: &str = include_str!("../sdk/examples/hello-system.tbaddon");
pub fn official_disk_package() -> Package {
    serde_json::from_str(toolboks_disk_manager::PACKAGE).expect("bundled disk package")
}
pub const AURAL_ID: &str = "toolboks.aural";
pub const OFFICIAL_ADDONS: [&str; 2] = [toolboks_disk_manager::ID, AURAL_ID];

pub fn official_package(id: &str) -> Option<Package> {
    match id {
        toolboks_disk_manager::ID => Some(official_disk_package()),
        AURAL_ID => Some(
            serde_json::from_str(include_str!("../assets/aural.tbaddon"))
                .expect("bundled Aural package"),
        ),
        _ => None,
    }
}

pub fn trusted_native(p: &Package) -> bool {
    let Ok(value) = serde_json::to_value(p) else {
        return false;
    };
    let official = official_package(&p.manifest.id).and_then(|p| serde_json::to_value(p).ok());
    official.as_ref() == Some(&value)
        || (p.manifest.id == toolboks_disk_manager::ID
            && serde_json::from_str::<Package>(toolboks_disk_manager::LEGACY_PACKAGE)
                .ok()
                .and_then(|p| serde_json::to_value(p).ok())
                .as_ref()
                == Some(&value))
}
fn validate_provider(p: &Package) -> Result<(), String> {
    if (p.manifest.native_provider.is_some() || OFFICIAL_ADDONS.contains(&p.manifest.id.as_str()))
        && !trusted_native(p)
    {
        Err("This native package does not match a trusted provider in this Toolboks build.".into())
    } else {
        Ok(())
    }
}
pub struct SystemAddon {
    pub tools: Vec<Box<dyn crate::tools::Tool>>,
    pub selected: (usize, usize),
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keep_in_tray: bool,
    pub sidebar_collapsed: bool,
    pub disabled: Vec<String>,
    pub preferences: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}
pub struct Registry {
    /// Built-in tools, in tool order (index matches `SystemAddon::tools`).
    pub builtins: Vec<toolboks_addon_sdk::Manifest>,
    pub packages: Vec<Package>,
    pub config: Config,
    pub root: PathBuf,
    pub errors: Vec<String>,
}
pub fn data_root() -> PathBuf {
    let name = if cfg!(feature = "preview") {
        "Toolboks Preview"
    } else {
        "Toolboks"
    };
    // TOOLBOKS_DATA_DIR is useful for isolated automated tests and portable developer runs.
    if let Some(path) = std::env::var_os("TOOLBOKS_DATA_DIR") {
        return PathBuf::from(path).join(name);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(Path::new("."))
                .to_owned()
        })
        .join(name)
}
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("pending");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            },
        };
        let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    fs::rename(temp, path).map_err(|e| e.to_string())?;
    Ok(())
}
fn reserved(id: &str) -> bool {
    id == LEGACY_BASE_ID || BUILTIN_IDS.contains(&id)
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
impl Registry {
    pub fn load(root: PathBuf) -> Self {
        let mut r = Self {
            builtins: builtin_manifests(),
            packages: vec![],
            config: Config::default(),
            root,
            errors: vec![],
        };
        let config = r.root.join("config.json");
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
        match fs::read_dir(&folder) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|x| x == "tbaddon") {
                        match read_package(&path) {
                            Ok(p) => {
                                if path
                                    .file_stem()
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
        r.packages
            .sort_by_key(|p| (!trusted_native(p), p.manifest.title.to_lowercase()));
        r
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
    pub fn builtin_index(&self, id: &str) -> Option<usize> {
        self.builtins.iter().position(|m| m.id == id)
    }
    pub fn verified(&self, id: &str) -> bool {
        self.builtin_index(id).is_some() || OFFICIAL_ADDONS.contains(&id)
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
            .sort_by_key(|a| (!trusted_native(a), a.manifest.title.to_lowercase()));
        Ok(())
    }
    pub fn uninstall(&mut self, id: &str) -> Result<(), String> {
        if reserved(id) || !self.packages.iter().any(|p| p.manifest.id == id) {
            return Err("This add-on cannot be removed.".into());
        }
        fs::remove_file(self.root.join("addons").join(format!("{id}.tbaddon")))
            .map_err(|e| e.to_string())?;
        self.packages.retain(|p| p.manifest.id != id);
        self.config.disabled.retain(|s| s != id);
        self.config.preferences.remove(id);
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_aural_package_cannot_be_impersonated() {
        let package = official_package(AURAL_ID).unwrap();
        package.validate(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(trusted_native(&package));
        assert_eq!(
            package.manifest.capabilities,
            [toolboks_addon_sdk::Capability::LiveAudio]
        );

        let mut spoof = package;
        spoof.manifest.native_provider = None;
        assert!(validate_provider(&spoof).is_err());
    }
    #[test]
    fn legacy_disk_package_keeps_read_only_permissions_until_update() {
        let legacy: Package = serde_json::from_str(toolboks_disk_manager::LEGACY_PACKAGE).unwrap();
        legacy.validate("0.8.2").unwrap();
        assert!(trusted_native(&legacy));
        assert!(!legacy
            .manifest
            .capabilities
            .contains(&toolboks_addon_sdk::Capability::DriveOptimize));
        let mut forged = legacy.clone();
        forged
            .manifest
            .capabilities
            .push(toolboks_addon_sdk::Capability::DriveOptimize);
        assert!(!trusted_native(&forged));
        let current = official_disk_package();
        assert!(trusted_native(&current));
        assert!(current
            .manifest
            .capabilities
            .contains(&toolboks_addon_sdk::Capability::DriveOptimize));
    }
    #[test]
    fn installation_persistence_and_removal() {
        let root = std::env::temp_dir().join(format!("toolboks-addons-{}", std::process::id()));
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
    fn cannot_impersonate_bundled_addon() {
        let mut r = Registry::load(std::env::temp_dir());
        let mut p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
        p.manifest.id = "toolboks.event-watch".into();
        assert!(r.install(p).is_err());
    }
    #[test]
    fn only_exact_first_party_native_packages_are_accepted() {
        let p = official_disk_package();
        assert!(validate_provider(&p).is_ok());
        let mut spoof = p.clone();
        spoof.manifest.author = "Someone else".into();
        assert!(validate_provider(&spoof).is_err());
        let mut spoof = p;
        spoof.manifest.native_provider = None;
        assert!(validate_provider(&spoof).is_err());
    }
    #[test]
    fn native_activation_package_roundtrips_through_installer() {
        let root =
            std::env::temp_dir().join(format!("toolboks-disk-package-{}", std::process::id()));
        let mut registry = Registry::load(root.clone());
        registry.install(official_disk_package()).unwrap();
        let mut loaded = Registry::load(root.clone());
        assert!(loaded.errors.is_empty());
        assert!(loaded.verified(toolboks_disk_manager::ID));
        assert_eq!(
            loaded.packages[0].manifest.native_provider.as_deref(),
            Some(toolboks_disk_manager::PROVIDER)
        );
        loaded.uninstall(toolboks_disk_manager::ID).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn package_update_preserves_preferences_and_ignores_invalid_files() {
        let root = std::env::temp_dir().join(format!("toolboks-upgrade-{}", std::process::id()));
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
    fn every_builtin_manifest_parses_and_validates() {
        let manifests = builtin_manifests();
        assert_eq!(manifests.len(), BUILTIN_IDS.len());
        let tool_names: Vec<_> = crate::tools::all().iter().map(|t| t.name()).collect();
        for (i, m) in manifests.iter().enumerate() {
            assert_eq!(m.id, BUILTIN_IDS[i]);
            assert_eq!(m.title, tool_names[i], "manifest order matches tool order");
            assert_eq!(m.version, env!("CARGO_PKG_VERSION"));
            assert_eq!(m.min_toolboks_version, env!("CARGO_PKG_VERSION"));
            let package = serde_json::json!({
                "schema_version": 1,
                "manifest": m,
                "pages": [{"id": "home", "title": "Home", "widgets": []}],
            });
            Package::parse(package.to_string().as_bytes(), env!("CARGO_PKG_VERSION")).unwrap();
        }
        assert!(manifests[1]
            .capabilities
            .contains(&toolboks_addon_sdk::Capability::HostMetrics));
    }
    #[test]
    fn disabling_a_builtin_persists_and_enabling_restores_it() {
        let root = std::env::temp_dir().join(format!("toolboks-builtin-{}", std::process::id()));
        let mut r = Registry::load(root.clone());
        for id in BUILTIN_IDS {
            assert!(r.enabled(id), "{id} starts enabled");
        }
        r.set_enabled("toolboks.resource-monitor", false).unwrap();
        let mut reloaded = Registry::load(root.clone());
        assert!(!reloaded.enabled("toolboks.resource-monitor"));
        assert!(reloaded.enabled("toolboks.memory-cleaner"));
        reloaded
            .set_enabled("toolboks.resource-monitor", true)
            .unwrap();
        assert!(Registry::load(root.clone()).enabled("toolboks.resource-monitor"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_system_essentials_settings_are_dropped_without_error() {
        let root = std::env::temp_dir().join(format!("toolboks-legacy-{}", std::process::id()));
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
        for id in BUILTIN_IDS {
            assert!(r.enabled(id));
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn local_packages_cannot_take_old_or_new_builtin_ids() {
        let mut r = Registry::load(std::env::temp_dir());
        for id in [LEGACY_BASE_ID, "toolboks.debloater"] {
            let mut p = Package::parse(SDK_EXAMPLE.as_bytes(), "0.8.2").unwrap();
            p.manifest.id = id.into();
            assert!(r.install(p).is_err());
        }
    }
}
