//! The Overview page: a resource list on the left and the detail of the selected row on the
//! right. The list rows are in `list.rs`, the detail views in `detail.rs`.

use eframe::egui::{self, Align, Color32, Layout, Vec2};

use super::power;
use super::Resources;
use crate::format::{rate, size_plain};
use crate::ui::{
    graph_card, GraphCard, Line, DATA_MEMORY, DATA_NETWORK, DATA_SECOND, GREEN, SPACE_L,
};

/// Width of the resource list.
const LIST_WIDTH: f32 = 280.0;
/// Width kept free on the right for the scrollbar that floats over the content.
pub(super) const SCROLL_GUTTER: f32 = 14.0;
/// Samples without GPU data in a row before the card says no GPU was found.
pub(super) const GPU_MISSING_AFTER: u32 = 3;
pub(super) const DISK_FLOOR: f32 = 1024.0 * 1024.0;
pub(super) const NET_FLOOR: f32 = 128.0 * 1024.0;
pub(super) const VIOLET: Color32 = DATA_MEMORY;
pub(super) const SKY: Color32 = DATA_NETWORK;
pub(super) const GREY: Color32 = DATA_SECOND;
pub(super) const NONE: &str = "-";

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
pub(super) fn peak_ceiling(series: &[&[f32]], floor: f32) -> f32 {
    let peak = series
        .iter()
        .flat_map(|s| s.iter().copied())
        .fold(0.0f32, f32::max);
    nice_ceiling(peak, floor)
}

/// "physical/logical", or just the logical count when the physical count is unknown.
pub(super) fn cores_label(physical: Option<usize>, logical: usize) -> String {
    match physical {
        Some(p) => format!("{p}/{logical}"),
        None => logical.to_string(),
    }
}

impl Resources {
    pub(super) fn overview_ui(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        let list_w = LIST_WIDTH.min((avail.x * 0.4).floor());
        let detail_w = (avail.x - list_w - SPACE_L).max(0.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_L;
            ui.allocate_ui_with_layout(
                Vec2::new(list_w, avail.y),
                Layout::top_down(Align::LEFT),
                |ui| self.resource_list(ui, list_w),
            );
            ui.allocate_ui_with_layout(
                Vec2::new(detail_w, avail.y),
                Layout::top_down(Align::LEFT),
                |ui| self.resource_detail(ui, avail.y),
            );
        });
    }

    /// GPU power (measured) and processor power (estimated) as two lines, with the energy used
    /// over the shown range for each.
    pub(super) fn power_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let range = self.range;
        let gpu = self.gpu_power.view(range);
        let cpu: Vec<f32> = self
            .cpu
            .view(range)
            .iter()
            .map(|l| power::cpu_estimate_w(*l, self.cpu_rated_w))
            .collect();
        let has_gpu = gpu.iter().any(|v| v.is_finite());
        let mut lines = vec![Line {
            values: &cpu,
            color: GREY,
            fill: false,
            label: "CPU (estimate)",
            fmt: power::fmt_watts,
        }];
        if has_gpu {
            lines.insert(
                0,
                Line {
                    values: gpu,
                    color: GREEN,
                    fill: true,
                    label: "GPU (measured)",
                    fmt: power::fmt_watts,
                },
            );
        }
        let max = peak_ceiling(&[gpu, &cpu], 50.0);
        let step = range.step_secs();
        let mut stats = Vec::new();
        if let Some(watts) = self.gpu_power.last().filter(|v| v.is_finite()) {
            stats.push((power::fmt_watts(watts), "GPU now"));
        }
        stats.push((
            cpu.last()
                .copied()
                .map_or(NONE.to_string(), power::fmt_watts),
            "CPU now (estimate)",
        ));
        if has_gpu {
            let label = if gpu.iter().any(|v| !v.is_finite()) {
                "GPU used (recorded intervals)"
            } else {
                "GPU used"
            };
            stats.push((power::fmt_energy(power::energy_wh(gpu, step)), label));
        }
        stats.push((
            power::fmt_energy(power::energy_wh(&cpu, step)),
            "CPU used (estimate)",
        ));
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "Power",
                detail: if has_gpu {
                    "GPU measured, CPU estimated from its load"
                } else {
                    "CPU estimated from its load; no GPU power reading"
                },
                lines: &lines,
                capacity: range.points(),
                max,
                step_secs: step,
                stats: &stats,
                note: None,
            },
        );
    }
}

pub(super) fn fmt_percent(v: f32) -> String {
    format!("{v:.0}%")
}

pub(super) fn fmt_rate(v: f32) -> String {
    rate(v as f64)
}

/// "3.1 GB of 10.0 GB", or "-" when the used amount is unknown.
#[allow(dead_code)]
pub(super) fn gpu_memory_label(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(u), Some(t)) => format!("{} of {}", size_plain(u), size_plain(t)),
        (Some(u), None) => size_plain(u),
        _ => NONE.to_string(),
    }
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
    fn gpu_memory_label_variants() {
        assert_eq!(gpu_memory_label(None, Some(1)), "-");
        assert_eq!(gpu_memory_label(None, None), "-");
        let gb = 1024u64 * 1024 * 1024;
        let s = gpu_memory_label(Some(gb), Some(4 * gb));
        assert!(s.contains(" of "), "{s}");
    }

    #[test]
    fn cores_label_variants() {
        assert_eq!(cores_label(Some(8), 16), "8/16");
        assert_eq!(cores_label(None, 4), "4");
    }
}
