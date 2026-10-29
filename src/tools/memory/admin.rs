//! Admin-only parts of the memory tool: system-wide memory list commands (run on a worker
//! thread so the window never freezes), the auto-clean tick, and their two panels.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use eframe::egui;

use super::auto::{decide, Reason, Snapshot, STANDBY_RULE_FREE_LIMIT_PERCENT};
use super::memlist::{self, Command, Lists, MemListError};
use super::stats::format_bytes;
use super::{Memory, AUTO_CHECK};
use crate::{elevation, win};

/// STATUS_UNSUCCESSFUL, reported if a worker thread dies without answering.
const STATUS_UNSUCCESSFUL: i32 = 0xC000_0001_u32 as i32;

/// A memory list command running on a worker thread.
pub struct Job {
    rx: Receiver<Result<(), MemListError>>,
    command: Command,
    /// `Some` for an auto-clean run, `None` for one the user started.
    reason: Option<Reason>,
    before: Option<Lists>,
}

/// "free A → B, standby C → D", or a note when the lists cannot be read.
fn change_text(before: Option<Lists>, after: Option<Lists>) -> String {
    match (before, after) {
        (Some(b), Some(a)) => format!(
            "free {} \u{2192} {}, standby {} \u{2192} {}",
            format_bytes(b.free),
            format_bytes(a.free),
            format_bytes(b.standby),
            format_bytes(a.standby)
        ),
        _ => "memory list sizes are not available".to_string(),
    }
}

fn failure_text(error: MemListError) -> String {
    match error {
        MemListError::NotElevated => "needs admin rights".to_string(),
        MemListError::Unavailable => "is not available on this version of Windows".to_string(),
        MemListError::Status(code) => format!("failed: Windows returned status {code:#010x}"),
    }
}

impl Memory {
    /// A system-wide command or an optimize run is in progress.
    pub(super) fn busy(&self) -> bool {
        self.job.is_some() || self.optimizing.is_some() || self.trim_job.is_some()
    }

    /// Runs `command` on a worker thread; ignored while another job is running.
    fn start_job(&mut self, command: Command, reason: Option<Reason>, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(memlist::run(command));
            wake.request_repaint();
        });
        self.job = Some(Job {
            rx,
            command,
            reason,
            before: self.lists,
        });
    }

    /// Picks up a finished job. Runs every frame, also while the window is hidden.
    fn poll_job(&mut self, ctx: &egui::Context) {
        let outcome = match self.job.as_ref() {
            None => return,
            Some(job) => job.rx.try_recv(),
        };
        let result = match outcome {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => Err(MemListError::Status(STATUS_UNSUCCESSFUL)),
        };
        let Some(job) = self.job.take() else {
            return;
        };
        self.finish_job(job, result, ctx);
    }

    fn finish_job(&mut self, job: Job, result: Result<(), MemListError>, ctx: &egui::Context) {
        let title = job.command.title();
        let text = match result {
            Ok(()) => {
                self.refresh_now();
                if job.reason.is_some() {
                    // Flash the taskbar button so a minimized window still tells you.
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                format!("{title}: {}", change_text(job.before, self.lists))
            }
            Err(error) => {
                if job.reason.is_some()
                    && matches!(error, MemListError::NotElevated | MemListError::Unavailable)
                {
                    self.auto.rules.enabled = false;
                    format!("{title} {}. Auto-clean switched off.", failure_text(error))
                } else {
                    format!("{title} {}", failure_text(error))
                }
            }
        };

        match job.reason {
            Some(reason) => {
                // Also after a failure, so a persistent error is retried at the cooldown pace.
                self.auto.mark_run();
                self.auto.push_log(format!(
                    "{}  Auto ({}): {text}",
                    win::local_time_hms(),
                    reason.describe()
                ));
            }
            None => self.status = Some(text),
        }
    }

    /// Background work for the memory tool: finish jobs, then check the auto-clean rules.
    pub(super) fn background(&mut self, ctx: &egui::Context) {
        self.poll_job(ctx);
        self.poll_optimize(ctx);
        self.poll_trim(ctx);
        self.auto_optimize_tick(ctx);

        if !self.elevated || !self.auto.rules.enabled {
            return;
        }
        ctx.request_repaint_after(AUTO_CHECK);
        if self.last_auto_check.elapsed() < AUTO_CHECK || self.busy() {
            return;
        }
        self.last_auto_check = std::time::Instant::now();

        let Some(sampler) = self.sampler.as_mut() else {
            return;
        };
        sampler.refresh_totals();
        self.lists = memlist::query();
        let snapshot = Snapshot {
            total: sampler.totals.total,
            free: self.lists.map(|l| l.free),
            standby: self.lists.map(|l| l.standby),
        };
        if let Some(reason) = decide(&self.auto.rules, &snapshot, self.auto.since_last_run()) {
            self.start_job(Command::PurgeLowPriorityStandbyList, Some(reason), ctx);
        }
    }

    pub(super) fn system_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.scope(|ui| {
            if self.busy() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Running\u{2026}");
                });
            }

            if let Some(command) = self.pending_system {
                ui.group(|ui| {
                    ui.label(format!("{}? {}", command.title(), command.warning()));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.busy(), egui::Button::new("Run"))
                            .clicked()
                        {
                            self.start_job(command, None, &ctx);
                            self.pending_system = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending_system = None;
                        }
                    });
                });
            }

            if self.elevated {
                ui.weak(
                    "Windows counts cached (standby) memory as available, so a purge raises \
                     Free, not Available.",
                );
                for command in Command::ALL {
                    ui.vertical(|ui| {
                        if ui
                            .add_enabled(!self.busy(), egui::Button::new(command.title()))
                            .clicked()
                        {
                            self.pending_system = Some(command);
                        }
                        ui.label(egui::RichText::new(command.warning()).weak().small());
                    });
                }
            } else {
                ui.label("These actions need administrator rights.");
                if ui.button("Restart as admin").clicked() {
                    if elevation::relaunch_elevated() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        self.status = Some("Elevation was cancelled.".to_string());
                    }
                }
            }
        });
    }

    pub(super) fn auto_panel(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(50.0);
            if !self.elevated {
                ui.label(
                    "Auto-clean runs the low-priority standby purge, which needs administrator \
                     rights. Restart as admin from the System tab to use it.",
                );
                return;
            }

            let rules = &mut self.auto.rules;
            let mut switched_on = false;
            ui.horizontal(|ui| {
                if crate::ui::toggle_switch(ui, &mut rules.enabled).changed() {
                    switched_on = rules.enabled;
                }
                ui.label("Auto-clean");
            });
            ui.weak(
                "Only works while this window is open or minimized. Settings are not saved and \
                 reset when the app closes. It never runs when under 256 MiB of standby memory \
                 exists, since there is nothing to purge.",
            );

            ui.checkbox(
                &mut rules.big_standby,
                format!(
                    "Clean when cached memory is large and free memory is under \
                     {STANDBY_RULE_FREE_LIMIT_PERCENT:.0} %"
                ),
            );
            ui.add_enabled_ui(rules.big_standby, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Cached memory above");
                    ui.add(
                        egui::Slider::new(&mut rules.big_standby_gib, 0.5..=32.0)
                            .step_by(0.5)
                            .suffix(" GiB"),
                    );
                });
            });
            ui.checkbox(&mut rules.interval, "Also clean on a schedule");
            ui.add_enabled_ui(rules.interval, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Every");
                    ui.add(egui::Slider::new(&mut rules.interval_minutes, 1..=240).suffix(" min"));
                });
            });
            ui.horizontal(|ui| {
                ui.label("At most once every");
                ui.add(egui::Slider::new(&mut rules.cooldown_minutes, 1..=60).suffix(" min"));
            });

            if switched_on {
                // Count the cooldown and interval from the moment it was switched on.
                self.auto.mark_run();
            }

            ui.add_space(4.0);
            ui.label("Activity");
            if self.auto.log.is_empty() {
                ui.weak("Nothing yet.");
            }
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for entry in &self.auto.log {
                        ui.label(entry);
                    }
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(free: u64, standby: u64) -> Lists {
        Lists {
            free,
            modified: 0,
            standby,
        }
    }

    #[test]
    fn change_text_shows_free_and_standby_not_available() {
        let mib = 1024 * 1024;
        let text = change_text(
            Some(lists(100 * mib, 4096 * mib)),
            Some(lists(4000 * mib, 90 * mib)),
        );
        assert!(text.contains("free 100.0 MiB \u{2192} 3.91 GiB"), "{text}");
        assert!(
            text.contains("standby 4.00 GiB \u{2192} 90.0 MiB"),
            "{text}"
        );
        assert!(!text.contains("available"));
    }

    #[test]
    fn change_text_handles_unreadable_lists() {
        assert!(change_text(None, Some(lists(1, 1))).contains("not available"));
        assert!(change_text(Some(lists(1, 1)), None).contains("not available"));
    }

    #[test]
    fn failure_texts_name_the_cause() {
        assert!(failure_text(MemListError::NotElevated).contains("admin"));
        assert!(failure_text(MemListError::Status(-1073741823)).contains("0xc0000001"));
    }
}
