# toolboks plan

A lightweight Windows 11 toolbox of small utilities (Rust, egui/eframe with the glow backend). Single exe, auto-update via GitHub Releases. The first tool is a memory cleaner.

## Decisions

| Topic | Decision |
|---|---|
| Cleaner scope | (Superseded in part by "UX direction" below: a safe one-click Optimize now exists.) "Honest tool": live dashboard, per-process trim with before/after, opt-in admin purge, warning that cleaning can slow disk reads. No one-click "free RAM". |
| Admin rights | Run unelevated; relaunch elevated on demand (UAC) only for admin actions. |
| Repo and signing | Public, open source, free signing via SignPath Foundation. Do not copy GPL-3.0 code from Mem Reduct; reimplement from Windows API docs. License: MIT (chosen). Making the repo public still needs the owner's explicit go-ahead; it is private for now. |
| v1 extras | Auto-clean rules, process exclusions. Tray, hide-to-tray and start-with-Windows are NOT in v1, so auto-clean only runs while the window is open or minimized. |
| Single exe, no files | The app is one self-contained exe. It never creates folders, config, logs or caches anywhere. All state (exclusions, auto-clean rules, sort/filter) is in memory only and resets on every launch. The C runtime is linked statically (`.cargo/config.toml`), and eframe persistence stays off. Anything that needs persistence (autostart, saved rules) is out unless the owner changes this rule. |
| Updater | Built in, manual only: a "Check for updates" button, no network traffic until clicked. Own implementation over WinHTTP (no TLS library, +0.1 MB) instead of `self_update`, because that crate writes temp files. The exe is downloaded into memory, checked against the release's `.sha256`, swapped by renaming the running exe to `<exe>.old` and writing the new one, then the app restarts and the next start deletes `.old`. Releases come from the public repo, so no token is embedded; while the repo is private the button reports "No public release found". |

## UX direction (changed 2026-10-29)

The owner wants a non-technical, single-click experience with the technical controls as a bonus. The tool is called **Memory Cleaner** and has two sidebar pages:

- **Home** (bento grid layout, chosen from three mockups): a 12-column card grid. Top row: a large hero card (7 columns: ring gauge, plain-words health message, one big **Optimize memory** button) beside a column with an **In use** stat card and a **Clean automatically** card (switch plus a slider for the percentage in use that starts it: 50-95 %, default 85 %, at most once every 5 minutes, runs while the window is open or minimized, works without admin). Below, full-width cards: the last result in plain words ("Released about 1.2 GB from 7 apps"), **Apps using the most memory** (bars), and a collapsed **More options** card (slider: skip apps below N MB; tick box: also clear cached memory when running as admin; optional "Restart with permission" link).
- **Advanced**: everything from Phases 1-4, unchanged in capability: process table (filter, group by name, sort, Trim, Protect tick box), system-wide cleaning, auto-clean rules (sliders and tick boxes), exclusions.

**Result honesty (changed after the owner saw "Released 9.6 GB" while the gauge only dropped about 10 %):** the headline is the measured drop in memory in use, not the sum of per-app working-set shrinkage (trimmed pages stay resident, shared pages are counted per app, apps pull pages back, and dirty pages only count as available after Windows writes them out). After a run the page watches memory in use for 10 s (cheap RAM totals every 500 ms, lowest value kept) and shows "Freed about X so far" until it settles. With admin and the deeper-clean tick, Optimize also flushes the modified list (`FlushModifiedList`) before the low-priority purge so the drop is immediate. The sentence below about trimming predates this change.

**Optimize** is the "gentle" run: it asks running apps to give back memory they are not using (never Windows, the desktop, audio, this app or protected apps, and not apps under the size limit), closes and deletes nothing, and needs no admin. With admin and the tick box on it also runs the low-priority cache cleanup. This supersedes the earlier "no one-click" scope. Trimming raises *available* memory (pages move to the standby list), so the gauge visibly drops; the app still says plainly that apps reload what they need. Technical terms (standby, working set) appear only on Advanced.

## Memory tool phases

1. **Dashboard (done).** Read-only. Used, available and total RAM, plus a filterable, sortable per-process table (working set, private bytes) via `sysinfo`. The standby/modified/free line was added in Phase 3 (the query works without admin). Not yet: commit totals and page faults.
2. **Trim and exclusions (done).** Per-process `EmptyWorkingSet` with confirm and before/after delta, and an exclusion list. No admin. Skip system and protected processes (System, Idle, Registry, Memory Compression, PPL, our own pid, the shell); access-denied means skip, not error.
3. **Elevation and purge (built, purge actions not yet exercised on a real admin session).** "Relaunch as admin" via `ShellExecute` with the `runas` verb; the old window closes once the elevated one starts. Opt-in actions in a collapsed admin panel: purge low-priority standby, purge standby, trim all working sets, each with a confirmation and a before/after of available and standby memory. Uses `NtSetSystemInformation(SystemMemoryListInformation)` and needs `SeProfileSingleProcessPrivilege`. The API is undocumented: it is resolved at runtime and every failure is reported in the UI.
4. **Auto-clean rules (built, redesigned after review; unattended run not yet seen on a real admin session).** Only the low-priority standby purge, only in the elevated instance, off by default. Triggers: standby above N GiB while *free* memory is under 20 %, and a fixed interval, with a cooldown between runs; nothing runs when under 256 MiB of standby exists. The earlier "available memory below N %" trigger was removed: Windows counts standby as available, so a purge never changes it and the rule could not clear itself. Purges run on a worker thread, so the window never freezes. The rules are checked every 5 s through a `Tool::tick` hook that also runs while the window is minimized. It reports through an in-window activity log and a taskbar flash (no tray, no toast, since a toast needs a registry identity). Rules and log are in memory only. Open question: whether `tick` keeps firing on a minimized window on every Windows setup.
5. **Release hardening (partly done).** Done: MIT license, release workflow that publishes the bare exe plus `.sha256`, in-app updater (see decisions). Not done: make the repo public, apply to SignPath Foundation (needs a public, released repo), end-to-end test of an update against a real release, no packers (UPX) on the exe. The checksum comes from the same release as the exe, so it catches corruption, not a compromised release; a signature check is a possible later hardening.

## Out of scope

One-click "free RAM", a scheduled blanket trim of all processes, registry flush, file-cache max-size limiting, combine-pages, flush-modified-list on a timer, trimming protected processes, and a "RAM freed" headline (it conflates available with free).

## Risks and open items

- Review done (independent Opus reviewer, 2026-10-29). Fixed: auto-clean premise, unreachable standby rule, purges on the UI thread, update swap now writes `<exe>.new` with fsync before renaming, final-URL host check after redirects, whole-download deadline, numeric pre-release ordering, trim re-checked on confirm, `audiodg.exe` and `fontdrvhost.exe` protected. Added: group-by-name table view.

- Auto-update vs "no files": the updater downloads the new exe and swaps it in, which briefly writes a temp file and the replaced exe next to it. Check that `self_update` leaves nothing behind, or download into memory only.
- Real benefit on Win11 is unproven; the UI shows measured deltas only.
- AV and SmartScreen false positives are likely until releases are signed; submit false positives to Microsoft.
- NT memory-list command values, verified against System Informer's phnt headers and git's `test-drop-caches.c` (the research report had them off by one): `EmptyWorkingSets` = 2, `FlushModifiedList` = 3, `PurgeStandbyList` = 4, `PurgeLowPriorityStandbyList` = 5. Unit tests pin these.
- The purge actions have only been compiled and unit-tested for layout and values; run them once in an admin session and watch the before/after numbers.
- egui idle CPU: a hidden or idle window should not repaint.
- The GitHub Actions workflows have not run yet (0 runs after two pushes); check Actions and billing settings.

## Key references

- EmptyWorkingSet: https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-emptyworkingset
- NT system information classes: https://ntdoc.m417z.com/system_information_class
- Russinovich, "Memory-Optimization Hoax": https://itprotoday.com/cloud-computing/memory-optimization-hoax
- Mem Reduct (GPL-3.0, ideas only): https://github.com/henrypp/memreduct
- Code signing options: https://learn.microsoft.com/windows/apps/package-and-deploy/code-signing-options
- SignPath open source: https://about.signpath.io/product/open-source
- self_update (GitHub backend): https://docs.rs/self_update/latest/self_update/backends/github/struct.UpdateBuilder.html
