//! Health on Home (#54, #182, the "Focus" design): drawing only, from [`crate::health::View`].
//!
//! The add-ons measure; the host shows what they reported, ranked by `health::Store::view`.
//! There is no score and no overall verdict: when nothing needs attention, Home says what was
//! checked and nothing else (docs/PLAN-health.md section 3).
//!
//! Home reads top to bottom: a header ("Home", what was checked and when, "Check again"); the
//! **focus card** for the one finding to start with (the first Problem, else the first Watch),
//! with its evidence picture, why it matters and what to do; "Also worth a look" for the other
//! attention rows as compact cards; "Everything else is fine", the current OK rows grouped by
//! add-on; and the rows without a current result (not checked, unavailable, out of date) as
//! quiet one-line rows with their reason.
//!
//! [`board`] turns a view into the text and order Home shows. It is pure (it takes the time), so
//! the words, the freshness, the focus and the primary action are tested without a window.
//! [`draw`] paints a board and returns what was pressed; [`App::health`] wires it to the store,
//! the supervisor ("Check again") and the link path (`OpenPage`).

use super::shell::{focus_ring, Screen};
use super::*;
use crate::health::{Row, View};
use crate::ui::{MUTED, TEXT};
use egui::{Color32, RichText, Vec2};
use std::sync::Arc;
use std::time::Instant;
use toolboks_addon_proto::{Evidence, FindingAction, FindingStatus, HostMessage, Severity};

/// "Checking…" shows until the next findings arrive or this long has passed.
pub(super) const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// While Home is shown it repaints this often, so "checked 4 min ago" stays true.
pub(super) const FRESHNESS_REPAINT: Duration = Duration::from_secs(30);
/// A row or card narrower than this puts its action (and its evidence) under the text instead
/// of beside it, and stacks the focus card's two columns.
pub(super) const NARROW_ROW: f32 = 560.0;
/// An OK row shows its observation as the value at the right when it is at most this long.
pub(super) const SHORT_VALUE: usize = 24;
/// Width of the status column of a quiet row (dot and word), so titles line up.
const STATUS_W: f32 = 84.0;
/// Height of a one-line quiet row.
const LINE_H: f32 = 28.0;
/// The focus card's title: the one exception to the type scale (DESIGN.md, "Focus card").
const FOCUS_TITLE: f32 = 28.0;
/// The focus card's observation.
const FOCUS_TEXT: f32 = 15.0;
/// The focus card's "Why it matters" and "What to do" text and headings.
const FOCUS_BODY: f32 = 14.0;
/// Width of the evidence picture on a compact card.
const EVIDENCE_W: f32 = 200.0;
/// The narrowest column of the "Everything else is fine" card.
const FINE_COLUMN: f32 = 240.0;
/// Space between the sections of Home.
const SECTION_GAP: f32 = 20.0;
/// Home's health part is at most this wide, so a wide window does not pull a label far from its
/// value or stretch the focus card's text into long lines.
pub(super) const MAX_WIDTH: f32 = 1000.0;
/// An empty column of a series: a stub, so the strip reads as "nothing then".
const TRACK: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x33);
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
    /// The dot's colour, also the colour of the evidence picture; the muted states have none.
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

/// What pressing something on Home's health part asks for.
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
    /// The accent fill: the focus card's action when it is a Problem, at most one per view.
    pub primary: bool,
}

/// One finding as Home shows it: the focus card, a compact card or a quiet row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Line {
    pub status: Status,
    pub title: String,
    /// What was observed (attention rows, and the reason on a not-checked or unavailable row).
    pub observation: String,
    /// "Why it matters" (focus card only).
    pub why: Option<String>,
    /// An `Instruction` action: "What to do" on the focus card, a line of text elsewhere, never
    /// a button.
    pub instruction: Option<String>,
    /// The picture of what was measured, in the status colour.
    pub evidence: Option<Evidence>,
    /// Source and freshness, muted: "Disk Manager, checked 3 min ago" on a card,
    /// "Disk Manager · checked 3 min ago" on a quiet row.
    pub meta: String,
    pub button: Option<Button>,
}

/// One current OK row in "Everything else is fine".
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fact {
    /// The finding's title, muted.
    pub label: String,
    /// The observation when it is at most [`SHORT_VALUE`] characters, else empty.
    pub value: String,
    /// "checked 2 min ago", shown on hover.
    pub detail: String,
}

/// The OK rows of one add-on: one column of "Everything else is fine".
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Group {
    /// The add-on's title ("Event Watch, not running" once it stopped).
    pub title: String,
    pub facts: Vec<Fact>,
}

/// An installed add-on that names checks in its catalog entry but has not reported yet (#183).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Waiting {
    pub id: String,
    pub title: String,
    pub checks: Vec<String>,
    pub running: bool,
}

/// Everything Home's health part shows, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Board {
    /// The muted 12 px line beside "Home": "Health, 11 checks, last result just now".
    pub caption: String,
    /// No add-on has reported: Home offers "Browse add-ons".
    pub nothing_reported: bool,
    /// The plain sentence shown instead of a focus card ("Nothing needs attention in what was
    /// checked.").
    pub headline: Option<String>,
    /// The one attention row to start with: the first Problem, else the first Watch.
    pub focus: Option<Line>,
    /// "Also worth a look": the other attention rows, in rank order.
    pub others: Vec<Line>,
    /// "2 more from Disk Manager" links, with the add-on id.
    pub more: Vec<(String, String)>,
    /// The heading over [`Self::fine`].
    pub fine_heading: &'static str,
    /// Current OK rows by add-on, in the store's order.
    pub fine: Vec<Group>,
    /// Not checked (also the add-ons still waiting), unavailable and out-of-date OK rows.
    pub quiet: Vec<Line>,
    /// "Toolboks checks only what your add-ons measure." (nothing needs attention).
    pub footer: bool,
    /// "Check again" has a running add-on to ask.
    pub can_check: bool,
    pub checking: bool,
}

pub(super) const FOOTER: &str = "Toolboks checks only what your add-ons measure.";
/// The heading over the rows without a current result.
pub(super) const QUIET_HEADING: &str = "Without a current result";

/// "just now" (under a minute), "4 min ago", "2 h ago", "3 days ago".
pub(super) fn ago(now: u64, measured_at: u64) -> String {
    let secs = now.saturating_sub(measured_at);
    match secs {
        0..60 => "just now".into(),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        _ => match secs / 86_400 {
            1 => "1 day ago".into(),
            days => format!("{days} days ago"),
        },
    }
}

/// "checked just now", "checked 4 min ago", "checked 2 h ago", "checked 3 days ago".
pub(super) fn freshness(now: u64, measured_at: u64) -> String {
    format!("checked {}", ago(now, measured_at))
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

/// Source and freshness: "Disk Manager", "checked 3 min ago". A stopped add-on is named at the
/// end instead ("checked 3 h ago", "Event Watch is not running"). `fresh` adds the freshness.
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

/// An attention row as a card: "Disk Manager, checked 3 min ago", "Out of date, …".
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
        why: row.finding.why.clone(),
        instruction,
        evidence: row.finding.evidence.clone(),
        meta: parts.join(", "),
        button,
    }
}

/// A row without a current result, one quiet line as before: "Disk Manager · checked …".
fn quiet_line(row: &Row, now: u64) -> Line {
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
        why: None,
        instruction,
        evidence: None,
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
            why: None,
            instruction: None,
            evidence: None,
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

/// The value an OK row shows at the right: its observation when short, else nothing.
pub(super) fn short_value(observation: &str) -> String {
    if observation.chars().count() <= SHORT_VALUE {
        observation.to_string()
    } else {
        String::new()
    }
}

/// Current OK rows grouped by add-on, in the order the add-ons first appear.
fn fine_groups(rows: &[&Row], now: u64) -> Vec<Group> {
    let mut groups: Vec<(String, Group)> = Vec::new();
    for row in rows {
        let fact = Fact {
            label: row.finding.title.clone(),
            value: short_value(&row.finding.observation),
            detail: freshness(now, row.finding.measured_at),
        };
        match groups.iter_mut().find(|(id, _)| *id == row.source) {
            Some((_, group)) => group.facts.push(fact),
            None => groups.push((
                row.source.clone(),
                Group {
                    title: if row.running {
                        row.source_title.clone()
                    } else {
                        format!("{}, not running", row.source_title)
                    },
                    facts: vec![fact],
                },
            )),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// "A", "A and B", "A, B and C".
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What Home's health part shows for `view` at `now` (Unix seconds). `waiting` are the
/// installed add-ons with catalog checks that have not reported yet.
pub(super) fn board(
    view: &View,
    waiting: &[Waiting],
    now: u64,
    checking: bool,
    can_check: bool,
) -> Board {
    if view.attention.is_empty() && view.checked.is_empty() {
        let mut headline = "No add-on has reported a check yet.".to_string();
        let names: Vec<String> = waiting.iter().map(|w| w.title.clone()).collect();
        if !names.is_empty() {
            let verb = if names.len() == 1 {
                "reports"
            } else {
                "report"
            };
            headline.push_str(&format!(" {} {verb} checks here.", and_list(&names)));
        }
        return Board {
            caption: if checking {
                "Health. Checking…".into()
            } else {
                "Health".into()
            },
            nothing_reported: true,
            headline: Some(headline),
            focus: None,
            others: Vec::new(),
            more: Vec::new(),
            fine_heading: "",
            fine: Vec::new(),
            quiet: waiting.iter().flat_map(waiting_lines).collect(),
            footer: true,
            can_check,
            checking,
        };
    }

    // The focus: the first Problem in rank order, else the first row (a Watch).
    let mut attention: Vec<Line> = view
        .attention
        .iter()
        .map(|row| attention_line(row, now))
        .collect();
    let focus_at = attention
        .iter()
        .position(|line| line.status == Status::Problem)
        .unwrap_or(0);
    let mut focus = (!attention.is_empty()).then(|| attention.remove(focus_at));
    // One primary per view: the focus card's action, when it is a Problem.
    if let Some(button) = focus
        .as_mut()
        .filter(|line| line.status == Status::Problem)
        .and_then(|line| line.button.as_mut())
    {
        button.primary = true;
    }
    let others = attention;

    // Not checked first (the add-ons' own and the ones still waiting), then the store's order.
    let mut not_checked: Vec<(String, Line)> = view
        .checked
        .iter()
        .filter(|row| row.finding.status == FindingStatus::NotChecked)
        .map(|row| (row.source_title.clone(), quiet_line(row, now)))
        .collect();
    for w in waiting {
        not_checked.extend(waiting_lines(w).into_iter().map(|l| (w.title.clone(), l)));
    }
    not_checked.sort_by(|a, b| a.0.cmp(&b.0));
    let mut quiet: Vec<Line> = not_checked.into_iter().map(|(_, line)| line).collect();
    let current_ok = |row: &&Row| row.finding.status == FindingStatus::Ok && !row.out_of_date;
    quiet.extend(
        view.checked
            .iter()
            .filter(|row| row.finding.status != FindingStatus::NotChecked && !current_ok(row))
            .map(|row| quiet_line(row, now)),
    );
    let ok_rows: Vec<&Row> = view.checked.iter().filter(current_ok).collect();
    let fine = fine_groups(&ok_rows, now);

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

    let headline = match focus {
        Some(_) => None,
        None if !fine.is_empty() => Some("Nothing needs attention in what was checked.".into()),
        None => Some("No add-on has a current result yet.".into()),
    };
    // "Everything else" only when there is something else and nothing without a result.
    let fine_heading = if focus.is_some() && quiet.is_empty() {
        "Everything else is fine"
    } else {
        "Checked and fine"
    };

    let checks =
        view.attention.len() + view.more.iter().map(|(_, n)| n).sum::<usize>() + view.checked.len();
    let mut caption = format!(
        "Health, {}",
        if checks == 1 {
            "1 check".to_string()
        } else {
            format!("{checks} checks")
        }
    );
    if checking {
        caption.push_str(". Checking…");
    } else {
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
            Some(at) => caption.push_str(&format!(", last result {}", ago(now, at))),
            None => caption.push_str(", no result yet"),
        }
    }
    Board {
        caption,
        nothing_reported: false,
        headline,
        footer: focus.is_none(),
        focus,
        others,
        more,
        fine_heading,
        fine,
        quiet,
        can_check,
        checking,
    }
}

/// An outlined button (the default) of `height`, with the focus ring.
fn outlined(ui: &mut egui::Ui, label: &str, size: f32, height: f32) -> egui::Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(size))
            .corner_radius(look::CONTROL_RADIUS)
            .min_size(Vec2::new(0.0, height)),
    );
    focus_ring(ui, response.rect, &response);
    response
}

/// A row's action: the accent fill when primary, else outlined. True when pressed.
fn action(ui: &mut egui::Ui, button: &Button, size: f32, height: f32) -> bool {
    let response = if button.primary {
        let response = ui.add(
            egui::Button::new(
                RichText::new(&button.label)
                    .size(size)
                    .color(Color32::WHITE),
            )
            .fill(look::HOST_ACCENT_FILL)
            .corner_radius(look::CONTROL_RADIUS)
            .min_size(Vec2::new(0.0, height)),
        );
        focus_ring(ui, response.rect, &response);
        response
    } else {
        outlined(ui, &button.label, size, height)
    };
    response.clicked()
}

/// The width a button with `label` at `size` takes.
fn button_width(ui: &egui::Ui, label: &str, size: f32) -> f32 {
    let galley = egui::WidgetText::from(RichText::new(label).size(size)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

/// `text` laid out for `width` (wrapped), for measuring before drawing.
fn galley(ui: &egui::Ui, text: impl Into<egui::WidgetText>, width: f32) -> Arc<egui::Galley> {
    text.into().into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        width,
        egui::TextStyle::Body,
    )
}

/// 600-weight text in the system face.
fn semibold(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .size(size)
        .family(egui::FontFamily::Name(super::store::SEMIBOLD.into()))
        .color(TEXT)
}

fn muted(text: &str, size: f32) -> RichText {
    RichText::new(text).size(size).color(MUTED)
}

fn label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).wrap());
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(semibold(text, look::TEXT_SECTION));
}

/// A card: hairline, radius 10, the card fill, `x` and `y` padding.
fn card(x: i8, y: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(look::PANEL_BG)
        .stroke(egui::Stroke::new(1.0, look::BORDER))
        .corner_radius(look::CARD_RADIUS)
        .inner_margin(egui::Margin::symmetric(x, y))
}

/// A filled dot of `diameter` at the left of a `height` tall cell, centred on `center_y`.
fn dot(ui: &egui::Ui, x: f32, center_y: f32, diameter: f32, color: Color32) {
    ui.painter().circle_filled(
        egui::pos2(x + diameter / 2.0, center_y),
        diameter / 2.0,
        color,
    );
}

/// The gap between the evidence picture and its labels.
const EVIDENCE_LABEL_GAP: f32 = 6.0;

/// The labels of `evidence` laid out for `width`: `(left, right, one_line)`.
fn evidence_labels(
    ui: &egui::Ui,
    evidence: &Evidence,
    width: f32,
    label_size: f32,
) -> (Arc<egui::Galley>, Arc<egui::Galley>, bool) {
    let (left, right) = evidence.labels();
    let left = galley(ui, muted(left, label_size), width);
    let right = galley(ui, muted(right, label_size), width);
    let one_line = left.size().x + right.size().x + look::GAP <= width;
    (left, right, one_line)
}

/// The height [`evidence`] takes with the same arguments.
fn evidence_height(
    ui: &egui::Ui,
    evidence: &Evidence,
    width: f32,
    (bar_h, series_h): (f32, f32),
    label_size: f32,
) -> f32 {
    let shape = match evidence {
        Evidence::Bar { .. } => bar_h,
        Evidence::Series { .. } => series_h,
    };
    let (left, right, one_line) = evidence_labels(ui, evidence, width, label_size);
    let labels = if one_line {
        left.size().y.max(right.size().y)
    } else {
        left.size().y + 2.0 + right.size().y
    };
    shape + EVIDENCE_LABEL_GAP + labels
}

/// The evidence picture, `width` wide, in `color`, with its labels at `label_size` under it.
/// A bar is `bar_h` tall and a series `series_h`; the labels carry the numbers, so the colour is
/// never the only signal.
fn evidence(
    ui: &mut egui::Ui,
    evidence: &Evidence,
    color: Color32,
    width: f32,
    (bar_h, series_h): (f32, f32),
    label_size: f32,
) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = EVIDENCE_LABEL_GAP;
        match evidence {
            Evidence::Bar { fraction, .. } => {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(width, bar_h), egui::Sense::hover());
                let radius = bar_h / 2.0;
                ui.painter().rect_filled(rect, radius, look::BORDER);
                let fraction = fraction.clamp(0.0, 1.0);
                if fraction > 0.0 {
                    let mut fill = rect;
                    fill.set_width((rect.width() * fraction).max(bar_h).min(rect.width()));
                    ui.painter().rect_filled(fill, radius, color);
                }
            }
            Evidence::Series { values, .. } => {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(width, series_h), egui::Sense::hover());
                let count = values.len().max(1) as f32;
                let gap = if width / count > 12.0 { 4.0 } else { 2.0 };
                let column = ((width - gap * (count - 1.0)) / count).max(1.0);
                for (index, value) in values.iter().enumerate() {
                    let value = if value.is_finite() {
                        value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let height = (series_h * value).max(4.0);
                    let left = rect.left() + index as f32 * (column + gap);
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(left, rect.bottom() - height),
                        egui::pos2(left + column, rect.bottom()),
                    );
                    let fill = if value > 0.0 { color } else { TRACK };
                    ui.painter().rect_filled(bar, 2.0, fill);
                }
            }
        }
        let (left, right, one_line) = evidence_labels(ui, evidence, width, label_size);
        if one_line {
            // One line: one label at each end.
            let height = left.size().y.max(right.size().y);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
            ui.painter().galley(rect.left_top(), left, MUTED);
            let x = rect.right() - right.size().x;
            ui.painter().galley(egui::pos2(x, rect.top()), right, MUTED);
        } else {
            ui.spacing_mut().item_spacing.y = 2.0;
            for text in [left, right] {
                if !text.text().is_empty() {
                    ui.add(egui::Label::new(text));
                }
            }
        }
    });
}

/// The header: "Home" with the caption beside it, "Check again" at the right.
pub(super) fn header(ui: &mut egui::Ui, board: &Board) -> Option<Act> {
    let mut pressed = None;
    ui.horizontal(|ui| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "Home",
            0.0,
            egui::TextFormat::simple(
                egui::FontId::new(
                    look::TEXT_TITLE,
                    egui::FontFamily::Name(super::store::SEMIBOLD.into()),
                ),
                TEXT,
            ),
        );
        job.append(
            &board.caption,
            look::SPACE_M,
            egui::TextFormat::simple(egui::FontId::proportional(look::TEXT_BODY), MUTED),
        );
        ui.label(job);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = ui
                .add_enabled_ui(board.can_check && !board.checking, |ui| {
                    outlined(ui, "Check again", look::TEXT_BODY, 28.0)
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
    pressed
}

/// "Why it matters" or "What to do": a 14/600 heading over 14 px muted text.
fn column(ui: &mut egui::Ui, title: &str, text: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(semibold(title, FOCUS_BODY));
        label(ui, muted(text, FOCUS_BODY));
    });
}

/// The focus card: the one finding to start with.
fn focus_card(ui: &mut egui::Ui, line: &Line) -> Option<Act> {
    let mut pressed = None;
    card(28, 24).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let width = ui.available_width();
        ui.spacing_mut().item_spacing.y = 0.0;
        // Status: dot, word in its colour, "· Start here".
        let color = line.status.word_color();
        let mut job = egui::text::LayoutJob::default();
        job.append(
            line.status.word(),
            16.0,
            egui::TextFormat::simple(
                egui::FontId::new(13.0, egui::FontFamily::Name(super::store::SEMIBOLD.into())),
                color,
            ),
        );
        job.append(
            "· Start here",
            6.0,
            egui::TextFormat::simple(egui::FontId::proportional(13.0), MUTED),
        );
        let response = ui.label(job);
        if let Some(color) = line.status.dot() {
            dot(
                ui,
                response.rect.left(),
                response.rect.center().y,
                9.0,
                color,
            );
        }
        ui.add_space(16.0);
        label(
            ui,
            semibold(&line.title, FOCUS_TITLE).extra_letter_spacing(-0.4),
        );
        if !line.observation.is_empty() {
            ui.add_space(8.0);
            label(
                ui,
                RichText::new(&line.observation)
                    .size(FOCUS_TEXT)
                    .color(TEXT),
            );
        }
        if let Some(picture) = &line.evidence {
            ui.add_space(20.0);
            let color = line.status.dot().unwrap_or(MUTED);
            let picture_w = match picture {
                Evidence::Bar { .. } => width,
                Evidence::Series { .. } => width.min(360.0),
            };
            evidence(ui, picture, color, picture_w, (10.0, 40.0), look::TEXT_BODY);
        }
        let columns: Vec<(&str, &str)> = [
            ("Why it matters", line.why.as_deref()),
            ("What to do", line.instruction.as_deref()),
        ]
        .into_iter()
        .filter_map(|(title, text)| text.map(|text| (title, text)))
        .collect();
        if !columns.is_empty() {
            ui.add_space(22.0);
            if columns.len() == 2 && !action_below(width) {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 40.0;
                    ui.columns(2, |halves| {
                        for (half, (title, text)) in halves.iter_mut().zip(&columns) {
                            column(half, title, text);
                        }
                    });
                });
            } else {
                for (index, (title, text)) in columns.iter().enumerate() {
                    if index > 0 {
                        ui.add_space(16.0);
                    }
                    column(ui, title, text);
                }
            }
        }
        // Footer: source and freshness at the left, the action at the right (or under it).
        ui.add_space(22.0);
        let below = action_below(width);
        let meta = muted(&line.meta, look::TEXT_BODY);
        match &line.button {
            Some(button) if !below => {
                ui.horizontal(|ui| {
                    ui.label(meta);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action(ui, button, 13.0, 32.0) {
                            pressed = Some(button.act.clone());
                        }
                    });
                });
            }
            Some(button) => {
                label(ui, meta);
                ui.add_space(look::GAP);
                if action(ui, button, 13.0, 32.0) {
                    pressed = Some(button.act.clone());
                }
            }
            None => label(ui, meta),
        }
    });
    pressed
}

/// One "Also worth a look" card: dot, title and status word, the observation and source, the
/// evidence strip and the action at the right (under the text when narrow).
fn compact_card(ui: &mut egui::Ui, line: &Line) -> Option<Act> {
    let mut pressed = None;
    card(20, 16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let width = ui.available_width();
        let below = action_below(width);
        let dot_w = 10.0;
        let gap = 20.0;
        let button_w = match (&line.button, below) {
            (Some(button), false) => button_width(ui, &button.label, look::TEXT_BODY) + gap,
            _ => 0.0,
        };
        let picture_w = match (&line.evidence, below) {
            (Some(_), false) => EVIDENCE_W + gap,
            _ => 0.0,
        };
        let text_w = (width - dot_w - gap - picture_w - button_w).max(120.0);
        // Measured first, so the dot, the picture and the button centre on the text.
        let mut title = egui::text::LayoutJob::default();
        title.append(
            &line.title,
            0.0,
            egui::TextFormat::simple(
                egui::FontId::new(
                    look::TEXT_SECTION,
                    egui::FontFamily::Name(super::store::SEMIBOLD.into()),
                ),
                TEXT,
            ),
        );
        title.append(
            line.status.word(),
            look::GAP,
            egui::TextFormat::simple(
                egui::FontId::new(
                    look::TEXT_BODY,
                    egui::FontFamily::Name(super::store::SEMIBOLD.into()),
                ),
                line.status.word_color(),
            ),
        );
        let mut texts = vec![galley(ui, title, text_w)];
        let body: Vec<&str> = [line.observation.as_str(), line.meta.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
        if !body.is_empty() {
            texts.push(galley(ui, muted(&body.join(" "), look::TEXT_BODY), text_w));
        }
        if let Some(text) = &line.instruction {
            texts.push(galley(
                ui,
                RichText::new(text).size(look::TEXT_BODY).color(TEXT),
                text_w,
            ));
        }
        let spacing = 4.0;
        let text_h: f32 =
            texts.iter().map(|g| g.size().y).sum::<f32>() + spacing * (texts.len() as f32 - 1.0);
        let picture_h = match (&line.evidence, below) {
            (Some(picture), false) => {
                evidence_height(ui, picture, EVIDENCE_W, (8.0, 34.0), look::TEXT_CAPTION)
            }
            _ => 0.0,
        };
        let row_h = text_h.max(picture_h).max(28.0);
        ui.allocate_ui_with_layout(
            Vec2::new(width, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(dot_w, row_h), egui::Sense::hover());
                if let Some(color) = line.status.dot() {
                    let first = texts[0].rows.first().map_or(16.0, |row| row.height());
                    dot(
                        ui,
                        rect.left(),
                        rect.center().y - text_h / 2.0 + first / 2.0,
                        dot_w,
                        color,
                    );
                }
                ui.allocate_ui_with_layout(
                    Vec2::new(text_w, row_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(text_w);
                        ui.add_space((row_h - text_h) / 2.0);
                        ui.spacing_mut().item_spacing.y = spacing;
                        for text in &texts {
                            ui.add(egui::Label::new(text.clone()));
                        }
                    },
                );
                if let (Some(picture), false) = (&line.evidence, below) {
                    ui.allocate_ui_with_layout(
                        Vec2::new(EVIDENCE_W, row_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add_space((row_h - picture_h) / 2.0);
                            let color = line.status.dot().unwrap_or(MUTED);
                            evidence(
                                ui,
                                picture,
                                color,
                                EVIDENCE_W,
                                (8.0, 34.0),
                                look::TEXT_CAPTION,
                            );
                        },
                    );
                }
                if let (Some(button), false) = (&line.button, below) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action(ui, button, look::TEXT_BODY, 28.0) {
                            pressed = Some(button.act.clone());
                        }
                    });
                }
            },
        );
        if below {
            // Narrow: the picture and the action go under the text, at its left edge.
            let indent = dot_w + gap;
            if let Some(picture) = &line.evidence {
                ui.add_space(look::SPACE_M);
                ui.horizontal(|ui| {
                    ui.add_space(indent - ui.spacing().item_spacing.x);
                    ui.vertical(|ui| {
                        let color = line.status.dot().unwrap_or(MUTED);
                        let picture_w = (width - indent).clamp(80.0, 280.0);
                        evidence(
                            ui,
                            picture,
                            color,
                            picture_w,
                            (8.0, 34.0),
                            look::TEXT_CAPTION,
                        );
                    });
                });
            }
            if let Some(button) = &line.button {
                ui.add_space(look::SPACE_M);
                ui.horizontal(|ui| {
                    ui.add_space(indent - ui.spacing().item_spacing.x);
                    if action(ui, button, look::TEXT_BODY, 28.0) {
                        pressed = Some(button.act.clone());
                    }
                });
            }
        }
    });
    pressed
}

/// One OK row of the fine card: a green dot, the label muted, the value at the right.
fn fact_row(ui: &mut egui::Ui, fact: &Fact) {
    let width = ui.available_width();
    let dot_w = 6.0;
    let value = galley(
        ui,
        RichText::new(&fact.value).size(look::TEXT_BODY).color(TEXT),
        width / 2.0,
    );
    let value_w = if fact.value.is_empty() {
        0.0
    } else {
        value.size().x + look::GAP
    };
    let label_w = (width - dot_w - look::GAP - value_w).max(60.0);
    let text = galley(ui, muted(&fact.label, look::TEXT_BODY), label_w);
    let height = text.size().y.max(value.size().y);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let first = text.rows.first().map_or(height, |row| row.height());
    dot(
        ui,
        rect.left(),
        rect.top() + first / 2.0,
        dot_w,
        look::GREEN,
    );
    let x = rect.left() + dot_w + look::GAP;
    ui.painter().galley(egui::pos2(x, rect.top()), text, MUTED);
    if !fact.value.is_empty() {
        let x = rect.right() - value.size().x;
        ui.painter().galley(egui::pos2(x, rect.top()), value, TEXT);
    }
    response.on_hover_text(&fact.detail);
}

/// "Everything else is fine": one card, one column per add-on (stacked when narrow).
fn fine_card(ui: &mut egui::Ui, groups: &[Group]) {
    card(20, 16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let width = ui.available_width();
        let gap = 32.0;
        let per_row =
            (((width + gap) / (FINE_COLUMN + gap)).floor() as usize).clamp(1, groups.len().max(1));
        ui.spacing_mut().item_spacing.x = gap;
        for (index, chunk) in groups.chunks(per_row).enumerate() {
            if index > 0 {
                ui.add_space(look::SPACE_L);
            }
            ui.columns(per_row, |columns| {
                for (column, group) in columns.iter_mut().zip(chunk) {
                    column.spacing_mut().item_spacing.y = 10.0;
                    column.label(semibold(&group.title, look::TEXT_BODY + 1.0));
                    for fact in &group.facts {
                        fact_row(column, fact);
                    }
                }
            });
        }
    });
}

/// The dot and the word of a quiet row, in a fixed-width column.
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

/// One quiet row: status, title, reason and source on one 28 px line, the action beside it (or
/// under it when narrow).
fn quiet_row(ui: &mut egui::Ui, line: &Line) -> Option<Act> {
    let mut pressed = None;
    let width = ui.available_width();
    let below = action_below(width);
    let button_w = match (&line.button, below) {
        (Some(button), false) => button_width(ui, &button.label, look::TEXT_BODY) + look::GAP,
        _ => 0.0,
    };
    let text_w = (width - STATUS_W - look::GAP - button_w).max(120.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = look::GAP;
        status_cell(ui, line.status, LINE_H);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            ui.spacing_mut().item_spacing.y = 2.0;
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
                    egui::TextFormat::simple(egui::FontId::proportional(look::TEXT_CAPTION), MUTED),
                );
            }
            ui.add_space(((LINE_H - 16.0) / 2.0).max(0.0));
            ui.add(egui::Label::new(job).wrap());
            if let Some(text) = &line.instruction {
                label(ui, RichText::new(text).size(look::TEXT_BODY).color(TEXT));
            }
            if below {
                if let Some(button) = &line.button {
                    ui.add_space(look::SPACE_XS);
                    if action(ui, button, look::TEXT_BODY, 28.0) {
                        pressed = Some(button.act.clone());
                    }
                }
            }
        });
        if !below {
            if let Some(button) = &line.button {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if action(ui, button, look::TEXT_BODY, 28.0) {
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

/// Paints Home's health part: [`header`], then [`body`], at most [`MAX_WIDTH`] wide. Tab order:
/// "Check again", "Browse add-ons" when shown, the focus card's action, each compact card's
/// action, the "more" links, then each quiet row's action.
#[cfg(test)]
pub(super) fn draw(ui: &mut egui::Ui, board: &Board) -> Option<Act> {
    ui.scope(|ui| {
        ui.set_max_width(ui.available_width().min(MAX_WIDTH));
        let header = header(ui, board);
        body(ui, board).or(header)
    })
    .inner
}

/// Everything under the header: the focus card (or the headline), "Also worth a look", the fine
/// card, the rows without a current result and the footer line.
pub(super) fn body(ui: &mut egui::Ui, board: &Board) -> Option<Act> {
    let mut pressed = None;
    let mut set = |act: Option<Act>| {
        if act.is_some() {
            pressed = act;
        }
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if let Some(line) = &board.focus {
            ui.add_space(SECTION_GAP);
            set(focus_card(ui, line));
        }
        if let Some(text) = &board.headline {
            ui.add_space(SECTION_GAP);
            label(ui, RichText::new(text).size(FOCUS_TEXT).color(TEXT));
        }
        if board.nothing_reported {
            ui.add_space(look::SPACE_M);
            if outlined(ui, "Browse add-ons", look::TEXT_BODY, 28.0).clicked() {
                set(Some(Act::Browse));
            }
        }
        if !board.others.is_empty() || !board.more.is_empty() {
            ui.add_space(SECTION_GAP + 4.0);
            heading(ui, "Also worth a look");
            for line in &board.others {
                ui.add_space(10.0);
                set(compact_card(ui, line));
            }
            for (id, text) in &board.more {
                ui.add_space(look::SPACE_XS);
                let response = ui.add(
                    egui::Button::new(RichText::new(text).size(look::TEXT_BODY).color(ACCENT_TEXT))
                        .frame(false),
                );
                focus_ring(ui, response.rect, &response);
                if response.clicked() {
                    set(Some(Act::Open {
                        id: id.clone(),
                        route: None,
                    }));
                }
            }
        }
        if !board.fine.is_empty() {
            ui.add_space(SECTION_GAP + 4.0);
            heading(ui, board.fine_heading);
            ui.add_space(10.0);
            fine_card(ui, &board.fine);
        }
        if !board.quiet.is_empty() {
            ui.add_space(SECTION_GAP + 4.0);
            if !board.nothing_reported {
                heading(ui, QUIET_HEADING);
                ui.add_space(10.0);
            }
            card(16, 6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (index, line) in board.quiet.iter().enumerate() {
                    if index > 0 {
                        hairline(ui, 0.0);
                    }
                    set(quiet_row(ui, line));
                }
            });
        }
        if board.footer {
            ui.add_space(look::SPACE_L);
            ui.label(muted(FOOTER, look::TEXT_CAPTION));
        }
        ui.add_space(look::SPACE_L);
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

    /// Health on Home: the header, then `banners` (restore, update, "Pick your tools"), then the
    /// rest, at most [`MAX_WIDTH`] wide.
    pub(super) fn health(
        &mut self,
        ui: &mut egui::Ui,
        banners: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
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
        let pressed = ui
            .scope(|ui| {
                ui.set_max_width(ui.available_width().min(MAX_WIDTH));
                let header = header(ui, &board);
                banners(self, ui);
                body(ui, &board).or(header)
            })
            .inner;
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
                // A detached add-on turns to the page in its own window.
                match self.detached.get_mut(id) {
                    Some(window) => window.page = index,
                    None => self.addon_page = index,
                }
                let route = route.unwrap_or_else(|| index.to_string());
                let sent =
                    crate::addon_view::navigate(self.views.get_mut(id), route.clone(), |message| {
                        self.supervisor
                            .as_ref()
                            .is_some_and(|supervisor| supervisor.send(id, message))
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
        assert_eq!(ago(NOW, NOW - 120), "2 min ago");
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
    fn the_first_problem_is_the_focus_with_the_one_primary_action() {
        let detailed = problem("drive-c")
            .why("Data on a failing drive can be lost.")
            .evidence(Evidence::bar(0.94, "94% worn", "6% left"));
        let view = View {
            attention: vec![
                row("disk", "Disk Manager", detailed),
                row("mem", "Memory Cleaner", watch("memory-pressure")),
                row("disk", "Disk Manager", problem("drive-d")),
            ],
            checked: vec![row("events", "Event Watch", ok("crashes", "No crashes"))],
            more: vec![("disk".into(), 2)],
        };
        let board = board(&view, &[], NOW, false, true);
        assert_eq!(
            board.caption, "Health, 6 checks, last result just now",
            "3 shown, 2 more and 1 OK"
        );
        assert_eq!(board.headline, None);
        let focus = board.focus.as_ref().unwrap();
        assert_eq!(focus.status, Status::Problem);
        assert_eq!(focus.status.dot(), Some(look::RED));
        assert_eq!(focus.title, "Drive C: may fail soon");
        assert_eq!(focus.observation, "2 media errors.");
        assert_eq!(
            focus.why.as_deref(),
            Some("Data on a failing drive can be lost.")
        );
        assert_eq!(
            focus.evidence,
            Some(Evidence::bar(0.94, "94% worn", "6% left"))
        );
        assert_eq!(focus.meta, "Disk Manager, checked 3 min ago");
        let button = focus.button.as_ref().unwrap();
        assert!(button.primary);
        assert_eq!(
            button.act,
            Act::Open {
                id: "disk".into(),
                route: Some("Optimization/C:".into())
            }
        );
        // The rest, in rank order, all outlined.
        let others: Vec<(&str, bool)> = board
            .others
            .iter()
            .map(|line| (line.status.word(), line.button.as_ref().unwrap().primary))
            .collect();
        assert_eq!(others, [("Watch", false), ("Problem", false)]);
        assert_eq!(board.others[0].meta, "Memory Cleaner, checked just now");
        assert_eq!(
            board.more,
            [("disk".into(), "2 more from Disk Manager".into())]
        );
        assert!(!board.footer);
        assert_eq!(board.fine_heading, "Everything else is fine");
        assert_eq!(
            board.fine,
            [Group {
                title: "Event Watch".into(),
                facts: vec![Fact {
                    label: "No crashes".into(),
                    value: String::new(),
                    detail: "checked 2 min ago".into(),
                }],
            }]
        );
        assert!(board.quiet.is_empty());
    }

    #[test]
    fn a_problem_further_down_is_still_the_focus() {
        // The ranking puts current rows first: a current Watch, then an out-of-date Problem.
        let mut stale = row("disk", "Disk Manager", problem("drive-c"));
        stale.out_of_date = true;
        stale.finding.measured_at = NOW - 2 * 86_400;
        let view = View {
            attention: vec![
                row("mem", "Memory Cleaner", watch("memory-pressure")),
                stale,
            ],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, false, true);
        let focus = board.focus.unwrap();
        assert_eq!(focus.status, Status::Problem);
        assert_eq!(focus.meta, "Out of date, Disk Manager, checked 2 days ago");
        assert_eq!(board.others.len(), 1);
        assert_eq!(board.others[0].status, Status::Watch);
    }

    #[test]
    fn a_watch_focus_is_outlined_and_an_instruction_is_what_to_do_not_a_button() {
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
                row("res", "Resource Monitor", instruction),
                row("mem", "Memory Cleaner", watch("memory-pressure")),
            ],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, false, true);
        let focus = board.focus.as_ref().unwrap();
        assert_eq!(focus.status, Status::Watch);
        assert_eq!(focus.button, None);
        assert_eq!(
            focus.instruction.as_deref(),
            Some("Turn on XMP in the BIOS setup.")
        );
        assert_eq!(focus.why, None);
        assert!(board
            .others
            .iter()
            .filter_map(|line| line.button.as_ref())
            .all(|button| !button.primary));
        // A Watch focus with a button is outlined too.
        let view = View {
            attention: vec![row("mem", "Memory Cleaner", watch("memory-pressure"))],
            ..Default::default()
        };
        let board = super::board(&view, &[], NOW, false, true);
        assert!(!board.focus.unwrap().button.unwrap().primary);
        assert!(board.others.is_empty());
        assert_eq!(board.caption, "Health, 1 check, last result just now");
    }

    #[test]
    fn ok_rows_are_grouped_by_add_on_with_short_values_only() {
        let mut stopped = row(
            "events",
            "Event Watch",
            Finding::ok("crashes", "Crashes", "None in 30 days").measured_at(NOW - 60),
        );
        stopped.running = false;
        let view = View {
            attention: vec![row("mem", "Memory Cleaner", watch("memory-pressure"))],
            checked: vec![
                // In the store's order: by source title, then title.
                row(
                    "disk",
                    "Disk Manager",
                    Finding::ok("health", "Drive health", "Good").measured_at(NOW - 60),
                ),
                row(
                    "disk",
                    "Disk Manager",
                    Finding::ok(
                        "space",
                        "Free space on D:",
                        "D: has 61% of its space free, 290 GB of 476 GB.",
                    )
                    .measured_at(NOW - 60),
                ),
                stopped,
                row(
                    "mem",
                    "Memory Cleaner",
                    Finding::ok("page-file", "Page file", "x".repeat(SHORT_VALUE))
                        .measured_at(NOW - 60),
                ),
            ],
            more: vec![],
        };
        let board = board(&view, &[], NOW, false, true);
        let groups: Vec<(&str, Vec<(&str, &str)>)> = board
            .fine
            .iter()
            .map(|group| {
                (
                    group.title.as_str(),
                    group
                        .facts
                        .iter()
                        .map(|fact| (fact.label.as_str(), fact.value.as_str()))
                        .collect(),
                )
            })
            .collect();
        let exactly_short = "x".repeat(SHORT_VALUE);
        assert_eq!(
            groups,
            [
                (
                    "Disk Manager",
                    vec![("Drive health", "Good"), ("Free space on D:", "")]
                ),
                (
                    "Event Watch, not running",
                    vec![("Crashes", "None in 30 days")]
                ),
                (
                    "Memory Cleaner",
                    vec![("Page file", exactly_short.as_str())]
                ),
            ]
        );
        assert_eq!(board.fine[0].facts[0].detail, "checked 1 min ago");
        assert_eq!(short_value(&"y".repeat(SHORT_VALUE + 1)), "");
    }

    #[test]
    fn rows_without_a_current_result_stay_as_quiet_rows_with_their_reason() {
        let mut stale_ok = row("events", "Event Watch", ok("boot-time", "Boot time"));
        stale_ok.finding.measured_at = NOW - 3 * 3_600;
        stale_ok.out_of_date = true;
        stale_ok.running = false;
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
        let not_checked = row(
            "mem",
            "Memory Cleaner",
            Finding::not_checked("pressure", "Memory pressure", "First read still running")
                .measured_at(NOW),
        );
        let view = View {
            attention: vec![row("mem", "Memory Cleaner", watch("memory-pressure"))],
            checked: vec![
                not_checked,
                unavailable,
                stale_ok,
                row(
                    "disk",
                    "Disk Manager",
                    ok("drive-c", "Drive C: no warnings"),
                ),
            ],
            more: vec![],
        };
        let board = board(&view, &[], NOW, false, true);
        let quiet: Vec<(&str, &str, &str, &str)> = board
            .quiet
            .iter()
            .map(|line| {
                (
                    line.status.word(),
                    line.title.as_str(),
                    line.observation.as_str(),
                    line.meta.as_str(),
                )
            })
            .collect();
        assert_eq!(
            quiet,
            [
                (
                    "Not checked",
                    "Memory pressure",
                    "First read still running",
                    "Memory Cleaner"
                ),
                (
                    "Unavailable",
                    "Drive health of D:",
                    "Needs administrator rights",
                    "Disk Manager"
                ),
                (
                    "Out of date",
                    "Boot time",
                    "",
                    "checked 3 h ago · Event Watch is not running"
                ),
            ]
        );
        assert!(board.quiet.iter().all(|line| line.status.dot().is_none()));
        // The current OK row is in the fine card, not among the quiet rows.
        assert_eq!(board.fine.len(), 1);
        assert_eq!(board.fine[0].facts[0].label, "Drive C: no warnings");
        // Not everything else is fine: some rows have no current result.
        assert_eq!(board.fine_heading, "Checked and fine");
    }

    #[test]
    fn nothing_needs_attention_says_what_was_checked_and_claims_nothing_else() {
        let view = View {
            checked: vec![
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
        assert_eq!(board.focus, None);
        assert!(board.others.is_empty());
        assert_eq!(
            board.headline.as_deref(),
            Some("Nothing needs attention in what was checked.")
        );
        assert_eq!(board.caption, "Health, 2 checks, last result 2 min ago");
        assert!(board.footer);
        assert!(!format!("{board:?}").to_lowercase().contains("healthy"));
        assert_eq!(board.fine.len(), 2);
        // The waiting add-on's checks stay visible, as Not checked, one row per name.
        let words: Vec<(&str, &str, &str)> = board
            .quiet
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
            ]
        );
        // One "Open Event Watch", on its first row.
        assert_eq!(
            board.quiet[0].button.as_ref().unwrap().label,
            "Open Event Watch"
        );
        assert_eq!(board.quiet[1].button, None);
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
            board.headline.as_deref(),
            Some("No add-on has a current result yet.")
        );
        assert!(board.fine.is_empty());
        assert_eq!(board.quiet.len(), 2);
        assert_eq!(board.caption, "Health, 2 checks, last result 2 min ago");
        let nothing = View {
            checked: vec![row(
                "disk",
                "Disk Manager",
                Finding::not_checked("drive-c", "Drive C:", "").measured_at(NOW),
            )],
            ..Default::default()
        };
        assert_eq!(
            super::board(&nothing, &[], NOW, false, true).caption,
            "Health, 1 check, no result yet"
        );
    }

    #[test]
    fn nothing_reported_names_the_add_ons_that_will_and_offers_browse() {
        let board = board(&View::default(), &[], NOW, false, false);
        assert_eq!(board.caption, "Health");
        assert_eq!(
            board.headline.as_deref(),
            Some("No add-on has reported a check yet.")
        );
        assert!(board.nothing_reported && board.quiet.is_empty() && !board.can_check);
        assert_eq!(board.focus, None);
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
            board.headline.as_deref(),
            Some("No add-on has reported a check yet. Memory Cleaner, Disk Manager and Event Watch report checks here.")
        );
        assert_eq!(board.quiet.len(), 3);
        assert_eq!(board.quiet[0].meta, "Memory Cleaner has not reported yet");
        assert_eq!(board.quiet[2].meta, "Event Watch is not running yet");
        let one = super::board(&View::default(), &waiting[..1], NOW, true, true);
        assert_eq!(
            one.headline.as_deref(),
            Some("No add-on has reported a check yet. Memory Cleaner reports checks here.")
        );
        assert_eq!(one.caption, "Health. Checking…");
    }

    #[test]
    fn checking_replaces_the_freshness_in_the_caption() {
        let view = View {
            attention: vec![row("mem", "Memory Cleaner", watch("memory-pressure"))],
            ..Default::default()
        };
        let board = board(&view, &[], NOW, true, true);
        assert_eq!(board.caption, "Health, 1 check. Checking…");
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

    /// Runs one frame of `draw` in a `width` wide window with the given input events.
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
                row(
                    "disk",
                    "Disk Manager",
                    problem("drive-c")
                        .why("Data can be lost.")
                        .evidence(Evidence::bar(0.9, "90% worn", "10% left")),
                ),
                row(
                    "mem",
                    "Memory Cleaner",
                    watch("memory-pressure").evidence(Evidence::series(
                        vec![0.0, 0.5, 1.0, 0.0],
                        "10 min ago",
                        "now",
                    )),
                ),
            ],
            checked: vec![
                row(
                    "events",
                    "Event Watch",
                    Finding::not_checked("crashes", "Recent crashes", "")
                        .measured_at(NOW)
                        .open_page("Open Event Watch", "Crashes"),
                ),
                row("events", "Event Watch", ok("boot", "Boot time")),
            ],
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
            // Check again sits at the top right, then the three actions go down the page.
            assert!(rects[0].top() < rects[1].top(), "{width}: {rects:?}");
            assert!(rects[1].top() < rects[2].top(), "{width}: {rects:?}");
            assert!(rects[2].top() < rects[3].top(), "{width}: {rects:?}");
            assert!(
                rects[0].right() >= rects[1].right() - 1.0,
                "{width}: {rects:?}"
            );
            for rect in &rects {
                assert!(rect.right() <= width + 0.5, "{width}: {rects:?}");
            }
            if width < NARROW_ROW + 40.0 {
                // Narrow: the focus action sits under the text, at its left edge.
                assert!(rects[1].left() < width / 2.0, "{width}: {rects:?}");
                assert!(rects[2].left() < width / 2.0, "{width}: {rects:?}");
            } else {
                assert!(rects[1].right() > width - 60.0, "{width}: {rects:?}");
                assert!(rects[2].right() > width - 60.0, "{width}: {rects:?}");
            }
        }
    }
}
