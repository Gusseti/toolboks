//! The Home page of the memory tool: plain words, one big button.
//!
//! "Optimize memory" runs the safe plan: it lowers the memory priority of apps you have not used
//! for a while and asks the biggest idle ones to move unused memory to the cache. It closes
//! nothing, needs no admin rights and never touches the app in front. The result is measured
//! and reported honestly (see `report`): what moved to cache and what came back.

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Vec2};

use super::commit::{self, Level};
use super::engine::{per_hour_text, verdict_line};
use super::groups::{Breakdown, Segment};
use super::growth::{GrowthInfo, Hint};
use super::report::{self, Trigger};
use super::style::{banner, meter, panel, primary_button, section, section_with_note, stat_row};
use super::{scheduler, Memory};
use crate::elevation;
use crate::format::size_plain;
use crate::ui::{
    stacked_bar, toggle_switch, BarPart, AMBER, GAP, GREEN, RED, SPACE_L, SPACE_XS, TEXT_SECTION,
};

/// The Home zones: the left column, the right inspector and the height of a legend row.
const LEFT_WIDTH: f32 = 220.0;
const RIGHT_WIDTH: f32 = 250.0;
const LEGEND_ROW: f32 = 22.0;

/// Shown under the "Clean automatically" switch.
const AUTO_DESCRIPTION: &str = "Off by default. Cleans only when Windows has really been short \
    of memory for about a minute, at most once an hour. It keeps working while the window is \
    minimized or hidden in the tray.";

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

impl Memory {
    pub(super) fn home_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let margin = |left: i8, right: i8| egui::Margin {
            left,
            right,
            top: SPACE_L as i8,
            bottom: SPACE_L as i8,
        };
        let gap = (GAP / 2.0) as i8;
        let edge = SPACE_L as i8;
        egui::Panel::left("memory_left")
            .resizable(false)
            .exact_size(LEFT_WIDTH)
            .frame(egui::Frame::NONE.inner_margin(margin(edge, gap)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        toolboks_addon_sdk::tutorial::region(ui, "memory-summary", |ui| {
                            self.left_column(ui, &ctx)
                        })
                    });
            });

        egui::Panel::right("memory_inspector")
            .resizable(false)
            .exact_size(RIGHT_WIDTH)
            .frame(egui::Frame::NONE.inner_margin(margin(gap, edge)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        toolboks_addon_sdk::tutorial::region(ui, "memory-options", |ui| {
                            self.inspector(ui)
                        })
                    });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(margin(gap, gap)))
            .show(ui, |ui| {
                self.hint(ui);
                toolboks_addon_sdk::tutorial::region(ui, "memory-apps", |ui| self.memory_goes(ui));
            });
    }

    /// The left column: memory in use, the numbers behind it, the button and the commit card.
    fn left_column(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let totals = self.sampler.as_ref().map(|s| s.totals).unwrap_or_default();
        let used_percent = totals.used_fraction() * 100.0;
        let state = health(used_percent);

        panel(ui, 0.0, |ui| {
            ui.horizontal(|ui| {
                section(ui, "Memory in use");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{used_percent:.0}%"))
                            .size(TEXT_SECTION)
                            .strong()
                            .color(state.color()),
                    );
                });
            });
            meter(ui, 6.0, totals.used_fraction(), state.color());
            ui.label(RichText::new(state.headline()).small());
            if let Some(line) = verdict_line(&self.assessment) {
                ui.label(RichText::new(line).weak().small());
            }
            ui.add_space(SPACE_XS);

            let cached = self
                .lists
                .map_or_else(|| "-".to_string(), |l| size_plain(l.standby));
            stat_row(ui, "In use", size_plain(totals.used));
            stat_row(ui, "Available", size_plain(totals.available));
            stat_row(ui, "Cached", cached);
            stat_row(ui, "Total", size_plain(totals.total));

            ui.add_space(GAP);
            let label = if self.run.is_some() {
                "Optimizing..."
            } else {
                "Optimize memory"
            };
            let response = primary_button(ui, !self.busy(), label).on_hover_text(
                "Lowers the memory priority of apps you have not used for a while and asks them to move unused memory to the cache. Windows does this by itself when it needs to. You may notice no difference.",
            );
            toolboks_addon_sdk::tutorial::mark(ui, "memory-optimize", response.rect);
            if response.clicked() {
                self.start_run(ctx, Trigger::Manual);
            }
        });
        toolboks_addon_sdk::tutorial::region(ui, "memory-commit", |ui| self.commit_row(ui));
    }

    /// Commit charge: promised memory against the limit, amber/red near the limit.
    fn commit_row(&self, ui: &mut egui::Ui) {
        let Some(c) = self.commit.filter(|c| c.limit > 0) else {
            return;
        };
        let color = match commit::level(&c) {
            Level::Calm => GREEN,
            Level::Warn => AMBER,
            Level::Hot => RED,
        };
        ui.add_space(GAP);
        panel(ui, 0.0, |ui| {
            let head = ui.horizontal(|ui| {
                section(ui, "Commit");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("{:.0}%", commit::percent(&c))).color(color));
                });
            });
            meter(ui, 4.0, commit::percent(&c) / 100.0, color);
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
        });
    }

    /// Advice from automatic mode (close some apps) while it applies, then a one-time hint that
    /// stays hidden for the rest of the session once dismissed.
    fn hint(&mut self, ui: &mut egui::Ui) {
        if let Some(advice) = self.advice.clone().filter(|_| !self.advice_hidden) {
            if banner(ui, &advice) {
                self.advice_hidden = true;
            }
            ui.add_space(GAP);
        }
        if self.hint_dismissed {
            return;
        }
        if banner(
            ui,
            "Used memory is not a problem. Free memory is wasted memory. Look at Commit and the last run report if your PC feels slow.",
        ) {
            self.hint_dismissed = true;
        }
        ui.add_space(GAP);
    }

    /// The right column: the options card, then the result of the last run.
    fn inspector(&mut self, ui: &mut egui::Ui) {
        panel(ui, 0.0, |ui| {
            section(ui, "Options");

            ui.horizontal(|ui| {
                let mut on = self.scheduler.is_enabled();
                if toggle_switch(ui, &mut on).changed() {
                    self.scheduler.set_enabled(on);
                }
                ui.label("Clean automatically");
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
                    RichText::new("The cache purge under Advanced needs permission.")
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
        });

        if self.last_run.is_some() || self.status.is_some() || !self.reports.is_empty() {
            ui.add_space(GAP);
            panel(ui, 0.0, |ui| {
                section(ui, "Last run");
                self.result(ui);
            });
        }
        self.history(ui);
    }

    /// The honest report of the last run, with its buttons.
    fn last_report(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.reports.back() else {
            return;
        };
        let text = r.sentences().join(" ");
        let why = why_it_ran(r.trigger, self.run_note.as_deref());
        let copy = r.copy_text();
        let offer_off = (r.automatic() || r.is_small()) && self.scheduler.is_enabled();
        ui.add_space(2.0);
        ui.label(RichText::new(text).small());
        if let Some(why) = why {
            ui.label(RichText::new(why).weak().small());
        }
        if self.settling.is_some() {
            ui.label(RichText::new("Still checking...").weak().small());
        }
        ui.label(RichText::new(report::honest_note()).weak().small());
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
    }

    /// A collapsed list of the last runs: how long ago and a one-line result.
    fn history(&self, ui: &mut egui::Ui) {
        if self.reports.is_empty() {
            return;
        }
        ui.add_space(GAP);
        panel(ui, 0.0, |ui| {
            egui::CollapsingHeader::new(RichText::new("History").size(TEXT_SECTION).strong())
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
                        ui.label(RichText::new(r.one_line()).small());
                    }
                });
        });
    }

    /// What the last run did and its honest report, or the last status message.
    fn result(&mut self, ui: &mut egui::Ui) {
        if self.run.is_some() {
            ui.label(RichText::new("Working...").weak());
        }
        if let Some(summary) = &self.last_run {
            ui.label(RichText::new(summary.text()).small());
        }
        self.last_report(ui);
        if let Some(status) = &self.status {
            ui.label(RichText::new(status).small());
        }
    }

    /// "Where your memory goes": one stacked bar plus a one-column legend, both with hover
    /// details, and each app's growth per hour.
    fn memory_goes(&self, ui: &mut egui::Ui) {
        let b = &self.breakdown;
        let now_ms = self.now_ms();
        let growth_of = |seg: &Segment| {
            seg.key
                .as_deref()
                .and_then(|key| self.growth.info(key, now_ms))
        };
        let height = ui.available_height();
        let mut hovered = None;
        panel(ui, height, |ui| {
            section_with_note(
                ui,
                "Where your memory goes",
                &format!("{} in use", size_plain(b.in_use)),
            );
            if b.segments.is_empty() {
                ui.label(RichText::new("Collecting...").weak());
                return;
            }
            ui.add_space(SPACE_XS);
            hovered = stacked_bar(ui, 22.0, &bar_parts(b));
            ui.add_space(GAP);

            let (swatch, size_w, pct_w, growth_w, gap) = (10.0, 64.0, 36.0, 76.0, 8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let row_w = ui.available_width();
                    let name_w = (row_w - swatch - size_w - pct_w - growth_w - 4.0 * gap).max(40.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, seg) in b.segments.iter().enumerate() {
                        let info = growth_of(seg);
                        let row = ui.allocate_ui_with_layout(
                            Vec2::new(row_w, LEGEND_ROW),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                let (r, _) =
                                    ui.allocate_exact_size(Vec2::splat(swatch), Sense::hover());
                                let c = seg.color;
                                ui.painter().rect_filled(
                                    r,
                                    3.0,
                                    Color32::from_rgb(c[0], c[1], c[2]),
                                );
                                cell(ui, name_w, Align::LEFT, |ui| {
                                    ui.add(
                                        egui::Label::new(&seg.name).truncate().selectable(false),
                                    );
                                });
                                cell(ui, growth_w, Align::RIGHT, |ui| {
                                    if let Some(g) = &info {
                                        let text = RichText::new(per_hour_text(g.per_hour));
                                        let text = if g.hint == Hint::Fine {
                                            text.weak()
                                        } else {
                                            text.color(AMBER)
                                        };
                                        ui.add(egui::Label::new(text).selectable(false));
                                    }
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
        });
        if let Some(seg) = hovered.and_then(|i| b.segments.get(i)) {
            let tip = segment_tooltip(seg, growth_of(seg).as_ref());
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::unique("memory_goes_tip"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| ui.label(tip));
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
    use crate::tools::memory::report::{HonestReport, RunMeasurement, RunMeasurements};

    const MB: u64 = 1024 * 1024;

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
    fn health_thresholds() {
        assert_eq!(health(0.0), Health::Good);
        assert_eq!(health(69.9), Health::Good);
        assert_eq!(health(70.0), Health::Busy);
        assert_eq!(health(84.9), Health::Busy);
        assert_eq!(health(85.0), Health::Full);
        assert_eq!(health(100.0), Health::Full);
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
    fn the_switch_description_explains_tray_and_limits() {
        assert!(AUTO_DESCRIPTION.contains("Off by default"));
        assert!(AUTO_DESCRIPTION.contains("once an hour"));
        assert!(AUTO_DESCRIPTION.contains("tray"));
    }
}
