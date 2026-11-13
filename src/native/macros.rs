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
    record_mouse: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            macros: vec![],
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
            record_mouse: true,
        }
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
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
    new_key: bool,
    new_name: Option<String>,
    focus_new_name: bool,
    confirm_delete: Option<usize>,
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
            new_key: false,
            new_name: None,
            focus_new_name: false,
            confirm_delete: None,
            configured_hotkeys: None,
            playing_macro: None,
            was_recording: false,
            pending_save: false,
            ignore_rect: None,
        }
    }
}

fn editor_card(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    look::card(ui, ui.available_width(), height, |ui| {
        egui::Frame::new().inner_margin(6.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((height - 34.0).max(0.0));
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            add(ui);
        });
    });
}
fn section_divider(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}
fn choice_button(label: &str, selected: bool) -> egui::Button<'_> {
    egui::Button::new(label)
        .fill(if selected {
            look::SELECTED_BG
        } else {
            look::PANEL_BG
        })
        .stroke(egui::Stroke::new(1.0, look::BORDER))
}
fn choice<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, candidate: T, label: &str) {
    if ui.add(choice_button(label, *value == candidate)).clicked() {
        *value = candidate;
    }
}
fn choices(ui: &mut egui::Ui, value: &mut u8, labels: [&str; 3]) {
    let width = (ui.available_width() - 16.0) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.spacing_mut().button_padding.x = 6.0;
        for (index, label) in labels.into_iter().enumerate() {
            if ui
                .add_sized([width, 30.0], choice_button(label, *value == index as u8))
                .clicked()
            {
                *value = index as u8;
            }
        }
    });
}
fn section(ui: &mut egui::Ui, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    let id = ui.make_persistent_id(("macro-options-section", name));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| {
            heading(ui, name);
        })
        .body_unindented(|ui| {
            ui.add_space(8.0);
            add(ui);
        });
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(look::MUTED));
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(look::TEXT_SECTION).strong());
}
fn creation_icon(ui: &mut egui::Ui, accept: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 28.0), egui::Sense::click());
    let color = if response.hovered() {
        if accept {
            look::GREEN
        } else {
            look::RED
        }
    } else {
        look::TEXT
    };
    let center = rect.center();
    let stroke = egui::Stroke::new(1.8, color);
    if accept {
        ui.painter().line_segment(
            [
                center + egui::vec2(-6.0, 0.0),
                center + egui::vec2(-2.0, 4.0),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [
                center + egui::vec2(-2.0, 4.0),
                center + egui::vec2(6.0, -5.0),
            ],
            stroke,
        );
    } else {
        ui.painter().line_segment(
            [
                center + egui::vec2(-5.0, -5.0),
                center + egui::vec2(5.0, 5.0),
            ],
            stroke,
        );
        ui.painter().line_segment(
            [
                center + egui::vec2(5.0, -5.0),
                center + egui::vec2(-5.0, 5.0),
            ],
            stroke,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            ui.is_enabled(),
            if accept {
                "Create macro"
            } else {
                "Cancel creation"
            },
        )
    });
    response
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
        Action::RecordedMovement { .. } => "Mouse movement".into(),
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
fn control_screen_rect(ctx: &egui::Context, rect: egui::Rect) -> Option<[i32; 4]> {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::ClientToScreen,
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };
    unsafe {
        let window = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid != GetCurrentProcessId() {
            return None;
        }
        let mut origin = POINT::default();
        if !ClientToScreen(window, &mut origin).as_bool() {
            return None;
        }
        let scale = ctx.pixels_per_point();
        Some([
            origin.x + (rect.left() * scale).floor() as i32,
            origin.y + (rect.top() * scale).floor() as i32,
            origin.x + (rect.right() * scale).ceil() as i32,
            origin.y + (rect.bottom() * scale).ceil() as i32,
        ])
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
    fn delete_step(&mut self, index: usize) {
        if let Some(m) = self.prefs.macros.get_mut(self.prefs.selected) {
            if index < m.steps.len() {
                m.steps.remove(index);
                self.step = None;
                self.pending_save = true;
            }
        }
    }
    fn delete_macro(&mut self, index: usize) {
        if index < self.prefs.macros.len() {
            self.prefs.macros.remove(index);
            self.prefs.selected = self
                .prefs
                .selected
                .min(self.prefs.macros.len().saturating_sub(1));
            self.step = None;
            self.pending_save = true;
            self.configure_hotkeys();
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(index) = self.confirm_delete {
            let name = self
                .prefs
                .macros
                .get(index)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            egui::Modal::new(egui::Id::unique("macro-delete-confirm")).show(ctx, |ui| {
                heading(ui, "Delete macro?");
                ui.label(format!("Delete {name} and all its steps?"));
                ui.horizontal(|ui| {
                    if ui.button("Confirm delete").clicked() {
                        self.delete_macro(index);
                        self.confirm_delete = None;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        self.confirm_delete = None;
                    }
                });
            });
        }
    }
    fn options(&mut self, ui: &mut egui::Ui, interactive: bool, editable: bool) {
        if self.step.is_some() {
            self.inspector(ui, editable);
            return;
        }
        editor_card(ui, 626.0, |ui| {
            let has_macro = !self.prefs.macros.is_empty();
            heading(
                ui,
                self.prefs
                    .macros
                    .get(self.prefs.selected)
                    .map_or("Macro settings", |m| m.name.as_str()),
            );
            muted(ui, "Macro options");
            if !has_macro {
                muted(ui, "Create a macro to enable its controls.");
            }
            section_divider(ui);
            self.controls(ui, interactive && has_macro);
            section_divider(ui);
            if ui
                .add_enabled(editable && has_macro, egui::Button::new("Delete macro"))
                .clicked()
            {
                self.confirm_delete = Some(self.prefs.selected);
            }
        });
    }
    fn inline_creation(&mut self, ui: &mut egui::Ui, editable: bool) {
        let Some(mut name) = self.new_name.take() else {
            if ui.add_enabled(editable, egui::Button::new("New")).clicked() {
                self.new_name = Some(String::new());
                self.focus_new_name = true;
            }
            return;
        };
        let mut create = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("Give it a name")
                    .desired_width((ui.available_width() - 64.0).max(60.0)),
            );
            if self.focus_new_name {
                field.request_focus();
                self.focus_new_name = false;
            }
            create = creation_icon(ui, true)
                .on_hover_text("Create macro")
                .clicked();
            cancel = creation_icon(ui, false).on_hover_text("Cancel").clicked();
            create |= ui.input(|i| i.key_pressed(egui::Key::Enter));
            cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if cancel {
            return;
        }
        if create && !name.trim().is_empty() {
            self.prefs.macros.push(Macro {
                name: name.trim().into(),
                ..Default::default()
            });
            self.prefs.selected = self.prefs.macros.len() - 1;
            self.step = None;
            self.pending_save = true;
        } else {
            self.new_name = Some(name);
        }
    }
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
        if self.shortcut_capture.is_some() || self.new_key {
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
        self.prefs.selected = self.prefs.selected.min(self.prefs.macros.len() - 1);
        &mut self.prefs.macros[self.prefs.selected]
    }
    fn selected_step(&mut self) -> Option<&mut Step> {
        if self.prefs.macros.is_empty() {
            return None;
        }
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
            self.selected_macro()
                .steps
                .extend(engine::group_recorded_movements(steps));
            self.step = Some(start);
            self.pending_save = true;
        }
    }
    fn record(&mut self) {
        if !self.allowed {
            return;
        }
        if self.prefs.macros.is_empty() {
            self.message = Some("Create a named macro before recording.".into());
            return;
        }
        if self.engine.status().recording {
            self.finish_recording();
            return;
        }
        let options = RecordOptions {
            keys: !self.prefs.record_mouse,
            mouse_movement: self.prefs.record_mouse,
            mouse_buttons: self.prefs.record_mouse,
            mouse_scroll: self.prefs.record_mouse,
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
        if self.new_key {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
            if let Some(key) = self.engine.take_captured_shortcut() {
                self.engine.stop();
                self.new_key = false;
                if key.vk != 0x1b && !self.prefs.macros.is_empty() {
                    let m = self.selected_macro();
                    let index = m.steps.len();
                    m.steps.extend([
                        Step {
                            delay_ms: 0,
                            action: Action::Key {
                                vk: key.vk as u16,
                                down: true,
                            },
                        },
                        Step {
                            delay_ms: 0,
                            action: Action::Key {
                                vk: key.vk as u16,
                                down: false,
                            },
                        },
                    ]);
                    self.step = Some(index);
                    self.pending_save = true;
                }
                self.configure_hotkeys();
            }
            return;
        }
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
    fn shortcut_control(&mut self, ui: &mut egui::Ui, record: bool, interactive: bool) {
        muted(
            ui,
            if record {
                if self.prefs.record_mouse {
                    "Mouse recording shortcut"
                } else {
                    "Keystroke recording shortcut"
                }
            } else {
                "Activation key"
            },
        );
        let binding = if record {
            self.prefs.record_hotkey
        } else {
            self.prefs
                .macros
                .get(self.prefs.selected)
                .and_then(|m| m.activation_key)
        };
        let label = if self.shortcut_capture == Some(record) {
            "Press shortcut…".into()
        } else {
            hotkey_text(&binding)
        };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    interactive && !self.engine.status().recording && !self.engine.status().playing,
                    egui::Button::new(label).min_size(egui::vec2(
                        ui.available_width() - if binding.is_some() { 48.0 } else { 0.0 },
                        30.0,
                    )),
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
                    .add_enabled(interactive, egui::Button::new("Clear"))
                    .clicked()
            {
                if record {
                    self.prefs.record_hotkey = None;
                } else if let Some(m) = self.prefs.macros.get_mut(self.prefs.selected) {
                    m.activation_key = None;
                }
                self.configure_hotkeys();
            }
        });
    }
    fn controls(&mut self, ui: &mut egui::Ui, interactive: bool) {
        tutorial::region(ui, "macro-recording", |ui| {
            section(ui, "Record", |ui| {
                let status = self.engine.status();
                for (mouse, label) in [(true, "Record Mouse"), (false, "Record keystrokes")] {
                    let active = status.recording && self.prefs.record_mouse == mouse;
                    let response = ui.add_enabled(
                        interactive && !status.playing && (!status.recording || active),
                        egui::Button::new(if active { "Stop recording" } else { label })
                            .min_size(egui::vec2(ui.available_width(), 30.0)),
                    );
                    if mouse {
                        if let Some(rect) = control_screen_rect(ui.ctx(), response.rect) {
                            self.ignore_rect = Some(rect);
                        }
                        if active {
                            self.engine.set_mouse_exclusion(self.ignore_rect);
                        }
                    }
                    if response.clicked() {
                        self.prefs.record_mouse = mouse;
                        self.record();
                    }
                }
                ui.add_space(look::GAP);
                ui.add_enabled_ui(interactive && !status.recording && !status.playing, |ui| {
                    muted(ui, "Delays");
                    choices(
                        ui,
                        &mut self.prefs.delay_mode,
                        ["Recorded", "Fixed", "Custom"],
                    );
                    if self.prefs.delay_mode == 1 {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.prefs.default_delay)
                                .desired_width(100.0),
                        );
                    }
                });
                ui.add_space(look::SPACE_XS);
                self.shortcut_control(ui, true, interactive);
                if status.recording {
                    muted(
                        ui,
                        format!("Recording · {} events · Esc stops", status.recorded_steps),
                    );
                }
            });
        });
        section_divider(ui);
        tutorial::region(ui, "macro-repeat", |ui| {
            section(ui, "Playback", |ui| {
                let playing = self.engine.status().playing;
                if ui
                    .add_enabled(
                        interactive && !self.engine.status().recording,
                        egui::Button::new(
                            RichText::new(if playing {
                                "Stop playback"
                            } else {
                                "Play macro"
                            })
                            .color(look::TEXT),
                        )
                        .fill(look::ACCENT)
                        .min_size(egui::vec2(118.0, 30.0)),
                    )
                    .clicked()
                {
                    if playing {
                        self.engine.stop();
                        self.playing_macro = None;
                    } else {
                        self.play();
                    }
                }
                ui.add_enabled_ui(
                    interactive && !playing && !self.engine.status().recording,
                    |ui| {
                        choices(
                            ui,
                            &mut self.prefs.repeat_mode,
                            ["Once", "For duration", "Until stopped"],
                        );
                        if self.prefs.repeat_mode != 0 {
                            muted(ui, "Delay between repeats");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.prefs.repeat_delay)
                                    .desired_width(100.0),
                            );
                        }
                        if self.prefs.repeat_mode == 1 {
                            muted(ui, "Run for");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.prefs.run_duration)
                                    .desired_width(100.0),
                            );
                        }
                    },
                );
                ui.add_space(look::SPACE_XS);
                self.shortcut_control(ui, false, interactive);
            });
        });
    }
    fn library(&mut self, ui: &mut egui::Ui, editable: bool) {
        editor_card(ui, 626.0, |ui| {
            heading(ui, "Macros");
            self.inline_creation(ui, editable);
            ui.add_space(look::SPACE_XS);
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
                let selected = self.prefs.selected == index;
                let response = ui
                    .add_enabled_ui(editable, |ui| {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 52.0),
                            egui::Sense::click(),
                        );
                        if selected || response.hovered() {
                            ui.painter().rect_filled(rect, 6, look::SELECTED_BG);
                        }
                        ui.painter_at(rect).text(
                            rect.min + egui::vec2(12.0, 11.0),
                            egui::Align2::LEFT_TOP,
                            &name,
                            egui::FontId::proportional(12.0),
                            look::TEXT,
                        );
                        ui.painter_at(rect).text(
                            rect.min + egui::vec2(12.0, 31.0),
                            egui::Align2::LEFT_TOP,
                            format!("{count} steps · {key}"),
                            egui::FontId::proportional(11.0),
                            look::MUTED,
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::Button,
                                editable,
                                selected,
                                format!("{name}, {count} steps, {key}"),
                            )
                        });
                        response
                    })
                    .inner;
                if response.clicked() {
                    self.prefs.selected = index;
                    self.step = None;
                }
            }
            if self.prefs.macros.is_empty() {
                muted(ui, "No macros yet. Choose New to create one.");
                ui.add_space(look::SPACE_M);
                heading(ui, "Steps");
                muted(ui, "Create a macro to add steps.");
                ui.add_space((ui.min_rect().top() + 592.0 - ui.cursor().top() - 30.0).max(8.0));
                ui.add_enabled_ui(false, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let _ = ui.button("Add key");
                        let _ = ui.button("Add path");
                        let _ = ui.button("Add scroll");
                    });
                });
                return;
            }
            section_divider(ui);
            heading(ui, "Steps");
            let rows = self
                .selected_macro()
                .steps
                .iter()
                .enumerate()
                .map(|(i, s)| (i, action_name(&s.action), s.delay_ms))
                .collect::<Vec<_>>();
            let mut remove_step = None;
            egui::ScrollArea::vertical()
                .id_salt("macro-steps")
                .max_height((ui.available_height() - 54.0).clamp(150.0, 360.0))
                .show(ui, |ui| {
                    for (index, label, _) in rows {
                        ui.push_id(index, |ui| {
                            egui::Frame::new()
                                .fill(if self.step == Some(index) {
                                    look::SELECTED_BG
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .corner_radius(6)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if ui
                                        .add_enabled(
                                            editable,
                                            egui::Button::selectable(
                                                self.step == Some(index),
                                                format!("{:02}  {label}", index + 1),
                                            )
                                            .frame(false)
                                            .min_size(egui::vec2(ui.available_width(), 24.0)),
                                        )
                                        .clicked()
                                    {
                                        self.step = Some(index);
                                        self.timing_text.clear();
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("Delay");
                                        ui.add_enabled(
                                            editable,
                                            egui::DragValue::new(
                                                &mut self.prefs.macros[self.prefs.selected].steps
                                                    [index]
                                                    .delay_ms,
                                            )
                                            .range(0..=86_400_000)
                                            .suffix(" ms"),
                                        );
                                        if ui
                                            .add_enabled(
                                                editable,
                                                egui::Button::new("Delete").frame(false),
                                            )
                                            .clicked()
                                        {
                                            remove_step = Some(index);
                                        }
                                    });
                                });
                        });
                    }
                    if self.selected_macro().steps.is_empty() {
                        muted(ui, "Record actions or add a step to begin.");
                    }
                });
            if let Some(index) = remove_step {
                self.delete_step(index);
            }
            ui.add_space((ui.min_rect().top() + 592.0 - ui.cursor().top() - 30.0).max(8.0));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(editable, egui::Button::new("Add key"))
                    .clicked()
                {
                    self.new_key = true;
                    let _ = self.engine.configure_shortcuts(None, Vec::new());
                    self.configured_hotkeys = None;
                    if let Err(error) = self.engine.start_shortcut_capture() {
                        self.new_key = false;
                        self.message = Some(error);
                        self.configure_hotkeys();
                    }
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
        let recorded = self.selected_step().and_then(|s| match &s.action {
            Action::RecordedMovement { samples } => {
                Some(samples.iter().map(|s| s.point).collect::<Vec<_>>())
            }
            _ => None,
        });
        tutorial::region(ui, "macro-mouse", |ui| {
            editor_card(ui, 430.0, |ui| {
                heading(
                    ui,
                    if recorded.is_some() {
                        "Mouse movement"
                    } else {
                        "Mouse path"
                    },
                );
                ui.add_space(4.0);
                if recorded.is_none() {
                    ui.horizontal(|ui| {
                        choice(ui, &mut self.tool, CanvasTool::Select, "Select");
                        choice(ui, &mut self.tool, CanvasTool::Draw, "Draw path");
                        choice(ui, &mut self.tool, CanvasTool::Click, "Add click");
                    });
                } else {
                    muted(ui, "Recorded mouse movements are not editable");
                    ui.add_space(14.0);
                }
                let bounds = virtual_screen();
                muted(ui, format!("{} × {} desktop", bounds[2], bounds[3]));
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 296.0),
                    egui::Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 6, look::WINDOW_BG);
                for i in 1..(rect.width() / 40.0) as i32 {
                    let x = rect.left() + 40.0 * i as f32;
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.0, look::BORDER),
                    );
                }
                for i in 1..(rect.height() / 40.0) as i32 {
                    let y = rect.top() + 40.0 * i as f32;
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
                if let Some(points) = &recorded {
                    for segment in points.windows(2) {
                        painter.line_segment(
                            [to_canvas(&segment[0]), to_canvas(&segment[1])],
                            egui::Stroke::new(2.0, look::ACCENT),
                        );
                    }
                    for (point, label) in [(points.first(), "Start"), (points.last(), "End")] {
                        if let Some(point) = point {
                            let pos = to_canvas(point);
                            painter.circle_filled(pos, 3.5, look::TEXT);
                            painter.text(
                                pos + egui::vec2(0.0, 12.0),
                                egui::Align2::CENTER_TOP,
                                label,
                                egui::FontId::proportional(11.0),
                                look::MUTED,
                            );
                        }
                    }
                    return;
                }
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
        let recorded = self.selected_step().and_then(|s| match &s.action {
            Action::RecordedMovement { samples } => {
                Some(samples.iter().map(|s| s.delay_ms).sum::<u64>())
            }
            _ => None,
        });
        editor_card(ui, 184.0, |ui| {
            if let Some(duration) = recorded {
                heading(ui, "Recorded movement");
                muted(
                    ui,
                    "The captured timing and trajectory play back as recorded.",
                );
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        muted(ui, "Duration");
                        ui.label(duration_text(duration));
                    });
                    ui.vertical(|ui| {
                        muted(ui, "Editing");
                        ui.label("Read-only");
                    });
                });
                return;
            }
            let current = self.selected_step().and_then(|s| match s.action {
                Action::Path { duration_ms, .. } => Some(duration_ms),
                _ => None,
            });
            let mut duration = current.unwrap_or(0);
            ui.horizontal_wrapped(|ui| {
                heading(ui, "Movement timing");
                if ui
                    .add(choice_button("Milliseconds", !self.seconds))
                    .clicked()
                {
                    self.seconds = false;
                    self.timing_text.clear();
                }
                if ui.add(choice_button("Seconds", self.seconds)).clicked() {
                    self.seconds = true;
                    self.timing_text.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let id = ui.make_persistent_id("movement-duration-value");
                    if !ui.memory(|m| m.has_focus(id)) && self.timing_text.is_empty() {
                        self.timing_text = if self.seconds {
                            format!("{:.2} s", duration as f64 / 1000.0)
                        } else {
                            format!("{duration} ms")
                        };
                    }
                    let response = ui.add_enabled(
                        editable && current.is_some(),
                        egui::TextEdit::singleline(&mut self.timing_text)
                            .id(id)
                            .desired_width(75.0),
                    );
                    if response.lost_focus()
                        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        match engine::parse_timing(
                            &self.timing_text,
                            if self.seconds {
                                TimingScope::Seconds
                            } else {
                                TimingScope::Milliseconds
                            },
                        ) {
                            Ok(value) => duration = value,
                            Err(error) => self.message = Some(error),
                        }
                    }
                    if !response.has_focus() {
                        self.timing_text = if self.seconds {
                            format!("{:.2} s", duration as f64 / 1000.0)
                        } else {
                            format!("{duration} ms")
                        };
                    }
                });
            });
            ui.add_space(12.0);
            let maximum = if self.seconds { 10_000_u64 } else { 1_000 };
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 48.0),
                egui::Sense::click_and_drag(),
            );
            let left = rect.left() + 8.0;
            let right = rect.right() - 8.0;
            let y = rect.top() + 13.0;
            let enabled = editable && current.is_some() && ui.is_enabled();
            if enabled && (response.clicked() || response.dragged()) {
                if let Some(pos) = response.interact_pointer_pos() {
                    let ratio = ((pos.x - left) / (right - left).max(1.0)).clamp(0.0, 1.0);
                    let tick = (ratio * 10.0).round();
                    duration = if response.clicked()
                        && (pos.x - (left + (right - left) * tick / 10.0)).abs() <= 8.0
                    {
                        maximum * tick as u64 / 10
                    } else {
                        (ratio * maximum as f32).round() as u64
                    };
                    self.timing_text.clear();
                    response.request_focus();
                }
            }
            if enabled && response.has_focus() {
                let delta = ui.input(|i| {
                    i.key_pressed(egui::Key::ArrowRight) as i32
                        - i.key_pressed(egui::Key::ArrowLeft) as i32
                });
                if delta != 0 {
                    duration = (duration as i64 + delta as i64 * if self.seconds { 10 } else { 1 })
                        .clamp(0, maximum as i64) as u64;
                    self.timing_text.clear();
                }
            }
            ui.painter().line_segment(
                [egui::pos2(left, y), egui::pos2(right, y)],
                egui::Stroke::new(5.0, look::BORDER),
            );
            let handle_x =
                left + (right - left) * (duration as f32 / maximum as f32).clamp(0.0, 1.0);
            if current.is_some() {
                ui.painter().line_segment(
                    [egui::pos2(left, y), egui::pos2(handle_x, y)],
                    egui::Stroke::new(5.0, look::ACCENT),
                );
            }
            for i in 0..=10 {
                let x = left + (right - left) * i as f32 / 10.0;
                ui.painter()
                    .circle_filled(egui::pos2(x, y), 3.0, look::MUTED);
                ui.painter().text(
                    egui::pos2(x, rect.top() + 28.0),
                    egui::Align2::CENTER_TOP,
                    format!("{}", if self.seconds { i } else { i * 100 }),
                    egui::FontId::proportional(10.0),
                    look::MUTED,
                );
            }
            let thumb =
                egui::Rect::from_center_size(egui::pos2(handle_x, y), egui::vec2(13.0, 24.0));
            ui.painter().rect(
                thumb,
                6,
                look::WINDOW_BG,
                egui::Stroke::new(1.0, look::TEXT),
                egui::StrokeKind::Inside,
            );
            ui.add_space(12.0);
            muted(
                ui,
                if current.is_none() {
                    "Select a mouse path to set its duration."
                } else if self.seconds {
                    "Click the duration value to type a time. Click a dot for an exact second."
                } else {
                    "Click the duration value to type a time. Click a dot for an exact 100 ms."
                },
            );
            if current.is_some() && editable {
                if let Some(Step {
                    action: Action::Path { duration_ms, .. },
                    ..
                }) = self.selected_step()
                {
                    *duration_ms = duration;
                }
            }
        });
    }
    fn inspector(&mut self, ui: &mut egui::Ui, editable: bool) {
        tutorial::region(ui, "macro-step", |ui| {
            editor_card(ui, 626.0, |ui| {
                heading(ui, "Selected step");
                let Some(index) = self.step else {
                    muted(ui, "Choose a step to edit its timing and action.");
                    return;
                };
                muted(ui, format!("Step {}", index + 1));
                section_divider(ui);
                let mut remove = false;
                let mut up = false;
                let mut down = false;
                let mut capture = false;
                if let Some(step) = self.selected_step() {
                    ui.add_enabled_ui(editable, |ui| {
                        heading(ui, &action_name(&step.action));
                        if matches!(step.action, Action::RecordedMovement { .. }) {
                            muted(ui, "Recorded mouse movements are not editable");
                        }
                        ui.add_space(look::SPACE_M);
                        muted(ui, "Delay before this step");
                        ui.add_sized(
                            [ui.available_width(), 30.0],
                            egui::DragValue::new(&mut step.delay_ms)
                                .range(0..=86_400_000)
                                .suffix(" ms"),
                        );
                        match &mut step.action {
                            Action::RecordedMovement { samples } => {
                                ui.add_space(16.0);
                                muted(ui, "Duration");
                                ui.label(duration_text(samples.iter().map(|s| s.delay_ms).sum()));
                            }
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
                        section_divider(ui);
                        ui.horizontal_wrapped(|ui| {
                            up = ui.button("Move up").clicked();
                            down = ui.button("Move down").clicked();
                        });
                        remove = ui.button("Delete step").clicked();
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
        for m in &mut self.prefs.macros {
            let grouped = engine::group_recorded_movements(m.steps.clone());
            if grouped != m.steps {
                m.steps = grouped;
                self.pending_save = true;
            }
        }
        if !self.prefs.macros.is_empty() {
            self.selected_macro();
        }
        if let Some(previous) = self.prefs.play_hotkey.take() {
            if !self.prefs.macros.is_empty() && self.selected_macro().activation_key.is_none() {
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

        ui.visuals_mut().selection.stroke.color = look::TEXT;
        ui.visuals_mut().selection.bg_fill = look::SELECTED_BG;
        ui.spacing_mut().interact_size.y = 30.0;
        ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
        ui.spacing_mut().item_spacing.x = 12.0;
        if interactive
            && !busy
            && !self.new_key
            && self.shortcut_capture.is_none()
            && !self.key_capture
            && self.new_name.is_none()
            && self.confirm_delete.is_none()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
        {
            if let Some(index) = self.step {
                self.delete_step(index);
            } else if !self.prefs.macros.is_empty() {
                self.confirm_delete = Some(self.prefs.selected);
            }
        }
        egui::ScrollArea::vertical().id_salt("macro-manager-workspace").show(ui,|ui|{
            ui.label(RichText::new("Macro Manager").size(look::TEXT_TITLE).strong());
            muted(ui,"Record keystrokes and mouse actions, edit paths and delays, and replay macros with your own shortcuts. Changes save automatically.");
            if let Some(message)=self.message.clone(){if look::banner(ui,&message){self.message=None;}}
            if self.new_key { ui.label("Press a key to add it. Esc cancels."); }
            ui.add_space(look::SPACE_M);
            let editable=interactive && !busy && !self.new_key && self.shortcut_capture.is_none();
            let width=ui.available_width();
            ui.horizontal_top(|ui|{
                ui.vertical(|ui|{ui.set_width(270.0_f32.min(width*0.25));self.library(ui,editable);});
                if width >= 1000.0 {
                    ui.vertical(|ui|{
                        ui.set_width((width-662.0).max(300.0));
                        ui.add_enabled_ui(!self.prefs.macros.is_empty(), |ui| {self.canvas(ui,editable);ui.add_space(8.0);self.timing(ui,editable);});
                    });
                    ui.vertical(|ui|{ui.set_width(ui.available_width().max(368.0));self.options(ui,interactive,editable);});
                } else {
                    ui.vertical(|ui|{
                        ui.set_width(ui.available_width());
                        self.options(ui,interactive,editable);
                        ui.add_space(look::SPACE_M);ui.add_enabled_ui(!self.prefs.macros.is_empty(), |ui| {self.canvas(ui,editable);self.timing(ui,editable);});
                    });
                }
            });
        });
        if !tour {
            self.dialogs(ui.ctx());
        }
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
    fn tick(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.engine.set_event_waker(move || ctx.request_repaint());
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
    fn inline_macro_creation_accepts_enter_and_escape_cancels() {
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut manager = MacroManager {
            new_name: Some("  My routine  ".into()),
            ..Default::default()
        };
        for key in [egui::Key::Enter, egui::Key::Escape] {
            if key == egui::Key::Escape {
                manager.new_name = Some("Cancelled".into());
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ui| manager.inline_creation(ui, true),
            );
            output.textures_delta.clear();
            assert!(manager.new_name.is_none());
            assert_eq!(manager.prefs.macros.len(), 1);
            assert_eq!(manager.prefs.macros[0].name, "My routine");
        }
    }
    #[test]
    fn timing_checkpoint_ten_sets_exactly_ten_seconds() {
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut manager = MacroManager::default();
        manager.prefs.macros.push(Macro {
            name: "Path".into(),
            steps: vec![Step {
                delay_ms: 0,
                action: Action::Path {
                    points: vec![],
                    duration_ms: 1000,
                    clicks: vec![],
                },
            }],
            ..Default::default()
        });
        manager.step = Some(0);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| manager.timing(ui, true));
        let endpoint = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.radius == 3.0 => Some(circle.center),
                _ => None,
            })
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .expect("ten-second dot");
        output.textures_delta.clear();
        for pressed in [true, false] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(endpoint),
                        egui::Event::PointerButton {
                            pos: endpoint,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| manager.timing(ui, true),
            );
            output.textures_delta.clear();
        }
        assert!(matches!(
            manager.prefs.macros[0].steps[0].action,
            Action::Path {
                duration_ms: 10_000,
                ..
            }
        ));
    }
    #[test]
    fn recorded_movement_view_has_no_path_edit_controls() {
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut manager = MacroManager::default();
        manager.prefs.macros.push(Macro {
            name: "Recorded".into(),
            steps: engine::group_recorded_movements(vec![
                Step {
                    delay_ms: 0,
                    action: Action::Move { x: 20, y: 30 },
                },
                Step {
                    delay_ms: 40,
                    action: Action::Move { x: 80, y: 90 },
                },
            ]),
            ..Default::default()
        });
        manager.step = Some(0);
        let before = manager.prefs.macros[0].clone();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            manager.canvas(ui, true);
            manager.timing(ui, true);
            manager.inspector(ui, true);
        });
        let labels = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(labels.contains("Recorded mouse movements are not editable"));
        for absent in [
            "Draw path",
            "Add click",
            "Remove last point",
            "Custom timing",
        ] {
            assert!(!labels.contains(absent));
        }
        assert_eq!(manager.prefs.macros[0], before);
        output.textures_delta.clear();
    }
    #[test]
    fn empty_library_and_deleting_last_macro_stay_empty_after_save_and_render() {
        let mut manager = MacroManager {
            saving_allowed: true,
            ..Default::default()
        };
        assert!(manager.prefs.macros.is_empty());
        manager.prefs.macros.push(Macro {
            name: "Only macro".into(),
            ..Default::default()
        });
        manager.delete_macro(0);
        let mut prefs = BTreeMap::new();
        assert!(manager.save_preferences(&mut prefs));
        let mut reopened = MacroManager {
            saving_allowed: true,
            ..Default::default()
        };
        reopened.load_preferences(&prefs);
        let ctx = egui::Context::default();
        look::apply_style(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            reopened.ui(ui, &mut prefs);
        });
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for panel in [
            "Steps",
            "Mouse path",
            "Movement timing",
            "Macro settings",
            "Playback",
        ] {
            assert!(labels.contains(panel), "Empty library must retain {panel}");
        }
        output.textures_delta.clear();
        assert!(reopened.prefs.macros.is_empty());
        assert!(reopened.selected_step().is_none());
    }
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
        if manager.prefs.macros.is_empty() {
            manager.prefs.macros.push(Macro {
                name: "Test macro".into(),
                ..Default::default()
            });
        }
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
            if manager.prefs.macros.is_empty() {
                manager.prefs.macros.push(Macro {
                    name: "Test macro".into(),
                    ..Default::default()
                });
            }
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
        if manager.prefs.macros.is_empty() {
            manager.prefs.macros.push(Macro {
                name: "Test macro".into(),
                ..Default::default()
            });
        }
        manager.prefs.macros[0].name = "Saved macro".into();
        if manager.prefs.macros.is_empty() {
            manager.prefs.macros.push(Macro {
                name: "Test macro".into(),
                ..Default::default()
            });
        }
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
