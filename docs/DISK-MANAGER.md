# Disk Manager by stratum

The condensed Stratum integration is a separately installable, first-party Toolboks
add-on. It renders inside the existing workspace. The original standalone Stratum
project and executable have not been modified.

## Try the local preview

Disk Manager is built in the [add-ons repository](https://github.com/fredhakon/toolboks-addons/tree/main/addons/disk-manager)
as `disk-manager.exe`; the host no longer includes its code or activation package.
After the owner publishes it in the catalog, use Add-on browser, choose Disk Manager,
review its access, and install it. The `.tbaddon` in the source repository describes the
executable; installing that file alone does not install or activate a native provider.

Disk Manager uses protocol 3 and the administrator helper for drive actions. Releases and
catalog entries belong to the owner/orchestrator.

Choose a drive, browse to a folder, or paste a folder path, then Analyze space. Scanning is read-only,
runs on a worker thread. Stop scan keeps partial results while Disk Manager is open.
Leaving Disk Manager cancels a scan and releases its results from memory. Report exports and
Windows drive optimizations already started can finish in the background.

- Overview: the packed space map leads, with a folder list beside it on wider windows and below
  it on narrower windows. Select a folder in either view to explore it. Use the path above the
  map to return to an earlier folder.
  Tiles stretch into full-height columns to close unused pockets, with a 48-pixel minimum side. Tiny items are enlarged;
  labels show actual sizes, while larger tiles retain proportional areas.
- Largest files: global ranking, search, size/age filters and sorting.
- File types: extension totals and shares of the scan.
- Scan details: skipped entries, access errors and measurement limitations.
- Right-click a table entry to copy its path or reveal it in Explorer.
- Open pages from the Toolboks sidebar, or from **Pages** when the sidebar is collapsed.
- Export report: CSV/JSON of every discovered entry. Enter a full filename in the inline form;
  existing files are not overwritten. Export runs off the UI thread.

Scan path, exclusions and filters persist in namespaced preferences. Results remain
in memory. Links and junctions are skipped. Sizes are logical lengths; hard links
count per entry, and sparse/cloud files may differ from allocated disk space.
There is no NTFS MFT fast path in this version.

## Framework boundary

The SDK runtime draws Disk Manager in its own process and sends frames through protocol 3.
The installed catalog entry supplies its five page names and background declaration; the host
provides navigation and preferences, and starts the administrator helper for drive actions.
The catalog generator copies page/background metadata from `host-services.json` in the add-on.

The ID remains `toolboks.disk-manager`. Preferences remain under that ID in `config.json`:
`settings`, `optimize_selected_drive`, and `fragmentation_reports`. The data folder remains
`addon-data/toolboks.disk-manager/`. Loading the updated host keeps those preferences and files,
including when an old local activation package is rejected. Install the executable from the
catalog to restore access; the old provider package cannot run in this host. Removal keeps
data by default. No automatic download or settings migration is performed by this pilot (#94).

The catalog entry declares background operation so report exports and fragmentation analysis
can finish while another page is open. Suspending cancels folder scans and releases their
results; idle background work does not poll. The host can end the process at any time, and
closing Toolboks ends it. Protocol and supervisor behavior are unchanged by this move.

The scanner was adapted from the standalone into the add-on crate. The two currently
retain independent source copies; a shared core distribution can be extracted when
the standalone is next updated.

## Validation

Tests cover scanner totals/exclusions/cancellation, links, proportional map layout,
CSV formula escaping, package compatibility and native-provider cancellation.
The native UI was exercised on 5 generated files across 4 folders (6,300,160 bytes).
Folder drill-down and global rankings matched. CSV export through the UI contained
9 entries and the same file-byte total.

The folder picker was exercised with a local fixture. A one-byte folder beside a
5 MiB file remained selectable after horizontal scrolling and opened correctly.




## Drive optimization (0.2)

The Optimization page remains available before a folder scan. Drive inventory, media type,
Windows history and cached reports remain available. Fragmentation analysis, Defragment (`/D`),
TRIM (`/L`) and Optimize with Windows (`/O`) require the `drive_optimize` capability. Each
action is sent to the separate administrator helper, which shows its own Confirm/Cancel window.
Analysis does not change the drive. See [DRIVE-OPTIMIZATION.md](DRIVE-OPTIMIZATION.md) and the
pilot record in `docs/reviews/88/`.
