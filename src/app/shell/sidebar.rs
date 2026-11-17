//! The add-on list in the sidebar: the rows, the groups, dragging, the right-click menus and the
//! small window that asks for a group name. The order and the groups themselves are plain data in
//! `crate::sidebar_layout`.

use super::*;
use crate::sidebar_layout::{self as layout, Drop};
use std::collections::HashMap;

/// What a row stands for.
#[derive(Clone, PartialEq, Debug)]
enum Kind {
    Group(String),
    Addon { id: String, group: Option<String> },
}

struct Slot {
    rect: egui::Rect,
    kind: Kind,
}

/// A change the user asked for. Collected while drawing, applied afterwards.
#[derive(Debug, PartialEq)]
enum Action {
    Open(String),
    Toggle(String),
    Drag(Kind, Drop),
    Step(Kind, i32),
    NewGroup(Option<String>),
    Rename(String),
    Delete(String),
    ToGroup(String, String),
    Out(String),
}

struct Info {
    title: String,
    icon: String,
    pages: Vec<String>,
}

/// Where the dragged row would land, as a mark to paint.
#[derive(Debug, PartialEq)]
enum Mark {
    Line { y: f32, left: f32, right: f32 },
    Header(egui::Rect),
}

fn drag_id() -> egui::Id {
    egui::Id::unique("sidebar-drag")
}

#[cfg(test)]
fn test_rect_id(kind: &Kind) -> egui::Id {
    egui::Id::unique(("sidebar-test-rect", format!("{kind:?}")))
}

/// What the list needs to draw itself.
struct List<'a> {
    rows: &'a [layout::Row],
    info: &'a HashMap<String, Info>,
    tags: &'a HashMap<String, Tag>,
    groups: Vec<String>,
    active: Option<&'a str>,
    expanded: bool,
}

impl App {
    /// The add-ons below Home: top-level add-ons and groups in the user's order.
    pub(super) fn sidebar_list(&mut self, ui: &mut egui::Ui, expanded: bool) {
        let all: Vec<String> = self
            .registry
            .packages
            .iter()
            .map(|p| p.manifest.id.clone())
            .collect();
        layout::normalize(&mut self.registry.config.sidebar_layout, &all);
        let info: HashMap<String, Info> = self
            .registry
            .packages
            .iter()
            .filter(|p| self.registry.enabled(&p.manifest.id))
            .map(|p| {
                (
                    p.manifest.id.clone(),
                    Info {
                        title: p.manifest.title.clone(),
                        icon: p.manifest.icon.clone(),
                        pages: p.pages.iter().map(|page| page.title.clone()).collect(),
                    },
                )
            })
            .collect();
        let tags: HashMap<String, Tag> = info
            .keys()
            .map(|id| (id.clone(), self.addon_tag(id)))
            .collect();
        let layout_now = self.registry.config.sidebar_layout.clone();
        let rows = layout::rows(&layout_now, |id| info.contains_key(id));
        let active = match &self.screen {
            Screen::External(id) => Some(id.clone()),
            _ => None,
        };
        let list = List {
            rows: &rows,
            info: &info,
            tags: &tags,
            groups: layout::group_names(&layout_now),
            active: active.as_deref(),
            expanded,
        };
        let actions = list_ui(ui, &list, &mut |ui, id| {
            let Some(i) = info.get(id) else { return };
            self.market_sidebar_status(ui, id, &i.title, expanded);
            if active.as_deref() == Some(id) && expanded && i.pages.len() > 1 {
                for (p, name) in i.pages.iter().enumerate() {
                    if page_row(ui, name, self.addon_page == p) {
                        self.addon_page = p;
                    }
                }
            }
        });
        self.apply_sidebar_actions(actions, &info);
    }

    fn apply_sidebar_actions(&mut self, actions: Vec<Action>, info: &HashMap<String, Info>) {
        let mut changed = false;
        let visible = |id: &str| info.contains_key(id);
        for action in actions {
            let layout = &mut self.registry.config.sidebar_layout;
            match action {
                Action::Open(id) => self.open_addon(&id),
                Action::Toggle(name) => {
                    let open = layout.iter().any(
                        |item| matches!(item, layout::Item::Group(g) if g.name == name && !g.collapsed),
                    );
                    layout::set_collapsed(layout, &name, open);
                    changed = true;
                }
                Action::Drag(Kind::Addon { id, .. }, to) => {
                    changed |= layout::move_addon(layout, &id, &to);
                }
                Action::Drag(Kind::Group(name), to) => {
                    changed |= layout::move_group(layout, &name, &to);
                }
                Action::Step(Kind::Addon { id, .. }, delta) => {
                    changed |= layout::step_addon(layout, &id, delta, visible);
                }
                Action::Step(Kind::Group(name), delta) => {
                    changed |= layout::step_group(layout, &name, delta, visible);
                }
                Action::NewGroup(addon) => {
                    self.group_dialog = Some(GroupDialog {
                        rename: None,
                        addon,
                        text: String::new(),
                        error: None,
                    });
                }
                Action::Rename(name) => {
                    self.group_dialog = Some(GroupDialog {
                        text: name.clone(),
                        rename: Some(name),
                        addon: None,
                        error: None,
                    });
                }
                Action::Delete(name) => changed |= layout::delete_group(layout, &name),
                Action::ToGroup(id, group) => {
                    changed |= layout::move_addon(layout, &id, &Drop::IntoGroup(group));
                }
                Action::Out(id) => changed |= layout::remove_from_group(layout, &id),
            }
        }
        if changed {
            self.persist();
        }
    }

    /// The window that asks for a group name, for a new group or a rename.
    pub(super) fn group_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.group_dialog.take() else {
            return;
        };
        let renaming = dialog.rename.is_some();
        let (mut submit, mut cancel) = (false, false);
        let response = egui::Modal::new(egui::Id::unique("group-dialog"))
            .frame(panel())
            .show(ctx, |ui| {
                ui.set_width(modal_width(ctx, 360.0));
                section(
                    ui,
                    if renaming {
                        "Rename group"
                    } else {
                        "New group"
                    },
                );
                ui.add_space(look::GAP);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut dialog.text)
                        .hint_text("Group name")
                        .char_limit(40)
                        .desired_width(f32::INFINITY),
                );
                // Enter ends the edit (the box loses focus): check before taking focus back.
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                } else if ui.memory(|m| m.focused()).is_none() {
                    edit.request_focus();
                }
                if let Some(error) = &dialog.error {
                    ui.add_space(look::SPACE_XS);
                    ui.label(
                        RichText::new(error)
                            .size(look::TEXT_BODY)
                            .color(look::AMBER),
                    );
                }
                ui.add_space(look::SPACE_M);
                ui.horizontal(|ui| {
                    let label = if renaming { "Rename" } else { "Create group" };
                    if primary(ui, label).clicked() {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if response.should_close() {
            cancel = true;
        }
        if submit {
            let layout = &mut self.registry.config.sidebar_layout;
            let result = match &dialog.rename {
                Some(old) => layout::rename_group(layout, old, &dialog.text),
                None => layout::new_group(layout, &dialog.text, dialog.addon.as_deref()),
            };
            match result {
                Ok(()) => {
                    self.persist();
                    return;
                }
                Err(e) => dialog.error = Some(e),
            }
        }
        if !cancel {
            self.group_dialog = Some(dialog);
        }
    }
}

/// Draws the rows, handles dragging and returns what the user asked for.
fn list_ui(
    ui: &mut egui::Ui,
    list: &List,
    after_row: &mut dyn FnMut(&mut egui::Ui, &str),
) -> Vec<Action> {
    let ctx = ui.ctx().clone();
    let dragging: Option<Kind> = ctx.data(|d| d.get_temp(drag_id()));
    let mut slots: Vec<Slot> = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    for row in list.rows {
        match row {
            layout::Row::Addon(id) => {
                addon_row(ui, list, id, None, &mut slots, &mut actions);
                after_row(ui, id);
            }
            layout::Row::Group {
                name,
                collapsed,
                ids,
            } => {
                let tag = if *collapsed {
                    ids.iter()
                        .filter_map(|id| list.tags.get(id).copied())
                        .max_by_key(|t| match t {
                            Tag::Update => 2,
                            Tag::New => 1,
                            Tag::None => 0,
                        })
                        .unwrap_or(Tag::None)
                } else {
                    Tag::None
                };
                let response = ui
                    .push_id(("sidebar-group", name), |ui| {
                        group_row(ui, name, ids.len(), *collapsed, list.expanded, tag)
                    })
                    .inner;
                if response.clicked() {
                    actions.push(Action::Toggle(name.clone()));
                }
                let (rename, delete) = (name.clone(), name.clone());
                row_events(
                    &response,
                    Kind::Group(name.clone()),
                    &mut slots,
                    &mut actions,
                    |ui, actions| {
                        if ui.button("Rename...").clicked() {
                            actions.push(Action::Rename(rename));
                            ui.close();
                        }
                        if ui.button("Delete group").clicked() {
                            actions.push(Action::Delete(delete));
                            ui.close();
                        }
                    },
                );
                for id in ids {
                    // The open add-on stays in sight under its closed group.
                    if !*collapsed || list.active == Some(id.as_str()) {
                        addon_row(ui, list, id, Some(name), &mut slots, &mut actions);
                        after_row(ui, id);
                    }
                }
            }
        }
    }
    // The way to make a group: always there, at the end of the list.
    let button = ui
        .push_id("sidebar-new-group", |ui| {
            let response = nav_row(ui, "New group", "plus", false, list.expanded, Tag::None);
            if list.expanded {
                response
            } else {
                focus_tip(&response, "New group");
                response.on_hover_text("New group")
            }
        })
        .inner;
    if button.clicked() {
        actions.push(Action::NewGroup(None));
    }
    if let Some(drag) = dragging {
        drag_ui(ui, list, &slots, drag, &mut actions);
    }
    actions
}

fn addon_row(
    ui: &mut egui::Ui,
    list: &List,
    id: &str,
    group: Option<&str>,
    slots: &mut Vec<Slot>,
    actions: &mut Vec<Action>,
) {
    let Some(info) = list.info.get(id) else {
        return;
    };
    let tag = list.tags.get(id).copied().unwrap_or(Tag::None);
    let on = list.active == Some(id);
    let draw = |ui: &mut egui::Ui| {
        let response = nav_row_sensing(
            ui,
            &info.title,
            sidebar_icon(id, &info.icon),
            on,
            list.expanded,
            tag,
            egui::Sense::click_and_drag(),
        );
        if list.expanded {
            response
        } else {
            let text = match tag {
                Tag::New => format!("{} (new)", info.title),
                Tag::Update => format!("{} (update available)", info.title),
                Tag::None => info.title.clone(),
            };
            focus_tip(&response, &text);
            response.on_hover_text(text)
        }
    };
    let response = ui
        .push_id(("sidebar-addon", id), |ui| {
            if group.is_some() && list.expanded {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 12,
                        ..Default::default()
                    })
                    .show(ui, draw)
                    .inner
            } else {
                draw(ui)
            }
        })
        .inner;
    if response.clicked() {
        actions.push(Action::Open(id.to_owned()));
    }
    let (to_group, out_of, new_for) = (id.to_owned(), id.to_owned(), id.to_owned());
    let current = group.map(str::to_owned);
    let groups = list.groups.clone();
    row_events(
        &response,
        Kind::Addon {
            id: id.to_owned(),
            group: current.clone(),
        },
        slots,
        actions,
        |ui, actions| {
            ui.menu_button("Move to group", |ui| {
                for name in groups.iter().filter(|g| Some(*g) != current.as_ref()) {
                    if ui.button(name).clicked() {
                        actions.push(Action::ToGroup(to_group.clone(), name.clone()));
                        ui.close();
                    }
                }
                if ui.button("New group...").clicked() {
                    actions.push(Action::NewGroup(Some(new_for)));
                    ui.close();
                }
            });
            if current.is_some() && ui.button("Remove from group").clicked() {
                actions.push(Action::Out(out_of));
                ui.close();
            }
        },
    );
}

/// A group's header row. Wide sidebar: chevron, name, count. Rail: the "layers" glyph.
fn group_row(
    ui: &mut egui::Ui,
    name: &str,
    count: usize,
    collapsed: bool,
    expanded: bool,
    tag: Tag,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_H),
        egui::Sense::click_and_drag(),
    );
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            look::SELECTED_BG
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, look::CONTROL_RADIUS, fill);
        focus_ring(ui, rect, &response);
        let dot = match tag {
            Tag::Update => Some(look::AMBER),
            Tag::New => Some(ACCENT),
            Tag::None => None,
        };
        if expanded {
            let c = egui::pos2(rect.left() + 18.0, rect.center().y);
            let stroke = egui::Stroke::new(1.6, MUTED);
            if collapsed {
                ui.painter().line(
                    vec![
                        c + Vec2::new(-2.0, -4.0),
                        c + Vec2::new(2.0, 0.0),
                        c + Vec2::new(-2.0, 4.0),
                    ],
                    stroke,
                );
            } else {
                ui.painter().line(
                    vec![
                        c + Vec2::new(-4.0, -2.0),
                        c + Vec2::new(0.0, 2.0),
                        c + Vec2::new(4.0, -2.0),
                    ],
                    stroke,
                );
            }
            let right = ui.painter().layout_no_wrap(
                count.to_string(),
                egui::FontId::proportional(look::TEXT_CAPTION),
                MUTED,
            );
            let right_x = rect.right() - 18.0 - right.size().x;
            let count_pos = egui::pos2(right_x, rect.center().y - right.size().y / 2.0);
            ui.painter().galley(count_pos, right, MUTED);
            if let Some(color) = dot {
                ui.painter()
                    .circle_filled(egui::pos2(right_x - 10.0, rect.center().y), 3.5, color);
            }
            ui.painter().with_clip_rect(rect).text(
                egui::pos2(rect.left() + LABEL_X, rect.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(look::TEXT_SECTION),
                TEXT,
            );
        } else {
            paint_glyph(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 18.0, rect.center().y),
                    Vec2::splat(18.0),
                ),
                "layers",
                if collapsed { MUTED } else { TEXT },
                18.0,
            );
            if let Some(color) = dot {
                ui.painter().circle_filled(
                    egui::pos2(rect.left() + 28.0, rect.center().y - 8.0),
                    3.5,
                    color,
                );
            }
        }
    }
    let state = if collapsed { "closed" } else { "open" };
    let noun = if count == 1 { "add-on" } else { "add-ons" };
    let label = format!("{name}, group, {count} {noun}, {state}");
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, !collapsed, &label));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if expanded {
        response
    } else {
        focus_tip(&response, &label);
        response.on_hover_text(label)
    }
}

/// Drag start, Alt+Up/Down and the menu key and right-click menu for one row.
fn row_events(
    response: &egui::Response,
    kind: Kind,
    slots: &mut Vec<Slot>,
    actions: &mut Vec<Action>,
    menu: impl FnOnce(&mut egui::Ui, &mut Vec<Action>),
) {
    let ctx = &response.ctx;
    #[cfg(test)]
    ctx.data_mut(|d| d.insert_temp(test_rect_id(&kind), response.rect));
    slots.push(Slot {
        rect: response.rect,
        kind: kind.clone(),
    });
    if response.drag_started() {
        ctx.data_mut(|d| d.insert_temp(drag_id(), kind.clone()));
    }
    if response.has_focus() {
        for (key, delta) in [(egui::Key::ArrowUp, -1), (egui::Key::ArrowDown, 1)] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, key)) {
                actions.push(Action::Step(kind.clone(), delta));
            }
        }
    }
    // The menu key and Shift+F10 open the same menu as a right-click, under the row.
    let at = response.id.with("menu-at");
    let by_key = response.has_focus()
        && ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Menu)
        });
    let mut popup = egui::Popup::context_menu(response);
    if by_key {
        ctx.data_mut(|d| d.insert_temp(at, response.rect.left_bottom()));
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    if response.secondary_clicked() {
        ctx.data_mut(|d| d.remove::<egui::Pos2>(at));
    }
    if let Some(pos) = ctx.data(|d| d.get_temp::<egui::Pos2>(at)) {
        popup = popup.at_position(pos);
    }
    popup.show(|ui| menu(ui, actions));
}

/// The row that follows the pointer, the mark where it would land, and the drop on release.
fn drag_ui(ui: &mut egui::Ui, list: &List, slots: &[Slot], drag: Kind, actions: &mut Vec<Action>) {
    let ctx = ui.ctx().clone();
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let released = !ctx.input(|i| i.pointer.any_down());
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) {
        let overlay = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("sidebar-ghost"),
        ));
        if let Some((drop, mark)) = drop_target(slots, &drag, pos.y) {
            match mark {
                Mark::Line { y, left, right } => {
                    overlay.line_segment(
                        [egui::pos2(left, y), egui::pos2(right, y)],
                        egui::Stroke::new(2.0, ACCENT),
                    );
                }
                Mark::Header(rect) => {
                    overlay.rect_stroke(
                        rect,
                        look::CONTROL_RADIUS,
                        egui::Stroke::new(1.5, ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            if released && !escape {
                actions.push(Action::Drag(drag.clone(), drop));
            }
        }
        if let Some(source) = slots.iter().find(|s| s.kind == drag) {
            let rect = source
                .rect
                .translate(Vec2::new(0.0, pos.y - source.rect.center().y));
            let (label, icon) = match &drag {
                Kind::Group(name) => (name.as_str(), "layers"),
                Kind::Addon { id, .. } => list.info.get(id).map_or(("", "apps"), |i| {
                    (i.title.as_str(), sidebar_icon(id, &i.icon))
                }),
            };
            let painter = overlay;
            // See-through, so the row it is dragged over stays readable.
            painter.rect_filled(rect, look::CONTROL_RADIUS, PANEL_BG.gamma_multiply(0.8));
            painter.rect_stroke(
                rect,
                look::CONTROL_RADIUS,
                egui::Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
            paint_glyph(
                &painter,
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 18.0, rect.center().y),
                    Vec2::splat(18.0),
                ),
                icon,
                ACCENT,
                18.0,
            );
            if list.expanded {
                painter.with_clip_rect(rect).text(
                    egui::pos2(rect.left() + LABEL_X, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(look::TEXT_SECTION),
                    TEXT,
                );
            }
        }
    }
    if released || escape {
        ctx.data_mut(|d| d.remove::<Kind>(drag_id()));
    }
}

/// Where a drag at height `y` would land, and the mark that shows it. `None` when dropping there
/// would change nothing.
///
/// A row swaps places with the one under the pointer as soon as the pointer is inside it: below
/// the dragged row it lands behind that row, above it lands in front. (Waiting for the pointer to
/// pass the middle made short drags do nothing.)
fn drop_target(slots: &[Slot], drag: &Kind, y: f32) -> Option<(Drop, Mark)> {
    let last = slots.last()?;
    let line = |y: f32, rect: &egui::Rect| Mark::Line {
        y,
        left: rect.left(),
        right: rect.right(),
    };
    let distance = |rect: &egui::Rect| (rect.top() - y).max(y - rect.bottom()).max(0.0);
    let source_top = slots.iter().find(|s| s.kind == *drag).map(|s| s.rect.top());
    match drag {
        Kind::Addon { id: dragged, .. } => {
            if y > last.rect.bottom() {
                return Some((Drop::End, line(last.rect.bottom() + 1.0, &last.rect)));
            }
            let slot = slots
                .iter()
                .min_by(|a, b| distance(&a.rect).total_cmp(&distance(&b.rect)))?;
            let rect = slot.rect;
            match &slot.kind {
                Kind::Group(name) => {
                    if y < rect.top() + rect.height() * 0.25 {
                        Some((
                            Drop::BeforeGroup(name.clone()),
                            line(rect.top() - 1.0, &rect),
                        ))
                    } else {
                        Some((Drop::IntoGroup(name.clone()), Mark::Header(rect)))
                    }
                }
                Kind::Addon { id, .. } if id == dragged => None,
                Kind::Addon { id, .. } => {
                    let below = source_top.map_or(y >= rect.center().y, |top| rect.top() > top);
                    if below {
                        Some((Drop::After(id.clone()), line(rect.bottom() + 1.0, &rect)))
                    } else {
                        Some((Drop::Before(id.clone()), line(rect.top() - 1.0, &rect)))
                    }
                }
            }
        }
        Kind::Group(dragged) => {
            let source_top = source_top?;
            // Top-level items: an add-on, or a header with the rows under it.
            let mut tops: Vec<(egui::Rect, Drop, bool)> = Vec::new();
            for slot in slots {
                match &slot.kind {
                    Kind::Group(name) => {
                        tops.push((slot.rect, Drop::BeforeGroup(name.clone()), name == dragged));
                    }
                    Kind::Addon { id, group: None } => {
                        tops.push((slot.rect, Drop::Before(id.clone()), false));
                    }
                    Kind::Addon { group: Some(_), .. } => {
                        if let Some(top) = tops.last_mut() {
                            top.0 = top.0.union(slot.rect);
                        }
                    }
                }
            }
            let (i, (rect, before, is_self)) = tops
                .iter()
                .enumerate()
                .min_by(|a, b| distance(&a.1 .0).total_cmp(&distance(&b.1 .0)))?;
            if *is_self {
                None
            } else if rect.top() > source_top {
                let drop = tops.get(i + 1).map_or(Drop::End, |next| next.1.clone());
                Some((drop, line(rect.bottom() + 1.0, rect)))
            } else {
                Some((before.clone(), line(rect.top() - 1.0, rect)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> HashMap<String, Info> {
        ["a", "b", "c"]
            .iter()
            .map(|id| {
                (
                    (*id).to_owned(),
                    Info {
                        title: id.to_uppercase(),
                        icon: "apps".into(),
                        pages: Vec::new(),
                    },
                )
            })
            .collect()
    }

    fn rows() -> Vec<layout::Row> {
        vec![
            layout::Row::Addon("a".into()),
            layout::Row::Addon("b".into()),
            layout::Row::Group {
                name: "G".into(),
                collapsed: false,
                ids: vec!["c".into()],
            },
        ]
    }

    /// One frame of the list in a sidebar-sized panel.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        expanded: bool,
        rows: &[layout::Row],
        active: Option<&str>,
    ) -> Vec<Action> {
        let info = info();
        let tags = HashMap::new();
        let list = List {
            rows,
            info: &info,
            tags: &tags,
            groups: vec!["G".into()],
            active,
            expanded,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(400.0, 600.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            egui::Panel::left("test-sidebar")
                .exact_size(if expanded { SIDEBAR_W } else { RAIL_W })
                .show(ui, |ui| {
                    actions = list_ui(ui, &list, &mut |_, _| {});
                });
        });
        output.textures_delta.clear();
        actions
    }

    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Centre of the nth row in the test panel (16 px margin, rows of ROW_H and 2 px gaps).
    fn row_center(n: usize, expanded: bool) -> egui::Pos2 {
        egui::pos2(
            if expanded { 100.0 } else { 26.0 },
            8.0 + ROW_H / 2.0 + n as f32 * (ROW_H + 2.0),
        )
    }

    fn settle(ctx: &egui::Context, expanded: bool, rows: &[layout::Row]) {
        crate::ui::apply_style(ctx);
        frame(ctx, vec![], expanded, rows, None);
        frame(ctx, vec![], expanded, rows, None);
    }

    #[test]
    fn dragging_a_row_down_drops_it_after_the_next_one_in_both_layouts() {
        for expanded in [true, false] {
            let ctx = egui::Context::default();
            let rows = rows();
            settle(&ctx, expanded, &rows);
            let from = row_center(0, expanded);
            let to = egui::pos2(from.x, row_center(1, expanded).y + 10.0);
            let mut all = Vec::new();
            all.extend(frame(
                &ctx,
                vec![egui::Event::PointerMoved(from), button(from, true)],
                expanded,
                &rows,
                None,
            ));
            for step in 1..=6 {
                let at = from.lerp(to, step as f32 / 6.0);
                all.extend(frame(
                    &ctx,
                    vec![egui::Event::PointerMoved(at)],
                    expanded,
                    &rows,
                    None,
                ));
            }
            all.extend(frame(&ctx, vec![button(to, false)], expanded, &rows, None));
            assert!(
                all.contains(&Action::Drag(
                    Kind::Addon {
                        id: "a".into(),
                        group: None
                    },
                    Drop::After("b".into())
                )),
                "expanded {expanded}: {all:?}"
            );
            assert!(!all.iter().any(|a| matches!(a, Action::Open(_))));
        }
    }

    #[test]
    fn a_plain_click_still_opens_the_add_on() {
        for expanded in [true, false] {
            let ctx = egui::Context::default();
            let rows = rows();
            settle(&ctx, expanded, &rows);
            let at = row_center(1, expanded);
            let mut all = Vec::new();
            all.extend(frame(
                &ctx,
                vec![egui::Event::PointerMoved(at), button(at, true)],
                expanded,
                &rows,
                None,
            ));
            all.extend(frame(&ctx, vec![button(at, false)], expanded, &rows, None));
            assert_eq!(all, vec![Action::Open("b".into())], "expanded {expanded}");
        }
    }

    #[test]
    fn a_click_on_a_group_header_toggles_it() {
        let ctx = egui::Context::default();
        let rows = rows();
        settle(&ctx, true, &rows);
        let at = row_center(2, true);
        let mut all = frame(
            &ctx,
            vec![egui::Event::PointerMoved(at), button(at, true)],
            true,
            &rows,
            None,
        );
        all.extend(frame(&ctx, vec![button(at, false)], true, &rows, None));
        assert_eq!(all, vec![Action::Toggle("G".into())]);
    }

    #[test]
    fn alt_down_on_a_focused_row_moves_it_and_keeps_focus_on_it() {
        let ctx = egui::Context::default();
        let rows = rows();
        settle(&ctx, true, &rows);
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![key(true)], true, &rows, None);
        frame(&ctx, vec![], true, &rows, None);
        assert!(ctx.memory(|m| m.focused()).is_some(), "a row has focus");
        let alt_down = egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: Some(egui::Key::ArrowDown),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::ALT,
        };
        let before = ctx.memory(|m| m.focused());
        let actions = frame(&ctx, vec![alt_down], true, &rows, None);
        assert_eq!(
            actions,
            vec![Action::Step(
                Kind::Addon {
                    id: "a".into(),
                    group: None
                },
                1
            )]
        );
        assert_eq!(ctx.memory(|m| m.focused()), before, "focus did not jump");
    }

    #[test]
    fn drop_targets() {
        let slot = |top: f32, kind: Kind| Slot {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, top), Vec2::new(200.0, 32.0)),
            kind,
        };
        let addon = |id: &str, group: Option<&str>| Kind::Addon {
            id: id.into(),
            group: group.map(str::to_owned),
        };
        let slots = vec![
            slot(0.0, addon("a", None)),
            slot(34.0, Kind::Group("G".into())),
            slot(68.0, addon("c", Some("G"))),
            slot(102.0, addon("b", None)),
        ];
        let drag_b = addon("b", None);
        // Middle of a header: into the group.
        assert_eq!(
            drop_target(&slots, &drag_b, 50.0).map(|t| t.0),
            Some(Drop::IntoGroup("G".into()))
        );
        // Upper edge of a header: in front of the group.
        assert_eq!(
            drop_target(&slots, &drag_b, 36.0).map(|t| t.0),
            Some(Drop::BeforeGroup("G".into()))
        );
        // Between the group's rows.
        assert_eq!(
            drop_target(&slots, &drag_b, 72.0).map(|t| t.0),
            Some(Drop::Before("c".into()))
        );
        // Below everything: the end of the top level. On itself: nothing.
        assert_eq!(
            drop_target(&slots, &drag_b, 400.0).map(|t| t.0),
            Some(Drop::End)
        );
        assert_eq!(drop_target(&slots, &drag_b, 110.0), None);
        // A group lands among the top-level items only.
        let drag_g = Kind::Group("G".into());
        assert_eq!(
            drop_target(&slots, &drag_g, 5.0).map(|t| t.0),
            Some(Drop::Before("a".into()))
        );
        // On its own rows nothing changes; in the row below it lands behind that row.
        assert_eq!(drop_target(&slots, &drag_g, 90.0), None);
        assert_eq!(
            drop_target(&slots, &drag_g, 115.0).map(|t| t.0),
            Some(Drop::End)
        );
        assert_eq!(
            drop_target(&slots, &drag_g, 400.0).map(|t| t.0),
            Some(Drop::End)
        );
        assert_eq!(drop_target(&slots, &drag_g, 40.0), None);
    }

    /// The real app's sidebar (panel, menu row, Home, the scrolling list, the foot) with three
    /// add-ons installed, driven with the events a real mouse makes.
    struct Whole {
        ctx: egui::Context,
        app: App,
        time: f64,
        /// Last, so the app is gone before its data folder is removed.
        _dir: crate::test_dir::TestDir,
    }

    impl Whole {
        fn new(expanded: bool) -> Self {
            let dir = crate::test_dir::TestDir::new(&format!("sidebar-whole-{expanded}"));
            std::env::set_var("TOOLBOKS_DATA_DIR", &*dir);
            let mut app = App::new();
            app.screen = Screen::Home;
            app.sidebar_expanded = expanded;
            app.registry.packages = ["a", "b", "c"]
                .iter()
                .map(|id| {
                    let mut p = toolboks_addon_sdk::Package::parse(
                        crate::addons::SDK_EXAMPLE.as_bytes(),
                        "0.8.2",
                    )
                    .unwrap();
                    p.manifest.id = (*id).into();
                    p.manifest.title = id.to_uppercase();
                    p
                })
                .collect();
            let ctx = egui::Context::default();
            crate::ui::apply_style(&ctx);
            let mut whole = Self {
                ctx,
                app,
                time: 0.0,
                _dir: dir,
            };
            whole.frame(vec![]);
            whole.frame(vec![]);
            whole
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.016;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 700.0),
                )),
                events,
                focused: true,
                time: Some(self.time),
                ..Default::default()
            };
            let expanded = self.app.sidebar_expanded;
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| app.sidebar_panel(ui, expanded));
            output.textures_delta.clear();
        }

        fn rect_of(&self, id: &str) -> egui::Rect {
            let kind = Kind::Addon {
                id: id.into(),
                group: None,
            };
            self.ctx
                .data(|d| d.get_temp(test_rect_id(&kind)))
                .expect("row was drawn")
        }

        fn order(&self) -> Vec<String> {
            self.app
                .registry
                .config
                .sidebar_layout
                .iter()
                .filter_map(|item| match item {
                    layout::Item::Addon(id) => Some(id.clone()),
                    _ => None,
                })
                .collect()
        }

        /// Press on `from`, move to `to` in small steps, one frame per step, release.
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.frame(vec![egui::Event::PointerMoved(from)]);
            self.frame(vec![button(from, true)]);
            let steps = ((to - from).length() / 3.0).ceil().max(1.0) as usize;
            for step in 1..=steps {
                let at = from.lerp(to, step as f32 / steps as f32);
                self.frame(vec![egui::Event::PointerMoved(at)]);
            }
            self.frame(vec![]);
            self.frame(vec![button(to, false)]);
            self.frame(vec![]);
        }
    }

    #[test]
    fn a_real_mouse_drag_in_the_whole_sidebar_reorders_the_rows() {
        for expanded in [true, false] {
            let mut w = Whole::new(expanded);
            assert_eq!(w.order(), ["a", "b", "c"]);
            let a = w.rect_of("a").center();
            let b = w.rect_of("b");
            // Down past the lower half of "b".
            let to = egui::pos2(a.x, b.bottom() - 4.0);
            w.drag(a, to);
            assert_eq!(w.order(), ["b", "a", "c"], "expanded {expanded}");
        }
    }

    #[test]
    fn a_plain_click_in_the_whole_sidebar_opens_the_add_on() {
        let mut w = Whole::new(true);
        let b = w.rect_of("b").center();
        w.frame(vec![egui::Event::PointerMoved(b)]);
        w.frame(vec![button(b, true)]);
        w.frame(vec![button(b, false)]);
        w.frame(vec![]);
        assert!(w.app.screen == Screen::External("b".into()));
        assert_eq!(w.order(), ["a", "b", "c"]);
    }

    #[test]
    fn a_short_drag_of_one_row_reorders_it() {
        // The pointer only just enters the next row: the row used to stay where it was.
        for expanded in [true, false] {
            let mut w = Whole::new(expanded);
            let a = w.rect_of("a").center();
            let b = w.rect_of("b");
            w.drag(a, egui::pos2(a.x, b.top() + 5.0));
            assert_eq!(w.order(), ["b", "a", "c"], "down, expanded {expanded}");
            let c = w.rect_of("c");
            let a = w.rect_of("a").center();
            w.drag(c.center(), egui::pos2(a.x, a.y + 5.0));
            assert_eq!(w.order(), ["b", "c", "a"], "up, expanded {expanded}");
        }
    }

    /// The row of the "New group" button, found by its place under the last add-on.
    fn new_group_center(w: &Whole) -> egui::Pos2 {
        let c = w.rect_of("c");
        egui::pos2(c.center().x, c.bottom() + 2.0 + ROW_H / 2.0)
    }

    #[test]
    fn the_new_group_button_opens_the_name_window_and_makes_an_empty_group() {
        for expanded in [true, false] {
            let mut w = Whole::new(expanded);
            let at = new_group_center(&w);
            w.frame(vec![egui::Event::PointerMoved(at)]);
            w.frame(vec![button(at, true)]);
            w.frame(vec![button(at, false)]);
            w.frame(vec![]);
            assert!(w.app.group_dialog.is_some(), "expanded {expanded}");
            // Type a name and press Enter in the window.
            let ctx = w.ctx.clone();
            let run = |w: &mut Whole, events: Vec<egui::Event>| {
                w.time += 0.016;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 700.0),
                    )),
                    events,
                    focused: true,
                    time: Some(w.time),
                    ..Default::default()
                };
                let app = &mut w.app;
                let mut output = ctx.run_ui(input, |ui| app.group_dialog_ui(ui.ctx()));
                output.textures_delta.clear();
            };
            for _ in 0..4 {
                run(&mut w, vec![]);
            }
            run(&mut w, vec![egui::Event::Text("Work".into())]);
            run(&mut w, vec![]);
            assert_eq!(
                w.app.group_dialog.as_ref().map(|d| d.text.as_str()),
                Some("Work")
            );
            let enter = |pressed| egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            };
            run(&mut w, vec![enter(true)]);
            run(&mut w, vec![enter(false)]);
            assert!(w.app.group_dialog.is_none(), "closed, expanded {expanded}");
            let layout = &w.app.registry.config.sidebar_layout;
            assert_eq!(
                layout.last(),
                Some(&layout::Item::Group(layout::Group {
                    name: "Work".into(),
                    collapsed: false,
                    ids: vec![]
                }))
            );
        }
    }

    #[test]
    fn an_empty_group_shows_and_takes_a_dropped_add_on() {
        let mut w = Whole::new(true);
        layout::new_group(&mut w.app.registry.config.sidebar_layout, "Work", None).unwrap();
        w.frame(vec![]);
        w.frame(vec![]);
        let header = w
            .ctx
            .data(|d| d.get_temp::<egui::Rect>(test_rect_id(&Kind::Group("Work".into()))))
            .expect("the empty group is drawn");
        let a = w.rect_of("a").center();
        w.drag(a, header.center());
        assert_eq!(
            layout::group_of(&w.app.registry.config.sidebar_layout, "a").as_deref(),
            Some("Work")
        );
    }
}
