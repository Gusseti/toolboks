//! Host view of an out-of-process add-on: paints the frames the add-on sends inside the page
//! rectangle and forwards input back, relative to that rectangle.
//!
//! The view does not start or watch the add-on process; the supervisor (#80) does that and
//! hands the view an [`AddonLink`] after the handshake. The view only ever sends input, the
//! page size, the display scale, the theme and focus; the add-on keeps its own clock.
//!
//! Pacing follows `toolboks_addon_proto::pacing`: one input batch in flight, at most 60
//! batches a second, add-on-initiated repaints rate limited. An idle add-on sends nothing, so
//! it costs the host no repaint: the host repaints only on input, on an arriving frame, or at
//! the time the add-on asked for.
//!
//! To try it in a real window, run `cargo run --example addon_view_demo` (debug only; see
//! "Host view" in `docs/ADDON-PROTOCOL.md`).

mod input;
#[cfg(test)]
mod tests;
mod textures;

use eframe::egui::{self, epaint::Mesh, Align2, FontId, Pos2, Rect, Shape};
use input::InputRouter;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use textures::TextureMap;
use toolboks_addon_proto::mesh::{ClippedMesh, MeshCache};
use toolboks_addon_proto::pacing::{FramePacer, RepaintLimiter, STALL_TIMEOUT};
use toolboks_addon_proto::{
    AddonMessage, Frame, FrameReader, FrameWriter, HostMessage, ImeOutput, InputBatch, InputEvent,
    ProtocolError, TabDirection, Theme,
};

/// The sending half of a connection to an add-on. The supervisor may wrap a writer thread
/// behind it; a plain [`FrameWriter`] works for pipes that never fill up.
pub trait AddonSender: Send {
    fn send(&mut self, message: &HostMessage) -> Result<(), ProtocolError>;
}

impl<W: Write + Send> AddonSender for FrameWriter<W> {
    fn send(&mut self, message: &HostMessage) -> Result<(), ProtocolError> {
        self.write(message)
    }
}

/// A sender shared by the view (input) and the supervisor (ping, suspend, quit).
pub type SharedSender = Arc<Mutex<Box<dyn AddonSender>>>;

/// Messages the view consumes: frames, repaint requests and the end of the connection.
pub type Inbox = mpsc::Receiver<Result<AddonMessage, ProtocolError>>;

/// One connection to a running add-on, after the handshake.
pub struct AddonLink {
    sender: SharedSender,
    inbox: Inbox,
}

impl AddonLink {
    pub fn new(sender: SharedSender, inbox: Inbox) -> Self {
        Self { sender, inbox }
    }

    /// Starts the reader thread. Frames and repaint requests go to the view; every other
    /// message (pong, log, preferences, host requests, status items, tutorial, bye) goes to
    /// `side` for the supervisor. Pongs and logs never wake the host's UI.
    pub fn spawn<R, W>(
        mut reader: FrameReader<R>,
        writer: FrameWriter<W>,
        ctx: &egui::Context,
        side: mpsc::Sender<AddonMessage>,
    ) -> std::io::Result<Self>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let (tx, inbox) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("toolboks-addon-view-reader".into())
            .spawn(move || loop {
                let message = reader.read::<AddonMessage>();
                match message {
                    Ok(message) if is_side_message(&message) => {
                        let quiet = matches!(
                            message,
                            AddonMessage::Pong { .. } | AddonMessage::Log { .. }
                        );
                        let _ = side.send(message);
                        if !quiet {
                            ctx.request_repaint();
                        }
                    }
                    message => {
                        wake_for(&ctx, &message);
                        let end = message.is_err();
                        if tx.send(message).is_err() || end {
                            break;
                        }
                    }
                }
            })?;
        let sender: Box<dyn AddonSender> = Box::new(writer);
        Ok(Self::new(Arc::new(Mutex::new(sender)), inbox))
    }

    /// The shared sender, for the supervisor's own messages.
    pub fn sender(&self) -> SharedSender {
        self.sender.clone()
    }

    fn send(&self, message: &HostMessage) -> Result<(), ProtocolError> {
        self.sender
            .lock()
            .map_err(|_| ProtocolError::Closed)?
            .send(message)
    }
}

fn is_side_message(message: &AddonMessage) -> bool {
    !matches!(
        message,
        AddonMessage::Frame(_) | AddonMessage::RequestRepaint { .. }
    )
}

/// Wakes the host for a message the view consumes. A repaint request wakes it only when the
/// add-on wants the repaint, never sooner.
fn wake_for(ctx: &egui::Context, message: &Result<AddonMessage, ProtocolError>) {
    match message {
        Ok(AddonMessage::RequestRepaint { after_ms }) => {
            ctx.request_repaint_after(Duration::from_millis(u64::from(*after_ms)))
        }
        _ => ctx.request_repaint(),
    }
}

/// Only these schemes open from an add-on: the web and mail.
pub fn allowed_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest
            .strip_prefix("//")
            .is_some_and(|host| !host.is_empty() && !host.starts_with('/')),
        "mailto" => !rest.is_empty(),
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewState {
    /// Waiting for the first frame of the add-on.
    Connecting,
    /// Frames arrive.
    Running,
    /// The add-on stopped answering (heartbeat or a batch without a frame for too long).
    Stalled,
    /// The supervisor restarts the add-on; the last frame stays, dimmed.
    Restarting,
    /// The connection ended and nobody restarts it.
    Stopped(String),
}

/// What the host last told the add-on besides events; a change triggers a batch.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Params {
    size: [f32; 2],
    pixels_per_point: f32,
    theme: Theme,
    window_focused: bool,
}

/// One mesh ready to paint: translated to the page origin, with host texture ids.
struct Painted {
    /// Keeps the cached mesh alive, so its address identifies it across frames.
    source: Arc<ClippedMesh>,
    clip: Rect,
    mesh: Arc<Mesh>,
}

pub struct AddonView {
    name: String,
    id: egui::Id,
    link: Option<AddonLink>,
    state: ViewState,
    pacer: FramePacer,
    limiter: RepaintLimiter,
    repaint_due: Option<Instant>,
    meshes: MeshCache,
    /// The meshes of the last frame, not yet translated, when a frame arrived since paint.
    fresh: Option<Vec<Arc<ClippedMesh>>>,
    painted: Vec<Painted>,
    origin: Pos2,
    has_frame: bool,
    textures: TextureMap,
    router: InputRouter,
    sent: Option<Params>,
    focused: bool,
    escape: Option<TabDirection>,
    cursor: egui::CursorIcon,
    ime: Option<ImeOutput>,
}

impl AddonView {
    pub fn new(name: impl Into<String>, ctx: &egui::Context) -> Self {
        let name = name.into();
        Self {
            id: egui::Id::unique(("addon_view", name.clone())),
            name,
            link: None,
            state: ViewState::Connecting,
            pacer: FramePacer::default(),
            limiter: RepaintLimiter::default(),
            repaint_due: None,
            meshes: MeshCache::default(),
            fresh: None,
            painted: Vec::new(),
            origin: Pos2::ZERO,
            has_frame: false,
            textures: TextureMap::new(ctx),
            router: InputRouter::default(),
            sent: None,
            focused: false,
            escape: None,
            cursor: egui::CursorIcon::Default,
            ime: None,
        }
    }

    pub fn state(&self) -> &ViewState {
        &self.state
    }

    /// Connects a (new) add-on process. During a restart the last frame stays dimmed until the
    /// new process paints its first frame.
    pub fn attach(&mut self, link: AddonLink) {
        self.textures.retire();
        self.meshes = MeshCache::default();
        self.pacer = FramePacer::default();
        self.repaint_due = None;
        self.sent = None;
        self.escape = None;
        if self.focused {
            self.pacer.push([InputEvent::FocusGained { via_tab: None }]);
        }
        self.link = Some(link);
        if self.state != ViewState::Restarting {
            self.state = ViewState::Connecting;
        }
    }

    /// The supervisor restarts the add-on. The connection is dropped; the view keeps the last
    /// frame and its textures.
    pub fn begin_restart(&mut self) {
        self.link = None;
        self.state = ViewState::Restarting;
    }

    /// The connection ended for good.
    pub fn stop(&mut self, reason: impl Into<String>) {
        self.link = None;
        self.state = ViewState::Stopped(reason.into());
    }

    /// The supervisor's heartbeat says the add-on is (or is no longer) stuck.
    pub fn set_stalled(&mut self, stalled: bool) {
        match (&self.state, stalled) {
            (ViewState::Running | ViewState::Connecting, true) => self.state = ViewState::Stalled,
            (ViewState::Stalled, false) => {
                self.state = if self.has_frame {
                    ViewState::Running
                } else {
                    ViewState::Connecting
                }
            }
            _ => {}
        }
    }

    /// The note painted over the page, if any.
    pub fn note(&self) -> Option<String> {
        let name = &self.name;
        match &self.state {
            ViewState::Connecting if !self.has_frame => Some(format!("Starting {name}…")),
            ViewState::Connecting | ViewState::Running => None,
            ViewState::Stalled => Some(format!("{name} is not responding. Waiting for it…")),
            ViewState::Restarting => Some(format!("{name} is restarting…")),
            ViewState::Stopped(reason) => Some(format!("{name} stopped. {reason}")),
        }
    }

    /// The last frame is shown dimmed under a note.
    pub fn dimmed(&self) -> bool {
        self.has_frame && self.note().is_some()
    }

    /// Paints the add-on in `rect` and forwards this pass's input.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect) -> egui::Response {
        self.show_at(ui, rect, Instant::now())
    }

    fn show_at(&mut self, ui: &mut egui::Ui, rect: Rect, now: Instant) -> egui::Response {
        let ctx = ui.ctx().clone();
        self.drain(&ctx, now);
        // Tab left the add-on's last widget: egui moves focus while widgets register, so the
        // move must be set before this view's own `interact`.
        let mut escaped = false;
        if let Some(direction) = self.escape.take() {
            escaped = ui.memory(|memory| memory.has_focus(self.id));
            if escaped {
                ui.memory_mut(|memory| {
                    memory.move_focus(match direction {
                        TabDirection::Forward => egui::FocusDirection::Next,
                        TabDirection::Backward => egui::FocusDirection::Previous,
                    })
                });
            }
        }
        let response = ui.interact(rect, self.id, egui::Sense::click());
        let mut events = ui.input(|input| input.events.clone());
        let (shift, window_focused) = ui.input(|input| (input.modifiers.shift, input.focused));

        // Focus: a click inside takes it, a click outside drops it.
        let presses = InputRouter::presses(&events, rect);
        let mut focused = response.has_focus() && !escaped;
        if presses.pressed_inside {
            if !focused {
                response.request_focus();
            }
            focused = true;
        } else if presses.pressed_outside && focused {
            response.surrender_focus();
            focused = false;
        }
        let mut routed = Vec::new();
        if focused != self.focused {
            routed.push(if focused {
                let via_tab = (!presses.pressed_inside).then_some(if shift {
                    TabDirection::Backward
                } else {
                    TabDirection::Forward
                });
                if via_tab.is_some() {
                    // The Tab that moved focus here must not move focus again inside.
                    events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: egui::Key::Tab,
                                ..
                            }
                        )
                    });
                }
                InputEvent::FocusGained { via_tab }
            } else {
                InputEvent::FocusLost
            });
            self.focused = focused;
        }
        if focused {
            // Tab and arrows belong to the add-on while it has focus.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        self.router.route(&events, rect, focused, &mut routed);

        if self.link.is_some() {
            self.pace(&ctx, rect, routed, window_focused, now);
        }
        self.apply_output(&ctx, rect, focused);
        self.paint(ui, rect);
        response
    }

    /// Queues input and sends a batch when the pacer allows one.
    fn pace(
        &mut self,
        ctx: &egui::Context,
        rect: Rect,
        events: Vec<InputEvent>,
        window_focused: bool,
        now: Instant,
    ) {
        if !events.is_empty() {
            self.pacer.push(events);
        }
        let params = Params {
            size: [rect.width(), rect.height()],
            pixels_per_point: ctx.pixels_per_point(),
            theme: if ctx.global_style().visuals.dark_mode {
                Theme::Dark
            } else {
                Theme::Light
            },
            window_focused,
        };
        if self.sent != Some(params) {
            self.pacer.request();
        }
        if let Some(due) = self.repaint_due {
            if now >= due {
                self.repaint_due = None;
                self.limiter.mark(now);
                self.pacer.request();
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
        if self.pacer.stalled(now, STALL_TIMEOUT) && self.state == ViewState::Running {
            self.state = ViewState::Stalled;
        }
        if let Some((seq, events)) = self.pacer.take(now) {
            let batch = InputBatch {
                seq,
                size: params.size,
                pixels_per_point: params.pixels_per_point,
                theme: params.theme,
                window_focused: params.window_focused,
                events,
            };
            let sent = self
                .link
                .as_ref()
                .map(|link| link.send(&HostMessage::Input(batch)));
            match sent {
                Some(Err(error)) => self.fail(error),
                _ => self.sent = Some(params),
            }
        } else if self.pacer.in_flight().is_none() {
            // Rate capped: come back when the next batch may go.
            if let Some(at) = self.pacer.next_send_at() {
                ctx.request_repaint_after(at.saturating_duration_since(now));
            }
        }
    }

    /// Reads everything that arrived since the last pass.
    fn drain(&mut self, ctx: &egui::Context, now: Instant) {
        let mut frames = Vec::new();
        let mut repaint: Option<u32> = None;
        let mut failure = None;
        if let Some(link) = &self.link {
            loop {
                match link.inbox.try_recv() {
                    Ok(Ok(AddonMessage::Frame(frame))) => frames.push(frame),
                    Ok(Ok(AddonMessage::RequestRepaint { after_ms })) => {
                        repaint = Some(repaint.map_or(after_ms, |old| old.min(after_ms)));
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => {
                        failure = Some(error);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        failure = Some(ProtocolError::Closed);
                        break;
                    }
                }
            }
        }
        for frame in frames {
            if let Err(error) = self.apply_frame(ctx, frame, now) {
                failure = Some(error);
                break;
            }
        }
        if let Some(after_ms) = repaint {
            self.schedule_repaint(now, after_ms);
        }
        if let Some(error) = failure {
            self.fail(error);
        }
    }

    fn fail(&mut self, error: ProtocolError) {
        self.link = None;
        if self.state != ViewState::Restarting {
            self.state = ViewState::Stopped(error.to_string());
        }
    }

    fn schedule_repaint(&mut self, now: Instant, after_ms: u32) {
        let due = self
            .limiter
            .deadline(now, Duration::from_millis(u64::from(after_ms)));
        self.repaint_due = Some(self.repaint_due.map_or(due, |old| old.min(due)));
    }

    fn apply_frame(
        &mut self,
        ctx: &egui::Context,
        frame: Frame,
        now: Instant,
    ) -> Result<(), ProtocolError> {
        self.pacer.ack(frame.seq);
        self.textures.apply(frame.textures_set, frame.textures_free);
        let meshes = self.meshes.apply(frame.meshes)?;
        // The new process has painted: the textures of the old one can go.
        self.textures.drop_stale();
        self.fresh = Some(meshes);
        self.has_frame = true;
        self.state = ViewState::Running;
        self.cursor = frame.cursor;
        self.ime = frame.ime;
        if frame.focus_escape.is_some() {
            self.escape = frame.focus_escape;
        }
        if let Some(text) = frame.copied_text {
            ctx.copy_text(text);
        }
        for url in frame.open_urls {
            if allowed_url(&url) {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
        }
        if let Some(after_ms) = frame.repaint_after_ms {
            self.schedule_repaint(now, after_ms);
        }
        Ok(())
    }

    /// Cursor and IME rectangle, every pass (egui resets both each pass).
    fn apply_output(&self, ctx: &egui::Context, rect: Rect, focused: bool) {
        if self.state != ViewState::Running {
            return;
        }
        if self.router.hovered() || self.router.captured() {
            ctx.set_cursor_icon(self.cursor);
        }
        if let (true, Some(ime)) = (focused, self.ime) {
            let offset = rect.min.to_vec2();
            ctx.output_mut(|output| {
                output.ime = Some(egui::output::IMEOutput {
                    purpose: Default::default(),
                    rect: ime.rect.translate(offset),
                    cursor_rect: ime.cursor_rect.translate(offset),
                    should_interrupt_composition: false,
                })
            });
        }
    }

    /// Translates meshes to the page origin when a frame arrived or the page moved; otherwise
    /// paints the cached translated meshes as they are.
    fn prepare(&mut self, origin: Pos2) {
        if let Some(fresh) = self.fresh.take() {
            let mut reusable: HashMap<*const ClippedMesh, Painted> = if origin == self.origin {
                self.painted
                    .drain(..)
                    .map(|painted| (Arc::as_ptr(&painted.source), painted))
                    .collect()
            } else {
                HashMap::new()
            };
            let offset = origin.to_vec2();
            self.painted = fresh
                .into_iter()
                .filter_map(|source| {
                    if let Some(painted) = reusable.remove(&Arc::as_ptr(&source)) {
                        return Some(painted);
                    }
                    let texture_id = self.textures.host_id(source.mesh.texture_id)?;
                    let mut mesh = source.mesh.clone();
                    mesh.texture_id = texture_id;
                    mesh.translate(offset);
                    Some(Painted {
                        clip: source.clip.translate(offset),
                        mesh: Arc::new(mesh),
                        source,
                    })
                })
                .collect();
        } else if origin != self.origin {
            // The page moved: shift what is there (also works for the last frame of a
            // restarting add-on, whose texture ids are retired).
            let delta = origin - self.origin;
            for painted in &mut self.painted {
                let mut mesh = (*painted.mesh).clone();
                mesh.translate(delta);
                painted.mesh = Arc::new(mesh);
                painted.clip = painted.clip.translate(delta);
            }
        }
        self.origin = origin;
    }

    fn paint(&mut self, ui: &egui::Ui, rect: Rect) {
        self.prepare(rect.min);
        let painter = ui.painter_at(rect);
        for painted in &self.painted {
            painter
                .with_clip_rect(painted.clip)
                .add(Shape::mesh(painted.mesh.clone()));
        }
        if let Some(note) = self.note() {
            let visuals = ui.visuals();
            if self.has_frame {
                painter.rect_filled(rect, 0.0, visuals.panel_fill.gamma_multiply(0.75));
            }
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                note,
                FontId::proportional(14.0),
                visuals.text_color(),
            );
        }
    }

    #[cfg(test)]
    fn texture_count(&self) -> usize {
        self.textures.len()
    }

    #[cfg(test)]
    fn painted_len(&self) -> usize {
        self.painted.len()
    }

    #[cfg(test)]
    fn focused(&self) -> bool {
        self.focused
    }
}
