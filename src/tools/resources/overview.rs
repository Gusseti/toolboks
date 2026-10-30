//! The Overview page: a CPU card and a Memory card with live graphs.

use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};

use super::{power, Resources, TopTab};
use crate::format::{clock, friendly_name, rate, size_plain, uptime};
use crate::ui::{card, graph_card, pill, GraphCard, Line, ACCENT, AMBER, CARD_PAD, GAP, GREEN};

const TOP_ROWS: usize = 5;
const TOP_TABS: [(TopTab, &str); 4] = [
    (TopTab::Cpu, "CPU"),
    (TopTab::Memory, "Memory"),
    (TopTab::Gpu, "GPU"),
    (TopTab::Disk, "Disk"),
];

/// The value column of the Top processes card for the given tab.
fn value_label(tab: TopTab, value: f64) -> String {
    match tab {
        TopTab::Cpu | TopTab::Gpu => {
            if value > 0.0 && value < 1.0 {
                "<1%".to_string()
            } else {
                format!("{value:.0}%")
            }
        }
        TopTab::Memory => size_plain(value.max(0.0) as u64),
        TopTab::Disk => rate(value),
    }
}

/// Width kept free on the right for the scrollbar that floats over the content.
pub(super) const SCROLL_GUTTER: f32 = 14.0;
/// Vertical gap between the lines inside a card.
pub(super) const INNER_GAP: f32 = 4.0;
const SMALL_CARD_HEIGHT: f32 = 112.0;
/// Samples without GPU data in a row before the card says no GPU was found.
pub(super) const GPU_MISSING_AFTER: u32 = 3;
const IO_CARD_HEIGHT: f32 = 150.0;
pub(super) const DISK_FLOOR: f32 = 1024.0 * 1024.0;
pub(super) const NET_FLOOR: f32 = 128.0 * 1024.0;
const VIOLET: Color32 = Color32::from_rgb(167, 139, 250);
pub(super) const SKY: Color32 = Color32::from_rgb(56, 189, 248);
pub(super) const GREY: Color32 = Color32::from_rgb(139, 144, 154);
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
        let width = ui.available_width() - SCROLL_GUTTER;
        ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::LEFT), |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(GAP);
            self.cards(ui, width);
        });
    }

    fn cards(&mut self, ui: &mut egui::Ui, total: f32) {
        let left = ((total - GAP) * 7.0 / 12.0).floor();
        let right = total - GAP - left;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(left, 0.0), Layout::top_down(Align::LEFT), |ui| {
                self.cpu_card(ui, left, 2.0 * SMALL_CARD_HEIGHT + GAP)
            });
            ui.allocate_ui_with_layout(
                Vec2::new(right, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.spacing_mut().item_spacing.y = GAP;
                    self.gpu_card(ui, right, SMALL_CARD_HEIGHT);
                    self.mem_card(ui, right, SMALL_CARD_HEIGHT);
                },
            );
        });

        let half = ((total - GAP) / 2.0).floor();
        let other = total - GAP - half;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui_with_layout(Vec2::new(half, 0.0), Layout::top_down(Align::LEFT), |ui| {
                self.disk_card(ui, half, IO_CARD_HEIGHT)
            });
            ui.allocate_ui_with_layout(
                Vec2::new(other, 0.0),
                Layout::top_down(Align::LEFT),
                |ui| self.net_card(ui, other, IO_CARD_HEIGHT),
            );
        });

        self.power_card(ui, total, IO_CARD_HEIGHT);

        card(ui, total, 0.0, |ui| self.top_card(ui, total));
    }

    fn top_card(&mut self, ui: &mut egui::Ui, total: f32) {
        ui.spacing_mut().item_spacing.y = INNER_GAP;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Top processes").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // Right-to-left, so add the last pill first.
                for (tab, text) in TOP_TABS.iter().rev() {
                    if pill(ui, text, self.top_tab == *tab).clicked() {
                        self.top_tab = *tab;
                    }
                }
            });
        });

        let tab = self.top_tab;
        let Some(sample) = &self.last else {
            ui.label(RichText::new("Collecting...").weak());
            return;
        };
        let list = match tab {
            TopTab::Cpu => &sample.top.cpu,
            TopTab::Memory => &sample.top.memory,
            TopTab::Gpu => &sample.top.gpu,
            TopTab::Disk => &sample.top.disk,
        };
        if list.is_empty() {
            ui.label(RichText::new("Nothing active").weak());
            return;
        }
        let biggest = list.iter().map(|p| p.value).fold(0.0f64, f64::max);

        let (name_w, value_w, gap) = (160.0, 72.0, 14.0);
        let bar_w = (total - 2.0 * (CARD_PAD + 1.0) - name_w - value_w - 2.0 * gap).max(80.0);
        egui::Grid::new("top_processes")
            .num_columns(3)
            .spacing([gap, 2.0])
            .show(ui, |ui| {
                for p in list.iter().take(TOP_ROWS) {
                    let mut name = friendly_name(&p.name);
                    if p.count > 1 {
                        name = format!("{name}  x{}", p.count);
                    }
                    ui.add_sized([name_w, 18.0], egui::Label::new(name).truncate());
                    let fraction = if biggest > 0.0 {
                        p.value / biggest
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction as f32)
                            .desired_width(bar_w)
                            .desired_height(10.0)
                            .fill(ACCENT),
                    );
                    ui.label(RichText::new(value_label(tab, p.value)).weak());
                    ui.end_row();
                }
            });
    }

    fn disk_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let max = peak_ceiling(
            &[
                self.disk_read.view(self.range),
                self.disk_write.view(self.range),
            ],
            DISK_FLOOR,
        );
        let lines = [
            Line {
                values: self.disk_read.view(self.range),
                color: AMBER,
                fill: true,
                label: "Read",
                fmt: fmt_rate,
            },
            Line {
                values: self.disk_write.view(self.range),
                color: GREY,
                fill: false,
                label: "Write",
                fmt: fmt_rate,
            },
        ];
        let s = self.last.as_ref();
        let stats = [
            (
                s.map_or(NONE.to_string(), |s| rate(s.disk_read_bps)),
                "Read",
            ),
            (
                s.map_or(NONE.to_string(), |s| rate(s.disk_write_bps)),
                "Write",
            ),
            (
                s.and_then(|s| s.disk_active)
                    .map_or(NONE.to_string(), |a| format!("{a:.0}%")),
                "Active",
            ),
        ];
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "Disk",
                detail: "All drives",
                lines: &lines,
                capacity: self.range.points(),
                max,
                step_secs: self.range.step_secs(),
                stats: &stats,
                note: None,
            },
        );
    }

    fn net_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let max = peak_ceiling(
            &[self.net_down.view(self.range), self.net_up.view(self.range)],
            NET_FLOOR,
        );
        let lines = [
            Line {
                values: self.net_down.view(self.range),
                color: SKY,
                fill: true,
                label: "Down",
                fmt: fmt_rate,
            },
            Line {
                values: self.net_up.view(self.range),
                color: GREY,
                fill: false,
                label: "Up",
                fmt: fmt_rate,
            },
        ];
        let s = self.last.as_ref();
        let stats = [
            (s.map_or(NONE.to_string(), |s| rate(s.net_down_bps)), "Down"),
            (s.map_or(NONE.to_string(), |s| rate(s.net_up_bps)), "Up"),
        ];
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "Network",
                detail: "All adapters",
                lines: &lines,
                capacity: self.range.points(),
                max,
                step_secs: self.range.step_secs(),
                stats: &stats,
                note: None,
            },
        );
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
        let has_gpu = self.gpu.as_ref().is_some_and(|g| g.power_w.is_some());
        let mut lines = vec![Line {
            values: &cpu,
            color: VIOLET,
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
        if has_gpu {
            stats.push((
                self.gpu_power
                    .last()
                    .map_or(NONE.to_string(), power::fmt_watts),
                "GPU now",
            ));
        }
        stats.push((
            cpu.last()
                .copied()
                .map_or(NONE.to_string(), power::fmt_watts),
            "CPU now (estimate)",
        ));
        if has_gpu {
            stats.push((power::fmt_energy(power::energy_wh(gpu, step)), "GPU used"));
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

    fn cpu_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let name = self.info.as_ref().map_or("", |i| i.cpu_name.as_str());
        let line = Line {
            values: self.cpu.view(self.range),
            color: ACCENT,
            fill: true,
            label: "Usage",
            fmt: fmt_percent,
        };
        let s = self.last.as_ref();
        let stats = [
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
            (
                s.map_or(NONE.to_string(), |s| uptime(s.uptime_secs)),
                "Up time",
            ),
        ];
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "CPU",
                detail: name,
                lines: &[line],
                capacity: self.range.points(),
                max: 100.0,
                step_secs: self.range.step_secs(),
                stats: &stats,
                note: None,
            },
        );
    }

    fn gpu_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let name = self.gpu.as_ref().map_or("", |g| g.name.as_str());
        let missing = self.gpu.is_none() && self.gpu_missing >= GPU_MISSING_AFTER;
        let line = Line {
            values: self.gpu_load.view(self.range),
            color: GREEN,
            fill: true,
            label: "Load",
            fmt: fmt_percent,
        };
        let g = self.gpu.as_ref();
        let mut stats = vec![
            (
                g.and_then(|g| g.load)
                    .map_or(NONE.to_string(), |l| format!("{l:.0}%")),
                "Load",
            ),
            (
                g.and_then(|g| g.mem_used)
                    .map_or(NONE.to_string(), size_plain),
                "Memory",
            ),
        ];
        if let Some(t) = g.and_then(|g| g.temp_c) {
            stats.push((format!("{t:.0} °C"), "Temp"));
        }
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "GPU",
                detail: name,
                lines: &[line],
                capacity: self.range.points(),
                max: 100.0,
                step_secs: self.range.step_secs(),
                stats: &stats,
                note: missing.then_some("No GPU found"),
            },
        );
    }

    fn mem_card(&self, ui: &mut egui::Ui, width: f32, height: f32) {
        let total = self.info.as_ref().map_or(0, |i| i.mem_total);
        let size = if total > 0 {
            size_plain(total)
        } else {
            String::new()
        };
        let line = Line {
            values: self.mem.view(self.range),
            color: VIOLET,
            fill: true,
            label: "In use",
            fmt: fmt_percent,
        };
        let stats = [
            (
                self.mem
                    .last()
                    .map_or(NONE.to_string(), |p| format!("{p:.0}%")),
                "In use",
            ),
            (
                self.last
                    .as_ref()
                    .map_or(NONE.to_string(), |s| size_plain(s.mem_used)),
                "Used",
            ),
        ];
        graph_card(
            ui,
            width,
            height,
            &GraphCard {
                title: "Memory",
                detail: &size,
                lines: &[line],
                capacity: self.range.points(),
                max: 100.0,
                step_secs: self.range.step_secs(),
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
    fn value_label_variants() {
        assert_eq!(value_label(TopTab::Cpu, 0.4), "<1%");
        assert_eq!(value_label(TopTab::Gpu, 0.0), "0%");
        assert_eq!(value_label(TopTab::Cpu, 12.4), "12%");
        assert_eq!(
            value_label(TopTab::Memory, 1024.0 * 1024.0),
            size_plain(1024 * 1024)
        );
        assert_eq!(value_label(TopTab::Disk, 2048.0), rate(2048.0));
    }

    #[test]
    fn cores_label_variants() {
        assert_eq!(cores_label(Some(8), 16), "8/16");
        assert_eq!(cores_label(None, 4), "4");
    }
}
