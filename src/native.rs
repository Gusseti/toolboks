//! Host-linked native providers. Packages select a provider; they never load arbitrary code.
use crate::addons::Registry;
use std::collections::BTreeMap;
use toolboks_addon_sdk::NativeAddon;
mod aural;

#[derive(Default)]
pub struct NativeHost {
    active: BTreeMap<String, Box<dyn NativeAddon>>,
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
    pub fn tick(&mut self, ctx: &eframe::egui::Context, registry: &Registry) {
        self.active.retain(|provider, addon| {
            let enabled = registry.packages.iter().any(|p| {
                p.manifest.native_provider.as_ref() == Some(provider)
                    && registry.enabled(&p.manifest.id)
            });
            if enabled {
                addon.tick(ctx);
            } else {
                addon.cancel();
            }
            enabled
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Probe(Arc<AtomicBool>);
    impl NativeAddon for Probe {
        fn ui(
            &mut self,
            _: &mut eframe::egui::Ui,
            _: &mut BTreeMap<String, serde_json::Value>,
        ) -> bool {
            false
        }
        fn cancel(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    #[test]
    fn disabling_native_addon_cancels_its_worker() {
        let flag = Arc::new(AtomicBool::new(false));
        let mut host = NativeHost::default();
        host.active.insert(
            toolboks_disk_manager::PROVIDER.into(),
            Box::new(Probe(flag.clone())),
        );
        let mut registry =
            Registry::load(std::env::temp_dir().join("toolboks-native-lifecycle-test"));
        registry.packages = vec![crate::addons::official_disk_package()];
        let ctx = eframe::egui::Context::default();
        host.tick(&ctx, &registry);
        assert!(!flag.load(Ordering::Relaxed));
        registry
            .config
            .disabled
            .push(toolboks_disk_manager::ID.into());
        host.tick(&ctx, &registry);
        assert!(flag.load(Ordering::Relaxed));
        assert!(host.active.is_empty());
    }
}
