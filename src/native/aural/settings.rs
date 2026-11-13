use super::*;
use serde::{Deserialize, Serialize};

pub(super) const KEY: &str = "audio_settings_v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DeviceIdentity {
    pub name: String,
}
impl DeviceIdentity {
    pub fn resolve(&self, names: impl Iterator<Item = String>) -> Option<usize> {
        let matches: Vec<_> = names
            .enumerate()
            .filter(|(_, name)| name == &self.name)
            .collect();
        (matches.len() == 1).then(|| matches[0].0)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct DeviceSettings {
    pub enabled: Option<bool>,
    pub gain_db: f32,
    pub volume: f32,
    pub outputs: [Option<DeviceIdentity>; 2],
    pub routes: [bool; 2],
}
impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            enabled: None,
            gain_db: 0.0,
            volume: 50.0,
            outputs: [None, None],
            routes: [false; 2],
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct AudioPreferences {
    pub auto_enable: bool,
    pub remember_devices: bool,
    pub default_input: Option<DeviceIdentity>,
    pub last_input: Option<DeviceIdentity>,
    pub devices: BTreeMap<String, DeviceSettings>,
}
impl AudioPreferences {
    pub(super) fn startup_enabled(&self) -> bool {
        let saved = self
            .remember_devices
            .then(|| {
                self.default_input
                    .as_ref()
                    .or(self.last_input.as_ref())
                    .and_then(|id| self.devices.get(&id.name))
                    .and_then(|profile| profile.enabled)
            })
            .flatten();
        saved.unwrap_or(self.auto_enable)
    }
}
struct Inventory {
    inputs: Vec<Device>,
    outputs: Vec<Device>,
}
pub(super) struct Discovery {
    stop: std::sync::mpsc::Sender<()>,
    inventory: std::sync::mpsc::Receiver<Result<Inventory, String>>,
}
impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}
impl Discovery {
    fn start() -> Self {
        let (stop, stopped) = std::sync::mpsc::channel();
        let (send, inventory) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || loop {
            let host = cpal::default_host();
            let result = (|| {
                Ok(Inventory {
                    inputs: host
                        .input_devices()
                        .map_err(|e| e.to_string())?
                        .map(Device::listed)
                        .collect(),
                    outputs: host
                        .output_devices()
                        .map_err(|e| e.to_string())?
                        .map(Device::listed)
                        .collect(),
                })
            })();
            if let Err(std::sync::mpsc::TrySendError::Disconnected(_)) = send.try_send(result) {
                break;
            }
            if stopped.recv_timeout(Duration::from_secs(2))
                != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                break;
            }
        });
        Self { stop, inventory }
    }
}
impl Aural {
    pub(super) fn request_settings_update(&mut self) {
        self.settings_update_requested = true;
    }
    pub(super) fn begin_ui_preferences(&mut self, prefs: &BTreeMap<String, serde_json::Value>) {
        if !self.preferences_loaded {
            self.load_preferences(prefs);
        }
    }
    fn identity(&self) -> Option<DeviceIdentity> {
        let device = self.inputs.get(self.selected_input?)?;
        (self.inputs.iter().filter(|d| d.name == device.name).count() == 1).then(|| {
            DeviceIdentity {
                name: device.name.clone(),
            }
        })
    }
    fn snapshot(&mut self) {
        if !self.preferences.remember_devices {
            return;
        }
        if let Some(id) = self.profile_input.clone() {
            self.preferences.last_input = Some(id.clone());
            self.preferences.devices.insert(
                id.name.clone(),
                DeviceSettings {
                    enabled: Some(
                        self.input.is_some() || (self.want_input && !self.manual_disabled),
                    ),
                    gain_db: self.gain_db,
                    volume: self.volume,
                    outputs: std::array::from_fn(|i| {
                        self.selected_outputs[i]
                            .and_then(|n| self.outputs.get(n))
                            .map(|d| DeviceIdentity {
                                name: d.name.clone(),
                            })
                            .or_else(|| {
                                self.preferences
                                    .devices
                                    .get(&id.name)
                                    .and_then(|p| p.outputs[i].clone())
                            })
                    }),
                    routes: self.route_intent,
                },
            );
        }
    }
    pub(super) fn finish_ui_preferences(
        &mut self,
        prefs: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        if !self.preferences_allowed {
            return false;
        }
        if self.input.is_some() {
            self.want_input = true;
        }
        for i in 0..2 {
            if self.routes[i].is_some() {
                self.route_intent[i] = true;
            }
        }
        self.profile_input = self.identity().or(self.profile_input.clone());
        self.snapshot();
        if self.input.is_some() && !self.manual_disabled {
            for i in 0..2 {
                if self.route_intent[i]
                    && self.routes[i].is_none()
                    && self.route_errors[i].is_none()
                    && self.selected_outputs[i].is_some()
                {
                    if let Err(error) = self.enable_route(i) {
                        self.route_errors[i] = Some(error);
                    }
                }
            }
        }
        self.ensure_discovery();
        if !prefs.contains_key(KEY) && self.preferences == AudioPreferences::default() {
            return false;
        }
        let value = serde_json::to_value(&self.preferences).unwrap();
        if prefs.get(KEY) == Some(&value) {
            false
        } else {
            prefs.insert(KEY.into(), value);
            true
        }
    }
    pub(super) fn set_default_input(&mut self) {
        self.preferences.default_input = self.identity();
        self.default_was_available = false;
        self.ensure_discovery();
    }
    pub(super) fn clear_default_input(&mut self) {
        self.preferences.default_input = None;
        self.ensure_discovery();
    }
    pub(super) fn default_input_available(&self) -> bool {
        self.preferences
            .default_input
            .as_ref()
            .and_then(|id| id.resolve(self.inputs.iter().map(|d| d.name.clone())))
            .is_some()
    }
    pub(super) fn restore_profile(&mut self) {
        self.profile_input = self.identity();
        self.restore_saved_profile();
    }
    fn restore_saved_profile(&mut self) {
        if !self.preferences.remember_devices {
            return;
        }
        let Some(saved) = self
            .profile_input
            .as_ref()
            .and_then(|id| self.preferences.devices.get(&id.name))
            .cloned()
        else {
            self.gain_db = 0.0;
            self.volume = 50.0;
            self.route_intent = [false; 2];
            return;
        };
        self.gain_db = if saved.gain_db.is_finite() {
            saved.gain_db.clamp(-30.0, 24.0)
        } else {
            0.0
        };
        self.volume = if saved.volume.is_finite() {
            saved.volume.clamp(0.0, 100.0)
        } else {
            50.0
        };
        self.monitor_level
            .store((self.volume / 100.0).to_bits(), Relaxed);
        self.selected_outputs = std::array::from_fn(|i| {
            saved.outputs[i]
                .as_ref()
                .and_then(|id| id.resolve(self.outputs.iter().map(|d| d.name.clone())))
        });
        self.route_intent = saved.routes;
        if let Some(enabled) = saved.enabled {
            self.want_input = enabled;
            self.manual_disabled = !enabled;
        }
    }
    pub(super) fn change_microphone(&mut self) {
        self.snapshot();
        self.cancel();
        self.want_input = false;
        self.manual_disabled = true;
        self.restore_profile();
        self.route_errors = [None, None];
        self.message = None;
    }
    pub(super) fn disable_input(&mut self) {
        self.cancel();
        self.want_input = false;
        self.manual_disabled = true;
    }
    pub(super) fn route_stopped(&mut self, index: usize) {
        self.stop_route(index);
        self.route_intent[index] = false;
    }
    pub(super) fn ensure_discovery(&mut self) {
        let needed = self.allowed
            && self.preferences_allowed
            && (self.preferences.auto_enable
                || self.want_input
                || self.preferences.default_input.is_some());
        if needed && self.discovery.is_none() {
            self.discovery = Some(Discovery::start());
        }
        if !needed {
            self.discovery = None;
        }
    }
    pub(super) fn poll_discovery(&mut self) {
        self.ensure_discovery();
        let result = self
            .discovery
            .as_ref()
            .and_then(|w| w.inventory.try_recv().ok());
        let Some(result) = result else {
            return;
        };
        let fresh = match result {
            Ok(fresh) => fresh,
            Err(error) => {
                self.microphone_refresh_error =
                    Some(format!("Could not refresh audio devices: {error}"));
                return;
            }
        };
        let target = self
            .preferences
            .default_input
            .as_ref()
            .or(self.preferences.last_input.as_ref())
            .cloned();
        let old = self.identity();
        let names: Vec<String> = fresh.inputs.iter().map(|d| d.name.clone()).collect();
        let preferred = target
            .as_ref()
            .and_then(|id| id.resolve(names.iter().cloned()));
        let selected = select_input(
            old.as_ref(),
            self.preferences.default_input.as_ref(),
            self.preferences.last_input.as_ref(),
            self.default_was_available,
            &names,
        );
        self.default_was_available = preferred.is_some();
        let next_id = selected
            .and_then(|i| fresh.inputs.get(i))
            .map(|d| DeviceIdentity {
                name: d.name.clone(),
            });
        let changed = old != next_id;
        if changed {
            self.snapshot();
            self.cancel();
        }
        let output_ids: [Option<DeviceIdentity>; 2] = std::array::from_fn(|i| {
            self.selected_outputs[i]
                .and_then(|n| self.outputs.get(n))
                .map(|d| DeviceIdentity {
                    name: d.name.clone(),
                })
        });
        self.selected_outputs = std::array::from_fn(|i| {
            output_ids[i]
                .as_ref()
                .and_then(|id| id.resolve(fresh.outputs.iter().map(|d| d.name.clone())))
        });
        self.inputs = fresh.inputs;
        self.outputs = fresh.outputs;
        self.selected_input = selected;
        self.loaded = true;
        if changed {
            self.restore_profile();
        } else if self.preferences.remember_devices {
            if let Some(saved) = self
                .profile_input
                .as_ref()
                .and_then(|id| self.preferences.devices.get(&id.name))
            {
                for i in 0..2 {
                    if self.selected_outputs[i].is_none() {
                        self.selected_outputs[i] = saved.outputs[i]
                            .as_ref()
                            .and_then(|id| id.resolve(self.outputs.iter().map(|d| d.name.clone())));
                    }
                }
            }
        }
        if target.is_some() && preferred.is_none() {
            self.microphone_refresh_error =
                Some("Waiting for the saved microphone to reconnect.".into());
        } else {
            self.microphone_refresh_error = None;
        }
        if self.want_input
            && !self.manual_disabled
            && self.input.is_none()
            && self.selected_input.is_some()
        {
            if let Err(error) = self.enable_input() {
                self.message = Some(error);
            }
        }
        if self.input.is_some() {
            for i in 0..2 {
                if self.route_intent[i] && self.routes[i].is_none() {
                    if let Err(error) = self.enable_route(i) {
                        self.route_errors[i] = Some(error);
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn microphone_activation_is_saved_and_restored_per_device_and_on_restart() {
        let mut addon = Aural {
            preferences_allowed: true,
            preferences_loaded: true,
            ..Aural::default()
        };
        addon.preferences.remember_devices = true;
        addon.profile_input = Some(DeviceIdentity {
            name: "Razer".into(),
        });
        addon.want_input = true;
        addon.snapshot();
        addon.profile_input = Some(DeviceIdentity {
            name: "Webcam".into(),
        });
        addon.disable_input();
        addon.snapshot();
        assert_eq!(addon.preferences.devices["Razer"].enabled, Some(true));
        assert_eq!(addon.preferences.devices["Webcam"].enabled, Some(false));

        addon.profile_input = Some(DeviceIdentity {
            name: "Razer".into(),
        });
        addon.restore_saved_profile();
        assert!(addon.want_input && !addon.manual_disabled);
        let mut prefs = BTreeMap::new();
        addon.finish_ui_preferences(&mut prefs);
        assert!(Aural::needs_background_preferences(&prefs));
        let mut restarted = Aural {
            preferences_allowed: true,
            ..Aural::default()
        };
        restarted.load_preferences(&prefs);
        assert!(
            restarted.want_input,
            "saved on state restores without the startup switch"
        );

        addon.disable_input();
        addon.finish_ui_preferences(&mut prefs);
        assert!(!Aural::needs_background_preferences(&prefs));
        let mut restarted = Aural {
            preferences_allowed: true,
            ..Aural::default()
        };
        restarted.load_preferences(&prefs);
        assert!(!restarted.want_input, "saved off state survives restart");
    }

    #[test]
    fn legacy_profiles_keep_startup_behavior_until_activation_is_saved() {
        let mut settings: AudioPreferences = serde_json::from_value(serde_json::json!({
            "auto_enable": true,
            "remember_devices": true,
            "last_input": {"name": "Razer"},
            "devices": {"Razer": {"gain_db": 18.0}}
        }))
        .unwrap();
        assert!(settings.startup_enabled());
        settings.devices.get_mut("Razer").unwrap().enabled = Some(false);
        assert!(!settings.startup_enabled());
        settings.remember_devices = false;
        assert!(
            settings.startup_enabled(),
            "device states apply only when enabled"
        );
    }

    #[test]
    fn defaults_are_opt_in_and_opening_settings_does_not_write() {
        let mut a = Aural {
            preferences_allowed: true,
            ..Aural::default()
        };
        let mut prefs = BTreeMap::new();
        a.begin_ui_preferences(&prefs);
        assert!(!a.preferences.auto_enable);
        assert!(!a.preferences.remember_devices);
        assert!(!a.finish_ui_preferences(&mut prefs));
        assert!(prefs.is_empty());
        assert!(a.discovery.is_none());
    }
    #[test]
    fn preferences_round_trip_preserves_absent_default_and_separate_profiles() {
        let mut settings = AudioPreferences {
            remember_devices: true,
            default_input: Some(DeviceIdentity {
                name: "Disconnected Razer".into(),
            }),
            ..AudioPreferences::default()
        };
        settings.devices.insert(
            "Razer".into(),
            DeviceSettings {
                gain_db: 18.0,
                routes: [false, true],
                ..DeviceSettings::default()
            },
        );
        settings.devices.insert(
            "Webcam".into(),
            DeviceSettings {
                gain_db: 3.0,
                ..DeviceSettings::default()
            },
        );
        let value = serde_json::to_value(&settings).unwrap();
        let decoded: AudioPreferences = serde_json::from_value(value).unwrap();
        assert_eq!(settings, decoded);
        assert_eq!(decoded.devices["Razer"].gain_db, 18.0);
        assert_eq!(decoded.devices["Webcam"].gain_db, 3.0);
        assert_eq!(
            decoded.default_input.unwrap().resolve(std::iter::empty()),
            None
        );
    }
    #[test]
    fn identity_never_chooses_an_ambiguous_name_or_another_endpoint() {
        let id = DeviceIdentity { name: "Mic".into() };
        assert_eq!(
            id.resolve(["Other".to_string(), "Mic".to_string()].into_iter()),
            Some(1)
        );
        assert_eq!(
            id.resolve(["Mic".to_string(), "Mic".to_string()].into_iter()),
            None
        );
        assert_eq!(id.resolve(["Other".to_string()].into_iter()), None);
    }
    #[test]
    fn missing_permission_ignores_startup_and_manual_disable_suppresses_reopen() {
        let mut a = Aural::default();
        let prefs = BTreeMap::from([(KEY.into(), serde_json::json!({"auto_enable":true}))]);
        a.load_preferences(&prefs);
        assert!(!a.want_input);
        assert!(a.discovery.is_none());
        a.want_input = true;
        a.route_intent = [false, true];
        a.disable_input();
        assert_eq!(a.route_intent, [false, true]);
        assert!(a.manual_disabled);
        assert!(!a.want_input);
    }
}

/// Selection policy has no audio handles so reconnect behavior can be verified without hardware.
fn select_input(
    current: Option<&DeviceIdentity>,
    default: Option<&DeviceIdentity>,
    last: Option<&DeviceIdentity>,
    default_was_available: bool,
    names: &[String],
) -> Option<usize> {
    let resolve = |id: &DeviceIdentity| id.resolve(names.iter().cloned());
    let current = current.and_then(resolve);
    let target = default.or(last);
    let preferred = target.and_then(resolve);
    if default.is_some() {
        if preferred.is_some() && !default_was_available {
            preferred
        } else {
            current.or(preferred)
        }
    } else {
        current.or(preferred).or_else(|| {
            if target.is_none() {
                names
                    .iter()
                    .enumerate()
                    .find(|(_, name)| {
                        !virtual_cable(name) && names.iter().filter(|n| *n == *name).count() == 1
                    })
                    .map(|(i, _)| i)
            } else {
                None
            }
        })
    }
}
#[cfg(test)]
mod reconnect_policy_tests {
    use super::*;
    fn id(name: &str) -> DeviceIdentity {
        DeviceIdentity { name: name.into() }
    }
    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn absent_default_waits_at_startup_but_keeps_manual_fallback() {
        assert_eq!(
            select_input(None, Some(&id("Razer")), None, false, &names(&["Webcam"])),
            None
        );
        assert_eq!(
            select_input(
                Some(&id("Webcam")),
                Some(&id("Razer")),
                None,
                false,
                &names(&["Webcam"])
            ),
            Some(0)
        );
    }
    #[test]
    fn reconnect_switches_to_default_once_and_keeps_manual_selection_afterward() {
        let inventory = names(&["Webcam", "Razer"]);
        assert_eq!(
            select_input(
                Some(&id("Webcam")),
                Some(&id("Razer")),
                None,
                false,
                &inventory
            ),
            Some(1)
        );
        assert_eq!(
            select_input(
                Some(&id("Webcam")),
                Some(&id("Razer")),
                None,
                true,
                &inventory
            ),
            Some(0)
        );
        assert_eq!(
            select_input(
                None,
                Some(&id("Razer")),
                None,
                false,
                &names(&["Razer", "Razer"])
            ),
            None
        );
    }
    #[test]
    fn last_microphone_wins_startup_and_absent_last_never_falls_back() {
        assert_eq!(
            select_input(
                None,
                None,
                Some(&id("Razer")),
                false,
                &names(&["Webcam", "Razer"])
            ),
            Some(1)
        );
        assert_eq!(
            select_input(None, None, Some(&id("Razer")), false, &names(&["Webcam"])),
            None
        );
    }
    #[test]
    fn actual_profile_snapshot_and_restart_keep_gain_routes_and_device_separation() {
        let mut a = Aural {
            preferences_allowed: true,
            preferences_loaded: true,
            ..Aural::default()
        };
        a.preferences.remember_devices = true;
        a.profile_input = Some(id("Razer"));
        a.gain_db = 18.0;
        a.route_intent = [false, true];
        a.snapshot();
        a.profile_input = Some(id("Webcam"));
        a.gain_db = 3.0;
        a.route_intent = [true, false];
        a.snapshot();
        let mut prefs = BTreeMap::new();
        // snapshot each saved profile before invoking the same persistence path used by UI.
        a.finish_ui_preferences(&mut prefs);
        let mut restarted = Aural {
            preferences_allowed: true,
            ..Aural::default()
        };
        restarted.load_preferences(&prefs);
        assert_eq!(restarted.preferences.devices["Razer"].gain_db, 18.0);
        assert_eq!(restarted.preferences.devices["Razer"].routes, [false, true]);
        assert_eq!(restarted.preferences.devices["Webcam"].gain_db, 3.0);
        assert_eq!(
            restarted.preferences.devices["Webcam"].routes,
            [true, false]
        );
        assert!(restarted.discovery.is_none());
    }
}
