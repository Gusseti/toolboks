//! The Overview detail pane: the graph and numbers for the resource selected in the list.

use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};

use super::history::{History, Range};
use super::overview::{
    cores_label, fmt_percent, fmt_rate, gpu_memory_label, nice_ceiling, peak_ceiling, DISK_FLOOR,
    GPU_MISSING_AFTER, GREY, NET_FLOOR, NONE, SCROLL_GUTTER, SKY, VIOLET,
};
use super::{power, Resource, Resources};
use crate::format::{clock, friendly_name, rate, size_plain, uptime};
use crate::ui::{
    card, graph_card, GraphCard, Line, ACCENT, AMBER, CARD_PAD, GAP, GREEN, TEXT_SECTION,
};

/// Rows kept in a Top processes card.
const TOP_ROWS: usize = 5;
const MINI_HEIGHT: f32 = 92.0;
const DRIVE_HEIGHT: f32 = 120.0;
/// Engine types that always get a graph when the GPU reports them.
const MAIN_ENGINES: [&str; 5] = ["3D", "Copy", "Video Decode", "Video Encode", "Compute"];

/// The value column of a Top processes card.
fn value_label(kind: Resource, value: f64) -> String {
    match kind {
        Resource::Memory => size_plain(value.max(0.0) as u64),
        Resource::Disk => rate(value),
        _ => {
            if value > 0.0 && value < 1.0 {
                "<1%".to_string()
            } else {
                format!("{value:.0}%")
            }
        }
    }
}

fn max_of(h: &History, range: Range) -> f32 {
    h.view(range).iter().copied().fold(0.0, f32::max)
}

fn peak(a: &History, b: &History, range: Range, floor: f32) -> f32 {
    nice_ceiling(max_of(a, range).max(max_of(b, range)), floor)
}

/// Used share of a drive from 0 to 1; 0 when the size is unknown.
fn used_percent(total: u64, free: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let used = total.saturating_sub(free);
    (used as f64 / total as f64).clamp(0.0, 1.0) as f32 * 100.0
}

/// Main engine types always show; others only once they have been busy.
fn engine_visible(name: &str, history_max: f32) -> bool {
    MAIN_ENGINES.iter().any(|m| m.eq_ignore_ascii_case(name)) || history_max > 0.0
}

/// Lays `cells` out in rows of `cols` equal columns across `width`; `cell` gets the cell width.
fn grid<T>(
    ui: &mut egui::Ui,
    width: f32,
    cols: usize,
    cells: &[T],
    mut cell: impl FnMut(&mut egui::Ui, &T, f32),
) {
    let cols = cols.max(1);
    let w = ((width - GAP * (cols - 1) as f32) / cols as f32)
        .floor()
        .max(0.0);
    for row in cells.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for c in row {
                cell(ui, c, w);
            }
        });
    }
}

/// One small per-core or per-engine graph with its share in the bottom bar.
fn mini_card(ui: &mut egui::Ui, w: f32, name: &str, h: &History, range: Range, color: Color32) {
    let line = Line {
        values: h.view(range),
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
        w,
        MINI_HEIGHT,
        &GraphCard {
            title: name,
            detail: "",
            lines: &[line],
            capacity: range.points(),
            max: 100.0,
            step_secs: range.step_secs(),
            stats: &stats,
            note: None,
        },
    );
}

/// A section title above a group of cards.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(GAP / 2.0);
    ui.label(RichText::new(text).size(TEXT_SECTION).strong());
}

/// Columns for small graphs at this width.
fn columns(width: f32) -> usize {
    if width < 520.0 {
        2
    } else {
        4
    }
}

impl Resources {
    /// The right-hand pane. Scrolls on its own; `height` is the room the pane has.
    pub(super) fn resource_detail(&mut self, ui: &mut egui::Ui, height: f32) {
        egui::ScrollArea::vertical()
            .id_salt(("resource-detail", self.selected))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = (ui.available_width() - SCROLL_GUTTER).max(0.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(width, 0.0),
                    Layout::top_down(Align::LEFT),
                    |ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                        let big = (height * 0.4).clamp(170.0, 300.0);
                        match self.selected {
                            Resource::Cpu => self.cpu_detail(ui, width, big),
                            Resource::Memory => self.memory_detail(ui, width, big),
                            Resource::Gpu => self.gpu_detail(ui, width, big),
                            Resource::Disk => self.disk_detail(ui, width, big),
                            Resource::Network => self.network_detail(ui, width, big),
                            Resource::Power => self.power_detail(ui, width, big),
                        }
                    },
                );
            });
    }

    fn cpu_detail(&self, ui: &mut egui::Ui, width: f32, big: f32) {
        let range = self.range;
        let name = self.info.as_ref().map_or("", |i| i.cpu_name.as_str());
        let s = self.last.as_ref();
        let line = Line {
            values: self.cpu.view(range),
            color: ACCENT,
            fill: true,
            label: "Usage",
            fmt: fmt_percent,
        };
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
        graph_card(
            ui,
            width,
            big,
            &GraphCard {
                title: "CPU",
                detail: name,
                lines: &[line],
                capacity: range.points(),
                max: 100.0,
                step_secs: range.step_secs(),
                stats: &stats,
                note: None,
            },
        );

        let cores: Vec<(usize, &History)> = self.cores.iter().enumerate().collect();
        toolboks_addon_sdk::tutorial::region(ui, "details-cpu", |ui| {
            grid(ui, width, columns(width), &cores, |ui, (i, h), w| {
                mini_card(ui, w, &format!("Core {i}"), h, range, ACCENT);
            });
        });

        self.top_card(ui, width, Resource::Cpu);
        ui.label(
            RichText::new(
                "CPU temperature is not shown: Windows gives apps no safe way to read it.",
            )
            .weak()
            .small(),
        );
    }

    fn memory_detail(&self, ui: &mut egui::Ui, width: f32, big: f32) {
        let range = self.range;
        let total = self.info.as_ref().map_or(0, |i| i.mem_total);
        let size = if total > 0 {
            size_plain(total)
        } else {
            String::new()
        };
        let line = Line {
            values: self.mem.view(range),
            color: VIOLET,
            fill: true,
            label: "In use",
            fmt: fmt_percent,
        };
        let used = self.last.as_ref().map(|s| s.mem_used);
        let stats = [
            (
                self.mem
                    .last()
                    .map_or(NONE.to_string(), |p| format!("{p:.0}%")),
                "In use",
            ),
            (used.map_or(NONE.to_string(), size_plain), "Used"),
            (
                used.filter(|_| total > 0)
                    .map_or(NONE.to_string(), |u| size_plain(total.saturating_sub(u))),
                "Free",
            ),
        ];
        graph_card(
            ui,
            width,
            big,
            &GraphCard {
                title: "Memory",
                detail: &size,
                lines: &[line],
                capacity: range.points(),
                max: 100.0,
                step_secs: range.step_secs(),
                stats: &stats,
                note: None,
            },
        );
        self.top_card(ui, width, Resource::Memory);
    }

    fn gpu_detail(&self, ui: &mut egui::Ui, width: f32, big: f32) {
        let range = self.range;
        let g = self.gpu.as_ref();
        let name = g.map_or("", |g| g.name.as_str());
        let note = if g.is_none() {
            Some(if self.gpu_missing >= GPU_MISSING_AFTER {
                "No GPU found"
            } else {
                "Reading GPU..."
            })
        } else {
            None
        };
        let line = Line {
            values: self.gpu_load.view(range),
            color: GREEN,
            fill: true,
            label: "Load",
            fmt: fmt_percent,
        };
        let mut stats = vec![
            (
                g.and_then(|g| g.load)
                    .map_or(NONE.to_string(), |l| format!("{l:.0}%")),
                "Load",
            ),
            (
                gpu_memory_label(g.and_then(|g| g.mem_used), g.and_then(|g| g.mem_total)),
                "Memory",
            ),
        ];
        if let Some(t) = g.and_then(|g| g.temp_c) {
            stats.push((format!("{t:.0} °C"), "Temp"));
        }
        if let Some(p) = g.and_then(|g| g.power_w) {
            stats.push((format!("{p:.0} W"), "Power"));
        }
        if let Some(c) = g.and_then(|g| g.clock_mhz) {
            stats.push((format!("{c} MHz"), "Clock"));
        }
        if let Some(c) = g.and_then(|g| g.mem_clock_mhz) {
            stats.push((format!("{c} MHz"), "Memory clock"));
        }
        graph_card(
            ui,
            width,
            big,
            &GraphCard {
                title: "GPU",
                detail: name,
                lines: &[line],
                capacity: range.points(),
                max: 100.0,
                step_secs: range.step_secs(),
                stats: &stats,
                note,
            },
        );

        let mut engines: Vec<(&str, &History)> = self
            .engines
            .iter()
            .filter(|(n, s)| engine_visible(n, max_of(&s[0], range)))
            .map(|(n, s)| (n, &s[0]))
            .collect();
        let rank = |n: &str| {
            MAIN_ENGINES
                .iter()
                .position(|m| m.eq_ignore_ascii_case(n))
                .unwrap_or(MAIN_ENGINES.len())
        };
        engines.sort_by_key(|(n, _)| rank(n));
        if !engines.is_empty() {
            section(ui, "Engines");
            grid(ui, width, columns(width), &engines, |ui, (n, h), w| {
                mini_card(ui, w, n, h, range, GREEN);
            });
        }
        self.top_card(ui, width, Resource::Gpu);
    }

    fn disk_detail(&self, ui: &mut egui::Ui, width: f32, big: f32) {
        let range = self.range;
        let (read, write) = (self.disk_read.view(range), self.disk_write.view(range));
        let lines = [
            Line {
                values: read,
                color: AMBER,
                fill: true,
                label: "Read",
                fmt: fmt_rate,
            },
            Line {
                values: write,
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
            big,
            &GraphCard {
                title: "Disk",
                detail: "All drives",
                lines: &lines,
                capacity: range.points(),
                max: peak_ceiling(&[read, write], DISK_FLOOR),
                step_secs: range.step_secs(),
                stats: &stats,
                note: None,
            },
        );

        let drives = s.map_or(&[][..], |s| &s.drives[..]);
        if drives.is_empty() {
            ui.label(RichText::new("No drives found").weak());
        } else {
            section(ui, "Drives");
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
                used_percent(d.total, d.free)
            );
            let stats = [(rate(d.read_bps), "Read"), (rate(d.write_bps), "Write")];
            let Some([r, w]) = self.drives.get(&d.mount) else {
                ui.label(RichText::new(format!("{title}  {free}")).weak());
                continue;
            };
            let lines = [
                Line {
                    values: r.view(range),
                    color: AMBER,
                    fill: true,
                    label: "Read",
                    fmt: fmt_rate,
                },
                Line {
                    values: w.view(range),
                    color: GREY,
                    fill: false,
                    label: "Write",
                    fmt: fmt_rate,
                },
            ];
            graph_card(
                ui,
                width,
                DRIVE_HEIGHT,
                &GraphCard {
                    title: &title,
                    detail: &free,
                    lines: &lines,
                    capacity: range.points(),
                    max: peak(r, w, range, DISK_FLOOR),
                    step_secs: range.step_secs(),
                    stats: &stats,
                    note: None,
                },
            );
        }
        self.top_card(ui, width, Resource::Disk);
    }

    fn network_detail(&self, ui: &mut egui::Ui, width: f32, big: f32) {
        let range = self.range;
        let (down, up) = (self.net_down.view(range), self.net_up.view(range));
        let lines = [
            Line {
                values: down,
                color: SKY,
                fill: true,
                label: "Down",
                fmt: fmt_rate,
            },
            Line {
                values: up,
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
            big,
            &GraphCard {
                title: "Network",
                detail: "All adapters",
                lines: &lines,
                capacity: range.points(),
                max: peak_ceiling(&[down, up], NET_FLOOR),
                step_secs: range.step_secs(),
                stats: &stats,
                note: None,
            },
        );

        let adapters = s.map_or(&[][..], |s| &s.adapters[..]);
        if adapters.is_empty() {
            ui.label(RichText::new("No adapters found").weak());
        } else {
            section(ui, "Adapters");
        }
        for a in adapters {
            let total = format!(
                "{} down, {} up in total",
                size_plain(a.total_down),
                size_plain(a.total_up)
            );
            let stats = [(rate(a.down_bps), "Down"), (rate(a.up_bps), "Up")];
            let Some([d, u]) = self.adapters.get(&a.name) else {
                ui.label(RichText::new(format!("{}  {total}", a.name)).weak());
                continue;
            };
            let lines = [
                Line {
                    values: d.view(range),
                    color: SKY,
                    fill: true,
                    label: "Down",
                    fmt: fmt_rate,
                },
                Line {
                    values: u.view(range),
                    color: GREY,
                    fill: false,
                    label: "Up",
                    fmt: fmt_rate,
                },
            ];
            graph_card(
                ui,
                width,
                DRIVE_HEIGHT,
                &GraphCard {
                    title: &a.name,
                    detail: &total,
                    lines: &lines,
                    capacity: range.points(),
                    max: peak(d, u, range, NET_FLOOR),
                    step_secs: range.step_secs(),
                    stats: &stats,
                    note: None,
                },
            );
        }
    }

    fn power_detail(&mut self, ui: &mut egui::Ui, width: f32, big: f32) {
        self.power_card(ui, width, big);
        card(ui, width, 0.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                power::rated_power_input(ui, &mut self.cpu_rated_w);
            });
            ui.label(
                RichText::new(
                    "The graphics card figure is measured. Windows cannot report processor \
                     power safely, so that line is estimated from its load and this rating.",
                )
                .weak()
                .small(),
            );
        });
    }

    /// The busiest programs for `kind`, from the latest sample.
    fn top_card(&self, ui: &mut egui::Ui, width: f32, kind: Resource) {
        let (by, color) = match kind {
            Resource::Memory => ("by memory", VIOLET),
            Resource::Gpu => ("by GPU use", GREEN),
            Resource::Disk => ("by disk activity", AMBER),
            _ => ("by CPU use", ACCENT),
        };
        toolboks_addon_sdk::tutorial::region(ui, "top-processes", |ui| {
            card(ui, width, 0.0, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Top processes").size(TEXT_SECTION).strong());
                    ui.label(RichText::new(by).weak());
                });
                let Some(sample) = &self.last else {
                    ui.label(RichText::new("Collecting...").weak());
                    return;
                };
                let list = match kind {
                    Resource::Memory => &sample.top.memory,
                    Resource::Gpu => &sample.top.gpu,
                    Resource::Disk => &sample.top.disk,
                    _ => &sample.top.cpu,
                };
                if list.is_empty() {
                    ui.label(RichText::new("Nothing active").weak());
                    return;
                }
                let biggest = list.iter().map(|p| p.value).fold(0.0f64, f64::max);

                let (name_w, value_w, gap) = (160.0, 72.0, 14.0);
                let bar_w =
                    (width - 2.0 * (CARD_PAD + 1.0) - name_w - value_w - 2.0 * gap).max(80.0);
                egui::Grid::new(("top_processes", kind))
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
                                    .fill(color),
                            );
                            ui.label(RichText::new(value_label(kind, p.value)).weak());
                            ui.end_row();
                        }
                    });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_label_variants() {
        assert_eq!(value_label(Resource::Cpu, 0.4), "<1%");
        assert_eq!(value_label(Resource::Gpu, 0.0), "0%");
        assert_eq!(value_label(Resource::Cpu, 12.4), "12%");
        assert_eq!(
            value_label(Resource::Memory, 1024.0 * 1024.0),
            size_plain(1024 * 1024)
        );
        assert_eq!(value_label(Resource::Disk, 2048.0), rate(2048.0));
    }

    #[test]
    fn used_percent_clamps() {
        assert_eq!(used_percent(0, 0), 0.0);
        assert_eq!(used_percent(100, 25), 75.0);
        assert_eq!(used_percent(100, 200), 0.0);
        assert_eq!(used_percent(100, 0), 100.0);
    }

    #[test]
    fn engine_visibility() {
        assert!(engine_visible("3D", 0.0));
        assert!(engine_visible("video decode", 0.0));
        assert!(!engine_visible("Security", 0.0));
        assert!(engine_visible("Security", 0.5));
    }
}
