//! The Home page of the memory tool: plain words, one big button.
//!
//! "Optimize memory" asks running apps to give back memory they are not actively using. It
//! closes nothing and needs no admin rights; with admin and the box ticked it also does the
//! gentlest cache cleanup. Apps and Windows skipped by the normal trim rules stay untouched.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, RichText, Sense, Shape, Stroke, Vec2,
};

use super::groups::{Breakdown, Segment};
use super::memlist::{self, Command};
use super::stats::ProcInfo;
use super::trim::{skip_reason, trim_process};
use super::{Memory, AUTO_CHECK};
use crate::elevation;
use crate::format::size_plain;
use crate::ui::{card, stacked_bar, toggle_switch, BarPart, ACCENT, AMBER, GAP, GREEN, RED};

const MB: u64 = 1024 * 1024;
pub const DEFAULT_MIN_APP_MB: u32 = 32;
/// Freeing less than this is reported as "already in good shape".
const NOTABLE_RELEASE: u64 = 50 * MB;
/// After a run, keep watching memory in use for this long, as Windows settles.
const SETTLE_FOR: Duration = Duration::from_secs(10);
/// How often the cheap RAM totals are re-read while settling.
const SETTLE_EVERY: Duration = Duration::from_millis(500);
/// The Home grid: spacing, card padding and the heights of the top row.
const HERO_HEIGHT: f32 = 364.0;
const STAT_HEIGHT: f32 = 112.0;
const MAX_WIDTH: f32 = 880.0;
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
    /// Started by the automatic rule rather than by the button.
    automatic: bool,
}

/// The last finished run, shown under the button.
pub struct Outcome {
    report: Report,
    used_before: u64,
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

    fn hint(self) -> &'static str {
        match self {
            Health::Good => "There is plenty of room for your apps.",
            Health::Busy => "Optimizing can make some room.",
            Health::Full => "Optimizing now can help your computer feel faster.",
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

/// A ring that fills with `fraction`, with a big number in the middle.
fn gauge(ui: &mut egui::Ui, size: f32, fraction: f32, color: Color32, big: &str, small: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter_at(rect);
    let (center, radius, width) = (rect.center(), size * 0.42, size * 0.074);

    painter.circle_stroke(
        center,
        radius,
        Stroke::new(width, ui.visuals().widgets.inactive.bg_fill),
    );
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.005 {
        let steps = ((fraction * 120.0).ceil() as usize).max(2);
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = std::f32::consts::TAU * fraction;
        let points = (0..=steps)
            .map(|i| {
                let a = start + sweep * i as f32 / steps as f32;
                center + radius * Vec2::new(a.cos(), a.sin())
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(width, color)));
    }
    painter.text(
        center - Vec2::new(0.0, size * 0.05),
        Align2::CENTER_CENTER,
        big,
        FontId::proportional(size * 0.22),
        ui.visuals().strong_text_color(),
    );
    painter.text(
        center + Vec2::new(0.0, size * 0.14),
        Align2::CENTER_CENTER,
        small,
        FontId::proportional((size * 0.08).max(12.0)),
        ui.visuals().weak_text_color(),
    );
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

        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(run_optimize(&targets, deep));
            wake.request_repaint();
        });
        self.optimizing = Some(Optimizing {
            rx,
            used_before,
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
        if running.automatic {
            // Flash the taskbar button so a minimized window still tells you.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        self.outcome = Some(Outcome {
            report,
            used_before: running.used_before,
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
        }
        ctx.request_repaint_after(SETTLE_EVERY);
    }

    pub(super) fn home_ui(&mut self, ui: &mut egui::Ui) {
        // Leave a gutter on the right for the scrollbar that floats over the content.
        let width = (ui.available_width() - 14.0).min(MAX_WIDTH);
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                    ui.label(RichText::new("Memory Cleaner").size(24.0).strong());
                    self.bento(ui, width);
                },
            );
        });
    }

    /// The Home grid: a big hero card beside a stat card and the automatic switch, then full
    /// width cards for the result, the top apps and the options.
    fn bento(&mut self, ui: &mut egui::Ui, total: f32) {
        let left = ((total - GAP) * 7.0 / 12.0).floor();
        let right = total - GAP - left;
        let ctx = ui.ctx().clone();

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(left, 0.0), Layout::top_down(Align::LEFT), |ui| {
                card(ui, left, HERO_HEIGHT, |ui| self.hero(ui, &ctx))
            });
            ui.allocate_ui_with_layout(
                Vec2::new(right, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing.y = GAP;
                    card(ui, right, STAT_HEIGHT, |ui| self.stat(ui));
                    card(ui, right, HERO_HEIGHT - STAT_HEIGHT - GAP, |ui| {
                        self.auto_card(ui)
                    });
                },
            );
        });

        if self.outcome.is_some() || self.status.is_some() {
            card(ui, total, 0.0, |ui| self.result(ui));
        }
        card(ui, total, 0.0, |ui| self.memory_goes(ui));
        card(ui, total, 0.0, |ui| self.options_card(ui));
    }

    fn hero(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let used_percent = totals.used_fraction() * 100.0;
        let state = health(used_percent);
        let running = self.optimizing.is_some();

        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            gauge(
                ui,
                160.0,
                totals.used_fraction(),
                state.color(),
                &format!("{used_percent:.0}%"),
                "in use",
            );
            ui.add_space(6.0);
            ui.label(RichText::new(state.headline()).size(18.0).strong());
            ui.label(RichText::new(state.hint()).weak());
            ui.add_space(10.0);

            let label = if running {
                "Optimizing\u{2026}"
            } else {
                "Optimize memory"
            };
            let button = egui::Button::new(
                RichText::new(label)
                    .size(19.0)
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(ACCENT)
            .min_size(Vec2::new((ui.available_width() * 0.85).max(180.0), 52.0))
            .corner_radius(26.0);
            if ui.add_enabled(!self.busy(), button).clicked() {
                self.start_optimize(ctx, false);
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Safe: nothing is closed or deleted.")
                    .weak()
                    .small(),
            );
        });
    }

    fn stat(&self, ui: &mut egui::Ui) {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        ui.label(RichText::new("In use").weak());
        ui.label(RichText::new(size_plain(totals.used)).size(28.0).strong());
        ui.label(RichText::new(format!("of {}", size_plain(totals.total))).weak());
    }

    /// The automatic switch and the slider for when it starts.
    fn auto_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let was_on = self.auto_opt_on;
            toggle_switch(ui, &mut self.auto_opt_on);
            ui.label(RichText::new("Clean automatically").strong());
            if self.auto_opt_on && !was_on {
                // Switched on: allow an immediate run if memory is already high.
                self.last_auto_opt = None;
            }
        });
        ui.add_space(2.0);
        ui.add_enabled_ui(self.auto_opt_on, |ui| {
            ui.label(RichText::new("Start when memory in use reaches").weak());
            ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(60.0);
            ui.add(egui::Slider::new(&mut self.auto_opt_percent, 50..=95).suffix(" %"));
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                "Runs the same safe clean as the button, at most once every 5 minutes, \
                 while this window is open or minimized.",
            )
            .weak()
            .small(),
        );
    }

    /// The result of the last optimize run in plain words, or the last status message.
    fn result(&self, ui: &mut egui::Ui) {
        if let Some(outcome) = &self.outcome {
            let (headline, detail) = summary(outcome);
            ui.label(RichText::new(headline).strong().color(GREEN));
            if outcome.settle_until.is_some() {
                ui.label(RichText::new("Still settling...").weak());
            }
            ui.label(detail);
            ui.label(
                RichText::new(format!(
                    "Memory in use: {} \u{2192} {}",
                    size_plain(outcome.used_before),
                    size_plain(outcome.used_after)
                ))
                .weak(),
            );
        }
        if let Some(status) = &self.status {
            ui.label(status);
        }
    }

    /// "Where your memory goes": one stacked bar plus a legend, both with hover details.
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
        let mut hovered = stacked_bar(ui, 34.0, &bar_parts(b));
        ui.add_space(6.0);

        let half = (ui.available_width() - 24.0) / 2.0;
        let (swatch, pct_w) = (12.0, 36.0);
        let name_w = (half - swatch - 64.0 - pct_w - 3.0 * 8.0).max(40.0);
        // Two columns filled top to bottom (largest first down the left column), so reading
        // down a column goes in size order.
        let n = b.segments.len();
        let rows = n.div_ceil(2);
        let mut cell = |ui: &mut egui::Ui, i: usize| {
            let seg = &b.segments[i];
            let row = ui.horizontal(|ui| {
                ui.set_width(half);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(swatch), Sense::hover());
                let c = seg.color;
                ui.painter()
                    .rect_filled(r, 4.0, Color32::from_rgb(c[0], c[1], c[2]));
                // A fixed-width, left-aligned name (add_sized would centre it).
                ui.allocate_ui_with_layout(
                    Vec2::new(name_w, 18.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(&seg.name).truncate().selectable(false));
                    },
                );
                ui.label(RichText::new(size_plain(seg.bytes)).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!("{:.0}%", seg.percent));
                });
            });
            if ui.rect_contains_pointer(row.response.rect) {
                hovered = Some(i);
            }
        };
        egui::Grid::new("memory_legend")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for r in 0..rows {
                    for col in 0..2 {
                        let i = r + col * rows;
                        if i < n {
                            cell(ui, i);
                        } else {
                            ui.label("");
                        }
                    }
                    ui.end_row();
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

    /// The few knobs behind the one button: a slider and tick boxes, nothing technical.
    fn options_card(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("More options").strong()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Skip apps using less than");
                ui.add(
                    egui::Slider::new(&mut self.opt_min_mb, 8..=512)
                        .logarithmic(true)
                        .suffix(" MB"),
                );
            });
            if self.elevated {
                ui.checkbox(
                    &mut self.opt_deep,
                    "Also clear cached memory (a deeper clean)",
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.weak("Want a deeper clean that also clears cached memory?");
                    if ui.link("Restart with permission").clicked() {
                        if elevation::relaunch_elevated() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        } else {
                            self.status = Some("The permission request was cancelled.".into());
                        }
                    }
                });
            }
            ui.weak("More controls are on the Advanced page.");
        });
    }
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
