//! SKELETON: the backend builder replaces every `todo!()` body. Keep these public types and
//! signatures exactly; the Overview UI is being written against them in parallel.

use eframe::egui;

/// Facts that do not change while the app runs. Sent once, before the first sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub cpu_name: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    /// Installed RAM in bytes.
    pub mem_total: u64,
}

/// One reading, taken about once a second.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// Total CPU use, 0 to 100.
    pub cpu: f32,
    /// CPU use per logical core, 0 to 100 each.
    pub cores: Vec<f32>,
    /// Average current clock speed over all logical cores, in MHz.
    pub cpu_mhz: u64,
    /// RAM in use in bytes (total minus available).
    pub mem_used: u64,
    pub uptime_secs: u64,
}

pub enum Msg {
    Info(Info),
    Sample(Sample),
}

pub struct Sampler;

impl Sampler {
    /// Starts the worker thread. It wakes the UI with `ctx.request_repaint()` after each sample.
    pub fn start(_ctx: &egui::Context) -> Sampler {
        todo!()
    }

    /// Tell the sampler the page is on screen right now. Call it from every `ui` frame.
    pub fn ping(&self) {
        todo!()
    }

    /// Everything the worker has sent since the last call, oldest first. Never blocks.
    pub fn drain(&self) -> Vec<Msg> {
        todo!()
    }
}
