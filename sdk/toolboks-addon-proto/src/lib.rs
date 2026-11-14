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

/// The wire protocol version. Bump it on any change to a message other than [`Hello`] and
/// the refusal variant, which are frozen so that two versions can always refuse each other
/// in plain words.
pub const PROTOCOL_VERSION: u32 = 1;

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
    RestartElevated { page: String },
    ReviewUpdate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusItem {
    pub text: String,
    pub tooltip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TutorialStep {
    pub page: String,
    pub target: String,
    pub title: String,
    pub description: String,
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
