# Add-on architecture

Every add-on follows the design system in [DESIGN.md](../DESIGN.md) and passes the
[add-on design checklist](ADDON-DESIGN-CHECKLIST.md). Declarative packages pick up the
host look with no change; native add-ons use the host helpers and the tokens only.

Toolboks owns the window, background palette, collapsible sidebar, overview, local
add-on browser, settings and status bar. Add-ons render inside that workspace.
The four original tools are separate preinstalled (built-in) add-ons with their own
manifests in `assets/`: `toolboks.memory-cleaner`, `toolboks.resource-monitor`,
`toolboks.debloater` and `toolboks.event-watch`. Each has a sidebar entry, can be turned
off and on like any other add-on, and keeps its pages and `--open Tool/Page` link.
They cannot be removed or replaced by a local package.

A turned-off tool is not ticked: Resource Monitor sampling and Memory Cleaner automatic
cleaning stop, its status bar items disappear and it leaves the sidebar. `--open` for a
turned-off tool opens the overview with a short message. Turning the tool on starts it
fresh. The tray shows memory only and does not depend on any tool. Older settings for
the former combined `toolboks.system-essentials` entry are ignored and dropped.

## Packages and providers

A `.tbaddon` is a validated UTF-8 JSON package. SDK API 1 renders declarative pages
with headings, notes, metrics and namespaced preference controls. SDK API 2 can
activate an approved native provider compiled into the host. Disk Manager by stratum
is the first native provider, implemented in a separate workspace crate.

`NativeAddon` is a source-level Rust contract, not a stable DLL ABI. Packages cannot
load arbitrary native executables. Independent third-party native binaries, signing,
an online catalog and remote add-on updates remain future work. Package ID, title,
descriptions, publisher, icon, byline, compatibility and capabilities are standardized
by the SDK; the Rust validator is authoritative for cross-field rules.

Verification is an embedded first-party allowlist, not a self-declared manifest flag
or cryptographic publisher signature. The host recognizes exact approved Disk Manager
packages. Version 0.1 keeps its read-only access; version 0.2 requests drive optimization
through the installation review. Modifying an old manifest to add that capability
fails the trust check. Verified first-party entries appear first in the browser.

The registry revalidates installed packages at launch, skips invalid/conflicting ones,
and reports errors in Settings. Installation and updates review the package's access;
updates require a higher version and preserve preferences. Disable hides the workspace
and releases native providers; uninstall also removes that add-on's preferences.
Existing Windows drive maintenance can continue independently of the native UI.

## Local state

- Stable: `%LOCALAPPDATA%/Toolboks/`; preview: `%LOCALAPPDATA%/Toolboks Preview/`.
- `TOOLBOKS_DATA_DIR` overrides the parent directory for isolated developer runs.
- `config.json` stores sidebar/tray choices, disabled IDs and namespaced preferences.
- `addons/<id>.tbaddon` stores independently versioned installed packages.

Config writes use a sibling pending file and atomic replacement. Text preference
writes are debounced; pending edits are saved on normal exit. Native tool-specific
settings are still session-only until their own migration. Disk scan results remain
in memory; fragmentation reports are cached with their volume identity and timestamp.

## Guided Help

The host supplies full and current-tab walkthroughs, page/inner-tab navigation,
Next/Back, jump navigation and Cancel/Escape. Actual layout rectangles provide
highlight targets and scroll containers reveal offscreen sections. Workspace controls
are disabled during a tour; existing background work continues. Tour navigation
must not execute actions or change preferences. SDK packages can provide descriptions;
packages without them receive basic page/widget tours. See [GUIDED-TOURS.md](GUIDED-TOURS.md).

## Native storage tools

Disk Manager follows DESIGN.md (tokens in its `theme.rs`, inline stats bar, hairline
cards, sentence-case table headers). It uses background scanning and export workers, a packed clickable map,
folder browsing, largest-file rankings, filters, type totals and coverage reports.
Drive optimization uses Windows numeric analysis and retained success history.
HDD defrag, SSD TRIM and Windows-selected fallback actions require an approved
capability, administrator access and explicit operation confirmation. See
[DISK-MANAGER.md](DISK-MANAGER.md) and [DRIVE-OPTIMIZATION.md](DRIVE-OPTIMIZATION.md).

The `preview` feature isolates the app's data/instance identity and disables all
self-update paths. Local builds do not publish releases. The stable app version is
unchanged; this feature branch is proposed for integration through `staging`.
