# Unreleased

## Added

- **`developer_integration` capability:** an add-on can declare it (SDK `Capability::DeveloperIntegration`, schema, catalog) for a local job queue, process priority and CPU core changes, and Claude Code scripts and settings. The install review explains it in plain words (`src/marketplace.rs`). It grants no administrator helper, so it is not in the native-only list.

- **No signatures (owner decision):** the add-on catalog is a plain list anyone can publish. It is not signed, so the catalog keys, key rotation, expiry and replay checks, the `catalog-sign` tool and `docs/catalog-signing.md` are gone. A download is still checked against the size and SHA-256 its entry declares. Older indexes with `sequence`, dates or `publisher_key_id` still read. See `docs/catalog.md` and the note at the top of `docs/ADDON-THREAT-MODEL.md`.

- **Add-on browser and first run (#86):** choose tools with nothing preselected, review several installs together, and download them through the checked catalog and existing installer. Browse by search, category or installed state; inspect screenshots, access and release notes; update, open or remove without leaving Toolboks. Installs show progress and failures stay in the sidebar until dismissed, including after a restart. Removal keeps data by default, with an explicit option to delete it. Local `.tbaddon` review and a browser walkthrough remain available.
- **Installed native add-ons:** catalog-installed programs open through the existing supervisor and drawing view in the Toolboks window. Updates wait for the old program to stop. A local test catalog is available only to preview builds for first-run checks; release builds never accept its file transport.
- **Browser look:** the add-on browser follows the landing page: wide rows, a 340 px detail pane, icon tiles, verified pills and category buttons, in Schibsted Grotesk. The source values are in `docs/reviews/86/visual-spec.md` and the side-by-side check in `docs/reviews/86/compare/`.
- **Other add-on screens (#115):** the first-run picker, full details page, install review, removal dialog and sidebar states use the browser's rows, Verified pills, buttons and type. Sidebar states are pills: purple for an update, red for a failed install. A failed install shows its message and Try again under the Install button.
- **Disk Manager pilot (#88):** moved its code and executable build to `toolboks-addons`. This host no longer includes its provider or activation package. The catalog executable retains ID `toolboks.disk-manager`, five pages, preferences and data folder; existing data is kept when an old activation package is ignored. First catalog publication is still required.
- **Catalog entries:** optional `byline` field, such as "by stratum", shown in the details.
- **Catalog address:** the browser and the first-run picker list only what the catalog at `src/catalog.rs::URL` returns. Nothing in those lists is built into the app.

## Not changed here

- The built-in tools and the add-ons shipped inside the app (Aural, System Profiles, Macro Manager) stay until they move to the catalog (#89 to #93) and are removed with a migration (#94). Local packages still cannot take a built-in tool's ID. The first-run picker opens by itself only when the app has no tools; until #94 it is reached from the sidebar.

## Release prerequisites

- **Disk Manager:** first owner/orchestrator release and the scheduled GUI catalog install check/screenshots (pending, not run) remain. Defrag, TRIM and Windows-selected optimization are gated on #132. After its protocol 3 helper merges to staging, repin host dependencies and rebuild/retest the add-on before publication.

- The public `toolboks-addons` repo and its catalog exist. Public downloads need the first add-on released into the catalog. The catalog address is `src/catalog.rs::URL`.

## Existing platform work

- **Add-on supervisor (#80, not reachable from the app yet):** the host can start, watch and stop add-on processes. Each launch uses two named pipes that only the current Windows user can open, plus a random token the add-on must present. The exe is checked before every start; privileged starts use the saved catalog entry and declared hash. All add-on processes end when Toolboks ends, also after a crash of Toolboks. An add-on that crashes is restarted at most 3 times per minute, then stays stopped with a plain reason, a restart action and a report that can be copied. Administrator rights are per operation: a separate helper starts through UAC while the add-on interface stays unelevated.
- **Installer:** records the SHA-256 of a native add-on's exe at install time and keeps the previous version's hash for a rollback.
- **Add-on SDK runtime:** `run_addon` accepts `--token` (or `TOOLBOKS_ADDON_TOKEN`) and presents it to the host before the handshake.
- **Host services for add-ons (#87; tray and add-on tour controls are not wired yet):** an out-of-process add-on can show status bar items, register tray entries, describe a guided tour with highlighted anchors, declare its pages for `--open "Title/Page"` links, be marked as a background add-on that starts at launch, and have its preferences saved in `config.json`. Status items and tray entries disappear when the add-on stops or is turned off. A tour forwards no input to the add-on, so it cannot run an action.
- **SDK administrator actions (#132):** each action uses a separate helper with its own native Confirm/Cancel dialog. The interface stays unelevated; host input cannot approve a change. Disk Manager uses this path; its drive report returns on completion rather than streaming phase progress.
- **Protocol 3 migration (#132):** rebuild native add-ons and update their catalog protocol version; none have been published. Legacy whole-interface elevation requests are refused; open add-ons from an unelevated host. System Profiles must adopt helpers in #92.
- **Catalog entries:** optional `pages` and `background` fields. The installer keeps the catalog entry beside each installed version, so they are known offline.
- **Add-on threat model (#95):** `docs/ADDON-THREAT-MODEL.md` lists the threats to the add-on platform, what covers each, what is accepted, and seven findings to settle before the first public add-on release.
- **Add-on author guide and sample (#96):** `docs/ADDON-AUTHOR-GUIDE.md`, a sample add-on exe (`sdk/examples/hello-addon`) and a test host window (`cargo run --example addon_host`) to build and try an add-on. The architecture and vision documents now describe the empty-shell direction and say which pieces are built.

## Changed

- **Shared code for add-ons (#99, no change in behaviour):** Audio Gainer, Macro Manager and System Profiles now import the shared crates directly and no host code. `data_root` and `write_atomic` moved to `toolboks-shared` (`storage`); the host re-exports them. The rest of the Debloater engine (apps, restore points, services, startup, scheduled tasks, and the change error with its rollback) moved to `toolboks-debloat-engine`; the Debloater page uses it from there.

- **Administrator Confirm window (#251):** the window is larger (400 by 260 dialog units), so long previews such as Memory Cleaner's eight clean regions and its freeze warnings fit with little scrolling. A preview is still limited to 8,000 bytes and is never cut.

## Fixed

- **Add-on launch hardening (#95, F2/F3):** every privileged native launch reads the saved catalog entry and uses its hash and granted elevation capability instead of writable install records. A missing saved catalog requires a refresh; rollback versions absent from the catalog cannot elevate. MSVC builds load implicit DLL dependencies from System32 only, and installed version folders with extra files or redirected paths are refused.
- **Hidden add-ons (#95, F7):** non-background add-ons exit when their page is left, including cancellation during launch. Reopening waits for an exiting process and starts a new one. Background add-ons retain their process.

- **Release-download address check:** GitHub-only release download paths reject deceptive hosts such as `https://evil.example?x=.github.com`, user info and custom ports.

## Release blockers

- **Administrator helper integration (#132/F4):** native dialog, real UAC and privileged-operation checks await an owner-approved GUI session before administrator add-ons ship.
- **Accepted signing risks (#95):** host-release and code signing (F5/F6) have no work planned under the owner's 2026-11-15 decision. See `docs/ADDON-THREAT-MODEL.md`.

## Not verified

- The "only the current user" rule for the pipes is tested by reading the pipe's access list, not with a second Windows account.
- The test host window ran with the sample add-on without errors, but its page was not looked at on screen.

