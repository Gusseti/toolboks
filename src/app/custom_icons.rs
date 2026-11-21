//! Drawing custom add-on icons (see `crate::icons`). The PNG's alpha channel becomes a white
//! texture that is tinted with the colour the named icons use in the same place, so a custom icon
//! is selected, muted or accented exactly like the others.
//!
//! Each file is decoded once per window and kept as a texture until [`forget`] drops it (an
//! install, an update or a removal). A file that is missing or bad is remembered as such, and the
//! default glyph is drawn instead.

use crate::icons;
use eframe::egui::{self, Color32, Rect, TextureHandle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Textures = Arc<Mutex<HashMap<String, Option<TextureHandle>>>>;

fn cache_id() -> egui::Id {
    egui::Id::unique("custom-addon-icons")
}

fn cache(ctx: &egui::Context) -> Textures {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(cache_id(), Textures::default)
            .clone()
    })
}

/// Paints the custom icon that `key` names into `rect` in `color`. False when `key` is not a
/// custom icon or its file cannot be used: the caller then draws its default glyph.
pub(crate) fn paint(painter: &egui::Painter, rect: Rect, key: &str, color: Color32) -> bool {
    if !key.starts_with(icons::KEY_PREFIX) {
        return false;
    }
    let ctx = painter.ctx();
    let textures = cache(ctx);
    let texture = {
        let mut map = textures.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.to_owned())
            .or_insert_with(|| load(ctx, key))
            .clone()
    };
    let Some(texture) = texture else {
        return false;
    };
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture.id(), rect, uv, color);
    true
}

fn load(ctx: &egui::Context, key: &str) -> Option<TextureHandle> {
    let path = icons::key_path(&crate::addons::data_root(), key)?;
    // Not downloaded (yet): the default glyph, without a log line.
    let bytes = std::fs::read(&path).ok()?;
    match icons::alpha_mask(&bytes) {
        Ok((side, alpha)) => Some(ctx.load_texture(
            key,
            mask_image(side, &alpha),
            egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)),
        )),
        Err(e) => {
            eprintln!("Toolboks: icon {} not used: {e}", path.display());
            None
        }
    }
}

/// A white picture with the icon's alpha: tinting it gives the icon in that colour.
fn mask_image(side: u32, alpha: &[u8]) -> egui::ColorImage {
    let side = side as usize;
    egui::ColorImage::new(
        [side, side],
        alpha
            .iter()
            .map(|a| Color32::from_white_alpha(*a))
            .collect(),
    )
}

/// Drops the textures of add-on `id` (all its icon versions), so the next frame reads the file
/// again and a removed add-on's texture is freed.
pub(crate) fn forget(ctx: &egui::Context, id: &str) {
    let prefix = format!("{}{id}-", icons::KEY_PREFIX);
    let textures = cache(ctx);
    let mut map = textures.lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|key, _| !key.starts_with(&prefix));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mask_is_white_with_the_icon_alpha() {
        let bytes = crate::icons::tests::png(32, 32);
        let (side, alpha) = icons::alpha_mask(&bytes).unwrap();
        let image = mask_image(side, &alpha);
        assert_eq!(image.size, [32, 32]);
        assert_eq!(image.pixels[0], Color32::TRANSPARENT);
        assert_eq!(image.pixels[16 * 32 + 16], Color32::WHITE);
    }

    #[test]
    fn named_icons_and_missing_files_fall_back_and_forget_frees_textures() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(18.0, 18.0));
            assert!(!paint(painter, rect, "chip", Color32::WHITE));
            assert!(!paint(painter, rect, "custom", Color32::WHITE));
            // No such file under the data folder: remembered as unusable, not retried.
            let key = format!("{}com.example.none-{}", icons::KEY_PREFIX, "0".repeat(64));
            assert!(!paint(painter, rect, &key, Color32::WHITE));
        });
        output.textures_delta.clear();
        assert_eq!(cache(&ctx).lock().unwrap().len(), 1);
        forget(&ctx, "com.example.other");
        assert_eq!(cache(&ctx).lock().unwrap().len(), 1);
        forget(&ctx, "com.example.none");
        assert!(cache(&ctx).lock().unwrap().is_empty());
    }
}
