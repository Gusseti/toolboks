# Disk Manager executable pilot

The paired add-ons change moves Disk Manager into `toolboks-addons/addons/disk-manager`.
The host no longer links its provider or embeds its activation packages. Other tools stay
compiled in. No production catalog, tag or release is changed by this pilot.

## Compatibility

- ID and data folder remain `toolboks.disk-manager` and `addon-data/toolboks.disk-manager/`.
- Pages stay Overview, Largest files, File types, Scan details, Optimization, in that order.
- Preferences stay `settings`, `optimize_selected_drive`, and `fragmentation_reports` in
  the host's existing namespaced configuration. The host regression test loads an old
  activation package alongside saved preferences and a data file and verifies both survive.
- Scanner, map, folder picker, `metadata.ps1` and `analyze.ps1` are byte-identical to host
  staging commit `1dbc8f25614dd48a3f7611ee3da6276db3f55e9d` (ignoring checkout line endings).
  Existing scanner/layout/export/tutorial tests moved with the source.

## Elevation and #132

The executable stays on protocol 2 and host dependency pin
`86c749cbd3a27de920329b512ea8a3f33baca74b`. Nothing here changes protocol or supervisor.
After #132 merges to staging, repin all add-ons host dependencies and rebuild for protocol 3
before publication, including the protocol integration tests and Windows helper wiring.

- `optimization/mod.rs` retains `HostRequest::RestartElevated { page: "4" }`, gated by
  `drive_optimize`. Returning to the page does not start an operation.
- `optimization/backend.rs` and `analyze.ps1` use administrator rights for numeric
  fragmentation analysis. Metadata/history/device queries and folder scanning do not
  request elevation.
- Defragment (`/D`), TRIM (`/L`) and Optimize with Windows (`/O`) are disabled in the UI
  and rejected before backend worker creation. The remote-drawn confirmation is removed.
  #132 supplies the separate elevated helper and confirmation window; this pilot builds
  no replacement. No UAC or drive-changing action was exercised.

## Checks

Formatting and workspace/all-target clippy with warnings denied pass in both repositories.
The add-ons workspace tests pass: 26 Disk Manager unit tests (two existing ignored checks),
two Disk Manager protocol tests and the remaining workspace tests. The protocol suite covers
all five page frames, loaded/saved preference keys, suspend/resume, heartbeat and clean exit.
It also passes against the repinned release executable through `ADDON_TEST_EXE`.

Host standard workspace tests pass after rebasing onto #134 (488 app tests, 12 ignored,
plus the other workspace crates). Final headless preview workspace tests also pass (489 app tests, 12 ignored, plus
the other workspace crates). An earlier preview run hit `AccessDenied` in the unchanged
Resource Monitor process-priority test; the final serial rerun passed. Earlier runs also exhausted disk space; the task's Cargo build artifacts were cleaned
and debug symbols disabled locally. No unrelated process/tool behavior was changed.

## Scheduled GUI acceptance and PR screenshots — pending, not run

The owner stopped all GUI launches while gaming. The final acceptance run, direct comparison
with the old linked provider, post-move host GUI startup/RAM measurement and PR screenshots
are **pending, not run**. Earlier local investigation is not presented as final acceptance
evidence. No screenshots are included in this PR. The orchestrator will schedule these checks
when the owner says it is a good time. No GUI is to be launched during this handoff.

The host branch is rebased onto unsigned-catalog staging commit
`86c749cbd3a27de920329b512ea8a3f33baca74b` (#134). The matching add-ons pin uses protocol 2.
The optional native fixture contains a plain `index.json` with only `format_version` and
`entries`, plus the executable; it creates no signatures or keys.

Run the following only during the orchestrator's scheduled GUI session, using fresh paths:

```powershell
cargo build --release --locked --features preview
cargo run --locked --features preview --example marketplace_fixture -- C:\Temp\disk-pilot-catalog --native-addon C:\src\toolboks-addons\addons\disk-manager\disk-manager.tbaddon C:\src\toolboks-addons\target\release\disk-manager.exe C:\src\toolboks-addons\addons\disk-manager\host-services.json
$env:TOOLBOKS_TEST_CATALOG = 'C:\Temp\disk-pilot-catalog'
$env:TOOLBOKS_DATA_DIR = 'C:\Temp\disk-pilot-data' # fresh, absent
target\release\toolboks.exe --disk-manager-check C:\Temp\disk-pilot-captures
```

The PREVIEW-only checker uses the app's browser review/queue, installer, supervisor and view.
It generates five files, four folders including root, and 6,300,160 logical bytes. Its egui
test input hook clicks the real Analyze space control at the SDK anchor's reported rectangle,
restoring the real layout before clicking. It captures all five pages and checks all five
`--open` page-name resolutions without requesting elevation or running maintenance.

The emitted result covers install/navigation only. Inspect captures for scan completion,
totals, ranking and the optimization gate before accepting the GUI test. Manual keyboard,
scaling, cancelled-UAC and elevated analysis/helper checks also remain for the scheduled run.

## Findings for #89–#93

- Native manifests have no declarative pages. Catalog generation must carry native page
  names separately; the new optional `host-services.json` preserves them and `--open` links.
- `background: true` starts/retains a process, including while idle. Disk Manager uses it
  for exports and analysis. Its suspend hook must separately cancel scans and release large
  results; merely keeping the original thread-based implementation is insufficient.
- The runtime calls `save_preferences` only when the host asks; hidden work does not by
  itself emit preferences. The app currently does not periodically ask through that service.
  Check hidden preference changes before moving automatic/background tools. Disk Manager
  keeps completed report results in memory until the page saves them or the host asks.
- The protocol 2 pin now includes the unsigned envelope and catalog byline support.
  Upgrade all host dependencies together when repinning for #132, regenerate the lockfile,
  and recheck the catalog generator as well as every executable.
- Stdio timings exclude download, named-pipe authentication and host rendering. Do not use
  them alone as evidence for the one-second visible-page budget or total app RAM.

## Measurements

See [measurements.md](measurements.md) for the release size comparison, headless first-frame
and idle-memory samples, earlier baseline measurements, limits and pending GUI measurements.
