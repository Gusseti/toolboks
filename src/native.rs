//! Host-linked native providers. Packages select a provider; they never load arbitrary code.
use crate::addons::Registry;
use std::collections::BTreeMap;
use toolboks_addon_sdk::NativeAddon;
mod aural;
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

    pub fn tick(&mut self, ctx: &eframe::egui::Context) {
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
}
