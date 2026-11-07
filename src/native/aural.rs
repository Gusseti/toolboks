//! Aural's live audio controls inside the Toolboks workspace.
mod routing;

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
    loaded: bool,
    inputs: Vec<Device>,
    outputs: Vec<Device>,
    selected_input: Option<usize>,
    selected_outputs: [Option<usize>; 2],
    input: Option<Input>,
    routes: [Option<Output>; 2],
    route_errors: [Option<String>; 2],
    message: Option<String>,
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
            loaded: false,
            inputs: Vec::new(),
            outputs: Vec::new(),
            selected_input: None,
            selected_outputs: [None; 2],
            input: None,
            routes: [None, None],
            route_errors: [None, None],
            message: None,
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
    fn refresh(&mut self) {
        self.cancel();
        self.loaded = true;
        self.message = None;
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

impl Aural {
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
                    .add_enabled_ui(interactive, |ui| {
                        picker(
                            ui,
                            "aural-input-picker",
                            &self.inputs,
                            &mut self.selected_input,
                            "Choose a microphone",
                            |_| true,
                        )
                    })
                    .inner
                {
                    self.cancel();
                    self.route_errors = [None, None];
                    self.message = None;
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
                        .add_enabled(interactive && self.selected_input.is_some(), button)
                        .clicked()
                    {
                        if live {
                            self.cancel();
                        } else if let Err(e) = self.enable_input() {
                            self.message = Some(e);
                        }
                    }
                    if ui
                        .add_enabled(interactive, egui::Button::new("Refresh devices"))
                        .clicked()
                    {
                        self.refresh();
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
                    muted(ui, "Connect a microphone, then refresh devices.");
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
                            self.stop_route(index);
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
        if !self.allowed && self.input.is_some() {
            self.cancel();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut BTreeMap<String, serde_json::Value>) -> bool {
        let interactive = self.allowed && !tutorial::active(ui.ctx());
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
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            self.detail_ui(ui, interactive);
                        });
                    });
                } else {
                    self.input_ui(ui, interactive);
                    ui.add_space(look::SPACE_M);
                    self.detail_ui(ui, interactive);
                }
            });
        false
    }

    fn tick(&mut self, _: &egui::Context) {
        self.tick_background();
        self.sample();
    }

    fn suspend(&mut self) {
        if let Some(input) = &self.input {
            input.signal.metering.store(false, Relaxed);
        }
        self.history = std::array::from_fn(|_| VecDeque::new());
        self.levels = [-60.0; 2];
    }

    fn resume(&mut self) {
        if let Some(input) = &self.input {
            for peak in &input.signal.peaks {
                peak.store(0, Relaxed);
            }
            input.signal.metering.store(true, Relaxed);
        }
        self.last_sample = Instant::now();
    }

    fn has_background_work(&self) -> bool {
        self.routes.iter().any(Option::is_some)
    }

    fn tick_background(&mut self) {
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
            ("aural-levels", "Check your levels", "Watch the left and right peaks. If clipping is reported, lower your gain, then clear the warning to check again."),
            ("aural-monitor", "Hear yourself", "Choose headphones and start the route. Listening volume affects only your headphones."),
            ("aural-cable", "Send audio to another app", "Install VB-CABLE separately. Choose CABLE Input here and CABLE Output as the microphone in your other app."),
        ].into_iter().map(|(id, title, text)| TutorialStep::new("0", id, title, text)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
