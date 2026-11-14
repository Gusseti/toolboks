# Unreleased

## Added

- **Add-on supervisor (#80, not reachable from the app yet):** the host can start, watch and stop add-on processes. Each launch uses two named pipes that only the current Windows user can open, plus a random token the add-on must present. The exe is checked against the SHA-256 recorded at install time before every start, and a changed file is not run. All add-on processes end when Toolboks ends, also after a crash of Toolboks. A hidden add-on is told to suspend. An add-on that crashes is restarted at most 3 times per minute, then stays stopped with a plain reason, a restart action and a report that can be copied. Administrator rights are per add-on: only the add-on that asks is restarted through the UAC prompt.
- **Installer:** records the SHA-256 of a native add-on's exe at install time and keeps the previous version's hash for a rollback.
- **Add-on SDK runtime:** `run_addon` accepts `--token` (or `TOOLBOKS_ADDON_TOKEN`) and presents it to the host before the handshake.

## Not verified

- The elevated start through the UAC prompt has not been tried on a real PC.
- The "only the current user" rule for the pipes is tested by reading the pipe's access list, not with a second Windows account.
