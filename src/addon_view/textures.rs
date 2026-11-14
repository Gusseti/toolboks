//! The add-on's textures in the host. The add-on numbers its textures in its own egui
//! context; this map allocates a host texture for each one and frees every host texture it
//! owns when it is dropped.
//!
//! Texture changes come from another process, so every change is checked before the host
//! applies any of them: a bad size would otherwise panic in the renderer.

use eframe::egui::{self, TextureId};
use std::collections::HashMap;
use toolboks_addon_proto::{ProtocolError, TextureUpdate};

/// Largest width or height an add-on texture may have, in pixels.
pub(super) const MAX_TEXTURE_SIDE: usize = 8192;

pub(super) struct TextureMap {
    ctx: egui::Context,
    /// Add-on texture id to host texture id and its size in pixels.
    map: HashMap<TextureId, (TextureId, [usize; 2])>,
    /// Host textures of an add-on process that has gone; freed once the new process has
    /// painted, so the last frame stays visible (dimmed) during a restart.
    stale: Vec<TextureId>,
}

impl TextureMap {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            ctx: ctx.clone(),
            map: HashMap::new(),
            stale: Vec::new(),
        }
    }

    /// Host textures owned by this map, current and stale.
    pub fn len(&self) -> usize {
        self.map.len() + self.stale.len()
    }

    pub fn host_id(&self, remote: TextureId) -> Option<TextureId> {
        self.map.get(&remote).map(|(host, _)| *host)
    }

    /// Applies the texture changes of one frame, or none of them when one is invalid. A partial
    /// update of a texture the host never saw is ignored.
    pub fn apply(
        &mut self,
        set: Vec<TextureUpdate>,
        free: Vec<TextureId>,
    ) -> Result<(), ProtocolError> {
        if set.is_empty() && free.is_empty() {
            return Ok(());
        }
        self.validate(&set)?;
        let manager = self.ctx.tex_manager();
        let mut manager = manager.write();
        for update in set {
            let size = update.delta.image.size();
            match (update.delta.pos, self.map.get_mut(&update.id)) {
                (None, Some((host, known))) => {
                    *known = size;
                    manager.set(*host, update.delta);
                }
                (Some(_), Some((host, _))) => manager.set(*host, update.delta),
                (None, None) => {
                    let host = manager.alloc(
                        format!("add-on texture {:?}", update.id),
                        update.delta.image,
                        update.delta.options,
                    );
                    self.map.insert(update.id, (host, size));
                }
                (Some(_), None) => {}
            }
        }
        for remote in free {
            if let Some((host, _)) = self.map.remove(&remote) {
                manager.free(host);
            }
        }
        Ok(())
    }

    /// Checks every change in order, so a full set followed by a partial update of the same
    /// texture in one frame is judged against the new size.
    fn validate(&self, set: &[TextureUpdate]) -> Result<(), ProtocolError> {
        let mut sizes: HashMap<TextureId, [usize; 2]> = HashMap::new();
        for update in set {
            let bad =
                |why: String| ProtocolError::Malformed(format!("texture {:?} {why}", update.id));
            let egui::ImageData::Color(image) = &update.delta.image;
            let [width, height] = image.size;
            if width == 0 || height == 0 {
                return Err(bad(format!("has no pixels ({width}x{height})")));
            }
            if width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
                return Err(bad(format!(
                    "is {width}x{height}, over the {MAX_TEXTURE_SIDE} pixel limit"
                )));
            }
            if image.pixels.len() != width * height {
                return Err(bad(format!(
                    "says {width}x{height} but carries {} pixels",
                    image.pixels.len()
                )));
            }
            let known = sizes
                .get(&update.id)
                .copied()
                .or_else(|| self.map.get(&update.id).map(|(_, size)| *size));
            match (update.delta.pos, known) {
                (None, _) => {
                    sizes.insert(update.id, image.size);
                }
                (Some([x, y]), Some([full_width, full_height])) => {
                    let fits = x.checked_add(width).is_some_and(|end| end <= full_width)
                        && y.checked_add(height).is_some_and(|end| end <= full_height);
                    if !fits {
                        return Err(bad(format!(
                            "update {width}x{height} at {x},{y} is outside the {full_width}x{full_height} texture"
                        )));
                    }
                }
                (Some(_), None) => {}
            }
        }
        Ok(())
    }

    /// The add-on process went away: its texture ids mean nothing to the next process, but
    /// the host textures stay until [`Self::drop_stale`] so the last frame can still paint.
    pub fn retire(&mut self) {
        self.stale
            .extend(self.map.drain().map(|(_, (host, _))| host));
    }

    pub fn drop_stale(&mut self) {
        if self.stale.is_empty() {
            return;
        }
        let manager = self.ctx.tex_manager();
        let mut manager = manager.write();
        for host in self.stale.drain(..) {
            manager.free(host);
        }
    }

    /// Frees every host texture this map owns.
    pub fn free_all(&mut self) {
        self.retire();
        self.drop_stale();
    }
}

impl Drop for TextureMap {
    fn drop(&mut self) {
        self.free_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, ColorImage, ImageData, TextureOptions};
    use eframe::epaint::ImageDelta;
    use std::sync::Arc;

    fn image(size: [usize; 2], pixels: usize) -> ImageData {
        ImageData::Color(Arc::new(ColorImage {
            size,
            source_size: egui::vec2(size[0] as f32, size[1] as f32),
            pixels: vec![Color32::WHITE; pixels],
        }))
    }

    fn full(id: u64, size: [usize; 2], pixels: usize) -> TextureUpdate {
        TextureUpdate {
            id: TextureId::Managed(id),
            delta: ImageDelta::full(image(size, pixels), TextureOptions::LINEAR),
        }
    }

    fn partial(id: u64, pos: [usize; 2], size: [usize; 2]) -> TextureUpdate {
        TextureUpdate {
            id: TextureId::Managed(id),
            delta: ImageDelta::partial(pos, image(size, size[0] * size[1]), TextureOptions::LINEAR),
        }
    }

    fn allocated(ctx: &egui::Context) -> usize {
        ctx.tex_manager().read().num_allocated()
    }

    /// Texture 1 is a valid 4x4 texture; `set` must be refused with `needle` in the error and
    /// leave the host's textures as they were.
    fn refused(set: Vec<TextureUpdate>, needle: &str) {
        let ctx = egui::Context::default();
        let before = allocated(&ctx);
        let mut map = TextureMap::new(&ctx);
        map.apply(vec![full(1, [4, 4], 16)], Vec::new())
            .expect("valid texture");
        let text = map
            .apply(set, Vec::new())
            .expect_err("invalid texture")
            .to_string();
        assert!(text.contains(needle), "{text}");
        assert_eq!(map.len(), 1, "nothing of the bad frame was applied");
        assert_eq!(allocated(&ctx), before + 1);
    }

    #[test]
    fn pixel_count_must_match_the_size() {
        refused(vec![full(2, [4, 4], 15)], "says 4x4 but carries 15 pixels");
        // A full replacement of a known texture is checked too.
        refused(vec![full(1, [4, 4], 17)], "says 4x4 but carries 17 pixels");
    }

    #[test]
    fn empty_textures_are_refused() {
        refused(vec![full(2, [0, 4], 0)], "has no pixels (0x4)");
    }

    #[test]
    fn oversized_textures_are_refused() {
        let side = MAX_TEXTURE_SIDE + 1;
        refused(vec![full(2, [side, 1], side)], "over the 8192 pixel limit");
    }

    #[test]
    fn partial_updates_must_fit_the_texture() {
        refused(vec![partial(1, [2, 2], [3, 1])], "outside the 4x4 texture");
        refused(
            vec![partial(1, [usize::MAX, 0], [1, 1])],
            "outside the 4x4 texture",
        );
    }

    #[test]
    fn one_bad_change_rejects_the_whole_frame() {
        refused(
            vec![full(2, [2, 2], 4), full(3, [2, 2], 3)],
            "texture Managed(3)",
        );
    }

    #[test]
    fn valid_changes_apply_and_unknown_ids_are_ignored() {
        let ctx = egui::Context::default();
        let before = allocated(&ctx);
        let mut map = TextureMap::new(&ctx);
        // Texture 1 grows; the partial update in the same frame fits the new size. Texture 9
        // was never set, so its partial update and the free of 42 are ignored.
        let set = vec![
            full(1, [2, 2], 4),
            full(1, [8, 8], 64),
            partial(1, [4, 4], [4, 4]),
            partial(9, [0, 0], [1, 1]),
        ];
        map.apply(set, vec![TextureId::Managed(42)])
            .expect("valid frame");
        assert_eq!(map.len(), 1);
        assert!(map.host_id(TextureId::Managed(9)).is_none());
        assert_eq!(allocated(&ctx), before + 1);
        drop(map);
        assert_eq!(allocated(&ctx), before);
    }
}
