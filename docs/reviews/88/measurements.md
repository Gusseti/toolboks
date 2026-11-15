# Disk Manager measurements

Measured on Windows, 2026-11-15. Raw samples and executable hashes: [measurements.json](measurements.json).
MB below is decimal. Local OS caches were not cleared between the three sequential launches.

| Measurement | Old linked provider | Moved executable |
|---|---:|---:|
| PREVIEW host executable | 8,462,336 bytes | 8,241,152 bytes |
| Disk Manager executable | Included in host | 3,579,392 bytes |
| Add-on spawn to first stdio frame, median | Not a separate process | 25.4 ms |
| Add-on suspended idle working set, median | Included in host | 10.51 MB |
| Add-on suspended idle private commit, median | Included in host | 2.76 MB |
| Host input-idle startup, Disk Manager open, median | 213.9 ms | pending, not run |
| Host working set / private commit, Disk Manager open, median | 106.01 / 81.88 MB | pending, not run |

The host shrinks by **221,184 bytes (2.61%)** against the
same unsigned-catalog staging base, so removal of catalog signing is not counted as a Disk
Manager saving. The candidate includes its PREVIEW-only checker. Other tools remain compiled in.

The add-on command is `cargo run --locked -p addon-repo -- measure disk-manager`.
It builds the release executable, runs three headless stdio sessions, and samples suspended
process memory after five seconds. Frame times were 145.4, 24.8, 25.4 ms.
These are not download, named-pipe startup or visible-window timings; they do not prove the
one-second visible-page budget. Process working set includes shared mapped pages and must
not be treated as the unique physical-memory cost of the whole app.

Old host startup/RAM samples were collected before the owner stopped GUI work. Startup uses
Windows `WaitForInputIdle`, not first paint; the fresh isolated profile installed the original
activation package and opened Overview without a scan. The post-move host startup/RAM
comparison requires a GUI run and remains **pending, not run**. No total RAM saving is claimed.

The <25 MB idle add-on budget is met by the headless sample. The <4 MB host target is not met
by this pilot, which retains the other compiled-in tools. The scheduled GUI acceptance test,
direct old-provider UI comparison, and PR screenshots are **pending, not run**; the orchestrator
will arrange them when the owner is not gaming.
