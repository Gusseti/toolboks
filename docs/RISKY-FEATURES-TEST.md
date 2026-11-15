# Test checklist: the risky features, on a real system

This is the checklist for issue [#49](https://github.com/Gusseti/toolboks/issues/49), "Test the
risky features on a real system". Follow it before a release, in a Windows 11 virtual machine
first, and then with the "harmless" items only on a real PC.

**Nothing in this file has been run.** It was written from the source code of the host and of the
add-ons (the file named under each feature). Every result column is empty until a person fills it
in. Do not tick a row you did not do. The automated tests mostly use fakes, which is why this
list exists.

Status of the code it was written against: host `staging` after 0.13.2, add-ons repo `staging`
(Memory Cleaner 0.1.0, Debloater 0.1.0, Disk Manager 0.3.0, plus System Profiles, Macro Manager
and Resource Monitor as in `addons/<name>/`). If the labels on screen differ from the ones quoted
here, the code changed: note it in the results table and fix this file.

## The rules

1. **Every failure becomes its own GitHub issue**, one per failure, with the exact steps, what you
   expected, what happened, the Toolboks and add-on versions, and the Windows build. Put the issue
   number in the results table. Do not fix a failure and carry on without an issue, and do not
   bundle two failures in one issue.
2. **Undo after every test.** Each section says how. Do not start the next item until the last one
   is undone or the VM is back on its snapshot.
3. **A "VM only" item is never run on the owner's PC**, not even with a backup.
4. **A "harmless item" is only run on the owner's PC with the owner's go-ahead for that item**,
   one item at a time. UAC prompts are always clicked by the owner.
5. Do not run any window check while the owner is gaming or away from the screen.
6. A test is **pass** only if the result you saw matches "Expected" in this file *and* the undo
   put things back. Otherwise it is **fail**. If you could not do a step, write **not run** and say
   why. Never leave a row blank and never write "pass" for something you inferred.

## Before you start

### 1. Pick the machine

- **VM (the real test).** A Windows 11 VM (Hyper-V, VMware or VirtualBox) with at least 8 GB of
  RAM, one system disk, and one extra small virtual disk (for Disk Manager). Install a normal
  Windows 11 Pro. Pro matters: several Debloater and System Profiles items need Pro and are
  skipped on Home. If you can, also keep a Home VM and run the Debloater and System Profiles
  sections there once, to see the "not available on this edition" skips.
- **Take a snapshot** named `clean-before-toolboks` before you install anything, and another
  named `toolboks-installed` after the first successful add-on install. Revert to a snapshot
  whenever a section says "VM only" and you are done, or whenever something looks wrong.
- Turn System Protection (System Restore) **on** for `C:` in the VM, in Windows Settings > System >
  About > System protection. Debloater and System Profiles restore points need it.
  System Profiles can also turn it on for you; test both ways (see its section).
- **Real PC (short pass, harmless items only).** Only the rows marked "harmless item" below. The
  owner reads the item first and says go.

### 2. Which builds

| What | Build | Why |
|---|---|---|
| Host, for everything except the self-update | **Preview build**: `cargo build --features preview` (host repo, build output on D:). | A preview build has its own single-instance lock and its own data folder, so it never joins or touches a normal Toolboks. It also has updates turned off ("This is a preview. Updates are turned off." in Settings). |
| Host, for the self-update handoff only | A **released** `toolboks.exe` (an older release) in a throw-away folder, in the VM. | The preview build refuses to update (`src/update.rs`, `install`). |
| Add-ons | Installed from the add-on list inside the host, or from a test catalog (see below). | That is how users get them. |

Never start the owner's normal Toolboks for these tests, never close it, never kill it. A normal
start joins the owner's running copy (`src/instance.rs`), so on the owner's PC use the preview
build only. In the VM nothing else runs, so a normal build is fine there.

### 3. Data folders: what to copy, where results go

Toolboks keeps everything under one root (`shared/toolboks-shared/src/storage.rs`, `data_root`):

| Folder | What is in it |
|---|---|
| `%LOCALAPPDATA%\Toolboks` | The owner's real data (normal build). **Never write to it.** |
| `<TOOLBOKS_DATA_DIR>\Toolboks Preview` | The data of a preview build when `TOOLBOKS_DATA_DIR` is set. (A non-preview build uses `<TOOLBOKS_DATA_DIR>\Toolboks`.) |
| `<root>\config.json` | Host settings, including `keep_in_tray`, `platform_migrated`, `restore_pending`. |
| `<root>\addons\<id>\<version>\` and `active.json` | Installed add-on versions, the one that runs, and the previous one kept for rollback (`src/installer.rs`). |
| `<root>\addon-data\<id>\` | An add-on's own data: Disk Manager saved scans under `addon-data\toolboks.disk-manager\scans`, Macro Manager macros, Memory Cleaner settings. |
| `<root>\Gaming profile\` | System Profiles recovery folder: `active.json` (the journal), `<id>\registry-before.reg` (the registry export), `activity.log`, `run.lock`. |

Set up a throw-away data folder for each run:

```powershell
$env:TOOLBOKS_DATA_DIR = "D:\tb-test\run1"        # any empty folder
$env:TOOLBOKS_TEST_CATALOG = "D:\tb-test\catalog"  # optional: a folder with index.json and the add-on exe
```

`TOOLBOKS_TEST_CATALOG` makes the add-on list come from a local folder instead of GitHub. Build
one with `~/.claude/gate/mkcat.py <catalog/index.json> <add-on id> <version> <exe> <out folder>`
(it writes a one-entry catalog with the exe's real size and SHA-256). Without the variable the
host reads the real public list.

To test with the owner's real settings, **copy** `%LOCALAPPDATA%\Toolboks` into
`<TOOLBOKS_DATA_DIR>\Toolboks Preview` and never point a test at the original. The owner's
`config.json` may have `"keep_in_tray": true`: turn it off in the copy or the preview window
starts hidden.

Before each section that writes to the system, also copy these as a manual backup (VM or PC):

- `<root>` as above (host data).
- Export the registry keys the section touches, as written in its "What it changes" line, with
  `reg export <key> <file>.reg`.
- For services and tasks: write down the start type and state (`Get-Service <name> | Select Name,StartType,Status`,
  `Get-ScheduledTask -TaskPath <path> -TaskName <name>`).

### 4. Automated checks first

Run the gate once before the manual pass, so you know the baseline is green:

```
bash ~/.claude/gate/gate.sh                 # host repo: format, lint, all tests
bash ~/.claude/gate/gate.sh -p <addon>      # add-ons repo: one add-on, plus its protocol tests against the release exe
```

The ignored tests (they need a real desktop or read real hardware) can be run on the VM with
`cargo test -- --ignored --test-threads=1` in the crate named in each section. Their names are
listed per feature below. They open windows, so the same "not while the owner is gaming" rule
applies.

## What the automated tests cover today, and what they do not

| Layer | Covered by tests (fakes, parsers, rules) | Only a person can check |
|---|---|---|
| Add-on code | Decisions (which process may be trimmed, which registry value to write, request validation, undo codec, text of the confirm window), round trips through fake registries, services, tasks, startup stores, and a fake restore API. | That Windows really accepts the call: the registry really changes, a service really stops, a restore point really appears, a process really changes priority, input really arrives in another app. |
| Admin helper (`sdk/toolboks-addon-sdk/src/admin*`, `src/supervisor/admin.rs`) | Cancel performs no change, one confirm runs once, a click, key or forged result from the host cannot confirm, a disconnect cancels, oversized requests fail closed (`sdk/toolboks-addon-sdk/src/admin/tests.rs`); the host refuses a helper without a saved catalog before UAC (`src/supervisor/tests.rs`). | Real UAC consent and decline, the native Confirm/Cancel window appearing in front, readable and in the right place, keyboard focus on Cancel. The only native-window test is ignored: `dialog_window_is_created_by_this_process_and_cancellation_closes_it` (`sdk/toolboks-addon-sdk/src/admin/dialog.rs`). |
| Host updater (`src/update.rs`) | Version compare, release parsing, checksum, URL trust. One ignored test downloads the published release. | The swap of the running exe, the restart, the `.old` cleanup, a failed swap. |
| Installer (`src/installer.rs`) | Local test server: install, bad hash, resume, update and rollback when the new version never starts, remove with and without data, daily check. | The same on a real PC with real downloads, real processes starting and dying, antivirus in the way. |
| Tray (`src/tray.rs`) | Icon drawing, colours, tooltip text. | Hiding to the tray, bringing the window back, Quit, background work continuing while hidden. |
| Real-window checks (`--addon-check`, `--restore-check`, preview build) | Installs an add-on from a test catalog and screenshots it. | Everything not in a screenshot. |

Real execution of any risky action (defrag/TRIM, a trim of real apps, a real cache purge, a
service switched off, an app removed, a restore point, a self-update) has **not** been recorded
anywhere in the repo as of this file. Earlier validation notes
(`docs/reviews/132/validation.md`, `docs/DRIVE-OPTIMIZATION.md`) say so explicitly for the helper
and for drive optimization.

---

## Part A: the add-ons

Use the add-on list in the host ("Add-ons" in the sidebar) to install each add-on, then open it
from the sidebar. Installing an add-on is itself tested in part B.

### A1. Memory Cleaner

Source: `addons/memory-cleaner/` (`src/home.rs`, `src/actions.rs`, `src/trim.rs`,
`src/scheduler.rs`, `src/admin.rs`, `src/advanced.rs`, `src/memlist.rs`) and the shared list
`crates/protected-processes/src/lib.rs`. Capabilities: `host_metrics`, `system_tuning`.
Pages: **Home** and **Advanced**.

**What it changes.** Only memory management of other processes in your own session:

- *Trim idle apps*: lowers the memory priority of idle apps (`SetProcessInformation`,
  priority 2) and asks the biggest idle ones to move unused memory to the standby cache
  (`EmptyWorkingSet`). It closes nothing and needs no admin rights.
- *Clean automatically*: the same, started by the scheduler. Off by default.
- *Clear low-priority cache* (Advanced): the admin purge of the low-priority standby list for the
  whole PC (`purge_standby`, through the admin helper).
- *Trim selected* (Advanced > Selected): trims the apps you ticked.

**Safety on the owner's PC.**

| Item | Where |
|---|---|
| Trim idle apps (manual) | Harmless item. It only moves cache around and can be undone with "Undo priority change". Run it with a few ordinary apps open, **never with a game, a launcher or an anti-cheat running** (they are skipped by design; that is exactly what step 6 checks, so check it in the VM, not on the owner's PC). |
| Clean automatically | VM only for the full wait; on the owner's PC only the on/off switch and the status line. |
| Clear low-priority cache | **VM only.** It can cause short stutters while apps and games read from disk again, and cannot be undone (it is safe to repeat). Owner's go for one run at most. |
| Protected-process skip | The *rule* is checked in the VM with a fake launcher (step 6). |

**Steps and expected results.**

1. *Trim idle apps, manual.* In the VM open about six apps (a browser with several tabs, Notepad,
   Paint, Calculator, Edge, Explorer windows). Leave them alone for more than one minute (manual
   runs only touch apps idle for a minute; automatic needs five). Open Memory Cleaner > Home.
   Note the numbers under "Where your memory goes". Click **Trim idle apps**.
   - Expected: the button shows "Trimming...", then "Last run" fills in with how much moved to the
     cache and what came back within two minutes, and the run is labelled "Trim idle apps". The
     note under the button says it does not free memory. No app closed. The app in front was not
     touched. If nothing qualified, the page says so ("Nothing to trim right now...") and that is
     also a pass.
   - Also: in Task Manager the trimmed apps' working set dropped and grows again when you use them.
2. *Undo.* Click **Undo priority change (N)** next to "Copy report".
   - Expected: it puts back the memory priority of the apps the run lowered, the count goes to
     zero, the button greys out. (Check with Process Explorer > Memory priority column, or trust
     the status line and the next trim list.)
3. *The apps in front, audio and full screen.* Put Notepad in front and run Trim idle apps again.
   - Expected: Notepad is not trimmed. Then play a video in a browser tab and run it from another
     window. **Known gap to record, not assume:** the audio check in `actions.rs`
     (`plays_audio`) is a stub that always returns false, so an idle app that is playing sound
     is not protected by it. Write what you saw; if the player was trimmed or lowered, that is a
     bug to file.
4. *Full screen.* Run a full-screen video or slideshow in front and start an *automatic* run
   (step 7). Expected: the full-screen app is skipped.
5. *Advanced > Selected.* Open Advanced, tick two apps, press **Trim selected (2)**, and press
   **Protect** on one of them (listed under "Protected apps (N)"). Expected: the protected app is
   not trimmed ("N protected processes will be skipped") and **Unprotect** reverses it. The
   last result shows under "Last trim".
6. *Protected-process skip list (VM, with fake stand-ins).* Make a copy of any small exe and
   rename the copies to names on the lists, then start them:
   `steam.exe`, `EasyAntiCheat.exe`, `cs2.exe` (game, anti-cheat, launcher families from
   `crates/protected-processes/src/lib.rs`), and a copy of Notepad renamed `IndieGame.exe`
   started *by* a copy renamed `steam.exe` (a child of a launcher). Add the two System names
   by looking at real ones in Task Manager: `explorer.exe`, `dwm.exe`, `svchost.exe`, `MsMpEng.exe`.
   Make the stand-ins big: a trim needs a working set of at least 100 MB (manual) and a priority
   change at least 25 MB, so use a small program that allocates and holds about 200 MB. A tiny
   stand-in would be skipped as "too small", and then the test proves nothing about the list.
   Leave them idle for more than a minute (five minutes for an automatic run).
   Run **Trim idle apps** and then Advanced > Selected.
   - Expected: none of these is trimmed or lowered. In the Advanced list the reason is shown
     (for example "Protected: game launcher, Toolboks leaves it alone." or "System and shell
     processes are never trimmed"). The skip is checked again against the live process name right
     before a write handle is opened, so renaming a running process in place does not work as a
     test. Start each copy fresh.
7. *Clean automatically.* Switch **Clean automatically** on (Home, "Status:" line explains it).
   In the VM give it less memory than the apps want (for example 2 GB of VM RAM and a memory
   hog, or run a small program that allocates and holds memory), so the verdict is "Under
   pressure" for a minute.
   - Expected: nothing happens before the pressure has lasted 60 seconds. The run starts only
     once, then never again for an hour (the status line counts it down). The run is labelled
     "Automatic" and shows its trigger. It does not run if commit is at or above 95 percent (the
     page advises closing apps). It pauses when you switch to another tool page or hide Toolboks
     in the tray (see B1). Press **Turn off auto-clean** to stop it.
   - Undo: switch it off; use Undo priority change.
8. *The admin purge, VM only.* Advanced > **Clear low-priority cache** ("Cache now: N" shows the
   size first).
   - Expected: the helper's own window appears (see B7): title "Confirm administrator change", text
     starting "Clear the low-priority cache?", and the line that it cannot be undone but is safe
     to repeat. Toolboks shows "Waiting for the administrator window..." and disables the button.
     Press **Cancel**: "Cancelled. Nothing was changed." and the cache size is unchanged. Press the
     button again and **Confirm** (after accepting UAC): a line like "Cache (standby) went from
     X to Y. Commit is unchanged." and "Cache now" updates. Decline UAC: also "Cancelled".
   - Undo: none (Windows refills the cache). Repeat is safe.
9. *Hiding.* Hide to the tray while a run is in progress. Expected: a running trim finishes and
   reports; nothing new starts while hidden.

**Automated coverage.** `addons/memory-cleaner`: eligibility rules and every skip reason
(`actions.rs` tests including `skips_system_security_and_launchers`,
`skips_audio_fullscreen_and_recent_or_unknown_input`, `manual_policy_keeps_the_same_protections`),
the real priority round trip on its own child process (`real_priority_round_trip_on_own_child`),
the protected-process lists (`crates/protected-processes`), the scheduler rules (pressure hold,
one-hour cooldown, commit near limit), the purge request/answer handling and the confirm text
(`admin.rs` tests), and the add-on protocol test against the release exe
(`tests/protocol.rs`). **Not covered:** a trim of real third-party apps, a real purge, the
scheduler running against real memory pressure, full-screen detection, audio, and the helper
window.

### A2. Debloater

Source: `addons/debloater/` (`src/page.rs`, `src/admin.rs`) and the shared engine
`shared/toolboks-debloat-engine/` (`catalog.rs`, `plan.rs`, `apps.rs`, `startup.rs`,
`tasks.rs`, `services.rs`, `restore.rs`). Capability: `system_tuning`. Pages: **Privacy and ads**,
**Apps**, **Startup**, **Tasks**, **Services**.

**What it changes.** Per page:

| Page | What changes | Where | Undo in Toolboks |
|---|---|---|---|
| Privacy and ads | Registry values from the catalog (about 26 items, "Safe" or "Careful"). Items for your own account write `HKCU`; items marked "all users of this PC" write `HKLM` (policies) and need the admin helper. | `HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager` and others; `HKLM\SOFTWARE\Policies\Microsoft\...` | "Undo last run" until Toolboks is closed. |
| Apps | Removes preinstalled consumer apps (Clipchamp, News, Weather, Solitaire, Tips, Feedback Hub, Skype, Mixed Reality Portal, Cortana, Copilot, Teams personal chat, People, Maps) **for your account only**, with WinRT `PackageManager`. | The account's installed packages. | **No undo in Toolboks.** Reinstall from the Microsoft Store (the page says so). A restore point does not bring apps back. |
| Startup | Switches an entry off or on through the Task Manager style `StartupApproved` key. The entry itself is not deleted. Entries classed "Never" (security and system) cannot be changed. | `HKCU`/`HKLM ...\Explorer\StartupApproved\Run`, `Run32`, `StartupFolder` | "Undo last change" and "Undo all" until closed. |
| Tasks | Disables a scheduled task from the list (CEIP consolidator and USB, feedback, error reporting queue, device info and others). | Task Scheduler. | "Undo last change" and "Undo all". |
| Services | Sets a service's start type to Disabled or Manual from the catalog (for example `DiagTrack`, `Fax`, `RetailDemo`, `MapsBroker`, `SysMain`, `Spooler`, Xbox services). A "never offered" list (for example `WinDefend`, `BITS`, `wuauserv`, per-user `_xxxxx` services) is refused. | Service Control Manager. | "Undo last change" and "Undo all". |

Undo data lives in memory only: **if Toolboks closes, the undo steps are gone.** Record the
original values before you start (see "Data folders" above) so a human can put things back.

**Safety on the owner's PC.**

| Item | Where |
|---|---|
| Privacy and ads items for your own account only, "Safe", for example *Lock screen tips and ads*, *Tip and suggestion notifications*, *Welcome screen after updates* | Harmless item, with the owner's go for each one. Undo right away. |
| Anything that says "all users of this PC" (HKLM), any "Careful" item, Recall, Edge policies, clipboard history, activity history | VM only. |
| Restore point | VM only (it writes a real system restore point; Windows allows one per 24 hours by default and says so). |
| Apps removal | **VM only.** Not undoable in Toolboks. |
| Startup, an entry of your own account that you added for the test (a copy of Notepad in `HKCU\...\Run`) | Harmless item. Entries of real software: VM only. |
| Tasks, Services | VM only. |

**Steps and expected results (do each section in the VM; revert to snapshot between sections if in doubt).**

1. *Privacy and ads, account only.* Export `HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager`.
   Tick one "Safe" item with scope "your account" (for example *Lock screen tips and ads*), click
   **Apply 1 change**; the inline question appears: "Apply 1 change? You can undo them until you
   close toolboks." Click **Apply**.
   - Expected: no helper window (a user-scope change needs none). The item shows as applied, the
     registry values named in the item (`RotatingLockScreenOverlayEnabled`, ... = 0) changed
     (check with `reg query`). Items that were already off are skipped.
   - Undo: **Undo last run**. Expected: the values are back to what the export shows
     (a value that did not exist is removed again).
2. *Privacy and ads, machine items, with the helper.* Tick one "all users of this PC" item.
   Click Apply and **Apply**.
   - Expected: UAC, then the helper's window (title "Confirm administrator change") with the
     text: "Change these Windows settings?", then for each item "• <title> (Safe|Careful, all
     users of this PC)", what changes, "Could stop working: ...", and the closing lines "Items
     that are already off, or not available on this edition, are skipped. You can undo this run in
     Toolboks until you close it." If more than one item is chosen, all are listed. **Cancel**:
     nothing changes, Toolboks shows that it was cancelled. Run again and **Confirm**: the `HKLM`
     value changes. On a Home edition, an item that needs Pro is skipped and says so.
   - Undo: **Undo last run** opens the helper window again: "Put these Windows settings back?"
     with lines "HKLM\\<path> · <name> → <previous>" ("removed" if there was none). Confirm.
     Expected: the registry matches the export from before.
3. *Restore point option.* Turn on **Create a restore point first** (needs the helper; the note
   "A separate administrator window asks you to confirm." shows) and apply any one item. The
   helper text adds "Windows will create a restore point first. This can take about 20 seconds."
   - Expected: "Creating restore point..." is shown, then the run; a restore point named
     "toolboks: privacy and ads changes" (or "toolboks: switch off service <title>" /
     "toolboks: switch task <title>") exists in `rstrui.exe` and in
     `Get-ComputerRestorePoint`. If System Protection is off, or a point was made in the last
     24 hours, the page must say so and (when blocking) the change must **not** run. Check both
     cases: protection off (change not applied, clear message) and a second apply within a day
     (reported as "recent point exists", change applied). The Tasks and Services pages make one
     point per session ("A restore point was already made this session.").
   - Undo: the restore point stays; remove it in System Protection > Configure if you want.
     Note that a restore point does not bring back removed apps.
4. *Apps.* Pick one app you do not need in the VM (for example Clipchamp). Click **Remove 1 app**;
   the question reads "Remove ... for your account? Removed apps can be reinstalled from the
   Microsoft Store." Click **Remove**.
   - Expected: "Removed 1, failed 0"; the app is gone from Start for this user. Failures list the
     reason line by line.
   - Undo: reinstall from the Microsoft Store (or revert the VM snapshot). There is no Undo
     button for apps. Confirm that the page says so.
5. *Startup.* Create a test entry: `reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v TbTest /d "C:\Windows\System32\notepad.exe"`.
   On the Startup page find "TbTest", press **Switch off**, then **Switch off** in the inline
   confirm.
   - Expected: Task Manager > Startup apps shows it Disabled; the `Run` value is still there; the
     `StartupApproved\Run` value `TbTest` starts with `03`. For an "all users" entry (HKLM or the
     common Startup folder) the helper window opens with "Switch this startup item off?" plus
     "Publisher: ...", "Runs: ..." and for a "Careful" item "This may be hardware software, a VPN
     or something unknown. Switching it off can stop that from working." Security and system
     entries must be refused ("is a security or system component and cannot be changed here").
   - Undo: **Undo last change** / **Undo all**; then `reg delete HKCU\...\Run /v TbTest` and delete
     the `StartupApproved` value.
6. *Tasks.* Record the state of `\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip`
   (`Get-ScheduledTask`). On the Tasks page switch it off through the helper.
   - Expected: helper text "Switch this scheduled task off?" with the title, "Task: <path>" and
     "Windows checks the task again before changing it."; after Confirm the task is Disabled.
     Cancel changes nothing.
   - Undo: **Undo last change**; the helper shows "Put these scheduled tasks back?" with "→ on" or
     "→ off". Confirm; the state equals your note.
7. *Services.* Record `Get-Service Fax` (or `RetailDemo`: a service that is normally disabled or
   absent on a clean VM is a good safe pick; you can also take `DiagTrack`). Switch it off on the
   Services page.
   - Expected: helper text "Switch this Windows service off?", the title, "(Safe|Careful, all users
     of this PC)", "Could stop working: ...", "Its start type becomes: ..." and "Windows checks the
     service again before changing it." After Confirm, `Get-Service` shows the new start type
     (the service is not stopped by the change; check whether it keeps running until reboot and
     write down which). A service on the never-offered list never appears as a choice.
   - Undo: **Undo last change**; helper: "Put these Windows services back?" with "Start type →
     <previous>". The start type equals your note.
8. *Edition and stale state.* Close Toolboks after a change **without** undoing, reopen: the page
   must show the current system state (applied), the Undo button must be unavailable, and nothing
   may be changed behind your back. Then undo by hand from your backup (registry import, service
   start type, task enable).

**Automated coverage.** The engine has unit tests with fake registry, packages, services, tasks,
startup store and restore API, including read-back and restore-on-failure
(`set_enabled_read_back_failure_restores`), the catalog rules (no never-offered service in the
list; Safe items first), and the add-on tests check the request codec, that undo steps outside
the catalog are refused (`undo_steps_outside_the_catalog_are_refused`), that the confirm text is
built from the catalog and not from the request (`the_apply_preview_comes_from_the_catalog`), and
that an undo batch never writes an older value over a failed newer one
(`an_undo_batch_never_puts_an_older_value_over_a_failed_newer_one`). Read-only smoke tests against
the real registry, packages, services, tasks and startup entries exist and are ignored
(`smoke_real_registry_read_only`, `smoke_list_real_packages`, `real_states_smoke`,
`real_statuses_smoke`, `real_listing_smoke`, `smoke_read_only`). **Not covered:** any real write,
a real restore point, real app removal, and the helper window. The docs at
`docs/PLAN-debloater.md` say the same ("never applied to the real system").

> Note while writing this file: `addons/debloater/debloater.tbaddon` still says that changes that
> need administrator rights "are listed but not available yet", but the code now sends them to the
> helper. If the add-on's own text disagrees with what you see, record it.

### A3. System Profiles (Gaming profile)

Source: `addons/system-profiles/` (`src/lib.rs`, `src/engine.rs`, `src/admin.rs`). Capability:
`system_tuning`. Page: **Gaming**. Everything goes through the admin helper.

**What it changes.** Only what you tick, after a review:

| Choice (title on screen) | Change |
|---|---|
| Turn on Game Mode | `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled` = 1 |
| Turn off Game DVR | the catalog's `game-bar-capture` values (`HKCU`) |
| Apply gaming scheduler settings | `HKLM ...\Multimedia\SystemProfile\Tasks\Games\Scheduling Category` = High, `HKLM\SYSTEM\CurrentControlSet\Control\PriorityControl\Win32PrioritySeparation` = 0x26, `HKLM ...\Multimedia\SystemProfile\SystemResponsiveness` = 10 |
| Activate Ultimate Performance | `powercfg /duplicatescheme e9a42b02-...` and activates the new plan |
| Privacy and cleanup choices | suggested app installs, diagnostic data, activity history, location (`HKLM`, Pro), PowerShell 7 telemetry (`HKCU\Environment`), Storage Sense (`HKLM`, Pro) |
| *Turn on System Protection for the Windows drive* | `Enable-ComputerRestore` for the system drive |

Before anything changes the engine (a) exports the old values to
`<root>\Gaming profile\<id>\registry-before.reg`, (b) creates and verifies a **new Windows restore
point** ("Toolboks Gaming <id>") with PowerShell `Checkpoint-Computer`, (c) writes a journal
(`active.json`) before each value, and only then writes. **Restore** puts back the values
Toolboks wrote (and removes the power plan it created), and leaves a value that something else
changed since.

**Safety.** The apply always creates a restore point and needs UAC and the helper, so it is **VM
only** even for the "harmless" HKCU choices. If the owner wants a pass on their own PC, the only
candidate is *Turn on Game Mode* together with *PowerShell 7 telemetry* (both `HKCU`), with a
restore point accepted by the owner and restored right after. Owner's go required.

**Steps and expected results.**

1. Open the Gaming page. Expected: nothing ticked by default; **Apply Gaming profile** is
   disabled with "Review changes first. Apply unlocks for exactly what you reviewed."
2. Tick *Turn on Game Mode* and *Apply gaming scheduler settings* and *Activate Ultimate
   Performance*. Click **Review changes**. Expected: "Now, then new value" lists every registry
   value (and the power plan) with the current and the new value. Change a tick: the review is
   dropped and Apply locks again until you review again.
3. Click **Apply Gaming profile**. Expected: UAC, then the helper window "Apply the Gaming
   profile?" with "Before anything changes, Windows must confirm a new restore point, and the
   current values are saved to a registry backup. Neither guarantees a complete recovery, so
   review the changes first." and the change list, ending "Restore puts back the settings
   Toolboks applied." **Cancel**: nothing changes, no `Gaming profile` journal, no new restore
   point. Apply again and **Confirm**: the activity log shows, in order, "Registry export saved
   to ...", "Creating and verifying a new Windows restore point.", "Windows confirmed restore
   point N.", then "Applying <value>." and "Verified <value>." for each, then the power plan
   lines. Check: the `.reg` backup exists; `Get-ComputerRestorePoint` lists "Toolboks Gaming ...";
   `reg query` shows the new values; `powercfg /getactivescheme` shows the new plan.
4. *Recovery folder.* Open **Open backup location** (`<root>\Gaming profile`). Expected:
   `active.json`, `activity.log`, `run.lock` and `<id>\registry-before.reg`. While `active.json`
   exists the choices are greyed out and the button says **Review registry edits**; a second apply
   is refused ("Restore the previous Gaming profile before applying another.").
5. **Restore.** Click **Review registry edits**, check the list, click **Restore**; the helper
   window says "Restore the settings Toolboks applied?" and "Registry edits to reverse:". Confirm.
   Expected: "Verified original value for ..." per value and "Removed the Gaming power plan and
   kept or restored the active plan.", then `active.json` is gone. The values match your own
   export; the old power plan is active again; `Win32PrioritySeparation` is back.
6. *Changed outside Toolboks.* Apply, then change one of the values by hand
   (`reg add ... /d <other>`), then Restore. Expected: that value is kept and reported
   ("changed outside Toolboks. Its current value was kept."), `active.json` stays so a second
   Restore can be tried, and the message says "Some changes still need attention."
7. *Interrupted run (VM).* Apply, and kill the helper process in Task Manager while it writes (or
   power the VM off). Reopen: `active.json` should exist and Restore should still work; values
   that were never written are skipped.
8. *System Protection off.* Turn System Protection off, apply without ticking *Turn on System
   Protection*: expected failure with a message naming System Protection and nothing changed. Then
   tick the box and apply again: protection is turned on and the point is created.
9. *Home edition (if you have the Home VM).* *Turn off Windows Location* and *Storage Sense*
   are skipped with "This Windows edition does not support its policy."
10. *Restore point frequency.* Apply a second time within 24 hours of the first (after Restore).
    Expected: the "Checkpoint-Computer" step reports the frequency limit clearly ("No new
    restore point was found. Check System Protection and the Windows restore-point frequency
    limit.") and nothing is changed. Record what happens.

**Automated coverage.** `addons/system-profiles` tests: the journal, restore order, "backup
cannot select unrelated registry values", the apply-lock tied to the exact reviewed selection
(`a_recovery_review_never_enables_apply`), the helper text, the protocol test. **Not covered:**
the real restore point, a real registry write, the real power plan, a crash in the middle, and the
helper window.

### A4. Disk Manager

Source: `addons/disk-manager/` (`src/lib.rs`, `src/scanner.rs`, `src/saved_scan.rs`,
`src/optimization/mod.rs`, `src/optimization/backend.rs`), `docs/DRIVE-OPTIMIZATION.md`.
Capabilities: `filesystem_read`, `local_preferences`, `user_exports`, `drive_optimize`.

**What it changes.**

- *Scan*: nothing (read only; "Scanning only reads. Your files are never changed.").
- *Saved scans*: the add-on writes scan files to `<root>\addon-data\toolboks.disk-manager\scans`
  when **Remember my last scan** is on (one scan per root, up to 384 MiB).
- *Defragment*, *TRIM*, *Optimize with Windows* (Optimization tab): runs
  `defrag.exe <drive>: /D | /L | /O /U /V` through the admin helper. *Analyze fragmentation*
  changes no files. Defragmenting a drive and TRIM of an SSD change the drive's layout and
  cannot be undone by Toolboks.

**Safety.**

| Item | Where |
|---|---|
| Scan, quick rescan, sort and filters | Harmless item. Pick a small folder first. |
| Delete saved scans / Remember off | Harmless item. It deletes only Toolboks' own scan files ("Your files stay unchanged."). |
| Analyze fragmentation | Harmless item **with UAC** (owner clicks it). It changes no files. |
| Defragment and TRIM | **VM only, on the extra virtual disk**, never on the system disk of a real PC without the owner's go. |

**Steps and expected results.**

1. *Scan.* Pick a folder with a few thousand files and press **Scan**. Expected: "Scanning in the
   background" then "Scan complete", counts and sizes match Explorer's Properties within a
   reasonable margin; **Stop scan** stops it; nothing in the folder changed (compare a directory
   listing before and after). Add an exclusion and scan again; the saved scan is not reused for a
   different exclusion set.
2. *Saved scans.* With **Remember my last scan** on, finish a scan, close Disk Manager (switch
   tool), reopen it. Expected: "Loading your last scan..." and the same result, with a "last full
   scan" age line. Look in `addon-data\toolboks.disk-manager\scans` for the file.
3. *Delete saved scans.* Click **Delete this drive's scans**: "Delete saved scans for this drive?
   Your files stay unchanged." -> **Delete scans**. Expected: the scan file is gone, "Saved scans
   deleted from this PC.", your files are unchanged. **Cancel** deletes nothing. Same for
   **Delete saved scans** (all).
4. *Remember off.* Switch **Remember my last scan** off. Expected: the confirm "Delete all saved
   scans?"; after **Delete scans** the scans are deleted and the switch stays off; **Cancel**
   leaves it as it was. With it off, a new scan writes nothing to the `scans` folder.
5. *Optimization is turned off card.* If the add-on shows "Optimization is turned off ... Review
   Disk Manager update", the `drive_optimize` permission was not approved for this install. Click
   **Review Disk Manager update**, approve, and continue. (This is the permission review the host
   shows for the capability; record what the review text says.)
6. *Analyze (VM).* Optimization tab > pick the extra virtual drive > **Analyze fragmentation**.
   Expected: UAC, then the helper window "Analyze fragmentation drive D:" with "Windows will read
   a fragmentation report. No files will be changed.", and "Toolboks cannot undo drive
   optimization. Windows maintenance may continue after Toolboks closes." **Cancel**: no drive
   work happens (check that no `defrag.exe` runs). **Confirm**: the three fragmentation numbers
   fill in with the analysis time; an unreadable value shows "Not analyzed", never 0%.
7. *Defrag or TRIM (VM, extra disk).* Fill the extra disk with some files and delete them. On a
   virtual HDD-type disk the button reads **Defragment**; on an SSD that reports TRIM it reads
   **TRIM**; if Windows does not report the media type it reads **Optimize with Windows** (a VM
   disk often falls in this last case, so write down which label you got). Press it.
   - Expected: UAC, helper window "Defragment drive D:" (or "TRIM", "Optimize with Windows") with
     "Windows will rearrange fragmented files. The drive may be busy while this runs." (or "Windows
     will tell the SSD which unused blocks it can reclaim." or "Windows will choose the appropriate
     optimization for this drive."). Confirm. The tab stays responsive, the duplicate buttons are
     disabled, and "Activity and Windows report" and "Last Windows report" show the Windows output.
     At the end: "<action> completed for D:." and the history ("Last defrag" / "Last TRIM") shows
     the new local time (Windows event log, `Microsoft-Windows-Defrag`). A failed fresh analysis
     afterward must not turn the success into a failure.
   - Also: below 15% free space on an HDD the page warns that Windows may only partially
     defragment. A drive Windows marked dirty offers no optimization.
   - Undo: none. Revert the VM.
8. *Wrong or changed drive.* Start an action, then in Disk Management change the drive letter or
   detach the disk before pressing Confirm. Expected: the helper refuses ("The drive identity
   changed ..."/validation error), no `defrag.exe` runs.
9. *Closing during a run.* Close the Toolboks window while Windows is defragmenting. Expected: the
   page text says Windows may continue after Toolboks closes; check Task Manager and that nothing
   is corrupt.

**Automated coverage.** `addons/disk-manager`: scanner, saved-scan format and deletion races
(`remembering_off_deletes_on_disk_and_exclusions_disable_quick_rescan`), request validation
(letters, device id, media rules), history classification, report ranges, the protocol test.
Ignored: `real_metadata_read_only` (reads inventory and the event log), `reads_this_pc`,
`bench_*`, `large_saved_scan_loads_while_frames_continue`. **Not covered:** real defrag and TRIM,
UAC, the helper window, antivirus or a locked drive during a scan. `docs/DRIVE-OPTIMIZATION.md`
still says optimization is disabled "pending #132"; the add-on now has the helper, so check that
document against what you see and note any difference.

### A5. Macro Manager

Source: `addons/macro-manager/` (`src/engine.rs`, `src/lib.rs`), `docs/MACRO-MANAGER.md`.
Capabilities: `input_automation`, `local_preferences`.

**What it changes.** It installs low-level keyboard and mouse hooks (`SetWindowsHookExW`
13 and 14) while recording, and plays back real input with `SendInput` into the active desktop.
It writes macros to its own add-on data. A macro that sends keys can type into anything in front,
including a password box or a game.

**Safety.**

| Item | Where |
|---|---|
| Record and play a mouse path over an empty Notepad or Paint window, and keys into Notepad | Harmless item (use a blank text file, no unsaved work, no browser). |
| Anything played over a browser, a game, a terminal, or with a repeat-until-stopped macro | VM only. |
| Stop keys | Harmless item, test every time. |

**Steps and expected results.**

1. Install Macro Manager and review its permissions ("Record and replay keyboard and mouse
   actions when you start a macro"). Open it. Expected: the library starts empty.
2. **New**, name it `tb-test`, confirm with Enter. Open Notepad, put it in front, click **Record
   keystrokes**, type `hello`, press **Stop recording**. Expected: steps for each key down and up
   with recorded delays; nothing is recorded just by opening Toolboks.
3. *Playback.* Empty the Notepad text, select the macro and press **Play macro**. Expected:
   `hello` is typed into the window that is in front; the button changes to **Stop playback**
   while it runs.
4. *Mouse.* In Paint, **Record Mouse**, drag a short line, press **Stop recording**. Play it back
   over a blank canvas. Expected: movement, buttons and scrolling replay; clicks on Toolboks'
   own Stop button are not in the recording; playback input is not re-recorded.
5. *Stop keys.* Set the macro to repeat until stopped and start it in Notepad. Press **Esc**:
   playback stops and held keys/buttons are released. Repeat with the macro's own activation key
   (assign one: click the shortcut button, press the key; **Esc** cancels assignment, ctrl/shift/
   alt alone keep it waiting): pressing it again stops that macro. Repeat with the playback
   button, and by waiting for a duration to expire. Expected: Esc is "reserved for Stop" and
   cannot be assigned ("Every macro needs a different activation key; Esc is reserved for
   Stop."). Stop recording with the button, the recording shortcut, and **Esc** (mouse-only
   recording uses Esc too).
6. *Held keys and drags.* Record a drag with a key held (Shift). Stop mid-playback. Expected: no
   key or button stays stuck (check Shift by typing a lowercase letter afterwards).
7. *Other tool in front / hidden.* With the activation key assigned, switch to another add-on's
   page and press it. Expected: the macro runs (shortcuts stay usable in another tool), and
   playback continues while Macro Manager's page is not drawn.
8. *Higher-privilege app.* Start a macro while an elevated window (admin Command Prompt) is in
   front. Expected: Windows rejects the input; Macro Manager must not crash or hang, and Esc
   still stops it.
9. *Keypad.* With Num Lock on and off assign `Keypad 4` and top-row `4` as two shortcuts: they
   stay separate and with Num Lock off the keypad navigation key is reported.

**Automated coverage.** `addons/macro-manager`: the model and timing, grouping of recorded
movement, dropping the Stop click over Toolboks, shortcut capture rules, and the protocol test.
Three tests that really use the Windows hooks and `SendInput` are ignored because they need an
interactive desktop: `windows_escape_stops_mouse_recording_without_keyboard_hook`,
`windows_shortcut_assignment_captures_keypad_without_blocking`,
`windows_activation_key_delivers_target_macro` (run them on the VM with `-- --ignored`).
`docs/testing-findings.md` Tests 4 to 7 record earlier smoke tests of Macro Manager; read them
before repeating. **Not covered:** playback into real apps, elevated windows, games, stuck keys.

### A6. Resource Monitor

Source: `addons/resource-monitor/src/processes.rs` and `crates/protected-processes`. Capability:
`host_metrics`. (Its actions run as your own user; no helper.)

**What it changes.** The selected process only:

- *Lower priority*: sets the priority class to Below Normal (`SetPriorityClass`) and remembers
  the old one. It only lowers (an app that is already Below Normal or Idle is refused: "Priority is
  already low"). **Restore priority** puts it back.
- *End task*: `TerminateProcess`, after a second click on the same button. **Unsaved work is lost.**

Never touched: the protected list (`System`, `System Idle Process`, `Registry`, `Memory
Compression`, `Secure System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `winlogon.exe`,
`services.exe`, `lsass.exe`, `svchost.exe`, `fontdrvhost.exe`, `dwm.exe`, `toolboks.exe`, pid 0
and 4, this app itself) and the game, launcher and anti-cheat names from
`crates/protected-processes` including anything started by a launcher.

**Safety.**

| Item | Where |
|---|---|
| Lower priority on a throw-away process you started (Notepad), then Restore priority | Harmless item. |
| End task on a throw-away Notepad with no unsaved work | Harmless item. |
| End task or priority on anything else, and the protected-list checks with fake `steam.exe` and similar | VM only. |

**Steps and expected results.**

1. Start two Notepads. In the Processes view find them (filter by name or pid), select one.
2. **Lower priority** (hover: "Lets other apps use the processor first. Undo it here any time.").
   Expected: result line "Lowered priority of notepad.exe (pid N)"; Task Manager > Details shows
   Below normal. The button becomes **Restore priority**; press it: the original class is back.
   Press Lower twice: the second says "Priority is already low".
3. *Protected rows.* Select `csrss.exe`, `lsass.exe`, `dwm.exe`, `svchost.exe`, and the fake
   `steam.exe`/`EasyAntiCheat.exe` copies from A1 step 6. Expected: the row is dimmed and the hover
   shows "Windows needs this to run" or "Protected: ... Toolboks leaves it alone."; **End task**
   and **Lower priority** are disabled or report "N of M are protected or could not be verified."
   No handle with write access is opened for them.
4. **End task.** Select one Notepad with some typed text and press **End task**. Expected: no
   immediate action; the second step appears: "End notepad.exe? Unsaved work in this app will be
   lost." with **End task** (red) and **Cancel**. **Cancel** ends nothing. **End task** ends only
   that process. With a grouped row of several processes: "End N processes of <app>? Unsaved work
   in them will be lost." and only the verified, unprotected ones end.
5. *Reused pid.* Select a process, let it exit and start another quickly (it may reuse the pid);
   press End. Expected: the identity check refuses it; the new process is untouched.
6. *Mixed selection.* Select Notepad plus a protected process and press End task. Expected: only
   Notepad is in the plan; the protected one is listed as skipped.

**Automated coverage.** `addons/resource-monitor`: the pure protected list and grouping
(`protected_names_and_pids`, `grouped_row_is_protected_only_when_all_are`, `plan_end_splits_protected`),
`game_and_anti_cheat_processes_are_refused_before_opening`, `live_name_gate_rejects_changed_or_protected_names`,
the confirm wording, and `process_actions_verify_identity_on_the_handle_they_modify` (real child
processes). **Not covered:** ending or lowering a real third-party app, grouped rows of real
multi-process apps, and anti-cheat interaction with a real game.

---

## Part B: the host (the shell)

### B1. Tray mode

Source: `src/tray.rs`, `src/app.rs` (`tray_tick`), `src/app/shell.rs` (Settings),
`src/instance.rs`.

**What it changes.** Closing the window hides it instead of quitting, adds a notification-area icon
with a live "memory in use" percentage, and keeps background add-on work running. Setting:
`keep_in_tray` in `config.json`.

**Safety.** Harmless on the owner's PC **with a preview build** (its own icon "Toolboks Preview",
its own data folder). Never with the owner's normal copy.

**Steps and expected results.**

1. Settings > "When I close the window" > switch **Keep Toolboks running in the tray** on.
   Expected: an icon appears with a number (memory percent; 100 is drawn as 99), tooltip text is
   plain, colours follow health.
2. Click the window's close button. Expected: the window hides (the app is not in the taskbar), the
   icon stays, the process keeps running (Task Manager), and add-on helpers keep running.
3. Left-click the icon: the window comes back; again: it hides (toggle). Right-click: menu "Show
   toolboks" and "Quit toolboks" (preview: "Toolboks Preview"). **Show** restores; **Quit**
   ends the process and the icon disappears (no stale icon).
4. Start the same Toolboks again while it is hidden. Expected: it brings the hidden window back
   and exits itself (single instance).
5. Turn the switch off while hidden or visible: the icon goes away and a hidden window comes back.
6. *Install in progress.* Start an install or an update and try Quit from the tray or close the
   window. Expected: the window shows and says "Wait for the update to finish before closing
   Toolboks."
7. *No icon.* If Windows refuses the icon, Toolboks must fall back to a normal window (no way to
   get a hidden window back otherwise). Hard to force; note "not run" if you cannot.
8. *Background work while hidden.* Start an add-on that works in the background (Memory Cleaner
   with a run, Event Watch, Macro Manager activation key), hide to the tray, and confirm the work
   continues (see A1 step 9, A5 step 7) while a hidden Memory Cleaner does not start a new
   automatic run (A1 step 7).
9. Reboot or sign out/in with the setting on. Expected: Toolboks does not start by itself (there
   is no autostart in the code; record whether that matches what the owner expects).

Undo: switch the setting off, **Quit** from the tray.

**Automated coverage.** `src/tray.rs`: icon drawing, colour thresholds, tooltip. The single
instance decision and the relaunch flag (`src/instance.rs`). **Not covered:** the real icon,
click and menu handling, hide and show, Quit, background work while hidden.

### B2. Self-update handoff (updater swaps the exe)

Source: `src/update.rs` (`check`, `install`, `replace_exe`, `cleanup_old_exe`), `src/app.rs`
(`start_install`, `poll_update`, `restart`), `src/instance.rs`.

**What it changes.** Downloads `toolboks.exe` and `toolboks.exe.sha256` from the latest GitHub
release of `Gusseti/toolboks` into memory, checks the SHA-256, writes `toolboks.exe.new` next to
the running exe, swaps it in with `ReplaceFileW` (the old exe becomes `toolboks.exe.old`), starts
the new exe with `--relaunch`, and closes the old window. The next start deletes `.old` (and any
`.new`) after the new window has been drawn for about two seconds.

**Safety.** **VM only.** It replaces the program file. It needs a **real, older release** exe and
the current latest release on GitHub (there is no way to point the check at another place; the
preview build disables updates).

**Steps and expected results.**

1. In the VM, create `C:\tbtest\` and put an **older released** `toolboks.exe` there (download an
   older release asset). Run it with `TOOLBOKS_DATA_DIR` set to a fresh folder (or let it use the
   VM's own `%LOCALAPPDATA%`).
2. *Check.* Settings > update row > **Check for updates**. Expected: "Checking for updates..."
   then "Version X is available." with **Install and restart**; at launch the prompt reads "You
   are using version Y. Update now? Toolboks restarts to finish." with **Update now** and the
   download size. If already latest: "You have the latest version."
3. *Install.* Click **Install and restart**. Expected: "Installing the update..." and closing the
   window is blocked ("Wait for the update to finish before closing Toolboks."). Then, in this
   order: `toolboks.exe.new` appears; `toolboks.exe` is replaced and `toolboks.exe.old` appears;
   a new Toolboks window opens (not a second copy next to the old one: the old one waits and
   exits, the new one waits for it, `--relaunch`), showing the new version; about two seconds
   after it rendered `toolboks.exe.old` is deleted. Verify the exe's version in Properties >
   Details and that the file hash equals the release's `.sha256`.
4. *Nothing left behind.* No `.new`, no `.old`, one Toolboks process.
5. *Add-ons and data survive.* Installed add-ons and their settings are unchanged after the update.
6. *Tray + update.* With **Keep Toolboks running in the tray** on, update. Expected: the old
   window really closes (a tray-mode close would otherwise only hide it; `instance::is_relaunching`
   prevents it) and the new one starts visible.
7. *Run as administrator.* Start the old exe elevated and update. Expected: the new copy starts
   with the same rights ("It inherits this process's rights, admin included").
8. *Failures.* (a) Disconnect the network before **Install and restart**: a plain error line in
   amber and the old version keeps working, no `.new` left. (b) Make the folder read-only or lock
   the exe with another handle: the error is shown and the exe is unchanged (if Windows reports
   a failure after moving it, the previous exe is restored from `.old`; look at
   `toolboks.exe.old` if the message says it is still available there). (c) Kill the process
   during the swap: the next start must still run, and clean up a stray `.new`/`.old`. (d) Power
   off the VM during the download: the old exe is untouched.
9. *Antivirus.* If the VM runs Defender, note whether the swap or the restart is flagged.

Undo: revert the VM snapshot.

**Automated coverage.** Version parsing and ordering, release JSON parsing, checksum parsing and
verification, trusted-URL rules, notes formatting (`src/update.rs` tests); one ignored test
downloads and verifies the real published release (`the_published_release_downloads_and_verifies`).
**Not covered:** `replace_exe` on a running exe, the restart, the single-instance wait, the `.old`
cleanup, failure and rollback of a half-written file.

### B3. Add-on install, update, rollback, remove

Source: `src/installer.rs`, `src/app/marketplace.rs`, `src/marketplace.rs`, `src/supervisor/`,
`src/catalog.rs`.

**What it changes.** Downloads an add-on exe (size and SHA-256 must match the catalog entry; the
URL must be a trusted GitHub URL, also after redirects), places it under
`<root>\addons\<id>\<version>\`, writes `active.json`, and starts it as its own process. A new
version gets **one** start: if it never reports started, the next launch rolls back to the previous
version (`installer::launching`, `rollback`). Remove deletes `addons\<id>` and, only if chosen,
`addon-data\<id>`.

**Safety.** Harmless on the owner's PC **with a preview build and its own data folder**; add-ons
are downloaded from the real public list or from a test catalog you built. No system settings
change.

**Steps and expected results.**

1. *Install.* Add-ons page > **Install** on one add-on (or use first-run **Pick your tools** >
   **Install add-on** / **Install these add-ons**). Expected: "Review installation" lists exactly
   the permissions ("Back up and change the Windows settings and power plan you select", "Record
   and replay keyboard and mouse actions...", administrator and drive optimization lines for
   those that ask), nothing downloads until you confirm; a progress state; then it appears in the
   sidebar with a "New" tag until opened.
2. *Corrupt or wrong download.* With a test catalog whose `sha256` or `size` is wrong (edit
   `index.json` after running `mkcat.py`): expected a clear failure, nothing is placed, the old
   version (if any) still runs. A `download_url` outside GitHub is refused.
3. *Interrupted download.* Cancel an install half way, retry: it resumes (or restarts if the list
   changed) and ends with the right hash.
4. *Update.* Install an older add-on version from a test catalog, then switch to a catalog with a
   newer version of the same add-on (or use the real list). Expected: Home shows "waiting"
   updates with an **Update** button; "Update" in the browser; the new version replaces the old
   one, its data and preferences are kept, "New" shows in the sidebar.
5. *Rollback after a failed update.* Build a bad newer version: take any exe that exits at
   once or never completes the handshake, name it as the add-on, and put it in a test catalog
   with `mkcat.py` (same id, higher version, real size and hash of that exe). Install the good
   old version first, run it once so it is confirmed, then update to the bad one and open it.
   Expected: the first start fails (the sidebar shows the failed state), the app does not hang; at
   the next start of that add-on Toolboks rolls back: the previous version is active again and
   `active.json` shows it, the bad version folder is deleted, and the add-on opens. Check the
   preferences and data are the same as before the update. Also close Toolboks during the first
   start of a new version: the next launch rolls back the same way.
6. *Remove, keeping data.* Add-on details > **Remove add-on** > "Remove <name>?" ("Its saved data
   stays on this PC by default, ready if you install it again.") > leave "Also delete its data"
   unticked > **Remove add-on**. Expected: gone from the sidebar and from `addons\<id>`;
   `addon-data\<id>` still there; installing again brings the old settings back. If the add-on is
   running the button reads **Close add-on before removing** first.
7. *Remove, deleting data.* Tick **Also delete its data** (text: "Saved settings and data will be
   deleted. This cannot be undone.") and remove. Expected: both folders gone.
8. *Locked file.* Start an add-on, then try to remove it by deleting its folder yourself while it
   runs: not a Toolboks path, skip. Instead, if removal fails with an error, the sidebar entry
   must stay consistent (not half removed); record the message.
9. *Install from file.* Use **Install from file** with a `.tbaddon` that needs a removed native
   provider: expected the plain refusal "This package needs a tool that is no longer built into
   Toolboks. Install it from the add-on browser."

Undo: remove the add-on, delete the test data folder.

**Automated coverage.** `src/installer.rs` tests against a local test server: good install and
offline start, corrupted download, untrusted URLs, resume and restart of partial downloads,
cancel, `update_rolls_back_when_the_new_version_never_starts`, `removal_keeps_or_deletes_data`,
daily update check, native exe hash kept for rollback. Review screenshots exist in
`docs/reviews/115`. **Not covered:** real GitHub downloads at scale, antivirus, a real crash of a
real add-on exe, and removal while files are held open.

### B4. "Restore your tools" (migration from an old data folder)

Source: `src/addons.rs` (`Registry::load`, `plan_restore`, `FORMER_BUILTIN_IDS`,
`obsolete_provider_package`), `src/app/marketplace.rs` (pick screen), `src/app/shell.rs` (Home
banner), `src/app/restore_check.rs`.

**What it changes.** On the first start after the update from the version that had the tools built
in, Toolboks works out which tools the user had (the first four built-in ones unless disabled, the
rest only if installed), deletes the obsolete host-linked `.tbaddon` files of those tools, writes
`restore_pending` and `platform_migrated: true` to `config.json`, and offers to install them.

**Safety.** Harmless **only on a copy**: use a copy of an old data folder under a fresh
`TOOLBOKS_DATA_DIR`, with a preview build. Never run it against the owner's real folder (it
rewrites `config.json` and deletes the obsolete `.tbaddon` files in the folder it is given).

**Steps and expected results.**

1. Get an old data folder: from a machine or backup that ran a version with the tools built in
   (a `config.json` without `"platform_migrated": true`, an `addons` folder with `.tbaddon` files
   of the old host-linked tools). Copy it to `<TOOLBOKS_DATA_DIR>\Toolboks Preview` (keep a second
   untouched copy to repeat the test).
2. Start the preview build with that `TOOLBOKS_DATA_DIR` (with the real public list, or a test
   catalog). Expected: the **Restore your tools** screen ("Toolboks now downloads each tool on
   its own. The tools you had are ticked below; install them to get them back with your settings.",
   "Your earlier tools are ticked."). Only the tools the user really had are ticked; disabled
   built-in ones are not.
3. Close without installing, reopen: Home shows "Some of the tools you had before are not
   installed yet." with **Restore your tools** (the list persists in `config.json`
   `restore_pending` until installed or skipped).
4. Install the ticked tools. Expected: preferences and data saved earlier are still there under
   the same ids; the offer disappears once all are installed.
5. *Unreadable config.* Corrupt `config.json` in a second copy. Expected: an error line
   ("Configuration could not be read. Defaults are in use"), the file is **not** overwritten,
   nothing is decided from defaults.
6. *Fresh install.* An empty folder: no offer, "Pick your tools" instead ("No tools selected by
   default.").
7. *Offline.* Start with no network: the offer list is kept and no data lost.
8. The scripted version: `toolboks.exe --restore-check <out folder> <comma separated ids>`
   (preview build only, with `TOOLBOKS_DATA_DIR`) installs them and takes screenshots. It opens
   a window: only when the owner is not gaming.

**Automated coverage.** `src/addons.rs` tests: first start after the update, offered ids, old
files removed, preferences preserved, unreadable config left alone
(`host_linked_add_ons_installed_before_are_offered_and_their_old_files_removed` and the legacy
tests). **Not covered:** a real old data folder from a real old release, and the screens.

### B5. The admin helper (Confirm/Cancel window and UAC)

Source: `src/supervisor/admin.rs`, `src/supervisor/process.rs` (`spawn_elevated`, `runas`),
`src/supervisor/mod.rs`, `sdk/toolboks-addon-sdk/src/admin.rs` and `admin/dialog.rs`,
`docs/ADDON-THREAT-MODEL.md`, `docs/reviews/132/validation.md`.

**What it changes.** Nothing by itself. For a change that needs administrator rights, the host
starts a **separate copy of the add-on exe, elevated through UAC** (`ShellExecuteExW` with
`runas`). That helper validates the request, shows its own native window "Confirm administrator
change" with the description built **inside the helper** and two buttons, **Confirm** and
**Cancel**, and only after Confirm performs the change. The drawing process stays unelevated.
Host clicks or keys cannot approve it. Focus starts on Cancel and Enter cannot accept a freshly
opened dialog.

**Safety.** The test itself is harmless if you use a harmless operation and press **Cancel**. Use
the Memory Cleaner purge (A1 step 8) or the Disk Manager analysis (A4 step 6) as the operation,
and a Debloater item you have decided to undo. The owner clicks every UAC prompt. Real **Confirm** on
anything that changes the system is VM only.

**Steps and expected results.**

1. *UAC accepted, Cancel.* Start an operation. Expected: Windows UAC for the add-on exe (publisher
   shown as in the exe), then the native window with title "Confirm administrator change", the
   description, **Confirm** and **Cancel**, in front of other windows, readable, resizable text
   not cut off (long Debloater lists are cut at about 6000 characters with "...": check one
   long list). Toolboks shows its waiting state and disables repeats. **Cancel**: the add-on shows
   "Cancelled. Nothing was changed." (or the add-on's equivalent); the elevated process exits (no
   leftover process in Task Manager).
2. *UAC declined.* Click **No** on UAC. Expected: the same cancelled result, nothing happened, no
   error dialog, Toolboks still responsive.
3. *Confirm.* Repeat and press **Confirm**. Expected: the change happens exactly once and the
   result is shown in the add-on.
4. *Keyboard and focus.* When the window opens, press Enter once immediately: it must **not**
   confirm (focus on Cancel). Tab to Confirm and press Enter: it confirms. Esc or the close
   button: cancels.
5. *Click injection.* While the window is open, drive the mouse over the Confirm button from
   Macro Manager playback, or a script using `SendInput`. Expected: it cannot confirm (the helper
   ignores forged host input; real hardware input still works). Windows' own UIPI also blocks
   unelevated input to an elevated window, so a failed injection here is the expected pass.
6. *Host closes.* While the helper window is open, close Toolboks from the tray or Task Manager
   (VM). Expected: the helper window closes and nothing is applied ("host disconnect cancels").
7. *Helper without a saved catalog.* The host refuses to start a helper when it has no saved
   catalog entry for the add-on, before any UAC prompt, and the drawing process keeps running (the
   supervisor test asserts this with a fake). A person can only reach it with an add-on that was
   not installed from the catalog; if you cannot build that case, write "not run".
8. *Capability.* An add-on without the matching capability cannot ask. Remove the permission
   (a hand-edited test add-on) and try: "This administrator action is not allowed."
9. *Already elevated host.* Start the host as administrator (VM) and run an operation. Record
   whether a second UAC appears and the dialog still shows.
10. *Two requests.* Click an operation twice quickly: only one helper starts (buttons are disabled
    while waiting).

**Automated coverage.** `sdk/toolboks-addon-sdk/src/admin/tests.rs` (cancel performs no change;
one confirmation runs once; a host click, key, or forged result cannot confirm; disconnect cancels;
forwarded input cannot be the initial request; oversized or invalid requests never prepare;
missing rights or capability never opens the dialog), and
`src/supervisor/tests.rs::refusing_a_helper_without_a_catalog_keeps_the_same_unelevated_drawing_process_alive`.
Ignored, needs a desktop: `dialog_window_is_created_by_this_process_and_cancellation_closes_it`
(`sdk/toolboks-addon-sdk/src/admin/dialog.rs`). The example `cargo run --example admin_dialog`
shows the same dialog without any system change. **Not covered:** real UAC, the native window on a
real screen, the owner clicking, the UIPI behaviour.

### B6. The add-on list refresh

Source: `src/app/marketplace.rs` (`Refresh list`, `poll_marketplace`), `src/marketplace.rs`,
`src/catalog.rs`.

**What it changes.** Nothing on the system. It fetches the catalog (`catalog/index.json` from the
add-ons repo `main`), first through GitHub's API (the plain file address can be cached by GitHub
for up to five minutes), and shows new add-ons and updates. While the app is open and visible it
refreshes by itself every few minutes, and not while hidden in the tray.

**Safety.** Harmless item. Preview build, own data folder.

**Steps and expected results.**

1. Open Add-ons. Expected: the list loads without pressing anything.
2. Press **Refresh list**. Expected: it reloads, errors (offline, rate limit) are shown plainly and
   the previous list stays.
3. *A new release shows up.* After an add-on release reached `main` (about a minute later is
   expected, not five), press **Refresh list**: the new version appears; Home shows the update
   with an **Update** button for an installed add-on; the sidebar shows the "Update" tag
   (`Tag` in `src/app/shell.rs`). Compare with the live file through the GitHub API (not the raw
   file URL, which may be cached).
4. *Offline.* Disable the network, press **Refresh list**: a plain message, the installed add-ons
   keep working, no crash.
5. *Hidden.* With the tray setting on and the window hidden, no refresh should run (watch the
   network).
6. *Test catalog.* With `TOOLBOKS_TEST_CATALOG` set, the list comes from that folder only
   (preview and tests).

**Automated coverage.** `src/marketplace.rs` and `src/catalog.rs` tests (index parsing, two-field
index, hidden/unsupported versions, entry rules) and the real-window `--addon-check`. **Not
covered:** real GitHub rate limits, the caching difference, timing of the periodic refresh.

---

## Results table

Fill one row per feature and run. Add a row for every new run. One failure, one issue: put the
issue link in the last column.

| Feature | Section | Where (VM / own PC, item) | Date | Result (pass / fail / not run) | Bug link |
|---|---|---|---|---|---|
| Memory Cleaner: Trim idle apps, undo | A1 steps 1 to 3, 5 | | | | |
| Memory Cleaner: protected-process skip list | A1 step 6 | | | | |
| Memory Cleaner: Clean automatically | A1 step 7 | | | | |
| Memory Cleaner: admin cache purge | A1 step 8 | | | | |
| Debloater: Privacy and ads apply and undo (account) | A2 step 1 | | | | |
| Debloater: Privacy and ads apply and undo (machine, helper) | A2 step 2 | | | | |
| Debloater: restore point option | A2 step 3 | | | | |
| Debloater: Apps removal | A2 step 4 | | | | |
| Debloater: Startup | A2 step 5 | | | | |
| Debloater: Tasks | A2 step 6 | | | | |
| Debloater: Services | A2 step 7 | | | | |
| Debloater: helper Confirm text | A2 steps 2, 6, 7 | | | | |
| System Profiles: apply | A3 steps 1 to 3 | | | | |
| System Profiles: restore and recovery folder | A3 steps 4 to 7 | | | | |
| Disk Manager: scan | A4 step 1 | | | | |
| Disk Manager: saved scans, delete, remember off | A4 steps 2 to 4 | | | | |
| Disk Manager: analyze, defrag, TRIM | A4 steps 5 to 9 | | | | |
| Macro Manager: record and play | A5 steps 1 to 4 | | | | |
| Macro Manager: stop keys | A5 steps 5, 6 | | | | |
| Resource Monitor: priority | A6 steps 1, 2 | | | | |
| Resource Monitor: End task and protected list | A6 steps 3 to 6 | | | | |
| Host: tray mode | B1 | | | | |
| Host: self-update handoff | B2 | | | | |
| Host: add-on install and update | B3 steps 1 to 4 | | | | |
| Host: rollback after a failed update | B3 step 5 | | | | |
| Host: remove (keep and delete data) | B3 steps 6, 7 | | | | |
| Host: Restore your tools | B4 | | | | |
| Host: admin helper (Confirm/Cancel, UAC) | B5 | | | | |
| Host: add-on list refresh | B6 | | | | |

## When you are done

- Revert the VM to `clean-before-toolboks` (or `toolboks-installed`).
- On any real PC: confirm every harmless item was undone; delete the test data folder; do not
  leave `TOOLBOKS_DATA_DIR` or `TOOLBOKS_TEST_CATALOG` set in a profile or a shortcut.
- Issue #49 closes only when every feature above has a recorded **pass** or a linked bug, with
  the date and where it was run.
