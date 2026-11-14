//! Out-of-process add-on runtime (feature `runtime`).
//!
//! [`run_addon`] runs a [`NativeAddon`] in a headless egui loop (an `egui::Context`, no
//! window, no eframe) and talks to the Toolboks host over the wire protocol in
//! [`toolboks_addon_proto`] (see `docs/ADDON-PROTOCOL.md`).
//!
//! The loop:
//! - answers the host's `Hello`, refusing a protocol mismatch in plain words;
//! - renders exactly one frame per `Input` batch (messages the batch caused, such as
//!   changed preferences, are sent before the frame that acknowledges it), with the add-on's own clock (the host
//!   sends no time), tessellates it and sends cached meshes plus texture deltas;
//! - asks the host for a frame (`RequestRepaint { after_ms }`) when something calls
//!   `ctx.request_repaint()` or `request_repaint_after` between frames, keeping the
//!   requested deadline and sending again only for an earlier one;
//! - forwards navigation, suspend and resume, preferences, capabilities and the tutorial
//!   calls to the add-on and sends back host requests, status items and log lines;
//! - answers heartbeat pings from the main loop, so a stuck `ui` call shows as stalled;
//! - while suspended with background work, calls `tick_background` every 200 ms;
//! - returns `Ok(())` on `Quit` or when the host closes the connection, and an error for
//!   anything malformed. It never panics on bad input.
//!
//! Connection, a placeholder until #80 defines the real pipe setup:
//! - `--pipe-in <path> --pipe-out <path>` (or the environment variables
//!   `TOOLBOKS_ADDON_PIPE_IN` and `TOOLBOKS_ADDON_PIPE_OUT`) open two paths, for example
//!   the two halves of a named pipe pair; the add-on reads host messages from `pipe-in`
//!   and writes its own messages to `pipe-out`;
//! - with neither, the add-on reads from stdin and writes to stdout, so it must never
//!   print to stdout itself (use [`log`] or stderr).
//!
//! [`run_addon_on`] takes any `Read` and `Write` pair and is what tests use.

use crate::{Capability, HostRequest, NativeAddon, StatusItem};
use proto::mesh::MeshEncoder;
use proto::{
    AddonMessage, Frame, FrameWriter, Hello, HostMessage, ImeOutput, InputBatch, InputEvent,
    LogLevel, ProtocolError, TabDirection, TextureUpdate, Theme, Transport,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub use toolboks_addon_proto as proto;

/// The SDK version an add-on reports in its `Hello`.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");
const BACKGROUND_TICK: Duration = Duration::from_millis(200);
/// Host messages waiting for the main loop. When it is full the reader stops reading, so a
/// fast host is held back by the pipe instead of growing memory.
const WAKE_QUEUE: usize = 64;

static LOG: Mutex<Vec<(LogLevel, String)>> = Mutex::new(Vec::new());

/// Queues a log line for the host. Lines are sent after the current message is handled.
pub fn log(level: LogLevel, line: impl Into<String>) {
    if let Ok(mut queue) = LOG.lock() {
        queue.push((level, line.into()));
    }
}

/// Connects from the command line or the environment (see the module docs) and runs the
/// add-on until the host quits or closes the connection.
pub fn run_addon<A: NativeAddon>(addon: A, addon_id: &str) -> Result<(), ProtocolError> {
    let paths = pipe_paths(
        std::env::args().skip(1),
        std::env::var("TOOLBOKS_ADDON_PIPE_IN").ok(),
        std::env::var("TOOLBOKS_ADDON_PIPE_OUT").ok(),
    );
    match paths {
        Some((input, output)) => {
            let reader = std::fs::File::open(input)?;
            let writer = std::fs::OpenOptions::new().write(true).open(output)?;
            run_addon_on(Transport::new(reader, writer), addon, addon_id)
        }
        None => run_addon_on(
            Transport::new(std::io::stdin(), std::io::stdout()),
            addon,
            addon_id,
        ),
    }
}

fn pipe_paths(
    args: impl IntoIterator<Item = String>,
    env_in: Option<String>,
    env_out: Option<String>,
) -> Option<(String, String)> {
    let (mut input, mut output) = (env_in, env_out);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pipe-in" => input = args.next(),
            "--pipe-out" => output = args.next(),
            _ => {}
        }
    }
    input.zip(output)
}

enum Wake {
    Host(Result<HostMessage, ProtocolError>),
    Repaint,
}

/// Repaint requests from egui's callback, coalesced to the earliest deadline per pass number.
/// The pass number tells requests made during a pass (already in that frame's hint) from
/// requests made between frames.
#[derive(Default)]
struct RepaintRequests {
    pending: Mutex<Vec<(u64, Instant)>>,
}

impl RepaintRequests {
    /// Records a request. Returns true when the main loop needs a wake-up.
    fn add(&self, pass: u64, deadline: Instant) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let wake = pending.is_empty();
        match pending.iter_mut().find(|(seen, _)| *seen == pass) {
            Some((_, earliest)) => *earliest = (*earliest).min(deadline),
            None => pending.push((pass, deadline)),
        }
        wake
    }

    /// Drains every request and returns the earliest deadline among those made at or after
    /// pass `painted` (the first pass not yet rendered).
    fn take_since(&self, painted: u64) -> Option<Instant> {
        let pending = std::mem::take(&mut *self.pending.lock().ok()?);
        pending
            .into_iter()
            .filter(|(pass, _)| *pass >= painted)
            .map(|(_, deadline)| deadline)
            .min()
    }
}

/// Runs the add-on over any transport. The reader moves to its own thread; the add-on and
/// the writer stay on the calling thread.
pub fn run_addon_on<R, W, A>(
    transport: Transport<R, W>,
    mut addon: A,
    addon_id: &str,
) -> Result<(), ProtocolError>
where
    R: Read + Send + 'static,
    W: Write,
    A: NativeAddon,
{
    let (mut reader, mut writer) = transport.split();
    proto::addon_handshake(&mut reader, &mut writer, &Hello::new(addon_id, SDK_VERSION))?;
    let (tx, rx) = mpsc::sync_channel(WAKE_QUEUE);
    let host_tx = tx.clone();
    std::thread::Builder::new()
        .name("toolboks-addon-reader".into())
        .spawn(move || loop {
            let message = reader.read::<HostMessage>();
            let end = message.is_err();
            if host_tx.send(Wake::Host(message)).is_err() || end {
                break;
            }
        })?;
    let ctx = egui::Context::default();
    let requests = Arc::new(RepaintRequests::default());
    let signal = requests.clone();
    ctx.set_request_repaint_callback(move |info| {
        // `request_repaint_after` carries a delay: keep its deadline. Never block here; the
        // callback also runs on the main loop's thread, which is the queue's only reader.
        let Some(deadline) = Instant::now().checked_add(info.delay) else {
            return;
        };
        if signal.add(info.current_cumulative_pass_nr, deadline) {
            let _ = tx.try_send(Wake::Repaint);
        }
    });
    let mut runtime = Runtime {
        ctx,
        writer,
        clock: Instant::now(),
        encoder: MeshEncoder::default(),
        preferences: BTreeMap::new(),
        suspended: false,
        requests,
        repaint_deadline: None,
        theme: None,
        status: Vec::new(),
    };
    let result = runtime.serve(&mut addon, &rx);
    addon.cancel();
    match result {
        Err(error) if error.is_closed() => Ok(()),
        other => other,
    }
}

struct Runtime<W> {
    ctx: egui::Context,
    writer: FrameWriter<W>,
    clock: Instant,
    encoder: MeshEncoder,
    preferences: BTreeMap<String, serde_json::Value>,
    suspended: bool,
    requests: Arc<RepaintRequests>,
    /// The earliest repaint deadline sent to the host since the last input batch.
    repaint_deadline: Option<Instant>,
    theme: Option<Theme>,
    status: Vec<StatusItem>,
}

impl<W: Write> Runtime<W> {
    fn send(&mut self, message: &AddonMessage) -> Result<(), ProtocolError> {
        self.writer.write(message)
    }

    fn serve<A: NativeAddon>(
        &mut self,
        addon: &mut A,
        rx: &mpsc::Receiver<Wake>,
    ) -> Result<(), ProtocolError> {
        loop {
            let wake = if self.suspended && addon.has_background_work() {
                match rx.recv_timeout(BACKGROUND_TICK) {
                    Ok(wake) => wake,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        addon.tick_background();
                        self.flush_side_channels(addon)?;
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
            } else {
                match rx.recv() {
                    Ok(wake) => wake,
                    Err(_) => return Ok(()),
                }
            };
            match wake {
                // Handled below with every wake, so a wake dropped on a full queue is not lost.
                Wake::Repaint => {}
                Wake::Host(message) => {
                    if !self.handle(addon, message?)? {
                        return Ok(());
                    }
                }
            }
            self.flush_side_channels(addon)?;
            self.forward_repaint()?;
        }
    }

    /// Sends a repaint request made between frames, with its remaining delay, unless an
    /// earlier one is already pending at the host.
    fn forward_repaint(&mut self) -> Result<(), ProtocolError> {
        let Some(deadline) = self.requests.take_since(self.ctx.cumulative_pass_nr()) else {
            return Ok(());
        };
        if self.repaint_deadline.is_some_and(|sent| sent <= deadline) {
            return Ok(());
        }
        self.repaint_deadline = Some(deadline);
        let after = deadline.saturating_duration_since(Instant::now());
        let after_ms = u32::try_from(after.as_millis()).unwrap_or(u32::MAX);
        self.send(&AddonMessage::RequestRepaint { after_ms })
    }

    /// Handles one host message. Returns false when the add-on should exit.
    fn handle<A: NativeAddon>(
        &mut self,
        addon: &mut A,
        message: HostMessage,
    ) -> Result<bool, ProtocolError> {
        match message {
            HostMessage::Hello(_) => return Err(ProtocolError::Unexpected("second Hello")),
            HostMessage::Refuse { reason, .. } => return Err(ProtocolError::Refused(reason)),
            HostMessage::Input(batch) => {
                self.repaint_deadline = None;
                self.frame(addon, batch)?;
            }
            HostMessage::Navigate { page } => addon.navigate(&page),
            HostMessage::Suspend => {
                addon.suspend();
                self.suspended = true;
            }
            HostMessage::Resume => {
                addon.resume();
                self.suspended = false;
            }
            HostMessage::LoadPreferences { json } => {
                self.preferences = serde_json::from_str(&json)
                    .map_err(|error| ProtocolError::Malformed(format!("preferences: {error}")))?;
                addon.load_preferences(&self.preferences);
            }
            HostMessage::SavePreferences => {
                let changed = addon.save_preferences(&mut self.preferences);
                self.send_preferences(changed)?;
            }
            HostMessage::Capabilities(names) => {
                // Names this SDK does not know (from a newer host) are ignored.
                let capabilities: Vec<Capability> = names
                    .into_iter()
                    .filter_map(|name| serde_json::from_value(name.into()).ok())
                    .collect();
                addon.set_capabilities(&capabilities);
            }
            HostMessage::TutorialQuery => {
                let steps = addon
                    .tutorial_steps()
                    .into_iter()
                    .map(|step| proto::TutorialStep {
                        page: step.page,
                        target: step.target,
                        title: step.title,
                        description: step.description,
                    })
                    .collect();
                let location = addon.tutorial_location();
                self.send(&AddonMessage::Tutorial { steps, location })?;
            }
            HostMessage::TutorialStart => addon.tutorial_start(),
            HostMessage::TutorialNavigate { page } => addon.tutorial_navigate(&page),
            HostMessage::TutorialFinish => addon.tutorial_finish(),
            HostMessage::Ping { nonce } => self.send(&AddonMessage::Pong { nonce })?,
            HostMessage::Quit => return Ok(false),
        }
        Ok(true)
    }

    fn send_preferences(&mut self, changed: bool) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(&self.preferences)
            .map_err(|error| ProtocolError::Malformed(format!("preferences: {error}")))?;
        self.send(&AddonMessage::Preferences { json, changed })
    }

    /// Host requests, status items and log lines, sent after every handled message.
    fn flush_side_channels<A: NativeAddon>(&mut self, addon: &mut A) -> Result<(), ProtocolError> {
        while let Some(request) = addon.take_host_request() {
            let request = match request {
                HostRequest::RestartElevated { page } => {
                    proto::HostRequest::RestartElevated { page }
                }
                HostRequest::ReviewUpdate => proto::HostRequest::ReviewUpdate,
            };
            self.send(&AddonMessage::HostRequest(request))?;
        }
        let status = addon.status_items();
        if status != self.status {
            let items = status
                .iter()
                .map(|item| proto::StatusItem {
                    text: item.text.clone(),
                    tooltip: item.tooltip.clone(),
                })
                .collect();
            self.status = status;
            self.send(&AddonMessage::StatusItems(items))?;
        }
        let lines = LOG
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for (level, line) in lines {
            self.send(&AddonMessage::Log { level, line })?;
        }
        Ok(())
    }

    fn frame<A: NativeAddon>(
        &mut self,
        addon: &mut A,
        batch: InputBatch,
    ) -> Result<(), ProtocolError> {
        let ctx = self.ctx.clone();
        if self.theme != Some(batch.theme) {
            self.theme = Some(batch.theme);
            ctx.set_theme(match batch.theme {
                Theme::Light => egui::Theme::Light,
                Theme::Dark => egui::Theme::Dark,
            });
        }
        let pixels_per_point = if batch.pixels_per_point.is_finite() {
            batch.pixels_per_point.clamp(0.25, 8.0)
        } else {
            1.0
        };
        let size = egui::vec2(
            sanitize(batch.size[0]).max(1.0),
            sanitize(batch.size[1]).max(1.0),
        );
        let mut events = Vec::with_capacity(batch.events.len());
        let mut tab = None;
        for event in batch.events {
            if let Some(direction) = convert(event, &mut events, &ctx) {
                tab = Some(direction);
            }
        }
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(self.clock.elapsed().as_secs_f64()),
            events,
            focused: batch.window_focused,
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(pixels_per_point);
        if !self.suspended {
            addon.tick(&ctx);
        }
        let mut changed = false;
        let preferences = &mut self.preferences;
        let mut discard_requested = false;
        let mut output = ctx.run_ui(raw, |ui| {
            // Invisible focusable sentinels before and after the page mark the two ends of
            // egui's traversal order. Tab past the last widget lands on the end sentinel in
            // the same pass; Shift+Tab before the first one is applied by egui one pass later,
            // so a Tab frame asks for a second pass to settle focus before it is sent.
            let anchor = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::ZERO);
            let sense = egui::Sense::focusable_noninteractive();
            ui.interact(anchor, focus_id(FOCUS_START), sense);
            egui::CentralPanel::default().show(ui, |ui| {
                changed |= addon.ui(ui, preferences);
            });
            ui.interact(anchor, focus_id(FOCUS_END), sense);
            if tab.is_some() && !discard_requested {
                discard_requested = true;
                ui.ctx().request_discard("focus traversal");
            }
        });
        let focused = ctx.memory(|memory| memory.focused());
        let focus_escape = match focused {
            Some(id) if id == focus_id(FOCUS_END) => Some(TabDirection::Forward),
            Some(id) if id == focus_id(FOCUS_START) => Some(TabDirection::Backward),
            _ => None,
        };
        if let (Some(id), Some(_)) = (focused, focus_escape) {
            // The host takes focus; the sentinel never keeps it.
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let repaint_after_ms = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.repaint_delay)
            .filter(|delay| *delay != Duration::MAX)
            .map(|delay| u32::try_from(delay.as_millis()).unwrap_or(u32::MAX));
        let platform = output.platform_output;
        let mut copied_text = None::<String>;
        let mut open_urls = Vec::new();
        for command in platform.commands {
            match command {
                egui::OutputCommand::CopyText(text) => {
                    copied_text.get_or_insert_with(String::new).push_str(&text)
                }
                egui::OutputCommand::OpenUrl(url) => open_urls.push(url.url),
                _ => {}
            }
        }
        let mut textures_set = Vec::new();
        for (id, deltas) in std::mem::take(&mut output.textures_delta.set) {
            textures_set.extend(deltas.into_iter().map(|delta| TextureUpdate { id, delta }));
        }
        let frame = Frame {
            seq: batch.seq,
            meshes: self.encoder.encode(primitives),
            textures_set,
            textures_free: std::mem::take(&mut output.textures_delta.free)
                .into_iter()
                .collect(),
            repaint_after_ms,
            cursor: platform.cursor_icon,
            copied_text,
            open_urls,
            ime: platform.ime.map(|ime| ImeOutput {
                rect: ime.rect,
                cursor_rect: ime.cursor_rect,
            }),
            wants_keyboard: focus_escape.is_none() && ctx.egui_wants_keyboard_input(),
            focus_escape,
            page_index: u32::try_from(addon.page_index()).unwrap_or(u32::MAX),
        };
        // Everything this batch caused goes out before the frame that acknowledges it.
        if changed {
            self.send_preferences(true)?;
        }
        self.flush_side_channels(addon)?;
        self.send(&AddonMessage::Frame(frame))
    }
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0e6, 1.0e6)
    } else {
        0.0
    }
}

fn pos(value: [f32; 2]) -> egui::Pos2 {
    egui::pos2(sanitize(value[0]), sanitize(value[1]))
}

fn tab_key(direction: TabDirection, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: Some(egui::Key::Tab),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers {
            shift: direction == TabDirection::Backward,
            ..Default::default()
        },
    }
}

/// Maps one wire event to egui events. Returns the direction of a Tab press the user made.
fn convert(
    event: InputEvent,
    out: &mut Vec<egui::Event>,
    ctx: &egui::Context,
) -> Option<TabDirection> {
    match event {
        InputEvent::PointerMoved { pos: p } => out.push(egui::Event::PointerMoved(pos(p))),
        InputEvent::PointerButton {
            pos: p,
            button,
            pressed,
            modifiers,
        } => out.push(egui::Event::PointerButton {
            pos: pos(p),
            button,
            pressed,
            modifiers,
        }),
        InputEvent::PointerLeft => out.push(egui::Event::PointerGone),
        InputEvent::DragReleasedOutside {
            pos: p,
            button,
            modifiers,
        } => {
            out.push(egui::Event::PointerButton {
                pos: pos(p),
                button,
                pressed: false,
                modifiers,
            });
            out.push(egui::Event::PointerGone);
        }
        InputEvent::Scroll {
            delta,
            unit,
            phase,
            source,
            modifiers,
        } => out.push(egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(sanitize(delta[0]), sanitize(delta[1])),
            phase,
            source,
            modifiers,
        }),
        InputEvent::Zoom(factor) if factor.is_finite() && factor > 0.0 => {
            out.push(egui::Event::Zoom(factor))
        }
        InputEvent::Zoom(_) => {}
        InputEvent::Key {
            key,
            physical_key,
            pressed,
            repeat,
            modifiers,
        } => {
            out.push(egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
            });
            if key == egui::Key::Tab && pressed {
                return Some(if modifiers.shift {
                    TabDirection::Backward
                } else {
                    TabDirection::Forward
                });
            }
        }
        InputEvent::Text(text) => out.push(egui::Event::Text(text)),
        InputEvent::Paste(text) => out.push(egui::Event::Paste(text)),
        InputEvent::Copy => out.push(egui::Event::Copy),
        InputEvent::Cut => out.push(egui::Event::Cut),
        InputEvent::ImePreedit(text) => out.push(egui::Event::Ime(egui::ImeEvent::Preedit {
            text,
            active_range_chars: None,
        })),
        InputEvent::ImeCommit(text) => out.push(egui::Event::Ime(egui::ImeEvent::Commit(text))),
        InputEvent::ImeDeleteSurrounding {
            before_chars,
            after_chars,
        } => out.push(egui::Event::Ime(egui::ImeEvent::DeleteSurrounding {
            before_chars: before_chars as usize,
            after_chars: after_chars as usize,
        })),
        InputEvent::FocusGained { via_tab } => {
            // Focus arriving by Tab starts on the sentinel at the other end, so one Tab press
            // lands on the first (or last) real widget in egui's own traversal order.
            if let Some(direction) = via_tab {
                let start = match direction {
                    TabDirection::Forward => FOCUS_START,
                    TabDirection::Backward => FOCUS_END,
                };
                ctx.memory_mut(|memory| memory.request_focus(focus_id(start)));
                out.push(tab_key(direction, true));
                out.push(tab_key(direction, false));
                return Some(direction);
            }
        }
        InputEvent::FocusLost => ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        }),
    }
    None
}

const FOCUS_START: &str = "toolboks-runtime-focus-start";
const FOCUS_END: &str = "toolboks-runtime-focus-end";

fn focus_id(name: &str) -> egui::Id {
    egui::Id::NULL.with(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_paths_from_args_or_env() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pipe_paths(args(&[]), None, None), None);
        assert_eq!(
            pipe_paths(args(&["--pipe-in", "a", "--pipe-out", "b"]), None, None),
            Some(("a".into(), "b".into()))
        );
        assert_eq!(
            pipe_paths(args(&["--pipe-out", "b"]), Some("x".into()), None),
            Some(("x".into(), "b".into()))
        );
        assert_eq!(
            pipe_paths(args(&["--pipe-in"]), None, Some("b".into())),
            None
        );
    }
}
