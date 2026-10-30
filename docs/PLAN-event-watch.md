# Event Watch plan

New top tab **Event Watch**: reads Windows' own records to show live errors and what happened before a bluescreen, hard reset or hardware fault. Status: **building** (research notes: event sources, bug checks, prior art; not committed as files, summarised here).

## Decisions (owner)

| Topic | Decision |
|---|---|
| Core idea | Read Windows' own records (Event Log, crash dump header); toolboks writes nothing and keeps no log of its own |
| Watches for | Bluescreens and unexpected restarts, hardware errors (WHEA, disk, GPU resets), early warnings before crashes |
| Placement | New top tab "Event Watch" with pages Live, Crashes, Summary and an inspector |
| Wording | Plain language first, raw event (provider, id, level, data, Windows message) one click away in the inspector |
| Depth | Events plus the bug check code (name, plain sentence, usual cause tendency); no full dump analysis, no symbol downloads |
| Window | Last 30 days; 10 minutes before each crash |

## Findings that shape the design

- **Event 41 (Kernel-Power) is a symptom.** A nonzero `BugcheckCode` (decimal in the event) means a bluescreen; a nonzero `PowerButtonTimestamp` means the power button was held; all zero means power loss, a hard hang, or a bug check that could not be written. Event 1001 (WER-SystemErrorReporting) carries the code and four parameters as structured data and the dump path. Both are readable without admin.
- **Never key logic on message text** (it is localised, Norwegian on this PC): use provider, event id and structured data. Windows' own message text (EvtFormatMessage) is shown on demand only.
- **Anchor the "before" window on the time it stopped** (EventLog 6008, else the last record before the boot), not on Event 41's own time (the next boot). A hard hang loses the last seconds because log writes are buffered.
- **Dump may be missing** even when event 1001 names a path (seen on the owner's PC). Dump header reading is optional: code at 0x38, parameters at 0x40/0x48/0x50/0x58, FILETIME at 0xFA8 (offsets from two sources, not yet checked against a real dump). No module name is guessed from the dump; for hardware-like codes never name a driver.
- **Honest labels:** Needs attention / Worth watching / Probably harmless / Info, plus "known" vs "generic" entries. Many event ids are unverified: unknown ids get a generic line from the level and Windows' own text, not an invented explanation. No numeric health score.
- **Detect cleared or unreadable logs** and show "log covers from <date>" instead of "all clear". Log size defaults are not reliable, read the real oldest record.
- **Thresholds in the Summary are proposals** (for example repeated disk resets in 24 h); marked as such in the UI wording ("worth watching").
- **Privacy:** events stay in memory; copy report offers redaction of user names and paths.

## Architecture

`events::model` (final types), `events::source` (EventSource trait, real EvtQuery/EvtRender, channels System and Application, dump header parse), `events::catalog` (verified provider+id table), `events::bugchecks` (about 50 codes), `events::analysis` (group, detect crashes, digest), page code in `events::mod`/`page`. Worker thread for all Event Log reads; Live page polls every 2 s only while visible.

## Phases

1. Core and pages: Live, Crashes (timeline of the 10 minutes before), Summary, inspector, copy report. (this build)
2. After use: tune the catalog, add channels (WHEA operational), tray alert on a new crash, optional dump header reading with admin, drift of thresholds.
