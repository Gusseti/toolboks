# Debloater plan

A new tool in toolboks (its own top tab): turn off Windows ads, suggestions and tracking toggles, and remove preinstalled consumer apps, safely and with undo. Status: **phases 1-3 built (Privacy and ads apply+undo, Apps removal), not looked at on screen, never applied to the real system**. Style: the compact "C" design, same inspector pattern as the other tools.

## Decisions

| Topic | Decision |
|---|---|
| v1 scope | **Privacy and ads** toggles + **Apps** (curated list). Startup entries, scheduled tasks and services are a second step |
| Apps | Curated consumer apps only; anything not on the list is shown greyed out as protected |
| Admin | Runs without admin; items that need it are marked, and a "Restart as admin" button appears when such an item is ticked (with a warning that the undo history is lost). Never always-admin |
| Undo | In-memory for the current session only (single exe, no files). Settings can always be flipped back on the page (current state is re-read from Windows on every launch). Removed apps are reinstalled from the Microsoft Store. "Copy undo steps" puts them on the clipboard (not a file) |
| State | The page always reads the real state from Windows; it never trusts a saved log (Windows updates can re-add or reset items) |

## Page layout

Top tab "Debloater"; page strip: **Privacy and ads**, **Apps** (later **Startup**). Center: items grouped by risk (Safe, Careful), each row a tick box, name, risk tag, current state ("Now on", "Already off", "Needs admin"), reversibility. Right inspector **Review**: number of changes, how many are safe and how many need admin, "Create a restore point first" switch (when admin and System Protection is on), the **Apply N changes** button, **Undo last run**, and a plain note: "Undo works until you close toolboks. Removed apps come back from the Microsoft Store." A summary line after applying lists what worked, what failed and what needs a restart. Header shows the Windows edition and whether the app is elevated.

## Starter items (from the research; verify exact registry paths on this build, 26300, before shipping)

**Privacy and ads, Safe (per user, no admin):** Start menu recommendations and promoted apps; lock screen tips and ads; suggestions in Settings (including Microsoft 365 promos); suggested notifications; Bing web results in Start search; advertising ID; tailored experiences with diagnostic data; activity history (user part); Widgets on the taskbar; taskbar Chat/Task View/Copilot buttons; Game Bar capture overlay (not Xbox removal).
**Privacy, Careful (admin):** consumer features policy (stops auto-installed apps), diagnostic data services DiagTrack and dmwappushservice (Manual/Disabled, reversible), usage-tracking scheduled tasks (later step).
**Apps (curated, current user):** Clipchamp, Bing News, Bing Weather, Solitaire Collection, Tips/Get Started, Feedback Hub, Skype, Cortana leftovers, Mixed Reality Portal, Copilot app (may return after big updates), plus other known consumer apps found on the machine.
**Never offered:** Microsoft Store, Edge and WebView2, Defender, Windows Update and its services, Photos, Calculator, Xbox Identity and Gaming Services, Desktop App Installer (winget), shell hosts (Start menu, Shell experience, Search), OneDrive removal, Recall removal.

## How each action works (native, no PowerShell)

- **Apps:** WinRT `PackageManager` (list for the current user; fields: family name, publisher, version, framework, signature kind). Remove with `RemovePackageAsync` for the current user (no admin). All-users removal and deprovisioning need admin and are NOT part of v1. A hard-coded denylist plus framework/dependency and signature checks is the real safeguard. Session undo by re-registering from the install location while the package is still staged, else "reinstall from the Store". Async WinRT runs on a worker thread (`RoInitialize`).
- **Settings:** registry reads and writes (HKCU without admin, HKLM policy keys with admin). Always read the previous value first (keep the registry value type, "missing" counts as a state), write, then re-read to confirm the value is effective (Home edition and Group Policy can ignore or override policy keys).
- **Later:** startup entries via the Run keys and the `StartupApproved` values (the same mechanism as Task Manager), scheduled tasks via the Task Scheduler COM API, services via the Service Control Manager, restore point via `SRSetRestorePointW` (admin, System Protection on, one per 24 h by default; it does not cover removed apps and the UI must say so).

## Architecture

`debloat::model` (Item, Risk, Edition, Change, UndoOp; pure, unit-tested), `debloat::catalog` (the static item table and the protected-app denylist), `debloat::plan` (diff of selected items against the current state, apply order, undo stack), a thin Windows layer behind traits (`Registry`, `Packages`, later `Services`, `Tasks`, `Startup`) with in-memory fakes for tests, and the UI page. One worker thread with a command/event channel (UI never blocks on WinRT or registry calls, repaint on events). The app stays non-elevated; the existing UAC relaunch helper restarts it as admin on demand and reopens this page with `--open "Debloater/Privacy and ads"`.

## Phases

1. Model, catalog, plan/undo engine, fakes and tests; page shell with the inspector; edition and elevation detection.
2. Privacy and ads items: current-state detection, apply, undo, effective-value check.
3. Apps: list, protected greyed out, remove for the current user, session undo, results summary.
4. (Next step) Startup entries, scheduled tasks, services; restore point and presets.
5. Polish, copy review (plain language, no alarm), release.

## Rules and risks

Single exe, nothing written, in-memory state. No "apply everything" button; Careful items are never pre-ticked. Every item has a plain "what changes / what could break" text. Top risks: removing a protected app (denylist and dependency checks, curated list only), undo of app removal not reliable, settings silently ignored or reverted (always re-read), restore point silently skipped (show its real status), antivirus flagging an unsigned exe that changes system settings, WinRT/COM threading mistakes. Checks by fmt, clippy -D warnings, unit tests on the fakes, read-only smoke tests on the real system, a release build; applying real changes is tested only on a scratch HKCU key in test code or with the owner's explicit say-so.

## Decisions after the research (docs/RESEARCH-debloat.md)

| Topic | Decision |
|---|---|
| Phase 4 first | Startup entries (Run keys + StartupApproved), then scheduled tasks (exact-path allow-list), then services last |
| Careful items | Shown collapsed under "Advanced", never pre-ticked, one-line consequence, never in presets |
| Services | Safe tier: Disabled; Careful tier: Manual only; never Disabled with active dependents |
| Signing | Investigate SignPath Foundation first (open source), OV certificate as fallback |
| Undo | In memory only plus "Copy undo steps"; no backup file (research suggestion rejected); re-read real state on every launch |
| Still open | Compatibility Appraiser task, AI/Recall switches, presets, Home-edition badges, domain/MDM read-only mode, when to elevate (see report section 13) |
