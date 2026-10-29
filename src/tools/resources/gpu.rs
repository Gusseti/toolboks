//! Main-GPU readings: name, memory and vendor from sysinfo, load and per-engine split from the
//! Windows "GPU Engine" performance counters (the same source Task Manager uses).

use std::collections::HashMap;

use sysinfo::Gpus;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

/// Basic readings for the main GPU, from sysinfo's `gpu` feature plus the Windows GPU counters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuBasic {
    pub name: String,
    pub vendor: Option<String>,
    /// Busiest engine, 0 to 100 (what Task Manager shows as GPU load).
    pub load: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// Load per engine type, e.g. ("3D", 21.0), ("Copy", 0.0), ("Video Decode", 3.0), 0 to 100.
    pub engines: Vec<(String, f32)>,
    /// GPU load per process id on the main adapter, 0 to 100 (busiest engine of that process),
    /// zeros dropped.
    pub per_process: Vec<(u32, f32)>,
}

/// Splits a counter instance name such as
/// `pid_10136_luid_0x00000000_0x000106DB_phys_0_eng_0_engtype_3D` into
/// (luid, engine index, engine type). `None` if it does not have that shape.
pub fn parse_engine_name(name: &str) -> Option<(String, u32, String)> {
    let after_luid = name.split_once("_luid_")?.1;
    let (luid, rest) = after_luid.split_once("_phys_")?;
    let after_eng = rest.split_once("_eng_")?.1;
    let (idx, ty) = after_eng.split_once("_engtype_")?;
    let idx: u32 = idx.parse().ok()?;
    if luid.is_empty() || ty.is_empty() {
        return None;
    }
    Some((luid.to_string(), idx, ty.to_string()))
}

/// "VideoDecode" -> "Video Decode", "Compute_0" -> "Compute", "3D" stays "3D".
pub fn prettify_engine(ty: &str) -> String {
    let base = match ty.rsplit_once('_') {
        Some((head, tail)) if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => ty,
    };
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in base.chars() {
        if c.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase()) {
            out.push(' ');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Result of folding the counter instances of the main adapter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineSummary {
    pub luid: Option<String>,
    pub load: Option<f32>,
    pub engines: Vec<(String, f32)>,
}

/// Sums values per (luid, engine) across processes (each capped at 100), picks the luid with the
/// most distinct engines as the main adapter, takes the max per engine type, and the busiest
/// type as the overall load.
pub fn aggregate_engines(items: &[(String, f64)]) -> EngineSummary {
    let mut sums: HashMap<(String, u32, String), f64> = HashMap::new();
    for (name, value) in items {
        let Some((luid, idx, ty)) = parse_engine_name(name) else {
            continue;
        };
        let v = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        *sums.entry((luid, idx, ty)).or_insert(0.0) += v;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (luid, _, _) in sums.keys() {
        *counts.entry(luid.as_str()).or_insert(0) += 1;
    }
    // Most engines wins; on a tie the smaller luid string, so the choice is stable.
    let Some(main) = counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(l, _)| l.to_string())
    else {
        return EngineSummary::default();
    };
    let mut by_type: HashMap<String, f32> = HashMap::new();
    for ((luid, _, ty), sum) in &sums {
        if *luid != main {
            continue;
        }
        let v = sum.min(100.0) as f32;
        let e = by_type.entry(prettify_engine(ty)).or_insert(0.0);
        if v > *e {
            *e = v;
        }
    }
    let load = by_type.values().copied().fold(0.0_f32, f32::max);
    let mut engines: Vec<(String, f32)> = by_type.into_iter().collect();
    engines.sort_by(|a, b| a.0.cmp(&b.0));
    EngineSummary {
        luid: Some(main),
        load: Some(load),
        engines,
    }
}

/// The process id from an instance name starting with `pid_<n>_`.
pub fn parse_engine_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    rest.split_once('_')?.0.parse().ok()
}

/// GPU load per process on the adapter `luid`: per pid and engine type the max over that pid's
/// engines of the type, then the busiest type, clamped to 0..=100, zeros dropped. Sorted by pid.
pub fn per_process_load(items: &[(String, f64)], luid: &str) -> Vec<(u32, f32)> {
    let mut by_type: HashMap<(u32, String), f64> = HashMap::new();
    for (name, value) in items {
        let (Some(pid), Some((l, _, ty))) = (parse_engine_pid(name), parse_engine_name(name))
        else {
            continue;
        };
        if l != luid {
            continue;
        }
        let v = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        let e = by_type.entry((pid, prettify_engine(&ty))).or_insert(0.0);
        if v > *e {
            *e = v;
        }
    }
    let mut by_pid: HashMap<u32, f64> = HashMap::new();
    for ((pid, _), v) in by_type {
        let e = by_pid.entry(pid).or_insert(0.0);
        if v > *e {
            *e = v;
        }
    }
    let mut out: Vec<(u32, f32)> = by_pid
        .into_iter()
        .map(|(pid, v)| (pid, v.min(100.0) as f32))
        .filter(|(_, v)| *v > 0.0)
        .collect();
    out.sort_by_key(|(pid, _)| *pid);
    out
}

/// A PDH query with one wildcard counter (GPU engine, adapter memory). Closed on drop.
struct PdhCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    collects: u32,
}

impl PdhCounter {
    fn new(path: PCWSTR) -> Option<PdhCounter> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is a valid out pointer; a null data source means live data.
        let status = unsafe { PdhOpenQueryW(None, 0, &mut query) };
        if status != 0 {
            return None;
        }
        let mut counter = PDH_HCOUNTER::default();
        // SAFETY: `query` was opened above and `counter` is a valid out pointer. The English
        // API is used so the path works on any display language.
        let status = unsafe { PdhAddEnglishCounterW(query, path, 0, &mut counter) };
        if status != 0 {
            // SAFETY: `query` is a valid open handle that nothing else owns.
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        let mut c = PdhCounter {
            query,
            counter,
            collects: 0,
        };
        c.collect();
        Some(c)
    }

    fn collect(&mut self) -> bool {
        // SAFETY: `self.query` is a valid open handle for the life of `self`.
        let ok = unsafe { PdhCollectQueryData(self.query) == 0 };
        if ok {
            self.collects += 1;
        }
        ok
    }

    /// Instance names with their values. `None` if a call fails.
    fn values(&self) -> Option<Vec<(String, f64)>> {
        let mut size: u32 = 0;
        let mut count: u32 = 0;
        // SAFETY: `self.counter` belongs to the open query; a null buffer with size 0 asks for
        // the required size.
        let mut status = unsafe {
            PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None)
        };
        // The instance list can grow between calls, so retry a few times with a fresh size.
        for _ in 0..4 {
            if status != PDH_MORE_DATA {
                break;
            }
            // u64 words keep the buffer 8-byte aligned for the item structs; pad for safety.
            let mut buf: Vec<u64> = vec![0; size as usize / 8 + 2];
            let mut size_bytes = (buf.len() * 8) as u32;
            // SAFETY: `buf` is at least `size_bytes` long and aligned for the item struct;
            // PDH writes up to `count` items plus their name strings inside that buffer.
            status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size_bytes,
                    &mut count,
                    Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
                )
            };
            if status == 0 {
                let items = buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
                let mut out = Vec::with_capacity(count as usize);
                for i in 0..count as usize {
                    // SAFETY: PDH reported `count` initialised items at the start of `buf`, and
                    // each `szName` points to a NUL-terminated string inside `buf`, which is
                    // still alive here.
                    let item = unsafe { &*items.add(i) };
                    let Ok(name) = (unsafe { item.szName.to_string() }) else {
                        continue;
                    };
                    // 0 = valid data, 1 = new data; anything else is not usable.
                    if item.FmtValue.CStatus > 1 {
                        continue;
                    }
                    // SAFETY: the format was PDH_FMT_DOUBLE and the status was success, so the
                    // double field is the one that was written.
                    let v = unsafe { item.FmtValue.Anonymous.doubleValue };
                    out.push((name, v));
                }
                return Some(out);
            }
            if status == PDH_MORE_DATA {
                // Grow with some headroom for instances that appeared meanwhile.
                size = size_bytes.saturating_add(size_bytes / 4 + 1024);
            }
        }
        None
    }
}

impl Drop for PdhCounter {
    fn drop(&mut self) {
        // SAFETY: `self.query` is a valid open handle, closed only here.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// Dedicated video memory in use (bytes) for the adapter whose instance name contains `luid`,
/// e.g. `luid_0x00000000_0x000106DB_phys_0`. Several matches (multiple phys) are summed.
pub fn adapter_value(items: &[(String, f64)], luid: &str) -> Option<u64> {
    if luid.is_empty() {
        return None;
    }
    let mut found = false;
    let mut total = 0.0_f64;
    for (name, v) in items {
        if name.contains(luid) && v.is_finite() && *v >= 0.0 {
            found = true;
            total += v;
        }
    }
    found.then_some(total as u64)
}

/// Counter value first, then a non-zero sysinfo value, else unknown (never a fake 0).
pub fn pick_mem_used(counter: Option<u64>, sysinfo: Option<u64>) -> Option<u64> {
    counter.or(sysinfo.filter(|n| *n > 0))
}

pub struct GpuReader {
    gpus: Option<Gpus>,
    counter: Option<PdhCounter>,
    mem_counter: Option<PdhCounter>,
}

impl GpuReader {
    pub fn new() -> Self {
        GpuReader {
            gpus: Gpus::new_with_refreshed_list().ok(),
            counter: PdhCounter::new(w!("\\GPU Engine(*)\\Utilization Percentage")),
            mem_counter: PdhCounter::new(w!("\\GPU Adapter Memory(*)\\Dedicated Usage")),
        }
    }

    /// One reading, or `None` if there is no usable GPU. Called about once a second from the
    /// sampler thread, never from the UI thread.
    pub fn read(&mut self) -> Option<GpuBasic> {
        let gpus = self.gpus.as_mut()?;
        gpus.refresh(true);
        let main = gpus.iter().max_by_key(|g| g.total_memory().unwrap_or(0))?;
        let mut basic = GpuBasic {
            name: main.model().unwrap_or("GPU").trim().to_string(),
            vendor: main.vendor().map(|v| v.trim().to_string()),
            load: main.usage().map(|u| u.clamp(0.0, 100.0)),
            mem_used: main.used_memory(),
            mem_total: main.total_memory(),
            engines: Vec::new(),
            per_process: Vec::new(),
        };

        let mut main_luid: Option<String> = None;
        if let Some(counter) = self.counter.as_mut() {
            let collected = counter.collect();
            if collected && counter.collects >= 2 {
                if let Some(items) = counter.values() {
                    let summary = aggregate_engines(&items);
                    basic.load = summary.load;
                    basic.engines = summary.engines;
                    if let Some(l) = summary.luid.as_deref() {
                        basic.per_process = per_process_load(&items, l);
                    }
                    main_luid = summary.luid;
                }
            } else {
                // Counter not primed yet (or the collect failed): no load until a real delta.
                basic.load = None;
            }
        }

        let mut counter_mem = None;
        if let (Some(mc), Some(luid)) = (self.mem_counter.as_mut(), main_luid.as_deref()) {
            if mc.collect() {
                if let Some(items) = mc.values() {
                    counter_mem = adapter_value(&items, luid);
                }
            }
        }
        basic.mem_used = pick_mem_used(counter_mem, basic.mem_used);
        Some(basic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N3D: &str = "pid_10136_luid_0x00000000_0x000106DB_phys_0_eng_0_engtype_3D";

    #[test]
    fn parse_good_names() {
        assert_eq!(
            parse_engine_name(N3D),
            Some(("0x00000000_0x000106DB".into(), 0, "3D".into()))
        );
        assert_eq!(
            parse_engine_name("pid_4_luid_0x00000000_0x000106DB_phys_0_eng_3_engtype_Compute_0"),
            Some(("0x00000000_0x000106DB".into(), 3, "Compute_0".into()))
        );
    }

    #[test]
    fn parse_bad_names() {
        assert_eq!(parse_engine_name(""), None);
        assert_eq!(parse_engine_name("_Total"), None);
        assert_eq!(
            parse_engine_name("pid_1_luid_x_phys_0_eng_z_engtype_3D"),
            None
        );
    }

    #[test]
    fn prettify_cases() {
        assert_eq!(prettify_engine("3D"), "3D");
        assert_eq!(prettify_engine("Copy"), "Copy");
        assert_eq!(prettify_engine("VideoDecode"), "Video Decode");
        assert_eq!(prettify_engine("VideoEncode"), "Video Encode");
        assert_eq!(prettify_engine("Compute_0"), "Compute");
        assert_eq!(prettify_engine("VideoProcessing"), "Video Processing");
    }

    fn n(luid: &str, pid: u32, eng: u32, ty: &str) -> String {
        format!("pid_{pid}_luid_{luid}_phys_0_eng_{eng}_engtype_{ty}")
    }

    #[test]
    fn aggregate_empty_and_garbage() {
        assert_eq!(aggregate_engines(&[]), EngineSummary::default());
        assert_eq!(
            aggregate_engines(&[("junk".into(), 5.0)]),
            EngineSummary::default()
        );
    }

    #[test]
    fn aggregate_main_luid_and_types() {
        let main = "0x0_0xA";
        let virt = "0x0_0xB";
        let items = vec![
            // two processes on 3D engine 0: summed 30
            (n(main, 1, 0, "3D"), 10.0),
            (n(main, 2, 0, "3D"), 20.0),
            // another 3D engine at 5: type max stays 30
            (n(main, 1, 1, "3D"), 5.0),
            (n(main, 1, 2, "Copy"), 2.0),
            (n(main, 1, 3, "VideoDecode"), 7.0),
            // over-subscribed sums cap at 100
            (n(main, 1, 4, "Compute_0"), 80.0),
            (n(main, 2, 4, "Compute_0"), 80.0),
            // virtual adapter: one engine, ignored even though busier
            (n(virt, 1, 0, "3D"), 99.0),
        ];
        let s = aggregate_engines(&items);
        assert_eq!(s.luid.as_deref(), Some(main));
        assert_eq!(s.load, Some(100.0));
        assert_eq!(
            s.engines,
            vec![
                ("3D".to_string(), 30.0),
                ("Compute".to_string(), 100.0),
                ("Copy".to_string(), 2.0),
                ("Video Decode".to_string(), 7.0),
            ]
        );
    }

    #[test]
    fn adapter_value_matches_luid() {
        let items = vec![
            ("luid_0x00000000_0x000106DB_phys_0".to_string(), 3.0e9),
            ("luid_0x00000000_0x0000AAAA_phys_0".to_string(), 5.0e8),
        ];
        assert_eq!(
            adapter_value(&items, "0x00000000_0x000106DB"),
            Some(3_000_000_000)
        );
        assert_eq!(adapter_value(&items, "0x00000000_0x0000BEEF"), None);
        assert_eq!(adapter_value(&items, ""), None);
        assert_eq!(adapter_value(&[], "0x0_0x1"), None);
    }

    #[test]
    fn pick_mem_used_order() {
        assert_eq!(pick_mem_used(Some(7), Some(9)), Some(7));
        assert_eq!(pick_mem_used(Some(0), Some(9)), Some(0));
        assert_eq!(pick_mem_used(None, Some(9)), Some(9));
        assert_eq!(pick_mem_used(None, Some(0)), None);
        assert_eq!(pick_mem_used(None, None), None);
    }

    #[test]
    fn parse_pid_cases() {
        assert_eq!(parse_engine_pid(N3D), Some(10136));
        assert_eq!(parse_engine_pid("pid_x_luid"), None);
        assert_eq!(parse_engine_pid("pid_5"), None);
        assert_eq!(parse_engine_pid("_Total"), None);
        assert_eq!(parse_engine_pid(""), None);
    }

    #[test]
    fn per_process_cases() {
        let main = "0x0_0xA";
        let items = vec![
            // pid 1: 3D engines 0 and 1 -> max 20 (not summed); Copy 5
            (n(main, 1, 0, "3D"), 20.0),
            (n(main, 1, 1, "3D"), 10.0),
            (n(main, 1, 2, "Copy"), 5.0),
            // pid 2: only zero -> dropped
            (n(main, 2, 0, "3D"), 0.0),
            // pid 3: over 100 -> clamped
            (n(main, 3, 0, "Compute_0"), 250.0),
            // other adapter ignored
            (n("0x0_0xB", 4, 0, "3D"), 50.0),
            // NaN counts as zero
            (n(main, 5, 0, "3D"), f64::NAN),
            ("junk".to_string(), 9.0),
        ];
        assert_eq!(per_process_load(&items, main), vec![(1, 20.0), (3, 100.0)]);
        assert!(per_process_load(&items, "nope").is_empty());
    }

    #[test]
    fn aggregate_nan_and_negative_count_as_zero() {
        let items = vec![
            (n("0x0_0xA", 1, 0, "3D"), f64::NAN),
            (n("0x0_0xA", 1, 1, "Copy"), -3.0),
        ];
        let s = aggregate_engines(&items);
        assert_eq!(s.load, Some(0.0));
    }
}
