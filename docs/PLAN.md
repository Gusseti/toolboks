# toolboks plan

A lightweight Windows 11 toolbox of small utilities (Rust, egui/eframe with the glow backend). Single exe, auto-update via GitHub Releases. The first tool is a memory cleaner.

## Decisions

| Topic | Decision |
|---|---|
| Cleaner scope | "Honest tool": live dashboard, per-process trim with before/after, opt-in admin purge, warning that cleaning can slow disk reads. No one-click "free RAM". |
| Admin rights | Run unelevated; relaunch elevated on demand (UAC) only for admin actions. |
| Repo and signing | Public, open source, free signing via SignPath Foundation. Do not copy GPL-3.0 code from Mem Reduct; reimplement from Windows API docs. Making the repo public and choosing a license (MIT or Apache-2.0) need the owner's explicit go-ahead. |
| v1 extras | Auto-clean rules, process exclusions. Tray, hide-to-tray and start-with-Windows are NOT in v1, so auto-clean only runs while the window is open or minimized. |
| Single exe, no files | The app is one self-contained exe. It never creates folders, config, logs or caches anywhere. All state (exclusions, auto-clean rules, sort/filter) is in memory only and resets on every launch. The C runtime is linked statically (`.cargo/config.toml`), and eframe persistence stays off. Anything that needs persistence (autostart, saved rules) is out unless the owner changes this rule. |
| Updater | `self_update` (pure Rust, single exe) with checksum verification; releases come from the public repo, so no token is embedded in the exe. |

## Memory tool phases

1. **Dashboard (done).** Read-only. Used, available and total RAM, plus a filterable, sortable per-process table (working set, private bytes) via `sysinfo`. Not yet: commit totals, page faults, standby/modified/free breakdown (needs the windows crate or the NT memory-list query; verify whether it works without admin).
2. **Trim and exclusions.** Per-process `EmptyWorkingSet` with confirm and before/after delta, and an exclusion list. No admin. Skip system and protected processes (System, Idle, Registry, Memory Compression, PPL, our own pid, the shell); access-denied means skip, not error.
3. **Elevation and purge.** "Relaunch as admin" via `ShellExecute` with the `runas` verb. Opt-in purge of the standby list and low-priority standby list via `NtSetSystemInformation(SystemMemoryListInformation)`. Needs `SeProfileSingleProcessPrivilege`. The API is undocumented: resolve it dynamically, fail soft, verify the command values against sources and test on this Win11 build.
4. **Auto-clean rules.** Trigger on low free memory and a large standby list, with a cooldown and a notification. Trim runs unelevated; purge only when elevated.
5. **Release hardening.** Public repo and license, SignPath signing, `self_update` with checksums, no packers (UPX) on the exe.

## Out of scope

One-click "free RAM", a scheduled blanket trim of all processes, registry flush, file-cache max-size limiting, combine-pages, flush-modified-list on a timer, trimming protected processes, and a "RAM freed" headline (it conflates available with free).

## Risks and open items

- Auto-update vs "no files": the updater downloads the new exe and swaps it in, which briefly writes a temp file and the replaced exe next to it. Check that `self_update` leaves nothing behind, or download into memory only.
- Real benefit on Win11 is unproven; the UI shows measured deltas only.
- AV and SmartScreen false positives are likely until releases are signed; submit false positives to Microsoft.
- NT memory-list command values must be checked before use (`EmptyWorkingSets` = 2, `FlushModifiedList` = 3, `PurgePageList` = 4, `PurgeStandbyList` = 5, `PurgeLowPriorityStandbyList` = 6 are from memory, unverified).
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
