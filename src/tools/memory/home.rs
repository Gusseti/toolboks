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

use super::memlist::{self, Command};
use super::stats::{format_size_plain, friendly_name, group_by_name, ProcInfo};
use super::trim::{skip_reason, trim_process};
use super::{Memory, AUTO_CHECK};
use crate::elevation;

const MB: u64 = 1024 * 1024;
pub const DEFAULT_MIN_APP_MB: u32 = 32;
/// Releasing less than this is reported as "already in good shape".
const NOTABLE_RELEASE: u64 = 50 * MB;
const TOP_APPS: usize = 5;
/// After an automatic run, wait at least this long before the next one.
const AUTO_OPT_COOLDOWN: Duration = Duration::from_secs(5 * 60);
pub const DEFAULT_AUTO_PERCENT: u32 = 85;

const GREEN: Color32 = Color32::from_rgb(52, 168, 107);
const AMBER: Color32 = Color32::from_rgb(232, 163, 23);
const RED: Color32 = Color32::from_rgb(214, 69, 65);

/// What an optimize run did.
pub struct Report {
    apps: usize,
    released: u64,
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
    used_after: u64,
    automatic: bool,
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
    let mut released = 0u64;
    let mut apps = 0usize;
    for &pid in targets {
        // Apps that refuse (other users, protected) or just exited are skipped quietly.
        if let Ok(t) = trim_process(pid) {
            if t.before > t.after {
                released += t.before - t.after;
                apps += 1;
            }
        }
    }
    let deep = deep.then(|| memlist::run(Command::PurgeLowPriorityStandbyList).is_ok());
    Report {
        apps,
        released,
        deep,
    }
}

/// Headline and detail lines for the result card.
fn summary(outcome: &Outcome) -> (String, String) {
    let r = &outcome.report;
    let (headline, mut detail) = if r.released < NOTABLE_RELEASE {
        (
            "Your memory was already in good shape".to_string(),
            "There was not much to release. Nothing was closed.".to_string(),
        )
    } else {
        (
            format!(
                "Released about {} from {} {}",
                format_size_plain(r.released),
                r.apps,
                if r.apps == 1 { "app" } else { "apps" }
            ),
            "Nothing was closed. Apps gave back what they were not using and load it again if they need it."
                .to_string(),
        )
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

/// A ring that fills with `fraction`, with a big number in the middle.
fn gauge(ui: &mut egui::Ui, fraction: f32, color: Color32, big: &str, small: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(190.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let (center, radius, width) = (rect.center(), 80.0, 14.0);

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
        center - Vec2::new(0.0, 10.0),
        Align2::CENTER_CENTER,
        big,
        FontId::proportional(42.0),
        ui.visuals().strong_text_color(),
    );
    painter.text(
        center + Vec2::new(0.0, 26.0),
        Align2::CENTER_CENTER,
        small,
        FontId::proportional(15.0),
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
            automatic: running.automatic,
        });
    }

    pub(super) fn home_ui(&mut self, ui: &mut egui::Ui) {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let used_percent = totals.used_fraction() * 100.0;
        let state = health(used_percent);
        let running = self.optimizing.is_some();

        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Memory Cleaner").size(28.0).strong());
            ui.add_space(14.0);
            gauge(
                ui,
                totals.used_fraction(),
                state.color(),
                &format!("{used_percent:.0}%"),
                "in use",
            );
            ui.add_space(10.0);
            ui.label(RichText::new(state.headline()).size(20.0).strong());
            ui.label(RichText::new(state.hint()).weak());
            ui.label(
                RichText::new(format!(
                    "{} of {} in use",
                    format_size_plain(totals.used),
                    format_size_plain(totals.total)
                ))
                .weak(),
            );
            ui.add_space(18.0);

            let label = if running {
                "Optimizing\u{2026}"
            } else {
                "Optimize memory"
            };
            let button = egui::Button::new(
                RichText::new(label)
                    .size(21.0)
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(Color32::from_rgb(38, 110, 214))
            .min_size(Vec2::new(300.0, 58.0))
            .corner_radius(29.0);
            if ui.add_enabled(!self.busy(), button).clicked() {
                self.start_optimize(&ui.ctx().clone(), false);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Safe: nothing is closed and nothing is deleted.").weak());
        });

        ui.add_space(16.0);
        ui.vertical_centered(|ui| self.result_card(ui));
        ui.add_space(14.0);
        ui.vertical_centered(|ui| self.auto_card(ui));
        ui.add_space(14.0);
        // Everything else stays out of the way until asked for.
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(500.0, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    egui::CollapsingHeader::new("More options").show(ui, |ui| self.options(ui));
                    egui::CollapsingHeader::new("What is using my memory?")
                        .show(ui, |ui| self.top_apps(ui));
                },
            );
        });
    }

    /// The result of the last optimize run, in plain words.
    fn result_card(&mut self, ui: &mut egui::Ui) {
        if let Some(outcome) = &self.outcome {
            let (headline, detail) = summary(outcome);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(RichText::new(headline).strong().color(GREEN));
                ui.label(detail);
                ui.label(
                    RichText::new(format!(
                        "Memory in use: {} \u{2192} {}",
                        format_size_plain(outcome.used_before),
                        format_size_plain(outcome.used_after)
                    ))
                    .weak(),
                );
            });
            ui.add_space(10.0);
        }
        if let Some(status) = &self.status {
            ui.label(status);
        }
    }

    /// The automatic switch: a tick box and a slider for when it starts.
    fn auto_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(520.0);
            let was_on = self.auto_opt_on;
            ui.checkbox(&mut self.auto_opt_on, "Clean automatically");
            if self.auto_opt_on && !was_on {
                // Switched on: allow an immediate run if memory is already high.
                self.last_auto_opt = None;
            }
            ui.add_enabled_ui(self.auto_opt_on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Start when memory in use reaches");
                    ui.add(egui::Slider::new(&mut self.auto_opt_percent, 50..=95).suffix(" %"));
                });
            });
            ui.label(
                RichText::new(
                    "Runs the same safe clean as the button, at most once every 5 minutes, \
                     while this window is open or minimized.",
                )
                .weak()
                .small(),
            );
        });
    }

    /// The few knobs behind the one button: a slider and tick boxes, nothing technical.
    fn options(&mut self, ui: &mut egui::Ui) {
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
    }

    fn top_apps(&self, ui: &mut egui::Ui) {
        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let mut apps = group_by_name(&sampler.processes);
        apps.sort_by_key(|a| std::cmp::Reverse(a.working_set));
        apps.truncate(TOP_APPS);
        let biggest = apps.first().map_or(1, |a| a.working_set.max(1));

        egui::Grid::new("top_apps")
            .num_columns(3)
            .spacing([14.0, 8.0])
            .show(ui, |ui| {
                for app in &apps {
                    ui.label(friendly_name(&app.name));
                    ui.add(
                        egui::ProgressBar::new(app.working_set as f32 / biggest as f32)
                            .desired_width(260.0),
                    );
                    ui.label(format_size_plain(app.working_set));
                    ui.end_row();
                }
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

    fn outcome(released_mb: u64, apps: usize, deep: Option<bool>) -> Outcome {
        Outcome {
            report: Report {
                apps,
                released: released_mb * MB,
                deep,
            },
            used_before: 10 * 1024 * MB,
            used_after: 9 * 1024 * MB,
            automatic: false,
        }
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
        assert!(summary(&o).0.starts_with("Automatic clean: Released about"));
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
    fn summary_in_plain_words() {
        let (headline, detail) = summary(&outcome(1200, 1, None));
        assert_eq!(headline, "Released about 1.2 GB from 1 app");
        assert!(detail.contains("Nothing was closed"));

        let (headline, _) = summary(&outcome(850, 7, None));
        assert_eq!(headline, "Released about 850 MB from 7 apps");
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
    fn a_run_with_no_targets_releases_nothing() {
        let report = run_optimize(&[], false);
        assert_eq!((report.apps, report.released, report.deep), (0, 0, None));
    }
}
