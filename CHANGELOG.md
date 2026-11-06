# Changelog

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
