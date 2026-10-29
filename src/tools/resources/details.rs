//! The Details page: per-core CPU, per-engine GPU, per-drive and per-adapter cards.

use eframe::egui::{self, Align, Layout, RichText, Vec2};

use super::history::History;
use super::overview::{
    cores_label, gpu_memory_label, header, nice_ceiling, DISK_FLOOR, GREY, MAX_WIDTH, NET_FLOOR,
    NONE, SKY,
};
use super::Resources;
use crate::format::{clock, rate, size_plain, uptime};
use crate::ui::{card, graph, stat, Line, ACCENT, AMBER, GAP, GREEN};

const COLS: usize = 4;
const MINI_HEIGHT: f32 = 36.0;
const IO_HEIGHT: f32 = 40.0;
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
    let w = grid_cell_width(ui.available_width(), COLS, GAP);
    for row in cells.chunks(COLS) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
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

fn mini_label(ui: &mut egui::Ui, name: &str, value: Option<f32>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).small());
        if let Some(v) = value {
            ui.label(RichText::new(format!("{v:.0}%")).weak().small());
        }
    });
}

impl Resources {
    pub(super) fn details_ui(&self, ui: &mut egui::Ui) {
        let width = (ui.available_width() - 14.0).min(MAX_WIDTH);
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                    ui.label(RichText::new("Resource Monitor").size(24.0).strong());
                    card(ui, width, 0.0, |ui| self.cpu_details(ui));
                    card(ui, width, 0.0, |ui| self.gpu_details(ui));
                    card(ui, width, 0.0, |ui| self.disk_details(ui));
                    card(ui, width, 0.0, |ui| self.net_details(ui));
                },
            );
        });
    }

    fn cpu_details(&self, ui: &mut egui::Ui) {
        let name = self.info.as_ref().map_or("", |i| i.cpu_name.as_str());
        header(ui, "CPU", name);
        let s = self.last.as_ref();
        let usage = s.map(|s| format!("{:.0}%", s.cpu));
        let speed = s.map(|s| clock(s.cpu_mhz));
        let up = s.map(|s| uptime(s.uptime_secs));
        let cores = self
            .info
            .as_ref()
            .map(|i| cores_label(i.physical_cores, i.logical_cores));
        let counts = s.and_then(|s| s.counts.as_ref());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, usage.as_deref().unwrap_or(NONE), "Usage");
            stat(ui, speed.as_deref().unwrap_or(NONE), "Speed");
            stat(ui, cores.as_deref().unwrap_or(NONE), "Cores / threads");
            if let Some(c) = counts {
                stat(ui, &c.processes.to_string(), "Processes");
                stat(ui, &c.threads.to_string(), "Threads");
                stat(ui, &c.handles.to_string(), "Handles");
            }
            stat(ui, up.as_deref().unwrap_or(NONE), "Up time");
        });

        let items: Vec<(usize, &History)> = self.cores.iter().enumerate().collect();
        grid(ui, &items, |ui, (i, h)| {
            mini_label(ui, &format!("Core {i}"), h.last());
            let line = Line {
                values: h.values(),
                color: ACCENT,
                fill: true,
            };
            graph(ui, MINI_HEIGHT, &[line], h.capacity(), 100.0);
        });
    }

    fn gpu_details(&self, ui: &mut egui::Ui) {
        let name = self.gpu.as_ref().map_or("", |g| g.name.as_str());
        header(ui, "GPU", name);
        let Some(gpu) = &self.gpu else {
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
            mini_label(ui, n, h.last());
            let line = Line {
                values: h.values(),
                color: GREEN,
                fill: true,
            };
            graph(ui, MINI_HEIGHT, &[line], h.capacity(), 100.0);
        });

        let mem = gpu_memory_label(gpu.mem_used, gpu.mem_total);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, &mem, "Memory");
            if let Some(t) = gpu.temp_c {
                stat(ui, &format!("{t:.0} °C"), "Temp");
            }
            if let Some(p) = gpu.power_w {
                stat(ui, &format!("{p:.0} W"), "Power");
            }
            if let Some(c) = gpu.clock_mhz {
                stat(ui, &format!("{c} MHz"), "Clock");
            }
            if let Some(c) = gpu.mem_clock_mhz {
                stat(ui, &format!("{c} MHz"), "Memory clock");
            }
        });
    }

    fn disk_details(&self, ui: &mut egui::Ui) {
        header(ui, "Disk", "");
        let drives = self.last.as_ref().map_or(&[][..], |s| &s.drives[..]);
        if drives.is_empty() {
            ui.label(RichText::new("No drives found").weak());
            return;
        }
        for d in drives {
            ui.add_space(4.0);
            let title = [d.mount.as_str(), d.label.as_str(), d.kind.as_str()]
                .iter()
                .filter(|p| !p.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("  ");
            ui.label(RichText::new(title).strong());
            let text = format!(
                "{} free of {}",
                size_plain(d.free.min(d.total)),
                size_plain(d.total)
            );
            ui.add(
                egui::ProgressBar::new(drive_used_fraction(d.total, d.free))
                    .desired_width(ui.available_width())
                    .fill(ACCENT)
                    .text(text),
            );
            if let Some([r, w]) = self.drives.get(&d.mount) {
                let lines = [
                    Line {
                        values: r.values(),
                        color: AMBER,
                        fill: true,
                    },
                    Line {
                        values: w.values(),
                        color: GREY,
                        fill: false,
                    },
                ];
                graph(ui, IO_HEIGHT, &lines, r.capacity(), peak(r, w, DISK_FLOOR));
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                stat(ui, &rate(d.read_bps), "Read");
                stat(ui, &rate(d.write_bps), "Write");
            });
        }
    }

    fn net_details(&self, ui: &mut egui::Ui) {
        header(ui, "Network", "");
        let adapters = self.last.as_ref().map_or(&[][..], |s| &s.adapters[..]);
        if adapters.is_empty() {
            ui.label(RichText::new("No adapters found").weak());
            return;
        }
        for a in adapters {
            ui.add_space(4.0);
            ui.label(RichText::new(&a.name).strong());
            if let Some([d, u]) = self.adapters.get(&a.name) {
                let lines = [
                    Line {
                        values: d.values(),
                        color: SKY,
                        fill: true,
                    },
                    Line {
                        values: u.values(),
                        color: GREY,
                        fill: false,
                    },
                ];
                graph(ui, IO_HEIGHT, &lines, d.capacity(), peak(d, u, NET_FLOOR));
            }
            let total = format!(
                "{} down, {} up",
                size_plain(a.total_down),
                size_plain(a.total_up)
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                stat(ui, &rate(a.down_bps), "Down");
                stat(ui, &rate(a.up_bps), "Up");
                stat(ui, &total, "Total");
            });
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
