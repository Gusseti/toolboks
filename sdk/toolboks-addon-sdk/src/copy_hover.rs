//! Copy what is under the pointer: hover any text or number in Toolboks and press Ctrl+C.
//!
//! No button, no setup, no change to a page: the copy reads the text that was drawn. It works
//! for every label, heading, value and table cell, also in the host itself and in every
//! add-on, because the add-on runtime calls the same code in the add-on's process.
//!
//! The rule everywhere: a Ctrl+C that something else already handles (a selection in a text
//! field or a selected label) keeps its meaning. Only a Ctrl+C that would copy nothing copies
//! the text under the pointer.

use egui::epaint::{ClippedShape, Shape};
use egui::{Area, Color32, Context, Frame, Id, LayerId, Order, Pos2, RichText};

/// How long the "Copied" note stays, in seconds.
const NOTE_SECONDS: f64 = 1.4;
/// How many characters of the copied text the note shows.
const NOTE_CHARS: usize = 36;
/// How far outside the drawn text a press still counts as on it, in points.
const REACH: f32 = 2.0;

/// True when this pass carries a Ctrl+C (or Cmd+C, Ctrl+Insert).
pub fn copy_pressed(ctx: &Context) -> bool {
    ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
    })
}

/// The text of the topmost drawn text that covers `pos`, or `None` when the pointer is not on
/// any text. Truncated text is returned in full. `shapes` run back to front, as egui paints.
pub fn text_at<'a>(
    shapes: impl IntoIterator<Item = &'a ClippedShape>,
    pos: Pos2,
) -> Option<String> {
    let shapes: Vec<&ClippedShape> = shapes.into_iter().collect();
    shapes
        .into_iter()
        .rev()
        .filter(|clipped| clipped.clip_rect.contains(pos))
        .find_map(|clipped| hit(&clipped.shape, pos))
}

fn hit(shape: &Shape, pos: Pos2) -> Option<String> {
    match shape {
        Shape::Text(text) if text.opacity_factor > 0.0 => {
            if !text.visual_bounding_rect().expand(REACH).contains(pos) {
                return None;
            }
            let full = text.galley.text().trim();
            (!full.is_empty()).then(|| full.to_owned())
        }
        Shape::Vec(inner) => inner.iter().rev().find_map(|shape| hit(shape, pos)),
        _ => None,
    }
}

/// The text under `pos` in what the host has drawn so far in this pass: the layer the pointer
/// is over, then the window background (panels and the central page).
pub fn host_text_at(ctx: &Context, pos: Pos2) -> Option<String> {
    let mut layers = Vec::with_capacity(2);
    layers.extend(ctx.layer_id_at(pos));
    if !layers.contains(&LayerId::background()) {
        layers.push(LayerId::background());
    }
    ctx.graphics(|graphics| {
        layers.iter().find_map(|layer| {
            graphics
                .get(*layer)
                .and_then(|list| text_at(list.all_entries(), pos))
        })
    })
}

/// For the host window, called at the end of each `ui` pass: copies the text under the pointer
/// when Ctrl+C asks for it, and shows the "Copied" note.
pub fn host_pass(ctx: &Context) {
    if copy_pressed(ctx) && !ctx.text_edit_focused() {
        if let Some(text) = ctx
            .pointer_hover_pos()
            .and_then(|pos| host_text_at(ctx, pos))
        {
            ctx.copy_text(text.clone());
            announce(ctx, &text);
        }
    }
    show_note(ctx);
}

/// One note per window: the host draws an add-on in a window of its own too (a detached add-on),
/// and a copy there must not show its note in the main window as well.
fn note_id(ctx: &Context) -> Id {
    Id::NULL
        .with("toolboks-copied-note")
        .with(ctx.viewport_id())
}

/// Remembers that `text` was just copied, so the next passes show the note.
pub fn announce(ctx: &Context, text: &str) {
    let now = ctx.input(|input| input.time);
    let shown: String = text.chars().take(NOTE_CHARS).collect();
    let shown = if text.chars().count() > NOTE_CHARS {
        format!("{}...", shown.trim_end())
    } else {
        shown
    };
    // The id first: `note_id` reads the context, which `data_mut` holds locked.
    let id = note_id(ctx);
    ctx.data_mut(|data| data.insert_temp(id, (now, shown)));
    ctx.request_repaint();
}

/// Draws the "Copied" note next to the pointer for a moment. Does nothing otherwise.
pub fn show_note(ctx: &Context) {
    let id = note_id(ctx);
    let Some((since, shown)) = ctx.data(|data| data.get_temp::<(f64, String)>(id)) else {
        return;
    };
    let now = ctx.input(|input| input.time);
    let left = NOTE_SECONDS - (now - since);
    if left <= 0.0 {
        ctx.data_mut(|data| data.remove::<(f64, String)>(id));
        return;
    }
    let pointer = ctx.pointer_latest_pos().unwrap_or(Pos2::new(24.0, 24.0));
    // Below and right of the pointer; moved back inside the window near its edges.
    let screen = ctx.content_rect();
    let x = (pointer.x + 14.0)
        .min(screen.max.x - 230.0)
        .max(screen.min.x + 4.0);
    let y = if pointer.y + 52.0 > screen.max.y {
        pointer.y - 40.0
    } else {
        pointer.y + 18.0
    };
    Area::new(id.with("area"))
        .order(Order::Tooltip)
        .interactable(false)
        .fixed_pos(Pos2::new(x, y))
        .show(ctx, |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Copied").size(12.0).strong());
                    ui.label(RichText::new(shown).size(12.0).color(Color32::GRAY));
                });
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.min(0.25)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{text::LayoutJob, TextShape};
    use egui::{FontId, Rect, Vec2};

    /// Lays out `text` at `pos` the way a label would draw it.
    fn drawn(ctx: &Context, text: &str, pos: Pos2) -> (Shape, Rect) {
        let galley = ctx.fonts_mut(|fonts| {
            fonts.layout_job(LayoutJob::simple_singleline(
                text.to_owned(),
                FontId::proportional(14.0),
                Color32::WHITE,
            ))
        });
        let shape = TextShape::new(pos, galley, Color32::WHITE);
        let rect = shape.visual_bounding_rect();
        (Shape::Text(shape), rect)
    }

    fn clip(shape: Shape) -> ClippedShape {
        ClippedShape {
            clip_rect: Rect::EVERYTHING,
            shape,
            transform_after_tessellation: Default::default(),
        }
    }

    fn with_ctx<R>(f: impl FnOnce(&Context) -> R) -> R {
        let ctx = Context::default();
        let mut f = Some(f);
        let mut out = None;
        let mut done = ctx.run_ui(Default::default(), |_| {
            if let Some(f) = f.take() {
                out = Some(f(&ctx));
            }
        });
        done.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn the_text_under_the_pointer_is_found_and_other_places_are_not() {
        with_ctx(|ctx| {
            let (a, ra) = drawn(ctx, "Memory used", Pos2::new(10.0, 10.0));
            let (b, rb) = drawn(ctx, "8.2 GB", Pos2::new(10.0, 60.0));
            let shapes = [clip(a), clip(b)];
            assert_eq!(
                text_at(&shapes, ra.center()).as_deref(),
                Some("Memory used")
            );
            assert_eq!(text_at(&shapes, rb.center()).as_deref(), Some("8.2 GB"));
            assert_eq!(text_at(&shapes, Pos2::new(300.0, 300.0)), None);
        });
    }

    #[test]
    fn text_in_front_wins_and_clipped_away_text_is_ignored() {
        with_ctx(|ctx| {
            let (back, r) = drawn(ctx, "behind", Pos2::new(10.0, 10.0));
            let (front, _) = drawn(ctx, "in front", Pos2::new(10.0, 10.0));
            let shapes = [clip(back.clone()), clip(front)];
            assert_eq!(text_at(&shapes, r.center()).as_deref(), Some("in front"));
            let mut clipped = clip(back);
            clipped.clip_rect = Rect::from_min_size(Pos2::new(500.0, 500.0), Vec2::splat(10.0));
            assert_eq!(text_at(&[clipped], r.center()), None);
        });
    }

    #[test]
    fn nested_shapes_blank_text_and_the_full_text_of_cut_off_labels() {
        with_ctx(|ctx| {
            let (inner, r) = drawn(ctx, "  12.5 %  ", Pos2::new(10.0, 10.0));
            let nested = clip(Shape::Vec(vec![Shape::Noop, inner]));
            assert_eq!(text_at([&nested], r.center()).as_deref(), Some("12.5 %"));
            let (blank, rb) = drawn(ctx, "   ", Pos2::new(10.0, 80.0));
            assert_eq!(text_at(&[clip(blank)], rb.center()), None);
            // A label cut short with "..." still copies everything.
            let mut job = LayoutJob::single_section(
                "C:\\Users\\someone\\Documents\\a very long file name.txt".to_owned(),
                egui::TextFormat::default(),
            );
            job.wrap.max_width = 40.0;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
            let shape = TextShape::new(Pos2::new(10.0, 120.0), galley, Color32::WHITE);
            let rect = shape.visual_bounding_rect();
            let found = text_at(&[clip(Shape::Text(shape))], rect.center());
            assert_eq!(
                found.as_deref(),
                Some("C:\\Users\\someone\\Documents\\a very long file name.txt")
            );
        });
    }

    #[test]
    fn a_press_of_ctrl_c_copies_the_text_under_the_pointer() {
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            events: vec![
                egui::Event::PointerMoved(Pos2::new(110.0, 25.0)),
                egui::Event::Copy,
            ],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.put(
                Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(200.0, 30.0)),
                egui::Label::new("Disk usage 71 %").selectable(false),
            );
            host_pass(ui.ctx());
        });
        let copied: Vec<_> = out
            .platform_output
            .commands
            .iter()
            .filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        out.textures_delta.clear();
        assert_eq!(copied, vec!["Disk usage 71 %".to_owned()]);
    }

    #[test]
    fn without_ctrl_c_nothing_is_copied_and_the_note_is_short() {
        let ctx = Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            events: vec![egui::Event::PointerMoved(Pos2::new(30.0, 20.0))],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label("Disk usage 71 %");
            host_pass(ui.ctx());
        });
        out.textures_delta.clear();
        assert!(out.platform_output.commands.is_empty());
        with_ctx(|ctx| {
            announce(ctx, &"x".repeat(100));
            let id = note_id(ctx);
            let (_, shown): (f64, String) = ctx.data(|data| data.get_temp(id)).expect("announced");
            assert!(shown.chars().count() <= NOTE_CHARS + 3);
            assert!(shown.ends_with("..."));
        });
    }
}
