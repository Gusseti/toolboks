# Resource Monitor plan

A second tool in toolboks: a Task Manager style overview of CPU, GPU, memory, disk and network with live graphs and numbers. Same design language as Memory Cleaner (bento cards, rounded corners, 12 px gaps). Status: **planned, not started**.

## Decisions

| Topic | Decision |
|---|---|
| Name | **Resource Monitor** (sidebar heading), pages **Overview** and **Details** |
| Scope of v1 | Overview plus Details (per-core CPU, GPU engines, per-drive disk, per-adapter network) |
| GPU depth | Basic (name, load, video memory) plus the engine split (3D, Copy, Video), plus NVIDIA extras (temperature, power, clocks) loaded from `nvml.dll` at runtime and skipped on other brands |
| Sampling | Only while the Resource Monitor page is visible and the window is not minimized. Graphs fill while you look and pause otherwise, so history has gaps |
| Persistence | None. History lives in memory and resets on every launch (single exe, no files) |
| Admin rights | Not needed. Some per-process figures for protected processes stay blank |
| CPU temperature | Not shown: Windows only exposes it through a kernel driver (this PC returned no readings) |

## Overview page (bento grid, like Memory Cleaner Home)

Top row: a wide **CPU** card (7 columns: usage graph, usage %, speed, cores/threads, up time) next to a column with **GPU** (load graph, load %, video memory used of total, NVIDIA temperature) and **Memory** (graph, % in use, GB in use). Second row: **Disk** (read and write lines, MB/s, active %) and **Network** (down and up lines, MB/s) at 6 columns each. Bottom: **Top processes** with pills for CPU, Memory, GPU, Disk and bars with values.

## Details page

CPU: one small graph per logical core plus overall, clock speed, process and thread counts. GPU: one graph per engine type (3D, Copy, Video Decode, Video Encode), video memory, plus NVIDIA temperature, power draw and clocks where available. Disk: one row per drive (capacity, free space, read and write speed, active time). Network: one row per adapter (down, up, totals since launch). Virtual and loopback adapters (for example the Parsec display adapter, loopback) are hidden.

## Data sources

| Metric | Source |
|---|---|
| CPU total and per core, frequency, name, up time, process count | `sysinfo` |
| Memory | existing `Sampler` and memory lists from Memory Cleaner |
| GPU name, vendor, load, video memory | `sysinfo` `gpu` feature (it has a Windows implementation using DXGI and the performance counters) |
| GPU engine split, per-process GPU | performance counters `GPU Engine(*)` through PDH, summed per engine type; overall load = busiest engine, as Task Manager shows it |
| NVIDIA temperature, power, clocks | `nvml.dll` loaded with `LoadLibrary` and `GetProcAddress`, no extra dependency; absent means the extras are simply hidden |
| Disk speed, active time, capacity | `sysinfo` disk usage deltas, PDH `PhysicalDisk` active time |
| Network speed per adapter | `sysinfo` network deltas |
| Top processes | `sysinfo` process CPU and memory; GPU and disk per process from PDH |

Counter names on this PC are localized (Norwegian). All PDH access goes through `PdhAddEnglishCounterW` and wildcard expansion, never through localized names.

## Architecture

- `src/tools/resources/` with `mod.rs` (the `Tool`, pages), `sampler.rs` (worker thread), `history.rs` (ring buffers), `gpu.rs`, `disk.rs`, `net.rs`, `cpu.rs`, `processes.rs`, `overview.rs`, `details.rs`.
- One sampler thread produces a snapshot per second and sends it over a channel; the UI never blocks. The thread runs only while the page is visible (the tool's `tick` and `ui` tell it).
- History: fixed-size ring buffers (default 60 points per series, about 1 minute), a few KB in total.
- `Sparkline` widget: painted with egui (filled area plus line, light grid, fixed 0-100 % for utilization, auto-scaled maximum for throughput). No charting crate.
- Shared UI module (`src/ui.rs`): the card, toggle switch and gauge helpers move out of `memory/home.rs` so both tools use the same look.
- Features added to `windows`: `Win32_System_Performance` (PDH) and `Win32_Graphics_Dxgi` where needed; `sysinfo` gains the `gpu` feature.

## Phases

1. Foundation (built, UI not looked at yet): sampler thread, history buffers, graph widget, shared UI module (`src/ui.rs`, `src/format.rs`), sidebar tool, Overview with CPU and Memory cards. Checked by fmt, clippy, unit tests (including a real sampler smoke test) and a release build only; nobody has seen the page on screen.
2. Disk and Network cards.
3. GPU card: `sysinfo` basics, then the engine split, then NVIDIA extras.
4. Top processes card with CPU, Memory, GPU and Disk pills.
5. Details page.
6. Polish, tests, real-window screenshots, release (new version, minor bump).

## Budgets

Exe growth under 0.5 MB; under 1 % CPU while the page is open at 1 s sampling; a few MB of extra RAM; near zero cost when the page is not visible.

## Tests and verification

Unit tests for ring buffers, axis auto-scaling, rate and size formatting, GPU instance-name parsing and per-engine aggregation, adapter filtering. Real-window screenshots (topmost window method) for both pages. Compare GPU, CPU and disk numbers against Task Manager side by side before release.

## Risks

- GPU percentage must match Task Manager (busiest engine, not an average).
- Virtual adapters and per-process counters can be noisy; filter and cache the wildcard expansion so it is not redone every second.
- A cold PDH query returns nothing on the first sample; rates need two samples.
- Protected processes show no GPU or disk numbers without admin.
- CPU temperature is intentionally absent.
