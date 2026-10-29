# Memory breakdown bar plan

A card on the Memory Cleaner Home page, "Where your memory goes": one thick stacked bar where each app family has its own color and a segment sized by its share of the memory in use, with a legend of sizes and percentages. Status: **planned, not started**.

## Decisions

| Topic | Decision |
|---|---|
| Placement | New full-width card on Home, **above** the existing "Apps using the most memory" card (both stay) |
| Grouping | Alias table plus shared name start (names only; no reading of exe version info) |
| Click | None. Hover only |
| Colors | Brand colors for known apps (Claude orange, Discord blurple, Chrome blue, ...), stable generated colors for everything else |

## What it shows

- Card title "Where your memory goes" and the memory in use on the right ("22.2 GB in use").
- A 34 px tall bar with rounded ends and 2 px gaps; one segment per group, widest first, then "Windows and other" in grey.
- A two-column legend under it: color swatch, group name, size (`size_plain`), percentage of memory in use.
- Hover on a segment or a legend row: tooltip with the group name, process count, size and percentage ("Claude: 14 processes, 5.3 GB, 24 % of memory in use").

## Grouping rules (in this order)

1. Normalize the name: lowercase, drop `.exe`.
2. Alias table of known families, e.g. `claude*` and its helpers -> Claude; `chrome`, `chromium` -> Chrome; `msedge`, `msedgewebview2` -> Edge; `discord*` -> Discord; `steam`, `steamwebhelper`, `steamservice` -> Steam; `chatgpt` -> ChatGPT; `node` -> Node.js; `firefox`, `spotify`, `code` (VS Code) and others as needed.
3. Names that share the same first word of at least 4 letters merge (e.g. the NVIDIA background services).
4. Windows services and shell processes (`svchost`, `services`, `explorer`, `dwm`, `System`, `Registry`, `Memory Compression`, ...) -> one group "Windows".
5. Any group under about 1 % rolls into "Other apps".

## Percentages

A group's size is the sum of its processes' working sets. The segments are scaled so that they fit inside the memory in use; what is left over is "Windows and other" (kernel, drivers, shared pages). Working sets include shared pages, so their sum can exceed "in use", which is why the scaling exists. The legend percentage is the group's share of memory in use.

## Colors

A table of brand colors for the known families (readable on both dark and light cards). Unknown groups get a stable color from a fixed palette chosen by hashing the group name, so a group keeps its color between runs. "Windows and other" is always neutral grey.

## Build steps

1. Pure grouping and color logic with unit tests (`src/tools/memory/groups.rs`): normalize, alias table, shared-prefix merge, Windows group, small-group roll-up, scaling to "in use", color lookup and hash fallback.
2. A `stacked_bar` widget in `src/ui.rs`: segments with gaps and rounded ends, hover detection, tooltip.
3. The card on Home, using the memory sampler's process list (grouped on the sampler's 2 s refresh, not every frame).
4. Optional follow-up: matching color dots in the Advanced table and the same grouping behind "Group by name".

## Rules this must respect

Single exe, nothing written to disk, in-memory only; same card design as the rest of the app; no desktop testing until the last phase and only when the owner says so (pure logic is covered by unit tests).

## Risks

- A wrong alias merges unrelated programs; keep the table small and explicit, test it.
- Shared pages make the sum of working sets larger than memory in use; the scaling and the "Windows and other" remainder must never go negative.
- Many tiny groups make a noisy bar; the 1 % roll-up and a cap of about 10 visible groups keep it readable.
