//! Wire protocol between the Toolboks host and an out-of-process add-on.
//!
//! The add-on runs egui headless and sends tessellated meshes; the host forwards input and
//! paints the meshes inside the add-on's page rectangle. See `docs/ADDON-PROTOCOL.md` for the
//! full description (version rules, message list, frame pacing and what may block).
//!
//! Layout:
//! - [`codec`]: length-prefixed postcard framing over any `Read` and `Write` pair.
//! - [`memory`]: an in-memory pipe pair for tests and in-process harnesses.
//! - [`mesh`]: per-primitive mesh cache, encoder (add-on side) and decoder (host side).
//! - [`pacing`]: one-frame-in-flight gate, 60 fps cap, repaint limiter and heartbeat monitor.
//! - [`handshake`]: the `Hello` exchange with the version check on both sides.

pub mod codec;
pub mod handshake;
pub mod memory;
pub mod mesh;
pub mod pacing;

pub use codec::{FrameReader, FrameWriter, Transport, MAX_FRAME_BYTES};
pub use handshake::{addon_handshake, host_handshake};

use serde::{Deserialize, Serialize};

pub use egui;

/// The newest wire protocol version this crate speaks. Bump it on any change to a message
/// other than [`Hello`] and the refusal variant, which are frozen so that two versions can
/// always refuse each other in plain words. New messages are appended at the end of the
/// enums, so every older message keeps its bytes (see `tests/golden.rs`).
///
/// Protocol 4 adds [`AddonMessage::Findings`] and [`HostMessage::CheckNow`].
pub const PROTOCOL_VERSION: u32 = 4;

/// The oldest wire protocol version this crate still speaks. A connection runs at the version
/// the host chose in its `Hello` when it lies in `MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION`;
/// messages newer than that version are never sent on it.
pub const MIN_PROTOCOL_VERSION: u32 = 3;

/// The first protocol version with [`AddonMessage::Findings`] and [`HostMessage::CheckNow`].
pub const FINDINGS_PROTOCOL_VERSION: u32 = 4;

/// The first message in both directions. Its layout and its variant index 0 never change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u32,
    /// The add-on id. The host sends the id it expects; the add-on answers with its own id.
    pub addon_id: String,
    /// The sender's software version: the SDK version for an add-on, the Toolboks version
    /// for the host. Informational only; compatibility is decided by `protocol_version`.
    pub software_version: String,
}

impl Hello {
    pub fn new(addon_id: impl Into<String>, software_version: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            addon_id: addon_id.into(),
            software_version: software_version.into(),
        }
    }

    /// A `Hello` for a given protocol version. The host speaks the version recorded for the
    /// installed add-on, which may be older than [`PROTOCOL_VERSION`].
    pub fn with_version(
        addon_id: impl Into<String>,
        software_version: impl Into<String>,
        protocol_version: u32,
    ) -> Self {
        Self {
            protocol_version,
            ..Self::new(addon_id, software_version)
        }
    }
}

/// Messages from the host to the add-on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HostMessage {
    /// Variant 0, frozen.
    Hello(Hello),
    /// Variant 1, frozen. The host refuses the add-on; `reason` is plain text for the user.
    Refuse {
        protocol_version: u32,
        reason: String,
    },
    /// Input and view state for exactly one frame. The add-on answers with one
    /// [`AddonMessage::Frame`] carrying the same `seq`.
    Input(InputBatch),
    Navigate {
        page: String,
    },
    /// The page left the foreground. The add-on keeps only background work.
    Suspend,
    Resume,
    /// Namespaced preferences as a JSON object (postcard cannot carry `serde_json::Value`).
    LoadPreferences {
        json: String,
    },
    /// Ask the add-on to collect its preferences; it answers with [`AddonMessage::Preferences`].
    SavePreferences,
    /// Granted capabilities as their manifest names, for example `"local_preferences"`.
    Capabilities(Vec<String>),
    /// Ask for the tutorial steps and location; answered by [`AddonMessage::Tutorial`].
    TutorialQuery,
    TutorialStart,
    TutorialNavigate {
        page: String,
    },
    TutorialFinish,
    /// Liveness check. The add-on answers from its main loop with the same nonce.
    Ping {
        nonce: u64,
    },
    /// Orderly shutdown. The add-on cancels its work and exits.
    Quit,
    /// The user chose one of the add-on's tray entries (see [`AddonMessage::TrayItems`]).
    TrayAction {
        id: String,
    },
    /// The tour step to highlight, or `None` when no step of this add-on is shown. The add-on
    /// reports the rectangle of that anchor with [`AddonMessage::TutorialAnchor`]. `reveal`
    /// scrolls the anchor into view once.
    TutorialTarget {
        target: Option<String>,
        reveal: bool,
    },
    /// Dedicated helper connection only. Never rendered or forwarded as input.
    AdminOperation {
        request: AdminRequest,
        capabilities: Vec<String>,
    },
    /// Informational result delivered to the unelevated UI.
    AdminResult {
        id: u64,
        outcome: AdminOutcome,
    },
    /// Protocol 4. The user asked Home to check again. Optional to honour: the add-on checks
    /// again when it can and sends [`AddonMessage::Findings`]. Never sent on a protocol 3
    /// connection.
    CheckNow,
}

/// Messages from the add-on to the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AddonMessage {
    /// Variant 0, frozen.
    Hello(Hello),
    /// Variant 1, frozen. The add-on refuses the host; `reason` is plain text for the user.
    Refuse {
        protocol_version: u32,
        reason: String,
    },
    /// The answer to one [`HostMessage::Input`]; it also acknowledges that batch.
    Frame(Frame),
    /// The add-on wants a frame (background data or an animation) while no input is pending.
    /// The host rate-limits these with [`pacing::RepaintLimiter`].
    RequestRepaint {
        after_ms: u32,
    },
    /// Preferences as a JSON object. `changed` is true when they differ from the last load.
    Preferences {
        json: String,
        changed: bool,
    },
    HostRequest(HostRequest),
    /// The full set of status bar items; replaces the previous set.
    StatusItems(Vec<StatusItem>),
    Tutorial {
        steps: Vec<TutorialStep>,
        location: String,
    },
    Log {
        level: LogLevel,
        line: String,
    },
    Pong {
        nonce: u64,
    },
    /// The add-on is exiting on its own.
    Bye,
    /// The full set of entries the add-on offers in the host's tray menu; replaces the
    /// previous set. Optional: most add-ons never send it.
    TrayItems(Vec<TrayItem>),
    /// Where the anchor asked for with [`HostMessage::TutorialTarget`] is, in the view's
    /// logical points (origin at the page's top-left corner). `None` while it is not laid
    /// out or not visible. Sent before the frame it belongs to, and only when it changes.
    TutorialAnchor {
        target: String,
        rect: Option<egui::Rect>,
    },
    /// Returned only by a dedicated helper.
    AdminResult {
        id: u64,
        outcome: AdminOutcome,
    },
    /// Protocol 4. The full set of this add-on's health findings; replaces the previous set.
    /// Sent when it changes, also while suspended. Never sent on a protocol 3 connection, and
    /// a host ignores it there.
    Findings(Vec<Finding>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabDirection {
    Forward,
    Backward,
}

/// One frame of input. Positions are in the add-on view's logical points, origin at the
/// top-left corner of the page rectangle. Positions outside `0..size` are valid during a drag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputBatch {
    pub seq: u64,
    /// The page rectangle size in logical points.
    pub size: [f32; 2],
    pub pixels_per_point: f32,
    pub theme: Theme,
    /// True while the host window has OS focus.
    pub window_focused: bool,
    pub events: Vec<InputEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    PointerMoved {
        pos: [f32; 2],
    },
    PointerButton {
        pos: [f32; 2],
        button: egui::PointerButton,
        pressed: bool,
        modifiers: egui::Modifiers,
    },
    /// The pointer left the view and no drag is active.
    PointerLeft,
    /// A drag that started inside the view ended outside it. The host keeps sending
    /// `PointerMoved` outside the view during the drag and ends it with this event.
    DragReleasedOutside {
        pos: [f32; 2],
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
    },
    Scroll {
        delta: [f32; 2],
        unit: egui::MouseWheelUnit,
        phase: egui::TouchPhase,
        source: egui::MouseWheelSource,
        modifiers: egui::Modifiers,
    },
    Zoom(f32),
    Key {
        key: egui::Key,
        physical_key: Option<egui::Key>,
        pressed: bool,
        repeat: bool,
        modifiers: egui::Modifiers,
    },
    Text(String),
    /// Clipboard text the host read on a paste shortcut while the add-on had keyboard focus.
    Paste(String),
    Copy,
    Cut,
    ImePreedit(String),
    ImeCommit(String),
    ImeDeleteSurrounding {
        before_chars: u32,
        after_chars: u32,
    },
    /// The add-on view got keyboard focus. `via_tab` is set when focus arrived by Tab, so the
    /// add-on focuses its first (forward) or last (backward) widget.
    FocusGained {
        via_tab: Option<TabDirection>,
    },
    FocusLost,
}

/// The add-on's answer to one input batch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Echoes [`InputBatch::seq`]; the frame is the acknowledgement of that batch.
    pub seq: u64,
    pub meshes: Vec<mesh::MeshRef>,
    pub textures_set: Vec<TextureUpdate>,
    pub textures_free: Vec<egui::TextureId>,
    /// When the add-on wants the next frame, in milliseconds; `None` means only on input.
    pub repaint_after_ms: Option<u32>,
    pub cursor: egui::CursorIcon,
    pub copied_text: Option<String>,
    pub open_urls: Vec<String>,
    pub ime: Option<ImeOutput>,
    /// True while an add-on widget has keyboard focus: the host routes keys to the add-on.
    pub wants_keyboard: bool,
    /// Tab moved focus past the add-on's first or last widget: the host takes focus back.
    pub focus_escape: Option<TabDirection>,
    pub page_index: u32,
}

/// One texture upload, in the order egui produced them. Managed ids are the add-on's own
/// namespace: the host maps them to its own textures.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TextureUpdate {
    pub id: egui::TextureId,
    pub delta: egui::epaint::ImageDelta,
}

impl std::fmt::Debug for TextureUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureUpdate")
            .field("id", &self.id)
            .field("pos", &self.delta.pos)
            .field("size", &self.delta.image.size())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImeOutput {
    pub rect: egui::Rect,
    pub cursor_rect: egui::Rect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostRequest {
    /// Legacy source API. The v3 host refuses whole-interface elevation.
    RestartElevated {
        page: String,
    },
    ReviewUpdate,
    AdminOperation(AdminRequest),
}

/// Untrusted action selector and parameters. This is never evidence of consent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminRequest {
    pub id: u64,
    pub operation: String,
    pub json: String,
}

impl AdminRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.operation.is_empty()
            || self.operation.len() > 80
            || !self
                .operation
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || self.json.len() > 32 * 1024
        {
            return Err("The administrator action is invalid or too large.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminOutcome {
    Completed { json: String },
    Cancelled,
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusItem {
    pub text: String,
    pub tooltip: Option<String>,
}

/// One entry an add-on adds to the host's tray menu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayItem {
    /// The add-on's own name for the entry; comes back in [`HostMessage::TrayAction`].
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TutorialStep {
    pub page: String,
    pub target: String,
    pub title: String,
    pub description: String,
}

/// The most findings the host keeps from one [`AddonMessage::Findings`]; the rest are dropped.
pub const MAX_FINDINGS: usize = 16;
/// [`Finding::id`]: 1 to this many characters of `a-z`, `0-9`, `.` and `-`.
pub const MAX_FINDING_ID_CHARS: usize = 48;
/// [`Finding::title`]: one line of at most this many characters.
pub const MAX_FINDING_TITLE_CHARS: usize = 80;
/// [`Finding::observation`] and [`FindingAction::Instruction`]: at most this many characters.
pub const MAX_FINDING_TEXT_CHARS: usize = 240;
/// The label of [`FindingAction::OpenPage`]: at most this many characters.
pub const MAX_FINDING_LABEL_CHARS: usize = 40;
/// [`Finding::stale_after_secs`] is clamped to `MIN_STALE_AFTER_SECS..=MAX_STALE_AFTER_SECS`:
/// one minute to seven days.
pub const MIN_STALE_AFTER_SECS: u32 = 60;
pub const MAX_STALE_AFTER_SECS: u32 = 604_800;

/// One check one add-on made, for Health on Home (protocol 4). Plain text only: the host
/// draws it as plain text (no markdown, no links) under the sending add-on's own name, and
/// cuts every field to the limits above. The host keeps findings in memory only; they are
/// never written to disk, put in a report or sent anywhere.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The add-on's own name for the check, stable across runs: `"memory-pressure"`,
    /// `"drive-health-c"`. 1 to [`MAX_FINDING_ID_CHARS`] characters of `a-z`, `0-9`, `.` and
    /// `-`, unique within the add-on (see [`Finding::valid_id`]).
    pub id: String,
    /// Plain words, one line, at most [`MAX_FINDING_TITLE_CHARS`]: "Memory is under pressure".
    pub title: String,
    /// What was observed, at most [`MAX_FINDING_TEXT_CHARS`]: "Available memory stayed under
    /// 1.1 GB for 6 minutes and Windows read 900 pages a second from disk."
    pub observation: String,
    pub status: FindingStatus,
    /// Used for ranking only when `status` is [`FindingStatus::Attention`]; ignored otherwise.
    pub severity: Severity,
    /// When the add-on measured it, Unix seconds. A time later than when the host received it
    /// is taken as the receive time.
    pub measured_at: u64,
    /// After this many seconds the reading is out of date. Clamped to
    /// [`MIN_STALE_AFTER_SECS`]..=[`MAX_STALE_AFTER_SECS`].
    pub stale_after_secs: u32,
    /// At most one action. None of them changes the system from Home.
    pub action: Option<FindingAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingStatus {
    /// What was measured is fine. Say what was measured ("No crashes recorded in the last 7
    /// days"), never "healthy PC".
    Ok,
    /// Worth the user's attention; ranked by [`Severity`].
    Attention,
    /// The add-on could check but has not yet (first read still running, page never opened).
    NotChecked,
    /// The add-on cannot check on this PC; the observation says why ("Needs administrator
    /// rights").
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Slows the PC or is worth a look (memory pressure, throttling, XMP off).
    Watch,
    /// Harm to the PC or its data now or soon (a drive "Replace soon", blue screens).
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingAction {
    /// Opens a page of the same add-on. `route` is what `NativeAddon::navigate` receives; its
    /// first segment (before '/') must be one of the add-on's catalog `pages`, or the host
    /// drops the finding. `label` at most [`MAX_FINDING_LABEL_CHARS`]: "Open drive health".
    OpenPage { label: String, route: String },
    /// A short instruction for something only the user can do (BIOS, Windows Settings), at
    /// most [`MAX_FINDING_TEXT_CHARS`].
    Instruction { text: String },
}

impl Finding {
    fn build(
        id: impl Into<String>,
        title: impl Into<String>,
        observation: impl Into<String>,
        status: FindingStatus,
        severity: Severity,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            observation: observation.into(),
            status,
            severity,
            measured_at: unix_now(),
            stale_after_secs: MIN_STALE_AFTER_SECS,
            action: None,
        }
    }

    /// What was measured is fine. Measured now; out of date after a minute unless
    /// [`Self::stale_after`] says otherwise.
    pub fn ok(
        id: impl Into<String>,
        title: impl Into<String>,
        observation: impl Into<String>,
    ) -> Self {
        Self::build(id, title, observation, FindingStatus::Ok, Severity::Watch)
    }

    /// Something needs attention, at `severity`.
    pub fn attention(
        id: impl Into<String>,
        title: impl Into<String>,
        observation: impl Into<String>,
        severity: Severity,
    ) -> Self {
        Self::build(id, title, observation, FindingStatus::Attention, severity)
    }

    /// The add-on cannot check this on this PC; `reason` says why.
    pub fn unavailable(
        id: impl Into<String>,
        title: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::build(
            id,
            title,
            reason,
            FindingStatus::Unavailable,
            Severity::Watch,
        )
    }

    /// The add-on can check this but has not yet.
    pub fn not_checked(
        id: impl Into<String>,
        title: impl Into<String>,
        observation: impl Into<String>,
    ) -> Self {
        Self::build(
            id,
            title,
            observation,
            FindingStatus::NotChecked,
            Severity::Watch,
        )
    }

    /// How long the reading stays current, in seconds.
    pub fn stale_after(mut self, secs: u32) -> Self {
        self.stale_after_secs = secs;
        self
    }

    /// When it was measured, Unix seconds (the builders use the current time).
    pub fn measured_at(mut self, unix_secs: u64) -> Self {
        self.measured_at = unix_secs;
        self
    }

    /// Adds an action that opens `route` in this add-on.
    pub fn open_page(mut self, label: impl Into<String>, route: impl Into<String>) -> Self {
        self.action = Some(FindingAction::OpenPage {
            label: label.into(),
            route: route.into(),
        });
        self
    }

    /// Adds an instruction for something only the user can do.
    pub fn instruction(mut self, text: impl Into<String>) -> Self {
        self.action = Some(FindingAction::Instruction { text: text.into() });
        self
    }

    /// True for an id the host accepts: 1 to [`MAX_FINDING_ID_CHARS`] characters of `a-z`,
    /// `0-9`, `.` and `-`.
    pub fn valid_id(id: &str) -> bool {
        (1..=MAX_FINDING_ID_CHARS).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Every failure on the wire. All are fatal for the connection: after an error the stream
/// position is unknown, so the host ends the add-on and the add-on exits.
#[derive(Debug)]
pub enum ProtocolError {
    /// The two sides speak different protocol versions.
    VersionMismatch {
        addon: u32,
        host: u32,
    },
    /// The other side refused the connection with this plain-text reason.
    Refused(String),
    /// The add-on answered with another id than the host expected.
    WrongAddon {
        expected: String,
        actual: String,
    },
    /// A frame announced more bytes than [`MAX_FRAME_BYTES`].
    FrameTooLarge {
        len: u32,
        max: u32,
    },
    /// The stream ended inside a frame.
    Truncated,
    /// The frame body is not a valid message.
    Malformed(String),
    /// A valid message arrived where the protocol does not allow it.
    Unexpected(&'static str),
    /// A cached mesh reference names an id the receiver does not hold.
    UnknownMesh(u64),
    /// The other side closed the stream at a frame boundary (a clean end).
    Closed,
    /// The host sent more messages than the add-on may hold unhandled.
    Overloaded {
        limit: usize,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionMismatch { addon, host } => write!(
                f,
                "Add-on speaks protocol {addon}, Toolboks speaks {host}. Update the add-on or Toolboks."
            ),
            Self::Refused(reason) => write!(f, "{reason}"),
            Self::WrongAddon { expected, actual } => {
                write!(f, "Expected add-on {expected}, but {actual} answered.")
            }
            Self::FrameTooLarge { len, max } => {
                write!(f, "Message of {len} bytes is over the {max} byte limit.")
            }
            Self::Truncated => write!(f, "The connection ended in the middle of a message."),
            Self::Malformed(detail) => write!(f, "Malformed message: {detail}"),
            Self::Unexpected(what) => write!(f, "Unexpected message: {what}"),
            Self::UnknownMesh(id) => write!(f, "Cached mesh {id:#x} is unknown."),
            Self::Closed => write!(f, "The connection was closed."),
            Self::Overloaded { limit } => write!(
                f,
                "The host sent more than {limit} messages the add-on had not handled yet."
            ),
            Self::Io(error) => write!(f, "Connection error: {error}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<std::io::Error> for ProtocolError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted => Self::Closed,
            _ => Self::Io(error),
        }
    }
}

impl ProtocolError {
    /// True for a clean end of the connection (the other side quit or the pipe was dropped).
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_ordered() {
        assert_eq!(MIN_PROTOCOL_VERSION, 3);
        assert_eq!(PROTOCOL_VERSION, 4);
        assert!((MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&FINDINGS_PROTOCOL_VERSION));
        let hello = Hello::with_version("a.b", "1.0.0", 3);
        assert_eq!(hello.protocol_version, 3);
        assert_eq!(hello.addon_id, "a.b");
        assert_eq!(Hello::new("a.b", "1").protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn finding_builders_fill_status_time_and_action() {
        let before = unix_now();
        let ok = Finding::ok("memory-pressure", "Memory", "No pressure");
        assert_eq!(ok.status, FindingStatus::Ok);
        assert!(ok.measured_at >= before && ok.measured_at <= unix_now());
        assert_eq!(ok.stale_after_secs, MIN_STALE_AFTER_SECS);
        assert_eq!(ok.action, None);
        let attention = Finding::attention("a", "T", "O", Severity::Problem)
            .stale_after(300)
            .open_page("Open", "Home");
        assert_eq!(attention.status, FindingStatus::Attention);
        assert_eq!(attention.severity, Severity::Problem);
        assert_eq!(attention.stale_after_secs, 300);
        assert_eq!(
            attention.action,
            Some(FindingAction::OpenPage {
                label: "Open".into(),
                route: "Home".into()
            })
        );
        let unavailable =
            Finding::unavailable("a", "T", "Needs administrator rights").instruction("Do it");
        assert_eq!(unavailable.status, FindingStatus::Unavailable);
        assert_eq!(unavailable.observation, "Needs administrator rights");
        assert_eq!(
            unavailable.action,
            Some(FindingAction::Instruction {
                text: "Do it".into()
            })
        );
        let not_checked = Finding::not_checked("a", "T", "O").measured_at(5);
        assert_eq!(not_checked.status, FindingStatus::NotChecked);
        assert_eq!(not_checked.measured_at, 5);
    }

    #[test]
    fn finding_ids_follow_the_pattern() {
        for good in [
            "a",
            "memory-pressure",
            "drive-health-c",
            "x.y-2",
            &"a".repeat(48),
        ] {
            assert!(Finding::valid_id(good), "{good}");
        }
        for bad in [
            "",
            &"a".repeat(49),
            "Memory",
            "drive_c",
            "drive c",
            "a/b",
            "\u{e5}",
            "a\n",
        ] {
            assert!(!Finding::valid_id(bad), "{bad:?}");
        }
    }
}
