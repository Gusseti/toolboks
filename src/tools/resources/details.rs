//! The Details page: per-core CPU, per-engine GPU, per-drive and per-adapter cards.

use eframe::egui::{self, Align, Layout, RichText, Vec2};

use super::history::History;
use super::overview::{
    cores_label, fmt_percent, fmt_rate, gpu_memory_label, nice_ceiling, DISK_FLOOR, GREY,
    NET_FLOOR, NONE, SCROLL_GUTTER, SKY, STEP_SECS,
};
use super::Resources;
use crate::format::{clock, rate, size_plain, uptime};
use crate::ui::{graph_card, GraphCard, Line, ACCENT, AMBER, GREEN};

const COLS: usize = 4;
const MINI_HEIGHT: f32 = 92.0;
const IO_HEIGHT: f32 = 120.0;
/// Gap between graph cards on this page.
const TIGHT: f32 = 4.0;
/// Engine types that always get a graph when the GPU reports them.
const MAIN_ENGINES: [&str; 5] = ["3D", "Copy", "Video Decode", "Video Encode", "Compute"];

/// Width of one grid cell for `cols` columns separated by `gap`, in `inner` total width.
pub(super) fn grid_cell_width(inner: f32, cols: usize, gap: f32) -> f32 {
    if cols == 0 {
        return inner.max(0.0);
    }
    ((inner - gap * (cols - 1) as f32) / cols as f32)
        .floor()
        .max(0.0)
}

/// Used share of a drive from 0 to 1; 0 when the size is unknown.
pub(super) fn drive_used_fraction(total: u64, free: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let used = total.saturating_sub(free);
    (used as f64 / total as f64).clamp(0.0, 1.0) as f32
}

/// Main engine types always show; others only once they have been busy.
pub(super) fn engine_visible(name: &str, history_max: f32) -> bool {
    MAIN_ENGINES.iter().any(|m| m.eq_ignore_ascii_case(name)) || history_max > 0.0
}

fn max_of(h: &History) -> f32 {
    h.values().iter().copied().fold(0.0, f32::max)
}

fn peak(a: &History, b: &History, floor: f32) -> f32 {
    nice_ceiling(max_of(a).max(max_of(b)), floor)
}

/// Lays `cells` out in rows of `COLS` equal columns.
fn grid<T>(ui: &mut egui::Ui, cells: &[T], mut cell: impl FnMut(&mut egui::Ui, &T)) {
    let w = grid_cell_width(ui.available_width(), COLS, TIGHT);
    for row in cells.chunks(COLS) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = TIGHT;
            for c in row {
                ui.allocate_ui_with_layout(
                    Vec2::new(w, 0.0),
                    Layout::top_down(Align::LEFT),
                    |ui| cell(ui, c),
                );
            }
        });
    }
}

/// One small per-core or per-engine graph card with its share in the bottom bar.
fn mini_card(ui: &mut egui::Ui, name: &str, h: &History, color: egui::Color32) {
    let line = Line {
        values: h.values(),
        color,
        fill: true,
        label: "Load",
        fmt: fmt_percent,
    };
    let stats = [(
        h.last().map_or(NONE.to_string(), |v| format!("{v:.0}%")),
        "",
    )];
    graph_card(
        ui,
        ui.available_width(),
        MINI_HEIGHT,
        &GraphCard {
            title: name,
            detail: "",
            lines: &[line],
            capacity: h.capacity(),
            max: 100.0,
            step_secs: STEP_SECS,
            stats: &stats,
            note: None,
        },
    );
}

/// A one-line section header: the title, the key numbers inline and the detail at the right.
fn section(ui: &mut egui::Ui, title: &str, detail: &str, stats: &[(String, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui.label(RichText::new(title).strong());
        for (value, label) in stats {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(value).strong());
                ui.label(RichText::new(*label).weak().small());
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(detail).weak()).truncate());
        });
    });
}

impl Resources {
    pub(super) fn details_ui(&self, ui: &mut egui::Ui) {
        let width = ui.available_width() - SCROLL_GUTTER;
        ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::LEFT), |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(TIGHT);
            self.cpu_details(ui);
            ui.add_space(6.0);
            self.gpu_details(ui);
            ui.add_space(6.0);
            self.disk_details(ui);
            ui.add_space(6.0);
            self.net_details(ui);
        });
    }

    fn cpu_details(&self, ui: &mut egui::Ui) {
        let name = self.info.as_ref().map_or("", |i| i.cpu_name.as_str());
        let s = self.last.as_ref();
        let mut stats = vec![
            (
                s.map_or(NONE.to_string(), |s| format!("{:.0}%", s.cpu)),
                "Usage",
            ),
            (s.map_or(NONE.to_string(), |s| clock(s.cpu_mhz)), "Speed"),
            (
                self.info.as_ref().map_or(NONE.to_string(), |i| {
                    cores_label(i.physical_cores, i.logical_cores)
                }),
                "Cores / threads",
            ),
        ];
        if let Some(c) = s.and_then(|s| s.counts.as_ref()) {
            stats.push((c.processes.to_string(), "Processes"));
            stats.push((c.threads.to_string(), "Threads"));
            stats.push((c.handles.to_string(), "Handles"));
        }
        stats.push((
            s.map_or(NONE.to_string(), |s| uptime(s.uptime_secs)),
            "Up time",
        ));
        section(ui, "CPU", name, &stats);

        let items: Vec<(usize, &History)> = self.cores.iter().enumerate().collect();
        grid(ui, &items, |ui, (i, h)| {
            mini_card(ui, &format!("Core {i}"), h, ACCENT);
        });
    }

    fn gpu_details(&self, ui: &mut egui::Ui) {
        let name = self.gpu.as_ref().map_or("", |g| g.name.as_str());
        let Some(gpu) = &self.gpu else {
            section(ui, "GPU", name, &[]);
            // Same rule as the Overview: only say so after a few samples without GPU data,
            // the first readings can take a few seconds on a cold start.
            let text = if self.gpu_missing >= super::overview::GPU_MISSING_AFTER {
                "No GPU found"
            } else {
                "Reading GPU..."
            };
            ui.label(RichText::new(text).weak());
            return;
        };

        let mut stats = vec![(gpu_memory_label(gpu.mem_used, gpu.mem_total), "Memory")];
        if let Some(t) = gpu.temp_c {
            stats.push((format!("{t:.0} °C"), "Temp"));
        }
        if let Some(p) = gpu.power_w {
            stats.push((format!("{p:.0} W"), "Power"));
        }
        if let Some(c) = gpu.clock_mhz {
            stats.push((format!("{c} MHz"), "Clock"));
        }
        if let Some(c) = gpu.mem_clock_mhz {
            stats.push((format!("{c} MHz"), "Memory clock"));
        }
        section(ui, "GPU", name, &stats);

        let mut engines: Vec<(&str, &History)> = self
            .engines
            .iter()
            .filter(|(n, s)| engine_visible(n, max_of(&s[0])))
            .map(|(n, s)| (n, &s[0]))
            .collect();
        let rank = |n: &str| {
            MAIN_ENGINES
                .iter()
                .position(|m| m.eq_ignore_ascii_case(n))
                .unwrap_or(MAIN_ENGINES.len())
        };
        engines.sort_by_key(|(n, _)| rank(n));
        grid(ui, &engines, |ui, (n, h)| {
            mini_card(ui, n, h, GREEN);
        });
    }

    fn disk_details(&self, ui: &mut egui::Ui) {
        section(ui, "Disk", "", &[]);
        let drives = self.last.as_ref().map_or(&[][..], |s| &s.drives[..]);
        if drives.is_empty() {
            ui.label(RichText::new("No drives found").weak());
            return;
        }
        for d in drives {
            let title = [d.mount.as_str(), d.label.as_str(), d.kind.as_str()]
                .iter()
                .filter(|p| !p.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("  ");
            let free = format!(
                "{} free of {} ({:.0}% used)",
                size_plain(d.free.min(d.total)),
                size_plain(d.total),
                drive_used_fraction(d.total, d.free) * 100.0
            );
            let stats = [(rate(d.read_bps), "Read"), (rate(d.write_bps), "Write")];
            if let Some([r, w]) = self.drives.get(&d.mount) {
                let lines = [
                    Line {
                        values: r.values(),
                        color: AMBER,
                        fill: true,
                        label: "Read",
                        fmt: fmt_rate,
                    },
                    Line {
                        values: w.values(),
                        color: GREY,
                        fill: false,
                        label: "Write",
                        fmt: fmt_rate,
                    },
                ];
                graph_card(
                    ui,
                    ui.available_width(),
                    IO_HEIGHT,
                    &GraphCard {
                        title: &title,
                        detail: &free,
                        lines: &lines,
                        capacity: r.capacity(),
                        max: peak(r, w, DISK_FLOOR),
                        step_secs: STEP_SECS,
                        stats: &stats,
                        note: None,
                    },
                );
            } else {
                section(ui, &title, &free, &stats);
            }
        }
    }

    fn net_details(&self, ui: &mut egui::Ui) {
        section(ui, "Network", "", &[]);
        let adapters = self.last.as_ref().map_or(&[][..], |s| &s.adapters[..]);
        if adapters.is_empty() {
            ui.label(RichText::new("No adapters found").weak());
            return;
        }
        for a in adapters {
            let total = format!(
                "{} down, {} up in total",
                size_plain(a.total_down),
                size_plain(a.total_up)
            );
            let stats = [(rate(a.down_bps), "Down"), (rate(a.up_bps), "Up")];
            if let Some([d, u]) = self.adapters.get(&a.name) {
                let lines = [
                    Line {
                        values: d.values(),
                        color: SKY,
                        fill: true,
                        label: "Down",
                        fmt: fmt_rate,
                    },
                    Line {
                        values: u.values(),
                        color: GREY,
                        fill: false,
                        label: "Up",
                        fmt: fmt_rate,
                    },
                ];
                graph_card(
                    ui,
                    ui.available_width(),
                    IO_HEIGHT,
                    &GraphCard {
                        title: &a.name,
                        detail: &total,
                        lines: &lines,
                        capacity: d.capacity(),
                        max: peak(d, u, NET_FLOOR),
                        step_secs: STEP_SECS,
                        stats: &stats,
                        note: None,
                    },
                );
            } else {
                section(ui, &a.name, &total, &stats);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_width_splits_inner_width() {
        assert_eq!(grid_cell_width(400.0, 4, 12.0), 91.0);
        assert_eq!(grid_cell_width(100.0, 1, 12.0), 100.0);
        assert_eq!(grid_cell_width(10.0, 4, 12.0), 0.0);
        assert_eq!(grid_cell_width(50.0, 0, 12.0), 50.0);
    }

    #[test]
    fn used_fraction_clamps() {
        assert_eq!(drive_used_fraction(0, 0), 0.0);
        assert_eq!(drive_used_fraction(100, 25), 0.75);
        assert_eq!(drive_used_fraction(100, 200), 0.0);
        assert_eq!(drive_used_fraction(100, 0), 1.0);
    }

    #[test]
    fn engine_visibility() {
        assert!(engine_visible("3D", 0.0));
        assert!(engine_visible("video decode", 0.0));
        assert!(!engine_visible("Security", 0.0));
        assert!(engine_visible("Security", 0.5));
    }
}
