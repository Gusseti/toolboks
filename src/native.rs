//! Host-linked native providers. Packages select a provider; they never load arbitrary code.
use crate::addons::Registry;
use std::collections::BTreeMap;
use toolboks_addon_sdk::NativeAddon;
mod aural;
mod macros;
mod profiles;

#[derive(Default)]
pub struct NativeHost {
    active: BTreeMap<String, Box<dyn NativeAddon>>,
    foreground: Option<String>,
}

impl NativeHost {
    pub fn pages(provider: &str) -> &'static [&'static str] {
        if provider == toolboks_disk_manager::PROVIDER {
            &toolboks_disk_manager::PAGES
        } else if provider == crate::addons::MACRO_ID {
            &macros::PAGES
        } else {
            &[]
        }
    }

    pub fn page_index(&self, provider: &str) -> usize {
        self.active
            .get(provider)
            .map_or(0, |addon| addon.page_index())
    }

    pub fn navigate(&mut self, provider: &str, page: &str) {
        if let Some(p) = self.provider(provider) {
            p.navigate(page);
        }
    }
    pub fn take_host_request(&mut self, provider: &str) -> Option<toolboks_addon_sdk::HostRequest> {
        self.provider(provider).and_then(|p| p.take_host_request())
    }
    fn provider(&mut self, provider: &str) -> Option<&mut Box<dyn NativeAddon>> {
        let create: fn() -> Box<dyn NativeAddon> = match provider {
            toolboks_disk_manager::PROVIDER => {
                || Box::new(toolboks_disk_manager::DiskManager::default())
            }
            crate::addons::AURAL_ID => || Box::new(aural::Aural::default()),
            crate::addons::PROFILES_ID => || Box::new(profiles::Gaming::default()),
            crate::addons::MACRO_ID => || Box::new(macros::MacroManager::default()),
            _ => return None,
        };
        Some(self.active.entry(provider.into()).or_insert_with(create))
    }
    pub fn tutorial_steps(&mut self, provider: &str) -> Vec<toolboks_addon_sdk::TutorialStep> {
        self.provider(provider)
            .map(|p| p.tutorial_steps())
            .unwrap_or_default()
    }
    pub fn tutorial_location(&mut self, provider: &str) -> String {
        self.provider(provider)
            .map(|p| p.tutorial_location())
            .unwrap_or_default()
    }
    pub fn tutorial_navigate(&mut self, provider: &str, page: &str) {
        if let Some(p) = self.provider(provider) {
            p.tutorial_navigate(page);
        }
    }
    pub fn tutorial_start(&mut self, provider: &str) {
        if let Some(p) = self.provider(provider) {
            p.tutorial_start();
        }
    }
    pub fn tutorial_finish(&mut self, provider: &str) {
        if let Some(p) = self.provider(provider) {
            p.tutorial_finish();
        }
    }
    pub fn ui(
        &mut self,
        provider: &str,
        ui: &mut eframe::egui::Ui,
        capabilities: &[toolboks_addon_sdk::Capability],
        prefs: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        let Some(addon) = self.provider(provider) else {
            ui.label("This native provider is not available in this Toolboks build.");
            return false;
        };
        addon.set_capabilities(capabilities);
        addon.ui(ui, prefs)
    }
    pub fn show(&mut self, provider: Option<&str>, registry: &Registry) {
        if self.foreground.as_deref() != provider {
            if let Some(addon) = self
                .foreground
                .as_deref()
                .and_then(|old| self.active.get_mut(old))
            {
                addon.suspend();
            }
            if let Some(addon) = provider.and_then(|new| self.active.get_mut(new)) {
                addon.resume();
            }
            self.foreground = provider.map(str::to_owned);
        }
        self.active.retain(|provider, addon| {
            let enabled = registry.packages.iter().any(|p| {
                p.manifest.native_provider.as_ref() == Some(provider)
                    && registry.enabled(&p.manifest.id)
            });
            let retain = enabled
                && (self.foreground.as_ref() == Some(provider) || addon.has_background_work());
            if !retain {
                addon.cancel();
            }
            retain
        });
    }

    pub fn sync_preferences(&mut self, registry: &Registry) {
        use toolboks_addon_sdk::Capability;
        if let Some(package) = registry.packages.iter().find(|p| {
            p.manifest.id == crate::addons::MACRO_ID
                && registry.enabled(&p.manifest.id)
                && crate::addons::trusted_native(p)
        }) {
            let prefs = registry
                .config
                .preferences
                .get(&package.manifest.id)
                .cloned()
                .unwrap_or_default();
            if macros::MacroManager::needs_background_preferences(&prefs)
                || self.active.contains_key(crate::addons::MACRO_ID)
            {
                if let Some(addon) = self.provider(crate::addons::MACRO_ID) {
                    addon.set_capabilities(&package.manifest.capabilities);
                    addon.load_preferences(&prefs);
                }
            }
        }
        let package = registry.packages.iter().find(|p| {
            p.manifest.native_provider.as_deref() == Some(crate::addons::AURAL_ID)
                && registry.enabled(&p.manifest.id)
                && crate::addons::trusted_native(p)
        });
        if let Some(package) = package {
            let caps = &package.manifest.capabilities;
            let prefs = registry
                .config
                .preferences
                .get(&package.manifest.id)
                .cloned()
                .unwrap_or_default();
            let startup = caps.contains(&Capability::LiveAudio)
                && caps.contains(&Capability::LocalPreferences)
                && aural::Aural::needs_background_preferences(&prefs);
            if startup || self.active.contains_key(crate::addons::AURAL_ID) {
                if let Some(addon) = self.provider(crate::addons::AURAL_ID) {
                    addon.set_capabilities(caps);
                    addon.load_preferences(&prefs);
                }
            }
        }
    }

    pub fn collect_preferences(&mut self, registry: &mut Registry) -> bool {
        let mut changed = false;
        for (provider, addon) in &mut self.active {
            if let Some(package) = registry.packages.iter().find(|p| {
                p.manifest.native_provider.as_ref() == Some(provider)
                    && registry.enabled(&p.manifest.id)
                    && crate::addons::trusted_native(p)
                    && p.manifest
                        .capabilities
                        .contains(&toolboks_addon_sdk::Capability::LocalPreferences)
            }) {
                changed |= addon.save_preferences(
                    registry
                        .config
                        .preferences
                        .entry(package.manifest.id.clone())
                        .or_default(),
                );
            }
        }
        changed
    }

    pub fn tick(&mut self, ctx: &eframe::egui::Context) {
        if self
            .active
            .get(crate::addons::MACRO_ID)
            .is_some_and(|a| a.has_background_work())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        // Consume discovery results and service live-stream failures even when
        // Home is idle and the tray heartbeat is disabled.
        if self
            .active
            .get(crate::addons::AURAL_ID)
            .is_some_and(|a| a.has_background_work())
        {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
        for (provider, addon) in &mut self.active {
            if self.foreground.as_ref() == Some(provider) {
                addon.tick(ctx);
            } else {
                addon.tick_background();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Probe {
        cancelled: Arc<AtomicBool>,
        background: bool,
    }
    impl NativeAddon for Probe {
        fn ui(
            &mut self,
            _: &mut eframe::egui::Ui,
            _: &mut BTreeMap<String, serde_json::Value>,
        ) -> bool {
            false
        }
        fn cancel(&mut self) {
            self.cancelled.store(true, Ordering::Relaxed);
        }
        fn has_background_work(&self) -> bool {
            self.background
        }
    }
    #[test]
    fn disabling_native_addon_cancels_its_worker() {
        let flag = Arc::new(AtomicBool::new(false));
        let mut host = NativeHost::default();
        host.active.insert(
            toolboks_disk_manager::PROVIDER.into(),
            Box::new(Probe {
                cancelled: flag.clone(),
                background: true,
            }),
        );
        let mut registry =
            Registry::load(std::env::temp_dir().join("toolboks-native-lifecycle-test"));
        registry.packages = vec![crate::addons::official_disk_package()];
        host.show(Some(toolboks_disk_manager::PROVIDER), &registry);
        assert!(!flag.load(Ordering::Relaxed));
        registry
            .config
            .disabled
            .push(toolboks_disk_manager::ID.into());
        host.show(Some(toolboks_disk_manager::PROVIDER), &registry);
        assert!(flag.load(Ordering::Relaxed));
        assert!(host.active.is_empty());
    }

    #[test]
    fn switching_releases_idle_native_addons_but_keeps_live_audio() {
        let mut host = NativeHost::default();
        let mut registry =
            Registry::load(std::env::temp_dir().join("toolboks-native-lifecycle-test"));
        registry.packages = vec![
            crate::addons::official_disk_package(),
            crate::addons::official_package(crate::addons::AURAL_ID).unwrap(),
        ];
        let disk_cancelled = Arc::new(AtomicBool::new(false));
        let audio_cancelled = Arc::new(AtomicBool::new(false));
        host.active.insert(
            toolboks_disk_manager::PROVIDER.into(),
            Box::new(Probe {
                cancelled: disk_cancelled.clone(),
                background: false,
            }),
        );
        host.show(Some(toolboks_disk_manager::PROVIDER), &registry);
        host.active.insert(
            crate::addons::AURAL_ID.into(),
            Box::new(Probe {
                cancelled: audio_cancelled.clone(),
                background: true,
            }),
        );
        host.show(Some(crate::addons::AURAL_ID), &registry);
        host.show(None, &registry);
        assert!(disk_cancelled.load(Ordering::Relaxed));
        assert!(!audio_cancelled.load(Ordering::Relaxed));
        assert_eq!(host.active.len(), 1);
    }

    #[test]
    fn saved_audio_startup_initializes_without_opening_page_and_disabled_addon_stops_it() {
        let root =
            std::env::temp_dir().join(format!("toolboks-audio-startup-{}", std::process::id()));
        let mut registry = Registry::load(root.clone());
        registry
            .install(crate::addons::official_package(crate::addons::AURAL_ID).unwrap())
            .unwrap();
        registry.config.preferences.insert(
            crate::addons::AURAL_ID.into(),
            BTreeMap::from([(
                "audio_settings_v1".into(),
                serde_json::json!({
                    "auto_enable":true,
                    "default_input":{"name":"Missing test microphone"}
                }),
            )]),
        );
        registry.save().unwrap();
        let mut reloaded = Registry::load(root.clone());
        let mut host = NativeHost::default();
        host.sync_preferences(&reloaded);
        assert!(host.foreground.is_none());
        assert!(host.active.contains_key(crate::addons::AURAL_ID));
        assert!(host.active[crate::addons::AURAL_ID].has_background_work());
        host.show(None, &reloaded);
        assert!(host.active.contains_key(crate::addons::AURAL_ID));
        reloaded
            .config
            .disabled
            .push(crate::addons::AURAL_ID.into());
        host.show(None, &reloaded);
        host.sync_preferences(&reloaded);
        assert!(host.active.is_empty());
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn audio_background_startup_requires_saved_choice_and_both_capabilities() {
        let root = std::env::temp_dir().join(format!(
            "toolboks-audio-startup-gates-{}",
            std::process::id()
        ));
        let mut registry = Registry::load(root);
        registry.packages = vec![crate::addons::official_package(crate::addons::AURAL_ID).unwrap()];
        let mut host = NativeHost::default();
        host.sync_preferences(&registry);
        assert!(host.active.is_empty(), "default-off must remain lazy");
        registry.config.preferences.insert(
            crate::addons::AURAL_ID.into(),
            BTreeMap::from([(
                "audio_settings_v1".into(),
                serde_json::json!({
                    "auto_enable":false,
                    "default_input":{"name":"Missing test microphone"}
                }),
            )]),
        );
        registry.packages =
            vec![serde_json::from_str(crate::addons::LEGACY_AURAL_PACKAGE).unwrap()];
        host.sync_preferences(&registry);
        assert!(
            host.active.is_empty(),
            "legacy package has no saved-preferences grant"
        );
        registry.packages = vec![crate::addons::official_package(crate::addons::AURAL_ID).unwrap()];
        host.sync_preferences(&registry);
        assert!(
            host.active.contains_key(crate::addons::AURAL_ID),
            "saved default needs reconnect discovery even with auto-enable off"
        );
        drop(host);
    }
}
