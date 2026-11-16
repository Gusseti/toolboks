# What Toolboks itself costs (issue #48)

Measured 2026-11-16. Toolboks is a small shell (the host) and every tool is a separate add-on
process, so the host and each add-on are measured and reported separately.

## Machine and method

- Intel Core i7-10700K (8 cores, 16 logical), 32 GB RAM, NVIDIA GeForce RTX 3080, Windows 11 Pro.
  The PC was in normal desktop use by its owner during the runs (other apps open).
- Build: `cargo build --release --features preview` of the host, before and after the change, each
  with its own fresh data folder (`TOOLBOKS_DATA_DIR`), so the owner's own Toolboks was never
  touched. A preview build never checks for app updates; it does fetch the add-on list as usual.
- Script: [`scripts/measure-host-overhead.ps1`](../scripts/measure-host-overhead.ps1). It starts the
  exe, puts the window in each state, waits 15 s, then measures for 60 s:
  - **open**: the window is shown and left alone (Home page, or the add-on page given with `-Open`).
  - **minimized**: the window is minimized to the taskbar.
  - **tray**: the window's X is pressed (WM_CLOSE) with "Keep running in the tray" on, so only the
    tray icon is left. Windows calls this window hidden.
- Figures per process: CPU time used during the 60 s as a share of the whole PC (all 16 logical
  cores, as Task Manager shows it) and of one core; wake-ups per second (context switches of the
  process's threads); working set, private bytes, handles and threads (averaged, sampled every
  second). "Pointer over window" says how often the mouse pointer happened to rest over the window
  (it was not moved by the script; no clicks or keys were sent).
- Resolution: Windows counts CPU time in ticks of about 15.6 ms, so 0.002% of the PC over 60 s is
  one tick. Values of 0.000 to 0.003% mean "one tick or less" and are the same within noise.
- With an add-on: Resource Monitor 0.1.0 (a page add-on, not background) with its page open, and
  Memory Cleaner 0.1.0 (a background add-on; automatic cleaning is off by default and stayed off)
  running while the window shows Home. Both were copied from an installed copy into the fresh data
  folder; no preferences were copied.

## Results: host process (CPU % of the whole PC)

| Scenario | State | Before | After | Wake-ups/s before → after |
|---|---|---:|---:|---:|
| No add-ons | open | 0.010 | 0.005 | 28.9 → 14.2 |
| No add-ons | minimized | 0.015 | 0.002 | 24.5 → 12.3 |
| No add-ons | tray | 0.000 | 0.002 | 10.5 → 12.6 |
| Memory Cleaner in the background | open (Home) | **0.205** | **0.010** | 125.3 → 17.1 |
| Memory Cleaner in the background | minimized | 0.002 | 0.000 | 13.8 → 13.2 |
| Memory Cleaner in the background | tray | 0.000 | 0.002 | 15.5 → 14.0 |
| Resource Monitor page open | open | **0.334, 0.420** (two runs) | **0.195, 0.238** (two runs) | 299 / 135 → 76 / 129 |
| Resource Monitor page open | minimized | 0.002, 0.000 | 0.010, 0.003 | 14.0 / 14.3 → 125 / 14.5 |
| Resource Monitor page open | tray | 0.002 | 0.005 | 15.7 → 13.1 |

Memory of the host, all runs: working set 75 to 90 MB, private bytes 80 to 100 MB (about 80 MB
with no add-on, about 97 MB with Resource Monitor's page drawn). Handles 450 to 500, threads 6 to 16.
The change did not move memory.

The one high "after, minimized" value with Resource Monitor (0.010%, 125 wake-ups/s) did not
repeat: the second run of the same case gave 0.003% and 14.5 wake-ups/s, the same as before.

## Results: add-on processes (CPU % of the whole PC)

| Add-on | State | Before | After | Working set / private MB |
|---|---|---:|---:|---|
| Resource Monitor | page open | 0.029, 0.028 | 0.047, 0.063 | 46 / 33 |
| Resource Monitor | minimized | 0.000, 0.005 | 0.002, 0.002 | 46 / 33 |
| Resource Monitor | tray | 0.000 | 0.000 | 46 / 33 |
| Memory Cleaner (background) | any | 0.000 to 0.005 | 0.000 | 13 / 5 |

Resource Monitor's own cost with its page open rose a little after the change (0.03 → 0.05 to
0.06%), while the host's fell by more (0.33 to 0.42 → 0.20 to 0.24%), so the total for that page,
host plus add-on, went down from about 0.41% (0.36 and 0.45 in the two runs) to about 0.27% (0.24
and 0.30) of the PC. Why the add-on's share rose is not known; the
host now answers its repaint requests when they are due instead of on its own 100 ms beat.

## Against the budget

The issue proposed "under 0.2% CPU and no repaint while hidden". That was a proposal, not a measured
guarantee. Measured:

| State | Host CPU (after) | Under 0.2%? | Repaint while hidden? |
|---|---:|---|---|
| tray | 0.000 to 0.005% | yes, before and after | No. eframe 0.37 runs only `App::logic` for a hidden window: no UI pass and no painting. Toolboks wakes every 2 s (was every 1 s) to refresh the tray icon. |
| minimized | 0.000 to 0.010% | yes, before and after | The window is minimized, not hidden; the numbers show it stays idle (one CPU tick or less per minute). |
| open, no add-on page | 0.005 to 0.010% | yes; before it was 0.205% as soon as any add-on (also a background one) had connected | the window is on screen; it repaints only when something changes or a timer is due |
| open, Resource Monitor page | 0.195 to 0.238% | about at the line | the page draws live graphs from the add-on; this is the cost of showing them |

## What was found and changed

1. **Every add-on view made the host redraw 10 times a second, forever** (`poll_marketplace` in
   `src/app/marketplace.rs`: `if !self.views.is_empty() { request_repaint_after(100 ms) }`). A view
   is created for every add-on that connects, background add-ons included, and kept after its page
   is left. So with one background add-on, the open window on Home redrew at 10 Hz: 0.205% of the
   PC (3.3% of one core) instead of 0.010%. Now the host looks at a view on its own only while that
   view's page is on screen and the add-on is starting, restarting or not answering
   (`addon_view::poll_after`). A running add-on wakes the host with its own frames and repaint
   requests, and the supervisor's heartbeat still comes every second, so stall detection is
   unchanged.
2. **The tray woke the app every second while hidden** although the icon refreshes every 2 s. It
   now wakes exactly when the next icon refresh is due (`tray::until_due`), shown or hidden. Clicks
   on the icon and its menu wake the app at once, as before. The effect is within noise (the tray
   state was already at one CPU tick or less per minute).

Not changed: the 30 s wake-up and the 5-minute list refresh (cost too small to see), the
supervisor, background add-ons, the tray icon itself.

Found, not changed (#176): while the window is hidden in the tray only `App::logic` runs, so the
supervisor is not ticked (no heartbeat, no restart of a crashed background add-on until the window
is shown) and a page add-on that was open when the window was hidden keeps its process (Resource
Monitor stayed running in the tray at 0% CPU, 46 MB). Fixing it changes behaviour, so it is its own
issue.

## Settings

The Settings page (About Toolboks) shows "Toolboks uses about X% CPU and Y MB (add-ons not
included)": CPU of the host process as a share of the whole PC and its private working set (the
"Memory" column of Task Manager). It is read with `GetProcessTimes` and `GetProcessMemoryInfo` on
the app's own process every 2 s, only while the Settings page is drawn (`src/own_usage.rs`).
Checked in a real window of the after build (own preview instance, no add-ons): "Toolboks uses
less than 0.1% CPU and 40 MB (add-ons not included)". The private working set (40 MB) is smaller
than the private bytes in the tables above (about 80 MB): the first is what Task Manager shows as
Memory, the second also counts private memory that is committed but not in RAM right now.

## Not measured

- Resource Monitor's background recording (#66, #73) is not in the add-on version measured here
  (0.1.0 is a page add-on), so "recording on" was not measured.
- Several background add-ons at once (the owner runs four). The 10 Hz redraw was one timer for all
  views, so the before cost with four is expected to be the same 0.2%, but this was not measured.
- Wake-ups are context switches of the process's threads (the GPU driver's threads included), not
  frames; the app has no frame counter.
- Release (non-preview) build, and other machines.
