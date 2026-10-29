# Ship a small, honest, undoable debloater

**Ship v1 as a short, evidence-graded list: startup entries first, then a ~10-task allow-list, privacy and ads registry settings, and only a handful of services.** The research supports far less than the typical debloat-tool catalog. Microsoft publishes no consumer "safe to disable" list for Windows 11. The only official service guidance is for Windows Server 2016 ([MS Learn](https://learn.microsoft.com/en-us/windows-server/security/windows-services/security-guidelines-for-disabling-system-services-in-windows-server)), and most service rows rest on community consensus or unverified memory. The policy layer is better documented but edition-fragmented. Home appears in no Policy CSP table, and many consumer-feature policies are Enterprise/Education only, with Pro explicitly excluded ([Experience CSP](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-experience)). Gains are small (about 0.5 GB RAM and 0.4-0.6 GB disk on a fresh 25H2 VM; SysMain off gave no gain on a 16 GB PC), so the honest pitch is privacy, fewer ads and a cleaner startup, not speed. Safety comes from design, not from the catalog: tiers with Safe-only defaults, read-back after every write, an "effective state" check, in-memory undo, clipboard undo steps, and refusing or going read-only on managed PCs. Failure reports against existing tools cluster around app removal, servicing and update plumbing, background-apps-off and search policies, not around telemetry registry values ([WinUtil issues](https://github.com/ChrisTitusTech/winutil/issues?q=is%3Aissue+broke)). The biggest unresolved risks are not technical: unsigned-exe blocking (Smart App Control, SmartScreen, ML antivirus) and the fact that nothing was verified on a Home PC or on build 26300 itself.

---

## 1. Executive summary for the product owner

### What the tool can honestly promise
| Promise | Basis |
|---|---|
| Turn off a defined list of telemetry, ads, suggestions and AI-surface settings | Microsoft policy docs for most; community for per-user "preference" keys |
| Show exactly what changed, and undo it during the same session | Our design; all state held in memory |
| Warn when Windows will ignore a setting (Home, managed PC) instead of claiming success | Design: read-back plus effective-state check |
| Make a typical PC faster | NOT supported. Gains are small (see section 8) |
| Keep changes after Windows updates | NOT guaranteed. Evidence is anecdotal; the tool re-reads state at launch and offers re-apply |
| Undo after the app is closed | NOT possible from saved state (see constraint override below) |

### Constraint override: no backup file
The tasks_startup notes suggested persisting a JSON/registry backup so Undo survives a restart. **That is not allowed for toolboks and is dropped.** Instead:
1. **In-memory undo** for the running session: before each change, capture the exact prior state (registry value type+data or absence, service start type + delayed flag + running state, task Enabled flag, StartupApproved 12 bytes) in a `Vec<UndoRecord>`. Per-row "Undo" and "Undo all".
2. **"Copy undo steps"** to the clipboard (`reg add` / `reg delete` / `sc config` / `schtasks /change` lines) so the user can keep the recipe outside the process. The UI must say plainly: closing the app discards undo.
3. **Re-read real state on every launch.** Show each item as Applied / Not applied / Differs, and offer "Re-apply". We cannot say "changed since last run" because that needs a prior snapshot, which would mean writing to disk.
4. **Consequence to state in the UI:** after a restart the tool does not know the pre-tool value. It can only offer "Revert to Windows default" for items where the default is known (delete a policy value; known stock start type), clearly labelled as such, because it could otherwise overwrite a value the admin set deliberately.
5. **Elevation consequence (engineering inference, not in the notes):** if "elevate on demand" is a relaunch with `runas`, the new process has an empty undo list. Either elevate before the first Apply, pass the pending selection by command line (no disk), or split the work so one process owns all state. Decide this before building Phase 5 (open decision 8).

### Recommended v1 scope (what to build, in order)
| Priority | Category | v1 contents | Why |
|---|---|---|---|
| 1 | Startup entries | Enumerate Run/StartupFolder entries, toggle via StartupApproved, show publisher, hard-deny list | Lowest risk, per-user (non-admin for HKCU), fully reversible, visible benefit; mechanism confirmed on the owner's PC |
| 2 | Privacy and ads registry (Safe tier) | ~15 settings: diagnostic data to Required, advertising ID, tailored experiences, activity history, feedback prompts, inking/typing, Start/Settings suggestions, Widgets policy, Game Bar capture prefs, Edge nags | Policy docs are Microsoft-verified for most rows; undo = restore prior value or delete |
| 3 | Scheduled tasks | Exact-path allow-list of ~8-10 telemetry/feedback tasks | Observed on build 26300; COM API is documented; absent tasks handled as "not present" |
| 4 | Services | Safe tier only: DiagTrack, Fax, RetailDemo, MapsBroker, lfsvc, PhoneSvc, wisvc, icssvc and a few more; Careful tier is Manual-only, off by default | Weakest evidence, highest blast radius, so last |
| 5 | AI switches | Recall, Click to Do, Paint AI, legacy Copilot policy as Careful opt-ins | Documented policies exist, but several are Insider-tagged and Recall off deletes snapshots |
| Later | Restore point, presets, signing | See roadmap | Needed before wide distribution |

**Not in v1:** removing apps/AppX, Edge removal, Defender/Update/Store anything, Compatibility Appraiser (default off, optional), Enterprise-only policies on Pro, Copilot app removal by policy.

### Evidence legend (used in every table)
| Code | Meaning |
|---|---|
| **MS** | Microsoft Learn/Support page fetched in the notes and stating the fact (client Policy CSP, Win32 API docs) |
| **MS-S** | Microsoft guidance, but for Windows Server 2016 Desktop Experience, not Windows 11 client. Real source, imperfect fit |
| **C** | Community only: Win11Debloat, WinUtil, Atlas, forums, blogs. Present in a tool or report, not confirmed by Microsoft |
| **U** | Unverified from the researcher's background knowledge. Do not rely on it until checked |
| **OBS** | Existence/default state observed first-hand (read-only) on the owner's build 26300 PC, 2026-10-29. Says nothing about safety |

Tier meaning: **Safe** = show, may be pre-selected, reversible, no known loss of core function. **Careful** = show collapsed, never pre-selected, with a consequence sentence. **Never-offer** = not shown, or shown read-only with a lock icon.

### Source key (used in the "Src" columns)
| Key | Source |
|---|---|
| S1 | [MS Learn: Security guidelines for system services (Server 2016)](https://learn.microsoft.com/en-us/windows-server/security/windows-services/security-guidelines-for-disabling-system-services-in-windows-server) |
| S2 | [MS Learn: Per-user services](https://learn.microsoft.com/en-us/windows/application-management/per-user-services-in-windows) |
| S3 | [SERVICE_DELAYED_AUTO_START_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_delayed_auto_start_info) |
| S4 | [SERVICE_TRIGGER](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_trigger) |
| S5 | [ChangeServiceConfigW](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-changeserviceconfigw) |
| S6 | [SERVICE_LAUNCH_PROTECTED_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_launch_protected_info) |
| S7 | [Tamper protection overview](https://learn.microsoft.com/en-us/defender-endpoint/tamper-protection-overview) |
| S8 | [WinUtil services tweak docs](https://winutil.christitus.com/dev/tweaks/essential-tweaks/services/) |
| S9 | [WinUtil tweaks.json](https://raw.githubusercontent.com/ChrisTitusTech/winutil/main/config/tweaks.json) |
| S10 | [Windows Latest SysMain benchmark](https://www.windowslatest.com/2026/10/26/everyone-says-disabling-sysmain-speeds-up-windows-11-so-i-benchmarked-it-on-two-pcs-k4l9/) |
| S11 | [ElevenForum SysMain NVMe](https://www.elevenforum.com/t/does-disabling-sysmain-superfetch-make-any-sense-with-an-nvme-system-drive.6625) |
| S12 | [MS Q&A WorkloadsSessionHost](https://learn.microsoft.com/en-ca/answers/questions/5891331/workloadssessionhost-using-4gb-ram-on-copilot-pc) |
| S13 | [Atlas debloat reference](https://www.mintlify.com/Atlas-OS/Atlas/reference/tweaks/debloat) |
| S14 | [winhelponline: feature update not offered](https://www.winhelponline.com/blog/feature-update-not-offered-wu/) |
| S15 | [IRegisteredTask::put_Enabled](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-iregisteredtask-put_enabled) |
| S16 | [Task security contexts](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks) |
| S17 | [windowsir: StartupApproved](https://windowsir.blogspot.com/2022/07/does-autostart-really-mean-autostart.html) |
| S18 | [Policy CSP System](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-system) |
| S19 | [Policy CSP Experience](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-experience) |
| S20 | [Policy CSP Search](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-search) |
| S21 | [Configure diagnostic data](https://learn.microsoft.com/en-us/windows/privacy/configure-windows-diagnostic-data-in-your-organization) |
| S22 | [Manage connections](https://learn.microsoft.com/en-us/windows/privacy/manage-connections-from-windows-operating-system-components-to-microsoft-services) |
| S23 | Policy CSP Privacy, Start, DeliveryOptimization, ApplicationManagement (same URL pattern, e.g. `.../mdm/policy-csp-privacy`; the notes name these pages without individual URLs) |
| S24 | [Policy CSP WindowsAI](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-windowsai) |
| S25 | [Policy CSP NewsAndInterests](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-newsandinterests) |
| S26 | [Edge policy example: StartupBoostEnabled](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-browser-policies/startupboostenabled) |
| S27 | Win11Debloat: [Features.json](https://raw.githubusercontent.com/Raphire/Win11Debloat/master/Config/Features.json), [Apps.json](https://raw.githubusercontent.com/Raphire/Win11Debloat/master/Config/Apps.json), [Reverting wiki](https://github.com/Raphire/Win11Debloat/wiki/Reverting-Changes) |
| S28 | [MS Support: policy-based removal of Store apps](https://support.microsoft.com/servicing/os/windows/docs/2025/08/policy-based-removal-of-pre-installed-microsoft-store-apps) |
| S29 | [The Register: Copilot removal policy](https://www.theregister.com/2026/01/12/microsoft_copilot_removal/), [Petri](https://petri.com/windows-11-admins-remove-copilot-policy/) |
| S30 | [Build 26300.8687 notes](https://learn.microsoft.com/en-us/windows-insider/release-notes/experimental/preview-build-26300-8687) |
| S31 | [SRSetRestorePointW](https://learn.microsoft.com/en-us/windows/win32/api/srrestoreptapi/nf-srrestoreptapi-srsetrestorepointw), [RESTOREPOINTINFOW](https://learn.microsoft.com/en-us/windows/win32/api/srrestoreptapi/ns-srrestoreptapi-restorepointinfow) |
| S32 | [NetGetJoinInformation](https://learn.microsoft.com/en-us/windows/win32/api/lmjoin/nf-lmjoin-netgetjoininformation), [GetProductInfo](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getproductinfo) |
| S33 | [Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview), [Code signing options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options) |
| S34 | [SignPath terms](https://signpath.org/terms), [origin verification](https://docs.signpath.io/origin-verification); [Defender FP/FN process](https://learn.microsoft.com/en-us/defender-endpoint/defender-endpoint-false-positives-negatives) |
| S35 | [Softpedia: update re-enables settings](https://news.softpedia.com/news/windows-10-update-re-enables-all-data-collection-features-disabled-by-third-party-apps-497277.shtml), [AskWoody](https://www.askwoody.com/forums/topic/stop-windows-from-changing-telemetry-settings-during-update) |
| S36 | [WinUtil issues](https://github.com/ChrisTitusTech/winutil/issues?q=is%3Aissue+broke), [Win11Debloat issues](https://github.com/Raphire/Win11Debloat/issues?q=is%3Aissue+broke) |
| S37 | [Winaero](https://winaero.com/windows-11-now-has-the-option-to-remove-recommended-from-start-but-you-cant-use-it), [Pureinfotech Start](https://pureinfotech.com/remove-start-menu-recommended-items-windows-11) |
| S38 | First-hand read-only inspection of the owner's PC (Win11 Pro 26300, 2026-10-29); no URL |

---

## 2. Conflicts between the notes, and how this report resolves them

| # | Conflict | Resolution used in this report |
|---|---|---|
| 1 | `DisableCloudOptimizedContent`: Policy CSP says Device scope (HKLM); the Manage-connections page says HKCU (S19, S22) | **Flagged, unresolved by evidence.** Write HKLM (documented, Ent/Edu only) and mirror to HKCU; read both back. Pro/Home effect undocumented either way. Verify on the PC |
| 2 | `HideRecommendedSection`: CSP lists Pro+; community says reliably honoured only on SE/Education; Win11Debloat uses HKCU (S23, S37) | Treat as **best-effort, Careful**. Gate on effective-state check (is the Recommended section actually empty/hidden?). Primary Pro route: the per-user Start toggles (`Start_IrisRecommendations`, `Start_AccountNotifications`, `Start_TrackProgs`), which community sources say auto-hide the section on 24H2/25H2 when all are off (C) |
| 3 | Consumer-features policies on Pro: policies.md table and new_features.md agree these are **Ent/Edu, Pro explicitly NOT supported** (S19). Community tools write them on Pro anyway | Catalog marks them "Ent/Edu documented; Pro undocumented, best-effort". Never claim success on Pro without read-back plus visible effect. Pair with HKCU ContentDeliveryManager prefs (C) as the Pro/Home route |
| 4 | `RemoveMicrosoftCopilotApp`: table says Pro unsupported, description text says "Enterprise, Professional and Education"; plus three AND-ed conditions (both Copilot apps installed, not user-installed, not launched in 28 days); the notes list **no registry mapping**; rollout status conflicts between sources (S24, S29) | **Not offered in v1.** Cannot be implemented as a reliable registry write, edition unclear, one-shot, user can reinstall |
| 5 | WerSvc and RemoteRegistry: Microsoft's server page says "Do not disable"; WinUtil and others disable them (S1, S9) | The server page is server-oriented and says non-default services should not be blanket-disabled. WerSvc = **Careful, opt-in** (Reliability Monitor and crash reports). RemoteRegistry = **Careful**, show current state; likely already Disabled on client (U), so little to gain. Neither is pre-selected |
| 6 | PcaSvc: the Safe list in the services takeaway includes it "as Manual"; the row table says Careful; MS-S says OK to disable | **Careful, Manual-only.** Compatibility prompts for legacy apps can be lost |
| 7 | Xbox services: the Safe list includes them; the row table says Careful (Store games, Game Bar depend); MS-S says "should be disabled" on Server | **Careful.** Server advice does not transfer to a gaming desktop. Offer only after a "do you use Xbox/Store games?" question |
| 8 | Telemetry level: Win11Debloat writes `AllowTelemetry=0` at a legacy path; Microsoft says 0 is Ent/Edu/Server only and Pro treats it as 1 (S18, S21) | Write **1 (Required)** on Home/Pro, 0 only on Enterprise/Education. Show "Required" as the honest label. Microsoft also notes failure data is not sent when off, which affects Windows Update diagnostics |
| 9 | `DODownloadMode`: CSP default 0 = HTTP-only; Manage-connections page suggests 99; WinUtil uses 0 (S22, S23, S9) | Use **0**; 99 is an alternative. Flagged: verify which value the Settings UI reflects on the PC |
| 10 | Edge `HideFirstRunExperience`: Win11Debloat writes 0, which looks like a bug (S26 area) | Use **1** (value semantics from Edge policy pages, MS). Do not copy the community file |
| 11 | tasks_startup: "better, persist a JSON/registry backup file" | **Rejected** (project constraint). Replaced as described in section 1 |
| 12 | Services: "trigger" services - Manual vs Disabled. Inference (U): Manual does not stop trigger starts; only Disabled does | Offer **Disabled** for Safe-tier trigger services (reversible via SCM). Careful tier gets **Manual only** because Disabled can break dependents |

---

## 3. Services: tiered catalog

**Read this first.** Defaults below are "Win11 client typical" and mostly **U**. The notes found no per-service default table for Win11 Home vs Pro and no 25H2/26300 values. The tool must read live values (QueryServiceConfigW) and never hard-code defaults for undo. Only the per-user template defaults (S2) are Microsoft-verified for client.

### 3.1 Safe tier (show; Disabled; not default-on except where noted)
| Service (id) | Typical default | Recommended action | What breaks | Edition / admin | Evidence | Src |
|---|---|---|---|---|---|---|
| DiagTrack (Connected User Experiences and Telemetry) | Automatic (U) | Disabled. Pair with diagnostic-data policy | Telemetry upload; Feedback Hub diagnostics. Microsoft may re-enable it (U) | Admin; all editions | C (WinUtil disables, original Automatic); MS-S says "No guidance" | S1, S8, S9 |
| dmwappushservice | Manual/trigger (U) | Disabled **only** if not MDM/Intune-enrolled | MDM/Intune enrollment, Unified Write Filter. MS: "required on client devices for Intune, MDM..." | Admin; hide on managed PCs | MS-S (OK on Server; client caveat) | S1 |
| Fax | Manual (U) | Disabled | Windows Fax and Scan | Admin | U | none |
| RetailDemo | Manual (U) | Disabled | Retail demo mode | Admin | U | none |
| MapsBroker (Downloaded Maps Manager) | Automatic delayed, trigger (U) | Disabled | Apps using offline maps | Admin | MS-S OK; WinUtil sets Manual (C) | S1, S8 |
| lfsvc (Geolocation) | Manual, trigger (U) | Disabled | Apps needing location | Admin | MS-S OK; WinUtil Disabled (C) | S1, S9 |
| PhoneSvc | Manual (U) | Disabled if Phone Link calling unused | Phone Link calling, VoIP apps | Admin | MS-S OK | S1 |
| wisvc (Windows Insider Service) | Manual (U) | Disabled if not an Insider. **Do not offer on this build line if the user is in Experimental/Insider channel** | Insider flight settings | Admin | MS-S OK | S1 |
| icssvc (Mobile Hotspot) | Manual (U) | Disabled | Mobile Hotspot | Admin | MS-S OK | S1 |
| WalletService | Manual (U) | Disabled | Wallet/payment app support | Admin | U | none |
| SEMgrSvc (Payments and NFC/SE Manager) | Manual (U) | Disabled | NFC/secure-element payments | Admin | U | none |
| Wecsvc (Event Collector) | Manual (U) | Disabled | Event forwarding | Admin | U | none |
| BcastDVRUserService (per-user template) | Manual (MS) | Disable the template | Game Bar recording/broadcast | Admin | Default MS (S2); recommendation C/U | S2 |

### 3.2 Careful tier (hidden behind "Show careful items"; never pre-selected; **Manual only unless stated**)
| Service (id) | Typical default | Recommended action | What breaks | Evidence | Src |
|---|---|---|---|---|---|
| SysMain | Automatic (U) | **Leave.** Offer Manual only on explicit request, with benchmark text | Prefetch/pre-load; cold app launch slower (Clipchamp 3.14 s to 8.48 s on a 4 GB VM) | C (benchmark); MS-S "No guidance" | S10, S11, S1 |
| WSearch | Automatic delayed (U) | Leave; Manual on request | Start/Explorer/Outlook search and indexing. Win11Debloat #70 (Bing/Cortana search policy) broke all search | U; C (issue) | S36 |
| Spooler | Automatic (U) | Opt-in Disable for no-printer PCs | ALL printing incl. Print to PDF. MS-S: OK if not a print server | MS-S | S1 |
| PrintNotify | Manual (U) | Leave | Printer notifications/extensions | U | none |
| WerSvc | Manual, trigger (U) | Manual/Disabled only if user opts out of crash reports | Crash reports, Reliability Monitor. MS-S: "Do not disable" (server) | MS-S conflict; C | S1, S9 |
| RemoteRegistry | Disabled on client (U) | Show state; disable if not already. Default off | Remote registry/management. MS-S "Do not disable" (server) | MS-S conflict; C | S1, S8 |
| XblAuthManager, XblGameSave, XboxNetApiSvc, XboxGipSvc | Manual (U) | Manual only; Disabled never | Xbox sign-in, cloud saves, accessories, Store/Game Pass games. MS-S says Should be disabled (XblAuthManager, XblGameSave) on Server | MS-S; U | S1 |
| GamingServices / GamingServicesNet | Manual (U) | Leave; skip if absent | Store/Xbox PC games fail to install/launch. Repair needs MS Gaming Services Repair Tool | U | S27 |
| SharedAccess (ICS) | Manual (U) | Leave (WinUtil disables) | Mobile Hotspot, Miracast both ends | MS-S OK; C | S1, S9 |
| TrkWks | Automatic (U) | Manual | NTFS link tracking | MS-S "No guidance" | S1 |
| DoSvc (Delivery Optimization) | Automatic delayed, trigger (U) | **Prefer policy** `DODownloadMode`; leave service | P2P updates; WU still works over HTTP | U | S23 |
| BITS | Manual (U) | Leave | Windows Update, Store, Office background transfers | MS-S "No guidance", relied on by WU | S1 |
| PcaSvc | Automatic (U) | Manual only | Compatibility prompts for legacy apps | MS-S OK; conflict 6 | S1 |
| SCardSvr, ScDeviceEnum | Manual (U) | Leave | Smart cards, enterprise logins, some Windows Hello | MS-S; U | S1 |
| TabletInputService | Manual (U) | Leave on touch/pen PCs | Touch keyboard, pen, handwriting | U | none |
| WbioSrvc | Manual (U) | Leave if Hello used | Fingerprint/face sign-in | MS-S "No guidance" | S1 |
| bthserv | Manual (U) | Leave unless no Bluetooth | All Bluetooth devices | MS-S OK if unused | S1 |
| CDPUserSvc, OneSyncSvc, MessagingService (per-user) | CDPUserSvc Automatic; OneSyncSvc Automatic delayed; Messaging Manual (MS) | Template only, optional | Nearby sharing, clipboard/mail/calendar sync | Defaults MS; effect U | S2 |
| DusmSvc | Automatic (U) | Leave | Data usage stats | U | none |
| DPS, WdiServiceHost, WdiSystemHost | Auto/Manual (U) | Leave | Troubleshooters, network diagnostics | U | none |
| WSAIFabricSvc (Windows AI Fabric) | Present mainly on Copilot+ PCs | **Manual only, only if present**; Win11Debloat does the same | Recall, Click to Do, Live Captions, Settings AI search. MS Q&A: no supported way to fully disable; one report of data wiped; updates may re-enable | C (forum-grade) | S12, S27 |
| edgeupdate, edgeupdatem | Automatic delayed / Manual (U) | Manual at most; **not recommended** | Edge security patches stop | U | S22 |
| Third-party updaters (gupdate, AdobeARMservice...) | varies | Detected list, opt-in, Manual | App auto-update. MS warns against touching non-default services | MS-S principle | S1 |

### 3.3 Never offer (services)
| Service(s) | Reason |
|---|---|
| WinDefend, WdNisSvc, Sense, SecurityHealthService, wscsvc, WdNisDrv, SgrmBroker | Antivirus and Security Center. Tamper Protection blocks changes (including registry) and the attempt looks like malware ([S7](https://learn.microsoft.com/en-us/defender-endpoint/tamper-protection-overview)). Launch-protected services refuse ChangeServiceConfig from unprotected processes ([S6](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_launch_protected_info)) |
| wuauserv, UsoSvc, WaaSMedicSvc, TrustedInstaller | Windows Update stack. WaaSMedic repairs update services (U), so disabling is unreliable and unsafe. WinUtil issues #4171/#4811 show real breakage (S36) |
| mpssvc, BFE | Firewall |
| StorSvc, Themes, WpnService | Storage Sense/app installs; visual styles; push notifications and Start/Store quirks (U) |
| DcomLaunch, RpcSs, PlugPlay, Power, ProfSvc, EventLog, CryptSvc, gpsvc, LSM, StateRepository, CoreMessagingRegistrar | Core plumbing (U list from notes) |
| Store/Appx service plumbing | Microsoft Store is the one app that is hard to reinstall (S27) |
| Anything not in the catalog | Allow-list only; no "disable everything telemetry-ish" |

### 3.4 Disable vs Manual: the recommendation
Safe-tier trigger services (lfsvc, MapsBroker, dmwappushservice, WerSvc) need **Disabled** to stop event-triggered starts (U: Manual does not block triggers, only Disabled makes the SCM refuse the start). Careful-tier items get **Manual only**, because Disabled makes the SCM refuse starts for dependents (`ERROR_SERVICE_DEPENDENCY_DEPRIVED`) even though the config change itself succeeds (U). Check dependents with `EnumDependentServicesW` before any change.

---

## 4. Scheduled tasks: tiered catalog

All paths were observed on the owner's build 26300 PC (OBS, S38) and were Enabled unless stated. **The recommended actions are the researcher's judgment (U/C), not Microsoft statements.** Tasks are matched by exact full path from a hard-coded allow-list. Missing task = "Not present", not an error. A task already Disabled by Microsoft is reported "already disabled" and excluded from Undo-all so Undo never enables something Microsoft ships disabled.

### 4.1 Safe tier (exact paths)
| Task (full path) | Default | Action | What breaks | Admin | Evidence | Src |
|---|---|---|---|---|---|---|
| `\Microsoft\Windows\Customer Experience Improvement Program\Consolidator` | Enabled (OBS) | Disable | CEIP upload only | Admin | OBS; C (Atlas, Qt CI disable it) | S13, S38 |
| `...\Customer Experience Improvement Program\UsbCeip` | Enabled (OBS) | Disable | CEIP USB data only | Admin | OBS; C | S13, S38 |
| `\Microsoft\Windows\Feedback\Siuf\DmClient` | Enabled (OBS) | Disable | Feedback prompt plumbing | Admin | OBS; U | S38 |
| `...\Feedback\Siuf\DmClientOnScenarioDownload` | Enabled (OBS) | Disable | Same | Admin | OBS; U | S38 |
| `\Microsoft\Windows\PerformanceTrace\ShowFeedbackToast` | Enabled (OBS), runs as user | Disable | Feedback toast | Admin | OBS; U | S38 |
| `\Microsoft\Windows\Maps\MapsToastTask` | Enabled (OBS) | Disable | Maps toast notifications | Admin | OBS; U | S38 |
| `\Microsoft\Windows\Maps\MapsUpdateTask` | **Disabled by default** (OBS) | Show "already disabled"; do not touch | n/a | n/a | OBS | S38 |
| `\Microsoft\Windows\Windows Error Reporting\QueueReporting` | Enabled (OBS) | Disable (pair with nothing else) | Crash-report upload | Admin | OBS; U | S38 |
| `\Microsoft\Windows\Device Information\Device`, `...\Device User` | Enabled (OBS) | Disable | Device info upload | Admin | OBS; U | S38 |
| `\Microsoft\Windows\DiskFootprint\Diagnostics` | Enabled (OBS) | Disable | Disk diagnostics data | Admin | OBS; U | S38 |
| `\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector` | **Disabled by default** (OBS) | "already disabled" | n/a | n/a | OBS | S38 |
| `\Microsoft\Windows\Application Experience\MareBackup`, `PcaPatchDbTask`, `StartupAppTask` | Enabled (OBS) | Disable (low impact; StartupAppTask feeds the startup-app notification only) | Minor compat/startup notifications | Admin | OBS; U | S38 |

### 4.2 Careful tier (opt-in, consequence text required)
| Task | Default | Action | What breaks | Evidence | Src |
|---|---|---|---|---|---|
| `\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser` and the `... Exp` variant (name truncated in the notes; re-list unfiltered) | Enabled (OBS) | Offer, **off by default** (open decision 3) | Third-party blog says it may stop assessment data Windows Update uses to offer feature updates. Users report it cannot be disabled in the Task Scheduler UI (search hit only) | OBS; C (blog) | S14, S38 |
| `\Microsoft\Windows\Autochk\Proxy` | Enabled (OBS) | Offer | SQM/telemetry proxy; not chkdsk itself | OBS; U | S38 |
| `\Microsoft\XblGameSave\XblGameSaveTask` (and `XblGameSaveTaskLogon`) | Enabled (OBS) | Offer if Xbox unused | Xbox cloud saves; MS-S says disable on Server | OBS; MS-S | S1, S38 |
| `\Microsoft\Windows\Shell\FamilySafetyMonitor`, `FamilySafetyRefreshTask` | Enabled (OBS) | Offer on non-family PCs | Family Safety | OBS; U | S38 |
| `\Microsoft\Windows\Flighting\FeatureConfig\UsageDataReporting`, `UsageDataReceiver`, `UsageDataFlushing`, `GovernedFeatureUsageProcessing` | Enabled (OBS); `BootstrapUsageDataReporting` Disabled | Offer only the usage-data ones | Feature-rollout telemetry; effect unverified | OBS; U | S38 |
| OneDrive Reporting / Standalone Update / Startup tasks in the root folder, suffixed `-S-1-5-21-<user SID>` | Enabled (OBS), run as user | Match by **prefix + the current user's SID**; offer only if the user disables/uninstalls OneDrive | Standalone Update keeps OneDrive patched | OBS; U | S38 |
| Edge Update tasks (`MicrosoftEdgeUpdateTaskMachineCore/UA`) | **Absent on the owner's PC** (OBS) | Offer only if present; not recommended | Edge stops auto-patching: security risk | OBS; U | S38 |

### 4.3 Never offer (tasks)
Anything under UpdateOrchestrator, WindowsUpdate, WaaSMedic, Windows Defender, SystemRestore, Time Synchronization, Servicing, TPM, BitLocker, Defrag, `DiskFootprint\StorageSense` (a functional feature task), `CloudExperienceHost\CreateObjectTask`, `Shell\CreateObjectTask`, and Flighting `ReconcileConfigs`, `ReconcileFeatures`, `SafeguardsReconciliation`, `OneSettings\RefreshCache` (feature gating; breakage risk is U). (U for the Flighting reasoning; the rest is judgment.)

**Not present on the owner's PC, so unknown:** WindowsAI/Recall/Copilot tasks, ContentDelivery, Clip license tasks, Edge update, Office telemetry, ProgramDataUpdater, KernelCeipTask, BthSQM. The catalog must tolerate all of them being absent.

---

## 5. Startup entries: catalog by class

The tool enumerates; it does not hard-code a list. Classification order: hard-deny, then suggest-disable, then unknown.

### 5.1 Where entries live and how each is toggled
| Entry location | Approval key (toggle target) | Admin | Evidence |
|---|---|---|---|
| `HKCU\...\CurrentVersion\Run` | `HKCU\...\Explorer\StartupApproved\Run` | No | OBS + C (S17) |
| `HKLM\...\CurrentVersion\Run` | `HKLM\...\StartupApproved\Run` | Yes | OBS (SecurityHealth entry seen) |
| `HKLM\SOFTWARE\WOW6432Node\...\Run` | `HKLM\...\StartupApproved\Run32` | Yes | OBS (RadminVPN `03...` seen) |
| HKCU Startup folder (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`) | `HKCU\...\StartupApproved\StartupFolder`, value name = shortcut filename incl. extension | No | OBS (`Citrix Workspace.lnk`) |
| Common Startup folder (`%ProgramData%\...\StartUp`) | `HKLM\...\StartupApproved\StartupFolder` | Yes | U |
| HKCU `Run32` | not verified | | U |
| `RunOnce`, `Policies\Explorer\Run`, logon-trigger tasks, auto-start services, `Winlogon` Shell/Userinit | **No approval layer.** RunOnce deletes itself; Policies Run needs delete (backup first); tasks use put_Enabled; services use SCM; Winlogon: **never touch** | | U |

**Stale approval values are normal** (the owner's PC has `Parsec.App.0` and `BingSvc` approval values with no Run value). Enumerate Run values first, then look up approval, never the reverse.

### 5.2 StartupApproved value format (12 bytes, REG_BINARY)
| Bytes | Meaning | Evidence |
|---|---|---|
| 0-3 (DWORD) | `02` or `06` = enabled; `03` = disabled by user | C (S17) + OBS |
| 4-11 | FILETIME (100 ns since 1601, little-endian) of when disabled; zeros when enabled | C + OBS |

Observed: `Spotify = 03 00 00 00 89 0C BE 33 CA 37 DD 01`; `OneDrive = 02 00...00`; `Docker Desktop = 03 00 00 00 00...00` (disabled with a zero FILETIME, so a zero timestamp is valid); `SecurityHealth = 06 00...`. **The meaning of `06` versus `02` is unverified.** Algorithm: Disable = `03 00 00 00` + now as FILETIME; Enable = `02 00 00 00` + 8 zero bytes; treat odd first byte as disabled, even as enabled, missing value as enabled (bit semantics U). Undo = write back the exact original 12 bytes, or delete the value if it did not exist.

### 5.3 Tier by class (observed names from the owner's PC; judgment, not sourced)
| Class | Examples (OBS) | Tier | Note |
|---|---|---|---|
| Launchers/updaters/chat/cloud | OneDrive, Steam, Spotify, Discord, utweb, CurseForge, EpicGamesLauncher, Battle.net, Docker Desktop, Everything, Citrix Workspace, SamsungRapidApp, `MicrosoftEdgeAutoLaunch_*`, Adobe updaters | **Safe** (suggest disable) | Judgment from common knowledge. User decides; Undo recorded |
| Peripheral/OEM helpers | RazerAppEngine (loses key mappings), Creative.SBCommand, audio managers, touchpad, hotkey/Fn helpers, GPU control panels, Bluetooth, VPN/remote clients (RadminVPN) | **Careful** (flag "may break X") | Could break hardware features |
| Security/management | SecurityHealth, third-party AV/EDR, deviceTRUST, AppControl | **Never-offer** (read-only) | Hard-deny by name and publisher |
| Unknown | anything else | **Careful**, "unknown, you decide" | Show publisher via VERSIONINFO CompanyName and WinVerifyTrust (U) |

"Startup impact" in Task Manager is not a documented API (U): show "Not measured". Resolve command lines with quotes/args/env vars (ExpandEnvironmentStrings) before reading publisher.

---

## 6. Policies and registry tweaks: tiered catalog

**Rules for every row.** (a) Policy keys live under `SOFTWARE\Policies` and trigger "Some settings are managed by your organization" and grey out the matching toggle (inferred from Policy CSP text: S18). (b) HKCU "preference" keys do not lock the UI and the user can flip them back, but may be reset (C). (c) **Home never appears in any Policy CSP table (S18, S19).** On Home, a policy write is best-effort: read back, check the effective state, and never report "applied" on the strength of the write alone. (d) Undo: restore the captured prior value, or delete the value if absent before. (e) Default if missing = "not configured" behaviour. All DWORD unless stated.

Edition column: **P+** = Pro, Enterprise, Education per CSP; **E/E** = Enterprise/Education only (Pro marked unsupported); **All** = per-user preference, no edition claim.

### 6.1 Telemetry and diagnostics
| Setting | Key \ value → "off" | Default | Action / Tier | What breaks | Edition / admin | Evid | Src |
|---|---|---|---|---|---|---|---|
| Diagnostic data level | `HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection` \ `AllowTelemetry` = 1 (Required) on Home/Pro; 0 only Ent/Edu | 1 if missing | Safe | Optional diagnostic UI disabled; failure data to MS not sent when off | P+; 0 = E/E; admin | MS | S18, S21 |
| Feedback notifications | same key \ `DoNotShowFeedbackNotifications` = 1 | 0 | Safe | Feedback prompts | P+; admin | MS | S19 |
| Limit diagnostic logs / dumps | same key \ `LimitDiagnosticLogCollection` = 1, `LimitDumpCollection` = 1 | 0 | Safe | Less data for MS troubleshooting | P+ (21H2+); admin | MS | S18 |
| OneSettings downloads | same key \ `DisableOneSettingsDownloads` = 1 | 0 | Careful | Blocks MS config pushes; effect on rollout unverified | P+; admin | MS (key); effect U | S18 |
| Device name in diagnostics | same key \ `AllowDeviceNameInDiagnosticData` = 0 | 0 | Safe | Nothing | P+; admin | MS | S18 |
| Tailored experiences (policy) | `HKCU\SOFTWARE\Policies\Microsoft\Windows\CloudContent` \ `DisableTailoredExperiencesWithDiagnosticData` = 1 | 0 | Safe | Less personalised tips | P+ (user); no admin | MS | S19 |
| Tailored experiences (pref) | `HKCU\Software\Microsoft\Windows\CurrentVersion\Privacy` \ `TailoredExperiencesWithDiagnosticDataEnabled` = 0 | 1 | Safe | Same | All; no admin | C | S27 |
| Advertising ID (policy) | `HKLM\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo` \ `DisabledByGroupPolicy` = 1 | not configured | Safe | Less personalised ads | P+; admin | MS | S23 |
| Advertising ID (pref) | `HKCU\...\CurrentVersion\AdvertisingInfo` \ `Enabled` = 0 | 1 | Safe | Same | All; no admin | C (+MS mention) | S22 |
| Activity history | `HKLM\Software\Policies\Microsoft\Windows\System` \ `EnableActivityFeed`, `PublishUserActivities`, `UploadUserActivities` = 0 | 1 each | Safe | Timeline-style history | P+; admin | MS | S23 |
| Clipboard history | same key \ `AllowClipboardHistory` = 0 | 1 | Careful | Loses Win+V history | P+; admin | MS | S19 |
| Clipboard cross-device | same key \ `AllowCrossDeviceClipboard` = 0 | 1 | Safe | Cloud clipboard sync | P+; admin | MS | S23 |
| Inking/typing collection | `HKCU\Software\Microsoft\InputPersonalization` \ `RestrictImplicitInkCollection`, `RestrictImplicitTextCollection` = 1 | 0 | Safe | Less personalised suggestions | All; no admin | MS | S22 |
| Inking/typing personalisation | `HKCU\Software\Microsoft\Input\TIPC` \ `Enabled` = 0 | 1 | Safe | Same | All | C | S27 |
| Online speech | `HKLM\Software\Policies\Microsoft\InputPersonalization` \ `AllowInputPersonalization` = 0; pref `HKCU\...\Speech_OneCore\Settings\OnlineSpeechPrivacy` \ `HasAccepted` = 0 | 1 | Careful | Cloud dictation/voice features | P+ / All | MS + C | S22, S23 |
| Feedback frequency | `HKCU\Software\Microsoft\Siuf\Rules` \ `NumberOfSIUFInPeriod` = 0 | auto | Safe | Feedback prompts | All | MS + C | S22 |
| Location (global) | `HKLM\Software\Policies\Microsoft\Windows\LocationAndSensors` \ `DisableLocation` = 1 | 0 | Careful | Maps, weather, Find My Device, time zone auto-set (U) | P+; admin | MS | S18, S22 |
| Apps access location | `...\Windows\AppPrivacy` \ `LetAppsAccessLocation` = 2 | 0 | Careful | Same | P+ | MS | S23 |
| Apps run in background | same key \ `LetAppsRunInBackground` = 2 | 0 | **Careful** | Notifications and sync stop. WinUtil #3401: Snipping Tool overlay broken after "background apps off" | P+ | MS (key); C (breakage) | S23, S36 |
| Voice activation | same key \ `LetAppsActivateWithVoice`, `...AboveLock` = 2 | 0 | Safe | Voice-activated apps | P+ | MS | S22 |
| Find My Device | `HKLM\SOFTWARE\Policies\Microsoft\FindMyDevice` \ `AllowFindMyDevice` = 0 | 1 | Careful | Cannot locate a lost PC | P+ | MS | S19 |
| Settings sync | `...\Windows\SettingSync` \ `DisableSettingSync` = 2, `DisableSettingSyncUserOverride` = 1 | allowed | Careful | Cross-device settings sync | P+ | MS | S22 |
| Windows Error Reporting off | `...\Windows\Windows Error Reporting` \ `Disabled` = 1 | 0 | Careful | Crash reports; Reliability Monitor | P+ | **U** (CSP shows only `DisableWindowsErrorReporting`) | S19 note |
| CEIP | `...\SQMClient\Windows` \ `CEIPEnable` = 0 | not configured | Careful | None known | P+ | **U** | none |
| App compat inventory | `...\Windows\AppCompat` \ `DisableInventory`, `DisablePCA` = 1, `AITEnable` = 0 | 0 | Careful | May degrade compat shims | P+ | **U** | none |
| Delivery Optimization | `...\Windows\DeliveryOptimization` \ `DODownloadMode` = 0 (HTTP only); 99 alternative (conflict 9) | 0 | Careful | P2P update sharing | P+ | MS | S22, S23 |

### 6.2 Ads, suggestions, Start, search, taskbar
| Setting | Key \ value → "off" | Default | Action / Tier | What breaks | Edition / admin | Evid | Src |
|---|---|---|---|---|---|---|---|
| Consumer features | `HKLM\Software\Policies\Microsoft\Windows\CloudContent` \ `DisableWindowsConsumerFeatures` = 1 | 0 | Safe (best-effort on Pro) | Auto-installed promo apps | **E/E**; admin | MS | S19 |
| Cloud-optimised content | same key \ `DisableCloudOptimizedContent` = 1 (HKLM and HKCU: conflict 1) | 0 | Safe (best-effort) | Cloud content suggestions | **E/E** (21H2+) | MS | S19, S22 |
| Consumer account state content | same \ `DisableConsumerAccountStateContent` = 1 | 0 | Safe (best-effort) | Settings 365 promos | E/E | MS + C | S19 |
| Soft landing / tips | same \ `DisableSoftLanding` = 1 | 0 | Safe (best-effort) | Tips | E/E | MS | S19 |
| Third-party suggestions | `HKCU\...\CloudContent` \ `DisableThirdPartySuggestions` = 1 | 0 | Safe | MS's own suggestions stay | **P+** (user) | MS | S19 |
| Spotlight features | `HKCU\...\CloudContent` \ `DisableWindowsSpotlightFeatures`, `DisableWindowsSpotlightOnActionCenter`, `DisableWindowsSpotlightOnSettings`, `DisableWindowsSpotlightWindowsWelcomeExperience` = 1; `ConfigureWindowsSpotlight` = 0; `DisableSpotlightCollectionOnDesktop` = 1 | 0 / 1 | Careful (lock-screen change) | Lock-screen picture rotation | E/E (Pro NOT supported) | MS | S19 |
| ContentDeliveryManager prefs | `HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager`: `SubscribedContent-310093/338387/338388/338389/338393/353694/353696/353698Enabled`, `SystemPaneSuggestionsEnabled`, `SoftLandingEnabled`, `SilentInstalledAppsEnabled`, `RotatingLockScreenOverlayEnabled` = 0 | 1 | Safe | Suggestions/promos | All; no admin | **C** (Win11Debloat; undocumented by MS). `PreInstalledAppsEnabled`, `OemPreInstalledAppsEnabled`, `ContentDeliveryAllowed`, `FeatureManagementEnabled`: **U**, not in the fetched files | S27 |
| Start recommendations (prefs) | `HKCU\...\Explorer\Advanced` \ `Start_IrisRecommendations`, `Start_AccountNotifications`, `Start_TrackProgs` = 0 | 1 | Safe | Recommended items | All | C | S27 |
| Start "Recommended" section | `HKLM`/`HKCU\...\Policies\Microsoft\Windows\Explorer` \ `HideRecommendedSection` = 1 | 0 | **Careful** (conflict 2) | Section may only empty, not vanish | CSP: P+; community: SE/Edu only | MS (key) vs C | S23, S37 |
| Settings suggestions/SCOOBE | `HKCU\...\UserProfileEngagement` \ `ScoobeSystemSettingEnabled` = 0; `Explorer\Advanced` \ `ShowSyncProviderNotifications` = 0; `...\Notifications\Settings\Windows.SystemToast.Suggested` \ `Enabled` = 0 | 1 | Safe | "Finish setting up" nags | All | C | S27 |
| Phone Link in Start | `HKCU\...\Start\Companions\Microsoft.YourPhone_8wekyb3d8bbwe` \ `IsEnabled` = 0 | 1 | Safe | Phone panel in Start | All | C | S27 |
| Widgets (policy) | `HKLM\SOFTWARE\Policies\Microsoft\Dsh` \ `AllowNewsAndInterests` = 0 | 1 | Safe | Whole Widgets experience incl. taskbar content | **P+**; admin | MS | S25 |
| Widgets board / lock screen | same key \ `DisableWidgetsBoard`, `DisableWidgetsOnLockScreen` = 1 | 0 | Careful | Insider-tagged | P+ (Insider Preview) | MS | S25 |
| Widgets button (pref) | `HKCU\...\Explorer\Advanced` \ `TaskbarDa` = 0 | 1 | Safe | Button hidden | All | **U** | none |
| Chat/Teams icon | `...\Windows\Windows Chat` \ `ChatIcon` = 3 | 0 | Careful (deprecated) | Chat icon | P+; value name U | MS (CSP deprecated), name **U** | S19 |
| Cortana | `...\Windows\Windows Search` \ `AllowCortana` = 0 | 1 | Safe | Cortana (largely retired) | P+ | MS | S19 |
| Search highlights | same \ `EnableDynamicContentInWSB` = 0 | 1 | Safe | Search-box doodles/highlights | P+ | MS | S20 |
| Cloud search | same \ `AllowCloudSearch` = 0 | 1 | Careful | Cloud account results | P+ | MS | S20 |
| Web results | same \ `ConnectedSearchUseWeb` = 0 | 1 | Careful | Bing results | **E/E** | MS | S20 |
| Disable web search | same \ `DisableWebSearch` = 1 | 0 | **Careful** | Win11Debloat #70: Bing/Cortana disable broke all search in 2024 | edition per Manage-connections | MS (key); C (breakage) | S22, S36 |
| Search uses location | same \ `AllowSearchToUseLocation` = 0 | 1 | Safe | Location-aware results | P+ | MS | S20 |
| Search box suggestions | `HKCU\SOFTWARE\Policies\Microsoft\Windows\Explorer` \ `DisableSearchBoxSuggestions` = 1 | 0 | Careful | Same search-breakage class as above | P+ (U) | C; `BingSearchEnabled` **U** | S27 |
| Search box mode | `...\Windows Search` \ `ConfigureSearchOnTaskbarMode` | 3 | Safe (cosmetic) | UI only | P+ (24H2+); value name **U** | MS (CSP) | S20 |

### 6.3 Gaming and OneDrive
| Setting | Key \ value | Default | Tier | What breaks | Edition | Evid | Src |
|---|---|---|---|---|---|---|---|
| Game DVR policy | `HKLM\Software\Policies\Microsoft\Windows\GameDVR` \ `AllowGameDVR` = 0 | 1 | Safe | Game Bar clip capture | P+ | MS | S23 |
| Game DVR prefs | `HKCU\System\GameConfigStore` \ `GameDVR_Enabled` = 0; `HKCU\...\GameDVR` \ `AppCaptureEnabled` = 0; `HKCU\SOFTWARE\Microsoft\GameBar` \ `UseNexusForGameBarEnabled` = 0 | 1 | Safe | Game Bar popups/recording | All | C | S27 |
| OneDrive sync | `HKLM\SOFTWARE\Policies\Microsoft\Windows\OneDrive` \ `DisableFileSyncNGSC` = 1 | 0 | Careful | Cloud backup of Documents/Desktop stops | P+ | MS | S22 |

### 6.4 Microsoft Edge (honoured on any Windows edition because Edge reads them)
Key `HKLM\SOFTWARE\Policies\Microsoft\Edge`, all REG_DWORD, Admin. Evidence **MS** (Edge policy pages; example [S26]). Side effect (U): Edge shows "managed by your organization". Restart Edge, no reboot.

| Policy = value | Effect | Tier |
|---|---|---|
| `HubsSidebarEnabled` = 0 (v99) | Sidebar never shown | Safe |
| `EdgeShoppingAssistantEnabled` = 0 (87); `ShowMicrosoftRewards` = 0 (88) | Shopping/Rewards nags | Safe |
| `StartupBoostEnabled` = 0 (88); `BackgroundModeEnabled` = 0 (77) | No pre-launch/background Edge. Slower first Edge open | Careful |
| `NewTabPageContentEnabled` = 0 (91); `NewTabPageQuickLinksEnabled` = 0 (91); `ShowRecommendationsEnabled` = 0 (89) | Cleaner new tab | Safe |
| `PersonalizationReportingEnabled` = 0 (80); `SpotlightExperiencesAndRecommendationsEnabled` = 0 (86); `UserFeedbackAllowed` = 0 (77); `DiagnosticData` = 0 | Edge telemetry/personalisation | Safe |
| `HideFirstRunExperience` = **1** (80) | Skip first-run (Win11Debloat's 0 looks like a bug; conflict 10) | Safe |
| `DefaultBrowserSettingEnabled` = 0; `EdgeWorkspacesEnabled` = 0 (106); `EdgeFollowEnabled` = 0 | Nags/features | Safe |
| `Microsoft365CopilotChatIconEnabled` = 0 (139); `EdgeEntraCopilotPageContext` = 0 (130); `CopilotPageContext` = 0 (124, Entra only) | Copilot toolbar/page context. As of Edge 141 only the first controls the toolbar icon | Safe |
| `MetricsReportingEnabled` | Learn says no longer supported: **do not write** | n/a |

### 6.5 Never offer (policies and registry)
| Item | Reason |
|---|---|
| Edge update-disabling values (`EdgeUpdate` `UpdateDefault=0` etc.) | Manage-connections page itself warns: security risk (S22) |
| `DisableRootAutoUpdate`, Windows Update disabling, Defender/real-time protection, Defender sample-submission toggles (WinUtil sets 2) | Page warns against; Tamper Protection blocks Defender registry changes and shows as malware behaviour (S7, S22) |
| Windows Update deferral keys (`DeferFeatureUpdates`, `NoAutoRebootWithLoggedOnUsers`, `ExcludeWUDriversInQualityUpdate`) | Not researched (section 8). Do not offer until verified |
| WinUtil-style irreversible actions: `Disable-BitLocker`, `DISM /ResetBase`, ACL-deny on Store `store.db`, Edge removal, hibernate off | No clean undo, or hide key features (S9). Excluded per section 7 principles |
| Mitigations/VBS/UAC/IPv6/Teredo changes | Security regressions (Atlas lesson) or game breakage (Teredo) |

---

## 7. New Windows 11 features: AI, Copilot, Recall, Widgets

Build 26300 is the Experimental channel (renamed Dev), "based on Windows 11, version 25H2 via an enablement package"; Pureinfotech calls it 26H2, which Microsoft's own notes do not ([S30](https://learn.microsoft.com/en-us/windows-insider/release-notes/experimental/preview-build-26300-8687)). Release 26300.8687 adds no new AI or Copilot features. Anything we target here is subject to Controlled Feature Rollout, so **a key present on one 26300 PC may be absent on another**. Microsoft announced in March 2026 it will reduce "unnecessary Copilot entry points", so this table will age ([thejournal.com](https://thejournal.com/articles/2026/03/30/microsoft-scales-back-copilot-integrations-in-windows-11.aspx)).

All `WindowsAI` policy values live at `HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsAI` unless stated (S24).

| Feature / switch | Value → off | Default | Action / Tier | What breaks | Edition / OS | Evid | Src |
|---|---|---|---|---|---|---|---|
| Recall availability | `AllowRecallEnablement` = 0 | 1 | **Careful** (strongest documented Recall off; removes bits, **deletes existing snapshots, not undoable**; restart needed) | Recall gone until set back | Pro, Ent, Edu; 24H2 KB5055627 (26100.3915)+ | MS | S24 |
| Recall snapshot saving | `DisableAIDataAnalysis` = 1 (HKLM or HKCU) | 0 | Careful (softer; component remains; deletes saved snapshots) | No new snapshots | Pro+; 24H2 KB5055627+ | MS | S24 |
| Recall as optional feature | `Dism /online /Disable-Feature /FeatureName:"Recall"` | enabled where present | **Not via DISM in v1**: the tool is native-API only | n/a | n/a | C (WinUtil) | [WinUtil](https://winutil.christitus.com/dev/tweaks/essential-tweaks/disablerecall) |
| Recall detail policies (deny lists, retention, storage cap, DLP provider) | `SetDenyAppListForRecall`, `SetMaximumStorage...` etc. | n/a | Not offered | Enterprise controls | **E/E** (some Insider) | MS | S24 |
| Click to Do | `DisableClickToDo` = 1 | 0 | Careful | Click to Do entry points | Pro+; **OS column "Windows Insider Preview"** - not documented as GA on 25H2 | MS (tagged Insider) | S24 |
| Settings agent | `DisableSettingsAgent` = 1 | 0 | Careful | Settings search limited to indexed + semantic search | **E/E**; Insider Preview | MS | S24 |
| Paint AI | `HKLM\Software\Microsoft\Windows\CurrentVersion\Policies\Paint` \ `DisableCocreator`, `DisableGenerativeFill`, `DisableImageCreator` = 1 | 0 | Safe | Paint generative features | Pro+; 22H2 (22621.4870)+ / 24H2 (26100.3360)+ | MS | S24 |
| Windows Copilot (legacy) | `HKCU\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot` \ `TurnOffWindowsCopilot` = 1 | 0 | Careful: **deprecated; does not control the new Copilot app** | Little on 25H2 | Pro+ (user) | MS | S24 |
| Copilot app removal | `RemoveMicrosoftCopilotApp` | n/a | **Never offer in v1** (conflict 4) | n/a | Ent/Edu per table | MS (contradictory) | S24, S29 |
| Copilot hardware key | `HKCU\SOFTWARE\Policies\Microsoft\Windows\CopilotKey` \ `SetCopilotHardwareKey` (AUMID string) | n/a | Optional remap; only if the PC has the key | Key opens another app | Pro (22H2 KB5044380+) | MS | S24 |
| Agent connectors / MCP | `ConfigureAgentConnectors` = 2, `AgentConnectorAccessPolicy`, `AgentConsentDuration` | 0 | Not offered | Enterprise agent control | E/E; Insider | MS | S24 |
| Experimental agentic features / Agent workspace | Settings > System > AI components; no documented registry value | Off by default | **Display-only** ("check it is off"). No Microsoft source found; forum-grade (admin-only toggle; creates agent accounts) | n/a | Dev/Beta 26220.7262 | C | [Pureinfotech](https://pureinfotech.com/enable-agentic-ai-features-windows-11/) |
| WSAIFabricSvc | see services 3.2 | present on Copilot+ | Careful, Manual only | Recall, Click to Do, Live Captions | | C | S12 |
| Widgets | `AllowNewsAndInterests` = 0 | 1 | Safe | Widgets | Pro+ | MS | S25 |
| Phone Link in Start | see 6.2 | | Safe | | | C | S27 |
| Store apps removal (25H2 policy) | `RemoveDefaultMicrosoftStorePackages` | off | **Never offer** (Ent/Edu, Entra/hybrid-joined only per MS; community registry layout; Pro claim conflicts) | n/a | Ent/Edu | MS vs C | S28 |
| Edge uninstall | EEA-only path, 2023-era guide | | Never offer | Edge + WebView2 dependency | | C (old) | [Neowin](https://www.neowin.net/guides/how-to-remove-microsoft-edge-from-windows-11-in-the-latest-eea-compliant-update) |

**Practical Pro conclusion:** the one clean, documented Recall switch that works on Pro is `AllowRecallEnablement`. On a non-Copilot+ PC like the owner's, Recall, Click to Do and WSAIFabricSvc likely do not exist (the notes found no WindowsAI/Recall/Copilot tasks here), so the tool must detect presence and hide inapplicable rows rather than write dead policy values.

---

## 8. Reality check: performance, drift, myths

| Claim | What the evidence shows | Src |
|---|---|---|
| "Debloat makes Windows fast" | A fresh 25H2 VM test saved **0.4-0.6 GB disk** and took RAM from **3.2 GB to ~2.6-2.7 GB**; the original article was not identified (moderate-low reliability). "Disabling twenty services will magically increase FPS is simply false on modern systems" (blog) | [daily.dev summary](https://app.daily.dev/posts/windows-11-debloat-tools-promise-speed-but-here-s-what-actually-happens-0mqcfiajx), [Windows Tweaks](https://www.windows-tweaks.info/en/?p=16704) |
| SysMain off helps | 16 GB PC: little to no change. 4 GB VM: restart 42.83 s to 35.18 s, but Clipchamp launch 3.14 s to 8.48 s; cached RAM down ~100 MB. **Keep SysMain on** | S10 |
| SysMain on NVMe | Forum users report no measurable difference; SSD-wear claims are unsourced | S11 |
| VBS off | "No performance gains" (Titus on Atlas) | [christitus.com](https://christitus.com/atlasos-review) |
| Fast startup off | Win11Debloat itself warns it "may slightly increase boot times" | S27 |
| Settings survive updates | **Anecdotal only.** Windows 10 Nov 2015 update re-enabled settings changed by privacy apps (S35); users report DiagTrack reset to Automatic and removed apps returning. Nothing specific for ContentDeliveryManager, Appraiser or 25H2. One Q&A: `SilentInstalledAppsEnabled=0` in the Default hive alone did not stop promo apps for a new user | S35 |
| Failure rate | Breakage reports cluster in app removal, servicing/Update tampering, background-apps-off, search policies; all sampled issues were closed (counts not representative) | S36 |

**Design response to drift:** policy keys (managed-owned) should last better than HKCU prefs (U); re-scan at launch, show Applied / Not applied / Differs, one-click Re-apply; no background watcher (it would need persistence and looks like malware). New user profiles may also need the Default hive (`C:\Users\Default\NTUSER.DAT`, load under a temp HKU key) - out of scope for v1.

---

## 9. Native API design notes (Rust, `windows` crate)

Exact crate signatures were **not verified** in the research (check docs.rs `windows::Win32::System::Services` / `TaskScheduler`). Constants below are from Microsoft docs unless marked U.

### 9.1 Service Control Manager
| Step | Detail |
|---|---|
| Open | `OpenSCManagerW(SC_MANAGER_CONNECT \| ENUMERATE_SERVICE)`; `OpenServiceW(SERVICE_QUERY_CONFIG \| SERVICE_CHANGE_CONFIG [\| SERVICE_QUERY_STATUS \| SERVICE_STOP])`. Read-only inspection should work non-elevated (U); CHANGE_CONFIG on system services needs elevation |
| Snapshot (for undo) | `QueryServiceConfigW` twice (first call returns `ERROR_INSUFFICIENT_BUFFER` for size; `QUERY_SERVICE_CONFIGW` is variable length) → `dwStartType`; `QueryServiceConfig2W(SERVICE_CONFIG_DELAYED_AUTO_START_INFO)` → delayed flag; `QueryServiceStatusEx` → was_running |
| Change | `ChangeServiceConfigW(h, SERVICE_NO_CHANGE, start, SERVICE_NO_CHANGE, ...)` with AUTO=2, DEMAND=3, DISABLED=4. Pass `SERVICE_NO_CHANGE` (0xFFFFFFFF) for everything else ([S5](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-changeserviceconfigw)). Changes to a running service do not take effect until it stops (except display name) |
| Delayed auto-start | Separate flag via `ChangeServiceConfig2W(SERVICE_CONFIG_DELAYED_AUTO_START_INFO)`; **ignored unless AUTO_START**; takes effect next boot ([S3](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_delayed_auto_start_info)). Setting Auto to Manual does not clear it. **Restore:** set start AUTO first, then set the flag TRUE; only re-apply the flag if the original type was AUTO |
| Trigger-start | Separate config (`SERVICE_CONFIG_TRIGGER_INFO`); Start type is untouched. The tool must **not** edit triggers, SID type or failure actions. U: Manual does not stop triggers, Disabled does (SCM returns `ERROR_SERVICE_DISABLED`) |
| Per-user services | Open the **template by exact name (no `_LUID` suffix)** and set start. Instances (`Name_<LUID>`) are ephemeral, created at sign-in from the template and deleted at sign-out; a Disabled template yields a stopped, disabled instance; `UserServiceFlags=0` under the template key prevents creation ([S2](https://learn.microsoft.com/en-us/windows/application-management/per-user-services-in-windows)). Four templates (PimIndexMaintenanceSvc, UnistoreSvc, UserDataSvc, WpnUserService) cannot be managed via security template, the rest can. Enumerate with `EnumServicesStatusExW` using the user-service type flags (constant names/values **U**; check the SDK header). Restore delayed flag for cbdhsvc/OneSyncSvc. Registry write is a fallback if the API refuses a template (U) |
| Launch-protected detection | `QueryServiceConfig2W(SERVICE_CONFIG_LAUNCH_PROTECTED)` (level numeric value **U**: 12) → NONE=0, WINDOWS=1, WINDOWS_LIGHT=2, ANTIMALWARE_LIGHT=3. Once protected, unprotected processes cannot call ChangeServiceConfig, ChangeServiceConfig2, ControlService, DeleteService, SetServiceObjectSecurity ([S6](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_launch_protected_info)) |
| "Locked, skip" | If level != NONE, or `OpenServiceW(CHANGE_CONFIG)` returns `ERROR_ACCESS_DENIED` even when elevated (DACL/TrustedInstaller): show a lock icon "Protected by Windows" and **never retry, take ownership or impersonate TrustedInstaller**. Which services deny admins on 25H2 is unknown |
| Stop after disable | `EnumDependentServicesW(SERVICE_ACTIVE)` first; then `ControlService(STOP)`. Never stop shared svchost-hosted critical services |
| Verify | After every change re-run `QueryServiceConfigW`; compare to requested; also compare to the delayed flag. Treat mismatch as failure |

### 9.2 Task Scheduler 2.0 (COM)
1. `CoInitializeEx(COINIT_MULTITHREADED)`. If SRSetRestorePointW is also used, `CoInitializeSecurity` is required once per process and must happen before COM use that depends on it.
2. `CoCreateInstance(CLSID_TaskScheduler)` → `ITaskService`; `Connect` with four empty VARIANTs (local).
3. `GetFolder(path)` then `GetTask(name)` → `IRegisteredTask`; read `Enabled()` and `State()` (DISABLED=1, QUEUED=2, READY=3, RUNNING=4).
4. `put_Enabled(VARIANT_BOOL)`: -1 true, 0 false; no error for other values ([S15](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-iregisteredtask-put_enabled)). Read `Enabled()` back.
5. **Missing task = not present.** HRESULT `0x80070002` (file not found) is "Not present", not an error (a missing folder presumably returns the same, U).
6. **Permissions:** Administrators and SYSTEM can read, update, delete and run any task; Users/Local Service/Network Service only tasks they created; the task DACL can override ([S16](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks)). `E_ACCESSDENIED` (0x80070005) → "Protected, skipped"; no ownership changes; no registry TaskCache fallback (service not notified).
7. **Exact-path allow-list.** Hard-coded full paths. Only the OneDrive tasks use prefix matching, restricted to the current user's SID suffix. Never enumerate-and-match patterns. Use `TASK_ENUM_HIDDEN` (1) only for listing.
8. Registry TaskCache (`...\Schedule\TaskCache\Tree\<path>`) is read-only for listing; the Enabled flag is in the Task XML, not a clean value (U).

### 9.3 StartupApproved
See section 5. Registry APIs: `RegOpenKeyExW` / `RegQueryValueExW` / `RegSetValueExW(REG_BINARY, 12 bytes)` / `RegDeleteValueW`. Always back up the original 12 bytes (or absence) in memory. HKLM approval keys need admin.

### 9.4 Registry write, read-back and effective state
1. Capture: type + data, or "absent". 2. Create key if needed (`RegCreateKeyExW`, correct view for WOW6432). 3. Write. 4. **Read back** and compare type and data. 5. **Effective-state check** (policy value present is not proof it took effect): examples: Settings toggle greyed/disabled; for Edge, the `edge://policy` page (manual); for Start Recommended, whether the section is actually hidden. Where no programmatic effective-state check exists, label the row "Written, effect not verifiable". 6. Report Home/unmanaged differently: "written, Windows may ignore on Home".
Tamper Protection and MDM can make writes appear to succeed but be reverted or blocked ([S7](https://learn.microsoft.com/en-us/defender-endpoint/tamper-protection-overview)); the read-back catches the former only if done after a short delay and on next launch.

### 9.5 Undo representation
```
enum UndoRecord {
  Reg   { hive, key, name, prior: Option<(type, bytes)> },
  Svc   { name, start_type, delayed: bool, was_running: bool },
  Task  { path, was_enabled: bool },        // only if the app changed it
  Startup { approval_key, name, prior: Option<[u8;12]> },
}
```
Clipboard export uses `reg add/delete`, `sc config <svc> start= auto|demand|disabled|delayed-auto`, `schtasks /change /tn "<path>" /enable`, and `reg add ... /t REG_BINARY /d <24 hex>`. These are text the user runs; the tool itself does not shell out and uses no PowerShell.

---

## 10. Safety machinery: restore point, edition and management detection, signing

### 10.1 Restore point (SRSetRestorePointW)
Design from the Microsoft docs ([S31](https://learn.microsoft.com/en-us/windows/win32/api/srrestoreptapi/nf-srrestoreptapi-srsetrestorepointw)):
| Item | Detail |
|---|---|
| Load | `LoadLibrary("SrClient.dll")` + `GetProcAddress("SRSetRestorePointW")` (Microsoft says do not link at load time) |
| COM | `CoInitializeEx` and `CoInitializeSecurity` so NetworkService/LocalService/System can call back; Microsoft says this is necessary |
| Event/type | `BEGIN_SYSTEM_CHANGE` (100) → apply → `END_SYSTEM_CHANGE` (101) with the same sequence number; type `MODIFY_SETTINGS` (12). To discard: `SRRemoveRestorePoint` or END with `CANCELLED_OPERATION` (13) |
| **24-hour trap** | If `HKLM\Software\Microsoft\Windows NT\CurrentVersion\SystemRestore\SystemRestorePointCreationFrequency` does not exist (default), Windows **skips creating a point if one was made in the last 24 hours, yet returns TRUE with the old sequence number and `nStatus = ERROR_SUCCESS`**. 0 = never skip; N = skip if a point exists within N minutes |
| Procedure | Read the old frequency value; set to 0 (admin); call BEGIN; **verify**: new `llSequenceNumber` differs from the previous latest point; restore the old value (or delete it if it did not exist); tell the user it did this. Note: one forum report says the value did not persist across restarts (unconfirmed) |
| Failure | Fails if System Restore is disabled or in safe mode. FALSE = "no safety net"; **do not enable System Restore silently**; ask. Failure codes (1058, 10, 5) are U |
| Default state | Disputed: some sources say off by default on Win11, ElevenForum says on. Unresolved |
| Scope | Docs: "full snapshot of the registry and other system databases", not user files. **Registry-backed items (policies, service config) are plausibly covered, but the notes could not confirm scheduled-task coverage or AppX; forum sources conflict.** Do NOT promise task or app recovery. Removed apps are not reliably restored; Win11Debloat's own registry backup "does NOT restore removed apps" (S27) |
| Caveat | Win11Debloat #576: users lost restore points. Never rely on the restore point as the only undo |
| Description | Tool name and time; MAX_DESC_W length **U** (recall 256) |
| Not to confuse | "Point-in-time restore" is a separate off-by-default feature ([Windows IT Pro blog](https://techcommunity.microsoft.com/blog/windows-itpro-blog/point-in-time-restore-for-windows-11-is-now-generally-available/4508101)) |

### 10.2 Edition and management detection
| Check | API | Notes |
|---|---|---|
| Edition | `GetProductInfo` (pass highest OS version, e.g. 10.0.0.0). PRODUCT_CORE=0x65 (Home), CORE_SINGLELANGUAGE=0x64, CORE_N=0x62, PROFESSIONAL=0x30, PRO_WORKSTATION=0xA1, ENTERPRISE=0x4, EDUCATION=0x79, ENTERPRISE_S=0x7D, IOTENTERPRISE=0xBC | Prefer over ProductName strings (S32). `EditionID` registry value is U. "ProductName wrong on 26xxx" is unsourced. S mode (blocks unsigned exe) not sourced |
| On-prem domain | `NetGetJoinInformation(NULL)` → `NetSetupDomainName`; free with `NetApiBufferFree`; no special privileges | **Does not detect Entra ID join** ([S32](https://learn.microsoft.com/en-us/windows/win32/api/lmjoin/nf-lmjoin-netgetjoininformation)); `NetGetAadJoinInformation` not researched |
| MDM/Intune | `HKLM\SOFTWARE\Microsoft\Enrollments` subkeys (UPN, ProviderID) | Community sources only; "MS DM Server" provider string unconfirmed |
| Policy applied | RSOP / GPO history key | Not researched |

Recommended behaviour: domain, Entra or MDM detected → **read-only mode with a banner** (policy refresh will revert HKLM edits; Intune can lock Tamper Protection). Home → catalog stays, but policy rows show "Windows Home ignores most policies; best-effort" and no row claims "Applied" without effective-state proof.

### 10.3 Antivirus, SmartScreen, Smart App Control, signing
**Risk is real.** Win11Debloat's 2026.06.14 release existed to address Defender and Bitdefender false positives (search snippet; the page returned 403) ([NewReleases](https://newreleases.io/project/github/Raphire/Win11Debloat/release/2026.06.14)); Flyby11 was flagged PUA then HackTool; ExplorerPatcher was flagged `HackTool:Win64/ExplorerPatcher!MTB` ([Neowin](https://www.neowin.net/news/microsoft-blocks-free-windows-11-24h2-system-requirements-bypass-app-as-potential-malware/)). Rust executables get ML false positives often (forum thread). An unsigned, low-prevalence exe that writes HKLM, changes services and relaunches elevated matches the flagged behaviour profile (inference). **Smart App Control in enforcement mode blocks "unknown, unsigned code"** ([S33](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview)); the user cannot override it without turning SAC off (permanence of that is U).

| Option | Facts from the notes | Caveats |
|---|---|---|
| Azure Artifact Signing (formerly Trusted Signing) | About $9.99/month; organisations in USA, Canada, EU, UK; **individuals only USA and Canada**; SmartScreen reputation still builds over time, no instant trust ([S33](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)) | Owner appears to be Norway-based (Norwegian locale on the PC, `.no` address). Norway is EEA, **not EU**; eligibility is unverified and probably excluded for an individual. A Q&A report of paused individual onboarding is unconfirmed |
| OV certificate | $150-300/yr; HSM/token required since June 2023 | EV no longer bypasses SmartScreen since 2024 |
| SignPath Foundation (free for OSS) | Requires OSI licence, no malware/PUA, actively maintained, documented functionality, public repo, fully automated build with origin verification, code-signing policy on the home page. **Certificate is issued to SignPath Foundation (they are the publisher)** and can be revoked on policy violation ([S34](https://signpath.org/terms)) | Requires toolboks to be **open source with public CI**. A registry/service tweaker may get extra scrutiny under the "no PUA" rule (inference). Reputation timeline "several weeks" is from a search summary |
| MSIX / Store | Avoids SmartScreen warnings | Not applicable to a plain single exe |

Mitigations that cost nothing: no packers (UPX), no string obfuscation, no self-update download-and-exec, no PowerShell `-EncodedCommand`, MSVC target (single anecdote), publish SHA-256, build in public CI, submit each release at the Microsoft Security Intelligence portal (aka.ms/wdsi; up to 100 hashes per submission; behaviour-only detections need `MpSupportFiles.cab`) ([S34](https://learn.microsoft.com/en-us/defender-endpoint/defender-endpoint-false-positives-negatives)).

---

## 11. What the research could NOT verify

| Gap | Impact | How to close |
|---|---|---|
| Service default start types, dependencies, dependents on Win11 25H2/26300 (Home vs Pro) | Every "typical default" in section 3 is U; only per-user template defaults are MS | Query the PC and a Home VM at runtime; never hard-code defaults for undo |
| Which services ACL-deny admins or are launch-protected on this build | "Locked, skip" list unknown | Attempt `OpenServiceW(CHANGE_CONFIG)` in a read-only probe (open only; do not change) and `QueryServiceConfig2W` level 12 |
| Black Viper tables, Raymond Chen/Windows-team statements on SysMain/WSearch; no rigorous WSearch or SSD-wear benchmark | Performance claims rest on one benchmark | Treat as "leave on" |
| Fax, RetailDemo, WalletService, SEMgrSvc, Wecsvc, Xbox service details | Fax/RetailDemo/Wallet/SEMgr/Wecsvc are U | Verify each exists and its default |
| Tools not read from source: privacy.sexy, Sophia, Windows10Debloater, Tron, BloatyNosy, Optimizer, Winhance, Revision, NTLite; WinUtil presets (`config/preset.json`) | Consensus matrix covers only Win11Debloat, WinUtil, O&O (FAQ via summary), Atlas | Read `config/preset.json` and privacy.sexy collections |
| Start/ContentDeliveryManager: `PreInstalledAppsEnabled`, `OemPreInstalledAppsEnabled`, `ContentDeliveryAllowed`, `FeatureManagementEnabled`; `TaskbarDa`, `BingSearchEnabled`, `ChatIcon` value name, `ConfigureSearchOnTaskbarMode` value name | Rows marked U | Check on the PC and in ADMX |
| WER `Disabled`, CEIP `CEIPEnable`, AppCompat `DisableInventory/DisablePCA/AITEnable` | U; AppCompat may degrade shims | Check admx.help; test |
| Windows Update policy keys (`NoAutoRebootWithLoggedOnUsers`, `DeferFeatureUpdates`, `ExcludeWUDriversInQualityUpdate`) | Section 8 of policies was not researched | Skip in v1 |
| `DisableDiagnosticDataViewer` and `ConfigureTelemetryOptInSettingsUx` registry paths | Not captured | Skip or look up |
| Whether Pro/Home honour Ent/Edu-only policies; "Managed by your organization" banner per policy | Core to Pro behaviour | Test on this Pro PC; no Home PC was tested |
| Whether feature or cumulative updates reset any of our settings or tasks (Appraiser, DiagTrack, ContentDeliveryManager, AllowRecallEnablement) on 24H2/25H2 | Drift is anecdotal | Before/after diff across one update |
| System Restore default state on Win11; failure codes; MAX_DESC_W; whether tasks or AppX are covered | Restore-point promise limited to registry | Test: create point, change, restore, diff |
| Elevation patterns (manifest, `runas`, standard-user failure), UAC UX, S mode, Entra-join detection | Not sourced | Prototype |
| Hardware/Insider: whether "26H2" is an official name; remaining 26300 flights (7877, 8346, 8376) not read in full | Unknown new bloat | Read the notes for the build the PC is on |
| StartupApproved `06` vs `02`; HKCU `Run32`; Task Manager startup-impact method | Minor | Test |
| Edge removal vs WebView2 breakage, Copilot-removal side effects, "removed apps return after update" counts | Why app removal stays out of scope | Separate research |
| Exact `Get-AppxPackage` names for the Copilot app; Notepad/Photos/Explorer AI policy values | AI app-surface cannot be fully offered | Separate research |
| Microsoft Compatibility Appraiser: "Exp" is a separate task? (output truncated) | Catalog row ambiguous | Re-run `schtasks /query /fo csv /v` unfiltered |
| Antivirus detection specifics for Rust/our patterns | False-positive risk is not quantified | Submit a signed build to wdsi and test several AVs |

### Verify on this PC before shipping (Windows 11 Pro, build 26300, reported as 26H2/Experimental, based on 25H2)
Read-only checks; run when the PC is free, nothing here should change state.
- [ ] `winver` / `reg query` for DisplayVersion, EditionID, CurrentBuild, UBR; compare with `GetProductInfo`.
- [ ] Is the PC domain-joined, Entra-joined, or MDM-enrolled? (`dsregcmd /status`, Enrollments key.)
- [ ] For every service in sections 3.1-3.2: `sc qc <name>` for start type, `sc qtriggerinfo <name>`, dependents; record whether it exists.
- [ ] List per-user service templates in `HKLM\SYSTEM\CurrentControlSet\Services` and their `Start`.
- [ ] Probe launch-protection (level 12) on WinDefend, Sense, WdNisSvc, SecurityHealthService.
- [ ] `schtasks /query /fo csv /v` **unfiltered**; confirm each allow-list path and whether a separate `Microsoft Compatibility Appraiser` exists.
- [ ] `HKLM\...\Policies\Microsoft\Windows\{DataCollection,CloudContent,WindowsAI,AdvertisingInfo}` current contents (so we know pre-existing values).
- [ ] Which ContentDeliveryManager values exist and their data.
- [ ] Does the Settings UI grey the matching toggle after a policy write on Pro (test one reversible policy, then undo)?
- [ ] `HKLM\Software\Microsoft\Windows NT\CurrentVersion\SystemRestore` exists? Is System Protection on? Does `SystemRestorePointCreationFrequency` exist?
- [ ] Smart App Control state (Settings > Windows Security > App & browser control).
- [ ] Does an unsigned release build run, and is it flagged by Defender or SmartScreen?
- [ ] Does WSAIFabricSvc exist on this PC (not Copilot+, so probably not)?

---

## 12. Proposed roadmap: phases 4-6

| Phase | Build | Why now | Exit criteria |
|---|---|---|---|
| **4 - Startup + foundation** | Startup entries (Run/Run32/Startup folders, StartupApproved toggle, publisher column, hard-deny list). Shared foundation: `UndoRecord`, per-row/all undo, clipboard export, "Copy undo steps", launch-time drift view (Applied / Not applied / Differs), edition and managed-PC detection, elevation strategy decided | Lowest risk; HKCU toggles need no admin; mechanism confirmed on the owner's PC; builds the engine all later phases reuse | Toggle and undo verified on HKCU and HKLM entries; read-back matches; read-only banner works on a domain/MDM test |
| **5 - Tasks + policies + restore point** | Scheduled-task allow-list (Safe tier, then Careful). Policy expansion (6.1-6.4 Safe rows, AI Safe/Careful rows). Restore point with 24-hour handling and sequence verification, created before the first admin apply | Tasks and registry writes share the verified COM/registry patterns; restore point belongs at the first batch of admin changes | Restore-point creation proven to produce a new sequence number; policy write/read-back/effective-state checks pass on Pro; Home banner works |
| **6 - Services + presets** | Services Safe tier (Disabled), Careful tier (Manual only), per-user templates, locked-service handling, dependents check. Presets: "Privacy basics" (Safe only), "Gaming PC", "Minimal". Re-apply buttons | Highest blast radius and weakest evidence; do it last, after all service defaults are checked on live PCs | Every shipped service row has a live-verified default; no Careful item is ever pre-selected; presets contain Safe items only |
| After 6 | Code signing and AV submission pipeline; maybe AppX/Edge scope (separate research) | Needed before wide distribution; unrelated to features | Signed release; wdsi submission done |

Build first within phase 5: tasks before policies, because tasks have an allow-list of exact paths and one COM sequence, whereas policies multiply edition caveats.

---

## 13. Open decisions for the owner

| # | Question | Recommended default |
|---|---|---|
| 1 | Show **Careful** items at all? | Yes, collapsed under "Advanced", never pre-selected, each with a one-line consequence, and excluded from presets |
| 2 | For services, offer **Disabled** or only **Manual**? | Safe tier: Disabled (needed to stop triggers). Careful tier: Manual only. Never offer Disabled on a service with active dependents |
| 3 | Include the **Compatibility Appraiser** task? | Include as Careful, off by default, with the warning that a third-party blog links it to feature-update offers |
| 4 | Include **AI/Copilot/Recall** switches? | Yes: Paint AI and Widgets (Safe); `AllowRecallEnablement` and `DisableClickToDo` as Careful with "deletes snapshots, not undoable" and "Insider-tagged" text; hide rows when the feature is not present. No Copilot app removal |
| 5 | Add **presets**? | Yes, in Phase 6: one "Privacy basics" preset (Safe, non-service). No "max debloat" preset |
| 6 | **Code signing**? | Treat signing as required before sharing outside the owner's PC. Investigate SignPath Foundation first (needs open source), then OV. Do not budget on Azure Artifact Signing until Norway eligibility is checked |
| 7 | **Home edition** behaviour? | Show everything, but badge rows "Windows Home may ignore this", forbid "Applied" status without an effective-state check, and disable E/E-only rows on Pro with an explanation |
| 8 | **Refuse on domain/MDM PCs**, and how to elevate? | Read-only mode with a banner (not a hard refuse). Elevate once, before the first Apply, and keep one process owning all undo state; do not relaunch mid-session |
| 9 | Should the tool offer to **enable System Restore** if it is off? | No. Explain and let the user decide; Apply stays available with a "no restore point" warning |

---

## Conclusion

The research reframes what a "safe debloater" is. The honest catalog is small, because Microsoft documents policies, not service safety, and only per-user template defaults and SCM/Task Scheduler mechanics are verified. The strongest differentiator is therefore not breadth but integrity: read-back plus effective-state truthfulness, a live-state view that survives restarts without writing anything to disk, and a refusal to claim success on Home or managed PCs. The no-disk constraint is a product choice with a plain cost (no cross-session undo) that the UI must state, and the clipboard export and launch-time re-read are the mitigations.

The biggest risks are outside the catalog: an unsigned exe may be blocked outright by Smart App Control or ML antivirus, signing eligibility from Norway is unverified, and several load-bearing facts (service defaults on 26300, Pro behaviour for Ent/Edu-only policies, restore-point coverage of tasks) must be checked on the owner's PC before any row ships. Build startup and the undo foundation first, validate on real machines, and let the evidence, not tool precedent, decide what graduates from Careful to Safe.
