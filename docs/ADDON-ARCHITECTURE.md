# Add-on architecture

Toolboks is becoming an empty shell: the window, the sidebar, the add-on browser, settings, the
tray and self-update. Every tool is an add-on the user asks for inside the app (plan: #77,
direction: [ADDON-VISION.md](ADDON-VISION.md)). This page describes how the pieces fit, and says
for each piece whether it is built and whether the app uses it yet.

Every add-on follows the design system in [DESIGN.md](../DESIGN.md) and passes the
[add-on design checklist](ADDON-DESIGN-CHECKLIST.md).

## Two kinds of add-on

- **Declarative** (`.tbaddon`): a validated JSON package of headings, notes, live readings and
  preference controls. The host draws it. No process, no code.
- **Native**: a separate exe. It runs its egui page without a window and sends the drawing to
  the host, which paints it inside the add-on's page and sends input back. One add-on that
  crashes or hangs does not take the app with it, and only the add-on that needs administrator
  rights gets them.

## The pieces

| Piece | Where | State |
|---|---|---|
| Wire protocol (version 2) and SDK runtime `run_addon` | `sdk/toolboks-addon-proto`, `sdk/toolboks-addon-sdk` | Built and tested |
| Host view: paints an add-on's frames, forwards input | `src/addon_view/` | Built and tested; used by the test host only |
| Supervisor: starts, checks, watches and stops add-on processes | `src/supervisor/` | Built and tested; not called by the app yet |
| Host services: status bar items, links, tours, tray, background add-ons, preferences | `src/addon_services.rs` | Built and tested; not called by the app yet |
| Signed catalog: format, verifier, offline signing tool | `shared/toolboks-catalog`, `src/catalog.rs`, `tools/catalog-sign` | Built and tested; the built-in keys are placeholders, so no index is accepted yet |
| Installer: download, verify, install, update, roll back, remove | `src/installer.rs` | Built and tested; not called by the app yet |
| Shared code for add-ons: look, formatting, elevation check, storage, Debloater engine | `shared/` | Built; used by the host and the add-ons |
| Sample add-on and test host | `sdk/examples/hello-addon`, `examples/addon_host.rs` | Built; see [ADDON-AUTHOR-GUIDE.md](ADDON-AUTHOR-GUIDE.md) |
| Add-on browser as the marketplace, first-run screen | `src/marketplace.rs`, `src/app/marketplace.rs`, `src/app/store.rs` | Built; lists only what the signed catalog returns, so it is empty until a catalog is published (#116) |
| Repository and release workflow for first-party add-ons | [`toolboks-addons`](https://github.com/fredhakon/toolboks-addons) | Built (#83); no real tool released yet |
| The tools as add-on exes | #88 to #93 | Not built; they are still compiled into the app (see "Today") |

Details: [ADDON-PROTOCOL.md](ADDON-PROTOCOL.md) (protocol, connection, supervisor, host
services), [catalog-signing.md](catalog-signing.md) (catalog format and the owner's runbook),
[ADDON-THREAT-MODEL.md](ADDON-THREAT-MODEL.md) (what can go wrong and what is still open).

## How a native add-on runs

1. **Install.** The user picks it in the add-on browser. The installer downloads the exe from
   GitHub over HTTPS and places it only when its size and SHA-256 match the signed catalog. The
   previous version stays until the new one has started once; a new version that never starts is
   rolled back.
2. **Start.** When the page is opened (or at launch for a background add-on), the supervisor
   checks the exe against the recorded SHA-256, creates two pipes that only the current Windows
   user can open, and starts the exe inside a job object that ends with Toolboks. The add-on
   presents a launch token, then both sides exchange `Hello`.
3. **Run.** The host sends input for one frame at a time, at most 60 a second; the add-on answers
   with the drawing. An idle add-on sends nothing and costs the host nothing. Status bar items,
   tray entries, tour steps and preferences travel as messages.
4. **Hide.** The add-on is told to suspend. (Whether a hidden add-on that is not a background
   add-on should hold no process at all is open, see #97.)
5. **Fail.** A crash, a failed start or a long stall restarts the add-on at most 3 times per
   minute. Then it stays stopped with a plain message, a Restart action and a report the user can
   copy.
6. **Administrator rights.** An add-on that may ask and does ask is restarted alone through the
   UAC prompt.

Local data: `addons/<id>/<version>/` holds the exe or package and the catalog entry it was
installed from; `addons/<id>/active.json` says which version runs; `addon-data/<id>/` is the
add-on's own data, kept or deleted on removal as the user chooses.

## Today

Until the tools move (#88 to #94), the app works as before. Toolboks owns the window, background
palette, collapsible sidebar, overview, local add-on browser, settings and status bar. Add-ons
render inside that workspace.
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

### Packages and providers (today)

A `.tbaddon` is a validated UTF-8 JSON package. SDK API 1 renders declarative pages
with headings, notes, metrics and namespaced preference controls. SDK API 2 can
activate an approved native provider compiled into the host. Disk Manager by stratum
is the first native provider, implemented in a separate workspace crate.

Disk Manager, Audio Gainer, Macro Manager and System Profiles are compiled into the host
today and switched on by their packages; `NativeAddon` is the same Rust trait an add-on exe
implements, so moving one out does not change its page code. Packages cannot load arbitrary
native executables. Publishing by anyone but the Toolboks team remains future work. Package ID, title,
descriptions, publisher, icon, byline, compatibility and capabilities are standardized
by the SDK; the Rust validator is authoritative for cross-field rules.

Verification of local packages is an embedded first-party allowlist, not a self-declared
manifest flag. For catalog add-ons it comes from the signed catalog. The host recognizes exact approved Disk Manager
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
