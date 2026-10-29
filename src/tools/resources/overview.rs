//! The Overview page: a CPU card and a Memory card with live graphs.

use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};

use super::Resources;
use crate::format::{clock, rate, size_plain, uptime};
use crate::ui::{card, graph, stat, Line, ACCENT, AMBER, GAP};

const MAX_WIDTH: f32 = 880.0;
const CARD_HEIGHT: f32 = 260.0;
const GRAPH_HEIGHT: f32 = 120.0;
const IO_CARD_HEIGHT: f32 = 210.0;
const IO_GRAPH_HEIGHT: f32 = 60.0;
const DISK_FLOOR: f32 = 1024.0 * 1024.0;
const NET_FLOOR: f32 = 128.0 * 1024.0;
const VIOLET: Color32 = Color32::from_rgb(167, 139, 250);
const SKY: Color32 = Color32::from_rgb(56, 189, 248);
const GREY: Color32 = Color32::from_rgb(139, 144, 154);
const NONE: &str = "-";

/// Memory in use as a percentage of the total; 0 when the total is unknown.
pub(super) fn mem_percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0) as f32
    }
}

/// Smallest of 1, 2 or 5 times a power of ten that is >= `max`, never below `floor`.
/// Used to auto-scale throughput graphs.
pub(super) fn nice_ceiling(max: f32, floor: f32) -> f32 {
    if !max.is_finite() || max <= floor {
        return floor;
    }
    let mut base = 10f32.powf(max.log10().floor());
    // Guard against float error in log10 near exact powers.
    if base > max {
        base /= 10.0;
    }
    [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * base)
        .find(|c| *c >= max)
        .unwrap_or(10.0 * base)
        .max(floor)
}

/// `nice_ceiling` of the largest value across several histories.
fn peak_ceiling(series: &[&[f32]], floor: f32) -> f32 {
    let peak = series
        .iter()
        .flat_map(|s| s.iter().copied())
        .fold(0.0f32, f32::max);
    nice_ceiling(peak, floor)
}

/// "physical/logical", or just the logical count when the physical count is unknown.
fn cores_label(physical: Option<usize>, logical: usize) -> String {
    match physical {
        Some(p) => format!("{p}/{logical}"),
        None => logical.to_string(),
    }
}

impl Resources {
    pub(super) fn overview_ui(&mut self, ui: &mut egui::Ui) {
        // Leave a gutter on the right for the scrollbar that floats over the content.
        let width = (ui.available_width() - 14.0).min(MAX_WIDTH);
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                    ui.label(RichText::new("Resource Monitor").size(24.0).strong());
                    self.cards(ui, width);
                },
            );
        });
    }

    fn cards(&self, ui: &mut egui::Ui, total: f32) {
        let left = ((total - GAP) * 7.0 / 12.0).floor();
        let right = total - GAP - left;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(left, 0.0), Layout::top_down(Align::LEFT), |ui| {
                card(ui, left, CARD_HEIGHT, |ui| self.cpu_card(ui))
            });
            ui.allocate_ui_with_layout(
                Vec2::new(right, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| card(ui, right, CARD_HEIGHT, |ui| self.mem_card(ui)),
            );
        });

        let half = ((total - GAP) / 2.0).floor();
        let other = total - GAP - half;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(half, 0.0), Layout::top_down(Align::LEFT), |ui| {
                card(ui, half, IO_CARD_HEIGHT, |ui| self.disk_card(ui))
            });
            ui.allocate_ui_with_layout(
                Vec2::new(other, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| card(ui, other, IO_CARD_HEIGHT, |ui| self.net_card(ui)),
            );
        });
    }

    fn disk_card(&self, ui: &mut egui::Ui) {
        header(ui, "Disk", "All drives");
        let max = peak_ceiling(
            &[self.disk_read.values(), self.disk_write.values()],
            DISK_FLOOR,
        );
        let lines = [
            Line {
                values: self.disk_read.values(),
                color: AMBER,
                fill: true,
            },
            Line {
                values: self.disk_write.values(),
                color: GREY,
                fill: false,
            },
        ];
        graph(ui, IO_GRAPH_HEIGHT, &lines, self.disk_read.capacity(), max);

        let read = self.last.as_ref().map(|s| rate(s.disk_read_bps));
        let write = self.last.as_ref().map(|s| rate(s.disk_write_bps));
        let active = self.last.as_ref().map(|s| {
            s.disk_active
                .map_or(NONE.to_string(), |a| format!("{a:.0}%"))
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, read.as_deref().unwrap_or(NONE), "Read");
            stat(ui, write.as_deref().unwrap_or(NONE), "Write");
            stat(ui, active.as_deref().unwrap_or(NONE), "Active");
        });
    }

    fn net_card(&self, ui: &mut egui::Ui) {
        header(ui, "Network", "All adapters");
        let max = peak_ceiling(&[self.net_down.values(), self.net_up.values()], NET_FLOOR);
        let lines = [
            Line {
                values: self.net_down.values(),
                color: SKY,
                fill: true,
            },
            Line {
                values: self.net_up.values(),
                color: GREY,
                fill: false,
            },
        ];
        graph(ui, IO_GRAPH_HEIGHT, &lines, self.net_down.capacity(), max);

        let down = self.last.as_ref().map(|s| rate(s.net_down_bps));
        let up = self.last.as_ref().map(|s| rate(s.net_up_bps));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, down.as_deref().unwrap_or(NONE), "Down");
            stat(ui, up.as_deref().unwrap_or(NONE), "Up");
        });
    }

    fn cpu_card(&self, ui: &mut egui::Ui) {
        let name = self.info.as_ref().map_or("", |i| i.cpu_name.as_str());
        header(ui, "CPU", name);
        let line = Line {
            values: self.cpu.values(),
            color: ACCENT,
            fill: true,
        };
        graph(ui, GRAPH_HEIGHT, &[line], self.cpu.capacity(), 100.0);

        let usage = self.last.as_ref().map(|s| format!("{:.0}%", s.cpu));
        let speed = self.last.as_ref().map(|s| clock(s.cpu_mhz));
        let up = self.last.as_ref().map(|s| uptime(s.uptime_secs));
        let cores = self
            .info
            .as_ref()
            .map(|i| cores_label(i.physical_cores, i.logical_cores));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, usage.as_deref().unwrap_or(NONE), "Usage");
            stat(ui, speed.as_deref().unwrap_or(NONE), "Speed");
            stat(ui, cores.as_deref().unwrap_or(NONE), "Cores / threads");
            stat(ui, up.as_deref().unwrap_or(NONE), "Up time");
        });
    }

    fn mem_card(&self, ui: &mut egui::Ui) {
        let total = self.info.as_ref().map_or(0, |i| i.mem_total);
        let size = if total > 0 {
            size_plain(total)
        } else {
            String::new()
        };
        header(ui, "Memory", &size);
        let line = Line {
            values: self.mem.values(),
            color: VIOLET,
            fill: true,
        };
        graph(ui, GRAPH_HEIGHT, &[line], self.mem.capacity(), 100.0);

        let percent = self.mem.last().map(|p| format!("{p:.0}%"));
        let used = self.last.as_ref().map(|s| size_plain(s.mem_used));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            stat(ui, percent.as_deref().unwrap_or(NONE), "In use");
            stat(ui, used.as_deref().unwrap_or(NONE), "Used");
        });
    }
}

/// Strong title on the left, weak detail on the right.
fn header(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(detail).weak()).truncate());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_percent_basic() {
        assert_eq!(mem_percent(50, 200), 25.0);
        assert_eq!(mem_percent(200, 200), 100.0);
    }

    #[test]
    fn mem_percent_zero_total() {
        assert_eq!(mem_percent(10, 0), 0.0);
    }

    #[test]
    fn nice_ceiling_exact_powers() {
        assert_eq!(nice_ceiling(1000.0, 1.0), 1000.0);
        assert_eq!(nice_ceiling(100.0, 1.0), 100.0);
        assert_eq!(nice_ceiling(2000.0, 1.0), 2000.0);
        assert_eq!(nice_ceiling(5000.0, 1.0), 5000.0);
    }

    #[test]
    fn nice_ceiling_between_steps() {
        assert_eq!(nice_ceiling(1100.0, 1.0), 2000.0);
        assert_eq!(nice_ceiling(2100.0, 1.0), 5000.0);
        assert_eq!(nice_ceiling(5100.0, 1.0), 10_000.0);
        assert_eq!(nice_ceiling(9999.0, 1.0), 10_000.0);
        assert_eq!(nice_ceiling(30.0, 1.0), 50.0);
    }

    #[test]
    fn nice_ceiling_respects_floor() {
        assert_eq!(nice_ceiling(10.0, 100.0), 100.0);
        assert_eq!(nice_ceiling(0.0, 128.0), 128.0);
        assert_eq!(nice_ceiling(0.0, 1.0), 1.0);
        assert_eq!(nice_ceiling(f32::NAN, 5.0), 5.0);
    }

    #[test]
    fn cores_label_variants() {
        assert_eq!(cores_label(Some(8), 16), "8/16");
        assert_eq!(cores_label(None, 4), "4");
    }
}
