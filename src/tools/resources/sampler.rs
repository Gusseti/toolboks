//! Background sampler for the Resources page. A worker thread reads CPU, memory and uptime about
//! once a second, but only while the page is on screen (the UI calls `ping()` every frame).

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::{DiskRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

use super::gpu::{GpuBasic, GpuReader};
use super::nvml::{Nvml, NvmlReading};
use windows::core::w;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

/// A ping older than this means the page is no longer shown.
const VISIBLE_WINDOW_MS: u64 = 2500;
const TICK: Duration = Duration::from_millis(1000);
/// Rows kept per top-processes list.
const TOP_ROWS: usize = 8;

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
    /// The main GPU, `None` if the PC has no usable GPU.
    pub gpu: Option<GpuSample>,
    /// The biggest users of CPU, memory, GPU and disk.
    pub top: TopProcesses,
    /// One entry per drive, for the Details page.
    pub drives: Vec<DriveInfo>,
    /// One entry per network adapter that counts (loopback and tunnel adapters excluded).
    pub adapters: Vec<AdapterInfo>,
    /// System-wide process, thread and handle counts, if Windows reports them.
    pub counts: Option<SystemCounts>,
}

/// A drive (disk volume) with its capacity and current throughput.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriveInfo {
    /// Mount point, e.g. "C:\\".
    pub mount: String,
    /// Volume label or device name, may be empty.
    pub label: String,
    /// "SSD", "HDD" or "" when unknown.
    pub kind: String,
    pub total: u64,
    pub free: u64,
    pub read_bps: f64,
    pub write_bps: f64,
}

/// A network adapter with its current speed and its totals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdapterInfo {
    pub name: String,
    pub down_bps: f64,
    pub up_bps: f64,
    /// Bytes received and sent since the counters started (as reported by sysinfo).
    pub total_down: u64,
    pub total_up: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemCounts {
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
}

/// The main GPU, merged from the generic readings and, on NVIDIA, the driver library.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuSample {
    pub name: String,
    /// Busiest engine, 0 to 100.
    pub load: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// Load per engine type (3D, Copy, Video Decode, ...), 0 to 100 each.
    pub engines: Vec<(String, f32)>,
    /// NVIDIA only; `None` on other GPUs or without the driver library.
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    pub clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
}

/// One row of a top-processes list: processes with the same name (case-insensitive) are merged.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcUse {
    /// The exe name as first seen, e.g. "chrome.exe".
    pub name: String,
    /// How many processes were merged into this row.
    pub count: usize,
    /// CPU and GPU: percent 0 to 100 of the whole machine; memory: working set in bytes;
    /// disk: read plus write bytes per second.
    pub value: f64,
}

/// The biggest users per resource, each list sorted largest first, at most 8 rows, zero values
/// left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopProcesses {
    pub cpu: Vec<ProcUse>,
    pub memory: Vec<ProcUse>,
    pub gpu: Vec<ProcUse>,
    pub disk: Vec<ProcUse>,
}

pub enum Msg {
    Info(Info),
    /// Boxed: a sample is large, and the enum would otherwise be as big as one.
    Sample(Box<Sample>),
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

/// Builds the adapter list from plain tuples `(name, rx_delta, tx_delta, total_rx, total_tx)`:
/// pseudo adapters are dropped, speeds are deltas over `secs`, sorted by name (case-insensitive).
fn build_adapters(items: &[(String, u64, u64, u64, u64)], secs: f64) -> Vec<AdapterInfo> {
    let mut list: Vec<AdapterInfo> = items
        .iter()
        .filter(|(name, ..)| is_counted_adapter(name))
        .map(|(name, rx, tx, total_rx, total_tx)| AdapterInfo {
            name: name.clone(),
            down_bps: bytes_per_second(*rx, secs),
            up_bps: bytes_per_second(*tx, secs),
            total_down: *total_rx,
            total_up: *total_tx,
        })
        .collect();
    list.sort_by_key(|a| a.name.to_lowercase());
    list
}

/// One drive as plain values: mount, label, kind, total, free, bytes read, bytes written.
type DriveRow = (String, String, String, u64, u64, u64, u64);

/// Builds the drive list from plain rows: drives with total 0 are skipped, speeds are deltas
/// over `secs`, sorted by mount.
fn build_drives(items: &[DriveRow], secs: f64) -> Vec<DriveInfo> {
    let mut list: Vec<DriveInfo> = items
        .iter()
        .filter(|(_, _, _, total, ..)| *total > 0)
        .map(
            |(mount, label, kind, total, free, read, written)| DriveInfo {
                mount: mount.clone(),
                label: label.clone(),
                kind: kind.clone(),
                total: *total,
                free: *free,
                read_bps: bytes_per_second(*read, secs),
                write_bps: bytes_per_second(*written, secs),
            },
        )
        .collect();
    list.sort_by(|a, b| a.mount.cmp(&b.mount));
    list
}

/// The process table is refreshed on every second reported sample (about every 2 s).
fn refresh_procs_now(sample_index: u64) -> bool {
    sample_index.is_multiple_of(2)
}

fn read_drives(disks: &Disks, secs: f64) -> Vec<DriveInfo> {
    let items: Vec<DriveRow> = disks
        .iter()
        .map(|d| {
            let u = d.usage();
            let kind = match d.kind() {
                sysinfo::DiskKind::SSD => "SSD",
                sysinfo::DiskKind::HDD => "HDD",
                _ => "",
            };
            (
                d.mount_point().to_string_lossy().into_owned(),
                d.name().to_string_lossy().into_owned(),
                kind.to_string(),
                d.total_space(),
                d.available_space(),
                u.read_bytes,
                u.written_bytes,
            )
        })
        .collect();
    build_drives(&items, secs)
}

fn read_adapters(nets: &Networks, secs: f64) -> Vec<AdapterInfo> {
    let items: Vec<_> = nets
        .iter()
        .map(|(name, n)| {
            (
                name.to_string_lossy().into_owned(),
                n.received(),
                n.transmitted(),
                n.total_received(),
                n.total_transmitted(),
            )
        })
        .collect();
    build_adapters(&items, secs)
}

/// System-wide process, thread and handle counts; None if the call fails.
fn read_counts() -> Option<SystemCounts> {
    let mut info = PERFORMANCE_INFORMATION::default();
    let cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    info.cb = cb;
    // SAFETY: `info` is a valid, writable PERFORMANCE_INFORMATION and `cb` is its exact size.
    unsafe { GetPerformanceInfo(&mut info, cb) }.ok()?;
    Some(SystemCounts {
        processes: info.ProcessCount,
        threads: info.ThreadCount,
        handles: info.HandleCount,
    })
}

/// Per-process CPU is percent of one core; this converts it to percent of the whole machine.
fn cpu_percent_of_machine(raw: f32, logical: usize) -> f64 {
    if logical == 0 || !raw.is_finite() {
        return 0.0;
    }
    (raw as f64 / logical as f64).clamp(0.0, 100.0)
}

/// Groups by name (case-insensitive, first-seen casing kept), sums values, counts merged items,
/// drops zero, negative and non-finite values, sorts largest first and keeps `n` rows.
fn top_n(items: impl IntoIterator<Item = (String, f64)>, n: usize) -> Vec<ProcUse> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut rows: Vec<ProcUse> = Vec::new();
    for (name, value) in items {
        if !value.is_finite() || value <= 0.0 {
            continue;
        }
        match index.entry(name.to_lowercase()) {
            Entry::Occupied(e) => {
                let r = &mut rows[*e.get()];
                r.count += 1;
                r.value += value;
            }
            Entry::Vacant(e) => {
                e.insert(rows.len());
                rows.push(ProcUse {
                    name,
                    count: 1,
                    value,
                });
            }
        }
    }
    rows.sort_by(|a, b| b.value.total_cmp(&a.value));
    rows.truncate(n);
    rows
}

fn refresh_procs(sys: &mut System) {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );
}

/// Builds the four top lists from the freshly refreshed process table. `secs` is the real time
/// since the previous refresh; `gpu_pids` is pid -> GPU percent.
fn build_top(sys: &System, logical: usize, secs: f64, gpu_pids: &[(u32, f32)]) -> TopProcesses {
    let name_of = |p: &sysinfo::Process| p.name().to_string_lossy().into_owned();
    let procs = || {
        sys.processes()
            .iter()
            .filter(|(pid, _)| pid.as_u32() != 0)
            .map(|(_, p)| p)
    };
    let cpu = top_n(
        procs().map(|p| (name_of(p), cpu_percent_of_machine(p.cpu_usage(), logical))),
        TOP_ROWS,
    );
    let memory = top_n(procs().map(|p| (name_of(p), p.memory() as f64)), TOP_ROWS);
    let disk = top_n(
        procs().map(|p| {
            let u = p.disk_usage();
            (
                name_of(p),
                bytes_per_second(u.read_bytes.saturating_add(u.written_bytes), secs),
            )
        }),
        TOP_ROWS,
    );
    // Sum per name first (clamped to 100), then rank; `count` counts processes using the GPU.
    let mut gpu_by_name: HashMap<String, (String, usize, f64)> = HashMap::new();
    for (pid, load) in gpu_pids {
        if let Some(p) = sys.process(sysinfo::Pid::from_u32(*pid)) {
            let name = name_of(p);
            let e = gpu_by_name
                .entry(name.to_lowercase())
                .or_insert((name, 0, 0.0));
            e.1 += 1;
            e.2 += *load as f64;
        }
    }
    let mut gpu = top_n(
        gpu_by_name
            .into_values()
            .map(|(name, _, v)| (name, v.min(100.0))),
        TOP_ROWS,
    );
    // top_n counted one per name; restore the real process counts.
    for row in &mut gpu {
        row.count = gpu_pids
            .iter()
            .filter(|(pid, _)| {
                sys.process(sysinfo::Pid::from_u32(*pid))
                    .is_some_and(|p| name_of(p).eq_ignore_ascii_case(&row.name))
            })
            .count();
    }
    TopProcesses {
        cpu,
        memory,
        gpu,
        disk,
    }
}

/// Clamps a counter value to 0..=100; NaN counts as 0.
fn clamp_percent(v: f64) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 100.0) as f32
    }
}

/// True when the GPU name or vendor says NVIDIA, so the driver library readings apply.
fn is_nvidia(basic: &GpuBasic) -> bool {
    let has = |s: &str| s.to_lowercase().contains("nvidia");
    has(&basic.name) || basic.vendor.as_deref().is_some_and(has)
}

/// Combines the generic GPU readings with the optional NVIDIA driver readings.
fn merge_gpu(basic: GpuBasic, nv: Option<NvmlReading>) -> GpuSample {
    let nv = nv.unwrap_or_default();
    GpuSample {
        name: basic.name,
        load: basic.load,
        mem_used: basic.mem_used,
        mem_total: basic.mem_total,
        engines: basic.engines,
        temp_c: nv.temp_c,
        power_w: nv.power_w,
        clock_mhz: nv.clock_mhz,
        mem_clock_mhz: nv.mem_clock_mhz,
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
                refresh_procs(&mut sys);

                let logical = sys.cpus().len();
                let info = Info {
                    cpu_name: sys
                        .cpus()
                        .first()
                        .map(|c| c.brand().trim().to_string())
                        .unwrap_or_default(),
                    physical_cores: System::physical_core_count().ok(),
                    logical_cores: logical,
                    mem_total: sys.total_memory(),
                };
                if tx.send(Msg::Info(info)).is_err() {
                    return;
                }
                ctx.request_repaint();

                let disk_kind = DiskRefreshKind::nothing()
                    .with_kind()
                    .with_storage()
                    .with_io_usage();
                let mut disks = Disks::new_with_refreshed_list_specifics(disk_kind).ok();
                let mut nets = Networks::new_with_refreshed_list().ok();
                let busy = DiskBusy::new();
                let mut gpu_reader = GpuReader::new();
                let nvml = Nvml::open();
                let mut last_tick = Instant::now();
                let mut last_proc = Instant::now();
                let mut top = TopProcesses::default();
                let mut sample_index: u64 = 0;

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
                        // Resets the GPU counter baseline; the reading itself is dropped.
                        refresh_procs(&mut sys);
                        let _ = gpu_reader.read();
                        last_tick = Instant::now();
                        last_proc = last_tick;
                        top = TopProcesses::default();
                        // The next sample refreshes the table (index 0).
                        sample_index = 0;
                        continue;
                    }
                    sys.refresh_memory();

                    let secs = last_tick.elapsed().as_secs_f64();
                    last_tick = Instant::now();
                    let (read, written, drives) = match disks.as_mut() {
                        Some(d) => {
                            d.refresh_specifics(true, disk_kind);
                            let (r, w) = disk_totals(d);
                            (r, w, read_drives(d, secs))
                        }
                        None => (0, 0, Vec::new()),
                    };
                    let (down, up, adapters) = match nets.as_mut() {
                        Some(n) => {
                            n.refresh(true);
                            let (d, u) = net_totals(n);
                            (d, u, read_adapters(n, secs))
                        }
                        None => (0, 0, Vec::new()),
                    };

                    let freqs: Vec<u64> = sys.cpus().iter().map(|c| c.frequency()).collect();
                    let mut gpu_basic = gpu_reader.read();
                    let gpu_pids = gpu_basic
                        .as_mut()
                        .map(|b| std::mem::take(&mut b.per_process))
                        .unwrap_or_default();
                    if refresh_procs_now(sample_index) {
                        refresh_procs(&mut sys);
                        let proc_secs = last_proc.elapsed().as_secs_f64();
                        last_proc = Instant::now();
                        top = build_top(&sys, logical, proc_secs, &gpu_pids);
                    }
                    sample_index += 1;
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
                        gpu: gpu_basic.map(|basic| {
                            let nv = if is_nvidia(&basic) {
                                nvml.as_ref().and_then(|n| n.read())
                            } else {
                                None
                            };
                            merge_gpu(basic, nv)
                        }),
                        top: top.clone(),
                        drives,
                        adapters,
                        counts: read_counts(),
                    };
                    if tx.send(Msg::Sample(Box::new(sample))).is_err() {
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
    fn adapters_filter_and_sort() {
        let items = vec![
            ("wi-fi".to_string(), 2000, 1000, 50, 60),
            ("Loopback Pseudo-Interface 1".to_string(), 9, 9, 9, 9),
            ("Ethernet".to_string(), 0, 500, 7, 8),
        ];
        let a = build_adapters(&items, 2.0);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].name, "Ethernet");
        assert_eq!(a[0].up_bps, 250.0);
        assert_eq!(a[1].name, "wi-fi");
        assert_eq!(a[1].down_bps, 1000.0);
        assert_eq!((a[1].total_down, a[1].total_up), (50, 60));
        assert!(build_adapters(&[], 1.0).is_empty());
        assert_eq!(build_adapters(&items, 0.0)[0].up_bps, 0.0);
    }

    #[test]
    fn drives_skip_empty_and_sort() {
        let d = |m: &str, total: u64| -> DriveRow {
            (m.into(), "L".into(), "SSD".into(), total, 5, 400, 200)
        };
        let l = build_drives(&[d("D:\\", 10), d("E:\\", 0), d("C:\\", 20)], 2.0);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].mount, "C:\\");
        assert_eq!(l[1].mount, "D:\\");
        assert_eq!(l[0].read_bps, 200.0);
        assert_eq!(l[0].write_bps, 100.0);
        assert_eq!((l[0].total, l[0].free), (20, 5));
    }

    #[test]
    fn process_refresh_every_second_sample() {
        let picks: Vec<bool> = (0..6).map(refresh_procs_now).collect();
        assert_eq!(picks, [true, false, true, false, true, false]);
    }

    #[test]
    fn counts_read_on_windows() {
        let c = read_counts().expect("GetPerformanceInfo");
        assert!(c.processes > 0 && c.threads > 0 && c.handles > 0);
    }

    #[test]
    fn clamp_percent_cases() {
        assert_eq!(clamp_percent(f64::NAN), 0.0);
        assert_eq!(clamp_percent(-5.0), 0.0);
        assert_eq!(clamp_percent(42.5), 42.5);
        assert_eq!(clamp_percent(250.0), 100.0);
    }

    fn it(name: &str, v: f64) -> (String, f64) {
        (name.to_string(), v)
    }

    #[test]
    fn cpu_percent_cases() {
        assert_eq!(cpu_percent_of_machine(100.0, 4), 25.0);
        assert_eq!(cpu_percent_of_machine(800.0, 4), 100.0);
        assert_eq!(cpu_percent_of_machine(-1.0, 4), 0.0);
        assert_eq!(cpu_percent_of_machine(50.0, 0), 0.0);
        assert_eq!(cpu_percent_of_machine(f32::NAN, 4), 0.0);
    }

    #[test]
    fn top_n_groups_case_insensitive() {
        let t = top_n(
            vec![
                it("Chrome.exe", 10.0),
                it("a.exe", 5.0),
                it("chrome.EXE", 7.0),
            ],
            8,
        );
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "Chrome.exe");
        assert_eq!(t[0].count, 2);
        assert_eq!(t[0].value, 17.0);
        assert_eq!(t[1].name, "a.exe");
        assert_eq!(t[1].count, 1);
    }

    #[test]
    fn top_n_orders_and_drops_bad_values() {
        let t = top_n(
            vec![
                it("low", 1.0),
                it("zero", 0.0),
                it("neg", -4.0),
                it("nan", f64::NAN),
                it("inf", f64::INFINITY),
                it("high", 9.0),
            ],
            8,
        );
        let names: Vec<&str> = t.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["high", "low"]);
        assert!(top_n(Vec::new(), 8).is_empty());
    }

    #[test]
    fn top_n_truncates() {
        let items: Vec<(String, f64)> = (1..=20).map(|i| (format!("p{i}"), i as f64)).collect();
        let t = top_n(items, 8);
        assert_eq!(t.len(), 8);
        assert_eq!(t[0].name, "p20");
        assert_eq!(t[7].name, "p13");
    }

    #[test]
    fn merge_gpu_cases() {
        let basic = GpuBasic {
            name: "NVIDIA GeForce".into(),
            vendor: Some("NVIDIA".into()),
            load: Some(40.0),
            mem_used: Some(1),
            mem_total: Some(2),
            engines: vec![("3D".into(), 40.0)],
            per_process: Vec::new(),
        };
        assert!(is_nvidia(&basic));
        let plain = merge_gpu(basic.clone(), None);
        assert_eq!(plain.name, "NVIDIA GeForce");
        assert_eq!(plain.load, Some(40.0));
        assert_eq!(plain.engines, basic.engines);
        assert_eq!(plain.temp_c, None);
        let nv = NvmlReading {
            temp_c: Some(55.0),
            power_w: Some(80.0),
            clock_mhz: Some(1800),
            mem_clock_mhz: Some(7000),
        };
        let full = merge_gpu(basic, Some(nv));
        assert_eq!(full.temp_c, Some(55.0));
        assert_eq!(full.power_w, Some(80.0));
        assert_eq!(full.clock_mhz, Some(1800));
        assert_eq!(full.mem_clock_mhz, Some(7000));
        let amd = GpuBasic {
            name: "Radeon".into(),
            vendor: Some("AMD".into()),
            ..Default::default()
        };
        assert!(!is_nvidia(&amd));
    }

    /// Prints a few live readings; run with `cargo test prints_live_samples -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn prints_live_samples() {
        let sampler = Sampler::start(&egui::Context::default());
        let mut shown = 0;
        {
            // Cost of the process refresh plus the top lists, measured on its own.
            let mut sys = System::new().unwrap();
            refresh_procs(&mut sys);
            let logical = sys.cpus().len().max(1);
            let mut total = Duration::ZERO;
            let runs = 10;
            for _ in 0..runs {
                std::thread::sleep(Duration::from_millis(300));
                let t = Instant::now();
                refresh_procs(&mut sys);
                let _ = build_top(&sys, logical, 0.3, &[]);
                total += t.elapsed();
            }
            println!(
                "process refresh + top lists: {:.2} ms avg over {runs}",
                total.as_secs_f64() * 1000.0 / runs as f64
            );
        }
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
                        println!("  gpu {:?}", s.gpu);
                        println!("  counts {:?}", s.counts);
                        for d in &s.drives {
                            println!("  drive {d:?}");
                        }
                        for a in &s.adapters {
                            println!("  adapter {a:?}");
                        }
                        for (label, list) in [
                            ("cpu %", &s.top.cpu),
                            ("mem B", &s.top.memory),
                            ("gpu %", &s.top.gpu),
                            ("disk B/s", &s.top.disk),
                        ] {
                            let rows: Vec<String> = list
                                .iter()
                                .take(3)
                                .map(|r| format!("{} x{} {:.1}", r.name, r.count, r.value))
                                .collect();
                            println!("  top {label}: {}", rows.join(" | "));
                        }
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
                Msg::Sample(s) => Some(&**s),
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
