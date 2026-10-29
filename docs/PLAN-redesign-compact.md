# Compact redesign ("C: tabs with an options inspector")

Owner brief: more compact and professional. Chosen from three mockups: **C, tabs with an inspector**, for the **whole app** (shell, Memory Cleaner, Resource Monitor), keeping the **colored per-app bar**. Dark theme only for now. Status: **in progress**.

## Style tokens (all tools use these, defined in `src/ui.rs`)

- Type: body 12 px, small 10.5-11 px, headings 14-15 px, big numbers 16-26 px. No more 24 px page titles.
- Corners 4 px (cards and controls), hairline 1 px borders, no shadows.
- Spacing: `GAP` = 8, `CARD_PAD` = 10, row height 22 px for tables and legends, buttons about 24 px tall.
- Colors: graphite surfaces (window `#131416`, panels `#1b1d21`, borders `#2a2d33`, status bar `#0f1012`), one muted accent blue `#3b82f6`; text `#e4e6ea`, muted `#8d939e`. Brand colors of the per-app bar stay as they are.

## Shell (`src/app.rs`, `src/main.rs`, `src/ui.rs`, `src/tools/mod.rs`)

- No left sidebar. A top tab strip, one tab per tool (text tabs, the selected one underlined 2 px in the accent), right-aligned: the version and the update control (compact: "Check for updates" small link/button, "Version X available" + "Install", Installing...).
- Under it a second, slimmer strip with the pages of the selected tool (Home | Advanced, Overview | Details), same underline style; hidden when a tool has a single page.
- A status bar at the bottom (22 px, darker): items from `Tool::status()` of every tool joined by spacing ("RAM 63%", "CPU 24%", "GPU 7%"), left aligned; right side empty or a short text.
- The update prompt window stays as it is. Window default about 960x640, minimum about 640x420.
- `ui::apply_style(&egui::Context)` sets fonts sizes, spacing, rounding and the dark visuals once at start-up.

## Memory Cleaner Home: three zones

- **Left column** (about 150 px): "Memory in use" label, big percentage (26 px, health-colored), a 6 px bar, the health sentence in small text, then rows In use / Available / Cached (standby) / Total, and a full-width **Optimize memory** button (accent, 28 px tall; "Optimizing..." while busy).
- **Center**: header "Where your memory goes" + memory in use on the right; the stacked bar (22 px high) and below it table rows (22 px: swatch, name, size right-aligned weak, share right-aligned) with the same hover tooltip as today.
- **Right inspector** (about 176 px, left border): "Options": switch **Clean automatically**, a slider for the start percentage, a slider "Skip apps under N MB", the deeper-clean tick box (admin) or the "Restart with permission" link, then a "Last run" block with the result (headline "Freed about X so far" / "Freed about X", "Still settling...", detail, "Memory in use A -> B") and the status line.
- No page title (the tab says it), no hero card, no gauge ring.

## Memory Cleaner Advanced

Same content as now (process table with filter, group by name, Protect, Trim; system-wide cleaning; auto-clean rules; exclusions), restyled compact: no big title, table rows 20-22 px, small headers.

## Resource Monitor

Same pages (Overview, Details), compact: no big title; cards use the shared tokens (8 px gap, 10 px padding, 4 px corners); smaller graphs (CPU about 120 px, others 40-60 px, per-core and engine graphs about 28 px); stat numbers 16 px. Overview should fit in about 560 px of height without scrolling. `Tool::status()` returns "CPU x%" and "GPU y%" (and RAM) while the sampler is live (last sample under 5 s old), else nothing.

## Rules

Single exe, no files, in-memory state, per-app colors unchanged, checks by fmt, clippy -D warnings, tests and a release build; the real-window check (screenshots, topmost window, guarded clicks) happens after the builders finish.
