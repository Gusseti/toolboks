//! Health on Home (#54, #182): drawing only, from [`crate::health::View`].
//!
//! The add-ons measure; the host shows what they reported, ranked by `health::Store::view`.
//! There is no score and no overall verdict: when nothing needs attention, Home says what was
//! checked and nothing else (docs/PLAN-health.md section 3).
//!
//! [`board`] turns a view into the text and order Home shows. It is pure (it takes the time), so
//! the words, the freshness and the primary action are tested without a window. [`draw`] paints
//! a board and returns what was pressed; [`App::health`] wires it to the store, the supervisor
//! ("Check again") and the link path (`OpenPage`).

use super::shell::{focus_ring, panel, Screen};
use super::*;
use crate::health::{Row, View};
use crate::ui::{MUTED, TEXT};
use egui::{Color32, RichText, Vec2};
use std::time::Instant;
use toolboks_addon_proto::{FindingAction, FindingStatus, HostMessage, Severity};

/// "Checking…" shows until the next findings arrive or this long has passed.
pub(super) const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// While Home is shown it repaints this often, so "checked 4 min ago" stays true.
pub(super) const FRESHNESS_REPAINT: Duration = Duration::from_secs(30);
/// A row narrower than this puts its action under the text instead of beside it.
pub(super) const NARROW_ROW: f32 = 560.0;
/// Width of the status column (dot and word), so titles line up.
const STATUS_W: f32 = 84.0;
/// Height of a one-line "Also checked" row.
const LINE_H: f32 = 28.0;
/// Text-sized accent that reaches 4.5:1 on a card.
const ACCENT_TEXT: Color32 = look::DATA_MEMORY;

/// The status of one row, always shown as a word (and a dot where it has a colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Problem,
    Watch,
    Ok,
    NotChecked,
    Unavailable,
    OutOfDate,
}

impl Status {
    pub(super) fn word(self) -> &'static str {
        match self {
            Status::Problem => "Problem",
            Status::Watch => "Watch",
            Status::Ok => "OK",
            Status::NotChecked => "Not checked",
            Status::Unavailable => "Unavailable",
            Status::OutOfDate => "Out of date",
        }
    }
    /// The dot's colour; the muted states have no coloured dot.
    pub(super) fn dot(self) -> Option<Color32> {
        match self {
            Status::Problem => Some(look::RED),
            Status::Watch => Some(look::AMBER),
            Status::Ok => Some(look::GREEN),
            Status::NotChecked | Status::Unavailable | Status::OutOfDate => None,
        }
    }
    fn word_color(self) -> Color32 {
        self.dot().unwrap_or(MUTED)
    }
}

/// What pressing something on the Health card asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Act {
    CheckAgain,
    Browse,
    /// Open the add-on `id`, on `route` (an `OpenPage` route) when given.
    Open {
        id: String,
        route: Option<String>,
    },
}

/// The one button of a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Button {
    pub label: String,
    pub act: Act,
    /// The accent fill: the first Problem row's action, at most one per view.
    pub primary: bool,
}

/// One row as Home shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Line {
    pub status: Status,
    pub title: String,
    /// What was observed (attention rows, and the reason on a not-checked or unavailable row).
    pub observation: String,
    /// An `Instruction` action: a 12 px line under the observation, no button.
    pub instruction: Option<String>,
    /// Source and freshness, 11 px muted.
    pub meta: String,
    pub button: Option<Button>,
}

/// An installed add-on that names checks in its catalog entry but has not reported yet (#183).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Waiting {
    pub id: String,
    pub title: String,
    pub checks: Vec<String>,
    pub running: bool,
}

/// Everything the Health card shows, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Board {
    /// The 11 px line under "Health".
    pub caption: String,
    /// No add-on has reported: the card offers "Browse add-ons".
    pub nothing_reported: bool,
    pub attention: Vec<Line>,
    /// "2 more from Disk Manager" links, with the add-on id.
    pub more: Vec<(String, String)>,
    pub checked: Vec<Line>,
    /// "Toolboks checks only what your add-ons measure." (nothing needs attention).
    pub footer: bool,
    /// "Check again" has a running add-on to ask.
    pub can_check: bool,
    pub checking: bool,
}

pub(super) const FOOTER: &str = "Toolboks checks only what your add-ons measure.";

/// "checked just now" (under a minute), "checked 4 min ago", "checked 2 h ago",
/// "checked 3 days ago".
pub(super) fn freshness(now: u64, measured_at: u64) -> String {
    let secs = now.saturating_sub(measured_at);
    match secs {
        0..60 => "checked just now".into(),
        60..3_600 => format!("checked {} min ago", secs / 60),
        3_600..86_400 => format!("checked {} h ago", secs / 3_600),
        _ => match secs / 86_400 {
            1 => "checked 1 day ago".into(),
            days => format!("checked {days} days ago"),
        },
    }
}

/// The action moves under the text when the row is narrower than [`NARROW_ROW`].
pub(super) fn action_below(row_width: f32) -> bool {
    row_width < NARROW_ROW
}

/// "Check again" was pressed: "Checking…" until the store receives another set of findings or
/// [`CHECK_TIMEOUT`] passes.
#[derive(Clone, Copy, Debug)]
pub(super) struct CheckAgain {
    started: Instant,
    received: u64,
}

impl CheckAgain {
    /// `received` is `health::Store::received` when the button was pressed.
    pub(super) fn new(started: Instant, received: u64) -> Self {
        Self { started, received }
    }
    pub(super) fn active(&self, now: Instant, received: u64) -> bool {
        received == self.received && now.saturating_duration_since(self.started) < CHECK_TIMEOUT
    }
    /// How long until the timeout, for the repaint.
    pub(super) fn left(&self, now: Instant) -> Duration {
        CHECK_TIMEOUT.saturating_sub(now.saturating_duration_since(self.started))
    }
}

fn status_of(row: &Row) -> Status {
    match row.finding.status {
        FindingStatus::Attention => match row.finding.severity {
            Severity::Problem => Status::Problem,
            Severity::Watch => Status::Watch,
        },
        FindingStatus::Ok if row.out_of_date => Status::OutOfDate,
        FindingStatus::Ok => Status::Ok,
        FindingStatus::NotChecked => Status::NotChecked,
        FindingStatus::Unavailable => Status::Unavailable,
    }
}

/// Source and freshness: "Disk Manager · checked 3 min ago". A stopped add-on is named at the
/// end instead ("checked 3 h ago · Event Watch is not running"). `fresh` adds the freshness.
fn meta(row: &Row, now: u64, fresh: bool) -> Vec<String> {
    let mut parts = Vec::new();
    if row.running {
        parts.push(row.source_title.clone());
    }
    if fresh {
        parts.push(freshness(now, row.finding.measured_at));
    }
    if !row.running {
        parts.push(format!("{} is not running", row.source_title));
    }
    parts
}

fn button_and_instruction(row: &Row) -> (Option<Button>, Option<String>) {
    match &row.finding.action {
        Some(FindingAction::OpenPage { label, route }) => (
            Some(Button {
                label: label.clone(),
                act: Act::Open {
                    id: row.source.clone(),
                    route: Some(route.clone()),
                },
                primary: false,
            }),
            None,
        ),
        Some(FindingAction::Instruction { text }) => (None, Some(text.clone())),
        None => (None, None),
    }
}

fn attention_line(row: &Row, now: u64) -> Line {
    let (button, instruction) = button_and_instruction(row);
    let mut parts = Vec::new();
    if row.out_of_date {
        parts.push("Out of date".to_string());
    }
    parts.extend(meta(row, now, true));
    Line {
        status: status_of(row),
        title: row.finding.title.clone(),
        observation: row.finding.observation.clone(),
        instruction,
        meta: parts.join(" · "),
        button,
    }
}

fn checked_line(row: &Row, now: u64) -> Line {
    let status = status_of(row);
    let (button, instruction) = button_and_instruction(row);
    // The reason matters on a row that was not measured; an OK row's title says what was.
    let observation = match status {
        Status::NotChecked | Status::Unavailable => row.finding.observation.clone(),
        _ => String::new(),
    };
    let fresh = matches!(status, Status::Ok | Status::OutOfDate);
    Line {
        status,
        title: row.finding.title.clone(),
        observation,
        instruction,
        meta: meta(row, now, fresh).join(" · "),
        button,
    }
}

/// "Recent crashes · Event Watch is not running yet", with "Open Event Watch" on the add-on's
/// first row only.
fn waiting_lines(waiting: &Waiting) -> Vec<Line> {
    let why = if waiting.running {
        format!("{} has not reported yet", waiting.title)
    } else {
        format!("{} is not running yet", waiting.title)
    };
    waiting
        .checks
        .iter()
        .enumerate()
        .map(|(index, check)| Line {
            status: Status::NotChecked,
            title: check.clone(),
            observation: String::new(),
            instruction: None,
            meta: why.clone(),
            button: (index == 0).then(|| Button {
                label: format!("Open {}", waiting.title),
                act: Act::Open {
                    id: waiting.id.clone(),
                    route: None,
                },
                primary: false,
            }),
        })
        .collect()
}

/// "A", "A and B", "A, B and C".
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// What the Health card shows for `view` at `now` (Unix seconds). `waiting` are the installed
/// add-ons with catalog checks that have not reported yet.
pub(super) fn board(
    view: &View,
    waiting: &[Waiting],
    now: u64,
    checking: bool,
    can_check: bool,
) -> Board {
    if view.attention.is_empty() && view.checked.is_empty() {
        let mut caption = "No add-on has reported a check yet.".to_string();
        let names: Vec<String> = waiting.iter().map(|w| w.title.clone()).collect();
        if !names.is_empty() {
            let verb = if names.len() == 1 {
                "reports"
            } else {
                "report"
            };
            caption.push_str(&format!(" {} {verb} checks here.", and_list(&names)));
        }
        if checking {
            caption.push_str(" Checking…");
        }
        return Board {
            caption,
            nothing_reported: true,
            attention: Vec::new(),
            more: Vec::new(),
            checked: waiting.iter().flat_map(waiting_lines).collect(),
            footer: true,
            can_check,
            checking,
        };
    }

    let mut attention: Vec<Line> = view
        .attention
        .iter()
        .map(|row| attention_line(row, now))
        .collect();
    // One primary per view: the first Problem row's action.
    if let Some(button) = attention
        .iter_mut()
        .find(|line| line.status == Status::Problem)
        .and_then(|line| line.button.as_mut())
    {
        button.primary = true;
    }

    // Not checked first (the add-ons' own and the ones still waiting), then the store's order.
    let mut not_checked: Vec<(String, Line)> = view
        .checked
        .iter()
        .filter(|row| row.finding.status == FindingStatus::NotChecked)
        .map(|row| (row.source_title.clone(), checked_line(row, now)))
        .collect();
    for w in waiting {
        not_checked.extend(waiting_lines(w).into_iter().map(|l| (w.title.clone(), l)));
    }
    not_checked.sort_by(|a, b| a.0.cmp(&b.0));
    let mut checked: Vec<Line> = not_checked.into_iter().map(|(_, line)| line).collect();
    checked.extend(
        view.checked
            .iter()
            .filter(|row| row.finding.status != FindingStatus::NotChecked)
            .map(|row| checked_line(row, now)),
    );

    let title_of = |id: &str| {
        view.attention
            .iter()
            .chain(&view.checked)
            .find(|row| row.source == id)
            .map_or_else(|| id.to_string(), |row| row.source_title.clone())
    };
    let more: Vec<(String, String)> = view
        .more
        .iter()
        .map(|(id, count)| (id.clone(), format!("{count} more from {}", title_of(id))))
        .collect();

    let total = view.attention.len() + view.more.iter().map(|(_, n)| n).sum::<usize>();
    let current_ok = view
        .checked
        .iter()
        .any(|row| row.finding.status == FindingStatus::Ok && !row.out_of_date);
    let mut caption = if total > 0 {
        if total == 1 {
            "1 thing needs attention.".to_string()
        } else {
            format!("{total} things need attention.")
        }
    } else if current_ok {
        "Nothing needs attention in what was checked.".to_string()
    } else {
        "No add-on has a current result yet.".to_string()
    };
    if checking {
        caption.push_str(" Checking…");
    } else {
        let mut sources: Vec<&str> = view
            .attention
            .iter()
            .chain(&view.checked)
            .map(|row| row.source.as_str())
            .collect();
        sources.sort_unstable();
        sources.dedup();
        caption.push_str(&format!(
            " From {}",
            plural(sources.len(), "add-on", "add-ons")
        ));
        // The newest real measurement; "not checked" and "unavailable" measured nothing.
        let newest = view
            .attention
            .iter()
            .chain(&view.checked)
            .filter(|row| {
                matches!(
                    row.finding.status,
                    FindingStatus::Ok | FindingStatus::Attention
                )
            })
            .map(|row| row.finding.measured_at)
            .max();
        match newest {
            Some(at) => caption.push_str(&format!(", {}.", freshness(now, at))),
            None => caption.push('.'),
        }
    }
    Board {
        caption,
        nothing_reported: false,
        footer: attention.is_empty(),
        attention,
        more,
        checked,
        can_check,
        checking,
    }
}

/// An outlined button (the default), 28 px tall like the primary, with the focus ring.
fn outlined(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(look::TEXT_BODY))
            .corner_radius(look::CONTROL_RADIUS)
            .min_size(Vec2::new(0.0, 28.0)),
    );
    focus_ring(ui, response.rect, &response);
    response
}

fn action(ui: &mut egui::Ui, button: &Button) -> bool {
    let response = if button.primary {
        let response = ui.add(
            egui::Button::new(
                RichText::new(&button.label)
                    .size(look::TEXT_BODY)
                    .color(Color32::WHITE),
            )
            .fill(look::HOST_ACCENT_FILL)
            .corner_radius(look::CONTROL_RADIUS)
            .min_size(Vec2::new(0.0, 28.0)),
        );
        focus_ring(ui, response.rect, &response);
        response
    } else {
        outlined(ui, &button.label)
    };
    response.clicked()
}

/// The width a button with `label` takes.
fn button_width(ui: &egui::Ui, label: &str) -> f32 {
    let galley = egui::WidgetText::from(RichText::new(label).size(look::TEXT_BODY)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// The dot and the word, in a fixed-width column.
fn status_cell(ui: &mut egui::Ui, status: Status, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(STATUS_W, height), egui::Sense::hover());
    let center_y = rect.top() + height.min(LINE_H) / 2.0;
    if let Some(color) = status.dot() {
        ui.painter()
            .circle_filled(egui::pos2(rect.left() + 4.0, center_y), 3.5, color);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 13.0, center_y),
        egui::Align2::LEFT_CENTER,
        status.word(),
        egui::FontId::proportional(look::TEXT_CAPTION),
        status.word_color(),
    );
}

fn label(ui: &mut egui::Ui, text: RichText) {
    ui.add(egui::Label::new(text).wrap());
}

/// One row: status, the text, and the action beside it (or under it when narrow).
fn row(ui: &mut egui::Ui, line: &Line, one_line: bool) -> Option<Act> {
    let mut pressed = None;
    let width = ui.available_width();
    let below = action_below(width);
    let button_w = match (&line.button, below) {
        (Some(button), false) => button_width(ui, &button.label) + look::GAP,
        _ => 0.0,
    };
    let text_w = (width - STATUS_W - look::GAP - button_w).max(120.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = look::GAP;
        let top = if one_line { LINE_H } else { 20.0 };
        status_cell(ui, line.status, top);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            if one_line {
                // Title, then the reason and the source in muted 11 on the same line.
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &line.title,
                    0.0,
                    egui::TextFormat::simple(
                        egui::FontId::new(
                            look::TEXT_BODY,
                            egui::FontFamily::Name(super::store::SEMIBOLD.into()),
                        ),
                        TEXT,
                    ),
                );
                let rest: Vec<&str> = [line.observation.as_str(), line.meta.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect();
                if !rest.is_empty() {
                    job.append(
                        &format!(" · {}", rest.join(" · ")),
                        0.0,
                        egui::TextFormat::simple(
                            egui::FontId::proportional(look::TEXT_CAPTION),
                            MUTED,
                        ),
                    );
                }
                ui.add_space(((LINE_H - 16.0) / 2.0).max(0.0));
                ui.add(egui::Label::new(job).wrap());
            } else {
                ui.add_space(2.0);
                label(ui, semibold(&line.title, look::TEXT_BODY));
                if !line.observation.is_empty() {
                    label(
                        ui,
                        RichText::new(&line.observation)
                            .size(look::TEXT_BODY)
                            .color(TEXT),
                    );
                }
            }
            if let Some(text) = &line.instruction {
                label(ui, RichText::new(text).size(look::TEXT_BODY).color(TEXT));
            }
            if !one_line && !line.meta.is_empty() {
                label(
                    ui,
                    RichText::new(&line.meta)
                        .size(look::TEXT_CAPTION)
                        .color(MUTED),
                );
            }
            if below {
                if let Some(button) = &line.button {
                    ui.add_space(look::SPACE_XS);
                    if action(ui, button) {
                        pressed = Some(button.act.clone());
                    }
                }
            }
        });
        if !below {
            if let Some(button) = &line.button {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if action(ui, button) {
                        pressed = Some(button.act.clone());
                    }
                });
            }
        }
    });
    pressed
}

/// A hairline across the card with `pad` points above and below.
fn hairline(ui: &mut egui::Ui, pad: f32) {
    ui.add_space(pad);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, look::BORDER),
    );
    ui.add_space(pad);
}

/// 600-weight text in the system face.
fn semibold(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .family(egui::FontFamily::Name(super::store::SEMIBOLD.into()))
        .color(TEXT)
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(semibold(text, look::TEXT_BODY));
}

/// Paints the Health card. Tab order: "Check again", "Browse add-ons" when shown, then each
/// row's action top to bottom.
pub(super) fn draw(ui: &mut egui::Ui, board: &Board) -> Option<Act> {
    let mut pressed = None;
    panel().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(semibold("Health", look::TEXT_SECTION));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui
                    .add_enabled_ui(board.can_check && !board.checking, |ui| {
                        outlined(ui, "Check again")
                    })
                    .inner;
                let response = if board.can_check {
                    response
                } else {
                    response.on_disabled_hover_text("No running add-on reports checks right now.")
                };
                if response.clicked() {
                    pressed = Some(Act::CheckAgain);
                }
            });
        });
        ui.add(
            egui::Label::new(
                RichText::new(&board.caption)
                    .size(look::TEXT_CAPTION)
                    .color(MUTED),
            )
            .wrap(),
        );
        if board.nothing_reported {
            ui.add_space(look::GAP);
            if outlined(ui, "Browse add-ons").clicked() {
                pressed = Some(Act::Browse);
            }
        }
        if !board.attention.is_empty() {
            ui.add_space(look::SPACE_M);
            heading(ui, "Needs attention");
            ui.add_space(look::SPACE_XS);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, line) in board.attention.iter().enumerate() {
                    if index > 0 {
                        hairline(ui, look::GAP);
                    }
                    if let Some(act) = row(ui, line, false) {
                        pressed = Some(act);
                    }
                }
            });
            for (id, text) in &board.more {
                ui.add_space(look::SPACE_XS);
                ui.horizontal(|ui| {
                    ui.add_space(STATUS_W + look::GAP);
                    let response = ui.add(
                        egui::Button::new(
                            RichText::new(text).size(look::TEXT_BODY).color(ACCENT_TEXT),
                        )
                        .frame(false),
                    );
                    focus_ring(ui, response.rect, &response);
                    if response.clicked() {
                        pressed = Some(Act::Open {
                            id: id.clone(),
                            route: None,
                        });
                    }
                });
            }
        }
        if !board.checked.is_empty() {
            ui.add_space(look::SPACE_M);
            if !board.nothing_reported {
                heading(ui, "Also checked");
            }
            ui.scope(|ui| {
                // 28 point rows, the hairline between them.
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, line) in board.checked.iter().enumerate() {
                    if index > 0 {
                        hairline(ui, 0.0);
                    }
                    if let Some(act) = row(ui, line, true) {
                        pressed = Some(act);
                    }
                }
            });
        }
        if board.footer {
            ui.add_space(look::SPACE_M);
            ui.label(RichText::new(FOOTER).size(look::TEXT_CAPTION).color(MUTED));
        }
    });
    pressed
}

/// The `resolve_link` target for an add-on titled `title` and an `OpenPage` route: the route's
/// first segment is the page.
pub(super) fn link_target(title: &str, route: Option<&str>) -> String {
    match route.and_then(|route| route.split('/').next()) {
        Some(page) if !page.is_empty() => format!("{title}/{page}"),
        _ => title.to_string(),
    }
}

impl App {
    /// The add-ons "Check again" asks: running at protocol 4 or later, turned on, and either
    /// reporting findings or naming checks in their catalog entry.
    fn check_targets(&self, installed: &[crate::catalog::Entry]) -> Vec<String> {
        let Some(supervisor) = &self.supervisor else {
            return Vec::new();
        };
        installed
            .iter()
            .filter(|entry| self.registry.enabled(&entry.id))
            .filter(|entry| self.services.health.reported(&entry.id) || !entry.checks.is_empty())
            .filter(|entry| {
                supervisor
                    .protocol_version(&entry.id)
                    .is_some_and(|v| v >= toolboks_addon_proto::FINDINGS_PROTOCOL_VERSION)
            })
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// The Health card on Home.
    pub(super) fn health(&mut self, ui: &mut egui::Ui) {
        let now = crate::addon_services::unix_now();
        let installed = crate::installer::installed(&self.registry.root);
        let registry = &self.registry;
        let titles = |id: &str| {
            installed
                .iter()
                .find(|entry| entry.id == id && registry.enabled(id))
                .map(|entry| entry.title.clone())
        };
        let view = self.services.health.view(now, &titles);
        let waiting: Vec<Waiting> = installed
            .iter()
            .filter(|entry| {
                !entry.checks.is_empty()
                    && self.registry.enabled(&entry.id)
                    && !self.services.health.reported(&entry.id)
            })
            .map(|entry| Waiting {
                id: entry.id.clone(),
                title: entry.title.clone(),
                checks: entry.checks.clone(),
                running: self
                    .supervisor
                    .as_ref()
                    .is_some_and(|s| s.protocol_version(&entry.id).is_some()),
            })
            .collect();
        let targets = self.check_targets(&installed);
        let instant = Instant::now();
        let received = self.services.health.received();
        if self
            .health_check
            .is_some_and(|check| !check.active(instant, received))
        {
            self.health_check = None;
        }
        let checking = self.health_check.is_some();
        // Fresh words while Home is shown, and the end of "Checking…" on time. Never while the
        // window is hidden in the tray (eframe may still run a pass for it).
        if !self.hidden {
            ui.ctx().request_repaint_after(match self.health_check {
                Some(check) => check.left(instant).min(FRESHNESS_REPAINT),
                None => FRESHNESS_REPAINT,
            });
        }
        let board = board(&view, &waiting, now, checking, !targets.is_empty());
        let pressed = draw(ui, &board);
        match pressed {
            Some(Act::CheckAgain) => {
                let mut sent = 0;
                if let Some(supervisor) = &self.supervisor {
                    for id in &targets {
                        if supervisor.send(id, &HostMessage::CheckNow) {
                            sent += 1;
                        }
                    }
                }
                if sent > 0 {
                    self.health_check = Some(CheckAgain::new(instant, received));
                }
            }
            Some(Act::Browse) => self.screen = Screen::Browser,
            Some(Act::Open { id, route }) => self.open_finding(&installed, &id, route),
            None => {}
        }
    }

    /// An `OpenPage` action (or "Open <add-on>"): the existing link path, then `Navigate` with the
    /// route. An add-on that is not running yet gets the route when it connects.
    fn open_finding(
        &mut self,
        installed: &[crate::catalog::Entry],
        id: &str,
        route: Option<String>,
    ) {
        let Some(entry) = installed.iter().find(|entry| entry.id == id) else {
            return;
        };
        let target = link_target(&entry.title, route.as_deref());
        match crate::addon_services::resolve_link(
            &target,
            installed,
            |id| self.registry.enabled(id),
            &[],
        ) {
            crate::addon_services::Link::Open { id: opened, page } if opened == id => {
                self.open_addon(id);
                let index = page
                    .and_then(|page| entry.pages.iter().position(|own| *own == page))
                    .unwrap_or(0);
                self.addon_page = index;
                let route = route.unwrap_or_else(|| index.to_string());
                let sent = self.supervisor.as_ref().is_some_and(|supervisor| {
                    supervisor.send(
                        id,
                        &HostMessage::Navigate {
                            page: route.clone(),
                        },
                    )
                });
                self.pending_route = (!sent).then(|| (id.to_string(), route));
            }
            crate::addon_services::Link::TurnedOff { title, .. } => {
                self.message = Some(format!(
                    "{title} is turned off. You can turn it on again in the add-on browser."
                ));
            }
            // Two add-ons with one title: open the one the finding came from, on its first page.
            _ => self.open_addon(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolboks_addon_proto::Finding;

    const NOW: u64 = 1_800_000_000;

    fn row(source: &str, title: &str, finding: Finding) -> Row {
        Row {
            source: source.into(),
            source_title: title.into(),
            out_of_date: false,
            running: true,
            received_at: NOW,
            finding,
        }
    }

    fn problem(id: &str) -> Finding {
        Finding::attention(
            id,
            "Drive C: may fail soon",
            "2 media errors.",
            Severity::Problem,
        )
        .measured_at(NOW - 180)
        .open_page("Open drive health", "Optimization/C:")
    }

    fn watch(id: &str) -> Finding {
        Finding::attention(id, "Memory is under pressure", "Low.", Severity::Watch)
            .measured_at(NOW - 20)
            .open_page("Open Memory Cleaner", "Home")
    }

    fn ok(id: &str, title: &str) -> Finding {
        Finding::ok(id, title, "").measured_at(NOW - 120)
    }

    #[test]
    fn freshness_words() {
        assert_eq!(freshness(NOW, NOW), "checked just now");
        assert_eq!(freshness(NOW, NOW - 59), "checked just now");
        assert_eq!(freshness(NOW, NOW - 60), "checked 1 min ago");
        assert_eq!(freshness(NOW, NOW - 4 * 60 - 30), "checked 4 min ago");
        assert_eq!(freshness(NOW, NOW - 3_599), "checked 59 min ago");
        assert_eq!(freshness(NOW, NOW - 2 * 3_600 - 5), "checked 2 h ago");
        assert_eq!(freshness(NOW, NOW - 86_399), "checked 23 h ago");
        assert_eq!(freshness(NOW, NOW - 86_400), "checked 1 day ago");
        assert_eq!(freshness(NOW, NOW - 3 * 86_400), "checked 3 days ago");
        // A clock that went back never says "ago" in the future.
        assert_eq!(freshness(NOW, NOW + 500), "checked just now");
    }

    #[test]
    fn the_action_moves_under_the_text_below_560_points() {
        assert!(action_below(559.0));
        assert!(!action_below(560.0));
        assert!(!action_below(820.0));
    }

    #[test]
    fn checking_lasts_until_new_findings_or_10_seconds() {
        let start = Instant::now();
        let check = CheckAgain::new(start, 7);
        assert!(check.active(start, 7));
        assert!(check.active(start + Duration::from_millis(9_999), 7));
        assert!(!check.active(start + CHECK_TIMEOUT, 7), "10 s passed");
        assert!(
            !check.active(start + Duration::from_secs(1), 8),
            "findings came"
        );
        assert_eq!(
            check.left(start + Duration::from_secs(4)),
            Duration::from_secs(6)
        );
        assert_eq!(check.left(start + Duration::from_secs(40)), Duration::ZERO);
    }

    #[test]
    fn attention_rows_say_what_where_and_when_and_the_first_problem_is_primary() {
        let view = View {
            attention: vec![
                row("disk", "Disk Manager", problem("drive-c")),
                row("mem", "Memory Cleaner", watch("memory-pressure")),
                row("disk", "Disk Manager", problem("drive-d")),
            ],
            checked: vec![row("events", "Event Watch", ok("crashes", "No crashes"))],
            more: vec![("disk".into(), 2)],
        };
        let board = board(&view, &[], NOW, false, true);
        assert_eq!(
            board.caption,
            "5 things need attention. From 3 add-ons, checked just now."
        );
        let first = &board.attention[0];
        assert_eq!(first.status, Status::Problem);
        assert_eq!(first.status.word(), "Problem");
        assert_eq!(first.status.dot(), Some(look::RED));
        assert_eq!(first.title, "Drive C: may fail soon");
        assert_eq!(first.observation, "2 media errors.");
        assert_eq!(first.meta, "Disk Manager · checked 3 min ago");
        let primaries: Vec<bool> = board
            .attention
            .iter()
            .map(|line| line.button.as_ref().unwrap().primary)
            .collect();
        assert_eq!(primaries, [true, false, false], "one primary per view");
        assert_eq!(
            first.button.as_ref().unwrap().act,
            Act::Open {
                id: "disk".into(),
                route: Some("Optimization/C:".into())
            }
        );
        assert_eq!(board.attention[1].status.word(), "Watch");
        assert_eq!(
            board.more,
            [("disk".into(), "2 more from Disk Manager".into())]
        );
        assert!(!board.footer);
        assert_eq!(board.checked[0].meta, "Event Watch · checked 2 min ago");
    }

    #[test]
    fn no_primary_without_a_problem_and_an_instruction_is_a_line_not_a_button() {
        let instruction = Finding::attention(
            "ram-speed",
            "XMP looks off",
            "2400 of 3600 MT/s.",
            Severity::Watch,
        )
        .measured_at(NOW)
        .instruction("Turn on XMP in the BIOS setup.");
        let view = View {
            attention: vec![
                row("mem", "Memory Cleaner", watch("memory-pressure")),
                row("res", "Resource Monitor", instruction),
            ],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, false, true);
        assert!(board
            .attention
            .iter()
            .filter_map(|line| line.button.as_ref())
            .all(|button| !button.primary));
        let line = &board.attention[1];
        assert_eq!(line.button, None);
        assert_eq!(
            line.instruction.as_deref(),
            Some("Turn on XMP in the BIOS setup.")
        );
        assert_eq!(
            board.caption,
            "2 things need attention. From 2 add-ons, checked just now."
        );
    }

    #[test]
    fn out_of_date_and_stopped_rows_say_so_and_are_never_ok() {
        let mut stale_ok = row("events", "Event Watch", ok("boot-time", "Boot time"));
        stale_ok.finding.measured_at = NOW - 3 * 3_600;
        stale_ok.out_of_date = true;
        stale_ok.running = false;
        let mut stale_watch = row("mem", "Memory Cleaner", watch("memory-pressure"));
        stale_watch.finding.measured_at = NOW - 2 * 86_400;
        stale_watch.out_of_date = true;
        let unavailable = row(
            "disk",
            "Disk Manager",
            Finding::unavailable(
                "drive-d",
                "Drive health of D:",
                "Needs administrator rights",
            )
            .measured_at(NOW),
        );
        let view = View {
            attention: vec![stale_watch],
            checked: vec![unavailable, stale_ok],
            more: vec![],
        };
        let board = board(&view, &[], NOW, false, true);
        assert_eq!(
            board.attention[0].meta,
            "Out of date · Memory Cleaner · checked 2 days ago"
        );
        assert_eq!(board.checked[0].status.word(), "Unavailable");
        assert_eq!(board.checked[0].status.dot(), None);
        assert_eq!(board.checked[0].observation, "Needs administrator rights");
        assert_eq!(board.checked[0].meta, "Disk Manager");
        assert_eq!(board.checked[1].status, Status::OutOfDate);
        assert_eq!(board.checked[1].status.dot(), None);
        assert_eq!(
            board.checked[1].meta,
            "checked 3 h ago · Event Watch is not running"
        );
    }

    #[test]
    fn nothing_needs_attention_says_what_was_checked_and_claims_nothing_else() {
        let view = View {
            checked: vec![
                // In the store's order: by source title.
                row(
                    "disk",
                    "Disk Manager",
                    ok("drive-c", "Drive C: no warnings"),
                ),
                row(
                    "mem",
                    "Memory Cleaner",
                    ok("memory-pressure", "Memory: no pressure"),
                ),
            ],
            ..Default::default()
        };
        let waiting = [Waiting {
            id: "events".into(),
            title: "Event Watch".into(),
            checks: vec!["Recent crashes".into(), "Boot time".into()],
            running: false,
        }];
        let board = board(&view, &waiting, NOW, false, true);
        assert_eq!(
            board.caption,
            "Nothing needs attention in what was checked. From 2 add-ons, checked 2 min ago."
        );
        assert!(board.footer);
        assert!(!board.caption.to_lowercase().contains("healthy"));
        // The waiting add-on's checks come first, as Not checked, one row per name.
        let words: Vec<(&str, &str, &str)> = board
            .checked
            .iter()
            .map(|line| (line.status.word(), line.title.as_str(), line.meta.as_str()))
            .collect();
        assert_eq!(
            words,
            [
                (
                    "Not checked",
                    "Recent crashes",
                    "Event Watch is not running yet"
                ),
                ("Not checked", "Boot time", "Event Watch is not running yet"),
                (
                    "OK",
                    "Drive C: no warnings",
                    "Disk Manager · checked 2 min ago"
                ),
                (
                    "OK",
                    "Memory: no pressure",
                    "Memory Cleaner · checked 2 min ago"
                ),
            ]
        );
        // One "Open Event Watch", on its first row.
        assert_eq!(
            board.checked[0].button.as_ref().unwrap().label,
            "Open Event Watch"
        );
        assert_eq!(board.checked[1].button, None);
    }

    #[test]
    fn missing_evidence_never_reads_as_nothing_needs_attention() {
        let mut stale = row("events", "Event Watch", ok("crashes", "No crashes"));
        stale.out_of_date = true;
        let not_checked = row(
            "disk",
            "Disk Manager",
            Finding::not_checked("drive-c", "Drive C:", "First read still running")
                .measured_at(NOW),
        );
        let view = View {
            checked: vec![not_checked, stale],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, false, true);
        assert_eq!(
            board.caption,
            "No add-on has a current result yet. From 2 add-ons, checked 2 min ago."
        );
    }

    #[test]
    fn nothing_reported_names_the_add_ons_that_will_and_offers_browse() {
        let board = board(&View::default(), &[], NOW, false, false);
        assert_eq!(board.caption, "No add-on has reported a check yet.");
        assert!(board.nothing_reported && board.checked.is_empty() && !board.can_check);
        let waiting = [
            Waiting {
                id: "mem".into(),
                title: "Memory Cleaner".into(),
                checks: vec!["Memory pressure".into()],
                running: true,
            },
            Waiting {
                id: "disk".into(),
                title: "Disk Manager".into(),
                checks: vec!["Drive health".into()],
                running: true,
            },
            Waiting {
                id: "events".into(),
                title: "Event Watch".into(),
                checks: vec!["Recent crashes".into()],
                running: false,
            },
        ];
        let board = super::board(&View::default(), &waiting, NOW, false, true);
        assert_eq!(
            board.caption,
            "No add-on has reported a check yet. Memory Cleaner, Disk Manager and Event Watch report checks here."
        );
        assert_eq!(board.checked.len(), 3);
        assert_eq!(board.checked[0].meta, "Memory Cleaner has not reported yet");
        assert_eq!(board.checked[2].meta, "Event Watch is not running yet");
        let one = super::board(&View::default(), &waiting[..1], NOW, true, true);
        assert_eq!(
            one.caption,
            "No add-on has reported a check yet. Memory Cleaner reports checks here. Checking…"
        );
    }

    #[test]
    fn checking_replaces_the_freshness_in_the_caption() {
        let view = View {
            attention: vec![row("mem", "Memory Cleaner", watch("memory-pressure"))],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, true, true);
        assert_eq!(board.caption, "1 thing needs attention. Checking…");
    }

    #[test]
    fn open_page_routes_resolve_by_their_first_segment() {
        assert_eq!(
            link_target("Disk Manager", Some("Optimization/C:")),
            "Disk Manager/Optimization"
        );
        assert_eq!(
            link_target("Memory Cleaner", Some("Home")),
            "Memory Cleaner/Home"
        );
        assert_eq!(link_target("Event Watch", None), "Event Watch");
        assert_eq!(link_target("Event Watch", Some("")), "Event Watch");
    }

    /// Runs one frame of `draw` in a 900 x 900 window with the given input events, and returns
    /// what was pressed and the label of the widget with keyboard focus.
    fn frame(ctx: &egui::Context, board: &Board, width: f32, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 1400.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            draw(ui, board);
        });
        output.textures_delta.clear();
    }

    fn tab() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Rect> {
        let id = ctx.memory(|m| m.focused())?;
        Some(ctx.read_response(id)?.rect)
    }

    fn sample() -> Board {
        let view = View {
            attention: vec![
                row("disk", "Disk Manager", problem("drive-c")),
                row("mem", "Memory Cleaner", watch("memory-pressure")),
            ],
            checked: vec![row(
                "events",
                "Event Watch",
                Finding::not_checked("crashes", "Recent crashes", "")
                    .measured_at(NOW)
                    .open_page("Open Event Watch", "Crashes"),
            )],
            more: vec![],
        };
        board(&view, &[], NOW, false, true)
    }

    #[test]
    fn tab_goes_check_again_then_each_action_top_to_bottom() {
        for width in [820.0, 500.0] {
            let ctx = egui::Context::default();
            crate::ui::apply_style(&ctx);
            super::super::store::install_fonts(&ctx);
            let board = sample();
            frame(&ctx, &board, width, vec![]);
            let mut rects = Vec::new();
            for _ in 0..4 {
                frame(&ctx, &board, width, tab());
                frame(&ctx, &board, width, vec![]);
                rects.push(focused(&ctx).expect("something has focus"));
            }
            // Check again sits at the top right, then the three actions go down the card.
            assert!(rects[0].top() < rects[1].top(), "{width}: {rects:?}");
            assert!(rects[1].top() < rects[2].top(), "{width}: {rects:?}");
            assert!(rects[2].top() < rects[3].top(), "{width}: {rects:?}");
            assert!(
                rects[0].right() >= rects[1].right() - 1.0,
                "{width}: {rects:?}"
            );
            if width < NARROW_ROW + 40.0 {
                // Narrow: the action sits under the text, at the text's left edge.
                assert!(rects[1].left() < width / 2.0, "{width}: {rects:?}");
            } else {
                assert!(rects[1].right() > width - 60.0, "{width}: {rects:?}");
            }
        }
    }
}
