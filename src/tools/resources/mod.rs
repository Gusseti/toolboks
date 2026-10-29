//! Resource Monitor: live CPU and memory graphs.

use eframe::egui;

mod history;
mod overview;
mod sampler;

use history::History;
use sampler::{Info, Msg, Sample, Sampler};

/// Points kept per graph (one per second, so a minute).
const HISTORY_POINTS: usize = 60;

pub struct Resources {
    sampler: Option<Sampler>,
    info: Option<Info>,
    last: Option<Sample>,
    cpu: History,
    mem: History,
    disk_read: History,
    disk_write: History,
    net_down: History,
    net_up: History,
}

impl Resources {
    pub fn new() -> Self {
        Self {
            sampler: None,
            info: None,
            last: None,
            cpu: History::new(HISTORY_POINTS),
            mem: History::new(HISTORY_POINTS),
            disk_read: History::new(HISTORY_POINTS),
            disk_write: History::new(HISTORY_POINTS),
            net_down: History::new(HISTORY_POINTS),
            net_up: History::new(HISTORY_POINTS),
        }
    }
}

impl crate::tools::Tool for Resources {
    fn name(&self) -> &'static str {
        "Resource Monitor"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Overview"]
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
                    self.last = Some(sample);
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _page: usize) {
        let sampler = self.sampler.get_or_insert_with(|| Sampler::start(ui.ctx()));
        sampler.ping();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.overview_ui(ui));
    }
}
