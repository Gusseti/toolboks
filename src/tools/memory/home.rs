//! The Home page of the memory tool: plain words, one big button.
//!
//! "Optimize memory" asks running apps to give back memory they are not actively using. It
//! closes nothing and needs no admin rights; with admin and the box ticked it also does the
//! gentlest cache cleanup. Apps and Windows skipped by the normal trim rules stay untouched.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};

use super::commit::{self, Level};
use super::groups::{Breakdown, Segment};
use super::memlist::{self, Command};
use super::report::{self, Snapshot};
use super::stats::ProcInfo;
use super::trim::{skip_reason, trim_process};
use super::{Memory, AUTO_CHECK};
use crate::elevation;
use crate::format::size_plain;
use crate::ui::{stacked_bar, toggle_switch, BarPart, ACCENT, AMBER, CARD_PAD, GAP, GREEN, RED};

const MB: u64 = 1024 * 1024;
pub const DEFAULT_MIN_APP_MB: u32 = 32;
/// Freeing less than this is reported as "already in good shape".
const NOTABLE_RELEASE: u64 = 50 * MB;
/// After a run, keep watching memory in use for this long, as Windows settles.
const SETTLE_FOR: Duration = Duration::from_secs(10);
/// How often the cheap RAM totals are re-read while settling.
const SETTLE_EVERY: Duration = Duration::from_millis(500);
/// The Home zones: the left column, the right inspector and the height of a legend row.
const LEFT_WIDTH: f32 = 150.0;
const RIGHT_WIDTH: f32 = 176.0;
const LEGEND_ROW: f32 = 22.0;
/// After an automatic run, wait at least this long before the next one.
const AUTO_OPT_COOLDOWN: Duration = Duration::from_secs(5 * 60);
pub const DEFAULT_AUTO_PERCENT: u32 = 85;

/// What an optimize run did.
pub struct Report {
    apps: usize,
    /// `Some(true)` if the cache cleanup ran and worked, `Some(false)` if it was tried and failed.
    deep: Option<bool>,
}

/// An optimize run in progress on a worker thread.
pub struct Optimizing {
    rx: Receiver<Report>,
    used_before: u64,
    /// Memory and per-process working sets when the run started.
    before: Snapshot,
    /// Started by the automatic rule rather than by the button.
    automatic: bool,
}

/// The last finished run, shown under the button.
pub struct Outcome {
    report: Report,
    used_before: u64,
    /// For the before/after report: the start of the run and the moment it finished.
    before: Snapshot,
    immediate: Snapshot,
    /// Memory in use now (while settling) or at the end of the watch.
    used_after: u64,
    /// The lowest memory in use seen since the run finished.
    lowest_in_use: u64,
    /// Keep watching until this moment; `None` once settled.
    settle_until: Option<Instant>,
    last_check: Instant,
    automatic: bool,
}

/// How much memory in use really dropped: `before` minus the lowest value seen afterwards.
fn freed_bytes(before: u64, lowest_in_use: u64) -> u64 {
    before.saturating_sub(lowest_in_use)
}

/// The commands of a deeper clean, in order: write out dirty pages first, then drop cache.
fn deep_commands() -> [Command; 2] {
    [
        Command::FlushModifiedList,
        Command::PurgeLowPriorityStandbyList,
    ]
}

/// Whether the automatic rule should start an optimize run now: memory in use is at or above
/// the chosen percentage and the last automatic run was long enough ago.
fn should_auto_optimize(used_percent: f32, threshold: u32, since_last: Option<Duration>) -> bool {
    used_percent >= threshold as f32 && since_last.is_none_or(|d| d >= AUTO_OPT_COOLDOWN)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Health {
    Good,
    Busy,
    Full,
}

fn health(used_percent: f32) -> Health {
    if used_percent < 70.0 {
        Health::Good
    } else if used_percent < 85.0 {
        Health::Busy
    } else {
        Health::Full
    }
}

impl Health {
    fn color(self) -> Color32 {
        match self {
            Health::Good => GREEN,
            Health::Busy => AMBER,
            Health::Full => RED,
        }
    }

    fn headline(self) -> &'static str {
        match self {
            Health::Good => "Your memory looks healthy",
            Health::Busy => "Memory is getting busy",
            Health::Full => "Memory is nearly full",
        }
    }
}

/// Which processes an optimize run trims: not Windows or the desktop, not this app, nothing the
/// user protected, and nothing smaller than `min_bytes` (too small to matter).
fn select_targets(
    procs: &[ProcInfo],
    own_pid: u32,
    exclusions: &HashSet<String>,
    min_bytes: u64,
) -> Vec<u32> {
    procs
        .iter()
        .filter(|p| p.working_set >= min_bytes)
        .filter(|p| skip_reason(p.pid, &p.name, own_pid, exclusions).is_none())
        .map(|p| p.pid)
        .collect()
}

fn run_optimize(targets: &[u32], deep: bool) -> Report {
    let mut apps = 0usize;
    for &pid in targets {
        // Apps that refuse (other users, protected) or just exited are skipped quietly.
        if let Ok(t) = trim_process(pid) {
            if t.before > t.after {
                apps += 1;
            }
        }
    }
    let deep = deep.then(|| {
        // Run every step even if one fails; all must work for a clean report.
        deep_commands()
            .into_iter()
            .map(|c| memlist::run(c).is_ok())
            .collect::<Vec<_>>()
            .into_iter()
            .all(|ok| ok)
    });
    Report { apps, deep }
}

/// Headline and detail lines for the result card.
fn summary(outcome: &Outcome) -> (String, String) {
    let r = &outcome.report;
    let freed = freed_bytes(outcome.used_before, outcome.lowest_in_use);
    let settling = outcome.settle_until.is_some();
    let (headline, mut detail) = if freed < NOTABLE_RELEASE && !settling {
        (
            "Your memory was already in good shape".to_string(),
            "There was not much to release. Nothing was closed.".to_string(),
        )
    } else {
        let headline = if settling {
            format!("Freed about {} so far", size_plain(freed))
        } else {
            format!("Freed about {}", size_plain(freed))
        };
        let mut detail = "Nothing was closed. Apps gave back what they were not using and load it again if they need it."
            .to_string();
        if r.apps > 0 {
            detail.push_str(&format!(
                " {} {} gave back memory they were not using.",
                r.apps,
                if r.apps == 1 { "app" } else { "apps" }
            ));
        }
        (headline, detail)
    };
    match r.deep {
        Some(true) => detail.push_str(" Cached memory was cleared too."),
        Some(false) => detail.push_str(" Clearing cached memory did not work this time."),
        None => {}
    }
    if outcome.automatic {
        return (format!("Automatic clean: {headline}"), detail);
    }
    (headline, detail)
}

/// The bar's parts: each segment's share of the memory in use. Empty when nothing is in use.
fn bar_parts(b: &Breakdown) -> Vec<BarPart> {
    if b.in_use == 0 {
        return Vec::new();
    }
    b.segments
        .iter()
        .map(|s| BarPart {
            fraction: s.bytes as f32 / b.in_use as f32,
            color: Color32::from_rgb(s.color[0], s.color[1], s.color[2]),
        })
        .collect()
}

/// The hover text of a segment, in plain words.
fn segment_tooltip(seg: &Segment) -> String {
    let size = size_plain(seg.bytes);
    match seg.count {
        0 => format!("{}: {}, {:.0}%", seg.name, size, seg.percent),
        1 => format!(
            "{}: 1 process, {}, {:.0}% of memory in use",
            seg.name, size, seg.percent
        ),
        n => format!(
            "{}: {} processes, {}, {:.0}% of memory in use",
            seg.name, n, size, seg.percent
        ),
    }
}

impl Memory {
    fn start_optimize(&mut self, ctx: &egui::Context, automatic: bool) {
        if self.busy() {
            return;
        }
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let min_bytes = u64::from(self.opt_min_mb) * MB;
        let targets = select_targets(
            &sampler.processes,
            std::process::id(),
            &self.exclusions,
            min_bytes,
        );
        let deep = self.elevated && self.opt_deep;
        let used_before = sampler.totals.used;
        let before = Snapshot::new(
            sampler.totals.used,
            sampler.totals.available,
            &sampler.processes,
        );

        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(run_optimize(&targets, deep));
            wake.request_repaint();
        });
        self.optimizing = Some(Optimizing {
            rx,
            used_before,
            before,
            automatic,
        });
        self.outcome = None;
    }

    /// The automatic rule: when memory in use passes the chosen percentage, optimize.
    /// Checked every few seconds in the background, also while the window is minimized.
    pub(super) fn auto_optimize_tick(&mut self, ctx: &egui::Context) {
        if !self.auto_opt_on {
            return;
        }
        ctx.request_repaint_after(AUTO_CHECK);
        if self.last_opt_check.elapsed() < AUTO_CHECK || self.busy() {
            return;
        }
        self.last_opt_check = Instant::now();

        let Some(sampler) = self.sampler.as_mut() else {
            return;
        };
        sampler.refresh_totals();
        let used_percent = sampler.totals.used_fraction() * 100.0;
        let since_last = self.last_auto_opt.map(|t| t.elapsed());
        if !should_auto_optimize(used_percent, self.auto_opt_percent, since_last) {
            return;
        }
        // The process list may be stale if the window was hidden, so refresh it first.
        self.refresh_now();
        self.last_auto_opt = Some(Instant::now());
        self.start_optimize(ctx, true);
    }

    /// Picks up a finished optimize run. Runs every frame, also while the window is hidden.
    pub(super) fn poll_optimize(&mut self, ctx: &egui::Context) {
        self.settle_tick(ctx);
        let outcome = match self.optimizing.as_ref() {
            None => return,
            Some(o) => o.rx.try_recv(),
        };
        let report = match outcome {
            Ok(report) => report,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => {
                self.optimizing = None;
                self.status = Some("Optimizing stopped unexpectedly.".to_string());
                return;
            }
        };
        let Some(running) = self.optimizing.take() else {
            return;
        };
        self.refresh_now();
        let used_after = self.sampler.as_ref().map_or(0, |s| s.totals.used);
        let immediate = self.snapshot_now();
        if running.automatic {
            // Flash the taskbar button so a minimized window still tells you.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        self.outcome = Some(Outcome {
            report,
            used_before: running.used_before,
            before: running.before,
            immediate,
            used_after,
            lowest_in_use: used_after,
            settle_until: Some(Instant::now() + SETTLE_FOR),
            last_check: Instant::now(),
            automatic: running.automatic,
        });
        ctx.request_repaint_after(SETTLE_EVERY);
    }

    /// After a run, re-read only the cheap RAM totals every ~500 ms for 10 s and remember the
    /// lowest memory in use, so the result shows the real drop as Windows settles.
    fn settle_tick(&mut self, ctx: &egui::Context) {
        let Some(outcome) = self.outcome.as_mut() else {
            return;
        };
        let Some(until) = outcome.settle_until else {
            return;
        };
        if outcome.last_check.elapsed() >= SETTLE_EVERY {
            outcome.last_check = Instant::now();
            if let Some(sampler) = self.sampler.as_mut() {
                sampler.refresh_totals();
                outcome.used_after = sampler.totals.used;
                outcome.lowest_in_use = outcome.lowest_in_use.min(sampler.totals.used);
            }
        }
        if Instant::now() >= until {
            outcome.settle_until = None;
            self.finish_report();
        }
        ctx.request_repaint_after(SETTLE_EVERY);
    }

    /// Memory and process working sets from the latest sample.
    fn snapshot_now(&self) -> Snapshot {
        self.sampler.as_ref().map_or_else(Snapshot::default, |s| {
            Snapshot::new(s.totals.used, s.totals.available, &s.processes)
        })
    }

    /// At the end of the settle watch: take the settled snapshot (one full refresh, no new
    /// thread), compare it with the start and the finish of the run and keep the report.
    fn finish_report(&mut self) {
        self.refresh_now();
        let settled = self.snapshot_now();
        let Some(outcome) = self.outcome.as_ref() else {
            return;
        };
        let r = report::compare(
            &outcome.before,
            &outcome.immediate,
            &settled,
            outcome.automatic,
        );
        report::push(&mut self.reports, r);
    }

    pub(super) fn home_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let side = egui::Frame::NONE.inner_margin(CARD_PAD);
        egui::Panel::left("memory_left")
            .resizable(false)
            .exact_size(LEFT_WIDTH)
            .frame(side)
            .show(ui, |ui| self.left_column(ui, &ctx));

        let inspector = egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .inner_margin(CARD_PAD);
        let panel = egui::Panel::right("memory_inspector")
            .resizable(false)
            .exact_size(RIGHT_WIDTH)
            .frame(inspector)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inspector(ui));
            });
        let rect = panel.response.rect;
        ui.painter().vline(
            rect.left(),
            rect.y_range(),
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        egui::CentralPanel::default().frame(side).show(ui, |ui| {
            self.hint(ui);
            self.memory_goes(ui);
        });
    }

    /// The left column: the percentage in use, the numbers behind it and the button.
    fn left_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let used_percent = totals.used_fraction() * 100.0;
        let state = health(used_percent);
        let width = ui.available_width();

        ui.label(RichText::new("Memory in use").weak());
        ui.label(
            RichText::new(format!("{used_percent:.0}%"))
                .size(26.0)
                .strong()
                .color(state.color()),
        );
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 6.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().widgets.inactive.bg_fill);
        let mut fill = rect;
        fill.set_width(width * totals.used_fraction().clamp(0.0, 1.0));
        painter.rect_filled(fill, 3.0, state.color());
        ui.add_space(2.0);
        ui.label(RichText::new(state.headline()).small());
        ui.add_space(6.0);

        let cached = self
            .lists
            .map_or_else(|| "-".to_string(), |l| size_plain(l.standby));
        for (label, value) in [
            ("In use", size_plain(totals.used)),
            ("Available", size_plain(totals.available)),
            ("Cached", cached),
            ("Total", size_plain(totals.total)),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(value);
                });
            });
        }

        ui.add_space(GAP);
        let label = if self.optimizing.is_some() {
            "Optimizing..."
        } else {
            "Optimize memory"
        };
        let button = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
            .fill(ACCENT)
            .min_size(Vec2::new(width, 28.0))
            .corner_radius(4.0);
        let response = ui.add_enabled(!self.busy(), button).on_hover_text(
            "Moves memory that apps are not using right now out of RAM. Windows does this by itself when it needs to. You may notice no difference.",
        );
        if response.clicked() {
            self.start_optimize(ctx, false);
        }
        self.commit_row(ui, width);
    }

    /// Commit charge: promised memory against the limit, amber/red near the limit.
    fn commit_row(&self, ui: &mut egui::Ui, width: f32) {
        let Some(c) = self.commit.filter(|c| c.limit > 0) else {
            return;
        };
        let color = match commit::level(&c) {
            Level::Calm => GREEN,
            Level::Warn => AMBER,
            Level::Hot => RED,
        };
        ui.add_space(GAP);
        let head = ui.horizontal(|ui| {
            ui.label(RichText::new("Commit").weak());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{:.0}%", commit::percent(&c))).color(color));
            });
        });
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 4.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, ui.visuals().widgets.inactive.bg_fill);
        let mut fill = rect;
        fill.set_width(width * (commit::percent(&c) / 100.0).clamp(0.0, 1.0));
        painter.rect_filled(fill, 2.0, color);
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!(
                "{} of {} promised",
                size_plain(c.total),
                size_plain(c.limit)
            ))
            .small(),
        );
        ui.label(RichText::new(commit::sentence(&c)).weak().small());
        head.response
            .on_hover_text(format!("Peak so far: {}", size_plain(c.peak)));
    }

    /// A one-time hint, hidden for the rest of the session once dismissed.
    fn hint(&mut self, ui: &mut egui::Ui) {
        if self.hint_dismissed {
            return;
        }
        egui::Frame::NONE
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(4.0)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Used memory is not a problem. Free memory is wasted memory. Look at Commit and the last run report if your PC feels slow.",
                    )
                    .small(),
                );
                if ui.small_button("Got it").clicked() {
                    self.hint_dismissed = true;
                }
            });
        ui.add_space(GAP);
    }

    /// The right inspector: the options, then the result of the last run.
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new("Options").strong());

        ui.horizontal(|ui| {
            let was_on = self.auto_opt_on;
            toggle_switch(ui, &mut self.auto_opt_on);
            ui.label("Clean automatically");
            if self.auto_opt_on && !was_on {
                // Switched on: allow an immediate run if memory is already high.
                self.last_auto_opt = None;
            }
        });
        ui.label(
            RichText::new(
                "Off by default. Cleaning on a timer is the least proven option and can make apps briefly slower.",
            )
            .weak()
            .small(),
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(50.0);
        ui.add_enabled_ui(self.auto_opt_on, |ui| {
            ui.label(RichText::new("Start when in use reaches").weak().small());
            ui.add(egui::Slider::new(&mut self.auto_opt_percent, 50..=95).suffix(" %"));
        })
        .response
        .on_hover_text(
            "Runs the same safe clean as the button, at most once every 5 minutes, \
             while this window is open or minimized.",
        );
        ui.label(RichText::new("Skip apps under N MB").weak().small());
        ui.add(
            egui::Slider::new(&mut self.opt_min_mb, 8..=512)
                .logarithmic(true)
                .suffix(" MB"),
        );
        if self.elevated {
            ui.checkbox(&mut self.opt_deep, "Also clear cached memory")
                .on_hover_text("A deeper clean that also clears cached memory.");
        } else {
            ui.label(
                RichText::new("Deeper clean needs permission.")
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

        if self.outcome.is_some() || self.status.is_some() {
            ui.add_space(GAP);
            ui.separator();
            ui.label(RichText::new("Last run").strong());
            self.result(ui);
        }
        self.history(ui);
    }

    /// The before/after report of the last settled run, with its two buttons.
    fn last_report(&mut self, ui: &mut egui::Ui) {
        let settled = self
            .outcome
            .as_ref()
            .is_some_and(|o| o.settle_until.is_none());
        let Some(r) = self.reports.back().filter(|_| settled) else {
            return;
        };
        let text = report::describe(r);
        let copy = report::copy_text(r);
        let offer_off = (r.automatic || r.is_small()) && self.auto_opt_on;
        ui.add_space(2.0);
        ui.label(RichText::new(text).small());
        ui.label(RichText::new(report::honest_note()).weak().small());
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Copy report").clicked() {
                ui.ctx().copy_text(copy);
            }
            if offer_off && ui.link("Turn off auto-clean").clicked() {
                self.auto_opt_on = false;
            }
        });
    }

    /// A collapsed list of the last runs: how long ago and a one-line result.
    fn history(&self, ui: &mut egui::Ui) {
        if self.reports.is_empty() {
            return;
        }
        ui.add_space(GAP);
        egui::CollapsingHeader::new(RichText::new("History").strong())
            .default_open(false)
            .show(ui, |ui| {
                for r in self.reports.iter().rev() {
                    let mins = r.finished.elapsed().as_secs() / 60;
                    let when = if mins == 0 {
                        "just now".to_string()
                    } else {
                        format!("{mins} min ago")
                    };
                    ui.label(RichText::new(when).weak().small());
                    ui.label(RichText::new(report::one_line(r)).small());
                }
            });
    }

    /// The result of the last optimize run in plain words, or the last status message.
    fn result(&mut self, ui: &mut egui::Ui) {
        if let Some(outcome) = &self.outcome {
            let (headline, detail) = summary(outcome);
            ui.label(RichText::new(headline).strong().color(GREEN));
            if outcome.settle_until.is_some() {
                ui.label(RichText::new("Still settling...").weak());
            }
            ui.label(RichText::new(detail).small());
            ui.label(
                RichText::new(format!(
                    "Memory in use: {} \u{2192} {}",
                    size_plain(outcome.used_before),
                    size_plain(outcome.used_after)
                ))
                .weak()
                .small(),
            );
        }
        self.last_report(ui);
        if let Some(status) = &self.status {
            ui.label(RichText::new(status).small());
        }
    }

    /// "Where your memory goes": one stacked bar plus a one-column legend, both with hover
    /// details.
    fn memory_goes(&self, ui: &mut egui::Ui) {
        let b = &self.breakdown;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Where your memory goes").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} in use", size_plain(b.in_use))).weak());
            });
        });
        if b.segments.is_empty() {
            ui.label(RichText::new("Collecting...").weak());
            return;
        }
        ui.add_space(4.0);
        let mut hovered = stacked_bar(ui, 22.0, &bar_parts(b));
        ui.add_space(6.0);

        let (swatch, size_w, pct_w, gap) = (10.0, 64.0, 36.0, 8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let row_w = ui.available_width();
                let name_w = (row_w - swatch - size_w - pct_w - 3.0 * gap).max(40.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, seg) in b.segments.iter().enumerate() {
                    let row = ui.allocate_ui_with_layout(
                        Vec2::new(row_w, LEGEND_ROW),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(swatch), Sense::hover());
                            let c = seg.color;
                            ui.painter()
                                .rect_filled(r, 3.0, Color32::from_rgb(c[0], c[1], c[2]));
                            cell(ui, name_w, Align::LEFT, |ui| {
                                ui.add(egui::Label::new(&seg.name).truncate().selectable(false));
                            });
                            cell(ui, size_w, Align::RIGHT, |ui| {
                                ui.label(RichText::new(size_plain(seg.bytes)).weak());
                            });
                            cell(ui, pct_w, Align::RIGHT, |ui| {
                                ui.label(format!("{:.0}%", seg.percent));
                            });
                        },
                    );
                    if ui.rect_contains_pointer(row.response.rect) {
                        hovered = Some(i);
                    }
                }
            });
        if let Some(seg) = hovered.and_then(|i| b.segments.get(i)) {
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::unique("memory_goes_tip"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| ui.label(segment_tooltip(seg)));
        }
    }
}

/// A fixed-width legend cell whose content is aligned left or right.
fn cell(ui: &mut egui::Ui, width: f32, align: Align, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, LEGEND_ROW),
        Layout::left_to_right(Align::Center).with_main_align(align),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, mb: u64) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.to_string(),
            working_set: mb * MB,
            private: 0,
            count: 1,
        }
    }

    /// A finished (settled) run that freed `freed_mb`.
    fn outcome(freed_mb: u64, apps: usize, deep: Option<bool>) -> Outcome {
        let before = 10 * 1024 * MB;
        Outcome {
            report: Report { apps, deep },
            used_before: before,
            before: Snapshot::default(),
            immediate: Snapshot::default(),
            used_after: before - freed_mb * MB,
            lowest_in_use: before - freed_mb * MB,
            settle_until: None,
            last_check: Instant::now(),
            automatic: false,
        }
    }

    fn seg(name: &str, mb: u64, count: usize, percent: f32) -> Segment {
        Segment {
            name: name.to_string(),
            bytes: mb * MB,
            count,
            percent,
            color: [1, 2, 3],
        }
    }

    #[test]
    fn segment_tooltip_wording() {
        assert_eq!(
            segment_tooltip(&seg("Chrome", 2048, 12, 25.0)),
            "Chrome: 12 processes, 2.0 GB, 25% of memory in use"
        );
        assert_eq!(
            segment_tooltip(&seg("Slack", 500, 1, 6.0)),
            "Slack: 1 process, 500 MB, 6% of memory in use"
        );
        assert_eq!(
            segment_tooltip(&seg("Windows and other", 1024, 0, 12.0)),
            "Windows and other: 1.0 GB, 12%"
        );
    }

    #[test]
    fn bar_parts_are_shares_of_memory_in_use() {
        let b = Breakdown {
            in_use: 1000 * MB,
            segments: vec![seg("A", 250, 2, 25.0), seg("B", 750, 0, 75.0)],
        };
        let parts = bar_parts(&b);
        assert_eq!(parts.len(), 2);
        assert!((parts[0].fraction - 0.25).abs() < 1e-6);
        assert!((parts[1].fraction - 0.75).abs() < 1e-6);
        assert_eq!(parts[0].color, Color32::from_rgb(1, 2, 3));
        assert!(bar_parts(&Breakdown::default()).is_empty());
    }

    #[test]
    fn auto_optimize_waits_for_the_threshold() {
        assert!(!should_auto_optimize(84.9, 85, None));
        assert!(should_auto_optimize(85.0, 85, None));
        assert!(should_auto_optimize(99.0, 50, None));
        assert!(!should_auto_optimize(49.0, 50, None));
    }

    #[test]
    fn auto_optimize_respects_the_cooldown() {
        let min = Duration::from_secs(60);
        assert!(!should_auto_optimize(95.0, 85, Some(4 * min)));
        assert!(should_auto_optimize(95.0, 85, Some(5 * min)));
        // Below the threshold the cooldown does not matter.
        assert!(!should_auto_optimize(60.0, 85, Some(60 * min)));
    }

    #[test]
    fn automatic_runs_are_labelled() {
        let mut o = outcome(900, 4, None);
        o.automatic = true;
        assert!(summary(&o).0.starts_with("Automatic clean: Freed about"));
    }

    #[test]
    fn health_thresholds() {
        assert_eq!(health(0.0), Health::Good);
        assert_eq!(health(69.9), Health::Good);
        assert_eq!(health(70.0), Health::Busy);
        assert_eq!(health(84.9), Health::Busy);
        assert_eq!(health(85.0), Health::Full);
        assert_eq!(health(100.0), Health::Full);
    }

    #[test]
    fn picks_only_big_unprotected_processes() {
        let procs = vec![
            proc(10, "chrome.exe", 800),
            proc(11, "tiny.exe", 5),
            proc(12, "explorer.exe", 500),
            proc(13, "System", 600),
            proc(14, "mine.exe", 400),
            proc(15, "game.exe", 900),
        ];
        let exclusions: HashSet<String> = ["game.exe".to_string()].into();
        let picked = select_targets(&procs, 14, &exclusions, 32 * MB);
        assert_eq!(picked, vec![10], "only chrome qualifies");
    }

    #[test]
    fn a_lower_minimum_includes_smaller_apps() {
        let procs = vec![proc(10, "a.exe", 40), proc(11, "b.exe", 10)];
        let none = HashSet::new();
        assert_eq!(select_targets(&procs, 1, &none, 32 * MB), vec![10]);
        assert_eq!(select_targets(&procs, 1, &none, 8 * MB), vec![10, 11]);
    }

    #[test]
    fn freed_bytes_is_the_measured_drop() {
        assert_eq!(freed_bytes(10 * MB, 4 * MB), 6 * MB);
        assert_eq!(freed_bytes(4 * MB, 10 * MB), 0, "growth is not negative");
    }

    #[test]
    fn deep_clean_flushes_before_purging() {
        assert_eq!(
            deep_commands(),
            [
                Command::FlushModifiedList,
                Command::PurgeLowPriorityStandbyList
            ]
        );
    }

    #[test]
    fn summary_in_plain_words() {
        let (headline, detail) = summary(&outcome(1200, 1, None));
        assert_eq!(headline, "Freed about 1.2 GB");
        assert!(detail.contains("Nothing was closed"));
        assert!(detail.contains("1 app gave back memory"));

        let (headline, detail) = summary(&outcome(850, 7, None));
        assert_eq!(headline, "Freed about 850 MB");
        assert!(detail.contains("7 apps gave back memory"));
    }

    #[test]
    fn summary_while_settling_says_so_far() {
        let mut o = outcome(300, 2, None);
        o.settle_until = Some(Instant::now() + SETTLE_FOR);
        assert_eq!(summary(&o).0, "Freed about 300 MB so far");
        // Even nothing yet is "so far" while the watch is running.
        let mut o = outcome(0, 2, None);
        o.settle_until = Some(Instant::now() + SETTLE_FOR);
        assert!(summary(&o).0.ends_with("so far"));
    }

    #[test]
    fn summary_when_there_was_little_to_do() {
        let (headline, detail) = summary(&outcome(10, 2, None));
        assert_eq!(headline, "Your memory was already in good shape");
        assert!(detail.contains("not much"));
    }

    #[test]
    fn summary_mentions_the_deeper_clean() {
        assert!(summary(&outcome(500, 3, Some(true)))
            .1
            .contains("Cached memory was cleared"));
        assert!(summary(&outcome(500, 3, Some(false)))
            .1
            .contains("did not work"));
        assert!(!summary(&outcome(500, 3, None)).1.contains("ached"));
    }

    #[test]
    fn a_run_with_no_targets_does_nothing() {
        let report = run_optimize(&[], false);
        assert_eq!((report.apps, report.deep), (0, None));
    }
}
