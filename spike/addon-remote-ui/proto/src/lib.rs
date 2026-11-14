//! Wire protocol of the #78 spike: host <-> add-on, postcard frames with a u32 length prefix.
use std::io::{Read, Write};

use egui::epaint::Vertex;
use serde::{Deserialize, Serialize};

/// Host -> add-on.
#[derive(Serialize, Deserialize, Debug)]
pub enum ToAddon {
    /// Run one frame. `events` are already in add-on local coordinates (origin = view corner).
    Frame(FrameRequest),
    Quit,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct FrameRequest {
    pub seq: u64,
    pub size: [f32; 2],
    pub ppp: f32,
    pub time: f64,
    pub modifiers: egui::Modifiers,
    pub events: Vec<egui::Event>,
    pub focused: bool,
}

/// One tessellated, clipped mesh.
#[derive(Serialize, Deserialize, Debug)]
pub struct WireMesh {
    pub clip: egui::Rect,
    pub texture: egui::TextureId,
    pub indices: Vec<u32>,
    pub vertices: Vec<Vertex>,
}

/// Add-on -> host.
#[derive(Serialize, Deserialize)]
pub struct FrameOut {
    /// Highest request `seq` this frame answers.
    pub seq: u64,
    pub meshes: Vec<WireMesh>,
    pub textures_set: Vec<(egui::TextureId, egui::epaint::ImageDelta)>,
    pub textures_free: Vec<egui::TextureId>,
    pub cursor: egui::CursorIcon,
    pub copied_text: String,
    pub open_urls: Vec<String>,
    /// Ask for another frame after this many milliseconds (0 = immediately), if any.
    pub repaint_after_ms: Option<u64>,
    /// Add-on side time to run + tessellate, microseconds.
    pub addon_us: u64,
    /// Bytes the same frame would take as untessellated `Shape`s (None when not serialisable).
    pub shapes_bytes: Option<u32>,
}

pub fn write_msg<T: Serialize>(w: &mut impl Write, msg: &T) -> std::io::Result<usize> {
    let body = postcard::to_stdvec(msg).map_err(std::io::Error::other)?;
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(body.len() + 4)
}

pub fn read_msg<T: for<'de> Deserialize<'de>>(r: &mut impl Read) -> std::io::Result<(T, usize)> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    let msg = postcard::from_bytes(&buf).map_err(std::io::Error::other)?;
    Ok((msg, n + 4))
}
