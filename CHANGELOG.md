# Changelog

## [Unreleased]

## [0.16.1] - 2026-11-16

### Fixed

- **Switching pages inside an add-on works at once.** Picking a page of an add-on in the sidebar (or in the header with the rail collapsed, or from a Home finding) often left the old page on screen until the mouse moved over the page, and on quiet pages such as Disk Manager's or Event Watch's the switch could seem to never happen. Add-ons that redraw on their own (Resource Monitor, Memory Cleaner) switched only after up to two seconds. The new page now shows right away, with every add-on, including those already installed.

## [0.16.0] - 2026-11-16

### Changed

- **Home's health view starts with one thing to look at.** The most important finding (the first problem, else the first thing to watch) now has its own card: what it is, what was measured, a small picture of the measurement where the add-on gives one (a filled bar such as "470 GB used / 6 GB free", or a strip of the last readings), why it matters, what to do, and the button to fix it. Other things worth a look follow as smaller cards, and everything that was checked and found fine is grouped by add-on, each with its value ("Good", "61% free"). Checks without a current result (not checked, unavailable, out of date) still show with their reason. Still no score and no "your PC is healthy": Toolboks only shows what your add-ons measured.

### Added

- **For add-on makers: protocol 5.** A finding can say why it matters (`.why(text)`) and carry a small picture of what was measured (`.evidence(Evidence::bar(..))` or `.evidence(Evidence::series(..))`), with labels that carry the numbers. Add-ons built for protocol 3 and 4 keep working unchanged; a protocol 4 add-on's findings show without the picture. See `docs/ADDON-PROTOCOL.md` and `docs/ADDON-AUTHOR-GUIDE.md`.

## [0.15.0] - 2026-11-16

### Added

- **Home now shows the health of your PC, as measured by your add-ons.** Instead of repeating the sidebar, Home lists what needs attention first (a drive that may fail soon, memory under pressure, and so on), then what was checked and found fine. Every line says what was measured, which add-on measured it, and how long ago. There is no score and no "your PC is healthy" claim: Toolboks only reports what your add-ons check. **Check again** asks the add-ons to re-check. An add-on that has not reported yet shows as "Not checked". Add-ons start reporting here as they are updated.
- **For add-on makers: protocol 4.** Add-ons can send small plain-text findings to Home (`findings()` in the SDK) and answer **Check again** (`check_now()`). Add-ons built for protocol 3 keep working unchanged. A catalog entry can list the checks an add-on reports (`checks`, optional). See `docs/ADDON-PROTOCOL.md` and `docs/ADDON-AUTHOR-GUIDE.md`.

### Changed

- The "Your toolkit" list is gone from Home; the sidebar and the Add-ons page already list your tools.

## [0.14.0] - 2026-11-16

### Added

- **Updates are shown on Home.** When an add-on has a newer version, the top of Home says "Update available: Audio Gainer" (or "2 updates available: ...") with an **Update** button that opens the normal access review. The add-on's row says "Update" instead of "On", and its card shows the new version with an **Update** button.
- **The install review says what data an add-on keeps on your PC.** An add-on can declare one short plain line about it (for example "Keeps your last scan of each drive on this PC (file and folder names)."). It is shown under what the add-on can do when you review an install (also for a package installed from a file), and in its details. It comes from the reviewed add-on list or package, at most 160 characters of plain text. For add-on makers: a package manifest with `data_note` is refused by Toolboks versions older than this release, so an add-on that sets it must raise its `min_toolboks_version` to this release.

### Changed

- **Keyboard use of the sidebar and header.** Tab now goes down the foot of the sidebar in the order shown (Settings, Add-ons, Pick your tools), a collapsed sidebar shows a row's name while it has keyboard focus, and the header switches to a menu whenever its buttons or page tabs would not fit, not at a fixed window width.
- **Toolboks costs almost nothing while you are not using it.** Once any add-on had connected (also a background one), the open window redrew 10 times a second, about 0.2% of the whole PC on an 8-core PC; it now redraws only when something changes (0.01%). Hidden in the tray it stays at about 0.002%. Settings shows the live figure: "Toolboks uses about X% CPU and Y MB". Numbers and method are in `docs/OVERHEAD.md`.

### Fixed

- **Add-ons are looked after while Toolboks is in the tray or minimized.** A background add-on that stops unexpectedly is now restarted within about a second even when the window is hidden, and a tool whose page was open when you closed the window to the tray is now closed instead of left running. Before, both waited until the window was shown again.
- **Keyboard use of the add-on status in the sidebar.** The small status pill (for example "Could not install") and the dot on the collapsed sidebar can now be reached with Tab, show a focus ring, and are named for screen readers and in a tooltip, for example "Disk Manager, could not install".

## [0.13.2] - 2026-11-15

### Fixed

- **"Refresh list" now shows a new release straight away.** GitHub keeps the plain file the add-on list comes from in its cache for up to 5 minutes, so an add-on released a moment ago was missing after a refresh. Toolboks now asks GitHub's API first, which is at most a minute behind, and uses the plain file only if that fails.

## [0.13.1] - 2026-11-15

### Added

- **Updates are visible.** An add-on with a newer version shows "Update" at the right end of its row in the sidebar (an amber dot on the collapsed sidebar), so you no longer have to open the Add-ons page to find out.
- **The add-on list refreshes itself every 5 minutes** while Toolboks is open, so a new release is noticed without a restart. If the connection fails, the list you already have stays as it is. The Add-ons page says when it was last checked, and "Refresh list" is still there.

## [0.13.0] - 2026-11-15

This is a big update: Toolboks is now a small shell and every tool is a separate add-on you download. **Read this before updating.** When you start the new version it offers to restore your tools; nothing is lost.

### Changed (breaking)

- **Toolboks is now a shell, and every tool is a separate add-on.** Memory Cleaner, Resource Monitor, Debloater, Event Watch, Disk Manager, Audio Gainer, Macro Manager and System Profiles are downloaded, updated and removed on their own from the add-on browser. The app itself no longer contains any tool, so it is smaller and starts faster.
- **Updating keeps your tools.** The first time you start the new version, "Restore your tools" lists the tools you had, ticked. Install them with one click and review what each one asks to access. Your saved settings stay under the same names. If you are offline or the list cannot be reached, nothing is lost: the offer stays until the tools are installed or you choose to forget them.
- **No more restarting as administrator.** Changes that need administrator rights are made by a small separate helper that shows its own Confirm or Cancel window listing exactly what will change. Toolboks and the add-ons never run as administrator themselves.
- **`--open "Name/Page"`** now opens an installed add-on by its name.
- **Cleaner sidebar:** every add-on has its own icon, a new add-on shows "New" until you open it (instead of "Installed" under every name), the title is bold, the open sidebar is wider (240 px), and "Pick your tools" only shows while you are on the Add-ons pages.
- **After "Restore your tools" or picking tools,** Toolboks goes to your toolkit when the installs are done, and the picker no longer lists every installed add-on a second time.
- Add-ons and the add-on list are not code signed (an owner decision); Windows SmartScreen may warn about each download.

## [0.12.6] - 2026-11-15

### Added

- **Privacy policy:** a new page on the Toolboks website states that Toolboks does not collect data and lists every time the app connects to the internet.

### Changed

- No changes to the app or its tools in this release.

## [0.12.5] - 2026-11-14

### Added

- **Macro Manager:** a verified installable add-on for recording and editing keystrokes, mouse movements, button presses/releases, clicks and scrolling inside Toolboks.
- **Mouse-path editor:** place and drag path points, add clicks at points, and use timed or instant movement. The full-width timing slider reaches 10 seconds with clickable checkpoints; edit its duration directly using `ms`, `s` or `sec`.
- **Macro playback:** use the prominent Play macro button, run once, repeat for a duration, or toggle repeating until stopped, with a configurable delay between completed runs. Each macro has its own saved activation key that triggers it even when another macro is selected. Recording shortcuts work across tools, and Esc stops active work.
- **Local macro library:** starts empty, requires a name for each new macro, and saves changes automatically. The last macro can be deleted. No recording or playback starts automatically; mouse recording excludes the Stop button click, and stopping playback releases its held keys/buttons.
- **Audio Gainer settings:** optionally enable your microphone when Toolboks starts, even when another page opens first. The general startup option is off by default; a remembered per-microphone on/off state takes precedence when per-microphone settings are enabled.
- **Per-microphone profiles:** remember microphone on/off state, gain, listening volume, output devices and routing separately for each microphone. Restore activation when switching microphones and when Toolboks launches; saved routes resume with an enabled microphone.
- **Default microphone:** save your preferred microphone by name, keep it remembered while disconnected, and automatically switch back when it reconnects—even while using another tool.
- **Saved-preferences permission:** existing Audio Gainer installations can use Review update to approve saving these settings locally. Older packages remain supported without automatically receiving the new permission.

### Changed

- Macro Manager uses one unified options bar with collapsible Record and Playback sections. Selecting a step shows its settings instead; the rename field is removed.
- Record Mouse captures movement, mouse buttons and wheel activity, including over Toolboks. Record keystrokes captures keyboard input separately; recording checkboxes are removed. The recording shortcut uses the most recently chosen recording mode. Only clicks on the mouse recorder's Stop button are excluded from mouse recordings.
- Macro Manager follows the approved editor layout: aligned full-height library and options panels, quiet full-width macro rows, spaced step controls, a separate canvas toolbar, and a full-width timing track with an editable duration. Record and Playback expand without indenting their controls; recorded paths show labelled start and end points.
- New macro creation happens inline: Give it a name, confirm with the checkmark or Enter, or cancel with X or Escape. White icons turn green or red on hover.
- Consecutive recorded mouse movements become one read-only Mouse movement step, with its full trajectory displayed. Coordinates, timing and input order are preserved, including older recordings grouped on load. Grouped recordings use a new local action format that older builds cannot read.
- The steps list, mouse editor, timing and macro controls remain visible with disabled actions when the library is empty, including after deleting its last macro.
- Step rows include editable delays and immediate deletion. Delete removes the selected step; deleting a macro requires confirmation. Add key starts key capture without inserting a default key. Selected buttons use readable light text.
- Removed the duplicate Refresh devices button. Kept Refresh microphones, which updates the microphone list without stopping active audio or resetting output choices.

### Fixed

- Esc stops mouse recording independently of keyboard-hook delivery and immediately refreshes the recorder controls.
- Macro activation and recording shortcuts preserve keypad keys instead of binding the number row. Keypad shortcuts are labelled explicitly; previously assigned keypad shortcuts need to be reassigned once.
- Shortcut assignment waits without a timeout for the first non-modifier key, with Ctrl/Shift/Alt/Windows supported in combinations. A dedicated key-state listener avoids recording-hook waits on the editor thread.
- Macro shortcuts wake the active editor immediately instead of waiting for its 200 ms background poll.
- Manually disabling the microphone keeps it off without forgetting its saved routing setup. With per-microphone settings enabled, the disabled state is also remembered across app launches.
- Automatic microphone selection rejects ambiguous duplicate names rather than choosing an arbitrary device.
- Audio Gainer's settings walkthrough does not initialize device discovery, resume saved routes or write preferences merely by displaying the tutorial.

## [0.12.4] - 2026-11-12

### Added

- Audio Gainer has a dedicated **Refresh microphones** button directly below the microphone selector.

### Fixed

- Opening or reopening Audio Gainer's microphone list now refreshes the available microphones before showing the choices, including when opened with the keyboard. The list does not repeatedly scan while left open.
- Microphone-only refresh keeps the selected Windows audio endpoint across device reordering or renaming, without stopping active input/routes or resetting output choices. A removed or ambiguous endpoint is not silently replaced by another microphone.
- Failed microphone scans keep the previous inventory and display a local retry message. An active microphone can still be disabled if its endpoint disappears from the list.
- Built-in add-on versions and minimum host versions are synchronized with Toolboks 0.12.4.

### Verification

- Added focused pointer/keyboard, selection-preservation and failure/retry regression tests. The permanent testing report records results and remaining physical-device verification limits.

## [0.12.3] - 2026-11-12

### Changed

- Toolboks has its own logo: it shows in the window, on the taskbar, in Alt+Tab and on toolboks.exe in Explorer, instead of the default icon. The tray icon still shows the live memory percentage.

## [0.12.2] - 2026-11-09

### Changed

- Disk Manager scans much faster: it reads file sizes from the folder listing instead of asking for each file, and walks folders on several threads, fewer on a hard disk or network drive. A test folder of about 100,000 files went from about 14 seconds to under one second.

## [0.12.1] - 2026-11-07

### Added

- **System Profiles → Gaming** is a verified add-on in the Add-on Browser. Choose which settings to apply, including Game Mode, Game DVR, Windows privacy and cleanup policies, PowerShell 7 telemetry, scheduler values, and the Ultimate Performance power plan. Windows policy options depend on your edition.
- Review current and proposed values before applying. Toolboks can enable System Protection, confirms a new Windows restore point before tuning, and saves an importable `.reg` export of the selected values it will change.
- The live **Activity log** records backup, apply, verification, and restore steps. **Restore** reverses the registry and power-plan changes made by Toolboks. **Open backup location** opens the folder containing the export and log.
- The Gaming walkthrough explains the workflow. Guided steps cover Windows graphics settings, NVIDIA Control Panel, Memory Integrity, Disk Cleanup, and other settings you change in Windows or the relevant app.

### Fixed

- Reduced RAM use by loading add-on workspaces on demand and releasing their page data when you switch away. Resource Monitor and Memory Cleaner stop sampling while inactive.
- Leaving Disk Manager cancels an active scan and releases its results from memory. Report exports and Windows drive optimizations already in progress can finish in the background.
- Audio Gainer keeps active audio routes running when you switch away and stops collecting offscreen meter history.
- **Review registry edits** stays available after applying the Gaming profile. It compares saved registry values with what Windows currently reports.

## [0.12.0] - 2026-11-07

### Added

- Audio Gainer by aural, a verified add-on for live microphone levels, input gain, headphone monitoring and routing audio to other apps through a virtual cable.

### Changed

- Memory Cleaner has a new page: a list of memory measures on the left and, on the right, a plain verdict (healthy, busy or under pressure), the memory graph with the 90 percent pressure line, where your memory goes, the biggest apps with a hint and growth per hour, the last run report and the automatic switch. "Trim idle apps" no longer usually ends with "no app was a good candidate".
- Memory Cleaner reports honestly: it measures what moved to cache and what came back after 30 seconds and two minutes, and never says memory was freed. Optimize memory now lowers the memory priority of idle background apps and trims only the biggest idle ones, one by one, never the app in front. Clean automatically is off by default, waits for real memory pressure and runs at most once an hour. Trim-all, the full cache purge and the modified-list flush are gone; the low-priority cache purge is under Advanced (admin only, with a warning). The apps list shows growth per hour.

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
