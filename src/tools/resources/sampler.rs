//! Background sampler for the Resources page. A worker thread reads CPU, memory and uptime about
//! once a second, but only while the page is on screen (the UI calls `ping()` every frame).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::System;

/// A ping older than this means the page is no longer shown.
const VISIBLE_WINDOW_MS: u64 = 2500;
const TICK: Duration = Duration::from_millis(1000);

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

pub struct Sampler {
    rx: Receiver<Msg>,
    /// Last ping as milliseconds since `epoch`, stored plus one (0 = never pinged).
    last_ping: Arc<AtomicU64>,
    epoch: Instant,
}

/// `last_ping_plus_one` is 0 when never pinged, else the ping time in ms plus one.
fn is_visible(last_ping_plus_one: u64, now_ms: u64) -> bool {
    if last_ping_plus_one == 0 {
        return false;
    }
    (now_ms + 1).saturating_sub(last_ping_plus_one) < VISIBLE_WINDOW_MS
}

fn mean_mhz(freqs: &[u64]) -> u64 {
    if freqs.is_empty() {
        0
    } else {
        freqs.iter().sum::<u64>() / freqs.len() as u64
    }
}

impl Sampler {
    /// Starts the worker thread. It wakes the UI with `ctx.request_repaint()` after each sample.
    pub fn start(ctx: &egui::Context) -> Sampler {
        let (tx, rx) = mpsc::channel();
        let last_ping = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        let ping = Arc::clone(&last_ping);
        let ctx = ctx.clone();

        let spawned = std::thread::Builder::new()
            .name("resource-sampler".into())
            .spawn(move || {
                let Ok(mut sys) = System::new() else {
                    return;
                };
                sys.refresh_cpu_all();
                sys.refresh_memory();

                let info = Info {
                    cpu_name: sys
                        .cpus()
                        .first()
                        .map(|c| c.brand().trim().to_string())
                        .unwrap_or_default(),
                    physical_cores: System::physical_core_count().ok(),
                    logical_cores: sys.cpus().len(),
                    mem_total: sys.total_memory(),
                };
                if tx.send(Msg::Info(info)).is_err() {
                    return;
                }
                ctx.request_repaint();

                // The thread was just primed above, so the first tick can already report.
                let mut primed = true;
                loop {
                    std::thread::sleep(TICK);
                    let now_ms = epoch.elapsed().as_millis() as u64;
                    if !is_visible(ping.load(Ordering::Relaxed), now_ms) {
                        primed = false;
                        continue;
                    }
                    sys.refresh_cpu_all();
                    if !primed {
                        // First reading after a pause averages over the whole pause.
                        primed = true;
                        continue;
                    }
                    sys.refresh_memory();

                    let freqs: Vec<u64> = sys.cpus().iter().map(|c| c.frequency()).collect();
                    let sample = Sample {
                        cpu: sys.global_cpu_usage(),
                        cores: sys.cpus().iter().map(|c| c.usage()).collect(),
                        cpu_mhz: mean_mhz(&freqs),
                        mem_used: sys.total_memory().saturating_sub(sys.available_memory()),
                        uptime_secs: System::uptime().unwrap_or(0),
                    };
                    if tx.send(Msg::Sample(sample)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        // If the thread cannot start, the channel just stays empty.
        let _ = spawned;

        Sampler {
            rx,
            last_ping,
            epoch,
        }
    }

    /// Tell the sampler the page is on screen right now. Call it from every `ui` frame.
    pub fn ping(&self) {
        let now_ms = self.epoch.elapsed().as_millis() as u64;
        self.last_ping.store(now_ms + 1, Ordering::Relaxed);
    }

    /// Everything the worker has sent since the last call, oldest first. Never blocks.
    pub fn drain(&self) -> Vec<Msg> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_pinged_is_not_visible() {
        assert!(!is_visible(0, 0));
        assert!(!is_visible(0, 10_000));
    }

    #[test]
    fn fresh_ping_is_visible() {
        // pinged at 1000 ms (stored 1001), now 1500 ms
        assert!(is_visible(1001, 1500));
        assert!(is_visible(1001, 1000));
    }

    #[test]
    fn limit_is_exclusive() {
        // pinged at 1000, elapsed 2499 -> visible; 2500 -> not
        assert!(is_visible(1001, 1000 + 2499));
        assert!(!is_visible(1001, 1000 + 2500));
    }

    #[test]
    fn stale_ping_is_not_visible() {
        assert!(!is_visible(1001, 60_000));
    }

    #[test]
    fn mean_mhz_cases() {
        assert_eq!(mean_mhz(&[]), 0);
        assert_eq!(mean_mhz(&[3000]), 3000);
        assert_eq!(mean_mhz(&[2000, 4000, 3000]), 3000);
    }

    #[test]
    fn real_sampler_smoke() {
        let sampler = Sampler::start(&egui::Context::default());
        for _ in 0..10 {
            sampler.ping();
            std::thread::sleep(Duration::from_millis(500));
        }
        let msgs = sampler.drain();

        let infos: Vec<&Info> = msgs
            .iter()
            .filter_map(|m| match m {
                Msg::Info(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(infos.len(), 1);
        let info = infos[0];
        assert!(info.logical_cores > 0);
        assert!(info.mem_total > 0);

        let samples: Vec<&Sample> = msgs
            .iter()
            .filter_map(|m| match m {
                Msg::Sample(s) => Some(s),
                _ => None,
            })
            .collect();
        assert!(!samples.is_empty());
        for s in samples {
            assert!((0.0..=100.0).contains(&s.cpu));
            assert_eq!(s.cores.len(), info.logical_cores);
            assert!(s.mem_used > 0 && s.mem_used <= info.mem_total);
        }
    }
}
