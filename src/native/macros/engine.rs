//! Macro execution. Hooks exist only for an explicitly active recording/playback.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    #[serde(default)]
    pub activation_key: Option<Hotkey>,
    pub name: String,
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub delay_ms: u64,
    pub action: Action,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathClick {
    pub point_index: usize,
    pub button: MouseButton,
    pub count: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Key {
        vk: u16,
        down: bool,
    },
    Move {
        x: i32,
        y: i32,
    },
    Click {
        button: MouseButton,
        count: u8,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Scroll {
        delta: i32,
        horizontal: bool,
    },
    Path {
        points: Vec<Point>,
        duration_ms: u64,
        clicks: Vec<PathClick>,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct RecordOptions {
    pub ignore_rect: Option<[i32; 4]>,
    pub keys: bool,
    pub mouse_movement: bool,
    pub mouse_buttons: bool,
    pub mouse_scroll: bool,
}
impl Default for RecordOptions {
    fn default() -> Self {
        Self {
            ignore_rect: None,
            keys: true,
            mouse_movement: true,
            mouse_buttons: true,
            mouse_scroll: true,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RepeatOptions {
    pub repeat: bool,
    pub delay_ms: u64,
    pub duration_ms: Option<u64>,
}
impl Default for RepeatOptions {
    fn default() -> Self {
        Self {
            repeat: false,
            delay_ms: 0,
            duration_ms: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub recording: bool,
    pub playing: bool,
    pub recorded_steps: usize,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}
#[derive(Clone, Copy, Debug)]
pub enum ControlEvent {
    Record,
    Play(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingScope {
    Milliseconds,
    Seconds,
}
pub fn parse_timing(input: &str, scope: TimingScope) -> Result<u64, String> {
    let input = input.trim().to_ascii_lowercase();
    let split = input
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let number: f64 = input[..split]
        .trim()
        .parse()
        .map_err(|_| "Enter a positive time, for example 250ms or 2.5s".to_owned())?;
    let multiplier = match input[split..].trim() {
        "ms" | "millisecond" | "milliseconds" => 1.0,
        "s" | "sec" | "second" | "seconds" => 1000.0,
        "" => {
            if scope == TimingScope::Seconds {
                1000.0
            } else {
                1.0
            }
        }
        _ => return Err("Use ms, milliseconds, s, sec or seconds".into()),
    };
    let millis = number * multiplier;
    if !millis.is_finite() || millis < 0.0 || millis >= u64::MAX as f64 {
        return Err("Time is outside the supported range".into());
    }
    Ok(millis.round() as u64)
}

#[derive(Default)]
struct Shared {
    capture_shortcut: bool,
    captured_shortcut: Option<Hotkey>,
    status: Status,
    steps: Vec<Step>,
    last: Option<Instant>,
    options: RecordOptions,
    suppressed_buttons: HashSet<MouseButton>,
    keys: HashSet<u16>,
    buttons: HashSet<MouseButton>,
}
struct Worker {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}
impl Shared {
    fn capture_key(&mut self, vk: u32, modifiers: u32) {
        self.captured_shortcut = Some(Hotkey { vk, modifiers });
        self.capture_shortcut = false;
    }
}
impl Worker {
    fn stop(self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.handle.join();
    }
}
#[derive(Default)]
pub struct Engine {
    shared: Arc<Mutex<Shared>>,
    worker: Option<Worker>,
    hotkeys: Option<Worker>,
    events: Arc<Mutex<Vec<ControlEvent>>>,
    bindings: (Option<Hotkey>, Vec<(usize, Hotkey)>),
}
impl Engine {
    pub fn start_shortcut_capture(&mut self) -> Result<(), String> {
        self.stop();
        *self.shared.lock().unwrap() = Shared {
            capture_shortcut: true,
            ..Default::default()
        };
        self.start_worker(None)
    }
    pub fn take_captured_shortcut(&mut self) -> Option<Hotkey> {
        self.shared.lock().unwrap().captured_shortcut.take()
    }
    pub fn shortcuts_configured(&self) -> bool {
        self.hotkeys.is_some()
    }
    pub fn status(&self) -> Status {
        self.shared.lock().unwrap().status.clone()
    }
    pub fn poll_recorded(&self) -> Vec<Step> {
        self.shared.lock().unwrap().steps.clone()
    }
    pub fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
        }
    }
    pub fn stop_record(&mut self) -> Vec<Step> {
        self.stop();
        self.poll_recorded()
    }
    pub fn start_record(&mut self, options: RecordOptions) -> Result<(), String> {
        self.stop();
        {
            let mut s = self.shared.lock().unwrap();
            *s = Shared {
                options,
                last: Some(Instant::now()),
                ..Default::default()
            };
            s.status.recording = true;
        }
        self.start_worker(None)
    }
    pub fn play(&mut self, steps: Vec<Step>, options: RepeatOptions) -> Result<(), String> {
        self.stop();
        if steps.is_empty() {
            return Err("Add or record some steps first".into());
        }
        {
            let mut s = self.shared.lock().unwrap();
            s.status.error = None;
            s.status.playing = true;
        }
        self.start_worker(Some((steps, options)))
    }
    fn start_worker(&mut self, playback: Option<(Vec<Step>, RepeatOptions)>) -> Result<(), String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let shared = self.shared.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handle = thread::spawn(move || unsafe {
            run_worker(shared, flag, playback, tx);
        });
        self.worker = Some(Worker { cancel, handle });
        match rx.recv() {
            Ok(result) => {
                if result.is_err() {
                    self.stop();
                }
                result
            }
            Err(_) => {
                self.stop();
                Err("Input worker could not start".into())
            }
        }
    }
    pub fn configure_shortcuts(
        &mut self,
        record: Option<Hotkey>,
        play: Vec<(usize, Hotkey)>,
    ) -> Result<(), String> {
        let desired = (record, play.clone());
        if self.bindings == desired {
            return Ok(());
        }
        let mut registrations: Vec<(Hotkey, ControlEvent)> = record
            .into_iter()
            .map(|key| (key, ControlEvent::Record))
            .collect();
        registrations.extend(
            play.into_iter()
                .map(|(index, key)| (key, ControlEvent::Play(index))),
        );
        for (i, (key, _)) in registrations.iter().enumerate() {
            if key.vk == 0x1b
                || key.vk == 0
                || registrations[..i].iter().any(|(other, _)| other == key)
            {
                return Err(
                    "Every macro needs a different activation key; Esc is reserved for Stop."
                        .into(),
                );
            }
        }
        if let Some(worker) = self.hotkeys.take() {
            worker.stop();
        }
        self.events.lock().unwrap().clear();
        self.bindings = (None, Vec::new());
        SHORTCUTS.lock().unwrap().clear();
        if registrations.is_empty() {
            return Ok(());
        }
        let shortcut_keys = registrations.iter().map(|(key, _)| *key).collect();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let events = self.events.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handle = thread::spawn(move || unsafe {
            let mut registered = Vec::new();
            for (index, (key, _)) in registrations.iter().enumerate() {
                let id = index as i32 + 1;
                if RegisterHotKey(0, id, key.modifiers | 0x4000, key.vk) == 0 {
                    for id in registered {
                        UnregisterHotKey(0, id);
                    }
                    let _ = tx.send(Err(
                        "Activation key is already in use or unavailable.".to_owned()
                    ));
                    return;
                }
                registered.push(id);
            }
            let _ = tx.send(Ok(()));
            while !flag.load(Ordering::SeqCst) {
                let mut msg = Msg::default();
                while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
                    if msg.message == 0x312 {
                        if let Some((_, event)) = msg
                            .wparam
                            .checked_sub(1)
                            .and_then(|index| registrations.get(index))
                        {
                            events.lock().unwrap().push(*event);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(10));
            }
            for id in registered {
                UnregisterHotKey(0, id);
            }
        });
        self.hotkeys = Some(Worker { cancel, handle });
        let result = rx
            .recv()
            .unwrap_or_else(|_| Err("Shortcut worker could not start.".into()));
        if result.is_err() {
            if let Some(worker) = self.hotkeys.take() {
                worker.stop();
            }
        }
        if result.is_ok() {
            self.bindings = desired;
            *SHORTCUTS.lock().unwrap() = shortcut_keys;
        }
        result
    }
    pub fn poll_control_events(&mut self) -> Vec<ControlEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.hotkeys.take() {
            worker.stop();
        }
    }
}

// Local ABI declarations keep input hooks isolated from UI/window wrappers.
#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: WinPoint,
    private: u32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct WinPoint {
    x: i32,
    y: i32,
}
#[repr(C)]
struct KeyboardHook {
    vk: u32,
    scan: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
struct MouseHook {
    point: WinPoint,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct MouseInput {
    dx: i32,
    dy: i32,
    data: u32,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct KeyInput {
    vk: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
}
#[repr(C)]
union InputData {
    mouse: MouseInput,
    key: KeyInput,
}
#[repr(C)]
struct Input {
    kind: u32,
    data: InputData,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowsHookExW(
        id: i32,
        callback: unsafe extern "system" fn(i32, usize, isize) -> isize,
        module: isize,
        thread: u32,
    ) -> isize;
    fn UnhookWindowsHookEx(hook: isize) -> i32;
    fn CallNextHookEx(hook: isize, code: i32, wp: usize, lp: isize) -> isize;
    fn PeekMessageW(msg: *mut Msg, window: isize, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn SendInput(count: u32, input: *const Input, size: i32) -> u32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn RegisterHotKey(window: isize, id: i32, modifiers: u32, vk: u32) -> i32;
    fn UnregisterHotKey(window: isize, id: i32) -> i32;
    fn GetAsyncKeyState(vk: i32) -> i16;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
    fn WindowFromPoint(point: WinPoint) -> isize;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetCurrentProcessId() -> u32;
}
struct HookState {
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
}
static HOOK: Mutex<Option<HookState>> = Mutex::new(None);
static SHORTCUTS: Mutex<Vec<Hotkey>> = Mutex::new(Vec::new());
fn control_key(vk: u16) -> bool {
    matches!(vk,0x10..=0x12|0x5b|0x5c|0xa0..=0xa5)
}
pub fn key_name(vk: u16) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0d => "Enter".into(),
        0x10 => "Shift".into(),
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0x1b => "Esc".into(),
        0x20 => "Space".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2e => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5a => (vk as u8 as char).to_string(),
        0x60..=0x69 => format!("Keypad {}", vk - 0x60),
        0x6a => "Keypad *".into(),
        0x6b => "Keypad +".into(),
        0x6d => "Keypad -".into(),
        0x6e => "Keypad .".into(),
        0x6f => "Keypad /".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6f),
        _ => format!("Key {vk:#04X}"),
    }
}
impl std::fmt::Display for MouseButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Left => "Left",
                Self::Right => "Right",
                Self::Middle => "Middle",
                Self::X1 => "X1",
                Self::X2 => "X2",
            }
        )
    }
}
unsafe extern "system" fn keyboard(code: i32, wp: usize, lp: isize) -> isize {
    if code >= 0 {
        let data = &*(lp as *const KeyboardHook);
        if data.flags & 0x10 == 0 {
            if let Some(hook) = HOOK.lock().unwrap().as_ref() {
                {
                    let mut s = hook.shared.lock().unwrap();
                    if s.capture_shortcut
                        && (wp == 0x100 || wp == 0x104)
                        && !control_key(data.vk as u16)
                    {
                        let modifiers = (if GetAsyncKeyState(0x12) < 0 { 1 } else { 0 })
                            | (if GetAsyncKeyState(0x11) < 0 { 2 } else { 0 })
                            | (if GetAsyncKeyState(0x10) < 0 { 4 } else { 0 })
                            | (if GetAsyncKeyState(0x5b) < 0 || GetAsyncKeyState(0x5c) < 0 {
                                8
                            } else {
                                0
                            });
                        s.capture_key(data.vk, modifiers);
                        hook.cancel.store(true, Ordering::SeqCst);
                    }
                }
                if data.vk == 0x1b {
                    hook.cancel.store(true, Ordering::SeqCst);
                } else {
                    let mut s = hook.shared.lock().unwrap();
                    if s.status.recording && s.options.keys {
                        let down = wp == 0x100 || wp == 0x104;
                        let modifiers = (if GetAsyncKeyState(0x12) < 0 { 1 } else { 0 })
                            | (if GetAsyncKeyState(0x11) < 0 { 2 } else { 0 })
                            | (if GetAsyncKeyState(0x10) < 0 { 4 } else { 0 })
                            | (if GetAsyncKeyState(0x5b) < 0 || GetAsyncKeyState(0x5c) < 0 {
                                8
                            } else {
                                0
                            });
                        let shortcut = SHORTCUTS
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|k| k.vk == data.vk && k.modifiers & 15 == modifiers);
                        let mut foreground_process = 0;
                        let in_app = GetWindowThreadProcessId(
                            GetForegroundWindow(),
                            &mut foreground_process,
                        ) != 0
                            && foreground_process == GetCurrentProcessId();
                        if shortcut {
                            while s.steps.last().is_some_and(|step| matches!(step.action,Action::Key{vk,down:true} if control_key(vk))) { s.steps.pop(); }
                            s.keys.retain(|vk| !control_key(*vk));
                            s.status.recorded_steps = s.steps.len();
                        } else if !in_app {
                            // Ignore releases of keys pressed before recording began.
                            if down {
                                s.keys.insert(data.vk as u16);
                                record(
                                    &mut s,
                                    Action::Key {
                                        vk: data.vk as u16,
                                        down,
                                    },
                                );
                            } else if s.keys.remove(&(data.vk as u16)) {
                                record(
                                    &mut s,
                                    Action::Key {
                                        vk: data.vk as u16,
                                        down,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(0, code, wp, lp)
}
unsafe extern "system" fn mouse(code: i32, wp: usize, lp: isize) -> isize {
    if code >= 0 {
        let data = &*(lp as *const MouseHook);
        if data.flags & 1 == 0 {
            if let Some(hook) = HOOK.lock().unwrap().as_ref() {
                let mut s = hook.shared.lock().unwrap();
                if s.status.recording {
                    let mut point_process = 0;
                    let own_window =
                        GetWindowThreadProcessId(WindowFromPoint(data.point), &mut point_process)
                            != 0
                            && point_process == GetCurrentProcessId();
                    let inside = own_window
                        || s.options.ignore_rect.is_some_and(|r| {
                            data.point.x >= r[0]
                                && data.point.y >= r[1]
                                && data.point.x < r[2]
                                && data.point.y < r[3]
                        });
                    let button = match wp {
                        0x201 | 0x202 => Some(MouseButton::Left),
                        0x204 | 0x205 => Some(MouseButton::Right),
                        0x207 | 0x208 => Some(MouseButton::Middle),
                        0x20b | 0x20c => Some(if data.data >> 16 == 1 {
                            MouseButton::X1
                        } else {
                            MouseButton::X2
                        }),
                        _ => None,
                    };
                    if let Some(button) = button {
                        let down = matches!(wp, 0x201 | 0x204 | 0x207 | 0x20b);
                        if inside && down {
                            s.suppressed_buttons.insert(button);
                        }
                        let suppressed = s.suppressed_buttons.contains(&button);
                        if !down {
                            s.suppressed_buttons.remove(&button);
                        }
                        // Pair an outside press even if its release lands over Toolboks.
                        if inside && !down && !suppressed && s.buttons.remove(&button) {
                            record(
                                &mut s,
                                Action::Button {
                                    button,
                                    down: false,
                                },
                            );
                        }
                        if !inside && !suppressed && s.options.mouse_buttons {
                            if s.options.mouse_movement {
                                record(
                                    &mut s,
                                    Action::Move {
                                        x: data.point.x,
                                        y: data.point.y,
                                    },
                                );
                            }
                            if down {
                                s.buttons.insert(button);
                                record(&mut s, Action::Button { button, down });
                            } else if s.buttons.remove(&button) {
                                record(&mut s, Action::Button { button, down });
                            }
                        }
                    } else if !inside {
                        match wp {
                            0x200 if s.options.mouse_movement => record(
                                &mut s,
                                Action::Move {
                                    x: data.point.x,
                                    y: data.point.y,
                                },
                            ),
                            0x20a | 0x20e if s.options.mouse_scroll => record(
                                &mut s,
                                Action::Scroll {
                                    delta: (data.data >> 16) as i16 as i32,
                                    horizontal: wp == 0x20e,
                                },
                            ),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(0, code, wp, lp)
}
fn record(s: &mut Shared, action: Action) {
    let now = Instant::now();
    let delay_ms = s.last.replace(now).map_or(0, |last| {
        now.duration_since(last).as_millis().min(u64::MAX as u128) as u64
    });
    s.steps.push(Step { delay_ms, action });
    s.status.recorded_steps = s.steps.len();
}
unsafe fn pump() {
    let mut msg = Msg::default();
    while PeekMessageW(&mut msg, 0, 0, 0, 1) != 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
fn wait(ms: u64, cancel: &AtomicBool, deadline: Option<Instant>) -> bool {
    let start = Instant::now();
    loop {
        unsafe {
            pump();
        }
        if cancel.load(Ordering::SeqCst) || deadline.is_some_and(|d| Instant::now() >= d) {
            return false;
        }
        if start.elapsed().as_millis() >= ms as u128 {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
}
unsafe fn send(input: Input) -> Result<(), String> {
    if SendInput(1, &input, std::mem::size_of::<Input>() as i32) == 1 {
        Ok(())
    } else {
        Err(
            "Windows blocked input injection. Elevated apps may require matching privileges."
                .into(),
        )
    }
}
unsafe fn key(vk: u16, down: bool) -> Result<(), String> {
    let extended = matches!(vk, 0x21..=0x2e | 0x5b | 0x5c | 0x6f | 0x90 | 0xa3 | 0xa5);
    send(Input {
        kind: 1,
        data: InputData {
            key: KeyInput {
                vk,
                scan: 0,
                flags: (if down { 0 } else { 2 }) | u32::from(extended),
                time: 0,
                extra: 0,
            },
        },
    })
}
unsafe fn button(button: MouseButton, down: bool) -> Result<(), String> {
    let (flags, data) = match button {
        MouseButton::Left => (if down { 2 } else { 4 }, 0),
        MouseButton::Right => (if down { 8 } else { 16 }, 0),
        MouseButton::Middle => (if down { 32 } else { 64 }, 0),
        MouseButton::X1 => (if down { 128 } else { 256 }, 1),
        MouseButton::X2 => (if down { 128 } else { 256 }, 2),
    };
    send(Input {
        kind: 0,
        data: InputData {
            mouse: MouseInput {
                dx: 0,
                dy: 0,
                data,
                flags,
                time: 0,
                extra: 0,
            },
        },
    })
}
unsafe fn clicks(b: MouseButton, count: u8) -> Result<(), String> {
    for _ in 0..count {
        button(b, true)?;
        if let Err(error) = button(b, false) {
            let _ = button(b, false);
            return Err(error);
        }
    }
    Ok(())
}
unsafe fn move_to(p: Point) -> Result<(), String> {
    if SetCursorPos(p.x, p.y) != 0 {
        Ok(())
    } else {
        Err("Windows could not move the pointer".into())
    }
}
unsafe fn run_worker(
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    playback: Option<(Vec<Step>, RepeatOptions)>,
    tx: std::sync::mpsc::SyncSender<Result<(), String>>,
) {
    {
        let mut hook = HOOK.lock().unwrap();
        if hook.is_some() {
            let _ = tx.send(Err("Another macro input session is active".into()));
            let mut s = shared.lock().unwrap();
            s.status.recording = false;
            s.status.playing = false;
            return;
        }
        *hook = Some(HookState {
            shared: shared.clone(),
            cancel: cancel.clone(),
        });
    }
    let module = GetModuleHandleW(std::ptr::null());
    let kh = SetWindowsHookExW(13, keyboard, module, 0);
    let mh = if playback.is_none() {
        SetWindowsHookExW(14, mouse, module, 0)
    } else {
        0
    };
    if kh == 0 || (playback.is_none() && mh == 0) {
        if kh != 0 {
            UnhookWindowsHookEx(kh);
        }
        if mh != 0 {
            UnhookWindowsHookEx(mh);
        }
        *HOOK.lock().unwrap() = None;
        let mut s = shared.lock().unwrap();
        s.status.recording = false;
        s.status.playing = false;
        s.status.error = Some("Windows input hooks could not start".into());
        let _ = tx.send(Err(s.status.error.clone().unwrap()));
        return;
    }
    let _ = tx.send(Ok(()));
    let mut held_keys = HashSet::new();
    let mut held_buttons = HashSet::new();
    let result = (|| -> Result<(), String> {
        if let Some((steps, options)) = playback {
            let deadline = options
                .duration_ms
                .and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
            'runs: loop {
                for step in &steps {
                    if !wait(step.delay_ms, &cancel, deadline) {
                        break 'runs;
                    }
                    match &step.action {
                        Action::Key { vk, down } => {
                            key(*vk, *down)?;
                            if *down {
                                held_keys.insert(*vk);
                            } else {
                                held_keys.remove(vk);
                            }
                        }
                        Action::Move { x, y } => move_to(Point { x: *x, y: *y })?,
                        Action::Button { button: b, down } => {
                            button(*b, *down)?;
                            if *down {
                                held_buttons.insert(*b);
                            } else {
                                held_buttons.remove(b);
                            }
                        }
                        Action::Click { button: b, count } => clicks(*b, *count)?,
                        Action::Scroll { delta, horizontal } => send(Input {
                            kind: 0,
                            data: InputData {
                                mouse: MouseInput {
                                    dx: 0,
                                    dy: 0,
                                    data: *delta as u32,
                                    flags: if *horizontal { 0x1000 } else { 0x800 },
                                    time: 0,
                                    extra: 0,
                                },
                            },
                        })?,
                        Action::Path {
                            points,
                            duration_ms,
                            clicks: path_clicks,
                        } => {
                            let start = Instant::now();
                            for (i, p) in points.iter().enumerate() {
                                let target = if points.len() > 1 {
                                    (*duration_ms as u128 * i as u128 / (points.len() - 1) as u128)
                                        as u64
                                } else {
                                    0
                                };
                                if i > 0 && *duration_ms > 0 {
                                    let previous = points[i - 1];
                                    let segment_start = (*duration_ms as u128 * (i - 1) as u128
                                        / (points.len() - 1) as u128)
                                        as u64;
                                    loop {
                                        let elapsed =
                                            start.elapsed().as_millis().min(u64::MAX as u128)
                                                as u64;
                                        if elapsed >= target {
                                            break;
                                        }
                                        if !wait((target - elapsed).min(8), &cancel, deadline) {
                                            break 'runs;
                                        }
                                        let elapsed =
                                            start.elapsed().as_millis().min(u64::MAX as u128)
                                                as u64;
                                        let fraction = (elapsed.saturating_sub(segment_start)
                                            as f64
                                            / target.saturating_sub(segment_start).max(1) as f64)
                                            .min(1.0);
                                        move_to(Point {
                                            x: (previous.x as f64
                                                + (p.x as f64 - previous.x as f64) * fraction)
                                                .round()
                                                as i32,
                                            y: (previous.y as f64
                                                + (p.y as f64 - previous.y as f64) * fraction)
                                                .round()
                                                as i32,
                                        })?;
                                    }
                                }
                                let elapsed =
                                    start.elapsed().as_millis().min(u64::MAX as u128) as u64;
                                if !wait(target.saturating_sub(elapsed), &cancel, deadline) {
                                    break 'runs;
                                }
                                move_to(*p)?;
                                for click in path_clicks.iter().filter(|c| c.point_index == i) {
                                    clicks(click.button, click.count)?;
                                }
                            }
                            let elapsed = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
                            if !wait(duration_ms.saturating_sub(elapsed), &cancel, deadline) {
                                break 'runs;
                            }
                        }
                    }
                }
                if !options.repeat || !wait(options.delay_ms.max(1), &cancel, deadline) {
                    break;
                }
            }
        } else {
            while wait(10, &cancel, None) {}
        }
        Ok(())
    })();
    for vk in held_keys {
        let _ = key(vk, false);
    }
    for b in held_buttons {
        let _ = button(b, false);
    }
    UnhookWindowsHookEx(kh);
    if mh != 0 {
        UnhookWindowsHookEx(mh);
    }
    *HOOK.lock().unwrap() = None;
    let mut s = shared.lock().unwrap();
    // A stopped recording must not replay a permanently held key/button.
    if s.status.recording {
        let keys: Vec<_> = s.keys.drain().collect();
        for vk in keys {
            record(&mut s, Action::Key { vk, down: false });
        }
        let buttons: Vec<_> = s.buttons.drain().collect();
        for button in buttons {
            record(
                &mut s,
                Action::Button {
                    button,
                    down: false,
                },
            );
        }
    }
    s.status.recording = false;
    s.status.playing = false;
    if let Err(error) = result {
        s.status.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_capture_preserves_keypad_identity_and_modifiers() {
        let mut shared = Shared::default();
        for vk in [0x34, 0x60, 0x64, 0x69, 0x6a, 0x6b, 0x6d, 0x6e, 0x6f] {
            shared.capture_shortcut = true;
            shared.capture_key(vk, 6);
            assert_eq!(shared.captured_shortcut, Some(Hotkey { vk, modifiers: 6 }));
            assert!(!shared.capture_shortcut);
        }
        assert_eq!(key_name(0x34), "4");
        assert_eq!(key_name(0x64), "Keypad 4");
    }

    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn windows_activation_key_delivers_target_macro() {
        let mut engine = Engine::default();
        engine
            .configure_shortcuts(
                None,
                vec![(
                    3,
                    Hotkey {
                        modifiers: 0,
                        vk: 0x87,
                    },
                )],
            )
            .unwrap();
        unsafe {
            key(0x87, true).unwrap();
            key(0x87, false).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let events = engine.poll_control_events();
            if !events.is_empty() {
                assert!(matches!(events.as_slice(), [ControlEvent::Play(3)]));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Windows did not deliver activation key"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn timing_units_and_active_scope() {
        for input in ["2.5s", "2.5 sec", "2.5 seconds"] {
            assert_eq!(
                parse_timing(input, TimingScope::Milliseconds).unwrap(),
                2500
            );
        }
        for input in ["250ms", "250 milliseconds"] {
            assert_eq!(parse_timing(input, TimingScope::Seconds).unwrap(), 250);
        }
        assert_eq!(parse_timing("2.50", TimingScope::Seconds).unwrap(), 2500);
        assert_eq!(parse_timing("250", TimingScope::Milliseconds).unwrap(), 250);
        assert_eq!(parse_timing("0", TimingScope::Seconds).unwrap(), 0);
        for input in ["-1s", "NaN", "inf", "2 minutes", ""] {
            assert!(
                parse_timing(input, TimingScope::Seconds).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn authored_and_recorded_model_round_trip() {
        let model = Macro {
            activation_key: Some(Hotkey {
                modifiers: 2,
                vk: 0x74,
            }),
            name: "Round trip".into(),
            steps: vec![
                Step {
                    delay_ms: 10,
                    action: Action::Key { vk: 65, down: true },
                },
                Step {
                    delay_ms: 20,
                    action: Action::Key {
                        vk: 65,
                        down: false,
                    },
                },
                Step {
                    delay_ms: 0,
                    action: Action::Move { x: -120, y: 42 },
                },
                Step {
                    delay_ms: 1,
                    action: Action::Button {
                        button: MouseButton::Left,
                        down: true,
                    },
                },
                Step {
                    delay_ms: 2,
                    action: Action::Button {
                        button: MouseButton::Left,
                        down: false,
                    },
                },
                Step {
                    delay_ms: 0,
                    action: Action::Click {
                        button: MouseButton::Right,
                        count: 2,
                    },
                },
                Step {
                    delay_ms: 0,
                    action: Action::Scroll {
                        delta: -120,
                        horizontal: true,
                    },
                },
                Step {
                    delay_ms: 0,
                    action: Action::Path {
                        points: vec![Point { x: 0, y: 0 }, Point { x: 100, y: 200 }],
                        duration_ms: 2500,
                        clicks: vec![PathClick {
                            point_index: 1,
                            button: MouseButton::Middle,
                            count: 1,
                        }],
                    },
                },
            ],
        };
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(serde_json::from_str::<Macro>(&json).unwrap(), model);
    }

    #[test]
    fn empty_play_does_not_start_input_worker() {
        let mut engine = Engine::default();
        assert!(engine.play(Vec::new(), RepeatOptions::default()).is_err());
        assert!(engine.worker.is_none());
        assert!(!engine.status().playing);
        assert!(!engine.status().recording);
        assert!(!engine.shortcuts_configured());
        assert!(engine.poll_recorded().is_empty());
    }
}
