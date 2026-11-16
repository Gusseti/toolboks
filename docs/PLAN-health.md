# Plan: Health on Home, fed by add-on findings (#54)

Status: plan, 2026-11-16. Nothing here is built. Umbrella issue: #54 (part of #47).

Home answers "what deserves attention on this PC, and what should I do?" from what the installed
add-ons measured. The host never measures anything itself: it is a shell. Each add-on reports
small plain-text **findings**; the host validates them, keeps them in memory and ranks them on
Home. There is no health score and no overall "your PC is healthy" verdict.

**Privacy.** Findings stay on the PC. They are kept in the host's memory only, never written to
`config.json` or any other file, never put in crash or add-on reports, and never sent anywhere.

## 1. The finding

One finding is one check one add-on made. New types in `sdk/toolboks-addon-proto/src/lib.rs`:

```rust
pub struct Finding {
    /// The add-on's own name for the check, stable across runs: "memory-pressure",
    /// "drive-health-c". 1 to 48 chars of a-z, 0-9, '.', '-'. Unique within the add-on.
    pub id: String,
    /// Plain words, one line, at most 80 chars: "Memory is under pressure".
    pub title: String,
    /// What was observed, at most 240 chars: "Available memory stayed under 1.1 GB for
    /// 6 minutes and Windows read 900 pages a second from disk."
    pub observation: String,
    pub status: FindingStatus,
    /// Used for ranking only when `status` is `Attention`; ignored otherwise.
    pub severity: Severity,
    /// When the add-on measured it, Unix seconds.
    pub measured_at: u64,
    /// After this many seconds the reading is out of date. 60 to 604 800 (7 days).
    pub stale_after_secs: u32,
    pub action: Option<FindingAction>,
}

pub enum FindingStatus { Ok, Attention, NotChecked, Unavailable }
pub enum Severity { Watch, Problem }

pub enum FindingAction {
    /// Open a page of the same add-on. `route` is what `NativeAddon::navigate` receives;
    /// its first segment (before '/') must be one of the add-on's catalog `pages`.
    /// `label` at most 40 chars: "Open drive health".
    OpenPage { label: String, route: String },
    /// A short instruction for something only the user can do (BIOS, Windows Settings),
    /// at most 240 chars.
    Instruction { text: String },
}
```

Rules for authors (in `docs/ADDON-AUTHOR-GUIDE.md`, checked in review, not by code):

- `Ok` says what was measured ("No crashes recorded in the last 7 days"), never "healthy PC".
- `NotChecked`: the add-on could check but has not yet (first read still running, page never
  opened). `Unavailable`: it cannot check on this PC, with the reason ("Needs administrator
  rights", "This drive does not report health").
- `Problem` is for harm to the PC or data now or soon (a drive "Replace soon", blue screens).
  `Watch` is for "slows the PC or worth a look" (memory pressure, throttling, XMP off).
- One action at most. Never an action that changes the system from Home: an `OpenPage` leads to
  the add-on's own page, where its usual preview, confirm and undo apply.

### Validation in the host (`src/health.rs`, see section 2)

- At most 16 findings per add-on per message; the rest are dropped.
- Each finding is checked on its own; a bad one is dropped (with one line in the add-on's
  report log), the rest are kept. Dropped when: the id breaks the pattern or repeats (first wins),
  the title is empty after cleaning, or an `OpenPage` route's first segment is not one of the
  add-on's catalog `pages`.
- Text is cleaned with the same `clip` as status items (control characters removed, trimmed, cut
  to the limit). Text is drawn as plain `RichText`: no markdown, no links, no URLs are ever
  interpreted, so there is no markup to strip.
- `measured_at` later than the time the host received it is set to the receive time.
  `stale_after_secs` is clamped to 60..=604 800.

### Why a hostile add-on cannot spoof or flood

- **No source field on the wire.** The host files every finding under the id of the connection
  it came from. That id is fixed by the supervisor before the add-on runs (exe hash checked
  against the saved catalog, per-launch token, pipe client PID, `Hello` id must match). The
  source name on Home is the installed entry's `title`, drawn by the host next to every finding.
- **No way to point at another add-on or the web.** `OpenPage` routes are checked against the
  sender's own pages; there is no URL action.
- **Bounded memory.** Each message replaces the add-on's whole set (like `StatusItems`), so one
  add-on holds at most 16 findings of about 1 KB.
- **Bounded share of Home.** The "Needs attention" list shows at most 3 rows per add-on, then a
  "2 more from Disk Manager" link to that add-on. One add-on marking everything `Problem` cannot
  push the others off the page.
- **Bounded work.** A flood of messages costs what a flood of `StatusItems` costs today: the
  reader thread, a replace, and at most one Home repaint a second, only while Home is shown.

This adds one line to threat 7 in `docs/ADDON-THREAT-MODEL.md`: "Findings are cut to fixed
limits, filed under the sending connection's id, and link only to the sender's own pages.
**Covered** (`health` tests)."

## 2. Protocol, SDK and host storage

### New messages (protocol 4)

Appended at the end of the enums, so every existing variant keeps its index:

| Direction | Message | Meaning |
|---|---|---|
| add-on → host | `AddonMessage::Findings(Vec<Finding>)` | The full set of this add-on's findings; replaces the previous set. Sent when it changes, also while suspended. |
| host → add-on | `HostMessage::CheckNow` | The user pressed "Check again" on Home. Optional to honour; the add-on re-checks when it can and sends `Findings`. |

`CheckNow` is added now so the button does not need a second protocol bump later.

### Version negotiation (older add-ons and older hosts keep working)

Today both sides refuse unless the versions are equal, and the host speaks first. The installed
catalog entry already records the add-on's `protocol_version`, so the host can speak the add-on's
version instead of its own:

1. `toolboks-addon-proto`: `PROTOCOL_VERSION = 4`, new `MIN_PROTOCOL_VERSION = 3`,
   `Hello::with_version(id, software, version)`. `Hello` and `Refuse` stay frozen.
2. Host: `catalog::PROTOCOL_VERSIONS = &[3, 4]`. `LaunchSpec` gets `protocol_version: u32`
   (from the installed entry in `addon_services::launch_spec`); `supervisor::launch` sends
   `Hello::with_version(id, .., spec.protocol_version)`. `host_handshake` is unchanged: it already
   compares the add-on's answer with the version it sent.
3. SDK (v4) `addon_handshake`: accepts a host version in `MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION`
   and answers with the host's version; returns the agreed version. Anything else is refused in
   the same plain words as today.
4. The SDK runtime stores the agreed version and sends `Findings` only when it is 4 or higher.
   The supervisor stores it per add-on (`Supervisor::protocol_version(id)`); the host sends
   `CheckNow` only to connections at 4 or higher, and ignores `Findings` on a version 3 connection.

| Add-on | Host | Result |
|---|---|---|
| v3 (today's exes) | new (3, 4) | Entry says 3, host sends `Hello` 3, works as today, no findings. |
| v4 | new (3, 4) | Entry says 4, findings flow. |
| v4 | old (3 only) | The old host's catalog filter already hides entries with protocol 4 (`hides_an_unknown_protocol_version`), so it keeps the v3 version it has. A v4 exe that does meet a v3 host (tests, sideload) agrees on 3 and sends no findings. |

Release order follows from this: the host with `[3, 4]` ships first; add-on entries move to 4
(and their `min_toolboks_version` to that host version) only after it.

### Background reporting

A background add-on (catalog `background: true`) is started at launch and kept suspended while
its page is closed; `tick_background` runs every 200 ms. The runtime's `flush_side_channels`
already runs after each background tick, so findings sent from there reach Home with the page
closed. A non-background add-on reports only while its page is open; when it quits its findings
stay on Home and age into "Out of date" (section 3). Today Memory Cleaner and Disk Manager are
background; Event Watch and Resource Monitor are not (see open question 2).

### The SDK API an author calls

In `sdk/toolboks-addon-sdk/src/lib.rs`, next to `status_items`:

```rust
/// This add-on's current health findings. Called after every frame and background tick;
/// sent to the host only when the set changed. Default: none.
fn findings(&self) -> Vec<Finding> { Vec::new() }
/// The user asked Home to check again. Default: nothing.
fn check_now(&mut self) {}
```

`Finding` and friends are re-exported from the SDK, with `Finding::ok(id, title, observation)`,
`Finding::attention(..., Severity)`, `Finding::unavailable(...)`, `Finding::not_checked(...)`
builders that fill `measured_at` from the clock. The runtime diffs `findings()` like
`status_items()` (field `self.findings`) and sends `AddonMessage::Findings`. Add-ons keep their
checks in pure functions (`fn check(input) -> Finding`) so they are tested on fakes.
`sdk/examples/hello-addon` gains one finding as the example.

### Storage in the host: memory only

New `src/health.rs`, owned by `addon_services::Services` as `pub health: health::Store`:

```rust
pub struct Store { sources: BTreeMap<String, Source> }      // key: add-on id
struct Source { findings: Vec<Kept>, running: bool }
struct Kept { finding: Finding, received_at: u64 }
impl Store {
    pub fn replace(&mut self, id: &str, list: &[Finding], pages: &[String], now: u64) -> Vec<String>; // dropped-reasons for the log
    pub fn set_running(&mut self, id: &str, running: bool);
    pub fn remove(&mut self, id: &str);                      // turned off or uninstalled
    pub fn view(&self, now: u64, titles: &dyn Fn(&str) -> Option<String>) -> View;
}
pub struct View { pub attention: Vec<Row>, pub checked: Vec<Row>, pub more: Vec<(String, usize)> }
```

- `Services::message` routes `AddonMessage::Findings` to `replace` (pages from the installed entry).
- `Event::Stopped` / `Restarting` call `set_running(id, false)`; findings are kept, not removed
  (unlike status items), so Home can say "Event Watch is not running" and show their age.
- Turning an add-on off or uninstalling it calls `remove` (next to `set_addon_enabled` in
  `src/app/shell.rs` and the callers of `installer::remove`).
- `view` is pure and takes `now`, so ranking, staleness and caps are tested with made-up times.

Nothing is written to disk. After a restart Home starts empty and fills within seconds as the
background add-ons report; Event Watch rereads the Windows log, which is its own history. Saving
findings would only show old readings as if they mattered, so there is no case for a file.

## 3. Home

Home becomes: the restore and update banners (unchanged), then **Health**. The "Your toolkit"
list and its detail card leave Home (open question 1); the sidebar and the Add-ons page already
list and manage the add-ons. No tool grid, no score, no hero numbers.

### Ranking (`health::Store::view`)

1. **Needs attention**: `Attention` findings. Current before out-of-date, then `Problem` before
   `Watch`, then newest `measured_at`, then source title, then id. At most 3 per source.
2. **Also checked**: every other finding, in this order: `NotChecked`, `Unavailable`,
   out-of-date `Ok`, current `Ok`; within a group by source title, then title.

A finding is **out of date** when `now - measured_at > stale_after_secs`. An out-of-date
`Attention` finding stays in "Needs attention" (it may still be true) with the word "Out of
date". An out-of-date `Ok` is shown as "Out of date", never as OK. When the source is not running,
its rows add "<Add-on> is not running".

### Sketch (content width 820 at the minimum window)

```
Home
┌──────────────────────────────────────────────────────────────────────────────┐
│ Health                                                         [Check again] │ 14/600, outlined button
│ 2 things need attention. From 3 add-ons, checked 1 min ago.                  │ 11 muted
│                                                                              │
│ Needs attention                                                              │ 12/600
│ ● Problem  Drive C: may fail soon                         [Open drive health]│ dot+word 11, title 12/600
│            The drive reports 2 media errors and 94% wear.                    │ 12 text
│            Disk Manager · checked 3 min ago                                  │ 11 muted
│ ──────────────────────────────────────────────────────────────────────────── │ hairline #24262c
│ ● Watch    Memory is under pressure                     [Open Memory Cleaner]│
│            Available memory stayed under 1.1 GB for 6 minutes.               │
│            Memory Cleaner · checked just now                                 │
│                                                                              │
│ Also checked                                                                 │ 12/600
│ Not checked  Recent crashes · Event Watch has not run yet   [Open Event Watch]│ one line each, 28 px
│ Unavailable  Drive health of D: · Needs administrator rights · Disk Manager  │
│ Out of date  Boot time · checked 3 h ago · Event Watch is not running        │
│ OK           No crashes in the last 7 days · Event Watch · checked 2 min ago │
└──────────────────────────────────────────────────────────────────────────────┘
```

Nothing needs attention (says what was checked, claims nothing else):

```
│ Health                                                         [Check again] │
│ Nothing needs attention in what was checked. 3 add-ons, checked 1 min ago.   │
│ Also checked                                                                 │
│ OK           Memory: no pressure in the last 30 min · Memory Cleaner · ...   │
│ OK           Drive C: no warnings from the drive · Disk Manager · ...        │
│ Not checked  Recent crashes · Event Watch has not run yet   [Open Event Watch]│
│ Toolboks checks only what your add-ons measure.                              │ 11 muted
```

No add-on has reported: "No add-on has reported a check yet. Memory Cleaner, Disk Manager and
Event Watch report checks here." with `[Browse add-ons]` (names from the catalog `checks` field,
phase H4; before H4 the sentence stops after "yet.").

### Rules from DESIGN.md

- One `card` (hairline, radius 10, `#15161a`); rows inside it separated by hairlines, no cards
  inside the card, no coloured side borders.
- Status always as dot plus word: Problem `#e5534b`, Watch `#f0a83a`, OK `#3ecf8e`; Not checked,
  Unavailable and Out of date in muted `#9a9ea8` with no dot colour. Words at 11, never colour
  alone.
- Text: section 14/600, finding title 12/600, observation 12 `#e8e9ec`, source and freshness 11
  muted. Nothing below 11 px.
- Buttons: the attention rows' actions are outlined; the first `Problem` row's action takes the
  accent fill (one primary per view). "Check again" is outlined. `Instruction` actions show the
  text as a 12 px line under the observation, no button.
- Freshness: "checked just now" (< 1 min), "checked 4 min ago", "checked 2 h ago",
  "checked 3 days ago". While Home is shown it repaints every 30 s for these words; hidden, never.
- Layout is measured: when the row is narrower than 560 points the action moves under the text.
  Long lists scroll inside the content area. Tab order: Check again, then each row's action top
  to bottom, with the focus ring. Check at 125-200 % scaling and 1060 x 720.
- "Check again" sends `CheckNow` to every running v4 add-on that reports findings, then shows
  "Checking…" in the caption until the next `Findings` or 10 seconds.
- An `OpenPage` action uses the existing link path (`addon_services::resolve_link` result
  `Link::Open { id, page }`, then `Navigate { page: route }`).

Code: new `src/app/health.rs` (drawing only, from `health::View`), called from `home` in
`src/app/shell.rs`. DESIGN.md gets a "Health row" component entry.

## 4. Who reports what first

| Add-on (repo `toolboks-addons`) | Findings | Source in the add-on | Background today |
|---|---|---|---|
| Memory Cleaner | `memory-pressure`: Ok (no pressure in the last 30 min, "busy" says so in the observation), Attention/Watch while Under pressure and for 30 min after, with the reasons from `Assessment::reasons`. Action: OpenPage `Home`. Refreshed each minute, stale after 5 min. | `src/pressure.rs` `PressureModel` | yes |
| Disk Manager | `drive-health-<letter>` per fixed drive: Healthy → Ok, Watch → Attention/Watch, Replace soon → Attention/Problem, SATA without admin and unsupported → Unavailable with the existing plain note. Read at start and every 6 h, stale after 24 h. Action: OpenPage `Optimization/<letter>:`. | `src/health.rs` `verdict()` (add-ons PR #55, merged) | yes |
| Event Watch | `recent-crashes`: blue screen in 7 days → Attention/Problem; 2+ unexpected restarts → Attention/Watch; none → Ok "No crashes recorded in the last 7 days"; log unreadable → Unavailable. Action: OpenPage `Crashes`. | `src/analysis.rs` `detect_crashes` | no (question 2) |

Memory pressure comes from Memory Cleaner, not Resource Monitor: Memory Cleaner already owns the
pressure model and runs in the background; computing it twice could give two answers.

Second wave:

- **Event Watch, boot time (#57).** `boot-time` from `Microsoft-Windows-Diagnostics-Performance/
  Operational` event 100 (`BootTime` ms) for the last 10 boots: Ok with "Last start took 38 s
  (usual: 35 s)", Attention/Watch when the last boot is over 90 s and over 1.5 x the median
  (both thresholds are proposals in one `const` block). Events 101 to 103 name slow apps, drivers
  and services; the Debloater Startup page shows them per entry (separate PR). Log needs admin on
  some PCs → Unavailable "Windows only lets administrators read the boot log on this PC".
- **Settings checks (#56) in Resource Monitor**, on a new read-only page **Checks** (open
  question 3). Why there: Resource Monitor already samples performance counters and reads the
  GPU, and the owner already decided it runs in the background for history (#64, moved to
  add-ons #36), so throttling can be watched over time at no extra process. All checks are
  read-only and need no admin; none changes anything; BIOS settings are explained, never changed.

  | id | Read | Attention when | Action |
  |---|---|---|---|
  | `ram-speed` | WMI `Win32_PhysicalMemory` `ConfiguredClockSpeed` vs `Speed` | configured < 90 % of rated: "XMP or EXPO looks off" (Watch) | Instruction: how to turn on XMP/EXPO in the BIOS setup |
  | `power-plan` | `PowerGetActiveScheme`, `PowerGetEffectiveOverlayScheme`, `GetSystemPowerStatus` (battery saver) | Power saver plan, "best power efficiency" mode on mains, or battery saver on mains (Watch) | Instruction: Settings > System > Power |
  | `memory-integrity` | `HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity` `Enabled` | never Attention: Ok with an honest note on the gaming cost and the security trade-off | Instruction: Windows Security > Device security > Core isolation |
  | `throttling` | PDH `\Processor Information(_Total)\% Performance Limit` and `% of Maximum Frequency`, sampled with the existing sampler | limit ≥ 10 % held 60 s while CPU load ≥ 50 % in the last 30 min (Watch) | OpenPage `Checks` |
  | `pending-restart` | `...\WindowsUpdate\Auto Update\RebootRequired`, `...\Component Based Servicing\RebootPending` keys exist | either exists (Watch) | Instruction: restart to finish updates |
  | `gpu-driver-age` | WMI `Win32_VideoController` `DriverDate` | older than 12 months (Watch) | Instruction: get the driver from the card maker's site |

  Each check is a pure function on a fake input struct with tests ("Done when" of #56).

## 5. Later, not in the first phases

- **#59 latency check** (opt-in, admin). Runs only when the user starts it, through the existing
  administrator helper (protocol 3 `AdminOperation`), in whichever add-on builds it. It reports
  a `latency` finding only after a run ("Last check 2 days ago: <driver> held the CPU 1.2 ms"),
  stale after 7 days. Until the first run it reports nothing, so Home never nags about it. No
  protocol change needed.
- **#55 stutter timeline and #60 before/after.** Both need history (#52, #64, now add-ons #36).
  When that exists they can report findings like any other ("Since you turned this off, starts
  are 12 s faster"); the finding format already carries text and a time. Out of phases 1 to 3.
- A catalog-driven "not installed" row per missing check is H4; nothing beyond that is planned.

## 6. Build order

One PR each, base `staging`. Gate: `bash ~/.claude/gate/gate.sh` (host) or
`bash ~/.claude/gate/gate.sh -p <addon>` (add-ons). Issues to open are named in **bold**.

### Phase 1: protocol and store (host repo `Gusseti/toolboks`)

**H1. Protocol 4: findings messages and version negotiation.** Risk **CRITICAL** (protocol,
handshake, supervisor). Files: `sdk/toolboks-addon-proto/src/{lib,handshake}.rs`,
`sdk/toolboks-addon-sdk/src/{lib,runtime}.rs`, `src/catalog.rs`, `src/supervisor/mod.rs`,
`src/addon_services.rs` (`launch_spec`), `examples/fake_addon.rs` (a `--protocol 3` mode),
`docs/ADDON-PROTOCOL.md`, `docs/ADDON-AUTHOR-GUIDE.md`. Acceptance tests:
- golden bytes: every v3 `AddonMessage` and `HostMessage` encodes to the same bytes under v4;
- a v3 fake add-on connects to the new host and runs (frames, status items) with `Hello` 3;
- the v4 SDK against a host sending `Hello` 3 agrees on 3 and sends no `Findings`;
- v4 to v4: `findings()` changes reach the host, also from `tick_background` while suspended;
- `CheckNow` is never sent to a v3 connection; `Findings` on a v3 connection is ignored;
- a peer at 5 is refused in the existing plain words.

**H2. `src/health.rs` store, validation and ranking.** Risk high (untrusted input). Wire into
`Services` (route `Findings`, `set_running` on stop/restart, `remove` on turn-off and uninstall).
No UI. Acceptance: unit tests with a fake clock for every limit in section 1 (17th finding,
duplicate and bad ids, route outside own pages, future `measured_at`, clamped `stale_after`,
control characters), out-of-date rules, kept-after-stop, removed-on-turn-off, 3-per-source cap
and the full ranking order.

### Phase 2: Home (host repo)

**H3. Home Health section.** Risk medium. `src/app/health.rs`, `home` in `src/app/shell.rs`,
"Health row" in `DESIGN.md`, a preview-only `--health-check <out>` (pattern of
`src/app/restore_check.rs`) that runs fake add-ons from fixtures and saves a screenshot.
Acceptance: the four states (attention, all OK, mixed with not checked/unavailable/out of date,
nothing reported) at 1060 x 720 and 150 % scaling, keyboard order; each #54 checkbox ticked
against a screenshot. Depends on the answer to question 1.

**H4. Catalog `checks` field.** Risk medium (catalog format; optional field, older hosts ignore
it). `shared/toolboks-catalog`: `checks: Vec<String>` (at most 6 names of 40 chars, default
empty), `tools/repo` writes it from the `.tbaddon`. Home: "Not checked: Recent crashes (Event
Watch is not running yet)" for installed add-ons that have not reported, and the names in the
nothing-reported sentence. Acceptance: catalog tests; Home view tests.

**H5. Host release** with `[3, 4]` (owner go). Must reach `main` and users before any A-phase
add-on release.

### Phase 3: add-ons (repo `fredhakon/toolboks-addons`)

**A1. Repin the SDK to protocol 4.** Risk **CRITICAL** (every add-on's protocol changes at once,
since the pin is workspace-wide). `Cargo.toml` rev, `tools/repo` (entry `protocol_version` 4,
`min_toolboks_version` = H5's version). Acceptance: full gate; every add-on's protocol tests pass
against its release exe; a v4 add-on runs in the H5 host (`addon-window.sh`, window check if
allowed).

**A2. Memory Cleaner: memory pressure finding.** Risk low. Tests on fake `Signals` sequences:
Ok, Under pressure, the 30-minute tail, unreadable source → Unavailable.

**A3. Disk Manager: drive health findings for every fixed drive in the background.** Risk low.
Tests on the existing NVMe/SATA fixtures; the admin and unsupported cases → Unavailable.

**A4. Event Watch: recent crashes finding** (and `background: true` if question 2 is yes).
Risk low. Tests on recorded event fixtures (none, one restart, two restarts, blue screen).

After A1 to A4 are released, #54's "Done when" is met: findings from three tools on Home.

### Phase 4: more checks (add-ons repo)

**A5. Event Watch: boot time (#57)**, then **A5b. Debloater Startup page: measured cost per
entry (#57)**. **A6. Resource Monitor: settings checks engine (#56)** (pure checks and findings,
`background: true` per #64), then **A6b. Resource Monitor: Checks page (#56)**. Each risk low to
medium, each with fake-input tests.

## 7. Questions for the owner

1. **Does Health replace the "Your toolkit" list on Home?** The list repeats the sidebar and the
   Add-ons page, which #54 asks Home not to do. Recommendation: yes, remove it from Home; keep
   the restore and update banners and the "Pick your tools" prompt when nothing is installed.
2. **May Event Watch run in the background** so crash and boot findings are current from start?
   Otherwise they appear only after Event Watch has been opened and then age into "Out of date".
   Recommendation: yes, one check at start and every 30 minutes; its idle cost is measured
   under #48 before release.
3. **Where do the #56 settings checks live?** Recommendation: a read-only "Checks" page in
   Resource Monitor (already sampling, already going background for history). The alternative is
   a new small read-only add-on, which costs a catalog entry, an icon and another process.
