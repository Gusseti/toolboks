# Changelog

## [Unreleased]

### Added

- Aural, a verified add-on for live microphone levels, input gain, headphone monitoring and routing audio to other apps through a virtual cable.

### Fixed

- Resource Monitor verifies the selected process before ending it or changing its priority and pauses those actions while a list is frozen.
- Debloater leaves already-disabled services disabled when the suggested off setting is Manual.

- Disk Manager streams Windows drive optimization output in memory and safely parses localized progress.
- Event Watch uses recent records of any severity before Windows started to estimate when a crash occurred. It shows an unknown stop time when the logs cannot support an estimate.
- Debloater blocks per-user changes when an elevated instance cannot verify that it belongs to the signed-in account.

- Toolboks creates its local configuration only after a settings or add-on change.
- Debloater preserves stricter diagnostic-data settings and no longer offers the Compatibility Appraiser, Flighting feature-configuration tasks or OneSettings download policy.
- Debloater keeps privacy and ads undo history across Apply runs. Undo works newest first, and failed steps stay available to retry.

## [0.11.0] - 2026-11-07

### Changed

- A new look for the whole app and every add-on: darker graphite surfaces, one purple accent, a compact layout, and a Windows system font. The design is written down in `DESIGN.md` so later add-ons follow it.
- A hamburger button opens the sidebar to show names; closed, it is a slim icon rail. The pages of the open tool are listed under it, and the header shows where you are.
- Resource Monitor Overview is now a list of resources (CPU, Memory, GPU, Disk, Network, Power) with a large detail view for the one you pick. The Resource Monitor guided tour covers all six.
- Memory Cleaner, Debloater, Event Watch, Home, the add-on browser, Settings, the update window and Disk Manager use the new look. Event Watch time and source columns are wider.
- The crash dialog now says in plain words what happened and that nothing was sent or saved.
- "Connected only" on the Connections page no longer lists UDP sockets (there is an "Include UDP" switch).
- Add-on authors: `DESIGN.md`, a new design checklist (`docs/ADDON-DESIGN-CHECKLIST.md`) and `tbaddon checklist`. The SDK examples and theme follow the new tokens.

### Notes

- The window can still be too narrow for tools with four pages; this is tracked for a later release.

## [0.10.0] - 2026-11-07

### Changed

- The preinstalled **System Essentials** add-on is split into four add-ons: **Memory Cleaner**, **Resource Monitor**, **Debloater** and **Event Watch**. Each has its own entry in the sidebar and in the add-on browser.
- Each of the four can now be turned off and on again. A tool that is turned off stops its background work, for example the Resource Monitor stops sampling, and its status bar items are hidden. Turning it on again starts it clean, so its in-memory graph history starts empty.
- `--open Tool/Page` for a tool that is turned off opens Home with a short message instead of failing.
- Guided walkthroughs are now one per tool (12 steps for Memory Cleaner, 22 for Resource Monitor, 10 for Debloater, 6 for Event Watch).
- The Home cards count the tools that are turned on instead of showing a fixed number.
- Existing settings keep working: the old System Essentials entry is dropped and all four tools start turned on.

## [0.9.0] - 2026-11-07

### Added

- An integrated add-on platform: installed tools open inside Toolboks and appear in the sidebar. The four original tools are grouped in the preinstalled **System Essentials** add-on.
- A local add-on browser with installation review, updates, enable/disable controls, removal, and verified first-party listings.
- **Disk Manager by stratum** with background folder/drive scans, a folder picker, a packed interactive storage map, breadcrumbs, largest-file rankings, search and size/age filters, file-type totals, coverage reports, and CSV/JSON exports.
- **Drive optimization** with Windows fragmentation analysis, separate last-defrag and last-TRIM records, HDD defragmentation, SSD TRIM, and a Windows-selected fallback when media type is unavailable. Operations require the add-on permission, administrator access, and explicit confirmation.
- **Help walkthroughs** for full add-ons or the current tab, with purple highlights, automatic tab navigation, Next/Back, a step picker, and Cancel/Escape. Includes 50 System Essentials steps and 21 Disk Manager steps.
- **Add-on SDK 0.4** with standardized metadata, capability validation, JSON Schema, examples, a validator/packager CLI, guided-tour support, and native provider integration hooks.

### Changed

- Refreshed the interface with graphite backgrounds, a dark purple accent, the original app icon, a larger startup window, and clearer text.
- Added a collapsible sidebar with icon navigation and saved layout preference.
- Save host preferences, installed add-ons, and namespaced add-on settings locally.
- Standard release builds retain automatic/manual update checking, update installation, and restart. The optional development preview uses a separate profile and disables self-updates.

### Current limits

- Native providers are compiled into Toolboks and activated by approved packages; independent third-party native binaries and an online catalog remain future work.
- Drive optimization uses Windows services and can continue after Toolboks closes. This version has no in-app stop control. Missing fragmentation measurements or history are shown as unavailable.
- Existing Disk Manager 0.1 installations retain read-only access until the 0.2 add-on update is reviewed and installed.

The standalone Stratum application remains separate. See [drive optimization documentation](docs/DRIVE-OPTIMIZATION.md) for operation behavior and validation limits.
