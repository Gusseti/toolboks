//! The Home page of the memory tool: a master list of what to look at on the left (Memory,
//! Promised to apps, Cache, Disk reads for memory) and a detail pane on the right with the
//! verdict, the graph, where the memory goes, the biggest apps, the last run and the automatic
//! switch.
//!
//! "Trim idle apps" runs the safe plan: it lowers the memory priority of apps you have not used
//! for a while and asks the biggest idle ones to move unused memory to the cache. It closes
//! nothing, needs no admin rights and never touches the app in front. The result is measured
//! and reported honestly (see `report`): what moved to cache and what came back.

use std::time::Instant;

use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::engine::per_hour_text;
use super::groups::Segment;
use super::growth::{GrowthInfo, Hint};
use super::memlist::Lists;
use super::pressure::{limits, Assessment, Verdict};
use super::report::{self, HonestReport, RunMeasurement, Trigger};
use super::series::{Range, Series};
use super::style::{panel, primary_button, section, section_with_note};
use super::{commit, scheduler, Memory};
use crate::elevation;
use crate::format::size_plain;
use crate::ui::{
    banner, card, graph_card, pill, row_rule, stacked_bar, toggle_switch, BarPart, GraphCard, Line,
    AMBER, BAR_DIVIDER, BORDER, DATA_MEMORY, GAP, GREEN, MUTED, SELECTED_BG, SPACE_L, SPACE_XS,
    TEXT,
};

/// The master list and the heights of its rows and of the graph.
const LIST_WIDTH: f32 = 290.0;
const ROW_HEIGHT: f32 = 58.0;
const SPARK_WIDTH: f32 = 48.0;
const VALUE_WIDTH: f32 = 52.0;
const GRAPH_HEIGHT: f32 = 176.0;
const TABLE_ROW: f32 = 26.0;
const BOTTOM_CARD: f32 = 112.0;
/// The part of a graph card above the plot and the bar below it (see `ui::graph_card`).
const GRAPH_TITLE: f32 = 22.0;
const GRAPH_BAR: f32 = 26.0;
/// The pressure line in percent: Windows is short of memory beyond this.
const PRESSURE_PERCENT: f32 = limits::COMMIT_PRESSURE_PERCENT as f32;
/// How many apps the "Biggest apps" table lists.
const BIGGEST_APPS: usize = 5;

/// The honest sub-line of the one action.
const TRIM_NOTE: &str = "Moves what idle apps are not using into cache. It does not free memory.";
const CACHE_NOTE: &str = "Cache is memory Windows keeps ready to reuse. It is not wasted.";

/// Shown under the "Clean automatically" switch.
const AUTO_DESCRIPTION: &str = "Off by default. When on, it only cleans after memory has been \
    under pressure for a minute, at most once an hour, and never touches the app you are using. \
    It keeps working while the window is minimized or hidden in the tray.";

/// Colors of the "Where your memory goes" bar. The names sit next to them in the legend.
const APPS_COLOR: Color32 = DATA_MEMORY;
const WINDOWS_COLOR: Color32 = Color32::from_rgb(0x8f, 0x7b, 0xd9);
const CACHE_COLOR: Color32 = Color32::from_rgb(0x4a, 0x4e, 0x5a);
const UNUSED_COLOR: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x33);

const GB: f32 = 1024.0 * 1024.0 * 1024.0;

/// What the master list can show on the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Memory,
    Commit,
    Cache,
    DiskReads,
}

impl Metric {
    const ALL: [Metric; 4] = [
        Metric::Memory,
        Metric::Commit,
        Metric::Cache,
        Metric::DiskReads,
    ];

    /// The name in the list and on the graph.
    fn name(self) -> &'static str {
        match self {
            Metric::Memory => "Memory",
            Metric::Commit => "Promised to apps",
            Metric::Cache => "Cache",
            Metric::DiskReads => "Disk reads for memory",
        }
    }

    fn graph_title(self) -> &'static str {
        match self {
            Metric::Memory => "Memory in use",
            other => other.name(),
        }
    }

    /// Percent graphs have the pressure line.
    fn is_percent(self) -> bool {
        matches!(self, Metric::Memory | Metric::Commit)
    }
}

/// A resource of the master list as it is drawn.
struct ListRow {
    metric: Metric,
    sub: String,
    value: String,
    color: Color32,
}

/// The headline and the plain sentence of the verdict. Reasons are the pressure model's own.
fn verdict_text(a: &Assessment, available: u64) -> (&'static str, String) {
    let reasons = a.reasons.join(". ");
    match a.verdict {
        Verdict::Healthy => (
            "Your memory looks healthy",
            format!(
                "{} is ready to use right now, and Windows has plenty of room to grow. Nothing \
                 needs cleaning.",
                size_plain(available)
            ),
        ),
        Verdict::Busy => (
            "Your memory is getting busy",
            if reasons.is_empty() {
                "Windows is still coping, but there is less room to spare.".to_string()
            } else {
                format!("{reasons}.")
            },
        ),
        Verdict::UnderPressure => (
            "Your memory is under pressure",
            if reasons.is_empty() {
                "Apps may slow down.".to_string()
            } else {
                format!("{reasons}. Apps may slow down.")
            },
        ),
    }
}

/// "31.6 of 48.9 GB": the unit is said once when both sizes share it.
fn of_text(part: u64, whole: u64) -> String {
    let (a, b) = (size_plain(part), size_plain(whole));
    match (a.split_once(' '), b.split_once(' ')) {
        (Some((n, unit_a)), Some((_, unit_b))) if unit_a == unit_b => format!("{n} of {b}"),
        _ => format!("{a} of {b}"),
    }
}

/// The value and the plain detail line of the disk reads row.
fn disk_reads_text(per_sec: Option<f64>) -> (String, &'static str) {
    match per_sec {
        None => ("-".to_string(), "Not available right now"),
        Some(v) => {
            let sub = if v >= limits::HARD_FAULTS_PER_SEC {
                "High, apps are waiting"
            } else if v >= limits::HARD_FAULTS_PER_SEC / 4.0 {
                "Rising, apps may wait"
            } else {
                "Low, apps are not waiting"
            };
            (format!("{v:.0} /s"), sub)
        }
    }
}

/// The plain detail line of the cache row.
fn cache_sub(verdict: Verdict) -> &'static str {
    if verdict == Verdict::Healthy {
        "Ready to reuse, not wasted"
    } else {
        "Small, Windows is short"
    }
}

/// One piece of the "Where your memory goes" bar.
#[derive(Clone, Debug, PartialEq)]
struct Part {
    name: &'static str,
    bytes: u64,
    color: Color32,
}

/// Apps, Windows, Cache and Unused. They add up to the installed memory: cache and unused are
/// what Windows can hand out, the rest is in use by apps (their working sets) and Windows.
fn composition(total: u64, used: u64, apps_ws: u64, lists: Option<Lists>) -> Vec<Part> {
    let apps = apps_ws.min(used);
    let (cache, modified) = lists.map_or((0, 0), |l| (l.standby, l.modified));
    let windows = used.saturating_sub(apps).saturating_sub(modified);
    let cache = cache.saturating_add(modified);
    let unused = total.saturating_sub(apps + windows + cache);
    let mut parts = vec![
        Part {
            name: "Apps",
            bytes: apps,
            color: APPS_COLOR,
        },
        Part {
            name: "Windows",
            bytes: windows,
            color: WINDOWS_COLOR,
        },
    ];
    if lists.is_some() {
        parts.push(Part {
            name: "Cache",
            bytes: cache,
            color: CACHE_COLOR,
        });
    }
    parts.push(Part {
        name: "Unused",
        bytes: unused,
        color: UNUSED_COLOR,
    });
    parts
}

/// A graph's top value: the biggest point with some headroom, never below `floor`.
fn nice_max(values: &[f32], floor: f32) -> f32 {
    (values.iter().copied().fold(0.0, f32::max) * 1.25).max(floor)
}

/// "ChatGPT" or "Claude  x3" for a group of several processes.
fn app_name(seg: &Segment) -> String {
    if seg.count > 1 {
        format!("{}  x{}", seg.name, seg.count)
    } else {
        seg.name.clone()
    }
}

/// The three columns of the last run card: before, right after and later.
fn run_columns(r: &HonestReport) -> [(&'static str, Option<RunMeasurement>); 3] {
    let m = &r.measurements;
    let later = match (m.at_120s, m.at_30s) {
        (Some(x), _) => ("2 minutes later", Some(x)),
        (None, Some(x)) => ("30 seconds later", Some(x)),
        (None, None) => ("2 minutes later", None),
    };
    [
        ("Before", Some(m.before)),
        ("Right after", Some(m.after)),
        later,
    ]
}

/// The hover text of a segment, in plain words, with its growth when known.
fn segment_tooltip(seg: &Segment, growth: Option<&GrowthInfo>) -> String {
    let size = size_plain(seg.bytes);
    let mut text = match seg.count {
        0 => format!("{}: {}, {:.0}%", seg.name, size, seg.percent),
        1 => format!(
            "{}: 1 process, {}, {:.0}% of memory in use",
            seg.name, size, seg.percent
        ),
        n => format!(
            "{}: {} processes, {}, {:.0}% of memory in use",
            seg.name, n, size, seg.percent
        ),
    };
    if let Some(g) = growth {
        text.push_str(&format!(
            "\nGrowth: {}. {}",
            per_hour_text(g.per_hour),
            g.hint.text()
        ));
    }
    text
}

/// The rule behind an automatic run: the scheduler's text when it has one.
fn why_it_ran(trigger: Trigger, note: Option<&str>) -> Option<String> {
    trigger.is_automatic().then(|| {
        format!(
            "Why it ran: {}",
            note.map_or_else(|| trigger.text().to_string(), str::to_string)
        )
    })
}

/// "just now", "4 minutes ago".
fn ago(mins: u64) -> String {
    match mins {
        0 => "just now".to_string(),
        1 => "1 minute ago".to_string(),
        n => format!("{n} minutes ago"),
    }
}

impl Memory {
    pub(super) fn home_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let edge = SPACE_L as i8;
        let gap = (GAP / 2.0) as i8;
        let margin = |left: i8, right: i8| egui::Margin {
            left,
            right,
            top: SPACE_L as i8,
            bottom: SPACE_L as i8,
        };
        egui::Panel::left("memory_left")
            .resizable(false)
            .exact_size(LIST_WIDTH)
            .frame(egui::Frame::NONE.inner_margin(margin(edge, gap)))
            .show(ui, |ui| self.master_list(ui));

        let advanced = self.advanced_open;
        let central = if advanced {
            egui::Frame::NONE
        } else {
            egui::Frame::NONE.inner_margin(margin(gap, edge))
        };
        egui::CentralPanel::default().frame(central).show(ui, |ui| {
            if advanced {
                self.advanced_ui(ui);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.detail(ui, &ctx));
        });
    }

    /// Takes the points the history is due, from the numbers of the latest refresh.
    pub(super) fn record_history(&mut self) {
        let Some(used) = self
            .sampler
            .as_ref()
            .filter(|s| s.totals.total > 0)
            .map(|s| s.totals.used_fraction() * 100.0)
        else {
            return;
        };
        let first = self.history.used_percent.last().is_none();
        let due = self.history.due(Instant::now(), first);
        if due == 0 {
            return;
        }
        let h = &mut self.history;
        let commit = self
            .commit
            .filter(|c| c.limit > 0)
            .map(|c| commit::percent(&c))
            .or_else(|| h.commit_percent.last())
            .unwrap_or(0.0);
        let cache = self
            .lists
            .map(|l| (l.standby + l.modified) as f32 / GB)
            .or_else(|| h.cache_gb.last())
            .unwrap_or(0.0);
        let reads = self.hard_faults.map_or(0.0, |f| f as f32);
        for _ in 0..due {
            h.used_percent.push(used);
            h.commit_percent.push(commit);
            h.cache_gb.push(cache);
            h.disk_reads.push(reads);
        }
    }

    // ---- the master list -------------------------------------------------------------------

    fn list_rows(&self) -> Vec<ListRow> {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let used_percent = totals.used_fraction() * 100.0;
        let verdict = self.assessment.verdict;
        let hot = |p: f32| {
            if p >= PRESSURE_PERCENT {
                AMBER
            } else {
                DATA_MEMORY
            }
        };
        let (reads, reads_sub) = disk_reads_text(self.hard_faults);
        let commit = self.commit.filter(|c| c.limit > 0);
        Metric::ALL
            .into_iter()
            .map(|metric| match metric {
                Metric::Memory => ListRow {
                    metric,
                    sub: format!("{} in use", of_text(totals.used, totals.total)),
                    value: format!("{used_percent:.0}%"),
                    color: hot(used_percent),
                },
                Metric::Commit => ListRow {
                    metric,
                    sub: commit.map_or_else(
                        || "Not available right now".to_string(),
                        |c| of_text(c.total, c.limit),
                    ),
                    value: commit.map_or_else(
                        || "-".to_string(),
                        |c| format!("{:.0}%", commit::percent(&c)),
                    ),
                    color: hot(commit.map_or(0.0, |c| commit::percent(&c))),
                },
                Metric::Cache => ListRow {
                    metric,
                    sub: cache_sub(verdict).to_string(),
                    value: self
                        .lists
                        .map_or_else(|| "-".to_string(), |l| size_plain(l.standby + l.modified)),
                    color: if verdict == Verdict::Healthy {
                        DATA_MEMORY
                    } else {
                        AMBER
                    },
                },
                Metric::DiskReads => ListRow {
                    metric,
                    sub: reads_sub.to_string(),
                    value: reads.clone(),
                    color: if self
                        .hard_faults
                        .is_some_and(|f| f >= limits::HARD_FAULTS_PER_SEC)
                    {
                        AMBER
                    } else {
                        DATA_MEMORY
                    },
                },
            })
            .collect()
    }

    fn series_of(&self, metric: Metric) -> &Series {
        match metric {
            Metric::Memory => &self.history.used_percent,
            Metric::Commit => &self.history.commit_percent,
            Metric::Cache => &self.history.cache_gb,
            Metric::DiskReads => &self.history.disk_reads,
        }
    }

    fn master_list(&mut self, ui: &mut egui::Ui) {
        let rows = self.list_rows();
        let mut picked = None;
        ui.spacing_mut().item_spacing.y = 2.0;
        for row in &rows {
            let selected = !self.advanced_open && self.selected == row.metric;
            let view = self.series_of(row.metric).view(Range::FiveMinutes);
            let max = match row.metric {
                Metric::Memory | Metric::Commit => 100.0,
                Metric::Cache => nice_max(&view.values, 1.0),
                Metric::DiskReads => nice_max(&view.values, 50.0),
            };
            let response = resource_row(ui, selected, row, &view.values, max);
            if row.metric == Metric::Commit {
                toolboks_addon_sdk::tutorial::mark(ui, "memory-commit", response.rect);
            }
            if response.clicked() {
                picked = Some(row.metric);
            }
        }
        if let Some(metric) = picked {
            self.selected = metric;
            self.advanced_open = false;
        }

        // Advanced stays one click away: it takes over the detail pane.
        let rest = (ui.available_height() - 32.0).max(GAP);
        ui.add_space(rest);
        let label = if self.advanced_open {
            "Back to the overview"
        } else {
            "Advanced options"
        };
        if disclosure_row(ui, self.advanced_open, label).clicked() {
            self.advanced_open = !self.advanced_open;
        }
    }

    // ---- the detail pane -------------------------------------------------------------------

    fn detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.spacing_mut().item_spacing.y = 10.0;
        toolboks_addon_sdk::tutorial::region(ui, "memory-summary", |ui| {
            self.verdict_row(ui, ctx);
            ui.add_space(SPACE_XS);
            self.range_row(ui);
            self.graph(ui);
        });
        self.advice(ui);
        toolboks_addon_sdk::tutorial::region(ui, "memory-apps", |ui| {
            self.composition_card(ui);
            ui.add_space(GAP + 2.0);
            self.biggest_apps(ui);
        });
        toolboks_addon_sdk::tutorial::region(ui, "memory-options", |ui| self.bottom_row(ui));
    }

    /// The dot, the verdict in plain words and the one action.
    fn verdict_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = self.sampler.as_ref().map_or(0, |s| s.totals.available);
        let (headline, sentence) = verdict_text(&self.assessment, available);
        let verdict = self.assessment.verdict;
        let action_w = 210.0;
        let text_w = (ui.available_width() - action_w - 40.0).max(180.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            let color = if verdict == Verdict::Healthy {
                GREEN
            } else {
                AMBER
            };
            ui.painter()
                .circle_filled(dot.center() + Vec2::new(0.0, 8.0), 5.0, color);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(headline).size(18.0).strong());
                ui.label(RichText::new(sentence).weak());
            });
            ui.vertical(|ui| {
                ui.set_width(action_w);
                ui.spacing_mut().item_spacing.y = 4.0;
                let busy = self.busy();
                let label = if self.run.is_some() {
                    "Trimming..."
                } else {
                    "Trim idle apps"
                };
                let hover = "Lowers the memory priority of apps you have not used for a while and asks them to move unused memory to the cache. Windows does this by itself when it needs to. You may notice no difference.";
                let response = if verdict == Verdict::UnderPressure {
                    primary_button(ui, !busy, label).on_hover_text(hover)
                } else {
                    ui.add_enabled(
                        !busy,
                        egui::Button::new(label).min_size(Vec2::new(ui.available_width(), 28.0)),
                    )
                    .on_hover_text(hover)
                };
                toolboks_addon_sdk::tutorial::mark(ui, "memory-optimize", response.rect);
                if response.clicked() {
                    self.start_run(ctx, Trigger::Manual);
                }
                ui.label(RichText::new(TRIM_NOTE).weak().small());
            });
        });
    }

    fn range_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.range.caption()).weak().small());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for range in Range::ALL.into_iter().rev() {
                    if pill(ui, range.label(), self.range == range).clicked() {
                        self.range = range;
                    }
                }
            });
        });
    }

    /// The graph of the selected resource, with the numbers of all four in its bar.
    fn graph(&self, ui: &mut egui::Ui) {
        let metric = self.selected;
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let commit = self.commit.filter(|c| c.limit > 0);
        let view = self.series_of(metric).view(self.range);
        let max = match metric {
            Metric::Memory | Metric::Commit => 100.0,
            Metric::Cache => nice_max(&view.values, 1.0),
            Metric::DiskReads => nice_max(&view.values, 50.0),
        };
        let (label, fmt): (&str, fn(f32) -> String) = match metric {
            Metric::Memory => ("In use", |v| format!("{v:.0}%")),
            Metric::Commit => ("Promised", |v| format!("{v:.0}%")),
            Metric::Cache => ("Cache", |v| format!("{v:.1} GB")),
            Metric::DiskReads => ("Disk reads", |v| format!("{v:.0} per second")),
        };
        let detail = match metric {
            Metric::Memory => of_text(totals.used, totals.total),
            Metric::Commit => commit.map_or_else(String::new, |c| of_text(c.total, c.limit)),
            Metric::Cache => self.lists.map_or_else(String::new, |l| {
                format!("{} ready to reuse", size_plain(l.standby + l.modified))
            }),
            Metric::DiskReads => self
                .hard_faults
                .map_or_else(String::new, |v| format!("{v:.0} per second")),
        };
        let stats = [
            (format!("{:.0}%", totals.used_fraction() * 100.0), "In use"),
            (size_plain(totals.available), "Ready to use"),
            (
                self.lists
                    .map_or_else(|| "-".to_string(), |l| size_plain(l.standby + l.modified)),
                "Cache",
            ),
            (
                commit.map_or_else(
                    || "-".to_string(),
                    |c| format!("{:.0}%", commit::percent(&c)),
                ),
                "Promised to apps",
            ),
        ];
        let line = Line {
            values: &view.values,
            color: DATA_MEMORY,
            fill: true,
            label,
            fmt,
        };
        let width = ui.available_width();
        let origin = ui.cursor().min;
        let title = metric.graph_title();
        graph_card(
            ui,
            width,
            GRAPH_HEIGHT,
            &GraphCard {
                title,
                detail: &detail,
                lines: &[line],
                capacity: view.capacity,
                max,
                step_secs: view.step_secs,
                stats: &stats,
                note: (view.values.len() < 2).then_some("Collecting..."),
            },
        );
        if metric.is_percent() {
            pressure_line(
                ui,
                Rect::from_min_size(origin, Vec2::new(width, GRAPH_HEIGHT)),
                title,
                &detail,
            );
        }
    }

    /// Advice from automatic mode (close some apps) while it applies.
    fn advice(&mut self, ui: &mut egui::Ui) {
        let Some(advice) = self.advice.clone().filter(|_| !self.advice_hidden) else {
            return;
        };
        if banner(ui, &advice) {
            self.advice_hidden = true;
        }
    }

    /// "Where your memory goes": one stacked bar of the installed memory and its legend.
    fn composition_card(&self, ui: &mut egui::Ui) {
        let Some(s) = self.sampler.as_ref().filter(|s| s.totals.total > 0) else {
            return;
        };
        let apps_ws: u64 = s.processes.iter().map(|p| p.working_set).sum();
        let parts = composition(s.totals.total, s.totals.used, apps_ws, self.lists);
        let total = s.totals.total as f32;
        let bar: Vec<BarPart> = parts
            .iter()
            .map(|p| BarPart {
                fraction: p.bytes as f32 / total,
                color: p.color,
            })
            .collect();
        panel(ui, 0.0, |ui| {
            section_with_note(ui, "Where your memory goes", CACHE_NOTE);
            ui.add_space(SPACE_XS);
            let hovered = stacked_bar(ui, 12.0, &bar);
            ui.add_space(SPACE_XS);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                for p in &parts {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (swatch, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                        ui.painter().rect_filled(swatch, 2.0, p.color);
                        ui.label(p.name);
                        ui.label(RichText::new(size_plain(p.bytes)).weak());
                    });
                }
            });
            if let Some(part) = hovered.and_then(|i| parts.get(i)) {
                let tip = match part.name {
                    "Apps" => "Memory held by the apps you run.",
                    "Windows" => "Memory held by Windows itself and its services.",
                    "Cache" => CACHE_NOTE,
                    _ => "Memory nothing is using right now.",
                };
                egui::Tooltip::always_open(
                    ui.ctx().clone(),
                    ui.layer_id(),
                    egui::Id::unique("memory_goes_tip"),
                    egui::PopupAnchor::Pointer,
                )
                .show(|ui| ui.label(format!("{}: {}. {tip}", part.name, size_plain(part.bytes))));
            }
        });
    }

    /// "Biggest apps": the largest app groups with how fast each grows.
    fn biggest_apps(&self, ui: &mut egui::Ui) {
        let now_ms = self.now_ms();
        let apps: Vec<&Segment> = self
            .breakdown
            .segments
            .iter()
            .filter(|s| s.key.is_some())
            .take(BIGGEST_APPS)
            .collect();
        panel(ui, 0.0, |ui| {
            section_with_note(
                ui,
                "Biggest apps",
                "Change shows how fast an app grows. A steady climb can mean a leak.",
            );
            if apps.is_empty() {
                ui.label(RichText::new("Collecting...").weak());
                return;
            }
            let row_w = ui.available_width();
            let (mem_w, change_w) = (80.0, 100.0);
            let tip_w = (row_w * 0.36).clamp(120.0, 240.0);
            let name_w = (row_w - mem_w - change_w - tip_w - 24.0).max(60.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let cells = |ui: &mut egui::Ui,
                         name: RichText,
                         mem: RichText,
                         change: RichText,
                         tip: RichText|
             -> egui::Response {
                let row = ui.allocate_ui_with_layout(
                    Vec2::new(row_w, TABLE_ROW),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let name = cell(ui, name_w, Align::LEFT, name);
                        cell(ui, mem_w, Align::RIGHT, mem);
                        cell(ui, change_w, Align::RIGHT, change);
                        cell(ui, tip_w, Align::LEFT, tip);
                        name
                    },
                );
                row_rule(ui.painter(), row.response.rect);
                row.inner
            };
            cells(
                ui,
                RichText::new("Name").weak().small(),
                RichText::new("Memory").weak().small(),
                RichText::new("Change per hour").weak().small(),
                RichText::new("Suggestion").weak().small(),
            );
            for seg in apps {
                let info = seg
                    .key
                    .as_deref()
                    .and_then(|key| self.growth.info(key, now_ms));
                let (change, change_color, tip) = match &info {
                    Some(g) => (
                        per_hour_text(g.per_hour),
                        if g.hint == Hint::Fine { MUTED } else { AMBER },
                        g.hint.text(),
                    ),
                    None => ("collecting".to_string(), MUTED, ""),
                };
                let name = cells(
                    ui,
                    RichText::new(app_name(seg)),
                    RichText::new(size_plain(seg.bytes)),
                    RichText::new(change).color(change_color),
                    RichText::new(tip).weak(),
                );
                name.on_hover_text(segment_tooltip(seg, info.as_ref()));
            }
        });
    }

    // ---- the last run and the automatic switch ---------------------------------------------

    fn bottom_row(&mut self, ui: &mut egui::Ui) {
        let total = ui.available_width();
        let gap = GAP + 2.0;
        let left = ((total - gap) * 0.6).floor();
        let right = total - gap - left;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            card(ui, left, BOTTOM_CARD, |ui| self.last_run_card(ui));
            card(ui, right, BOTTOM_CARD, |ui| self.auto_card(ui));
        });
    }

    fn last_run_card(&mut self, ui: &mut egui::Ui) {
        let running = self.run.is_some();
        let nothing = self.last_run.is_some_and(|r| r == Default::default());
        let report = self
            .reports
            .back()
            .filter(|_| !nothing && !running)
            .map(|r| {
                (
                    ago(r.finished.elapsed().as_secs() / 60),
                    r.automatic(),
                    run_columns(r),
                    r.sentences().join(" "),
                    why_it_ran(r.trigger, self.run_note.as_deref()),
                    r.copy_text(),
                    (r.automatic() || r.is_small()) && self.scheduler.is_enabled(),
                )
            });

        let Some((when, automatic, columns, sentence, why, copy, offer_off)) = report else {
            section(ui, "Last run");
            if running {
                ui.label(RichText::new("Working...").weak());
            } else if let Some(summary) = &self.last_run {
                ui.label(summary.text());
            } else {
                ui.label(RichText::new("No run yet this session.").weak());
            }
            if let Some(status) = &self.status {
                ui.label(RichText::new(status).small());
            }
            if !running && self.last_run.is_none() {
                ui.label(
                    RichText::new(
                        "After a run this shows how much moved to cache, what came back within \
                         two minutes, and whether apps had to read from disk again.",
                    )
                    .weak()
                    .small(),
                );
            }
            self.history_list(ui);
            return;
        };

        section_with_note(
            ui,
            &format!("Last run, {when}"),
            if automatic {
                "Automatic"
            } else {
                "Trim idle apps"
            },
        );
        if let Some(summary) = &self.last_run {
            ui.label(RichText::new(summary.text()).small());
        }
        ui.columns(3, |cols| {
            for (ui, (heading, reading)) in cols.iter_mut().zip(columns) {
                ui.label(RichText::new(heading).weak().small());
                match reading {
                    Some(m) => {
                        ui.label(format!(
                            "{} ready   {} cache",
                            size_plain(m.zero_free),
                            size_plain(m.standby + m.modified)
                        ));
                        let reads = m
                            .pages_input_per_sec
                            .map_or_else(|| "-".to_string(), |v| format!("{v:.0}"));
                        ui.label(
                            RichText::new(format!("{reads} disk reads per second"))
                                .weak()
                                .small(),
                        );
                    }
                    None => {
                        ui.label(RichText::new("Checking...").weak());
                    }
                }
            }
        });
        ui.label(RichText::new(sentence).weak().small());
        if let Some(why) = why {
            ui.label(RichText::new(why).weak().small());
        }
        if self.settling.is_some() {
            ui.label(RichText::new("Still checking...").weak().small());
        }
        ui.label(RichText::new(report::honest_note()).weak().small());
        if let Some(status) = &self.status {
            ui.label(RichText::new(status).small());
        }
        let can_restore = !self.undo.is_empty() && !self.busy();
        let mut restore = false;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Copy report").clicked() {
                ui.ctx().copy_text(copy);
            }
            if ui
                .add_enabled(
                    can_restore,
                    egui::Button::new(format!("Undo priority change ({})", self.undo.len()))
                        .small(),
                )
                .on_hover_text("Puts the memory priority of the apps the tool lowered back")
                .clicked()
            {
                restore = true;
            }
            if offer_off && ui.link("Turn off auto-clean").clicked() {
                self.scheduler.set_enabled(false);
            }
        });
        if restore {
            self.restore_priorities();
        }
        self.history_list(ui);
    }

    /// A collapsed list of the last runs: how long ago and a one-line result.
    fn history_list(&self, ui: &mut egui::Ui) {
        if self.reports.len() < 2 {
            return;
        }
        egui::CollapsingHeader::new(RichText::new("History").strong())
            .default_open(false)
            .show(ui, |ui| {
                for r in self.reports.iter().rev() {
                    let when = ago(r.finished.elapsed().as_secs() / 60);
                    ui.label(RichText::new(when).weak().small());
                    ui.label(RichText::new(r.one_line()).small());
                }
            });
    }

    fn auto_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            section(ui, "Clean automatically");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut on = self.scheduler.is_enabled();
                let response = toggle_switch(ui, &mut on);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Switch, true, on, "Clean automatically")
                });
                if response.changed() {
                    self.scheduler.set_enabled(on);
                }
            });
        });
        ui.label(RichText::new(AUTO_DESCRIPTION).weak().small());
        ui.label(
            RichText::new(format!("Status: {}", self.scheduler.status(self.now_ms()))).small(),
        );
        ui.label(
            RichText::new(scheduler::legacy_threshold_note())
                .weak()
                .small(),
        );
        if !self.elevated {
            ui.label(
                RichText::new("The cache purge under Advanced options needs permission.")
                    .weak()
                    .small(),
            );
            if ui.link("Restart with permission").clicked() {
                if elevation::relaunch_elevated() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.status = Some("The permission request was cancelled.".into());
                }
            }
        }
    }
}

/// One row of the master list: a small line, the name and one plain detail line, the value.
/// Selected is the lighter fill.
fn resource_row(
    ui: &mut egui::Ui,
    selected: bool,
    row: &ListRow,
    spark: &[f32],
    max: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            SELECTED_BG
        } else if response.hovered() {
            BAR_DIVIDER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                8.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let line = Rect::from_center_size(
            Pos2::new(rect.left() + 8.0 + SPARK_WIDTH / 2.0, rect.center().y),
            Vec2::new(SPARK_WIDTH, 30.0),
        );
        sparkline(ui, line, spark, max, row.color);

        let text_left = line.right() + 8.0;
        let value_left = rect.right() - 8.0 - VALUE_WIDTH;
        let text = Rect::from_min_max(
            Pos2::new(text_left, rect.top() + 10.0),
            Pos2::new(value_left - 6.0, rect.bottom() - 4.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(text), |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(
                egui::Label::new(RichText::new(row.metric.name()).strong())
                    .truncate()
                    .selectable(false),
            );
            ui.add(
                egui::Label::new(RichText::new(&row.sub).weak().small())
                    .truncate()
                    .selectable(false),
            );
        });
        let value = Rect::from_min_size(
            Pos2::new(value_left, rect.top()),
            Vec2::new(VALUE_WIDTH, ROW_HEIGHT),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(value)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&row.value).size(13.0).strong())
                        .selectable(false),
                );
            },
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
            format!("{}, {}, {}", row.metric.name(), row.value, row.sub),
        )
    });
    response.on_hover_text(&row.sub)
}

/// A small line of the newest points, scaled to `max`.
fn sparkline(ui: &egui::Ui, rect: Rect, values: &[f32], max: f32, color: Color32) {
    if values.len() < 2 {
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, BORDER));
        return;
    }
    let max = max.max(f32::EPSILON);
    let step = rect.width() / (values.len() - 1) as f32;
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let t = (v / max).clamp(0.0, 1.0);
            Pos2::new(
                rect.left() + i as f32 * step,
                rect.bottom() - t * rect.height(),
            )
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.5, color)));
}

/// The disclosure row at the bottom of the list.
fn disclosure_row(ui: &mut egui::Ui, open: bool, label: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if open {
            SELECTED_BG
        } else if response.hovered() {
            BAR_DIVIDER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 8.0, fill);
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                8.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let color = if open { TEXT } else { MUTED };
        let font = egui::TextStyle::Body.resolve(ui.style());
        // A drawn chevron: right when closed, down when open.
        let c = Pos2::new(rect.left() + 16.0, rect.center().y);
        let tips = if open {
            [
                c + Vec2::new(-4.0, -2.0),
                c + Vec2::new(0.0, 2.0),
                c + Vec2::new(4.0, -2.0),
            ]
        } else {
            [
                c + Vec2::new(-2.0, -4.0),
                c + Vec2::new(2.0, 0.0),
                c + Vec2::new(-2.0, 4.0),
            ]
        };
        ui.painter()
            .add(egui::Shape::line(tips.to_vec(), Stroke::new(1.5, color)));
        ui.painter().text(
            Pos2::new(rect.left() + 30.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            color,
        );
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, label));
    response
}

/// The dashed pressure line over a percent graph, and its label in the title row when there is
/// room. The geometry follows `ui::graph_card`.
fn pressure_line(ui: &egui::Ui, card: Rect, title: &str, detail: &str) {
    let inner = card.shrink(1.0);
    let data = Rect::from_min_max(
        Pos2::new(inner.left(), inner.top() + GRAPH_TITLE),
        Pos2::new(inner.right(), inner.bottom() - GRAPH_BAR),
    );
    let y = data.bottom() - (PRESSURE_PERCENT / 100.0) * (data.height() - 2.0) - 1.0;
    let painter = ui.painter_at(inner);
    painter.extend(egui::Shape::dashed_line(
        &[Pos2::new(data.left(), y), Pos2::new(data.right(), y)],
        Stroke::new(1.0, AMBER),
        5.0,
        4.0,
    ));
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width_of = |text: &str| {
        painter
            .layout_no_wrap(text.to_string(), font.clone(), MUTED)
            .size()
            .x
    };
    let label = format!("Pressure starts here ({PRESSURE_PERCENT:.0}%)");
    let right = inner.right() - 10.0 - width_of(detail) - 16.0;
    let left = right - width_of(&label);
    if left > inner.left() + 10.0 + width_of(title) + 12.0 {
        painter.text(
            Pos2::new(right, inner.top() + 5.0),
            egui::Align2::RIGHT_TOP,
            label,
            font,
            AMBER,
        );
    }
}

/// A fixed-width table cell whose text is aligned left or right and cut off with dots.
fn cell(ui: &mut egui::Ui, width: f32, align: Align, text: RichText) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, TABLE_ROW),
        Layout::left_to_right(Align::Center).with_main_align(align),
        |ui| {
            ui.set_width(width);
            ui.add(egui::Label::new(text).truncate().selectable(false))
        },
    )
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::memory::engine::NOTHING_TO_TRIM;
    use crate::tools::memory::report::RunMeasurements;

    const MB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MB;

    fn seg(name: &str, mb: u64, count: usize, percent: f32) -> Segment {
        Segment {
            name: name.to_string(),
            bytes: mb * MB,
            count,
            percent,
            color: [1, 2, 3],
            key: None,
        }
    }

    fn assessment(verdict: Verdict, reasons: &[&str]) -> Assessment {
        Assessment {
            verdict,
            reasons: reasons.iter().map(|r| r.to_string()).collect(),
        }
    }

    #[test]
    fn segment_tooltip_wording() {
        assert_eq!(
            segment_tooltip(&seg("Chrome", 2048, 12, 25.0), None),
            "Chrome: 12 processes, 2.0 GB, 25% of memory in use"
        );
        assert_eq!(
            segment_tooltip(&seg("Slack", 500, 1, 6.0), None),
            "Slack: 1 process, 500 MB, 6% of memory in use"
        );
        assert_eq!(
            segment_tooltip(&seg("Windows and other", 1024, 0, 12.0), None),
            "Windows and other: 1.0 GB, 12%"
        );
    }

    #[test]
    fn segment_tooltip_adds_growth_and_hint() {
        let info = GrowthInfo {
            bytes: 2048 * MB,
            per_hour: Some(300 * MB as i64),
            hint: Hint::Growing,
        };
        let tip = segment_tooltip(&seg("Chrome", 2048, 12, 25.0), Some(&info));
        assert!(tip.ends_with("\nGrowth: +300 MB/h. Growing. Restart it if things feel slow."));
    }

    #[test]
    fn app_names_show_the_process_count() {
        assert_eq!(app_name(&seg("ChatGPT", 10, 1, 1.0)), "ChatGPT");
        assert_eq!(app_name(&seg("Claude", 10, 3, 1.0)), "Claude  x3");
    }

    #[test]
    fn verdict_words_follow_the_assessment() {
        let (head, text) = verdict_text(&assessment(Verdict::Healthy, &[]), 14 * GIB);
        assert_eq!(head, "Your memory looks healthy");
        assert!(
            text.starts_with("14.0 GB is ready to use right now"),
            "{text}"
        );
        assert!(text.ends_with("Nothing needs cleaning."));

        let reasons = [
            "Only 1.4 GB is ready to use",
            "Windows reports that memory is low",
        ];
        let (head, text) = verdict_text(&assessment(Verdict::UnderPressure, &reasons), GIB);
        assert_eq!(head, "Your memory is under pressure");
        assert_eq!(
            text,
            "Only 1.4 GB is ready to use. Windows reports that memory is low. Apps may slow down."
        );
        let (head, text) = verdict_text(&assessment(Verdict::Busy, &reasons[..1]), GIB);
        assert_eq!(head, "Your memory is getting busy");
        assert_eq!(text, "Only 1.4 GB is ready to use.");
        assert_eq!(
            verdict_text(&assessment(Verdict::UnderPressure, &[]), GIB).1,
            "Apps may slow down."
        );
    }

    #[test]
    fn sizes_say_the_unit_once() {
        assert_eq!(of_text(31 * GIB + GIB / 2, 48 * GIB), "31.5 of 48.0 GB");
        assert_eq!(of_text(500 * MB, 2 * GIB), "500 MB of 2.0 GB");
    }

    #[test]
    fn disk_reads_are_described_plainly() {
        assert_eq!(disk_reads_text(None).0, "-");
        assert_eq!(
            disk_reads_text(Some(3.0)),
            ("3 /s".to_string(), "Low, apps are not waiting")
        );
        assert_eq!(disk_reads_text(Some(120.0)).1, "Rising, apps may wait");
        assert_eq!(disk_reads_text(Some(400.0)).1, "High, apps are waiting");
        assert_eq!(cache_sub(Verdict::Healthy), "Ready to reuse, not wasted");
        assert_eq!(cache_sub(Verdict::UnderPressure), "Small, Windows is short");
    }

    #[test]
    fn composition_adds_up_to_installed_memory() {
        let lists = Lists {
            free: GIB,
            modified: GIB / 2,
            standby: 13 * GIB,
        };
        let parts = composition(32 * GIB, 17 * GIB, 9 * GIB, Some(lists));
        let names: Vec<&str> = parts.iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Apps", "Windows", "Cache", "Unused"]);
        let sum: u64 = parts.iter().map(|p| p.bytes).sum();
        assert_eq!(sum, 32 * GIB);
        assert_eq!(parts[0].bytes, 9 * GIB);
        assert_eq!(parts[2].bytes, 13 * GIB + GIB / 2);
        // Without the list numbers there is no Cache part, and nothing is invented.
        let parts = composition(32 * GIB, 17 * GIB, 20 * GIB, None);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].bytes, 17 * GIB);
        assert_eq!(parts[1].bytes, 0);
        assert_eq!(parts[2].bytes, 15 * GIB);
    }

    #[test]
    fn graph_top_leaves_headroom() {
        assert_eq!(nice_max(&[], 50.0), 50.0);
        assert_eq!(nice_max(&[10.0, 80.0], 50.0), 100.0);
    }

    #[test]
    fn the_pressure_line_is_where_the_model_says() {
        assert_eq!(PRESSURE_PERCENT, 90.0);
    }

    #[test]
    fn automatic_runs_show_the_rule_that_started_them() {
        assert_eq!(why_it_ran(Trigger::Manual, Some("x")), None);
        assert_eq!(
            why_it_ran(
                Trigger::Pressure,
                Some("Memory had been under pressure for a minute: a")
            ),
            Some("Why it ran: Memory had been under pressure for a minute: a".to_string())
        );
        assert_eq!(
            why_it_ran(Trigger::Pressure, None).unwrap(),
            "Why it ran: Memory had been under pressure for a minute."
        );
    }

    fn reading(zero_free: u64, ws: u64) -> RunMeasurement {
        RunMeasurement {
            zero_free: zero_free * MB,
            standby: 0,
            modified: 0,
            commit_used: 1000 * MB,
            private_ws: ws * MB,
            pages_input_per_sec: None,
            page_faults_per_sec: None,
        }
    }

    #[test]
    fn report_sentences_use_cache_wording_and_never_freed() {
        let m = RunMeasurements {
            before: reading(100, 1000),
            after: reading(700, 400),
            at_30s: Some(reading(500, 600)),
            at_120s: None,
        };
        let r = HonestReport::compute(Trigger::Manual, 3, m);
        let text = r.sentences().join(" ");
        assert!(text.contains("600 MB moved to cache."), "{text}");
        assert!(
            text.contains("200 MB came back within 30 seconds"),
            "{text}"
        );
        assert!(text.contains("Commit stayed the same"), "{text}");
        let lower = format!("{} {}", text, r.one_line()).to_lowercase();
        assert!(!lower.contains("freed"));
        assert!(!lower.contains("out of app memory"));
    }

    #[test]
    fn last_run_columns_wait_for_the_later_reading() {
        let mut m = RunMeasurements {
            before: reading(100, 1000),
            after: reading(700, 400),
            at_30s: None,
            at_120s: None,
        };
        let r = HonestReport::compute(Trigger::Manual, 1, m);
        let cols = run_columns(&r);
        assert_eq!(cols[0].0, "Before");
        assert_eq!(cols[1].0, "Right after");
        assert_eq!(cols[2], ("2 minutes later", None));
        m.at_30s = Some(reading(500, 600));
        let cols = run_columns(&HonestReport::compute(Trigger::Manual, 1, m));
        assert_eq!(cols[2].0, "30 seconds later");
        m.at_120s = Some(reading(450, 700));
        let cols = run_columns(&HonestReport::compute(Trigger::Manual, 1, m));
        assert_eq!(cols[2].0, "2 minutes later");
        assert_eq!(cols[2].1, Some(reading(450, 700)));
    }

    #[test]
    fn run_ages_read_plainly() {
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(1), "1 minute ago");
        assert_eq!(ago(4), "4 minutes ago");
    }

    #[test]
    fn the_switch_description_explains_tray_and_limits() {
        assert!(AUTO_DESCRIPTION.contains("Off by default"));
        assert!(AUTO_DESCRIPTION.contains("once an hour"));
        assert!(AUTO_DESCRIPTION.contains("tray"));
    }

    #[test]
    fn the_honest_words_are_kept() {
        assert_eq!(
            TRIM_NOTE,
            "Moves what idle apps are not using into cache. It does not free memory."
        );
        assert_eq!(
            NOTHING_TO_TRIM,
            "Nothing to trim right now. Your idle apps are already small."
        );
    }
}
