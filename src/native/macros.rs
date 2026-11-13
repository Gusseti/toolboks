mod engine;

use crate::ui as look;
use eframe::egui::{self, RichText};
use engine::{
    Action, ControlEvent, Engine, Hotkey, Macro, MouseButton, PathClick, Point, RecordOptions,
    RepeatOptions, Step, TimingScope,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use toolboks_addon_sdk::{tutorial, Capability, NativeAddon, TutorialStep};

pub(super) const PAGES: [&str; 2] = ["Editor", "Mouse editor"];
const KEY: &str = "macro_manager_v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    macros: Vec<Macro>,
    selected: usize,
    keys: bool,
    movement: bool,
    clicks: bool,
    scroll: bool,
    delay_mode: u8,
    default_delay: String,
    repeat_mode: u8,
    repeat_delay: String,
    run_duration: String,
    record_hotkey: Option<Hotkey>,
    play_hotkey: Option<Hotkey>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            macros: vec![Macro {
                activation_key: None,
                name: "New macro".into(),
                steps: vec![],
            }],
            selected: 0,
            keys: true,
            movement: true,
            clicks: true,
            scroll: true,
            delay_mode: 0,
            default_delay: "100ms".into(),
            repeat_mode: 0,
            repeat_delay: "100ms".into(),
            run_duration: "10s".into(),
            record_hotkey: None,
            play_hotkey: None,
        }
    }
}

#[derive(Default, PartialEq)]
enum CanvasTool {
    #[default]
    Select,
    Draw,
    Click,
}

pub(super) struct MacroManager {
    prefs: Preferences,
    initialized: bool,
    allowed: bool,
    saving_allowed: bool,
    engine: Engine,
    page: usize,
    step: Option<usize>,
    tool: CanvasTool,
    drag_point: Option<usize>,
    seconds: bool,
    timing_text: String,
    message: Option<String>,
    shortcut_capture: Option<bool>,
    key_capture: bool,
    configured_hotkeys: Option<(Option<Hotkey>, Vec<(usize, Hotkey)>)>,
    playing_macro: Option<usize>,
    was_recording: bool,
    pending_save: bool,
    ignore_rect: Option<[i32; 4]>,
}
impl Default for MacroManager {
    fn default() -> Self {
        Self {
            prefs: Preferences::default(),
            initialized: false,
            allowed: false,
            saving_allowed: false,
            engine: Engine::default(),
            page: 0,
            step: None,
            tool: CanvasTool::default(),
            drag_point: None,
            seconds: true,
            timing_text: String::new(),
            message: None,
            shortcut_capture: None,
            key_capture: false,
            configured_hotkeys: None,
            playing_macro: None,
            was_recording: false,
            pending_save: false,
            ignore_rect: None,
        }
    }
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(look::MUTED));
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(look::TEXT_SECTION).strong());
}
fn duration_text(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}
fn button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "Left",
        MouseButton::Right => "Right",
        MouseButton::Middle => "Middle",
        _ => "Mouse",
    }
}
fn action_name(action: &Action) -> String {
    match action {
        Action::Key { vk, down } => format!(
            "{} {}",
            if *down { "Key down" } else { "Key up" },
            engine::key_name(*vk)
        ),
        Action::Move { x, y } => format!("Move pointer · {x}, {y}"),
        Action::Click { button, count } => format!("{} click · {count}×", button_name(*button)),
        Action::Button { button, down } => format!(
            "{} button {}",
            button_name(*button),
            if *down { "down" } else { "up" }
        ),
        Action::Scroll { delta, horizontal } => format!(
            "Scroll {} · {delta}",
            if *horizontal {
                "horizontal"
            } else {
                "vertical"
            }
        ),
        Action::Path {
            points,
            duration_ms,
            ..
        } => format!(
            "Mouse path · {} points · {}",
            points.len(),
            duration_text(*duration_ms)
        ),
    }
}
fn virtual_screen() -> [i32; 4] {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        [
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        ]
    }
}
fn app_window_rect() -> Option<[i32; 4]> {
    use windows::Win32::{
        Foundation::RECT,
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId},
    };
    unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid != GetCurrentProcessId() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok()?;
        Some([rect.left, rect.top, rect.right, rect.bottom])
    }
}
fn key_vk(key: egui::Key) -> Option<u32> {
    let name = format!("{key:?}");
    if name.len() == 1 {
        let c = name.as_bytes()[0];
        if c.is_ascii_uppercase() {
            return Some(c as u32);
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x6f + n);
        }
    }
    if let Some(n) = name.strip_prefix("Num").and_then(|n| n.parse::<u32>().ok()) {
        if n < 10 {
            return Some(0x30 + n);
        }
    }
    Some(match key {
        egui::Key::Enter => 0x0d,
        egui::Key::Space => 0x20,
        egui::Key::Tab => 0x09,
        egui::Key::Backspace => 0x08,
        egui::Key::Delete => 0x2e,
        egui::Key::Insert => 0x2d,
        egui::Key::Home => 0x24,
        egui::Key::End => 0x23,
        egui::Key::PageUp => 0x21,
        egui::Key::PageDown => 0x22,
        egui::Key::ArrowLeft => 0x25,
        egui::Key::ArrowUp => 0x26,
        egui::Key::ArrowRight => 0x27,
        egui::Key::ArrowDown => 0x28,
        _ => return None,
    })
}
fn hotkey_text(hotkey: &Option<Hotkey>) -> String {
    let Some(key) = hotkey else {
        return "Not assigned".into();
    };
    let mut parts = vec![];
    for (mask, label) in [(2, "Ctrl"), (1, "Alt"), (4, "Shift"), (8, "Win")] {
        if key.modifiers & mask != 0 {
            parts.push(label.to_owned())
        }
    }
    parts.push(engine::key_name(key.vk as u16));
    parts.join(" + ")
}

impl MacroManager {
    pub(super) fn needs_background_preferences(
        prefs: &BTreeMap<String, serde_json::Value>,
    ) -> bool {
        prefs.get(KEY).is_some_and(|p| {
            ["record_hotkey", "play_hotkey"]
                .iter()
                .any(|k| p.get(*k).is_some_and(|v| !v.is_null()))
                || p.get("macros")
                    .and_then(|v| v.as_array())
                    .is_some_and(|macros| {
                        macros
                            .iter()
                            .any(|m| m.get("activation_key").is_some_and(|v| !v.is_null()))
                    })
        })
    }
    fn configure_hotkeys(&mut self) {
        if self.shortcut_capture.is_some() {
            return;
        }
        let desired = if self.allowed {
            (
                self.prefs.record_hotkey.clone(),
                self.prefs
                    .macros
                    .iter()
                    .enumerate()
                    .filter_map(|(index, m)| m.activation_key.map(|key| (index, key)))
                    .collect::<Vec<_>>(),
            )
        } else {
            (None, Vec::new())
        };
        if self.configured_hotkeys.as_ref() == Some(&desired) {
            return;
        }
        match self
            .engine
            .configure_shortcuts(desired.0.clone(), desired.1.clone())
        {
            Ok(()) => self.configured_hotkeys = Some(desired),
            Err(error) => {
                self.message = Some(error);
                self.configured_hotkeys = Some(desired);
            }
        }
    }
    fn selected_macro(&mut self) -> &mut Macro {
        if self.prefs.macros.is_empty() {
            self.prefs.macros.push(Macro {
                activation_key: None,
                name: "New macro".into(),
                steps: vec![],
            })
        }
        self.prefs.selected = self.prefs.selected.min(self.prefs.macros.len() - 1);
        &mut self.prefs.macros[self.prefs.selected]
    }
    fn selected_step(&mut self) -> Option<&mut Step> {
        let index = self.step?;
        self.selected_macro().steps.get_mut(index)
    }
    fn finish_recording(&mut self) {
        let mut steps = self.engine.stop_record();
        self.was_recording = false;
        if self.prefs.delay_mode == 1 {
            match engine::parse_timing(&self.prefs.default_delay, TimingScope::Milliseconds) {
                Ok(delay) => {
                    for step in &mut steps {
                        step.delay_ms = delay
                    }
                }
                Err(e) => self.message = Some(e),
            }
        }
        if !steps.is_empty() {
            let start = self.selected_macro().steps.len();
            self.selected_macro().steps.extend(steps);
            self.step = Some(start);
            self.pending_save = true;
        }
    }
    fn record(&mut self) {
        if !self.allowed {
            return;
        }
        if self.engine.status().recording {
            self.finish_recording();
            return;
        }
        let options = RecordOptions {
            keys: self.prefs.keys,
            mouse_movement: self.prefs.movement,
            mouse_buttons: self.prefs.clicks,
            mouse_scroll: self.prefs.scroll,
            ignore_rect: self.ignore_rect,
        };
        match self.engine.start_record(options) {
            Ok(()) => {
                self.was_recording = true;
                self.message = None
            }
            Err(e) => self.message = Some(e),
        }
    }
    fn play(&mut self) {
        self.play_macro(self.prefs.selected);
    }
    fn play_macro(&mut self, index: usize) {
        if !self.allowed {
            return;
        }
        if self.engine.status().playing {
            self.engine.stop();
            if self.playing_macro == Some(index) {
                self.playing_macro = None;
                return;
            }
        }
        if self.engine.status().recording {
            self.finish_recording()
        }
        let delay = match engine::parse_timing(&self.prefs.repeat_delay, TimingScope::Milliseconds)
        {
            Ok(v) => v,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        let duration = if self.prefs.repeat_mode == 1 {
            match engine::parse_timing(&self.prefs.run_duration, TimingScope::Seconds) {
                Ok(v) if v > 0 => Some(v),
                _ => {
                    self.message = Some("Enter a run duration greater than zero.".into());
                    return;
                }
            }
        } else {
            None
        };
        let Some(macro_data) = self.prefs.macros.get(index) else {
            return;
        };
        let steps = macro_data.steps.clone();
        if steps.is_empty() {
            self.message = Some("Add or record some steps first.".into());
            return;
        }
        let options = RepeatOptions {
            repeat: self.prefs.repeat_mode != 0,
            delay_ms: delay,
            duration_ms: duration,
        };
        if let Err(error) = self.engine.play(steps, options) {
            self.message = Some(error)
        } else {
            self.message = None;
            self.playing_macro = Some(index);
        }
    }
    fn service(&mut self) {
        let status = self.engine.status();
        if self.was_recording && !status.recording {
            self.finish_recording()
        }
        for event in self.engine.poll_control_events() {
            match event {
                ControlEvent::Record => self.record(),
                ControlEvent::Play(index) => self.play_macro(index),
            }
        }
        if let Some(error) = self.engine.status().error {
            self.message = Some(error)
        }
    }
    fn capture_binding(&mut self, ui: &egui::Ui) {
        if let Some(record) = self.shortcut_capture {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
            if let Some(key) = self.engine.take_captured_shortcut() {
                self.engine.stop();
                self.shortcut_capture = None;
                if key.vk != 0x1b {
                    if record {
                        self.prefs.record_hotkey = Some(key);
                    } else {
                        self.selected_macro().activation_key = Some(key);
                    }
                    self.pending_save = true;
                }
                self.configure_hotkeys();
            }
            return;
        }
        if self.shortcut_capture.is_none() && !self.key_capture {
            return;
        }
        for event in ui.input(|i| i.events.clone()) {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                ..
            } = event
            {
                if key == egui::Key::Escape {
                    let was_binding = self.shortcut_capture.take().is_some();
                    self.key_capture = false;
                    if was_binding {
                        self.configure_hotkeys();
                    }
                    break;
                }
                let Some(vk) = key_vk(key) else { continue };
                if self.key_capture {
                    let mut paired_release = None;
                    if let Some(step) = self.selected_step() {
                        if let Action::Key { vk: key, down } = &mut step.action {
                            if *down {
                                paired_release = Some(*key);
                            }
                            *key = vk as u16
                        }
                    }
                    if let (Some(old), Some(index)) = (paired_release, self.step) {
                        if let Some(Step {
                            action:
                                Action::Key {
                                    vk: key,
                                    down: false,
                                },
                            ..
                        }) = self.selected_macro().steps.get_mut(index + 1)
                        {
                            if *key == old {
                                *key = vk as u16;
                            }
                        }
                    }
                    self.key_capture = false;
                }
                self.pending_save = true;
                break;
            }
        }
    }
    fn controls(&mut self, ui: &mut egui::Ui, interactive: bool) {
        tutorial::region(ui, "macro-recording", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                let status = self.engine.status();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            interactive && !status.playing,
                            egui::Button::new(if status.recording {
                                "Stop recording"
                            } else {
                                "Record"
                            })
                            .fill(look::ACCENT),
                        )
                        .clicked()
                    {
                        self.record()
                    }
                    ui.checkbox(&mut self.prefs.keys, "Keystrokes");
                    ui.checkbox(&mut self.prefs.movement, "Movement");
                    ui.checkbox(&mut self.prefs.clicks, "Clicks");
                    ui.checkbox(&mut self.prefs.scroll, "Scroll");
                    ui.separator();
                    muted(ui, "Delays");
                    for (mode, name) in [(0, "Recorded"), (1, "Fixed"), (2, "Custom")] {
                        ui.selectable_value(&mut self.prefs.delay_mode, mode, name);
                    }
                    if self.prefs.delay_mode == 1 {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prefs.default_delay)
                                .desired_width(72.0),
                        );
                    }
                    if status.recording {
                        muted(
                            ui,
                            format!("Recording · {} events · Esc stops", status.recorded_steps),
                        );
                    } else {
                        muted(ui, "Esc always stops");
                    }
                });
                if self.prefs.delay_mode == 2 {
                    muted(ui, "Recorded delays remain editable for every step below.");
                }
            })
        });
        ui.add_space(look::SPACE_M);
        tutorial::region(ui, "macro-repeat", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                ui.horizontal_wrapped(|ui| {
                    heading(ui, "Playback");
                    ui.selectable_value(&mut self.prefs.repeat_mode, 0, "Once");
                    ui.selectable_value(&mut self.prefs.repeat_mode, 1, "For duration");
                    ui.selectable_value(&mut self.prefs.repeat_mode, 2, "Until stopped (toggle)");
                    if self.prefs.repeat_mode != 0 {
                        muted(ui, "Between repeats");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prefs.repeat_delay)
                                .desired_width(72.0),
                        );
                    }
                    if self.prefs.repeat_mode == 1 {
                        muted(ui, "Run for");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prefs.run_duration)
                                .desired_width(72.0),
                        );
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for record in [true, false] {
                        muted(
                            ui,
                            if record {
                                "Record shortcut"
                            } else {
                                "This macro's activation key"
                            },
                        );
                        let binding = if record {
                            self.prefs.record_hotkey
                        } else {
                            self.prefs.macros[self.prefs.selected].activation_key
                        };
                        let label = if self.shortcut_capture == Some(record) {
                            "Press shortcut…".into()
                        } else {
                            hotkey_text(&binding)
                        };
                        if ui
                            .add_enabled(
                                interactive
                                    && !self.engine.status().recording
                                    && !self.engine.status().playing,
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            self.shortcut_capture = Some(record);
                            let _ = self.engine.configure_shortcuts(None, Vec::new());
                            self.configured_hotkeys = None;
                            if let Err(error) = self.engine.start_shortcut_capture() {
                                self.shortcut_capture = None;
                                self.message = Some(error);
                                self.configure_hotkeys();
                            }
                        }
                        if binding.is_some()
                            && ui
                                .add_enabled(
                                    interactive,
                                    egui::Button::new(if record {
                                        "Clear record key"
                                    } else {
                                        "Clear activation key"
                                    }),
                                )
                                .clicked()
                        {
                            if record {
                                self.prefs.record_hotkey = None
                            } else {
                                self.selected_macro().activation_key = None
                            };
                            self.configure_hotkeys();
                        }
                    }
                });
            })
        });
    }
    fn library(&mut self, ui: &mut egui::Ui, editable: bool) {
        look::card(ui, ui.available_width(), 0.0, |ui| {
            ui.horizontal(|ui| {
                heading(ui, "Macros");
                if ui.add_enabled(editable, egui::Button::new("New")).clicked() {
                    self.prefs.macros.push(Macro {
                        activation_key: None,
                        name: format!("Macro {}", self.prefs.macros.len() + 1),
                        steps: vec![],
                    });
                    self.prefs.selected = self.prefs.macros.len() - 1;
                    self.step = None;
                }
            });
            let names = self
                .prefs
                .macros
                .iter()
                .map(|m| {
                    (
                        m.name.clone(),
                        m.steps.len(),
                        hotkey_text(&m.activation_key),
                    )
                })
                .collect::<Vec<_>>();
            for (index, (name, count, key)) in names.into_iter().enumerate() {
                if ui
                    .add_enabled(
                        editable,
                        egui::Button::selectable(
                            self.prefs.selected == index,
                            format!("{name}  ·  {count} steps\n{key}"),
                        ),
                    )
                    .clicked()
                {
                    self.prefs.selected = index;
                    self.step = None;
                }
            }
            ui.separator();
            ui.add_enabled(
                editable,
                egui::TextEdit::singleline(&mut self.selected_macro().name)
                    .hint_text("Macro name")
                    .desired_width(ui.available_width()),
            );
            if ui
                .add_enabled(
                    editable && self.prefs.macros.len() > 1,
                    egui::Button::new("Delete macro"),
                )
                .clicked()
            {
                let i = self.prefs.selected;
                self.prefs.macros.remove(i);
                self.prefs.selected = 0;
                self.step = None;
                self.configure_hotkeys();
            }
            ui.add_space(look::SPACE_M);
            heading(ui, "Steps");
            let rows = self
                .selected_macro()
                .steps
                .iter()
                .enumerate()
                .map(|(i, s)| (i, action_name(&s.action), s.delay_ms))
                .collect::<Vec<_>>();
            egui::ScrollArea::vertical()
                .id_salt("macro-steps")
                .max_height(430.0)
                .show(ui, |ui| {
                    for (index, label, delay) in rows {
                        if ui
                            .add_enabled(
                                editable,
                                egui::Button::selectable(
                                    self.step == Some(index),
                                    format!(
                                        "{:02}  {label}\n      Delay {}",
                                        index + 1,
                                        duration_text(delay)
                                    ),
                                ),
                            )
                            .clicked()
                        {
                            self.step = Some(index);
                            self.timing_text.clear();
                        }
                    }
                    if self.selected_macro().steps.is_empty() {
                        muted(ui, "Record actions or add a step to begin.");
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(editable, egui::Button::new("Add key"))
                    .clicked()
                {
                    let m = self.selected_macro();
                    let i = m.steps.len();
                    m.steps.extend([
                        Step {
                            delay_ms: 0,
                            action: Action::Key { vk: 65, down: true },
                        },
                        Step {
                            delay_ms: 0,
                            action: Action::Key {
                                vk: 65,
                                down: false,
                            },
                        },
                    ]);
                    self.step = Some(i);
                }
                if ui
                    .add_enabled(editable, egui::Button::new("Add path"))
                    .clicked()
                {
                    let m = self.selected_macro();
                    let i = m.steps.len();
                    m.steps.push(Step {
                        delay_ms: 0,
                        action: Action::Path {
                            points: vec![],
                            duration_ms: 1000,
                            clicks: vec![],
                        },
                    });
                    self.step = Some(i);
                    self.page = 1;
                    self.tool = CanvasTool::Draw;
                }
                if ui
                    .add_enabled(editable, egui::Button::new("Add scroll"))
                    .clicked()
                {
                    let m = self.selected_macro();
                    let i = m.steps.len();
                    m.steps.push(Step {
                        delay_ms: 0,
                        action: Action::Scroll {
                            delta: -120,
                            horizontal: false,
                        },
                    });
                    self.step = Some(i);
                }
            });
        });
    }
    fn canvas(&mut self, ui: &mut egui::Ui, editable: bool) {
        tutorial::region(ui, "macro-mouse", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                ui.horizontal_wrapped(|ui| {
                    heading(ui, "Mouse path");
                    ui.selectable_value(&mut self.tool, CanvasTool::Select, "Select");
                    ui.selectable_value(&mut self.tool, CanvasTool::Draw, "Draw path");
                    ui.selectable_value(&mut self.tool, CanvasTool::Click, "Add click");
                });
                let bounds = virtual_screen();
                muted(
                    ui,
                    format!(
                        "{} × {} desktop · Click to place points; drag to adjust",
                        bounds[2], bounds[3]
                    ),
                );
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 330.0),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 6, look::WINDOW_BG);
                for i in 1..12 {
                    let x = rect.left() + rect.width() * i as f32 / 12.0;
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.0, look::BORDER),
                    );
                }
                for i in 1..8 {
                    let y = rect.top() + rect.height() * i as f32 / 8.0;
                    painter.line_segment(
                        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                        egui::Stroke::new(1.0, look::BORDER),
                    );
                }
                let to_canvas = |p: &Point| {
                    egui::pos2(
                        rect.left() + (p.x - bounds[0]) as f32 / bounds[2] as f32 * rect.width(),
                        rect.top() + (p.y - bounds[1]) as f32 / bounds[3] as f32 * rect.height(),
                    )
                };
                let to_screen = |p: egui::Pos2| Point {
                    x: bounds[0]
                        + ((p.x - rect.left()) / rect.width() * bounds[2] as f32).round() as i32,
                    y: bounds[1]
                        + ((p.y - rect.top()) / rect.height() * bounds[3] as f32).round() as i32,
                };
                let tool = &self.tool;
                let drag = &mut self.drag_point;
                let selected = self.prefs.selected;
                let step = self.step;
                if let Some(Step {
                    action: Action::Path { points, clicks, .. },
                    ..
                }) = step.and_then(|i| {
                    self.prefs
                        .macros
                        .get_mut(selected)
                        .and_then(|m| m.steps.get_mut(i))
                }) {
                    if editable {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let nearest = points
                                .iter()
                                .enumerate()
                                .map(|(i, p)| (i, to_canvas(p).distance(pos)))
                                .min_by(|a, b| a.1.total_cmp(&b.1));
                            if response.clicked() {
                                match tool {
                                    CanvasTool::Draw => points.push(to_screen(pos)),
                                    CanvasTool::Click => {
                                        if let Some((i, _)) = nearest {
                                            clicks.push(PathClick {
                                                point_index: i,
                                                button: MouseButton::Left,
                                                count: 1,
                                            });
                                            *drag = Some(i);
                                        }
                                    }
                                    CanvasTool::Select => {
                                        *drag = nearest.filter(|(_, d)| *d < 18.0).map(|(i, _)| i)
                                    }
                                }
                            }
                            if response.drag_started() {
                                *drag = nearest.filter(|(_, d)| *d < 18.0).map(|(i, _)| i);
                            }
                            if response.dragged() {
                                if let Some(index) = *drag {
                                    if let Some(point) = points.get_mut(index) {
                                        *point = to_screen(pos.clamp(rect.min, rect.max));
                                    }
                                }
                            }
                        }
                    }
                    for pair in points.windows(2) {
                        painter.line_segment(
                            [to_canvas(&pair[0]), to_canvas(&pair[1])],
                            egui::Stroke::new(2.0, look::ACCENT),
                        );
                    }
                    for (index, point) in points.iter().enumerate() {
                        let pos = to_canvas(point);
                        painter.circle_filled(pos, 4.0, look::DATA_MEMORY);
                        painter.text(
                            pos + egui::vec2(0.0, -16.0),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", index + 1),
                            egui::FontId::proportional(11.0),
                            look::TEXT,
                        );
                    }
                    for click in clicks.iter() {
                        if let Some(point) = points.get(click.point_index) {
                            let pos = to_canvas(point);
                            painter.circle_stroke(pos, 10.0, egui::Stroke::new(1.5, look::ACCENT));
                            painter.text(
                                pos + egui::vec2(13.0, 13.0),
                                egui::Align2::LEFT_TOP,
                                format!("{} click", button_name(click.button)),
                                egui::FontId::proportional(11.0),
                                look::TEXT,
                            );
                        }
                    }
                    muted(
                        ui,
                        format!("{} points · {} click actions", points.len(), clicks.len()),
                    );
                } else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Select a mouse path, or use Add path.",
                        egui::FontId::proportional(13.0),
                        look::MUTED,
                    );
                }
            })
        });
    }
    fn timing(&mut self, ui: &mut egui::Ui, editable: bool) {
        look::card(ui, ui.available_width(), 0.0, |ui| {
            ui.horizontal_wrapped(|ui| {
                heading(ui, "Movement timing");
                if ui.selectable_label(!self.seconds, "Milliseconds").clicked() {
                    self.seconds = false;
                    self.timing_text.clear();
                }
                if ui.selectable_label(self.seconds, "Seconds").clicked() {
                    self.seconds = true;
                    self.timing_text.clear();
                }
            });
            let scope = if self.seconds {
                TimingScope::Seconds
            } else {
                TimingScope::Milliseconds
            };
            let seconds = self.seconds;
            if let Some(Step {
                action: Action::Path { duration_ms, .. },
                ..
            }) = self.selected_step()
            {
                let scale = if seconds { 1000.0 } else { 1.0 };
                let maximum = if seconds { 10.0 } else { 1000.0 };
                let mut value = *duration_ms as f64 / scale;
                if ui
                    .add_enabled(
                        editable,
                        egui::Slider::new(&mut value, 0.0..=maximum)
                            .suffix(if seconds { " s" } else { " ms" })
                            .max_decimals(if seconds { 3 } else { 0 })
                            .clamping(egui::SliderClamping::Edits),
                    )
                    .changed()
                {
                    *duration_ms = (value * scale).round() as u64;
                }
                let width = ui.available_width();
                let (r, _) = ui.allocate_exact_size(egui::vec2(width, 15.0), egui::Sense::hover());
                for i in 0..=10 {
                    let x = r.left() + r.width() * i as f32 / 10.0;
                    ui.painter().text(
                        egui::pos2(x, r.top()),
                        egui::Align2::CENTER_TOP,
                        format!("{}", if seconds { i } else { i * 100 }),
                        egui::FontId::proportional(10.0),
                        look::MUTED,
                    );
                }
            } else {
                muted(ui, "Select a mouse path to set its duration.");
            }
            ui.horizontal(|ui| {
                muted(ui, "Custom timing");
                let response = ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut self.timing_text)
                        .hint_text(if self.seconds {
                            "2.5s or 2.5 sec"
                        } else {
                            "250ms"
                        })
                        .desired_width(110.0),
                );
                if response.lost_focus() && !self.timing_text.trim().is_empty() {
                    match engine::parse_timing(&self.timing_text, scope) {
                        Ok(value) => {
                            if let Some(Step {
                                action: Action::Path { duration_ms, .. },
                                ..
                            }) = self.selected_step()
                            {
                                *duration_ms = value;
                            }
                        }
                        Err(error) => self.message = Some(error),
                    }
                }
            });
        });
    }
    fn inspector(&mut self, ui: &mut egui::Ui, editable: bool) {
        tutorial::region(ui, "macro-step", |ui| {
            look::card(ui, ui.available_width(), 0.0, |ui| {
                heading(ui, "Selected step");
                let Some(index) = self.step else {
                    muted(ui, "Choose a step to edit its timing and action.");
                    return;
                };
                muted(ui, format!("Step {}", index + 1));
                let mut remove = false;
                let mut up = false;
                let mut down = false;
                let mut capture = false;
                if let Some(step) = self.selected_step() {
                    ui.add_enabled_ui(editable, |ui| {
                        ui.label(action_name(&step.action));
                        ui.add_space(look::SPACE_M);
                        muted(ui, "Delay before this step");
                        ui.add(
                            egui::DragValue::new(&mut step.delay_ms)
                                .range(0..=86_400_000)
                                .suffix(" ms"),
                        );
                        match &mut step.action {
                            Action::Path {
                                duration_ms,
                                points,
                                clicks,
                            } => {
                                muted(ui, "Movement");
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(*duration_ms > 0, "Timed").clicked()
                                        && *duration_ms == 0
                                    {
                                        *duration_ms = 1000
                                    }
                                    if ui.selectable_label(*duration_ms == 0, "Instant").clicked() {
                                        *duration_ms = 0;
                                    }
                                });
                                muted(ui, "Movement duration");
                                ui.label(duration_text(*duration_ms));
                                if let Some(point) = points.last_mut() {
                                    muted(ui, "End position");
                                    ui.horizontal_wrapped(|ui| {
                                        ui.add(egui::DragValue::new(&mut point.x).prefix("X "));
                                        ui.add(egui::DragValue::new(&mut point.y).prefix("Y "));
                                    });
                                }
                                for click in clicks.iter_mut() {
                                    mouse_buttons(ui, &mut click.button);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(format!("Point {} click", click.point_index + 1));
                                        ui.add(
                                            egui::DragValue::new(&mut click.count).range(1..=20),
                                        );
                                    });
                                }
                                if ui.button("Remove last point").clicked() {
                                    points.pop();
                                    clicks.retain(|c| c.point_index < points.len());
                                }
                                if ui.button("Clear click actions").clicked() {
                                    clicks.clear();
                                }
                            }
                            Action::Move { x, y } => {
                                ui.add(egui::DragValue::new(x).prefix("X "));
                                ui.add(egui::DragValue::new(y).prefix("Y "));
                            }
                            Action::Key { vk, down } => {
                                ui.label(engine::key_name(*vk));
                                capture = ui.button("Capture key").clicked();
                                ui.checkbox(down, "Key held down (off = release)");
                            }
                            Action::Click { button, count } => {
                                mouse_buttons(ui, button);
                                ui.add(egui::DragValue::new(count).range(1..=20).prefix("Clicks "));
                            }
                            Action::Button { button, down } => {
                                mouse_buttons(ui, button);
                                ui.checkbox(down, "Button down");
                            }
                            Action::Scroll { delta, horizontal } => {
                                ui.add(egui::DragValue::new(delta).speed(120.0).prefix("Scroll "));
                                ui.checkbox(horizontal, "Horizontal");
                            }
                        }
                        ui.add_space(look::SPACE_M);
                        ui.horizontal_wrapped(|ui| {
                            up = ui.button("Move up").clicked();
                            down = ui.button("Move down").clicked();
                            remove = ui.button("Delete step").clicked();
                        });
                    });
                }
                self.key_capture |= capture;
                if self.key_capture {
                    muted(ui, "Press a key to assign it. Esc cancels.");
                }
                let steps = &mut self.selected_macro().steps;
                if remove && index < steps.len() {
                    steps.remove(index);
                    self.step = None;
                } else if up && index > 0 && index < steps.len() {
                    steps.swap(index, index - 1);
                    self.step = Some(index - 1);
                } else if down && index + 1 < steps.len() {
                    steps.swap(index, index + 1);
                    self.step = Some(index + 1);
                }
            })
        });
    }
}
fn mouse_buttons(ui: &mut egui::Ui, button: &mut MouseButton) {
    ui.horizontal_wrapped(|ui| {
        for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            ui.selectable_value(button, b, button_name(b));
        }
    });
}

impl NativeAddon for MacroManager {
    fn set_capabilities(&mut self, caps: &[Capability]) {
        self.allowed = caps.contains(&Capability::InputAutomation);
        self.saving_allowed = caps.contains(&Capability::LocalPreferences);
        if !self.allowed {
            self.engine.stop();
        }
        self.configure_hotkeys();
    }
    fn load_preferences(&mut self, prefs: &BTreeMap<String, serde_json::Value>) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if self.saving_allowed {
            if let Some(saved) = prefs
                .get(KEY)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                self.prefs = saved;
            }
        }
        self.selected_macro();
        if let Some(previous) = self.prefs.play_hotkey.take() {
            if self.selected_macro().activation_key.is_none() {
                self.selected_macro().activation_key = Some(previous);
                self.pending_save = true;
            }
        }
        self.configure_hotkeys();
    }
    fn navigate(&mut self, page: &str) {
        self.page = PAGES
            .iter()
            .position(|p| p.eq_ignore_ascii_case(page))
            .or_else(|| page.parse::<usize>().ok())
            .unwrap_or(0)
            .min(PAGES.len() - 1);
    }
    fn page_index(&self) -> usize {
        self.page
    }
    fn ui(&mut self, ui: &mut egui::Ui, prefs: &mut BTreeMap<String, serde_json::Value>) -> bool {
        let tour = tutorial::active(ui.ctx());
        if !tour {
            self.load_preferences(prefs);
            self.service();
            self.capture_binding(ui);
        }
        let before = serde_json::to_value(&self.prefs).ok();
        let interactive = self.allowed && !tour;
        let busy = self.engine.status().recording || self.engine.status().playing;
        if let Some(rect) = app_window_rect() {
            self.ignore_rect = Some(rect);
        }
        egui::ScrollArea::vertical().id_salt("macro-manager-workspace").show(ui,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.label(RichText::new(&self.selected_macro().name).size(look::TEXT_TITLE).strong());
                let playing=self.engine.status().playing;
                if ui.add_enabled(interactive && !self.engine.status().recording,egui::Button::new(if playing{"Stop playback"}else{"Play macro"}).min_size(egui::vec2(110.0,32.0)).fill(look::ACCENT)).clicked(){
                    if playing{self.engine.stop();self.playing_macro=None;}else{self.play();}
                }
                if ui.add_enabled(interactive && !busy,egui::Button::new("Save macro")).clicked(){self.pending_save=true;}
            });
            muted(ui,"Record actions, edit steps, and draw mouse paths. Playback sends real desktop input.");
            if let Some(message)=self.message.clone(){if look::banner(ui,&message){self.message=None;}}
            ui.add_space(look::SPACE_M);self.controls(ui,interactive);
            ui.add_space(look::SPACE_M);
            let editable=interactive && !busy;let width=ui.available_width();
            if width>=1000.0 {
                ui.horizontal_top(|ui|{
                    ui.vertical(|ui|{ui.set_width(260.0);self.library(ui,editable);});
                    ui.vertical(|ui|{ui.set_width((width-520.0).max(380.0));self.canvas(ui,editable);ui.add_space(look::SPACE_M);self.timing(ui,editable);});
                    ui.vertical(|ui|{ui.set_width(ui.available_width().max(220.0));self.inspector(ui,editable);});
                });
            }else{
                ui.horizontal_top(|ui|{ui.vertical(|ui|{ui.set_width(260.0_f32.min(width*0.4));self.library(ui,editable);});ui.vertical(|ui|{ui.set_width(ui.available_width());self.canvas(ui,editable);self.timing(ui,editable);self.inspector(ui,editable);});});
            }
        });
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let changed = before != serde_json::to_value(&self.prefs).ok() || self.pending_save;
        if !tour && self.saving_allowed && changed {
            if let Ok(value) = serde_json::to_value(&self.prefs) {
                prefs.insert(KEY.into(), value);
                self.pending_save = false;
                return true;
            }
        }
        false
    }
    fn tick(&mut self, _: &egui::Context) {
        self.service();
    }
    fn tick_background(&mut self) {
        self.service();
    }
    fn save_preferences(&mut self, prefs: &mut BTreeMap<String, serde_json::Value>) -> bool {
        if !self.saving_allowed || !self.pending_save {
            return false;
        }
        if let Ok(value) = serde_json::to_value(&self.prefs) {
            prefs.insert(KEY.into(), value);
            self.pending_save = false;
            return true;
        }
        false
    }
    fn has_background_work(&self) -> bool {
        let s = self.engine.status();
        s.recording || s.playing || self.engine.shortcuts_configured()
    }
    fn cancel(&mut self) {
        self.engine.stop();
        let _ = self.engine.configure_shortcuts(None, Vec::new());
        self.configured_hotkeys = None;
    }
    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        vec![
        TutorialStep::new("0","macro-recording","Record input","Choose keyboard and mouse inputs, then Record. Escape stops recording without replaying anything."),
        TutorialStep::new("0","macro-repeat","Repeat and shortcuts","Play once, run for a duration, or toggle repeating. The repeat delay is a pause between runs, not playback speed."),
        TutorialStep::new("1","macro-mouse","Edit mouse paths","Add a path, click to place points, drag to adjust them, and add clicks at a point."),
        TutorialStep::new("1","macro-step","Edit a step","Change the selected step or its delay. Movement uses duration rather than pixels per second."),
    ]
    }
    fn tutorial_location(&self) -> String {
        self.page.to_string()
    }
    fn tutorial_navigate(&mut self, page: &str) {
        self.navigate(page)
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;
    #[test]
    fn activation_keys_are_saved_per_macro_and_legacy_key_is_migrated() {
        let mut manager = MacroManager {
            saving_allowed: true,
            pending_save: true,
            ..Default::default()
        };
        let first = Hotkey {
            modifiers: 2,
            vk: 0x74,
        };
        let second = Hotkey {
            modifiers: 2,
            vk: 0x75,
        };
        manager.prefs.macros[0].activation_key = Some(first);
        manager.prefs.macros.push(Macro {
            name: "Second".into(),
            activation_key: Some(second),
            steps: Vec::new(),
        });
        let mut prefs = BTreeMap::new();
        assert!(manager.save_preferences(&mut prefs));
        let mut reopened = MacroManager {
            saving_allowed: true,
            ..Default::default()
        };
        reopened.load_preferences(&prefs);
        assert_eq!(reopened.prefs.macros[0].activation_key, Some(first));
        assert_eq!(reopened.prefs.macros[1].activation_key, Some(second));
        let value = prefs.get_mut(KEY).unwrap();
        value["macros"][0]["activation_key"] = serde_json::Value::Null;
        value["play_hotkey"] = serde_json::to_value(first).unwrap();
        let mut migrated = MacroManager {
            saving_allowed: true,
            ..Default::default()
        };
        migrated.load_preferences(&prefs);
        assert_eq!(migrated.prefs.macros[0].activation_key, Some(first));
        assert!(migrated.prefs.play_hotkey.is_none());
    }
    #[test]
    fn editor_and_mouse_page_render_without_starting_input() {
        for width in [900.0, 1280.0] {
            let ctx = egui::Context::default();
            look::apply_style(&ctx);
            let mut manager = MacroManager::default();
            manager.set_capabilities(&[Capability::InputAutomation, Capability::LocalPreferences]);
            manager.prefs.macros[0].steps.push(Step {
                delay_ms: 100,
                action: Action::Path {
                    points: vec![Point { x: 100, y: 100 }, Point { x: 400, y: 300 }],
                    duration_ms: 2500,
                    clicks: vec![PathClick {
                        point_index: 1,
                        button: MouseButton::Left,
                        count: 1,
                    }],
                },
            });
            manager.step = Some(0);
            manager.initialized = true;
            let mut prefs = BTreeMap::new();
            for page in PAGES {
                manager.navigate(page);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        manager.ui(ui, &mut prefs);
                    },
                );
                assert!(!output.shapes.is_empty());
                assert!(!manager.engine.status().recording && !manager.engine.status().playing);
                output.textures_delta.clear();
            }
        }
    }
    #[test]
    fn background_recording_results_use_namespaced_preferences() {
        let mut manager = MacroManager {
            saving_allowed: true,
            pending_save: true,
            ..Default::default()
        };
        manager.prefs.macros[0].name = "Saved macro".into();
        manager.prefs.macros[0].steps.push(Step {
            delay_ms: 100,
            action: Action::Key { vk: 65, down: true },
        });
        let mut prefs = BTreeMap::new();
        assert!(manager.save_preferences(&mut prefs));
        let mut reopened = MacroManager::default();
        reopened.set_capabilities(&[Capability::InputAutomation, Capability::LocalPreferences]);
        reopened.load_preferences(&prefs);
        assert_eq!(reopened.prefs.macros[0].name, "Saved macro");
        assert_eq!(reopened.prefs.macros[0].steps.len(), 1);
        assert!(!reopened.has_background_work());
    }
}
