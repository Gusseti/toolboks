mod stats;

use std::time::{Duration, Instant};

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use self::stats::{format_bytes, sort_and_filter, Sampler, SortKey};
use super::Tool;

const REFRESH: Duration = Duration::from_secs(2);
const ROW_HEIGHT: f32 = 18.0;

/// Live RAM usage and a per-process table. Read-only; nothing is stored anywhere.
pub struct Memory {
    /// `None` if the platform refused to provide system info.
    sampler: Option<Sampler>,
    last_refresh: Instant,
    sort_key: SortKey,
    descending: bool,
    filter: String,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            sampler: Sampler::new(),
            last_refresh: Instant::now(),
            sort_key: SortKey::WorkingSet,
            descending: true,
            filter: String::new(),
        }
    }

    /// Clicking the active column flips the order; a new column starts in its natural order.
    fn apply_sort(&mut self, key: SortKey) {
        if self.sort_key == key {
            self.descending = !self.descending;
        } else {
            self.sort_key = key;
            // Numbers read best largest-first, names and pids smallest-first.
            self.descending = matches!(key, SortKey::WorkingSet | SortKey::Private);
        }
    }
}

fn sort_arrow(active: bool, descending: bool) -> &'static str {
    match (active, descending) {
        (true, true) => " \u{25BC}",
        (true, false) => " \u{25B2}",
        _ => "",
    }
}

impl Tool for Memory {
    fn name(&self) -> &'static str {
        "Memory"
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.sampler.is_none() {
            ui.label("Memory information is not available on this system.");
            return;
        }

        if self.last_refresh.elapsed() >= REFRESH {
            if let Some(sampler) = self.sampler.as_mut() {
                sampler.refresh();
            }
            self.last_refresh = Instant::now();
        }
        ui.request_repaint_after(REFRESH);

        let Some(sampler) = self.sampler.as_ref() else {
            return;
        };
        let totals = sampler.totals;

        ui.add(egui::ProgressBar::new(totals.used_fraction()).text(format!(
            "{} used of {} ({:.0}%)",
            format_bytes(totals.used),
            format_bytes(totals.total),
            totals.used_fraction() * 100.0
        )));
        ui.label(format!("Available: {}", format_bytes(totals.available)));
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.filter);
            ui.label(format!("{} processes", sampler.processes.len()));
        });
        ui.add_space(4.0);

        let rows = sort_and_filter(
            &sampler.processes,
            self.sort_key,
            self.descending,
            &self.filter,
        );

        // Headers need `&mut self`, so remember which one was clicked and apply it after.
        let mut clicked: Option<SortKey> = None;
        let (sort_key, descending) = (self.sort_key, self.descending);

        TableBuilder::new(ui)
            .striped(true)
            .column(Column::remainder().at_least(120.0))
            .column(Column::exact(64.0))
            .column(Column::exact(110.0))
            .column(Column::exact(110.0))
            .header(ROW_HEIGHT + 6.0, |mut header| {
                for (label, key) in [
                    ("Name", SortKey::Name),
                    ("PID", SortKey::Pid),
                    ("Working set", SortKey::WorkingSet),
                    ("Private", SortKey::Private),
                ] {
                    header.col(|ui| {
                        let arrow = sort_arrow(sort_key == key, descending);
                        if ui
                            .button(egui::RichText::new(format!("{label}{arrow}")).strong())
                            .clicked()
                        {
                            clicked = Some(key);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let p = rows[row.index()];
                    row.col(|ui| {
                        ui.label(&p.name);
                    });
                    row.col(|ui| {
                        ui.label(p.pid.to_string());
                    });
                    row.col(|ui| {
                        ui.label(format_bytes(p.working_set));
                    });
                    row.col(|ui| {
                        ui.label(format_bytes(p.private));
                    });
                });
            });

        if let Some(key) = clicked {
            self.apply_sort(key);
        }
    }
}
