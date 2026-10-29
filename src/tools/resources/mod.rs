//! Resource Monitor: live CPU and memory graphs.

use std::time::{Duration, Instant};

use eframe::egui;

mod details;
mod gpu;
mod history;
mod nvml;
mod overview;
mod sampler;

use history::History;
use sampler::{GpuSample, Info, Msg, Sample, Sampler};

/// Points kept per graph (one per second, so a minute).
const HISTORY_POINTS: usize = 60;

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
                let series = std::array::from_fn(|_| History::new(HISTORY_POINTS));
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
    cores: Vec<History>,
    engines: Keyed<1>,
    drives: Keyed<2>,
    adapters: Keyed<2>,
    /// Latest sample that carried GPU data.
    gpu: Option<GpuSample>,
    /// Samples in a row that arrived without GPU data.
    gpu_missing: u32,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            top_tab: TopTab::default(),
            sampler: None,
            info: None,
            last: None,
            last_at: None,
            cpu: History::new(HISTORY_POINTS),
            mem: History::new(HISTORY_POINTS),
            disk_read: History::new(HISTORY_POINTS),
            disk_write: History::new(HISTORY_POINTS),
            net_down: History::new(HISTORY_POINTS),
            net_up: History::new(HISTORY_POINTS),
            gpu_load: History::new(HISTORY_POINTS),
            cores: Vec::new(),
            engines: Keyed::new(),
            drives: Keyed::new(),
            adapters: Keyed::new(),
            gpu: None,
            gpu_missing: 0,
        }
    }
}

impl crate::tools::Tool for Resources {
    fn name(&self) -> &'static str {
        "Resource Monitor"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Overview", "Details"]
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

    fn tick(&mut self, _ctx: &egui::Context) {
        let Some(sampler) = &self.sampler else {
            return;
        };
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
                    self.cores
                        .resize_with(sample.cores.len(), || History::new(HISTORY_POINTS));
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
        assert_eq!(k.get("a").unwrap()[0].values(), &[1.0, 2.0]);
        assert!(k.get("c").is_none());
    }

    #[test]
    fn update_pushes_zero_for_absent_keys() {
        let mut k = Keyed::<1>::new();
        k.update(&[("a", [5.0]), ("b", [6.0])]);
        k.update(&[("a", [7.0])]);
        assert_eq!(k.get("a").unwrap()[0].values(), &[5.0, 7.0]);
        assert_eq!(k.get("b").unwrap()[0].values(), &[6.0, 0.0]);
    }
}
