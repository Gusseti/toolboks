# Plan: bigger graphs, hover, long history, power

Status: plan only, nothing built. Requested 2026-10-30.

## 1. Graph fills the card, thin info bar at the bottom
- `ui::graph` today draws a fixed-height strip (`MINI_HEIGHT`, `IO_HEIGHT`) above a row of stats.
- New `ui::graph_card`: the graph fills the whole card rectangle. The title (left) and device name
  (right) are painted over the top edge, the stats sit in a slim translucent bar along the bottom
  (about 26 px). Card height stays the same, so the plotted area grows by roughly 60 percent.
- Applies to Overview (CPU, GPU, Memory, Disk, Network) and the Details cards.
- Text over the graph needs a scrim (gradient from the card colour) so lines never hide labels.

## 2. Hover read-out
- Vertical cursor line plus a dot on each line at the pointer, and a small tooltip: time ago
  ("2 min 10 s ago" or clock time) and the value of every line (for example Read 3.5 MB/s,
  Write 47 KB/s).
- Pure UI: `ui::graph` returns the hovered point index; the caller formats the values.
- Works with the long history below (the tooltip says which tier it comes from).

## 3. Longer history (memory only, nothing written)
- Today: 60 points at 1 s, so 1 minute.
- Tiers, all in memory: 1 s x 5 min, 10 s x 1 h (360 points), 1 min x 24 h (1440 points).
  Each tier is an average (and a max for spikes) of the finer tier. About 12 metrics x 2 000
  points x 4 bytes is under 100 KB.
- Range switch on each card or one switch for the page: 1 min, 5 min, 1 h, 4 h, 24 h.
- History is lost when toolboks closes (the standing "write nothing" rule). Tray mode keeps it
  running, which is the natural way to get 4 h and 24 h.

## 4. CPU temperature: why it is missing, what is possible
- Windows has no no-admin, no-driver API for CPU core temperature. GPU temperature works because
  NVIDIA's NVML exposes it.
- Measured on this PC: the Thermal Zone counter fails, there is no Energy Meter counter set, and
  the ACPI WMI thermal zone needs admin (and normally reports a motherboard zone, not the CPU).
- The real source (Intel MSR / AMD SMN) needs a kernel driver such as PawnIO or WinRing0. That
  breaks the single-exe rule, WinRing0 is on Microsoft's driver blocklist, and Defender flags
  tools that load it.
- Options: (a) leave it out and say so on the card; (b) with admin only, show the ACPI thermal
  zone labelled "Motherboard zone", never "CPU"; (c) later, a documented optional driver.
  Recommendation: (a), plus (b) behind the existing "Restart with permission" if it reads sanely.

## 5. Power graphs ("total wattage")
- Real today: GPU power from NVML (`power_w` is already sampled, not yet graphed).
- Not measurable without a driver: CPU package power (Intel RAPL needs the MSR driver), and
  whole-PC wall power. Desktops have no battery counter.
- Possible honest versions:
  - GPU power graph, plus GPU energy over the last 1 h / 4 h in Wh (sum of samples).
  - CPU power as a labelled ESTIMATE from load and the CPU's rated power, always shown with
    "estimate" and never added to the real GPU number without saying so.
  - Laptops: battery discharge rate is real; add it when a battery exists.
- Energy card: "Used in the last hour / 4 h" in Wh, with the part that is measured and the part
  that is estimated shown separately. Optional price per kWh (memory only).

## 6. Build order
1. Graph card layout + hover (UI only, safe, visible at once).
2. Tiered history + range switch.
3. GPU power graph and energy totals.
4. CPU estimate and temperature options, once the owner has chosen.

Each step: fmt, clippy -D warnings, tests, release build, look at the real window (when the
owner's PC is free), then commit on staging.
