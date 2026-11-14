//! The add-on's textures in the host. The add-on numbers its textures in its own egui
//! context; this map allocates a host texture for each one and frees every host texture it
//! owns when it is dropped.

use eframe::egui::{self, TextureId};
use std::collections::HashMap;
use toolboks_addon_proto::TextureUpdate;

pub(super) struct TextureMap {
    ctx: egui::Context,
    /// Add-on texture id to host texture id.
    map: HashMap<TextureId, TextureId>,
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
        self.map.get(&remote).copied()
    }

    pub fn apply(&mut self, set: Vec<TextureUpdate>, free: Vec<TextureId>) {
        if set.is_empty() && free.is_empty() {
            return;
        }
        let manager = self.ctx.tex_manager();
        let mut manager = manager.write();
        for update in set {
            match (update.delta.pos, self.map.get(&update.id)) {
                (_, Some(host)) => manager.set(*host, update.delta),
                (None, None) => {
                    let host = manager.alloc(
                        format!("add-on texture {:?}", update.id),
                        update.delta.image,
                        update.delta.options,
                    );
                    self.map.insert(update.id, host);
                }
                // A partial update of a texture the host never saw: nothing to patch.
                (Some(_), None) => {}
            }
        }
        for remote in free {
            if let Some(host) = self.map.remove(&remote) {
                manager.free(host);
            }
        }
    }

    /// The add-on process went away: its texture ids mean nothing to the next process, but
    /// the host textures stay until [`Self::drop_stale`] so the last frame can still paint.
    pub fn retire(&mut self) {
        self.stale.extend(self.map.drain().map(|(_, host)| host));
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
