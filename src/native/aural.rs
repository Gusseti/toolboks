//! Aural's live audio controls inside the Toolboks workspace.
mod routing;
mod settings;
use settings::{AudioPreferences, DeviceIdentity, Discovery};

use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering::Relaxed},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui::{self, RichText};
use toolboks_addon_sdk::{tutorial, Capability, NativeAddon, TutorialStep};

use crate::ui as look;

const HISTORY: usize = 160;
const INTERVAL: Duration = Duration::from_millis(50);
const VB_CABLE: &str = "https://vb-audio.com/Cable/";
const ROUTE_LABELS: [&str; 2] = ["Hear myself", "Send to other apps"];

type AudioError = Arc<Mutex<Option<String>>>;

#[derive(Default)]
struct Signal {
    gain: AtomicU32,
    metering: AtomicBool,
    peaks: [AtomicU32; 2],
    clipped: AtomicBool,
    routes: routing::Routes,
    error: AudioError,
}

fn db(amplitude: f32) -> f32 {
    if amplitude > 0.0 {
        (20.0 * amplitude.log10()).max(-60.0)
    } else {
        -60.0
    }
}

fn gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn process<T: cpal::Sample>(samples: &[T], channels: usize, signal: &Signal)
where
    f32: cpal::FromSample<T>,
{
    let gain = f32::from_bits(signal.gain.load(Relaxed));
    if signal.metering.load(Relaxed) {
        let mut peaks = [0.0_f32; 2];
        let mut clipped = false;
        for (index, sample) in samples.iter().enumerate() {
            let level = (sample.to_sample::<f32>() * gain).abs();
            if let Some(peak) = peaks.get_mut(index % channels) {
                *peak = peak.max(level);
            }
            clipped |= level >= 1.0;
        }
        if channels == 1 {
            peaks[1] = peaks[0];
        }
        for (slot, peak) in signal.peaks.iter().zip(peaks) {
            slot.fetch_max(peak.to_bits(), Relaxed);
        }
        if clipped {
            signal.clipped.store(true, Relaxed);
        }
    }
    routing::forward(samples, channels, gain, &signal.routes);
}

struct Input {
    _stream: cpal::Stream,
    config: cpal::StreamConfig,
    signal: Arc<Signal>,
}

impl Input {
    fn open(device: &cpal::Device, gain_db: f32) -> Result<Self, String> {
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = usize::from(config.channels);
        if channels == 0 || config.sample_rate.0 == 0 {
            return Err("The microphone reported an invalid format.".into());
        }
        let signal = Arc::new(Signal::default());
        signal.gain.store(gain(gain_db).to_bits(), Relaxed);
        signal.metering.store(true, Relaxed);
        macro_rules! build {
            ($t:ty) => {{
                let samples = signal.clone();
                let error = signal.error.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[$t], _| process(data, channels, &samples),
                    move |e| {
                        *error.lock().unwrap() =
                            Some(format!("Microphone disconnected: {e}. Enable it again."))
                    },
                    None,
                )
            }};
        }
        let stream = match format {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::F64 => build!(f64),
            cpal::SampleFormat::I8 => build!(i8),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::I32 => build!(i32),
            cpal::SampleFormat::I64 => build!(i64),
            cpal::SampleFormat::U8 => build!(u8),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::U32 => build!(u32),
            cpal::SampleFormat::U64 => build!(u64),
            _ => return Err("This microphone format is unsupported.".into()),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            config,
            signal,
        })
    }
}

struct Output {
    _stream: cpal::Stream,
    error: AudioError,
}

struct Device {
    device: cpal::Device,
    name: String,
}

impl Device {
    fn listed(device: cpal::Device) -> Self {
        let name = device.name().unwrap_or_else(|_| "Unnamed device".into());
        Self { device, name }
    }

    fn same_endpoint(&self, other: &Self) -> bool {
        #[cfg(windows)]
        {
            // CPAL's WASAPI equality compares IMMDevice endpoint IDs, not names.
            #[allow(irrefutable_let_patterns)]
            if let (
                cpal::platform::DeviceInner::Wasapi(a),
                cpal::platform::DeviceInner::Wasapi(b),
            ) = (self.device.as_inner(), other.device.as_inner())
            {
                return a == b;
            }
        }
        // Other backends expose no portable identity. Reconciliation below rejects
        // ambiguous names rather than switching to an arbitrary microphone.
        self.name == other.name
    }
}

fn refreshed_selection<T>(
    old: &[T],
    selected: Option<usize>,
    fresh: &[T],
    same: impl Fn(&T, &T) -> bool,
) -> Option<usize> {
    let previous = old.get(selected?)?;
    if old.iter().filter(|item| same(previous, item)).count() != 1 {
        return None;
    }
    let mut matches = fresh
        .iter()
        .enumerate()
        .filter(|(_, item)| same(previous, item));
    let index = matches.next()?.0;
    matches.next().is_none().then_some(index)
}

fn virtual_cable(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("virtual cable")
        || name.contains("vb-audio")
        || name.contains("cable input")
        || name.contains("cable output")
        || name.contains("voicemeeter")
}

pub(super) struct Aural {
    allowed: bool,
    ui_active: bool,
    preferences_allowed: bool,
    preferences_loaded: bool,
    preferences: AudioPreferences,
    profile_input: Option<DeviceIdentity>,
    discovery: Option<Discovery>,
    want_input: bool,
    manual_disabled: bool,
    default_was_available: bool,
    route_intent: [bool; 2],
    settings_update_requested: bool,
    loaded: bool,
    inputs: Vec<Device>,
    outputs: Vec<Device>,
    selected_input: Option<usize>,
    selected_outputs: [Option<usize>; 2],
    input: Option<Input>,
    routes: [Option<Output>; 2],
    route_errors: [Option<String>; 2],
    message: Option<String>,
    microphone_refresh_error: Option<String>,
    gain_db: f32,
    volume: f32,
    monitor_level: Arc<AtomicU32>,
    levels: [f32; 2],
    history: [VecDeque<f32>; 2],
    last_sample: Instant,
}

impl Default for Aural {
    fn default() -> Self {
        Self {
            allowed: false,
            ui_active: false,
            preferences_allowed: false,
            preferences_loaded: false,
            preferences: AudioPreferences::default(),
            profile_input: None,
            discovery: None,
            want_input: false,
            manual_disabled: false,
            default_was_available: false,
            route_intent: [false; 2],
            settings_update_requested: false,
            loaded: false,
            inputs: Vec::new(),
            outputs: Vec::new(),
            selected_input: None,
            selected_outputs: [None; 2],
            input: None,
            routes: [None, None],
            route_errors: [None, None],
            message: None,
            microphone_refresh_error: None,
            gain_db: 0.0,
            volume: 50.0,
            monitor_level: Arc::new(AtomicU32::new(0.5_f32.to_bits())),
            levels: [-60.0; 2],
            history: std::array::from_fn(|_| VecDeque::with_capacity(HISTORY)),
            last_sample: Instant::now(),
        }
    }
}

impl Aural {
    pub(super) fn needs_background_preferences(
        prefs: &BTreeMap<String, serde_json::Value>,
    ) -> bool {
        prefs
            .get(settings::KEY)
            .and_then(|value| serde_json::from_value::<AudioPreferences>(value.clone()).ok())
            .is_some_and(|settings| settings.startup_enabled() || settings.default_input.is_some())
    }

    fn refresh_microphones(&mut self) {
        // Enumerate on demand with a fresh host. No cached device list or resident
        // watcher is kept, and existing streams retain their original endpoints.
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .map(|devices| devices.map(Device::listed).collect())
            .map_err(|error| format!("Could not refresh microphones: {error}. Try again."));
        self.apply_microphone_refresh(devices);
    }

    fn apply_microphone_refresh(&mut self, devices: Result<Vec<Device>, String>) {
        let fresh = match devices {
            Ok(devices) => devices,
            Err(error) => {
                self.microphone_refresh_error = Some(error);
                return;
            }
        };
        let selected = refreshed_selection(
            &self.inputs,
            self.selected_input,
            &fresh,
            Device::same_endpoint,
        );
        self.microphone_refresh_error = (self.selected_input.is_some() && selected.is_none())
            .then(|| "The selected microphone is unavailable. Choose a microphone.".into());
        self.inputs = fresh;
        self.selected_input = selected;
    }

    fn refresh(&mut self) {
        self.cancel();
        self.loaded = true;
        self.message = None;
        self.microphone_refresh_error = None;
        self.route_errors = [None, None];
        let host = cpal::default_host();
        let default_in = host.default_input_device().and_then(|d| d.name().ok());
        let default_out = host.default_output_device().and_then(|d| d.name().ok());
        self.inputs = match host.input_devices() {
            Ok(list) => list.map(Device::listed).collect(),
            Err(error) => {
                self.message = Some(format!("Could not list microphones: {error}"));
                Vec::new()
            }
        };
        self.outputs = match host.output_devices() {
            Ok(list) => list.map(Device::listed).collect(),
            Err(error) => {
                self.message
                    .get_or_insert_with(|| format!("Could not list audio outputs: {error}"));
                Vec::new()
            }
        };
        self.selected_input = self
            .inputs
            .iter()
            .position(|d| Some(&d.name) == default_in.as_ref())
            .filter(|&i| !virtual_cable(&self.inputs[i].name))
            .or_else(|| self.inputs.iter().position(|d| !virtual_cable(&d.name)));
        self.selected_outputs = [
            self.outputs
                .iter()
                .position(|d| Some(&d.name) == default_out.as_ref() && !virtual_cable(&d.name))
                .or_else(|| self.outputs.iter().position(|d| !virtual_cable(&d.name))),
            self.outputs.iter().position(|d| virtual_cable(&d.name)),
        ];
        if let Some(target) = self
            .preferences
            .default_input
            .as_ref()
            .or(self.preferences.last_input.as_ref())
        {
            self.selected_input = target.resolve(self.inputs.iter().map(|d| d.name.clone()));
        }
        self.restore_profile();
    }

    fn enable_input(&mut self) -> Result<(), String> {
        if !self.allowed {
            return Err("Live audio access is unavailable.".into());
        }
        let device = self
            .selected_input
            .and_then(|i| self.inputs.get(i))
            .ok_or("Choose a microphone first.")?;
        self.input = Some(Input::open(&device.device, self.gain_db)
            .map_err(|e| format!("Could not open the microphone: {e}. Check the device and Windows microphone access."))?);
        self.want_input = true;
        self.manual_disabled = false;
        if let Some(input) = &self.input {
            input.signal.metering.store(self.ui_active, Relaxed);
        }
        self.message = None;
        self.last_sample = Instant::now();
        Ok(())
    }

    fn stop_route(&mut self, index: usize) {
        if let Some(input) = &self.input {
            input.signal.routes.lock().unwrap()[index] = None;
        }
        self.routes[index] = None;
    }

    fn enable_route(&mut self, index: usize) -> Result<(), String> {
        let output = self.selected_outputs[index]
            .and_then(|i| self.outputs.get(i))
            .ok_or("Choose an audio output first.")?;
        if self.routes[1 - index].is_some()
            && self.selected_outputs[index] == self.selected_outputs[1 - index]
        {
            return Err("Choose a separate output for each route.".into());
        }
        if index == 1
            && self
                .selected_input
                .and_then(|i| self.inputs.get(i))
                .is_some_and(|d| virtual_cable(&d.name))
        {
            return Err(
                "Choose your physical microphone before sending audio to a virtual cable.".into(),
            );
        }
        let device = output.device.clone();
        let started_input = self.input.is_none();
        if started_input {
            self.enable_input()?;
        }
        let input = self.input.as_ref().ok_or("Enable the microphone first.")?;
        let error = AudioError::default();
        let level = if index == 0 {
            self.monitor_level.clone()
        } else {
            Arc::new(AtomicU32::new(1.0_f32.to_bits()))
        };
        let (stream, bridge) =
            match routing::open(&device, input.config.sample_rate.0, level, error.clone()) {
                Ok(route) => route,
                Err(e) => {
                    if started_input {
                        self.cancel();
                    }
                    return Err(format!(
                        "Could not open the output: {e}. Choose another device or refresh."
                    ));
                }
            };
        input.signal.routes.lock().unwrap()[index] = Some(bridge);
        self.route_intent[index] = true;
        self.routes[index] = Some(Output {
            _stream: stream,
            error,
        });
        Ok(())
    }

    fn sample(&mut self) {
        let Some(input) = &self.input else {
            return;
        };
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample);
        if elapsed < INTERVAL {
            return;
        }
        self.last_sample = now;
        for (channel, level) in self.levels.iter_mut().enumerate() {
            let peak = f32::from_bits(input.signal.peaks[channel].swap(0, Relaxed));
            *level = db(peak).max(*level - elapsed.as_secs_f32() * 48.0);
            let history = &mut self.history[channel];
            if history.len() == HISTORY {
                history.pop_front();
            }
            history.push_back(*level + 60.0);
        }
    }
}

fn muted(ui: &mut egui::Ui, message: impl Into<String>) {
    ui.label(RichText::new(message.into()).color(look::MUTED));
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(look::TEXT_SECTION).strong());
}

fn picker(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    devices: &[Device],
    selected: &mut Option<usize>,
    empty: &str,
    filter: impl Fn(&Device) -> bool,
) -> bool {
    let old = *selected;
    let label = old
        .and_then(|i| devices.get(i))
        .map_or(empty, |d| d.name.as_str());
    egui::ComboBox::from_id_salt(id)
        .width((ui.available_width() - 22.0).clamp(110.0, 260.0))
        .selected_text(label)
        .truncate()
        .show_ui(ui, |ui| {
            for (index, device) in devices.iter().enumerate().filter(|(_, d)| filter(d)) {
                ui.selectable_value(selected, Some(index), &device.name);
            }
        });
    *selected != old
}

fn audio_setting_switch(ui: &mut egui::Ui, value: &mut bool, label: &str) {
    ui.horizontal_top(|ui| {
        let response = look::toggle_switch(ui, value);
        let text = ui.add(egui::Label::new(label).wrap().sense(egui::Sense::click()));
        if text.clicked() {
            *value = !*value;
        }
        let response = response.labelled_by(text.id);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *value, label)
        });
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect.expand(2.0),
                look::CONTROL_RADIUS,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Outside,
            );
        }
    });
}

fn refreshing_picker(
    ui: &mut egui::Ui,
    id: &str,
    label: String,
    contents: impl FnOnce(&mut egui::Ui, bool) -> bool,
) -> bool {
    let enabled = ui.is_enabled();
    let was_open = egui::ComboBox::is_open(ui.ctx(), ui.make_persistent_id(egui::IdSalt::new(id)));
    egui::ComboBox::from_id_salt(id)
        .width((ui.available_width() - 22.0).clamp(110.0, 260.0))
        .selected_text(label)
        .truncate()
        .show_ui(ui, |ui| {
            // The popup's initial sizing pass disables its child Ui, but opening
            // still needs the fresh list. Only the parent control gates refresh.
            let opening = enabled && !was_open;
            contents(ui, opening)
        })
        .inner
        .unwrap_or(false)
}

impl Aural {
    fn microphone_picker(&mut self, ui: &mut egui::Ui) -> bool {
        let label = self
            .selected_input
            .and_then(|index| self.inputs.get(index))
            .map_or("Choose a microphone", |device| device.name.as_str())
            .to_owned();
        refreshing_picker(ui, "aural-input-picker", label, |ui, opening| {
            if opening {
                self.refresh_microphones();
            }
            let mut changed = false;
            for (index, device) in self.inputs.iter().enumerate() {
                changed |= ui
                    .selectable_value(&mut self.selected_input, Some(index), &device.name)
                    .changed();
            }
            if self.inputs.is_empty() {
                muted(
                    ui,
                    "No microphones found. Connect one, then reopen this list.",
                );
            }
            changed
        })
    }

    fn input_ui(&mut self, ui: &mut egui::Ui, interactive: bool) {
        tutorial::region(ui, "aural-input", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                section(ui, "Microphone");
                muted(
                    ui,
                    if self.input.is_some() {
                        "Input live"
                    } else {
                        "Input off"
                    },
                );
                ui.add_space(look::SPACE_M);
                if ui
                    .add_enabled_ui(interactive, |ui| self.microphone_picker(ui))
                    .inner
                {
                    self.change_microphone();
                }
                if ui
                    .add_enabled(interactive, egui::Button::new("Refresh microphones"))
                    .clicked()
                {
                    self.refresh_microphones();
                }
                if let Some(error) = self.microphone_refresh_error.as_deref() {
                    muted(ui, error);
                }
                ui.add_space(look::SPACE_M);
                ui.horizontal_wrapped(|ui| {
                    let live = self.input.is_some();
                    let label = if live {
                        "Disable microphone"
                    } else {
                        "Enable microphone"
                    };
                    let mut button = egui::Button::new(label);
                    if !live {
                        button =
                            egui::Button::new(RichText::new(label).color(look::WINDOW_BG).strong())
                                .fill(look::ACCENT);
                    }
                    if ui
                        .add_enabled(
                            interactive && (live || self.selected_input.is_some()),
                            button,
                        )
                        .clicked()
                    {
                        if live {
                            self.disable_input();
                        } else if let Err(e) = self.enable_input() {
                            self.message = Some(e);
                        }
                    }
                });
                if let Some(input) = &self.input {
                    muted(
                        ui,
                        format!(
                            "{} Hz · {} channel(s)",
                            input.config.sample_rate.0, input.config.channels
                        ),
                    );
                } else if self.inputs.is_empty() {
                    muted(
                        ui,
                        "Connect a microphone, then open the list or refresh microphones.",
                    );
                }
                ui.add_space(look::SPACE_M);
                section(ui, "Input gain");
                ui.add_enabled_ui(interactive, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.gain_db, -30.0..=24.0)
                                .suffix(" dB")
                                .fixed_decimals(1),
                        );
                        if ui.button("Reset gain").clicked() {
                            self.gain_db = 0.0;
                        }
                    });
                });
                if let Some(input) = &self.input {
                    input
                        .signal
                        .gain
                        .store(gain(self.gain_db).to_bits(), Relaxed);
                }
                muted(ui, "Gain applies to both routes.");
            });
        });
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, interactive: bool) {
        tutorial::region(ui, "aural-settings", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                section(ui, "Settings");
                let enabled = interactive && self.preferences_allowed;
                ui.add_space(look::SPACE_M);
                ui.add_enabled_ui(enabled, |ui| {
                    audio_setting_switch(
                        ui,
                        &mut self.preferences.auto_enable,
                        "Enable microphone on startup",
                    );
                });
                muted(
                    ui,
                    "Starts live audio when Toolboks opens. A saved per-microphone on/off state takes priority when remembering settings is enabled.",
                );
                ui.add_space(look::SPACE_M);
                ui.add_enabled_ui(enabled, |ui| {
                    audio_setting_switch(
                        ui,
                        &mut self.preferences.remember_devices,
                        "Remember settings for each microphone",
                    );
                });
                muted(
                    ui,
                    "Save microphone on/off, gain, listening volume and routing. Restore them when switching microphones or reopening Toolboks.",
                );
                if self.preferences.remember_devices {
                    if let Some(device) = self.selected_input.and_then(|i| self.inputs.get(i)) {
                        muted(ui, format!("Saving settings for {}.", device.name));
                    } else {
                        muted(ui, "Choose a microphone to save its settings.");
                    }
                }
                ui.add_space(look::SPACE_L);
                section(ui, "Default microphone");
                if let Some(default) = &self.preferences.default_input {
                    ui.label(egui::RichText::new(&default.name).strong());
                    muted(
                        ui,
                        if self.default_input_available() {
                            "Available. Toolboks switches back when it reconnects."
                        } else {
                            "Waiting for this microphone. Toolboks will switch to it when it connects."
                        },
                    );
                } else {
                    muted(
                        ui,
                        "No default set. Choose a microphone above, then save it here.",
                    );
                }
                ui.add_enabled_ui(enabled, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let uniquely_named = self
                            .selected_input
                            .and_then(|index| self.inputs.get(index))
                            .is_some_and(|selected| {
                                self.inputs
                                    .iter()
                                    .filter(|d| d.name == selected.name)
                                    .count()
                                    == 1
                            });
                        if ui
                            .add_enabled(
                                uniquely_named,
                                egui::Button::new("Use selected microphone"),
                            )
                            .clicked()
                        {
                            self.set_default_input();
                        }
                        if ui
                            .add_enabled(
                                self.preferences.default_input.is_some(),
                                egui::Button::new("Clear default"),
                            )
                            .clicked()
                        {
                            self.clear_default_input();
                        }
                    });
                });
                if !self.preferences_allowed {
                    muted(ui, "Review the Audio Gainer update to allow these settings to be saved on this PC.");
                    if ui
                        .add_enabled(
                            !tutorial::active(ui.ctx()),
                            egui::Button::new("Review update"),
                        )
                        .clicked()
                    {
                        self.request_settings_update();
                    }
                } else {
                    muted(ui, "Changes are saved automatically on this PC.");
                }
            });
        });
    }

    fn levels_ui(&mut self, ui: &mut egui::Ui, interactive: bool) {
        tutorial::region(ui, "aural-levels", |ui| {
            let history = self.history.each_mut().map(|line| &*line.make_contiguous());
            let lines = [
                look::Line {
                    values: history[0],
                    color: look::DATA_NETWORK,
                    fill: false,
                    label: "Left",
                    fmt: |v| format!("{:.1} dBFS", v - 60.0),
                },
                look::Line {
                    values: history[1],
                    color: look::MUTED,
                    fill: false,
                    label: "Right",
                    fmt: |v| format!("{:.1} dBFS", v - 60.0),
                },
            ];
            let stats = if self.input.is_some() {
                self.levels
                    .iter()
                    .zip(["Left", "Right"])
                    .map(|(level, channel)| (format!("{level:.1} dBFS"), channel))
                    .collect::<Vec<_>>()
            } else {
                vec![("Input off".into(), "")]
            };
            look::graph_card(
                ui,
                ui.available_width(),
                185.0,
                &look::GraphCard {
                    title: "Input levels",
                    detail: "Recent peaks",
                    lines: &lines,
                    capacity: HISTORY,
                    max: 60.0,
                    step_secs: INTERVAL.as_secs_f32(),
                    stats: &stats,
                    note: self
                        .input
                        .is_none()
                        .then_some("Enable the microphone to see live levels."),
                },
            );
            if let Some(input) = &self.input {
                ui.add_space(look::SPACE_XS);
                for (channel, level) in ["Left", "Right"].into_iter().zip(self.levels) {
                    ui.horizontal(|ui| {
                        ui.add_sized([38.0, 18.0], egui::Label::new(channel));
                        ui.add(
                            egui::ProgressBar::new((level + 60.0) / 60.0)
                                .desired_width((ui.available_width() - 86.0).max(50.0))
                                .fill(if channel == "Left" {
                                    look::DATA_NETWORK
                                } else {
                                    look::MUTED
                                }),
                        );
                        ui.add_sized([78.0, 18.0], egui::Label::new(format!("{level:.1} dBFS")));
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if input.signal.clipped.load(Relaxed) {
                        ui.label(RichText::new("Clipping detected").color(look::AMBER));
                        if ui
                            .add_enabled(interactive, egui::Button::new("Clear warning"))
                            .clicked()
                        {
                            input.signal.clipped.store(false, Relaxed);
                        }
                        muted(
                            ui,
                            "Lower the input gain or your microphone's hardware gain.",
                        );
                    } else {
                        muted(ui, "No clipping detected");
                    }
                });
            }
        });
    }

    fn route_ui(&mut self, ui: &mut egui::Ui, index: usize, interactive: bool) {
        tutorial::region(ui, ["aural-monitor", "aural-cable"][index], |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                section(ui, ROUTE_LABELS[index]);
                if index == 0 {
                    muted(ui, "Use headphones to avoid feedback.");
                } else {
                    let has_vb_cable = self.outputs.iter().any(|device| {
                        let name = device.name.to_ascii_lowercase();
                        name.contains("vb-audio") && name.contains("cable")
                    });
                    muted(
                        ui,
                        if has_vb_cable {
                            "VB-Audio Virtual Cable is available for routing to other apps."
                        } else {
                            "Install VB-Audio Virtual Cable to route audio to another app."
                        },
                    );
                    ui.add_enabled_ui(interactive, |ui| {
                        ui.visuals_mut().hyperlink_color = look::DATA_MEMORY;
                        ui.hyperlink_to("VB-CABLE download and installation", VB_CABLE);
                    });
                    muted(ui, "Select CABLE Input here, then choose CABLE Output as the microphone in the other app.");
                    if self.selected_outputs[index].is_none() {
                        muted(
                            ui,
                            "No virtual cable detected. After installing it, refresh devices.",
                        );
                    }
                }
                ui.add_space(look::SPACE_M);
                ui.horizontal_wrapped(|ui| {
                    let changed = ui
                        .add_enabled_ui(interactive, |ui| {
                            picker(
                                ui,
                                ("aural-output", index),
                                &self.outputs,
                                &mut self.selected_outputs[index],
                                if index == 0 {
                                    "Choose headphones"
                                } else {
                                    "Choose a virtual cable"
                                },
                                |d| index == 1 || !virtual_cable(&d.name),
                            )
                        })
                        .inner;
                    if changed {
                        let live = self.routes[index].is_some();
                        self.stop_route(index);
                        self.route_errors[index] = if live {
                            self.enable_route(index).err()
                        } else {
                            None
                        };
                    }
                    let live = self.routes[index].is_some();
                    let label = if live { "Stop route" } else { "Start route" };
                    if ui
                        .add_enabled(
                            interactive && self.selected_outputs[index].is_some(),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        self.route_errors[index] = None;
                        if live {
                            self.route_stopped(index);
                        } else {
                            self.route_errors[index] = self.enable_route(index).err();
                        }
                    }
                    ui.label(
                        RichText::new(if live { "Live" } else { "Off" }).color(if live {
                            look::GREEN
                        } else {
                            look::MUTED
                        }),
                    );
                });
                if index == 0 {
                    ui.add_enabled_ui(interactive, |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.volume, 0.0..=100.0)
                                .suffix("%")
                                .fixed_decimals(0)
                                .text("Listening volume"),
                        );
                    });
                    self.monitor_level
                        .store((self.volume / 100.0).to_bits(), Relaxed);
                }
                if let Some(error) = &self.route_errors[index] {
                    muted(ui, error);
                }
            });
        });
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui, interactive: bool) {
        self.levels_ui(ui, interactive);
        ui.add_space(look::SPACE_M);
        muted(ui, "Starting a route turns on the selected microphone. Disable the microphone to stop both routes.");
        for index in 0..2 {
            ui.add_space(look::SPACE_M);
            self.route_ui(ui, index, interactive);
        }
    }
}

impl NativeAddon for Aural {
    fn set_capabilities(&mut self, capabilities: &[Capability]) {
        self.allowed = capabilities.contains(&Capability::LiveAudio);
        let preferences_allowed = capabilities.contains(&Capability::LocalPreferences);
        if preferences_allowed != self.preferences_allowed {
            self.preferences_loaded = false;
        }
        self.preferences_allowed = preferences_allowed;
        if !self.allowed || !self.preferences_allowed {
            self.discovery = None;
        }
        if !self.allowed && self.input.is_some() {
            self.cancel();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, prefs: &mut BTreeMap<String, serde_json::Value>) -> bool {
        let tutorial_active = tutorial::active(ui.ctx());
        if !tutorial_active {
            self.begin_ui_preferences(prefs);
        }
        let interactive = self.allowed && !tutorial_active;
        if interactive && !self.loaded {
            self.refresh();
        }
        if self.input.is_some() {
            ui.request_repaint_after(INTERVAL);
        }
        egui::ScrollArea::vertical()
            .id_salt("aural-workspace")
            .show(ui, |ui| {
                ui.label(RichText::new("Live audio").size(look::TEXT_TITLE).strong());
                muted(
                    ui,
                    "Check your microphone level and choose where the sound goes.",
                );
                if let Some(message) = self.message.as_deref() {
                    if look::banner(ui, message) {
                        self.message = None;
                    }
                }
                ui.add_space(look::SPACE_L);
                let width = ui.available_width();
                if width >= 680.0 {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(270.0);
                            self.input_ui(ui, interactive);
                            ui.add_space(look::SPACE_M);
                            self.settings_ui(ui, interactive);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            self.detail_ui(ui, interactive);
                        });
                    });
                } else {
                    self.input_ui(ui, interactive);
                    ui.add_space(look::SPACE_M);
                    self.settings_ui(ui, interactive);
                    ui.add_space(look::SPACE_M);
                    self.detail_ui(ui, interactive);
                }
            });
        if tutorial_active {
            false
        } else {
            self.finish_ui_preferences(prefs)
        }
    }

    fn load_preferences(&mut self, prefs: &BTreeMap<String, serde_json::Value>) {
        if self.preferences_loaded {
            return;
        }
        self.preferences_loaded = true;
        if self.preferences_allowed {
            self.preferences = prefs
                .get(settings::KEY)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            self.want_input = self.preferences.startup_enabled();
            self.ensure_discovery();
        }
    }
    fn take_host_request(&mut self) -> Option<toolboks_addon_sdk::HostRequest> {
        std::mem::take(&mut self.settings_update_requested)
            .then_some(toolboks_addon_sdk::HostRequest::ReviewUpdate)
    }
    fn tick(&mut self, _: &egui::Context) {
        self.tick_background();
        self.sample();
    }

    fn suspend(&mut self) {
        self.ui_active = false;
        if let Some(input) = &self.input {
            input.signal.metering.store(false, Relaxed);
        }
        self.history = std::array::from_fn(|_| VecDeque::new());
        self.levels = [-60.0; 2];
    }

    fn resume(&mut self) {
        self.ui_active = true;
        if let Some(input) = &self.input {
            for peak in &input.signal.peaks {
                peak.store(0, Relaxed);
            }
            input.signal.metering.store(true, Relaxed);
        }
        self.last_sample = Instant::now();
    }

    fn has_background_work(&self) -> bool {
        self.input.is_some() || self.discovery.is_some()
    }

    fn tick_background(&mut self) {
        self.poll_discovery();
        let error = self
            .input
            .as_ref()
            .and_then(|input| input.signal.error.lock().unwrap().take());
        if let Some(error) = error {
            self.cancel();
            self.message = Some(error);
        }
        for index in 0..2 {
            let error = self.routes[index]
                .as_ref()
                .and_then(|route| route.error.lock().unwrap().take());
            if error.is_some() {
                self.stop_route(index);
                self.route_errors[index] = error;
            }
        }
    }

    fn cancel(&mut self) {
        for index in 0..2 {
            self.stop_route(index);
        }
        self.input = None;
        self.levels = [-60.0; 2];
        self.history.iter_mut().for_each(VecDeque::clear);
    }

    fn tutorial_location(&self) -> String {
        "0".into()
    }

    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        [
            ("aural-input", "Choose your microphone", "Choose a physical input, then enable it. Input gain changes the level sent to both routes."),
            ("aural-settings", "Remember your setup", "Startup activation is optional. Save gain and routing separately for each microphone, and choose a default whose name stays saved while disconnected. Toolboks switches back when it reconnects."),
            ("aural-levels", "Check your levels", "Watch the left and right peaks. If clipping is reported, lower your gain, then clear the warning to check again."),
            ("aural-monitor", "Hear yourself", "Choose headphones and start the route. Listening volume affects only your headphones."),
            ("aural-cable", "Send audio to another app", "Install VB-CABLE separately. Choose CABLE Input here and CABLE Output as the microphone in your other app."),
        ].into_iter().map(|(id, title, text)| TutorialStep::new("0", id, title, text)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_frame(
        ctx: &egui::Context,
        addon: &mut Aural,
        width: f32,
        enabled: bool,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        fn texts(shape: &egui::Shape, result: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => result.push((
                    text.galley.text().to_owned(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, result);
                    }
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ui| addon.settings_ui(ui, enabled),
        );
        let mut result = Vec::new();
        for shape in &output.shapes {
            texts(&shape.shape, &mut result);
        }
        output.textures_delta.clear();
        result
    }

    #[test]
    fn audio_settings_controls_change_only_when_interactive() {
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut addon = Aural {
            preferences_allowed: true,
            ..Default::default()
        };
        let texts = settings_frame(&ctx, &mut addon, 290.0, true, Vec::new());
        let startup = texts
            .iter()
            .find(|(text, _)| text == "Enable microphone on startup")
            .unwrap()
            .1
            .center();
        assert!(!addon.preferences.auto_enable);
        settings_frame(&ctx, &mut addon, 290.0, true, picker_pointer(startup, true));
        settings_frame(
            &ctx,
            &mut addon,
            290.0,
            true,
            picker_pointer(startup, false),
        );
        assert!(addon.preferences.auto_enable);
        let texts = settings_frame(&ctx, &mut addon, 290.0, true, Vec::new());
        let remember = texts
            .iter()
            .find(|(text, _)| text == "Remember settings for each microphone")
            .unwrap()
            .1
            .center();
        settings_frame(
            &ctx,
            &mut addon,
            290.0,
            true,
            picker_pointer(remember, true),
        );
        settings_frame(
            &ctx,
            &mut addon,
            290.0,
            true,
            picker_pointer(remember, false),
        );
        assert!(addon.preferences.remember_devices);
        settings_frame(&ctx, &mut addon, 290.0, false, Vec::new());
        settings_frame(
            &ctx,
            &mut addon,
            290.0,
            false,
            picker_pointer(startup, true),
        );
        settings_frame(
            &ctx,
            &mut addon,
            290.0,
            false,
            picker_pointer(startup, false),
        );
        assert!(
            addon.preferences.auto_enable,
            "disabled settings must not change"
        );
        assert!(addon.input.is_none());
        assert!(
            addon.discovery.is_none(),
            "drawing settings must not start discovery"
        );
    }

    #[test]
    fn audio_settings_show_missing_default_and_fit_narrow_scaled_layouts() {
        for width in [270.0, 400.0, 610.0] {
            for scale in [1.0, 1.25, 2.0] {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(scale);
                look::apply_style(&ctx);
                let mut addon = Aural {
                    preferences_allowed: true,
                    ..Default::default()
                };
                addon.preferences.default_input = Some(settings::DeviceIdentity {
                    name: "Disconnected Razer microphone".into(),
                });
                for _ in 0..2 {
                    settings_frame(&ctx, &mut addon, width, true, Vec::new());
                }
                let texts = settings_frame(&ctx, &mut addon, width, true, Vec::new());
                assert!(texts
                    .iter()
                    .any(|(text, _)| text == "Disconnected Razer microphone"));
                assert!(texts
                    .iter()
                    .any(|(text, _)| text.starts_with("Waiting for this microphone")));
                for (text, rect) in texts {
                    assert!(
                        rect.right() <= width + 1.0,
                        "{text} overflows at {width}, scale {scale}: {rect:?}"
                    );
                    assert!(rect.left() >= -1.0, "{text} starts outside settings card");
                }
                assert!(!addon.preferences.auto_enable);
                assert!(addon.discovery.is_none());
                assert!(addon.input.is_none());
            }
        }
    }

    #[test]
    fn audio_settings_startup_switch_can_be_toggled_by_keyboard() {
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut addon = Aural {
            preferences_allowed: true,
            ..Default::default()
        };
        settings_frame(&ctx, &mut addon, 290.0, true, Vec::new());
        settings_frame(&ctx, &mut addon, 290.0, true, picker_key(egui::Key::Tab));
        assert!(ctx.memory(|memory| memory.focused()).is_some());
        settings_frame(&ctx, &mut addon, 290.0, true, picker_key(egui::Key::Space));
        assert!(addon.preferences.auto_enable);
        assert!(addon.input.is_none());
        assert!(addon.discovery.is_none());
    }

    #[test]
    fn audio_settings_tutorial_does_not_load_startup_preferences_or_start_audio() {
        let ctx = egui::Context::default();
        let mut addon = Aural::default();
        addon.set_capabilities(&[Capability::LiveAudio, Capability::LocalPreferences]);
        let mut prefs = BTreeMap::from([(
            settings::KEY.into(),
            serde_json::json!({
                "auto_enable": true,
                "remember_devices": true,
                "default_input": {"name": "Disconnected Razer microphone"}
            }),
        )]);
        let original = prefs.clone();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 1000.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    tutorial::prepare(ui.ctx(), Some("aural-settings"), true);
                    assert!(!addon.ui(ui, &mut prefs));
                },
            );
            output.textures_delta.clear();
        }
        assert!(!addon.preferences_loaded);
        assert!(!addon.loaded);
        assert!(addon.discovery.is_none());
        assert!(addon.input.is_none());
        assert!(addon.routes.iter().all(Option::is_none));
        assert_eq!(prefs, original);
    }

    fn picker_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        enabled: bool,
        refreshes: &mut usize,
    ) -> egui::Id {
        let mut id = egui::Id::NULL;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    id = ui.make_persistent_id(egui::IdSalt::new("refresh-test-picker"));
                    refreshing_picker(
                        ui,
                        "refresh-test-picker",
                        "Microphone".into(),
                        |ui, opening| {
                            if opening {
                                *refreshes += 1;
                            }
                            ui.selectable_label(false, "Available microphone").clicked()
                        },
                    );
                });
            },
        );
        output.textures_delta.clear();
        id
    }

    fn picker_pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn picker_key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    #[test]
    fn microphone_picker_refreshes_once_per_pointer_open_and_reopen() {
        let ctx = egui::Context::default();
        let mut refreshes = 0;
        let id = picker_frame(&ctx, Vec::new(), true, &mut refreshes);
        let center = ctx.read_response(id).unwrap().rect.center();
        assert_eq!(refreshes, 0);
        picker_frame(&ctx, picker_pointer(center, true), true, &mut refreshes);
        picker_frame(&ctx, picker_pointer(center, false), true, &mut refreshes);
        assert!(egui::ComboBox::is_open(&ctx, id));
        assert_eq!(refreshes, 1);
        for _ in 0..3 {
            picker_frame(&ctx, Vec::new(), true, &mut refreshes);
        }
        assert_eq!(refreshes, 1, "an open list must not rescan every frame");
        picker_frame(&ctx, picker_key(egui::Key::Escape), true, &mut refreshes);
        assert!(!egui::ComboBox::is_open(&ctx, id));
        picker_frame(&ctx, picker_pointer(center, true), true, &mut refreshes);
        picker_frame(&ctx, picker_pointer(center, false), true, &mut refreshes);
        assert!(egui::ComboBox::is_open(&ctx, id));
        assert_eq!(refreshes, 2, "reopening must refresh the current inventory");
    }

    #[test]
    fn microphone_picker_refreshes_on_keyboard_open_but_not_when_disabled() {
        let ctx = egui::Context::default();
        let mut refreshes = 0;
        let id = picker_frame(&ctx, Vec::new(), true, &mut refreshes);
        ctx.memory_mut(|memory| memory.request_focus(id));
        picker_frame(&ctx, picker_key(egui::Key::Enter), true, &mut refreshes);
        assert!(egui::ComboBox::is_open(&ctx, id));
        assert_eq!(refreshes, 1);
        picker_frame(&ctx, Vec::new(), true, &mut refreshes);
        assert_eq!(refreshes, 1);
        picker_frame(&ctx, picker_key(egui::Key::Escape), true, &mut refreshes);
        picker_frame(&ctx, Vec::new(), false, &mut refreshes);
        let center = ctx.read_response(id).unwrap().rect.center();
        picker_frame(&ctx, picker_pointer(center, true), false, &mut refreshes);
        picker_frame(&ctx, picker_pointer(center, false), false, &mut refreshes);
        assert!(!egui::ComboBox::is_open(&ctx, id));
        assert_eq!(refreshes, 1, "disabled controls must not enumerate devices");
    }

    #[test]
    fn microphone_refresh_preserves_endpoint_identity_across_reorder_and_names() {
        let old = [(10, "Microphone"), (20, "Microphone")];
        let fresh = [(20, "Renamed microphone"), (30, "New"), (10, "Microphone")];
        assert_eq!(
            refreshed_selection(&old, Some(1), &fresh, |a, b| a.0 == b.0),
            Some(0)
        );
        // A replacement with the same display name must not inherit selection.
        assert_eq!(
            refreshed_selection(&old, Some(1), &[(30, "Microphone")], |a, b| a.0 == b.0),
            None
        );
    }

    #[test]
    fn microphone_refresh_requires_unambiguous_selection_without_endpoint_ids() {
        let old = ["USB microphone", "USB microphone"];
        assert_eq!(
            refreshed_selection(&old, Some(0), &["USB microphone"], |a, b| a == b),
            None
        );
        assert_eq!(
            refreshed_selection(&["USB microphone"], Some(0), &old, |a, b| a == b),
            None
        );
        assert_eq!(refreshed_selection(&old, None, &old, |a, b| a == b), None);
    }

    #[test]
    fn microphone_refresh_failure_retains_state_and_retry_clears_only_refresh_error() {
        let mut addon = Aural {
            selected_outputs: [Some(2), Some(4)],
            route_errors: [Some("Existing route error".into()), None],
            message: Some("Existing audio error".into()),
            ..Default::default()
        };
        addon.apply_microphone_refresh(Err("Enumeration unavailable".into()));
        assert_eq!(
            addon.microphone_refresh_error.as_deref(),
            Some("Enumeration unavailable")
        );
        assert_eq!(addon.selected_outputs, [Some(2), Some(4)]);
        addon.apply_microphone_refresh(Ok(Vec::new()));
        assert!(addon.microphone_refresh_error.is_none());
        assert_eq!(addon.message.as_deref(), Some("Existing audio error"));
        assert_eq!(
            addon.route_errors[0].as_deref(),
            Some("Existing route error")
        );
        assert_eq!(addon.selected_outputs, [Some(2), Some(4)]);
        assert!(!addon.loaded);
    }

    #[test]
    fn gain_updates_the_stereo_peaks_and_latches_clipping() {
        let signal = Signal::default();
        signal.gain.store(2.0_f32.to_bits(), Relaxed);
        signal.metering.store(true, Relaxed);
        process(&[0.25_f32, -0.75], 2, &signal);
        assert_eq!(
            signal
                .peaks
                .each_ref()
                .map(|v| f32::from_bits(v.load(Relaxed))),
            [0.5, 1.5]
        );
        assert!(signal.clipped.load(Relaxed));
        process(&[0.0_f32; 2], 2, &signal);
        assert!(signal.clipped.load(Relaxed));
        assert!((db(gain(-18.0)) + 18.0).abs() < 0.001);
    }

    #[test]
    fn background_audio_still_forwards_without_metering() {
        let signal = Signal::default();
        signal.gain.store(2.0_f32.to_bits(), Relaxed);
        let bridge = Arc::new(Mutex::new(routing::Bridge::new(100, 100)));
        signal.routes.lock().unwrap()[1] = Some(bridge.clone());
        process(&[0.75_f32; 8], 1, &signal);
        assert_eq!(signal.peaks[0].load(Relaxed), 0);
        assert!(!signal.clipped.load(Relaxed));
        assert_eq!(bridge.lock().unwrap().next(), [1.0; 2]);
    }

    #[test]
    fn recognizes_standard_and_additional_vb_cable_endpoints() {
        assert!(virtual_cable("CABLE Input (VB-Audio Virtual Cable)"));
        assert!(virtual_cable("CABLE-A Output (VB-Audio Cable A)"));
        assert!(!virtual_cable("Headphones (USB Audio)"));
    }

    #[test]
    fn walkthrough_targets_render_at_both_widths_without_opening_audio() {
        for width in [610.0, 900.0] {
            let ctx = egui::Context::default();
            let mut addon = Aural::default();
            addon.set_capabilities(&[Capability::LiveAudio]);
            let mut prefs = BTreeMap::new();
            for step in addon.tutorial_steps() {
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 720.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            tutorial::prepare(ui.ctx(), Some(&step.target), true);
                            addon.ui(ui, &mut prefs);
                        },
                    );
                    output.textures_delta.clear();
                }
                let anchor = tutorial::anchor(&ctx).expect("walkthrough anchor");
                assert!(
                    anchor.right() <= width + 1.0,
                    "{} overflows at {width}",
                    step.title
                );
                assert!(!addon.loaded && addon.input.is_none());
                assert!(addon.routes.iter().all(Option::is_none));
            }
        }
    }
}
