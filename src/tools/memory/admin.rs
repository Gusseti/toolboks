//! The "Advanced" block of the memory tool: the one disruptive action, an admin-only purge of
//! the low-priority cache. It runs on a worker thread so the window never freezes, only when
//! the user asks for it, and never automatically.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use eframe::egui;

use super::memlist::{self, MemListError, StandbyBreakdown};
use super::style::section;
use super::Memory;
use crate::format::size_plain;
use crate::{elevation, win};

/// STATUS_UNSUCCESSFUL, reported if a worker thread dies without answering.
const STATUS_UNSUCCESSFUL: i32 = 0xC000_0001_u32 as i32;

/// Shown above the button.
pub const PURGE_WARNING: &str = "Drops the cached files Windows keeps in spare memory and values \
    least. This can cause short stutters while apps and games read from disk again. It never \
    changes the memory promised to programs (commit), and Windows refills the cache by itself. \
    It only runs when you press the button.";

/// Why the button is off when the app is not elevated.
pub const NEEDS_ADMIN: &str = "Needs administrator rights, because it changes how Windows \
    manages memory for the whole PC.";

/// What a finished purge reported.
struct PurgeDone {
    result: Result<(), MemListError>,
    before: Option<StandbyBreakdown>,
    after: Option<StandbyBreakdown>,
}

/// A purge running on a worker thread.
pub struct PurgeJob {
    rx: Receiver<PurgeDone>,
}

fn failure_text(error: MemListError) -> String {
    match error {
        MemListError::NotElevated => "needs admin rights".to_string(),
        MemListError::Unavailable => "is not available on this version of Windows".to_string(),
        MemListError::Status(code) => format!("failed: Windows returned status {code:#010x}"),
    }
}

/// The result line: the cache before and after, or why it did not work.
fn purge_text(
    result: Result<(), MemListError>,
    before: Option<StandbyBreakdown>,
    after: Option<StandbyBreakdown>,
) -> String {
    match result {
        Ok(()) => match (before, after) {
            (Some(b), Some(a)) => format!(
                "Cache (standby) went from {} to {}. Commit is unchanged.",
                size_plain(b.total()),
                size_plain(a.total())
            ),
            _ => {
                "The low-priority cache was dropped. The cache size could not be read.".to_string()
            }
        },
        Err(error) => format!("Clearing the low-priority cache {}.", failure_text(error)),
    }
}

impl Memory {
    /// A purge, a "Trim selected" or an optimize run is in progress.
    pub(super) fn busy(&self) -> bool {
        self.purge.is_some()
            || self.run.is_some()
            || self.trim_job.is_some()
            || self.refresh_action_pending
    }

    /// Runs the purge on a worker thread; ignored while another job is running or when not
    /// elevated.
    fn start_purge(&mut self, ctx: &egui::Context) {
        if self.busy() || !self.elevated {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let before = memlist::standby_breakdown();
            let result = memlist::purge_low_priority_standby();
            let after = memlist::standby_breakdown();
            let _ = tx.send(PurgeDone {
                result,
                before,
                after,
            });
            wake.request_repaint();
        });
        self.purge = Some(PurgeJob { rx });
    }

    /// Picks up a finished purge. Runs every frame via `background`.
    pub(super) fn poll_purge(&mut self, ctx: &egui::Context) {
        let outcome = match self.purge.as_ref() {
            None => return,
            Some(job) => job.rx.try_recv(),
        };
        let done = match outcome {
            Ok(done) => done,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return;
            }
            Err(TryRecvError::Disconnected) => PurgeDone {
                result: Err(MemListError::Status(STATUS_UNSUCCESSFUL)),
                before: None,
                after: None,
            },
        };
        self.purge = None;
        self.last_purge = Some(format!(
            "{}  {}",
            win::local_time_hms(),
            purge_text(done.result, done.before, done.after)
        ));
        self.refresh_now(ctx);
    }

    pub(super) fn advanced_block(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        section(ui, "Advanced");
        ui.label(egui::RichText::new(PURGE_WARNING).weak().small());
        if self.purge.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Running\u{2026}");
            });
        }
        let cache = self
            .lists
            .map_or_else(|| "unknown".to_string(), |l| size_plain(l.standby));
        ui.label(egui::RichText::new(format!("Cache now: {cache}")).small());

        let button = ui
            .add_enabled(
                self.elevated && !self.busy(),
                egui::Button::new("Clear low-priority cache"),
            )
            .on_hover_text("Drops only the lowest-priority cached pages.");
        if button.clicked() {
            self.start_purge(&ctx);
        }
        if !self.elevated {
            ui.label(egui::RichText::new(NEEDS_ADMIN).weak().small());
            if ui.button("Restart as admin").clicked() {
                if elevation::relaunch_elevated() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.status = Some("Elevation was cancelled.".to_string());
                }
            }
        }
        if let Some(text) = &self.last_purge {
            ui.label(egui::RichText::new(text).small());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breakdown(total_mb: u64) -> StandbyBreakdown {
        let mut by_priority = [0u64; 8];
        by_priority[0] = total_mb * 1024 * 1024;
        StandbyBreakdown { by_priority }
    }

    #[test]
    fn purge_text_shows_cache_before_and_after() {
        let text = purge_text(Ok(()), Some(breakdown(4096)), Some(breakdown(900)));
        assert!(text.contains("from 4.0 GB to 900 MB"), "{text}");
        assert!(text.contains("Commit is unchanged"));
        assert!(!text.to_lowercase().contains("freed"));
    }

    #[test]
    fn purge_text_handles_unreadable_sizes() {
        assert!(purge_text(Ok(()), None, Some(breakdown(1))).contains("could not be read"));
    }

    #[test]
    fn purge_failures_name_the_cause() {
        assert!(purge_text(Err(MemListError::NotElevated), None, None).contains("admin"));
        assert!(
            purge_text(Err(MemListError::Status(-1073741823)), None, None).contains("0xc0000001")
        );
        assert!(failure_text(MemListError::Unavailable).contains("not available"));
    }

    #[test]
    fn warning_covers_stutters_and_commit() {
        assert!(PURGE_WARNING.contains("short stutters"));
        assert!(PURGE_WARNING.contains("commit"));
        assert!(NEEDS_ADMIN.contains("administrator"));
    }
}
