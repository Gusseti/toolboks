# Unreleased

## Added

- **Add-on supervisor (#80, not reachable from the app yet):** the host can start, watch and stop add-on processes. Each launch uses two named pipes that only the current Windows user can open, plus a random token the add-on must present. The exe is checked before every start; privileged starts use a valid signed catalog. All add-on processes end when Toolboks ends, also after a crash of Toolboks. An add-on that crashes is restarted at most 3 times per minute, then stays stopped with a plain reason, a restart action and a report that can be copied. Administrator rights are per add-on: only the add-on that asks is restarted through the UAC prompt.
- **Installer:** records the SHA-256 of a native add-on's exe at install time and keeps the previous version's hash for a rollback.
- **Add-on SDK runtime:** `run_addon` accepts `--token` (or `TOOLBOKS_ADDON_TOKEN`) and presents it to the host before the handshake.
- **Host services for add-ons (#87, not reachable from the app yet):** an out-of-process add-on can show status bar items, register tray entries, describe a guided tour with highlighted anchors, declare its pages for `--open "Title/Page"` links, be marked as a background add-on that starts at launch, and have its preferences saved in `config.json`. Status items and tray entries disappear when the add-on stops or is turned off. A tour forwards no input to the add-on, so it cannot run an action.
- **Add-on protocol version 2:** adds the tray and tour-anchor messages. Version 1 add-ons are refused with the plain "update the add-on or Toolboks" message; none have been published.
- **Catalog entries:** optional `pages` and `background` fields. The installer keeps the catalog entry beside each installed version, so they are known offline.
- **Add-on threat model (#95):** `docs/ADDON-THREAT-MODEL.md` lists the threats to the add-on platform, what covers each, what is accepted, and seven findings to settle before the first public add-on release.
- **Add-on author guide and sample (#96):** `docs/ADDON-AUTHOR-GUIDE.md`, a sample add-on exe (`sdk/examples/hello-addon`) and a test host window (`cargo run --example addon_host`) to build and try an add-on. The architecture and vision documents now describe the empty-shell direction and say which pieces are built.

## Changed

- **Shared code for add-ons (#99, no change in behaviour):** Audio Gainer, Macro Manager and System Profiles now import the shared crates directly and no host code. `data_root` and `write_atomic` moved to `toolboks-shared` (`storage`); the host re-exports them. The rest of the Debloater engine (apps, restore points, services, startup, scheduled tasks, and the change error with its rollback) moved to `toolboks-debloat-engine`; the Debloater page uses it from there.

## Fixed

- **Add-on launch hardening (#95, F2/F3):** every privileged native launch needs a still-valid signed catalog and uses its hash and granted elevation capability instead of writable install records. Missing or expired catalogs require a refresh; rollback versions absent from the catalog cannot elevate. MSVC builds load implicit DLL dependencies from System32 only, and installed version folders with extra files or redirected paths are refused.
- **Hidden add-ons (#95, F7):** non-background add-ons exit when their page is left, including cancellation during launch. Reopening waits for an exiting process and starts a new one. Background add-ons retain their process.

- **Download address check:** the check that only GitHub may serve an update or an add-on now reads the host correctly. An address such as `https://evil.example?x=.github.com` was accepted before; addresses with user info or a port are refused too. No known way to reach the flaw existed, since addresses come from a signed catalog or from GitHub's own answers.

## Release blockers

- **Add-on security owner decisions (#95):** F4 (privileged UI confirmation boundary), F5 (offline host release-signing key custody/recovery) and F6 (code-signing certificate) are accepted risks pending owner decision, documented in `docs/ADDON-THREAT-MODEL.md`. They do not authorize a first public add-on release.

## Not verified

- The elevated start through the UAC prompt has not been tried on a real PC.
- The "only the current user" rule for the pipes is tested by reading the pipe's access list, not with a second Windows account.
- The test host window ran with the sample add-on without errors, but its page was not looked at on screen.
