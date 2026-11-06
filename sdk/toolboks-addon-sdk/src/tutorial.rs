//! Stable UI anchors for host-guided walkthroughs. Mark actual layout rectangles,
//! never guessed coordinates. The host clears requests and anchors every frame.
use egui::{Align, Context, Id, Rect, Ui};

#[derive(Clone)]
struct Request {
    target: String,
    reveal: bool,
    rect: Option<Rect>,
}
fn id() -> Id {
    Id::unique("toolboks-guided-tour")
}
pub fn prepare(ctx: &Context, target: Option<&str>, reveal: bool) {
    ctx.data_mut(|d| {
        d.remove::<Request>(id());
        if let Some(target) = target {
            d.insert_temp(
                id(),
                Request {
                    target: target.into(),
                    reveal,
                    rect: None,
                },
            );
        }
    });
}
pub fn mark(ui: &mut Ui, target: &str, rect: Rect) {
    let request = ui.ctx().data_mut(|d| d.get_temp::<Request>(id()));
    if let Some(mut request) = request.filter(|r| r.target == target) {
        if request.reveal {
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
        let visible = rect.intersect(ui.clip_rect());
        if visible.is_positive() {
            request.rect = Some(visible);
        }
        ui.ctx().data_mut(|d| d.insert_temp(id(), request));
    }
}
pub fn region<R>(ui: &mut Ui, target: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let response = ui.scope(add);
    let requested = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Request>(id()))
        .is_some_and(|r| r.target == target);
    if requested && !response.response.rect.is_positive() {
        let empty = ui.label("This section has no data to display yet.");
        mark(ui, target, empty.rect);
    } else {
        mark(ui, target, response.response.rect);
    }
    response.inner
}
pub fn anchor(ctx: &Context) -> Option<Rect> {
    ctx.data_mut(|d| d.get_temp::<Request>(id()))
        .and_then(|r| r.rect)
}
pub fn active(ctx: &Context) -> bool {
    ctx.data_mut(|d| d.get_temp::<Request>(id())).is_some()
}
