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
`5f7c39b04dc367379be0eb9396cfb7362af2ffa3`. Nothing here changes protocol or supervisor.
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

Both repositories pass formatting and workspace/all-target clippy with warnings denied.
The add-ons workspace tests pass, including 26 Disk Manager unit tests (two existing ignored
machine/benchmark checks) and a two-test protocol suite. The suite checks all five frames,
loaded/saved preference keys, suspension/resumption, heartbeat and clean exit, including the
actual release executable through `ADDON_TEST_EXE`.

Before the unsigned-catalog staging rebase, the host workspace tests pass in standard and
preview mode with `--test-threads=1` (491 and 492 app tests respectively, 12 ignored, plus
the other workspace crates). Debug symbols were disabled locally to fit the build volume.
An earlier run hit `AccessDenied` in an unchanged process-priority test and another ran out
of disk space. Clean reruns passed; these failures were not hidden or fixed as part of #88.

## Native catalog check — pending

The owner changed the catalog to an unsigned plain list on 2026-11-15. Waiting for that
host change to merge to staging, then rebase this branch and rerun the isolated check below.
No catalog is signed and no key is generated or committed by the pilot tooling.

```powershell
cargo build --release --locked --features preview
cargo run --locked --features preview --example marketplace_fixture -- C:\Temp\disk-pilot-catalog --native-addon C:\src\toolboks-addons\addons\disk-manager\disk-manager.tbaddon C:\src\toolboks-addons\target\release\disk-manager.exe C:\src\toolboks-addons\addons\disk-manager\host-services.json
$env:TOOLBOKS_TEST_CATALOG = 'C:\Temp\disk-pilot-catalog'
$env:TOOLBOKS_DATA_DIR = 'C:\Temp\disk-pilot-data' # fresh, absent
target\release\toolboks.exe --disk-manager-check C:\Temp\disk-pilot-captures
```

The PREVIEW-only checker uses the app's normal browser review/queue, installer, supervisor
and drawing view. Its generated fixture has five files, four folders including the root,
and 6,300,160 logical file bytes. The test input hook clicks the real Analyze space control
at the rectangle reported by its tour anchor, then captures all five pages and checks all
five `--open` page-name resolutions. It does not request elevation or run maintenance.
Screenshots must be inspected for the scan totals, ranking and optimization gate; the
checker does not substitute tour sample data for the real scan.

Direct computer-use inspection was refused with “Computer Use was not approved to use
toolboks.” The built-in isolated PREVIEW integration checker supplies its own captures;
manual keyboard/scaling and UAC checks remain outside this automated run.

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
- The protocol 2 dependency pin predates catalog byline support and the unsigned envelope.
  The required #132 repin must update release/catalog tooling to the new host types as well
  as rebuilding executables, and carry the manifest's `by stratum` into catalog entries.
- Stdio timings exclude download, named-pipe authentication and host rendering. Do not use
  them alone as evidence for the one-second visible-page budget or total app RAM.

## Measurements

Final release measurements and their raw samples are recorded beside this file before
handoff. Host timing is Windows input-idle startup, not first paint. Add-on timing is spawn
to the first headless protocol frame. Working set and private commit are separate values.
The host still contains the other tools, so this pilot alone is not the <4 MB host target.
