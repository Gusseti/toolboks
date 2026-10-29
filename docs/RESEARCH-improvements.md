# Reposition toolboks around visibility, trust and honest cleaning

**Bottom line.** The research says toolboks' biggest opportunity is not more cleaning power but more *visibility and trust*. Memory cleaning is the weakest claim in its category: even the developer of CleanMem states it "WILL NOT make your system faster" ([MakeUseOf](https://www.makeuseof.com/tag/cleanmem-a-memory-cleaner-that-works)), trimming only moves pages to the standby list and costs page faults later ([engineered.at](https://engineered.at/articles/32-mib-working-sets-on-a-64-gib-machine), secondary source), and Microsoft material says low free memory is normal because free pages are wasted ([MS Learn archive](https://learn.microsoft.com/en-us/archive/blog/tims/pdc10-mysteries-of-windows-memory-management-revealed-part-two), snippet only). So the Memory Cleaner should become "analyzer first, cleaner second": a before/after report, leak detector, commit/pagefile view, and a game-mode standby rule that is explicitly labelled unproven. The Resource Monitor's best gaps are a Processes page with a safe End task, a Connections page and alerts, all doable without admin or drivers. The Debloater needs trust polish (preview, in-session change log, drift check), not new tweaks. App-wide, the highest-value items are code signing (SignPath Foundation), a crash-report dialog, verified self-update and winget. The "nothing written to disk" rule can hold as the default, but four features cannot honour it fully (settings, start with Windows, self-update swap, toasts); each is given an opt-in design below. Much of the research is thin: many API names, effort ratings and rankings are the researchers' own judgment (flagged throughout).

**Evidence legend used in every table:**
`C` = cited in the notes with a URL. `J` = researcher/author judgment, no source. `U` = unverified or contradicted/weak in the notes; treat as a hypothesis to test. Effort S/M/L (S under ~2 days, M ~1-2 weeks, L longer; my own scale). Risk = risk of harm or regression to the user.

---

## 1. Executive summary

### 1.1 Top 10 improvements across the whole app

Ranking is by value, then low effort, then low risk. Ranking itself is `J` (the notes say no demand data was gathered).

| # | Improvement | Area | Value | Effort | Risk | Admin | Fits "no disk"? | Evidence |
|---|---|---|---|---|---|---|---|---|
| 1 | **Before/after "what changed" report** after every Optimize/auto-clean, plus honest copy ("Windows already reclaims memory by itself") | Memory | High | S | None | Same as today | Yes (Vec in RAM) | J (placebo finding: C) |
| 2 | **Crash dialog with "Copy report"** (panic hook, no log file, no auto-send) | App | High | S | None | No | Yes | J |
| 3 | **Processes page**: group/tree, sort, filter, Pause, End task with confirm, Copy row | Monitor | Very high | M | Low | No (own-user processes) | Yes | C (patterns) / J (ranking) |
| 4 | **Signed exe** via SignPath Foundation + public "Code signing policy" and privacy statement | App | Very high | M | Low | No | Yes (signing is CI) | C |
| 5 | **Verified self-update**: SHA-256 (optional minisign), "What's new", system proxy | App | High | M | Low-Med | No | **Partly** (transient exe + rename-aside) | C (crates) / J |
| 6 | **Tray + single instance + minimize-to-tray**, with opt-in slow background sampling | App/Monitor/Memory | High | M | Low | No | Yes | J |
| 7 | **Memory insight pack**: leak detector (top growers), commit charge/pagefile/compressed card | Memory | High | M | None | No | Yes | J |
| 8 | **Debloater trust polish**: dry-run preview, session change log, "drift check", preset cards with plain risk labels | Debloater | High | M | Low | Already as today | Yes (session log) | C (O&O guidance) / J |
| 9 | **Connections page** (owning process, state, remote), End process, on-demand reverse DNS only | Monitor | High | M | Low | No to view | Yes | C (API) / J |
| 10 | **Opt-in persistence** ("Remember my settings" in HKCU + "Remove all traces" + clipboard export/import) | App | High | S | Low | No | **Partly** (registry, opt-in) | J |

Also strong but just outside the top 10: winget (then Scoop) listing (value high, effort S-M, C/J), alerts/runaway-process banner (high, S, J), accessibility pass (med-high, M, C/J), Norwegian (nb) localization (med-high, M, J), a "Why is my PC slow?" button (high, M, J), Disk Cleaner as the first new tool (high, M, J).

### 1.2 The three things to tell the owner in one minute

| Finding | Consequence for the roadmap |
|---|---|
| Memory trimming is mostly placebo on modern PCs (C); real problems show up as sustained pressure, hard faults and commit exhaustion (C, forum consensus) | Stop implying speedups. Add visibility features. Default auto-clean to **off**. |
| Trusted tools are open source or Microsoft-owned; distrusted ones bundle, upsell, run registry cleaners or had a signed supply-chain compromise (CCleaner 2017) ([CyberScoop](https://cyberscoop.com/ccleaner-malware-avast-cisco-talos/)) | Trust work (signing, verified update, no telemetry statement, preview + undo) is a feature, not overhead. Auto-update is exactly what the CCleaner attack abused, so verification matters. |
| The research hit real limits on tray, toast, hotkey and settings-store behaviour for an unpackaged exe (the notes say so) | Prototype these on a clean VM before committing to them (Roadmap step 2). |

---

## 2. Memory Cleaner

### 2.1 What the research says about "does cleaning help?"

| Claim | Source | Evidence |
|---|---|---|
| Working-set trim moves memory from the process to the standby list; it is not "freed" memory | [engineered.at](https://engineered.at/articles/32-mib-working-sets-on-a-64-gib-machine) (AI-summarized page; the ~250x slowdown figure is for an extreme 32 MiB cap, do not generalize) | C, weak |
| Windows trims working sets itself under memory pressure | [MS Learn (xperf)](https://learn.microsoft.com/fr-fr/previous-versions/windows/desktop/xperf/identifying-drivers-that-allocate-contiguous-memory) via search summary | C, weak |
| Unused RAM is wasted RAM; optimizers are "placebo" | [Wilders](https://www.wilderssecurity.com/posts/1326364), [Chemtable](https://www.chemtable.com/blog/en/?p=5864), [Ask Bob Rankin](https://askbobrankin.com/do_i_need_a_memory_optimizer.html) | C, forum/opinion |
| A few users report benefit on old/low-RAM machines or repeated low-memory warnings | [Wilders](https://www.wilderssecurity.com/posts/1326364), [BleepingComputer](https://www.bleepingcomputer.com/forums/t/650232/are-memory-cleaners-really-snake-oil/) | C, anecdote |
| Standby purge may reduce game stutter (ISLC author's claim, "you take all the risk") | [Wagnardsoft forum](https://www.wagnardsoft.com/forums/viewtopic.php?p=8066) via search snippet | U: no independent benchmark found |
| Razer Cortex-style boosting has little measurable effect above ~4-8 GB RAM | [Razer Insider](https://insider.razer.com/razer-cortex-39/why-cortex-beta-43015), forum posts 2015-2018 | C, dated and weak |
| Raymond Chen / Russinovich statements on trimming | Not found | **Not verified** |

**Position taken here (J):** toolboks already matches the free competitors on controls (threshold, standby rule, cooldown, protect list) and exceeds them on process visibility. The category's weakest part is the claim, so differentiate on explanation. Keep the cleaner, but frame it as a tool for specific situations: a leaking app, a one-off low-memory moment, a game starting.

### 2.2 Recommended Memory Cleaner items

Common to all items below: state lives in RAM ring buffers and `Vec`s, lost on exit, and the UI should say so. All API names are researcher knowledge and **not verified against Microsoft Learn** in the research (verify before coding).

#### M1. Before/after report (do first)
| Field | Detail |
|---|---|
| What | After every Optimize or auto-clean, snapshot system memory and per-process working sets, re-check at 3 s and 30 s, show a card. Keep the last ~20 in memory. |
| Why / comparable | RAMMap's value is analysis with saved snapshots ([MS Learn](https://learn.microsoft.com/en-us/sysinternals/downloads/rammap)); no free cleaner shows whether the clean was worth it. This turns the placebo problem into a feature. |
| Example screen | Optimize is pressed. A card appears: "Moved 2.1 GB out of app memory. 30 seconds later Windows had 0.4 GB more available, and 640 MB was loaded back in. Apps that grew back: chrome.exe, Teams.exe." Under it, one sentence: "Windows already reclaims unused memory on its own. Cleaning rarely makes a PC faster." Buttons: [Copy report] [Turn off auto-clean]. |
| APIs / crates | GlobalMemoryStatusEx, NtQuerySystemInformation (SystemMemoryListInformation), K32EnumProcesses, GetProcessMemoryInfo, EmptyWorkingSet, NtSetSystemInformation; crate `windows`, `sysinfo` (existing) |
| Admin | Same as the current cleaner (some system calls need elevation, as in Mem Reduct, C) |
| Effort / risk | S / none |
| No-disk fit | Yes |
| Evidence | Design = J; APIs = J/U |

#### M2. Leak detector / top growers
| Field | Detail |
|---|---|
| What | Sample private bytes per process every ~5 s into fixed ring buffers; flag steady growth (linear fit, e.g. more than X MB per 10 min). |
| Why / comparable | Process Explorer's per-process graphs and System Informer's graph-click-to-inspect (C: [Softpedia](https://www.softpedia.com/progChangelog/Process-Explorer-Changelog-15387.html), [SourceForge](https://systeminformer.sourceforge.io)). Answers the one real memory problem (a leaking app) with data. |
| Example screen | "Growing" list: "Discord.exe  +620 MB in 40 min  [sparkline]  [Restart tip] [Trim this] [Never flag]". A browser family is grouped so open tabs are not flagged. Text: "This app keeps asking for more memory and not giving it back. Closing and reopening it usually fixes it." |
| APIs / crates | GetProcessMemoryInfo (PROCESS_MEMORY_COUNTERS_EX2) or NtQuerySystemInformation; ring buffers (`VecDeque`) |
| Admin | No for own-user processes; elevated for others |
| Effort / risk | M / none (read-only) |
| No-disk fit | Yes in RAM. **Conflict:** needs sampling while the window is hidden to be useful; see section 6.1. |
| Evidence | J |

#### M3. Commit charge, pagefile and compressed memory card
| Field | Detail |
|---|---|
| What | A Home sub-card: "Commit 18.3 / 31.9 GB (57%), pagefile 2.1 GB, compressed 0.9 GB". Warn near the commit limit. |
| Why / comparable | Task Manager only shows "In use (Compressed)" ([How-To Geek](https://www.howtogeek.com/319933/what-is-memory-compression-in-windows-10)). Out-of-commit, not low free RAM, is when apps actually fail (J). |
| Example screen | Under the existing memory-lists bar: three small stats and one sentence "Windows can promise 13.6 GB more to programs before it runs out." Turns amber at 85%: "Close big apps or add pagefile space. Cleaning memory will not help here." |
| APIs / crates | GetPerformanceInfo (CommitTotal/Limit/Peak), GlobalMemoryStatusEx, GetSystemFileCacheSize |
| Admin | No |
| Effort / risk | S / none |
| No-disk fit | Yes |
| Evidence | J; compressed-size API route is **U** (the notes say it is uncertain; may need a query of the "Memory Compression" process) |

#### M4. Game mode (opt-in, standby rule only)
| Field | Detail |
|---|---|
| What | When a fullscreen/borderless game is in the foreground, suspend working-set trims and run only the standby rule at a higher threshold; show one notice. |
| Why / comparable | ISLC (standby list only, aimed at game stutter) ([Wagnardsoft](https://www.wagnardsoft.com/forums/viewtopic.php?p=8066)). Note the evidence is a vendor claim. |
| Example screen | Settings row: "Game mode (experimental, off by default). When a game is running, toolboks only clears the standby cache if it grows above 50% of RAM. This may reduce stutter on some PCs. It is unproven and can do nothing." |
| APIs / crates | GetForegroundWindow, GetWindowThreadProcessId, GetMonitorInfo, SHQueryUserNotificationState; NtSetSystemInformation (standby purge) |
| Admin | Yes for standby purge |
| Effort / risk | M / Medium (harm is possible per the trim-harm sources; mitigated by opt-in + honest copy) |
| No-disk fit | Yes in RAM; useful only while the app keeps running (tray, see section 6.1) |
| Evidence | Value = U; detection design = J |

#### M5. Smaller items
| Item | What / example | Effort | Risk | Evidence |
|---|---|---|---|---|
| Tray percentage icon | Render a small RGBA image with the % and swap at 1-2 s; left click = compact panel, right menu = pause auto-clean. Mem Reduct and CleanMem both have tray displays ([MajorGeeks](https://www.majorgeeks.com/files/details/mem_reduct.html), [MakeUseOf](https://www.makeuseof.com/tag/cleanmem-a-memory-cleaner-that-works)). APIs: Shell_NotifyIconW, CreateIconIndirect; crate `tray-icon` | M | Low | C (demand), J (design) |
| Per-process memory sparkline | In the existing process list, a 5-minute sparkline column from the same ring buffers as M2 | M | None | J |
| Global hotkey for Optimize | RegisterHotKey (e.g. Ctrl+Alt+M); show in UI if the key is taken. CleanMem has a hotkey | S | Low | C (CleanMem), J |
| Rate-limited notifications | Balloon via Shell_NotifyIcon, at most one per N minutes | S | None | J |
| RAM by category (kernel/driver/cache) | RAMMap-style breakdown; needs pool/driver info | L | Low | J; value is medium |
| Default changes | Standby rule and timer auto-clean default **off**; keep "skip small apps" and protect list | S | None | J |

### 2.3 Honest copy (suggested text)
| Where | Text |
|---|---|
| Optimize button tooltip | "Moves memory that apps are not using right now out of RAM. Windows does this by itself when it needs to. You may notice no difference." |
| First open | "Used memory is not a problem. Free memory is wasted memory. Look at 'Commit' and 'Growing apps' if your PC feels slow." |
| Auto-clean toggle | "Off by default. Cleaning on a timer is the least proven option and can make apps briefly slower." |

---

## 3. Resource Monitor

### 3.1 What the research says
Task Manager wins on familiarity and Efficiency Mode but is criticised for slowness and for processes that will not terminate ([XDA via scour.ing](https://scour.ing/p/https://www.xda-developers.com/windows-task-manager-is-fine-but-i-prefer-process-explorer)). Process Explorer and System Informer win power users with trees, handles, per-process graphs and reliable kill ([Softpedia](https://www.softpedia.com/progChangelog/Process-Explorer-Changelog-15387.html), [SourceForge](https://systeminformer.sourceforge.io)). HWiNFO owns sensors and overlays, with a forum report that slow sensors (EC, SMART) stutter overlays ([HWiNFO forum](https://www.hwinfo.com/forum/threads/support-of-rtss-displaying-hwinfo-sensor-values-in-games.45/page-5)). Research was weak on Resource Monitor, Performance Monitor, RTSS/Afterburner, GlassWire/NetLimiter, TCPView, which are background knowledge only (`U`).

**Principle (J):** with a single exe and no drivers, stay with the cheap, unprivileged tier (process enumeration, connection tables, PDH) and do not chase sensors or per-process network bytes.

### 3.2 Recommended Resource Monitor items

#### R1. Processes page
| Field | Detail |
|---|---|
| What | New tab: sortable list with grouping (Chrome (14) 38%), search, **Pause** (freeze order while reading), End task / End tree with confirm, Lower priority / Efficiency mode, Copy row, Open file location. Deny-list for critical processes (csrss, wininit, system). |
| Why / comparable | Process Explorer tree + kill process tree + spacebar pause (C: [Softpedia](https://www.softpedia.com/progChangelog/Process-Explorer-Changelog-15387.html), [TechTarget](https://www.techtarget.com/enterprise-software/definition/Process-Explorer)); Efficiency Mode as a gentler alternative to End task (C: [PC Gamer](https://www.pcgamer.com/new-windows-11-task-manager-lets-you-put-greedy-programs-in-eco-jail)). |
| Example screen | Overview card says "Top: chrome.exe 38% CPU". Clicking it opens Processes sorted by CPU with groups collapsed. Columns: CPU, Memory, GPU, Disk, Threads, Handles, PID. Right-click Teams.exe: [Limit it (Efficiency mode)] [End task...] [Copy]. System processes are greyed with "Windows needs this to run". The End task dialog says "Unsaved work in this app will be lost." |
| APIs / crates | `sysinfo` (existing), PDH (existing), TerminateProcess, SetPriorityClass, SetProcessInformation (EcoQoS, **U**: not verified), Process32First/NtQuerySystemInformation |
| Admin | No for own-user processes; protected ones refuse |
| Effort / risk | M / Low (confirm dialog, deny-list) |
| No-disk fit | Yes |
| Evidence | C (pattern), J (ranking); Pause-as-fix-for-reordering is **U** (common sense, not a documented complaint) |

#### R2. Connections page
| Field | Detail |
|---|---|
| What | Table: Process, Proto, Local, Remote, State; filters "Established only" / "Listening"; Copy remote address; End process; an **on-demand** "Resolve host" button. |
| Why / comparable | System Informer shows per-process connections and can close them ([SourceForge](https://systeminformer.sourceforge.io)); TCPView and CurrPorts are `U` (no sources fetched). |
| Example screen | Network card -> "Connections". A badge shows "spotify.exe: 23". Row action menu: Copy 142.250.x.x:443, End process. Banner: "Showing who your PC talks to right now. Names are looked up only if you press Resolve. This sends lookups over the network." |
| APIs / crates | GetExtendedTcpTable / GetExtendedUdpTable with OWNER_PID classes; doc lists no admin requirement ([MS Learn](https://learn.microsoft.com/en-us/windows/win32/api/iphlpapi/nf-iphlpapi-getextendedtcptable)). Gives connections, **not byte counts**. |
| Admin | No to view |
| Effort / risk | M / Low (privacy: remote IPs; keep in RAM, warn on clipboard export) |
| No-disk fit | Yes |
| Evidence | C (API), J (design) |

#### R3. Alerts and runaway-process banner
| Field | Detail |
|---|---|
| What | Thresholds ("RAM > 90%", "a process above 50% CPU for 30 s") producing an inline banner and, optionally, a tray balloon. |
| Why / comparable | HWiNFO threshold alerts ([PCGH](https://www.pcgameshardware.de/Benchmarks-Thema-58180/Specials/HWiNFO-Pro-Download-Portable-Test-1374622/)); Process Lasso watchdog rules (vendor claims, [Bitsum](https://bitsum.com/?p=268326)). |
| Example screen | Banner on Overview: "game.exe has used 85% CPU for 40 s. [Show] [Lower priority] [End task] [Ignore this app]". Settings strip with three presets (Quiet / Normal / Sensitive) rather than raw numbers. |
| APIs / crates | Existing 1 s samples; Shell_NotifyIcon balloon; `tray-icon` |
| Admin | No |
| Effort / risk | S-M / Low |
| No-disk fit | Yes in RAM; thresholds reset every launch unless persistence is on (section 6.3) |
| Evidence | C (demand), J (design). **Needs background sampling**, section 6.1 |

#### R4. "Why is my PC slow?" button
| Field | Detail |
|---|---|
| What | A 10-second sample then a ranked plain-language verdict: memory pressure/commit, disk queue and latency, one pegged core, GPU/thermal throttle. |
| Why | Combines existing counters into one action (J). It also gives the Memory Cleaner a better answer than "clean now". |
| Example screen | Result card: "1. Memory: 92% used and commit near the limit. Biggest: Docker (6 GB). 2. Disk: C: busy, 45 ms latency. Biggest: SearchIndexer. Hints, not diagnoses." Each line links to the detail and the relevant action. |
| APIs / crates | PDH (Avg. Disk sec/Transfer, queue length), GlobalMemoryStatusEx, GetPerformanceInfo, NVML (existing for GPU) |
| Admin | No |
| Effort / risk | M / Low. Thresholds are **unsourced** and need tuning. Label as hints. |
| No-disk fit | Yes |
| Evidence | J |

#### R5. Smaller Resource Monitor items
| Item | Notes | Effort | Admin | Evidence |
|---|---|---|---|---|
| In-memory history 10-60 min | Ring buffer; small footprint | S | No | J |
| Mini overlay (always-on-top borderless viewport, opacity, click-through, hotkey) | Works for borderless games; does not appear over exclusive fullscreen (U). RTSS is the in-game standard ([HWiNFO forum](https://www.hwinfo.com/forum/threads/support-of-rtss-displaying-hwinfo-sensor-values-in-games.45/page-5)). Build **after** Processes/Connections. | M | No | J/U |
| Per-process GPU via PDH "GPU Engine" | Take the busiest engine per process to mirror Task Manager ([Winaero](https://winaero.com/track-gpu-usage-performance-windows-10)); instance count is large, so expand once, not per tick; cost unmeasured | M | No | C (semantics), U (cost) |
| Copy as text/CSV to clipboard | Fits the no-files rule; warn about hostnames and paths | S | No | J |
| Battery view | GetSystemPowerStatus | S | No | J |
| Thread/handle columns, DPI/interrupt % | Cheap columns on existing data | S | No | J |
| Services list | Control needs admin; overlaps Debloater services page | M | Yes | U |

### 3.3 Do not do (Resource Monitor)
| Item | Reason | Evidence |
|---|---|---|
| Per-core CPU temperature / fan | Needs a third-party driver. LibreHardwareMonitor moved from WinRing0 (flagged by antivirus) to PawnIO, which must be installed separately ([docs.rs](https://docs.rs/crate/cpu-temp/0.1.0/source/src/pawnio/mod.rs), [OhmGraphite notes](https://newreleases.io/project/github/nickbabcock/OhmGraphite/release/v0.36.0)). Show "N/A without sensor driver"; keep NVML for GPU. | C (current LHM behaviour unconfirmed) |
| SMART/NVMe health | Driver-dependent, errors on some drives ([smartmontools #947](https://www.smartmontools.org/ticket/947)); elevation need unresolved | C |
| Per-process network bytes | ETW real-time sessions need admin ([Velociraptor](https://docs.velociraptor.app/artifact_references/pages/windows.etw.kernelnetwork)); costly | C |
| WMI polling at 1 s | Slow; qualitative ranking only | U |

---

## 4. Debloater: polish only

This section deliberately does not re-research tweaks; see `D:\Workspace\GitHub\toolboks\docs\RESEARCH-debloat.md`. It applies only what the utility-suite research says about trust, undo, drift and presets. The notes did **not** inspect the current Debloater UI, so every "improve" below assumes the item is missing; check each against the existing pages first.

| # | Improvement | Why (comparable) | Example in toolboks | Effort | Risk | Disk fit | Evidence |
|---|---|---|---|---|---|---|---|
| D1 | **Dry-run preview ("Review changes") before Apply**: list each change with before/after value in plain words | O&O ShutUp10++ reviewers warn against "apply everything", which can interfere with updates, Defender, Search and OneDrive ([MacMyths](https://macmyths.com/oo-shutup10-review-is-it-safe-and-worth-using-in-2026/), [Neowin](https://www.neowin.net/amp/oo-shutup10-191441/)). Preview is also how BleachBit-style cleaners earn trust (J). | Apply opens a sheet: "5 changes. Turn off ad ID (now: on). Disable Xbox Game Bar (now: on)... [Apply 5] [Back]". Items with side effects get an amber line: "Search may find fewer results." | S-M | Low | Yes | C (O&O) / J |
| D2 | **Session change log with one-click Undo per item and "Undo all"** | Undo is the answer to "safe with undo" and the main trust lever (J). | After Apply, a "This session" list: each row with time, what changed, [Undo]. Header: "Undo lasts until you close toolboks. Create a restore point for longer protection." | M | Low | Yes in RAM; **honest limit**: lost on exit | J |
| D3 | **Restore point nudge before first Apply**, with the result shown | O&O offers a restore point before changes ([Neowin](https://www.neowin.net/amp/oo-shutup10-191441/)). Already a toolboks page; make it a gate, not a separate page. | Apply sheet checkbox "Create a restore point first (recommended)" on by default; shows "Restore point created 14:02" or why it failed. | S | None | Writes a system restore point by design (OS state, not an app file) | C/J |
| D4 | **Drift check: "Windows changed these back"** | O&O notes Windows updates can reset settings so reapply the profile ([MacMyths](https://macmyths.com/oo-shutup10-review-is-it-safe-and-worth-using-in-2026/)). | On open, compare current state to what was applied this session; if persistence is on, to the remembered profile. Banner: "3 of your 12 privacy choices were reset by Windows. [Reapply] [Ignore]". | M | Low | Needs remembered profile for cross-launch drift (section 6.3) | C (drift exists), J (design) |
| D5 | **Presets as named cards with plain risk labels** (Safe / Balanced / Aggressive) with count and what each touches; never a single "optimize everything" button | Suite research: "one-click optimize everything" is a distrust pattern ([Computer Active via PressReader](https://www.pressreader.com/uk/computer-active-uk/20200715/281578062964402), anecdotal); O&O guidance as above. | Cards: "Safe (8 changes, nothing breaks)", "Balanced (21 changes, Search and Xbox features change)". Each opens to the preview. | S-M | Low | Yes | C/J |
| D6 | **Plain-language row copy**: what / why / risk, one line, expandable "?" (not hover-only) | UX research (J). | "Advertising ID. Apps use it to show ads that follow you. Turning off is safe. Some ads become less relevant." | S | None | Yes | J |
| D7 | **Scope guardrail**: never offer to disable Defender, SmartScreen or Windows Update | SignPath terms prohibit software that circumvents security measures and require warnings before system configuration changes ([SignPath terms](https://signpath.org/terms)); Microsoft does not support registry cleaners ([KB 2563254](https://support.Microsoft.com/kb/2563254)). | Not a screen; a design rule. State it in the About page. | S | None | Yes | C |
| D8 | **Copy "What I changed" summary** to clipboard | Support and trust (J). | Button after Apply: copies a plain list. | S | None | Yes | J |
| D9 | **Startup/Tasks tabs: add boot impact** (read Diagnostics-Performance events 100/101: last boot time and apps slowing it) | Autoruns-lite value (J). | Startup page header: "Last boot 48 s. Slowest: OneDrive (+9 s) [Disable]". | S-M | Low | Yes; admin partial | J/U (API names `U`) |

---

## 5. New tools shortlist

All candidate details are the researchers' judgment (`J`); API names are **unverified**. PowerToys (open source, Microsoft) already has Hosts editor, Environment Variables, File Locksmith, Awake ([MS Learn](https://learn.microsoft.com/en-us/windows/powertoys/)), so duplicating those has little value beyond "one exe, no install".

| Rank | Tool | Why users want it (comparable) | Example flow in toolboks | APIs / crates (names only) | Admin | Effort | Risk | Disk fit |
|---|---|---|---|---|---|---|---|---|
| 1 | **Disk Cleaner** (temp, caches, thumbnails, crash dumps, Recycle Bin, update leftovers) | Top reason people install CCleaner/BleachBit ([Neowin](https://www.neowin.net/amp/bleachbit-600-final/)) | Scan lists categories with sizes and checkboxes (safe ones pre-checked; "Update cache" marked "needs admin, cannot be undone"); expand to see top files; Clean shows "Freed 6.2 GB, skipped 14 files in use". **No registry cleaning, ever.** | SHGetKnownFolderPath, FindFirstFileEx, SHQueryRecycleBin, SHEmptyRecycleBin, SHFileOperation | Partial | M | Medium (deletes are irreversible; mitigate with preview, per-category sizes, send user files to Recycle Bin) | Yes (scan in RAM; it deletes, never writes) |
| 2 | **Disk Space Analyzer** (treemap + top 100 files) | WizTree reads the NTFS MFT directly for speed ([diskanalyzer.com](https://diskanalyzer.com/)) | Pick a drive; with admin the scan finishes in seconds, without it a slower walk; click a block to open in Explorer or send to Recycle Bin. | FSCTL_ENUM_USN_DATA, GetFileInformationHandleEx, NtQueryDirectoryFile, SHFileOperation | Yes for fast path | L | Low (read-only) | Yes (memory-heavy on large drives) |
| 3 | **Network tools** (connections reused from R2, DNS flush, ping, adapter info, public IP) | TCPView/CurrPorts (U) | One page; "Flush DNS" with confirm; ping with a live graph. | GetExtendedTcpTable, IcmpSendEcho2, GetAdaptersAddresses, `ipconfig /flushdns` (DnsFlushResolverCache is undocumented) | No to view; Yes for flush | M | Low | Yes. **Public IP lookup is an extra network call; must be on-demand and disclosed.** |
| 4 | **Boot analyzer** (merge with D9) | Autoruns / Task Manager startup impact | Last 10 boot times, slowest apps, link to Debloater Startup. | EvtQuery/EvtNext/EvtRender | Partial | S-M | Low | Yes |
| 5 | **System info sheet with "Copy as text"** | Forum/support posts | One page, Copy button. | GetNativeSystemInfo, GlobalMemoryStatusEx, DXGI, GetLogicalProcessorInformationEx | No | S | None | Yes |
| 6 | **Event log error digest** | Fast answer to "what keeps crashing" | Top 10 recurring errors, counts, plain hint. | EvtQuery with XPath | Partial (Security log needs admin) | M | Low | Yes |
| 7 | **Explorer toggles / classic context menu** | Winaero Tweaker, ExplorerPatcher ([WindowsLoop](https://windowsloop.com/how-to-restore-classic-right-click-menu-in-file-explorer-in-windows-11/)) | Switches with before/after state and Undo. | HKCU CLSID `{86ca1aa0-...}` InhProcServer32, restart Explorer | No | S | Low; reliability on current builds is **U** | Registry write, user-directed (like Debloater) |
| 8 | **Power plan / game mode quick switch** | Convenience; OS holds the state | Dropdown of plans; current plan shown. | PowerEnumerate, PowerSetActiveScheme | No | S | Low | Yes |
| 9 | **Battery wear** | Laptop owners | Wear %, design vs full capacity. Avoid `powercfg /batteryreport` (writes a file). | IOCTL_BATTERY_QUERY_INFORMATION, GetSystemPowerStatus | No | S | Low | Yes |
| 10 | **Sleep/shutdown timer** | Convenience | "Shut down in 45 min" with a countdown; note timer ends if toolboks closes, or use `shutdown /s /t` which the OS keeps. | InitiateShutdownW, AbortSystemShutdownW | Partial | S | Low | Yes |
| Later | Uninstaller with leftovers (BCU/Revo-like) | BCU is free and open source, Revo is freemium ([ghacks](https://www.ghacks.net/2015/04/10/bulk-crap-uninstaller-removes-multiple-programs-silently-on-windows/)) | List leftovers unchecked by default; registry leftovers shown, never auto-deleted. | Uninstall registry keys, MsiEnumProducts | Partial | L | Medium-High | Yes |
| Later | PATH editor (dead-path highlighter + dedupe is the differentiator) | Duplicates PowerToys | In-memory backup of previous value, one-click revert. | HKCU/HKLM Environment, WM_SETTINGCHANGE | Partial | S-M | Medium | Registry write |

**Skip for now (J):** clipboard history (Windows Win+V and Ditto exist; privacy risk), hosts editor (writes a system file; duplicates PowerToys), optional features/DISM, driver list, default-apps fixer (UserChoice is hash-protected, unreliable), Windows Update pause (high risk; deep-link to Settings instead).

**Suggested order (J):** Disk Cleaner, System info, Network tools, Boot analyzer, Disk Space Analyzer, then toggles.

---

## 6. App-wide product, UX, distribution and trust

### 6.1 Conflict: background sampling vs "sample only while visible"

| Option | Behaviour | Cost | Enables |
|---|---|---|---|
| A. Visible only (today) | Sampling stops when hidden/minimized | ~0% idle | Nothing that needs history, alerts, game mode, tray % |
| B. **Opt-in slow background mode (recommended)** | A single toggle "Keep watching in the background". When hidden: sample every 2-5 s from a worker thread; repaint only on state change; one tray icon | Budget (J, proposed, unsourced): under 0.1% CPU and under 40 MB while hidden | Alerts, leak detector, game mode, tray % |
| C. Always background | Always sample at 1 s | Higher | Overlay, but overkill |

Recommended default: **A** until the user switches on B (a single toggle that also reveals the tray). B uses `request_repaint_after` of 5-10 s hidden (J). Because B only matters while the process is running, it works without any disk state. Show the app's own CPU/RAM in About as a dogfood check (J). Budgets are proposals, not measured.

### 6.2 Tray, hotkey, single instance, notifications

| Item | What | APIs / crates | Admin | Effort | Risk | Disk fit | Evidence |
|---|---|---|---|---|---|---|---|
| Single instance | Second launch brings the first window forward | CreateMutexW, registered window message; crate `single-instance` | No | S | None | Yes | J |
| Minimize to tray | Close hides the window; tray menu: Open, Optimize, Pause auto-clean, Quit | Shell_NotifyIcon, TrackPopupMenu; crates `tray-icon`, `muda` | No | S-M | Low | Yes | J |
| Hotkey | Global hotkey for Optimize | RegisterHotKey; crate `global-hotkey` | No | S | Low | Yes | J |
| Notifications: **balloon** (recommended) | NIF_INFO balloon from the tray icon | Shell_NotifyIcon ([NOTIFYICONDATA](https://msdn.microsoft.com/en-us/library/bb773352.aspx)) | No | S | None | Yes (no extra registry) | C |
| Notifications: **modern toast** | Actions and history, nicer | WinRT toast; crates `winrt-notification` | No | M | Low | **No.** Unpackaged apps need a registered AUMID (registry) per [MS Learn](https://learn.microsoft.com/en-us/windows/apps/design/shell/tiles-and-notifications/send-local-toast-other-apps) (snippet; direct fetch 404); the WRL doc says a Start-menu shortcut (a file) ([MS Learn](https://learn.microsoft.com/en-us/windows/apps/design/shell/tiles-and-notifications/send-local-toast-desktop-cpp-wrl)). Whether an HKCU AUMID alone works is **unverified**. | C/U |

Honest trade-off: use balloons by default; offer toasts only as an opt-in that writes an HKCU AUMID key and is removable via "Remove all traces". Windows 11 tray overflow behaviour (users must manually pin the icon) is unverified.

### 6.3 Conflict: persistence rule vs convenience (3 options)

| Option | What is stored and where | Honours "nothing written"? | UX cost | Pros / cons |
|---|---|---|---|---|
| **1. In-memory only** | Nothing | Fully | Thresholds, protect list, alerts reset every launch | Zero footprint; the strongest trust story; but auto-clean/alerts are annoying to reconfigure |
| **2. Opt-in "Remember my settings" (HKCU)** + "Remove all traces" | One HKCU subkey (e.g. `Software\toolboks`) | Technically yes (registry, not a file); **not in spirit**, so label it honestly: "Stores settings in the Windows registry, not a file. Remove any time." | One checkbox | Tiny, deletable, no files; but apps using the registry are not truly portable ([PortableApps](https://portableapps.com/node/1540)) |
| **3. Clipboard export/import + command-line flags** | Nothing by toolboks; the user pastes into their own notes or a shortcut: `toolboks.exe --tool=memory --autoclean=80` | Yes | Manual | Honest and private; the user owns the file (J) |
| Rejected: exe-adjacent `.ini` (Mem Reduct portable style, [GitHub](https://github.com/henrypp/memreduct)) or config appended to the exe | A file | No | None | Breaks the rule; patching the exe breaks signature and hash |

**Recommended default: Option 1 as the shipped default, with Options 2 and 3 both available as opt-ins** ("Remember my settings" checkbox plus "Copy settings"/"Paste settings"). Say it plainly in the first-run tip: "toolboks forgets everything when you close it, unless you turn on Remember."

### 6.4 Start with Windows

| Field | Detail |
|---|---|
| What | Opt-in checkbox, off by default, one HKCU `Run` value; respect `StartupApproved\Run` (do not re-enable what the user disabled in Task Manager). |
| Why | Continuous tools (alerts, auto-clean, game mode) are useless if the app is not running. Mem Reduct, CleanMem and Wise all live in the tray. |
| Example | Settings: "Start toolboks when I sign in. Adds one entry to your Windows startup list. Remove it here any time." "Remove all traces" deletes the Run value, the StartupApproved value and any settings key. |
| Trade-offs | Persistent state outside the exe (registry, not a file). Moving the exe breaks the path; self-update must keep the same path. **Elevation issue:** a Run key does not silently auto-elevate; the fix (scheduled task) writes more system state. Recommend starting **non-elevated** and degrading gracefully (J, unverified in sources). StartupApproved byte values come from community posts, not official docs. |
| Admin / effort / risk | No / S / Low |
| Evidence | C (community: [ElevenForum](https://www.elevenforum.com/t/none-of-my-startup-apps-in-the-registry-is-starting-when-i-login.29084), [TheITBros](https://theitbros.com/add-or-remove-program-to-startup-on-windows/)), J |

### 6.5 Trust: signing, privacy, crash reports

| Item | Detail | Admin | Effort | Risk | Disk fit | Evidence |
|---|---|---|---|---|---|---|
| **SignPath Foundation signing** | Free for OSS: OSI licence, no proprietary components, built in CI from the repo, MFA for all members, manual release approval, published "Code signing policy" and privacy statement, attribution text ([SignPath terms](https://signpath.org/terms)). Needs the repo to be public and open source. | No | M | Low | Yes | C |
| Azure Artifact Signing | GA Jan 2026 but regionally limited (US/Canada orgs; EU/UK orgs); individual onboarding reported paused ([DevClass](https://devclass.com/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/), [MS Q&A](https://learn.microsoft.com/en-us/answers/questions/5810735/cant-create-a-new-trusted-signing-individual-ident)). Norway eligibility **unverified**; pricing figures unconfirmed. | No | M | Low | Yes | C/U |
| Paid OV certificate | Fallback; hundreds USD/yr (J, unsourced) | No | M | Low | Yes | U |
| Set expectations | EV no longer bypasses SmartScreen; reputation builds per file hash, publisher and certificate ([MS Learn](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation), [ToDesktop](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore)). BleachBit saw SmartScreen/AV warnings after a certificate renewal ([Neowin](https://www.neowin.net/amp/bleachbit-600-final/)). Process-killing, startup-registry and memory-trim patterns look malware-like; NirSoft shows false positives persist ([NirSoft](https://www.nirsoft.net/about_nirsoft_freeware.html)). Do not use UPX/packers (J). | - | - | - | - | C/J |
| **Crash dialog, "Copy report"** | `std::panic::set_hook` shows a native dialog with version, message and backtrace; nothing written, nothing sent; optional prefilled GitHub issue link opened only on click. Avoid `human-panic` (writes files). Verify the hook behaves with `panic=abort` (J, unverified). | No | S | None | Yes | J |
| **Privacy statement** | "No telemetry. Only network request: GitHub release check at startup (can be disabled). No data leaves your PC." In About, README and the SignPath policy. Disclose any on-demand lookups (reverse DNS, public IP). | - | S | None | Yes | J |
| Open-source hygiene | SECURITY.md; `cargo audit`/`cargo-deny`; `cargo auditable`; `--locked`, pinned toolchain; publish build hash. | No | S-M | None | Yes | U (all general knowledge) |

### 6.6 Updates

| Item | Detail | Effort | Risk | Disk fit | Evidence |
|---|---|---|---|---|---|
| Verified update | SHA-256 verify (`sha2`) before swap; stronger: embedded-key minisign/ed25519 (`minisign-verify`, `ed25519-dalek`); WinVerifyTrust once signed. Never silent install; show version, size, notes, [Verify and update]. | M | Low-Med | **Partly**: the new exe must exist as a file at least transiently; `self-replace` moves the old exe to a temporary location ([docs.rs](https://docs.rs/crate/self-replace/latest)) | C (crate) / J |
| What's new | Render the GitHub release body (`egui_commonmark`); after relaunch pass `--updated-from X` since there is no stored state | S | None | Yes | J |
| Rollback | A kept `.old` exe is a file, so only a minutes-long window; true multi-version rollback = link to the previous GitHub release (zero local files). Recommended. | S | Low | Yes (link) | J |
| Proxy / TLS | Prefer WinHTTP/Schannel (system proxy and corporate roots, smaller exe) | M | Low | Yes | J |
| Pre-release channel | Checkbox using the release `prerelease` flag; in memory unless persisted | S | Low | Yes | J |
| Delta updates | Skip at ~5 MB; complexity exceeds savings | - | - | - | J |
| Rate limit | Check once per launch (unauthenticated GitHub API limit of 60/h is general knowledge, not verified in the notes) | S | None | Yes | U |

### 6.7 Distribution

| Channel | Notes | Effort | Evidence |
|---|---|---|---|
| GitHub release: signed `.exe` + `.sha256` | Baseline | S | J |
| **winget** | Manifest needs InstallerUrl/InstallerSha256; first version submitted manually; `komac`/`wingetcreate` can automate later; silent install required; test with `winget validate` and `winget install -m` ([MS Learn](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest), [komac](https://www.lib.rs/crates/komac)). Whether bare-exe `portable` type is documented there is **unverified**. A portable winget install copies the exe to LocalAppData and adds a tracking entry: disclose in the README. Compute the hash **after** signing. | S-M | C/J |
| Scoop own bucket | `checkver` + `autoupdate` automate version bumps ([Scoop wiki](https://github.com/lukesampson/scoop/wiki/App-Manifest-Autoupdate)) | S | C |
| Chocolatey, MSIX/Store | Skip: heavier, and MSIX/Store sandboxing could break Memory Cleaner/Debloater. Store avoids the SmartScreen warning per MS docs. | - | J |

### 6.8 Accessibility, localization, quality

| Item | Detail | Admin | Effort | Evidence |
|---|---|---|---|---|
| Screen reader basics | eframe enables AccessKit by default (UI Automation on Windows) ([eframe Cargo.toml](https://github.com/emilk/egui/blob/main/crates/eframe/Cargo.toml)); coverage is incomplete. Give custom graphs a text label ("RAM 62 percent used"); test with Narrator and NVDA. | No | M | C/J |
| Keyboard | Tab order, Enter/Space, Esc closes dialogs, visible focus, Ctrl+1/2/3 tool switch | No | S | J |
| High contrast / light theme / text size | SPI_GETHIGHCONTRAST; `Visuals::light()`; text-size setting; PerMonitorV2 manifest; minimum ~13-14 px body text because "compact dark" risks small text. Never use colour alone for status. | No | M | J |
| Norwegian (nb) | `sys-locale`, `fluent` or `rust-i18n`; decimal comma; ship en + nb in the binary | No | M | J |
| "?" help rows | Expandable text, not hover-only (keyboard and touch accessible) | No | S | J |
| Testing | A `SystemApi` trait with fakes; destructive tests in Windows Sandbox (Pro: disposable, nothing persists, one instance) ([MS Learn](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-overview)), plus a before/after registry diff to prove "nothing written" | No | M | C/J |
| CI | Manual `workflow_dispatch`: build, clippy, test, audit, SignPath sign, release (free minutes unverified) | No | M | U |

---

## 7. What NOT to build

| Don't build | Why | Evidence |
|---|---|---|
| **Registry cleaner / "fix issues" scanner** | Microsoft does not support registry cleaners and disclaims responsibility ([KB 2563254](https://support.Microsoft.com/kb/2563254)); they "always find errors" ([Malwarebytes](https://www.malwarebytes.com/blog/news/2016/08/systweak-redux-our-response)) | C |
| **Driver booster/updater** | Distrust pattern around IObit/Glary (anecdotal: [Computer Active](https://www.pressreader.com/uk/computer-active-uk/20200715/281578062964402), [MalwareTips](https://malwaretips.com/threads/iobit-is-scummy.94540)); list-only is fine, defer updates to Windows Update | C (anecdotal) |
| **RAM "booster" claims, health score, "optimize everything" button** | The category's weakest claim; CleanMem's own developer disclaims speedup | C/J |
| Disabling Defender, SmartScreen, Windows Update, System Restore; auto-update bypass | Trust, and SignPath's terms on circumventing security measures | C/J |
| Kernel driver or bundled PawnIO/WinRing0 | AV flags; installation burden; breaks single-exe | C |
| Timer-based auto-trim of all processes as a default | Weakest evidence, highest harm | J |
| Always-on telemetry, ads, bundling, nag popups | Core trust differentiator | C |
| Config file next to the exe, or config patched into the exe | Breaks the rule; breaks signature | J |
| Process-hiding or "stealth" features, UPX packing | AV false positives | J |
| MSI/MSIX packaging | Clashes with the single-exe ethos | J |
| Features duplicating PowerToys with no differentiator (hosts, env editor, Awake) | Low value | C |

---

## 8. Proposed 3-step roadmap

| Step | Build (in order) | Exit criteria |
|---|---|---|
| **1. Honest and visible (about 2-3 weeks)** | M1 before/after report; M3 commit/pagefile card; honest copy and auto-clean default off; R1 Processes page (sort, group, Pause, End task + deny-list); D1 preview + D2 session undo log + D6 plain copy; crash dialog; privacy statement in About | A user can press Optimize and see a before/after card; End task never targets critical processes; Debloater Apply always shows a preview and an Undo list; panic shows a copyable report; registry diff in Windows Sandbox shows nothing written beyond documented state |
| **2. Present but quiet (about 2-3 weeks)** | Single instance + tray + minimize to tray + opt-in slow background mode (6.1); R3 alerts (banner + balloon); M2 leak detector; R2 Connections page; opt-in "Remember settings" + "Remove all traces" + clipboard export/import; **prototype on a clean VM**: balloon vs toast, hotkey conflicts, Run key | Hidden idle CPU under 0.1% and under 40 MB working set (self-set budgets, measured and recorded); alerts fire while hidden; "Remove all traces" leaves zero toolboks keys (verified by registry diff); an unresolved-risk list from the prototype is closed or explicitly dropped |
| **3. Trusted and distributed (about 3-4 weeks)** | SignPath signing and public policy (repo public, OSI licence, CI); verified self-update with SHA-256 + What's new; winget then Scoop; accessibility pass; nb localization; Disk Cleaner as the first new tool; D4 drift check | Signed release produced by CI; update refuses a tampered download in a test; `winget install` works from a clean VM; Narrator reads graphs; Disk Cleaner scan/clean reports bytes freed and skipped files, with no registry cleaning |

---

## 9. Open decisions for the owner

| # | Question | Recommended default |
|---|---|---|
| 1 | Should Memory Cleaner be renamed/reframed (e.g. "Memory") with the cleaner as one action instead of the headline? | Yes: reframe as analyzer first; keep the Optimize button; auto-clean off by default |
| 2 | Persistence: in-memory only, opt-in registry, or clipboard export? | Ship in-memory by default; add opt-in HKCU "Remember" and clipboard export/import (6.3) |
| 3 | Allow an opt-in slow background mode (and a tray icon) when "sample only while visible" is the current rule? | Yes, opt-in, 2-5 s sampling, hidden budget under 0.1% CPU |
| 4 | Is the registry acceptable as "not a file" for Start with Windows and settings, or is any persistent state out? | Acceptable if opt-in, visible, removable via "Remove all traces" |
| 5 | Will the repo be public and open source (needed for SignPath Foundation)? Which licence (MIT/Apache-2.0 vs GPL-3.0)? | Public, MIT/Apache-2.0 dual, apply to SignPath; budget an OV cert as a fallback |
| 6 | Notifications: balloon only, or also modern toasts with an HKCU AUMID? | Balloon only until the clean-VM test shows HKCU AUMID works |
| 7 | Keep a `.old` exe after update for rollback, or rely on previous GitHub releases? | Previous GitHub releases (no local file); delete any transient `.old` on next successful start |
| 8 | Should the Resource Monitor get Processes/Connections (a Task Manager role) before the mini overlay and the "Why is my PC slow?" button? | Yes: Processes, then Connections, then alerts, then overlay |
| 9 | Which first new tool: Disk Cleaner or Disk Space Analyzer? | Disk Cleaner (M effort, no admin for the safe categories); analyzer later (L) |
| 10 | Include Norwegian (nb) and light theme in the first public release? | After accessibility basics; nb first, light theme second |

---

## 10. What the research could NOT verify

| Area | Not verified |
|---|---|
| Trimming evidence | No Raymond Chen or Russinovich statements; no independent benchmark of standby purging in games; no Windows 11 24H2/25H2 standby-behaviour research; the MS PDC10 and xperf quotes are from search snippets only; the engineered.at 250x figure is a secondary AI-summarised page and an extreme case |
| Competitor facts | Pricing, ads and telemetry for Wise, Razer Cortex, RAMBooster, Memory Cleaner (Store); Mem Reduct hotkey/notification behaviour; Process Lasso current SmartTrim and licence; System Informer memory tab |
| Demand | No downloads, search-volume or AlternativeTo/Reddit data; every ranking in this report is judgment |
| Windows APIs | Almost all API names (NtSetSystemInformation memory lists, compressed-memory route, EcoQoS, PDH counter names, GPU Engine cost, IOCTL privileges, DnsFlushResolverCache, EvtQuery details) are researcher knowledge, not checked against Microsoft Learn |
| Resource tools | No sources for Resource Monitor, Performance Monitor, MSI Afterburner/RTSS (beyond HWiNFO integration), HWMonitor, GlassWire, NetLimiter, TCPView, WizTree sizes |
| Persistence/tray | Whether an HKCU AUMID alone allows toasts for an unpackaged exe; Windows 11 tray-overflow behaviour; StartupApproved byte values (community only); PowerToys/System Informer settings locations |
| Updates/distribution | GitHub rate limit; winget `portable` type rules; Chocolatey/MSIX requirements; Scoop Extras acceptance; Komac first-package rule (low-confidence URL); delta-update practice |
| Signing | Norway/EEA eligibility for Azure Artifact Signing; Azure pricing (the official page showed blank fields); SignPath turnaround time |
| Quality | Idle CPU/RAM of comparable tools; GitHub Actions free minutes; whether `panic=abort` still runs the hook; current AccessKit/egui widget coverage and Windows screen-reader bugs |
| Suite sentiment | CCleaner post-2017 telemetry/ads complaints; IObit/Glary evidence is mostly forum and Trustpilot; no test data on whether registry cleaners ever help |
| Debloater | The notes did not inspect the current toolboks Debloater pages; the polish list assumes gaps that must be checked against the app and against `docs/RESEARCH-debloat.md` |
| Usability | None of the proposed screens or flows were user-tested; all heuristic thresholds (diagnosis, leak detection, alerts) are unsourced |

---

## Conclusion

The research moves the question from "what else can toolboks clean?" to "what can it show and prove?". The one finding that held across every source type is that memory cleaning's value is mostly perceived, so the defensible product is the one that measures before and after and says so. The second is that trust is cheap to lose and expensive to rebuild: the vendors users avoid are the ones that bundle, upsell and over-promise, and the tools they trust are small, open, signed and plain about what they change.

Three of the app's own rules need explicit policy rather than silence. "Nothing written" cannot be absolute once the app can remember settings, start with Windows, post toasts or update itself. The report's answer is to keep zero-footprint as the shipped default, make every exception opt-in, visible and removable, and test that claim with a registry diff in Windows Sandbox. The thinnest evidence sits under tray behaviour, toast registration and API details, so Step 2 of the roadmap puts a clean-VM prototype before any commitment.
