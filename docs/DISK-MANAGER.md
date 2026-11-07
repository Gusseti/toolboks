# Disk Manager by stratum

The condensed Stratum integration is a separately installable, first-party Toolboks
add-on. It renders inside the existing workspace. The original standalone Stratum
project and executable have not been modified.

## Try the local preview

Close older previews and open the Disk Manager preview build. In Add-on browser,
find **Disk Manager by stratum**, choose Install add-on, review access, then install.
It appears in the sidebar with the Toolboks verified mark. Alternatively, install
`addons/disk-manager/disk-manager.tbaddon` through the local package installer.

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

SDK API 2 adds `byline`, `native_provider` and a `NativeAddon` source-level contract.
Toolboks links the separate `toolboks-disk-manager` crate and resolves its provider
only for the exact, embedded first-party package. Installing the package activates
that provider; no child app or browser is launched.

This is not a dynamic DLL ABI or an arbitrary native-code loader. Third-party
packages cannot claim this provider through their manifest. API 1 declarative
packages remain compatible. Independent native binary distribution, signing and
provider updates are future work. This preview ships the backend with the host and
its compatible activation package. Verification is an embedded trust decision,
not a manifest flag or cryptographic signature.

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

Optimization is available before a folder scan. It shows fragmentation analysis,
separate Windows defrag/TRIM history and media-appropriate actions. Existing 0.1
packages must review and install the 0.2 update to grant drive_optimize access.
Administrator access and explicit confirmation are required for optimization.
See DRIVE-OPTIMIZATION.md for behavior, validation and limits.
