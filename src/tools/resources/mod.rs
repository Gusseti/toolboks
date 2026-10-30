//! Resource Monitor: live CPU and memory graphs.

use std::time::{Duration, Instant};

use eframe::egui;

mod connections;
mod details;
mod gpu;
mod history;
mod nvml;
mod overview;
mod power;
mod processes;
mod sampler;

use history::{History, Range};
use sampler::{GpuSample, Info, Msg, Sample, Sampler};

/// A sample older than this means the sampler is no longer live.
const LIVE_WITHIN: Duration = Duration::from_secs(5);

/// Histories keyed by name (engine, drive mount, adapter), `N` series each.
pub(crate) struct Keyed<const N: usize> {
    items: Vec<(String, [History; N])>,
}

impl<const N: usize> Keyed<N> {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Finds the series for `key`, creating empty ones when it is new.
    fn find_or_create(&mut self, key: &str) -> &mut [History; N] {
        let at = match self.items.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                let series = std::array::from_fn(|_| History::new());
                self.items.push((key.to_string(), series));
                self.items.len() - 1
            }
        };
        &mut self.items[at].1
    }

    /// Pushes one sample; known keys missing from `present` get 0.0.
    fn update(&mut self, present: &[(&str, [f32; N])]) {
        for (key, values) in present {
            let series = self.find_or_create(key);
            for (h, v) in series.iter_mut().zip(values) {
                h.push(*v);
            }
        }
        for (key, series) in &mut self.items {
            if !present.iter().any(|(k, _)| k == key) {
                for h in series.iter_mut() {
                    h.push(0.0);
                }
            }
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&[History; N]> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, s)| s)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &[History; N])> {
        self.items.iter().map(|(k, s)| (k.as_str(), s))
    }
}

/// Which list the "Top processes" card shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum TopTab {
    #[default]
    Cpu,
    Memory,
    Gpu,
    Disk,
}

pub struct Resources {
    top_tab: TopTab,
    /// How far back the graphs look.
    range: Range,
    sampler: Option<Sampler>,
    info: Option<Info>,
    last: Option<Sample>,
    /// When `last` arrived.
    last_at: Option<Instant>,
    cpu: History,
    mem: History,
    disk_read: History,
    disk_write: History,
    net_down: History,
    net_up: History,
    gpu_load: History,
    gpu_power: History,
    /// Rated power of the processor for the power estimate, set by the user.
    cpu_rated_w: f32,
    cores: Vec<History>,
    engines: Keyed<1>,
    drives: Keyed<2>,
    adapters: Keyed<2>,
    /// Latest sample that carried GPU data.
    gpu: Option<GpuSample>,
    /// Samples in a row that arrived without GPU data.
    gpu_missing: u32,
    processes: processes::Page,
    connections: connections::Page,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            top_tab: TopTab::default(),
            range: Range::default(),
            sampler: None,
            info: None,
            last: None,
            last_at: None,
            cpu: History::new(),
            mem: History::new(),
            disk_read: History::new(),
            disk_write: History::new(),
            net_down: History::new(),
            net_up: History::new(),
            gpu_load: History::new(),
            gpu_power: History::new(),
            cpu_rated_w: power::DEFAULT_RATED_W,
            cores: Vec::new(),
            engines: Keyed::new(),
            drives: Keyed::new(),
            adapters: Keyed::new(),
            gpu: None,
            gpu_missing: 0,
            processes: processes::Page::new(),
            connections: connections::Page::new(),
        }
    }
}

impl Resources {
    /// The "how far back" switch above the graph pages.
    fn range_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(egui::RichText::new("Graphs show the last").weak());
            for range in Range::ALL {
                if crate::ui::pill(ui, range.label(), self.range == range).clicked() {
                    self.range = range;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::DragValue::new(&mut self.cpu_rated_w)
                        .range(10.0..=400.0)
                        .speed(1.0)
                        .suffix(" W"),
                )
                .on_hover_text(
                    "Rated power of your processor, used for the processor power estimate.                      Find it in your processor's specifications (often called TDP).",
                );
                ui.label(egui::RichText::new("CPU rated power").weak());
            });
        });
        ui.add_space(2.0);
    }
}

impl crate::tools::Tool for Resources {
    fn name(&self) -> &'static str {
        "Resource Monitor"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Overview", "Details", "Processes", "Connections"]
    }

    fn status(&self) -> Vec<String> {
        let live = self.last_at.is_some_and(|t| t.elapsed() < LIVE_WITHIN);
        let (true, Some(sample)) = (live, &self.last) else {
            return Vec::new();
        };
        let mut items = vec![format!("CPU {:.0}%", sample.cpu)];
        if let Some(load) = self.gpu.as_ref().and_then(|g| g.load) {
            items.push(format!("GPU {load:.0}%"));
        }
        items
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.processes.tick();
        self.connections.tick();
        // Recording starts with the app, so every range is complete even when this tool is
        // not on screen.
        let sampler = self.sampler.get_or_insert_with(|| Sampler::start(ctx));
        for msg in sampler.drain() {
            match msg {
                Msg::Info(info) => self.info = Some(info),
                Msg::Sample(sample) => {
                    self.cpu.push(sample.cpu);
                    let total = self.info.as_ref().map_or(0, |i| i.mem_total);
                    self.mem.push(overview::mem_percent(sample.mem_used, total));
                    self.disk_read.push(sample.disk_read_bps as f32);
                    self.disk_write.push(sample.disk_write_bps as f32);
                    self.net_down.push(sample.net_down_bps as f32);
                    self.net_up.push(sample.net_up_bps as f32);
                    self.cores.resize_with(sample.cores.len(), History::new);
                    for (h, v) in self.cores.iter_mut().zip(&sample.cores) {
                        h.push(*v);
                    }
                    let drives: Vec<_> = sample
                        .drives
                        .iter()
                        .map(|d| (d.mount.as_str(), [d.read_bps as f32, d.write_bps as f32]))
                        .collect();
                    self.drives.update(&drives);
                    let adapters: Vec<_> = sample
                        .adapters
                        .iter()
                        .map(|a| (a.name.as_str(), [a.down_bps as f32, a.up_bps as f32]))
                        .collect();
                    self.adapters.update(&adapters);
                    let engines: Vec<_> = sample
                        .gpu
                        .iter()
                        .flat_map(|g| g.engines.iter())
                        .map(|(n, v)| (n.as_str(), [*v]))
                        .collect();
                    self.engines.update(&engines);
                    match &sample.gpu {
                        Some(g) => {
                            self.gpu_load.push(g.load.unwrap_or(0.0));
                            self.gpu_power.push(g.power_w.unwrap_or(0.0));
                            self.gpu = Some(g.clone());
                            self.gpu_missing = 0;
                        }
                        None => self.gpu_missing = self.gpu_missing.saturating_add(1),
                    }
                    self.last = Some(*sample);
                    self.last_at = Some(Instant::now());
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        let sampler = self.sampler.get_or_insert_with(|| Sampler::start(ui.ctx()));
        sampler.ping();
        match page {
            2 => {
                self.processes.ui(ui);
                return;
            }
            3 => {
                self.connections.ui(ui);
                return;
            }
            _ => {}
        }
        self.range_bar(ui);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match page {
                1 => self.details_ui(ui),
                _ => self.overview_ui(ui),
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_or_create_reuses_series() {
        let mut k = Keyed::<2>::new();
        k.find_or_create("a")[0].push(1.0);
        k.find_or_create("b");
        k.find_or_create("a")[0].push(2.0);
        assert_eq!(k.iter().count(), 2);
        assert_eq!(k.get("a").unwrap()[0].view(Range::Minute), &[1.0, 2.0]);
        assert!(k.get("c").is_none());
    }

    #[test]
    fn update_pushes_zero_for_absent_keys() {
        let mut k = Keyed::<1>::new();
        k.update(&[("a", [5.0]), ("b", [6.0])]);
        k.update(&[("a", [7.0])]);
        assert_eq!(k.get("a").unwrap()[0].view(Range::Minute), &[5.0, 7.0]);
        assert_eq!(k.get("b").unwrap()[0].view(Range::Minute), &[6.0, 0.0]);
    }
}
