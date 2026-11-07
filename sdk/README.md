# Toolboks Add-on SDK 0.4 — developer preview

Build an add-on that stays inside the Toolboks workspace. This SDK includes a Rust
manifest/UI contract, validation and packaging CLI, JSON Schema, a working starter,
and the shared background palette. No account is required for local development.

Design: every add-on follows [DESIGN.md](../DESIGN.md), the Toolboks design system, and
passes the [add-on design checklist](../docs/ADDON-DESIGN-CHECKLIST.md). `tbaddon validate`
prints the checklist after a package passes; `tbaddon checklist` prints it on its own.

## Start here

1. Copy `examples/hello-system.tbaddon` and edit the manifest, pages, and widgets.
2. Give the add-on a unique lowercase ID, such as `com.yourname.yourtool`.
3. Validate and package it using `tbaddon.exe` (included in the downloadable SDK):

```powershell
.\tbaddon.exe validate .\my-addon.json
.\tbaddon.exe pack .\my-addon.json .\my-addon.tbaddon
```

From source, build the CLI with `cargo build --release --manifest-path toolboks-addon-sdk/Cargo.toml`.
The Rust library can also generate packages using its public `Package`, `Manifest`,
`Page`, and `Widget` types. Toolboks uses this same validator when installing.

4. Open the preview's Add-on browser, choose Install package, and enter the full path
   or drop the `.tbaddon` file on the window. Review the requested access and install.
5. The add-on appears in the sidebar. Its preferences survive app restarts. A higher
   major.minor.patch version can replace an installed version; equal/older versions
   are rejected. Disabling hides the add-on; removal deletes its preferences too.

## Package contract

A `.tbaddon` is a UTF-8 JSON bundle (maximum 2 MiB), containing `schema_version: 1`,
`manifest`, and `pages`. No extraction, DLL loading, executable entry point, remote
script, or child window is involved in this first runtime.

The manifest is everything the host showcases: ID, title, description (short and
long), version, author, publisher display name, category, icon, accent, compatibility,
optional website/support/license/screenshots, and capabilities. Screenshot links
are exposed in details; this preview does not fetch remote image galleries.

Versions use major.minor.patch. Declarative packages use `api_version: 1`; host-linked
native providers use API 2. `min_toolboks_version` is
checked on install and load. Icons: grid, activity, layers, storage, wrench, leaf.
`accent` is `#RRGGBB`. URLs must be HTTPS. All unrecognized manifest fields are rejected.

Pages contain headings, text, notes, live metrics, toggles, and text inputs. They render
in the host's style (see DESIGN.md): headings as section titles, notes as one-line muted
banners with a short title, metrics as inline value and label, toggles as switches.
A package has no styling fields, so it looks like the built-in tools with no change.
Write headings and page names in sentence case, use plain words and label estimates.
Each
page has a unique ID. Preference keys must be unique across the entire add-on.
Widgets are keyed by `kind`; see the starter and `package.schema.json`.

Capabilities:

- `host_metrics`: read memory percentage, total/available memory, and system uptime.
- `local_preferences`: read/write this add-on's own toggles and text preferences.
- `filesystem_read`: approved native providers can read selected filesystem metadata.
- `user_exports`: approved native providers can save reports on the user's request.

Toolboks owns the window, navigation, and backgrounds. Add-ons choose their content,
pages and accent color. The design tokens (backgrounds, text, accent, data and status
colours) are in `theme.json` and mirror DESIGN.md. They are not editable by a package.
The four built-in tools keep their native egui UI. Native add-ons use the host
helpers and the tokens only, with no palette, font, radius or icon style of their own.

## Trust and scope

Publisher names in local manifests are descriptive, not authenticated identities.
They never grant a checkmark. Embedded first-party records grant the checkmark to
the four built-in tools and the exact Disk Manager package; these are listed first.
Signed publisher identity, online discovery,
and remote add-on updates are future work.

API 2 adds the optional `byline` and `native_provider` manifest fields. `native-ui`
exposes the `NativeAddon` Rust trait with integrated rendering, background tick and
cancellation hooks. The byline is subdued attribution next to an add-on's title.
Native packages have no declarative pages and must match a provider trusted by the
current host. Disk Manager by stratum is the first example.

This is a **source-level host-linked contract**, not a dynamic DLL ABI. The preview
does not load arbitrary executable code from packages. Third-party native binary
distribution, signing and provider updates remain future work. API 1 packages keep
working. See the included native reference source and its README for the boundary.

JSON Schema helps editors; the Rust validator is authoritative for cross-field
rules, capability checks, duplicate IDs/keys and host compatibility.

## Guided tutorials (SDK 0.4)

The host provides Help on every add-on workspace, with full and current-tab tours.
Tours start only when requested. Back, Next, Cancel, Escape, arrow keys and a step
picker are supplied by the host. The host disables workspace controls during tours.

A declarative package can include an optional top-level `tutorial` array:

```json
"tutorial": [
  {
    "page": "overview",
    "target": "widget-2",
    "title": "Memory in use",
    "description": "This reading updates from the host's live memory metrics."
  }
]
```

`page` must match a page ID. `target` is `page` or `widget-N`, where N is the zero-based
widget index on that page. Titles are limited to 120 bytes, descriptions to 4000,
and tours to 300 steps. The validator rejects missing targets and executable actions.
When omitted, Toolboks supplies a basic tour of every page and widget automatically.
Custom tours should cover every feature and explain consequences in plain language.

Native source providers implement `tutorial_steps`, `tutorial_location`,
`tutorial_start`, `tutorial_navigate`, and `tutorial_finish`. Route strings identify
pages and inner tabs. Mark controls with `tutorial::mark(ui, id, response.rect)` or
wrap a component with `tutorial::region(ui, id, |ui| ...)`. These use actual rendered
rectangles and automatically reveal offscreen targets in scroll containers. Stable
IDs must match step targets. Native routes must navigate only, never run an action.
Save transient UI state on start and restore it on finish, including cancellation.
Disk Manager demonstrates safe, in-memory sample data before a scan is available.

Tutorial metadata is local content; it does not confer verification or extra access.
These additions require this updated development host. Older packages without tours
remain compatible; the app release version is unchanged during local development.

## Drive optimization and host requests (SDK 0.4)

`drive_optimize` is a native-only capability for fragmentation analysis and Windows
storage optimization. Source providers receive the installed package capabilities
through `set_capabilities` and must gate actions accordingly. The host trusts only
its exact first-party providers; adding a capability to a manifest does not grant it.
Disk Manager 0.1 remains read-only until its approved 0.2 package update is installed.

`live_audio` is a native-only capability for microphone monitoring and routing to
user-selected outputs. Audio Gainer opens audio streams only after an explicit action.

`system_tuning` is a native-only capability for the System Profiles add-on. Applying
or restoring a profile requires an explicit action and administrator rights.

`take_host_request` can return `ReviewUpdate` or `RestartElevated { page }`. The host
handles the package review or user-requested administrator restart. `navigate(page)`
restores the requested native page after restart without automatically doing work.
A tutorial route remains navigation only. The native example includes the updated
Disk Manager source, Windows backend, legacy manifest and permission-gating example.
See DRIVE-OPTIMIZATION.md for the execution boundaries and intentional test limits.
