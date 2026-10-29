//! Background sampler for the Resources page. A worker thread reads CPU, memory and uptime about
//! once a second, but only while the page is on screen (the UI calls `ping()` every frame).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::{DiskRefreshKind, Disks, Networks, System};
use windows::core::w;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};

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
    /// Bytes per second read from and written to all drives since the previous sample.
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    /// Share of time the drives were busy, 0 to 100, if the Windows counter is available.
    pub disk_active: Option<f32>,
    /// Bytes per second received and sent over all network adapters (loopback and pseudo
    /// tunnel adapters excluded) since the previous sample.
    pub net_down_bps: f64,
    pub net_up_bps: f64,
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

/// Bytes over a span of seconds; zero when the span is not a usable positive number.
fn bytes_per_second(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 || !secs.is_finite() {
        0.0
    } else {
        bytes as f64 / secs
    }
}

/// False for loopback and pseudo tunnel adapters, which would double count or add noise.
fn is_counted_adapter(name: &str) -> bool {
    let lower = name.to_lowercase();
    !["loopback", "isatap", "teredo", "6to4"]
        .iter()
        .any(|bad| lower.contains(bad))
}

/// Clamps a counter value to 0..=100; NaN counts as 0.
fn clamp_percent(v: f64) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 100.0) as f32
    }
}

/// A PDH query with one counter for total physical disk busy time. Closed on drop.
struct DiskBusy {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl DiskBusy {
    fn new() -> Option<DiskBusy> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is a valid out pointer; a null data source means live data.
        let status = unsafe { PdhOpenQueryW(None, 0, &mut query) };
        if status != 0 {
            return None;
        }
        let mut counter = PDH_HCOUNTER::default();
        // SAFETY: `query` was opened above and `counter` is a valid out pointer. The English
        // API is used so the path works on any display language.
        let status = unsafe {
            PdhAddEnglishCounterW(
                query,
                w!("\\PhysicalDisk(_Total)\\% Disk Time"),
                0,
                &mut counter,
            )
        };
        if status != 0 {
            // SAFETY: `query` is a valid open handle that nothing else owns.
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        let busy = DiskBusy { query, counter };
        busy.collect();
        Some(busy)
    }

    /// Takes a new reading of the query data. Returns false on failure.
    fn collect(&self) -> bool {
        // SAFETY: `self.query` is a valid open handle for the life of `self`.
        unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    /// Collects and reads the busy share, 0 to 100. None if any call fails or no value yet.
    fn read(&self) -> Option<f32> {
        if !self.collect() {
            return None;
        }
        let mut value = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: `self.counter` belongs to `self.query`, which is open; `value` is a valid out
        // pointer.
        let status =
            unsafe { PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut value) };
        if status != 0 || value.CStatus != 0 {
            return None;
        }
        // SAFETY: the format was PDH_FMT_DOUBLE and the status was success, so the double field
        // is the one that was written.
        let v = unsafe { value.Anonymous.doubleValue };
        Some(clamp_percent(v))
    }
}

impl Drop for DiskBusy {
    fn drop(&mut self) {
        // SAFETY: `self.query` is a valid open handle, closed only here.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Sums bytes read and written by all disks since the previous refresh.
fn disk_totals(disks: &Disks) -> (u64, u64) {
    disks.iter().fold((0, 0), |(r, w), d| {
        let u = d.usage();
        (r + u.read_bytes, w + u.written_bytes)
    })
}

/// Sums bytes received and sent by counted adapters since the previous refresh.
fn net_totals(nets: &Networks) -> (u64, u64) {
    nets.iter()
        .filter(|(name, _)| is_counted_adapter(&name.to_string_lossy()))
        .fold((0, 0), |(d, u), (_, n)| {
            (d + n.received(), u + n.transmitted())
        })
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

                let disk_kind = DiskRefreshKind::nothing().with_io_usage();
                let mut disks = Disks::new_with_refreshed_list_specifics(disk_kind).ok();
                let mut nets = Networks::new_with_refreshed_list().ok();
                let busy = DiskBusy::new();
                let mut last_tick = Instant::now();

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
                        if let Some(d) = disks.as_mut() {
                            d.refresh_specifics(true, disk_kind);
                        }
                        if let Some(n) = nets.as_mut() {
                            n.refresh(true);
                        }
                        if let Some(b) = busy.as_ref() {
                            b.collect();
                        }
                        last_tick = Instant::now();
                        continue;
                    }
                    sys.refresh_memory();

                    let secs = last_tick.elapsed().as_secs_f64();
                    last_tick = Instant::now();
                    let (read, written) = match disks.as_mut() {
                        Some(d) => {
                            d.refresh_specifics(true, disk_kind);
                            disk_totals(d)
                        }
                        None => (0, 0),
                    };
                    let (down, up) = match nets.as_mut() {
                        Some(n) => {
                            n.refresh(true);
                            net_totals(n)
                        }
                        None => (0, 0),
                    };

                    let freqs: Vec<u64> = sys.cpus().iter().map(|c| c.frequency()).collect();
                    let sample = Sample {
                        cpu: sys.global_cpu_usage(),
                        cores: sys.cpus().iter().map(|c| c.usage()).collect(),
                        cpu_mhz: mean_mhz(&freqs),
                        mem_used: sys.total_memory().saturating_sub(sys.available_memory()),
                        uptime_secs: System::uptime().unwrap_or(0),
                        disk_read_bps: bytes_per_second(read, secs),
                        disk_write_bps: bytes_per_second(written, secs),
                        disk_active: busy.as_ref().and_then(|b| b.read()),
                        net_down_bps: bytes_per_second(down, secs),
                        net_up_bps: bytes_per_second(up, secs),
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
    fn bytes_per_second_cases() {
        assert_eq!(bytes_per_second(1000, 2.0), 500.0);
        assert_eq!(bytes_per_second(1000, 0.0), 0.0);
        assert_eq!(bytes_per_second(1000, -1.0), 0.0);
        assert_eq!(bytes_per_second(1000, f64::NAN), 0.0);
        assert_eq!(bytes_per_second(1000, f64::INFINITY), 0.0);
    }

    #[test]
    fn adapter_filter() {
        assert!(is_counted_adapter("Ethernet"));
        assert!(is_counted_adapter("Wi-Fi"));
        assert!(!is_counted_adapter("Loopback Pseudo-Interface 1"));
        assert!(!is_counted_adapter("isatap.{ABC}"));
        assert!(!is_counted_adapter("Teredo Tunneling Pseudo-Interface"));
        assert!(!is_counted_adapter("6TO4 Adapter"));
    }

    #[test]
    fn clamp_percent_cases() {
        assert_eq!(clamp_percent(f64::NAN), 0.0);
        assert_eq!(clamp_percent(-5.0), 0.0);
        assert_eq!(clamp_percent(42.5), 42.5);
        assert_eq!(clamp_percent(250.0), 100.0);
    }

    /// Prints a few live readings; run with `cargo test prints_live_samples -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn prints_live_samples() {
        let sampler = Sampler::start(&egui::Context::default());
        let mut shown = 0;
        for _ in 0..16 {
            sampler.ping();
            std::thread::sleep(Duration::from_millis(500));
            for msg in sampler.drain() {
                match msg {
                    Msg::Info(i) => println!("{i:?}"),
                    Msg::Sample(s) => {
                        shown += 1;
                        println!(
                            "cpu {:.1}%  {} MHz  mem {} MiB  disk r {:.0} w {:.0} B/s busy {:?}  net down {:.0} up {:.0} B/s",
                            s.cpu,
                            s.cpu_mhz,
                            s.mem_used / (1024 * 1024),
                            s.disk_read_bps,
                            s.disk_write_bps,
                            s.disk_active,
                            s.net_down_bps,
                            s.net_up_bps
                        );
                    }
                }
            }
        }
        assert!(shown > 0);
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
            for v in [
                s.disk_read_bps,
                s.disk_write_bps,
                s.net_down_bps,
                s.net_up_bps,
            ] {
                assert!(v.is_finite() && v >= 0.0);
            }
            if let Some(a) = s.disk_active {
                assert!((0.0..=100.0).contains(&a));
            }
        }
    }
}
