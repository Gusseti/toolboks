# Moving the tools out of the app

This is the plan for where each tool goes and how the move works. It belongs to the epic
[#77](https://github.com/Gusseti/toolboks/issues/77). The design of the platform itself is in
[ADDON-ARCHITECTURE.md](ADDON-ARCHITECTURE.md); how to write an add-on is in
[ADDON-AUTHOR-GUIDE.md](ADDON-AUTHOR-GUIDE.md).

## The goal

Toolboks ships as a platform with no tools inside. The user picks tools in the add-on browser,
and each one is downloaded, updated and removed on its own.

## Two repositories

| Repository | Holds | Releases |
|---|---|---|
| [`Gusseti/toolboks`](https://github.com/Gusseti/toolboks) (this one, "the host") | The window, sidebar, add-on browser, first-run screen, settings, tray, self-update, supervisor, installer, catalog checker, SDK, protocol and shared crates | The Toolboks installer only |
| [`fredhakon/toolboks-addons`](https://github.com/fredhakon/toolboks-addons) ("the add-ons repo") | Every first-party tool, one folder each, and the catalog that lists them | One release per add-on version |

The add-ons repo may move to `Gusseti` later. The app reads its address from one constant,
`src/catalog.rs::URL`, and the release workflow takes the owner from GitHub, so a move is a
one-line change in the host.

Other publishers can keep an add-on in their own repository and have it listed in the catalog.
That is not open yet and needs its own plan.

## Where each tool goes

Every tool becomes a native add-on: its own exe, drawn inside the Toolboks window. The folder,
the crate and the exe share one name. The ID does not change, so a user's settings and data
follow the tool.

| Tool | ID (unchanged) | Code in the host today | Folder in the add-ons repo | Issue |
|---|---|---|---|---|
| Disk Manager | `toolboks.disk-manager` | Removed by the pilot | `addons/disk-manager/` | [#88](https://github.com/Gusseti/toolboks/issues/88) (code moved; publication and #132 rebuild pending) |
| Resource Monitor | `toolboks.resource-monitor` | `src/tools/resources/` (copy also in the add-ons repo; host copy goes with #94) | `addons/resource-monitor/` | [#89](https://github.com/Gusseti/toolboks/issues/89) (code moved and window-checked; publication pending) |
| Memory Cleaner | `toolboks.memory-cleaner` | `src/tools/memory/` (copy also in the add-ons repo; host copy goes with #94) | `addons/memory-cleaner/` | [#90](https://github.com/Gusseti/toolboks/issues/90) (code moved and window-checked; purge uses the helper; publication pending) |
| Event Watch | `toolboks.event-watch` | `src/tools/events/` (copy also in the add-ons repo; the host copy goes with #94) | `addons/event-watch/` | [#91](https://github.com/Gusseti/toolboks/issues/91) (code moved; publication pending) |
| Debloater | `toolboks.debloater` | `src/tools/debloat/` (copy also in the add-ons repo; host copy goes with #94) | `addons/debloater/` | [#92](https://github.com/Gusseti/toolboks/issues/92) (moved without administrator changes; those are #142) |
| System Profiles | `toolboks.system-profiles` | `src/native/profiles/` (copy also in the add-ons repo; host copy goes with #94) | `addons/system-profiles/` | [#92](https://github.com/Gusseti/toolboks/issues/92) (review only; applying and restoring are #142) |
| Audio Gainer (Aural) | `toolboks.aural` | `src/native/aural/` (copy also in the add-ons repo; host copy goes with #94) | `addons/aural/` | [#93](https://github.com/Gusseti/toolboks/issues/93) (code moved and window-checked; publication pending) |
| Macro Manager | `toolboks.macro-manager` | `src/native/macros/` (copy also in the add-ons repo; host copy goes with #94) | `addons/macro-manager/` | [#93](https://github.com/Gusseti/toolboks/issues/93) (code moved and window-checked; recording behavior needs a real-input check; publication pending) |

### What stays in the host

- The SDK and protocol: `sdk/toolboks-addon-sdk`, `sdk/toolboks-addon-proto`.
- Shared code that add-ons build against: `shared/toolboks-shared` (look, formatting,
  elevation check, storage), `shared/toolboks-debloat-engine` (used by Debloater and System
  Profiles), `shared/toolboks-catalog`.
- The tray's memory percentage. The host measures it with its own small sampler, so it still
  works with Memory Cleaner removed.

The add-ons repo uses these crates as git dependencies pinned to one host commit. All of them
move to a new commit together.

## The add-ons repo

```
addons/<name>/            one crate per add-on
  Cargo.toml              version = the add-on's version
  <name>.tbaddon          manifest: ID, title, version, what it can access, minimum host
  src/main.rs             calls run_addon from the SDK
  tests/protocol.rs       starts the built exe and talks the protocol to it
catalog/index.json        the list the app reads
tools/repo/               checks manifests and updates the catalog after a release
.github/workflows/        pull request checks and the release workflow
```

`addons/hello` is a placeholder used to try the release workflow. It is not a real tool.

## How a release reaches a user

1. **Version.** Raise the version in the add-on's `Cargo.toml` and `.tbaddon`, and merge to
   `main` in the add-ons repo.
2. **Tag.** Push a tag named `<name>-v<version>`, for example `disk-manager-v1.0.0`. A tag
   publishes a real release.
3. **Build.** The workflow checks the tag, builds and tests only that add-on on Windows, and
   attaches the exe and its SHA-256 to a GitHub release. It reports the exe size.
4. **Catalog pull request.** The workflow opens a pull request that adds the new version to
   `catalog/index.json` with its download address, size, SHA-256 and the host versions it works
   with. This step needs the `CATALOG_PR_TOKEN` secret.
5. **Merge.** Review and merge the pull request. Once the catalog is on `main`, every Toolboks
   install sees the new version. The catalog is not signed; see [catalog.md](catalog.md).

In the app:

1. The add-on browser downloads the catalog and refuses a list that is not valid.
2. The user picks an add-on and sees what it can access before anything is installed.
3. The installer downloads the exe and keeps it only if its size and SHA-256 match the catalog.
4. The supervisor checks the exe again before each start, starts it, and restarts it if it
   crashes (at most 3 times a minute).

On the user's PC, under `%LOCALAPPDATA%\Toolboks\`:

| Folder | Holds |
|---|---|
| `addons\<id>\<version>\` | The exe and the catalog entry it was installed from |
| `addons\<id>\active.json` | Which version runs |
| `addon-data\<id>\` | The add-on's own data. Kept when the add-on is removed, unless the user chooses to delete it |

## How to move one tool

Each tool follows the same steps. The pilot (#88) does them first and writes down what it
learns before the others start.

1. **Create the crate** at `addons/<name>/` in the add-ons repo, with the manifest and the
   protocol test.
2. **Move the code** from the host folder in the table above. Replace calls into the host with
   the SDK and the shared crates. Code that two add-ons need goes into a shared crate in the
   host, not into both add-ons.
3. **Keep the ID, the page names and the preference names**, so `--open "Tool/Page"` links and
   saved settings keep working.
4. **Use the host services** for anything outside the page: status bar items, tray entries, the
   guided tour and background running.
5. **Use protocol 3 administrator helpers** for each action that needs administrator rights. Keep the drawing process unelevated; the helper owns its Confirm/Cancel dialog. See #132 and the author guide.
6. **Release it** with the steps above and install it from the catalog in a preview build.
7. **Compare with the built-in tool**: same pages, same numbers, same behaviour. Measure exe
   size, start time and memory against the budgets in
   [#97](https://github.com/Gusseti/toolboks/issues/97).
8. **Remove that tool's built-in code from the host** in the same change that makes the catalog
   version the one users get. During the move, the built-in tool and the add-on must not both
   appear.

What each tool needs beyond the common steps:

| Tool | Extra care |
|---|---|
| Disk Manager | Fragmentation analysis uses the existing elevation request. Defrag, TRIM and Windows-selected optimization are gated on #132; repin and rebuild after its protocol 3 helper merges. Results and remaining checks: `docs/reviews/88/`. |
| Resource Monitor | Runs in the background so the history recorder (#64) keeps recording with the window hidden. The history files live in its data folder. |
| Memory Cleaner | Runs in the background for automatic cleaning. Some actions need administrator rights. |
| Event Watch | Checked: no page needs administrator rights. Event logs are read per channel, and a channel the user cannot read is reported in the page header as before. Crash dump headers are read as files (`filesystem_read`). It changes nothing and has no saved settings. |
| Debloater, System Profiles | They change Windows. Undo information must survive an update or removal of the add-on. Use the per-change helper boundary decided in [#118](https://github.com/Gusseti/toolboks/issues/118) and implemented in #132; real UAC validation is still required. |
| Audio Gainer, Macro Manager | Global input recording and live audio must work from a background process. The install review must name these rights plainly. |

## Status, 2026-11-15

**Update, same day:** all eight tools are released (`fredhakon/toolboks-addons` tags) and listed in the public catalog, the Debloater's administrator changes work through the helper (#145), and the host no longer contains any tool (#94): the built-in tools, host-linked providers, bundled manifests and per-tool tours are gone, the host exe is 6.77 MB (it was 8.24 MB with seven tools still built in), and updating from an earlier version offers "Restore your tools". Still open: a real UAC run of the administrator operations, and the host release itself.

All eight tools are in the add-ons repo and each was installed from a test catalog and opened in
a real window with the preview app's `--addon-check <id> <folder>`. Found that way: an add-on
process has no fonts of its own, so each add-on applies the Toolboks look when it first draws,
and the shared font set-up loads Consolas for fixed-width text. Still open: every change that
needs administrator rights in Debloater and System Profiles (#142), real UAC tests, the first
releases and catalog entries, and the migration (#94). Until #142 is done, removing the built-in
Debloater and System Profiles would take features away from users.

## Order

1. **Before any tool moves:** the add-on browser ([#86](https://github.com/Gusseti/toolboks/issues/86)),
   the security findings ([#95](https://github.com/Gusseti/toolboks/issues/95)), a public
   catalog ([#116](https://github.com/Gusseti/toolboks/issues/116)) and the
   administrator-helper implementation and validation ([#132](https://github.com/Gusseti/toolboks/issues/132); owner decision #118).
2. **Pilot:** Disk Manager (#88).
3. **The rest:** #89 to #93, in any order, after the pilot.
4. **Last:** remove everything built in and migrate existing users
   ([#94](https://github.com/Gusseti/toolboks/issues/94)). At the first start of that version
   the app offers "Restore your tools", which downloads the tools the user had turned on, with
   their settings kept.

Until step 4, the tools that have not moved stay built into the app and keep working.

A tool that is built into the app today (Resource Monitor, Memory Cleaner, Event Watch,
Debloater) is added to the add-ons repo first and keeps its built-in copy in the host until
step 4, so nobody loses the tab or sees the tool twice. Its host code is removed together with
the migration in #94. Disk Manager was already an official add-on, so its host copy went with
the pilot.

## Open points

- The add-ons repo is public and its catalog is an unsigned plain list. Disk Manager still needs its first owner/orchestrator release and catalog entry.
- The protocol 2 pilot must be repinned and rebuilt after #132 merges to staging, before it can run on that protocol 3 host.
- The add-ons repo is pinned to a host commit on `staging`. Before the first real release it
  should point at a released host version ([#117](https://github.com/Gusseti/toolboks/issues/117)).
- Nothing is signed: not the catalog and not the exes (owner decision, 2026-11-15). Windows
  SmartScreen and antivirus programs are likely to warn about each download.

## Protocol 3 migration (#132)

Nothing is published and the catalog is empty. Rebuild `addons/hello` in
`fredhakon/toolboks-addons` against the host commit containing #132. Its root `Cargo.toml`
currently pins `5f7c39b04dc367379be0eb9396cfb7362af2ffa3`: update all four dependencies
(SDK, protocol, shared UI and catalog) together and regenerate `Cargo.lock`. Rebuild/test
`addons/hello` and `tools/repo` (`addon-repo`, which generates catalog entries using the
protocol constant). Update the repo's README pin/protocol note and use `protocol_version: 3`
in generated entries. The pin update also needs the current catalog `Entry::byline` field
in `tools/repo/src/main.rs` (for example `byline: m.byline`), added upstream since that pin.
The unsigned catalog types from #134 also remove signing/sequence/date fields from new
`Entry`/`Index` initializers in that tool; migrate its tests alongside it. Rebuild the host repository's
hello example, test hosts and fixtures too (their `Hello::new` uses the current version).
Version 2 entries are hidden and v2 handshakes receive a plain update message. No catalog
is published, release created, or tool moved by this change.
