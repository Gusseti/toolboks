# Toolboks testing findings

This is the permanent testing record. Each testing request has its own numbered report below.

# Test-1: 🔵 [INFO] Toolboks backend security testing

On 12 November 2026, I ran a **Light test** of Toolboks 0.12.3 on staging commit `327f1388e04ebec6f324b05b9dec91b8dca9381f`.

## What I tested

In this test, I checked add-on validation, protection against impersonating built-in add-ons, update validation, administrator-related input checks, registry recovery safeguards, and HTTPS address parsing. I ran these checks on Windows using the local stable Rust GNU toolchain.

**All 22 unit tests passed.** I found no confirmed security defect within this coverage. The protections below worked for the inputs and conditions exercised by their tests; this does not establish that the entire backend is secure.

The following sections explain each result. They describe the original testing session, rewritten for readability; no additional tests were run for this presentation update.


## Check 1: 🔵 [INFO] SDK version and identifier syntax

**PASS** — I checked the rules for add-on versions and identifiers. Valid versions and identifiers were accepted, while the invalid forms covered by the test were rejected.

Evidence: `tests::version_and_id_rules` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:434`.

## Check 2: 🔵 [INFO] SDK package size and widget boundary

**PASS** — I checked oversized packages and unsupported widgets. Packages larger than 2 MiB and widgets the host does not recognize were rejected.

Evidence: `tests::oversized_packages_and_unrecognized_widgets_are_rejected` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:427`.

## Check 3: 🔵 [INFO] Valid SDK package baseline

**PASS** — I checked that a valid starter add-on still works with a compatible host. The bundled starter package parsed successfully, providing a baseline alongside the rejection tests.

Evidence: `tests::starter_is_compatible` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:382`.

## Check 4: 🔵 [INFO] Declarative package executable/spoofed fields

**PASS** — I checked whether a declarative package could claim verified status or include unknown executable fields. The test rejected both forms of invalid package data.

Evidence: `tests::rejects_spoofed_verification_and_unknown_code` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:386`.

## Check 5: 🔵 [INFO] SDK namespace collisions

**PASS** — I checked for duplicate page identifiers and preference keys. These collisions were rejected, so the tested package could not introduce ambiguous names.

Evidence: `tests::repeated_page_ids_and_preference_keys_are_rejected` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:418`.

## Check 6: 🔵 [INFO] SDK traversal and compatibility guards

**PASS** — I checked path-traversal identifiers, incompatible API versions, and packages requiring a newer host. Each invalid form covered by the test was rejected.

Evidence: `tests::rejects_traversal_incompatible_api_and_future_host` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:394`.

## Check 7: 🔵 [INFO] SDK permission declaration

**PASS** — I checked whether widgets could use capabilities their package had not declared. The validator rejected those widgets, so the tested permission declarations were enforced.

Evidence: `tests::capabilities_are_enforced` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:406`.

## Check 8: 🔵 [INFO] Tutorial declarative boundary

**PASS** — I checked tutorial target validation. Supported declarative targets were accepted, while the invalid targets and executable forms covered by the test were rejected.

Evidence: `tests::tutorial_targets_are_validated_without_executable_actions` passed with exit code 0. Test source: `sdk/toolboks-addon-sdk/src/lib.rs:442`.

## Check 9: 🔵 [INFO] Audio Gainer package trust

**PASS** — I checked whether another package could impersonate Audio Gainer. Approved current and legacy metadata remained trusted, while the altered provider identity was rejected.

Evidence: `addons::tests::official_aural_package_cannot_be_impersonated` passed with exit code 0. Test source: `src/addons.rs:352`.

## Check 10: 🔵 [INFO] System Profiles package trust

**PASS** — I checked the native trust rules for System Profiles. Only the exact approved package received native trust in this test.

Evidence: `addons::tests::system_profiles_requires_the_exact_official_package` passed with exit code 0. Test source: `src/addons.rs:372`.

## Check 11: 🔵 [INFO] Disk Manager capability integrity

**PASS** — I checked the older Disk Manager package and its read-only permissions. The approved legacy package remained trusted, but adding optimization permission to that package invalidated its trust.

Evidence: `addons::tests::legacy_disk_package_keeps_read_only_permissions_until_update` passed with exit code 0. Test source: `src/addons.rs:386`.

## Check 12: 🔵 [INFO] Bundled identifier reservation

**PASS** — I checked whether a local package could install under a reserved built-in add-on identifier. The attempted impersonation was rejected.

Evidence: `addons::tests::cannot_impersonate_bundled_addon` passed with exit code 0. Test source: `src/addons.rs:424`.

## Check 13: 🔵 [INFO] Native provider trust boundary

**PASS** — I checked the first-party native package boundary by changing author and provider metadata. The altered packages were rejected.

Evidence: `addons::tests::only_exact_first_party_native_packages_are_accepted` passed with exit code 0. Test source: `src/addons.rs:431`.

## Check 14: 🔵 [INFO] Update asset origins

**PASS** — I checked update asset addresses from untrusted hosts and addresses using plain HTTP. Both types of invalid address covered by the test were rejected.

Evidence: `update::tests::rejects_assets_from_untrusted_hosts` passed with exit code 0. Test source: `src/update.rs:650`.

## Check 15: 🔵 [INFO] Update checksum format

**PASS** — I checked checksum parsing with valid and malformed SHA-256 values. A valid checksum parsed successfully; short, invalid-hex, and empty values were rejected.

Evidence: `update::tests::parses_checksum_files` passed with exit code 0. Test source: `src/update.rs:660`.

## Check 16: 🔵 [INFO] Downloaded executable validation

**PASS** — I checked downloaded file validation. The test accepted the expected executable shape with a matching checksum and rejected checksum mismatches and non-executable data.

Evidence: `update::tests::verifies_checksum_and_exe_shape` passed with exit code 0. Test source: `src/update.rs:672`.

## Check 17: 🔵 [INFO] Elevated relaunch command arguments

**PASS** — I checked argument quoting for an administrator relaunch. Spaces, embedded quotes, empty arguments, and trailing backslashes were escaped according to the Windows command-line rules asserted by the test. This checked quoting logic rather than performing an administrator relaunch.

Evidence: `elevation::tests::args_are_quoted` passed with exit code 0. Test source: `src/elevation.rs:205`.

## Check 18: 🔵 [INFO] Elevated signed-in-user guard

**PASS** — I checked the signed-in-user guard for elevated profile changes. Mismatched or unknown users were rejected, and uncertain elevation/user information did not grant access in the tested cases.

Evidence: `elevation::tests::elevated_profile_changes_require_a_matching_shell_user` passed with exit code 0. Test source: `src/elevation.rs:224`.

## Check 19: 🔵 [INFO] Recovery journal target allowlist

**PASS** — I checked whether a recovery backup could select unrelated registry values. The tested backup could not target arbitrary entries outside the known settings catalog.

Evidence: `native::profiles::engine::tests::backup_cannot_select_unrelated_registry_values` passed with exit code 0. Test source: `src/native/profiles/engine.rs:745`.

## Check 20: 🔵 [INFO] Safe restoration behavior

**PASS** — I checked restoration using mocked registry state. It removed values created by Toolboks and preserved external edits instead of overwriting them. This confirmed the tested restore logic without changing the real registry.

Evidence: `native::profiles::engine::tests::restore_removes_new_values_and_preserves_external_edits` passed with exit code 0. Test source: `src/native/profiles/engine.rs:676`.

## Check 21: 🔵 [INFO] HTTP transport URL parsing

**PASS** — I checked HTTPS address parsing. The valid forms covered by the test were split into the expected host and path.

Evidence: `winhttp::tests::splits_urls` passed with exit code 0. Test source: `src/winhttp.rs:190`.

## Check 22: 🔵 [INFO] HTTP transport scheme and authority rejection

**PASS** — I checked invalid address schemes and authorities. Plain HTTP, empty hosts, user information in the address, and explicit nonstandard ports were rejected. This checked parsing rules rather than live server redirects.

Evidence: `winhttp::tests::rejects_non_https_and_odd_hosts` passed with exit code 0. Test source: `src/winhttp.rs:202`.

## What I also reviewed in the source

Alongside the executed tests, I inspected the package read limits and exact bundled-package validation in `src/addons.rs:77,183`, checksum/executable checks and final redirect-origin validation in `src/update.rs:336,353`, and download size/time bounds in `src/winhttp.rs:79-179`.

I also inspected the profile engine's restricted restore targets, absolute Windows-system path for powercfg, environment-variable handling for restore-point data, and elevation/signed-in-user gates in `src/native/profiles/engine.rs:172,368,411,602`. These are source-review observations, not additional runtime confirmations.

## Findings and agent follow-up

No defect was confirmed by these checks, so there was no defect to attribute to an implementation agent and no author explanation to request. No application code was changed. These results remain confirmations of the tested assertions at this commit; changes to the relevant code or conditions may require retesting.

## How the results can be reproduced

The eight SDK cases ran with `cargo test -p toolboks-addon-sdk --lib`. The host test binary was built with `cargo test -p toolboks --bin toolboks --no-run`, then the 14 host cases named above were executed using `--exact <test-name> --nocapture`. Every recorded case passed.

The initial offline attempt was blocked by a missing winresource dependency. A network-enabled retry fetched the dependencies and allowed testing to finish. The build succeeded with a warning about a missing optional icon resource compiler; this did not prevent the tests from running.

## What remains untested

I did not run a dependency vulnerability audit, fuzzing, penetration testing, live hostile-server or redirect tests, filesystem race/ACL checks, or full backend/UI coverage. I did not perform a real update installation or change Windows registry values, services, power plans, or drive optimization settings. The recovery tests used mocked state, and the elevation tests checked logic rather than a complete privileged workflow.

The updater's SHA-256 checksum from the same release helps detect corruption; it is not an independent publisher signature. Further security testing would need to cover the untested areas above before making broader claims.

# Test-2: 🟠 [MEDIUM] Audio Gainer microphone hot-plug refresh

On 12 November 2026, I ran a **Light test** of Audio Gainer in Toolboks 0.12.3, using current staging commit `327f1388e04ebec6f324b05b9dec91b8dca9381f`.

## What I checked

In this test, I traced when the microphone list is populated, what happens after the first load, and whether the foreground, background or resume paths notice a newly connected microphone. I also ran the four existing Audio Gainer unit tests as a small regression smoke check. I did not physically connect or disconnect a microphone.

## 🟠 [MEDIUM] The microphone list has no automatic refresh trigger

**FAIL — source-confirmed behaviour gap.** The device list is a snapshot. Audio Gainer fills it when its page first loads and when the user presses **Refresh devices**. Once that first scan sets `loaded` to true, drawing the page again does not enumerate devices. Plugging in a microphone does not invalidate this snapshot: I found no device-change notification or periodic inventory scan in the app's audio path.

I inspected every `refresh()` call site and the foreground tick, background error handling and resume path. None of the latter refresh the inventory. Therefore, while the same Audio Gainer instance stays loaded, it has no path that adds a newly connected device automatically. This explains the reported stale list without needing to assume that Windows failed to detect the microphone.

The user-visible impact is that a new microphone can remain absent until an explicit scan. The expected behaviour is that the list updates after connection while preserving unaffected audio. This finding remains open; no fix was made in this test.

Evidence: enumeration and `loaded = true` are in `src/native/aural.rs:199-239`; the only refresh callers are the button at `:420-423` and initial page load at `:666-669`. Foreground/background/resume handling at `:709-753` does not enumerate. The picker reads the stored vector at `:343-360`. This is source-path verification, not a real hardware hot-plug reproduction.

## 🔵 [INFO] Manual refresh also stops the current audio

I followed the manual refresh path to understand the available workaround. Pressing **Refresh devices** asks CPAL for the current microphone and output lists, but it first calls `cancel()`. That stops both routes and the input, and the scan then chooses default or first-matching devices instead of preserving the user's selections.

This means the existing button is a manual workaround, but it may interrupt routing and require the user to enable their input and routes again. It would be unsafe for audio continuity to call this exact method repeatedly as an automatic-refresh fix. A future change should separate inventory updates from stopping streams, preserve device selections by stable identity rather than enumeration index, and handle an actually removed endpoint explicitly.

Evidence: `src/native/aural.rs:199-239` and `:756-762`. These effects were traced in source; I did not press the button against a live route or claim a runtime refresh pass.

## 🔵 [INFO] The existing Audio Gainer smoke checks passed

**PASS — 4 of 4 existing unit tests passed.** I checked gain/clipping behaviour, background audio forwarding without metering, virtual-cable name recognition, and the headless tutorial layout at widths 610 and 900. These checks are working for their tested inputs. They do not simulate device insertion and do not cover the missing hot-plug trigger.

The executed tests were `native::aural::tests::gain_updates_the_stereo_peaks_and_latches_clipping`, `background_audio_still_forwards_without_metering`, `recognizes_standard_and_additional_vb_cable_endpoints`, and `walkthrough_targets_render_at_both_widths_without_opening_audio` in `src/native/aural.rs:780-856`.

I ran `cargo test --locked --offline -p toolboks --bin toolboks --target-dir ../toolboks/target native::aural::tests -- --nocapture` with the local GNU Rust toolchain and shared Cargo cache. The process exited 0 and reported 4 passed, 0 failed, 479 filtered out. The build warned that the optional icon resource compiler was missing; testing still completed.

## What the Backend Programmer explained

I asked the investigating Backend Programmer about the stale-list behaviour. Their diagnosis, paraphrased, was that `inputs` and `outputs` are snapshots, `loaded` stays true for the existing instance, and no device event or timed rescan requests a new snapshot. Stream-error callbacks stop failed streams but do not update device inventory. This agrees with the source paths I inspected.

They recommended separating a non-destructive inventory update from the current destructive refresh, preserving selections and unaffected routes, and avoiding expensive or overlapping scans on the UI thread. They also noted that lazy add-on loading must remain intact: inactive UI should not stay resident merely to poll devices, while active background routes must continue normally.

This was the current agent's investigation explanation, not the original author's rationale. Git blame attributes the original refresh and initial-load logic to commit `a766385e`, authored as `fredhakon`. The identity of the implementation agent behind that commit is unknown, and its original design rationale was unavailable. The code-history author name is not proof that a particular current agent caused the defect.

## What remains untested and what should happen next

**BLOCKED — physical hot-plug reproduction was not performed.** I did not have a controlled microphone insertion event in this session and did not alter Windows devices or a live audio route to manufacture one. I have not confirmed how quickly a particular installed microphone becomes available to CPAL, whether the user's running executable exactly matches this commit, or how reconnecting the same device behaves during live routing.

The next fix should add a device-change refresh path that preserves unaffected routing and selections, then be checked by connecting a microphone while Audio Gainer is already open, disconnecting/reconnecting it, and switching away and back with and without background routes. These hardware checks and a refreshed-list regression test remain necessary before calling the issue fixed. No application source, device settings or GitHub state changed during this test.

# Test-3: 🟠 [MEDIUM] Microphone list refresh on opening and a dedicated refresh button

On 12 November 2026, I ran a **Light test** of the microphone-list changes in Toolboks 0.12.3. I tested the working changes on staging base `327f1388e04ebec6f324b05b9dec91b8dca9381f`, before commit. The final tested `src/native/aural.rs` had SHA-256 `DD76880D7A57E6B191F66E675AC4CF14A860DA1B5D3566516C7C0EA6B2A45C81`.

## What I tested

In this test, I checked that opening the microphone dropdown requests a fresh list, reopening it requests another refresh, and leaving it open does not repeatedly scan devices. I tested pointer and keyboard opening through the real egui picker helper with a refresh counter, without opening a microphone stream. I also checked selection reconciliation, scan-failure handling, the dedicated Refresh microphones button wiring, and the existing audio smoke tests.

**The final result was 9 of 9 tests passing, with formatting and workspace Clippy checks also passing.** Two issues in the first UI implementation were found and corrected during this request. They are recorded below so the permanent report retains what happened rather than showing only the final success. This follows up on Test-2's stale-list finding; it implements refresh on demand, not a background hot-plug watcher.

## 🟠 [MEDIUM] First-opening and repeated-refresh defects were found and fixed

**FAIL on the initial implementation; PASS after correction.** The first actual-event tests exposed two related assumptions in the new picker. Its open-state lookup used the raw string identifier, whereas egui 0.37's ComboBox wraps that string in an IdSalt before making its widget ID. Those IDs differ, so the helper could not reliably recognize a popup that was already open. In addition, the first popup content pass is a sizing pass whose child UI is disabled. Checking that child UI's enabled state suppressed the very first refresh.

These defects could leave the first displayed list stale and cause unnecessary subsequent scans. I asked Designer, who owned the picker changes, for the reasoning. Designer explained that they had assumed the raw salt and wrapped IdSalt produced the same ID, and that the child UI's enabled state represented whether the parent control was interactive. Both assumptions were incorrect for this egui version.

Designer corrected the helper to use the exact ComboBox IdSalt and capture the parent's enabled state before drawing popup content. I reran the unchanged event assertions: pointer opening refreshed once, three subsequent open frames did not refresh again, closing and reopening refreshed a second time, and keyboard opening refreshed once. The disabled control did not open or refresh. The issue is resolved in the tested working changes.

Evidence: actual egui event tests `microphone_picker_refreshes_once_per_pointer_open_and_reopen` and `microphone_picker_refreshes_on_keyboard_open_but_not_when_disabled` in `src/native/aural.rs:967` and `:990`; production helper at `:434`. The first test-harness attempt also used the wrong raw identifier; I corrected the harness to egui's actual widget ID before interpreting the event failures as product defects.

## 🔵 [INFO] Refresh keeps selections tied to the same microphone

**PASS for the tested reconciliation rules.** I checked the backend tests for a selected microphone moving within a reordered list, a changed display name with the same endpoint identity, and a different endpoint reusing the old name. The same endpoint remained selected after reordering/renaming; a replacement did not silently inherit that selection. Ambiguous names in the fallback path did not select an arbitrary device, and an absent selection did not automatically become a new one.

On Windows, source inspection confirmed that the production comparison uses CPAL WASAPI device equality. The installed CPAL implementation compares endpoint IDs, rather than just display names. The generic selection tests exercise the reconciliation with supplied identities; they do not constitute a physical WASAPI hot-plug test.

Evidence: `microphone_refresh_preserves_endpoint_identity_across_reorder_and_names` and `microphone_refresh_requires_unambiguous_selection_without_endpoint_ids` passed. Production code is in `src/native/aural.rs:146,165`; tests begin at `:1010,1025`.

## 🔵 [INFO] Failed scans retain state and allow retry

**PASS for the injected failure/retry case.** I tested the actual refresh-result handler with an enumeration error and then an empty successful result. The failure produced a local microphone-refresh error while retaining the existing output selections and unrelated audio/route errors. A successful retry cleared the refresh error without clearing those unrelated errors or changing outputs.

Source inspection confirmed that the input-only refresh does not call cancel, replace output devices, change route objects or reset input gain. Losing the selected endpoint clears the selection and explains that it is unavailable; the Disable microphone action remains enabled for an already-live input even when no inventory selection remains. This avoids making a running input impossible to stop from its control.

Evidence: `microphone_refresh_failure_retains_state_and_retry_clears_only_refresh_error` passed at `src/native/aural.rs:1039`. The scan/result paths are at `:237,248`, and the live-input action condition is at `:529`. A real active-stream continuity check was not performed.

## 🔵 [INFO] The new microphone refresh button is connected to the input-only refresh

**PASS by source-path inspection.** The Microphone card now has a dedicated Refresh microphones button immediately below its selector. It calls the same input-only refresh used when opening the list. Both controls are disabled when live audio access is unavailable or a tutorial disables interaction; local scan errors appear beside the microphone controls. The separate existing Refresh devices action retains its broader reset behaviour, so it should not be confused with the new microphone-only action.

I traced the new button at `src/native/aural.rs:505` and the dropdown callback at `:457`. The headless tutorial test still passed at widths 610 and 900 without loading devices or opening audio. I did not press the dedicated button in a running native window; this part establishes the actual event-handler wiring and disabled layout path, not a manual native UI pass.

## 🔵 [INFO] Existing audio behaviour and development checks passed

**PASS — all 9 focused tests passed.** In addition to the five new picker/refresh tests above, the existing gain/clipping, forwarding-without-metering, cable-name recognition and tutorial-layout tests passed. These checks confirm their asserted behaviour; they do not prove uninterrupted output through physical devices.

I ran `cargo test --locked --offline -p toolboks --bin toolboks --target-dir ../toolboks/target native::aural::tests -- --nocapture` with the local GNU Rust toolchain. The final result was 9 passed, 0 failed, with exit code 0. `cargo fmt --all --check`, `git diff --check`, and `cargo clippy --workspace --all-targets --locked --offline --target-dir ../toolboks/target -- -D warnings` also completed successfully.

Clippy initially rejected two new backend style patterns: a single-pattern match and field assignments after Default construction. I asked Backend Programmer, who owned those changes, to correct them. They changed the match to a conditional pattern and initialized the test state directly. A narrow irrefutable-pattern allowance supports the Windows backend variant configuration without changing endpoint comparison. The final targeted suite and Clippy pass include these corrections. The build emitted the existing optional icon-resource warning, and incremental cache hard-link fallback warnings during test compilation; neither prevented the checks from passing.

## What remains untested

**BLOCKED — physical hot-plug and live-route checks were not performed.** I did not connect/disconnect real microphones, disable Windows devices, or start/stop the user's audio routes. Native visual inspection, scan latency on the user's driver, physical duplicate-device behaviour, and uninterrupted live routing across a scan remain to be checked on real hardware. No full standard/preview CI matrix or release build was run for this light test.

The requested on-demand refresh paths are implemented and the focused tests pass. A physical microphone plugged in while the list is closed should be discovered on the next opening or Refresh microphones action; this expected hardware behaviour still needs a real-device check. The list does not continuously rescan while left open. Background automatic device notifications were not part of this request.

# Test-4: 🟠 [MEDIUM] Macro Manager smoke test

On 13 November 2026, I ran the requested smoke test after the Macro Manager implementation was finished. The tested build was the working staging changes based on `1b6ab8e` (Toolboks 0.12.4). This request did not resume the previously stopped Audio Gainer settings test.

## What I checked

I checked package validation, timing units, action-model serialization, empty playback rejection, local preference persistence, and headless rendering of the Editor and Mouse editor at widths of 900 and 1280 points. The final result was **6 passed, 0 failed**. No input hooks, recording, playback or global shortcuts were started by these tests.

## 🟠 [MEDIUM] The new sidebar icon initially blocked installation

**FAIL initially; PASS after correction.** The Macro Manager package used the new `macro` icon, but I had added its drawing code without updating the SDK's allowed icon names. The package validation smoke test caught that mismatch. I added `macro` to the SDK and JSON schema lists, then reran the same smoke suite successfully. Responsibility: Main agent, which owned host/package integration. The cause was an omitted validation-list update, not a backend-engine decision. This issue is resolved in the working build.

## 🔵 [INFO] Editor rendering, data and timing checks passed

**PASS.** The editor rendered both pages without activating recording or playback. Macro data survived a save/load through the actual namespaced preference methods. The engine action model round-tripped with key/button releases and path click actions. Timing parsing accepted the requested `ms`, `s`, and `sec` inputs and bare numbers in the selected scope; invalid times were rejected. Empty playback returned an error without starting an input worker.

The first UI test attempt also needed an egui test-harness correction: unused texture deltas must be explicitly cleared. I corrected that cleanup rather than treating it as an application rendering defect. The unchanged rendering assertions then passed.

## Evidence and limits

I ran `cargo test --locked --offline -p toolboks --bin toolboks --target-dir ../toolboks/target macro -- --test-threads=1`. The six passing tests are `official_macro_manager_package_validates_and_rejects_impersonation`, `authored_and_recorded_model_round_trip`, `empty_play_does_not_start_input_worker`, `timing_units_and_active_scope`, `background_recording_results_use_namespaced_preferences`, and `editor_and_mouse_page_render_without_starting_input`. Sources are `src/addons.rs`, `src/native/macros.rs`, and `src/native/macros/engine.rs`. Formatting and the preview compilation check also passed.

This was a smoke test only, not an exhaustive review or a real desktop-input check. Recording actual keyboard/mouse activity, playback against another application, global shortcut registration, Esc cancellation during live playback, multi-monitor coordinates and real elapsed repeat duration remain to be checked by hand. No claim of an end-to-end recording/playback pass is made.

# Test-5: 🟠 [MEDIUM] Macro activation-key smoke test

On 13 November 2026, I checked macro activation on staging at `b1c84f2`. The requested medium test was reduced to a smoke test by the user. In-app testing was then explicitly stopped; no further UI interaction was performed.

## 🔵 [INFO] Windows delivered the assigned macro index — PASS

I added an opt-in automated Windows integration test, `windows_activation_key_delivers_target_macro`. It registers F24 for macro index 3, sends a press/release through the existing input engine, and waits for the actual Windows hotkey queue to deliver `Play(3)`. This passed. The test is ignored during ordinary runs because it needs an interactive Windows desktop and injects an F24 key press. It does not launch macro actions or alter saved user macros.

Command: `cargo test --locked --offline -p toolboks --bin toolboks --target-dir ../toolboks/target windows_activation_key_delivers_target_macro -- --ignored --test-threads=1`.

## 🟠 [MEDIUM] Reported activation failure remains unresolved — NOT REPRODUCED

Before the user stopped in-app testing, pressing the saved `4` activation key in the preview changed Play macro to Stop playback and disabled editing, showing that playback started. This observation does not establish that activation works in the user's failing context or that every playback action completed. The following attempt to check another selected macro was interrupted and has no result.

The current smoke test found no confirmed implementation defect to fix. Responsibility for the preceding activation-key implementation is the main agent. Its current diagnosis is that Windows delivery works in the tested conditions; the cause of the user's failure is still unknown. This is not an explanation from the original engine author. Activation while a game has focus, minimized-window operation, and end-to-end playback in the failing environment remain unverified. No product behavior was changed and no issue should be marked resolved based on this test.

## 🔵 [INFO] Existing macro checks — PASS

The focused macro suite finished with 7 passed, 0 failed and the desktop integration test intentionally ignored. The separately invoked desktop integration test passed as described above. These checks cover saved per-macro keys, legacy shortcut migration, model serialization, timing, empty playback, package validation and headless editor rendering. They do not confirm the reported failing scenario.
