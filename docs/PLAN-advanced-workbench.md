# Memory Cleaner: Advanced as a workbench

Status: **built, not looked at on screen**. Checked by fmt, clippy, 139 tests and a release build. Known gaps: the Share and Procs columns are not sortable, the memory-lists bar has no hover tooltip (the legend shows the sizes), Space does not toggle the selection, and Trim selected has no confirmation step. Style: the compact "C" design (see `PLAN-redesign-compact.md`).

## Decisions

| Topic | Decision |
|---|---|
| Structure | One page, "Advanced", with a right inspector that has three tabs: **Selected**, **System**, **Rules** (not split into separate pages) |
| Keep | Every current feature: filter, group by name, sort, Protect, Trim, system-wide cleaning (admin), auto-clean rules, exclusions |

## Layout

- **Memory lists strip** (top, full width of the center): one segmented bar of Active (in use by apps), Modified, Cached (standby) and Free memory from the existing memory-list query, with sizes underneath. Replaces the plain "used of total" progress bar and the technical summary line.
- **Toolbar** under it: filter box, "Group by name" tick box, "Trim selected (N)" button, process count on the right.
- **Process table** (center, 22 px rows): columns select box, Name (color dot from the breakdown colors), Procs, Working set, Private, Share of memory in use (mini bar), Lock (Protect tick box). Proper sort arrows in the headers, click a row to select, Ctrl/Shift click for several, Space toggles the selection box, the table scrolls, header stays.
- **Right inspector** (about 190 px, left border, darker fill):
  - **Selected**: group or process name, count, sizes, share; buttons Trim, Protect/Unprotect; "Last trim" result (before and after); for system and own processes a short reason why they cannot be trimmed.
  - **System** (admin): the existing actions (purge low-priority cache, purge cache, trim all working sets, flush modified pages) with the same confirmation text and a before/after of free and cached memory; "Restart as admin" when not elevated.
  - **Rules**: auto-clean (switch, sliders for the standby size, interval, cooldown, the activity log) and the list of protected apps with Remove buttons.

## Build steps

1. Pure helpers with tests: the memory-lists segments (Active = total minus free minus standby minus modified, never negative), selection set logic (toggle, range, clear), column sorting (stable, by group or by process).
2. `stacked_bar`-based memory-lists strip and legend.
3. Table with selection and the Share column; toolbar; "Trim selected" (runs the existing trim per process with the existing protection rules, one summary line).
4. Inspector tabs: move the current system panel, auto-clean panel and exclusions list into them; Selected tab content.
5. Remove the old collapsing panels and the old progress bar; keep all behaviour and tests passing.

## Rules and risks

Single exe, nothing written, in-memory state; protected and system processes stay untrimmable (same `skip_reason`); multi-trim must report skipped items instead of failing silently; table performance with about 400 rows (virtualized rows as today). Checks: fmt, clippy -D warnings, tests, release build; looking at the real window only when the owner says the PC is free.
