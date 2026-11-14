# Agreed add-on direction

These requirements describe the owners' direction for Toolboks. What is built, and how it fits
together, is in [ADDON-ARCHITECTURE.md](ADDON-ARCHITECTURE.md). The plan and its issues are in
#77.

## Decided by the owner (2026-11-14)

- **Toolboks ships as an empty shell.** No tool is built into the app. Every tool, including
  Resource Monitor, Memory Cleaner, Debloater, Event Watch, Disk Manager, Audio Gainer, Macro
  Manager and System Profiles, is an add-on that the user asks for inside the app, downloads in
  seconds, and can update or remove.
- **Native add-ons are separate exes drawn inside the same Toolboks window.** Not a DLL loaded
  into the app, not a window of their own, not WASM.
- **The catalog is signed offline.** The owner holds an offline signing key and an offline
  recovery key; CI and the app only verify.

## What stays true

- Each workspace stays in the same window, and installed and turned-on add-ons appear in a
  collapsible sidebar.
- Add-ons share the host's design system ([DESIGN.md](../DESIGN.md)): graphite backgrounds, one
  purple accent, the Toolboks icon style and plain wording. They choose their content and pages,
  not their palette, font or icons. The checklist is in
  [ADDON-DESIGN-CHECKLIST.md](ADDON-DESIGN-CHECKLIST.md).
- The standalone Stratum remains separate. Its condensed Toolboks workspace is Disk Manager, with
  subdued "by stratum" attribution.
- Only the team's own add-ons receive the Toolboks verified checkmark and the first positions in
  the add-on browser. A package cannot claim that status itself.
- Configuration and add-on choices persist in locally generated files. Preview data stays
  separate from the stable app.
- The audience is ordinary users with above-average interest in their system: clear explanations
  and defaults, with deeper controls available when useful.
- Every workspace offers Help with full and current-tab guided tours. Tours open only when
  requested, highlight their subject, never run an action, and can be cancelled at any point.
- An add-on gets administrator rights only when it needs them, and only that add-on.
- One add-on that crashes or hangs does not close the app.

## Assumptions in the plan (say so if any is wrong)

- Only first-party add-ons for now. Publishing by others needs its own plan and a sandbox
  discussion.
- An update of the host does not force add-on updates unless the protocol version changes.
- Removing the built-in tools is a breaking release with a version bump and clear notes (#94).
- The history recorder lives in the Resource Monitor add-on as a background add-on (#64, #89).

## Still to decide

- Whether to buy a code-signing certificate before the first add-on release (#95).
- One repository for all first-party add-ons, as proposed, or one per add-on (#83).
- Whether a hidden add-on that is not a background add-on keeps a suspended process or none
  (#97).
- The budgets for size, memory and start-up time (#97).

Changes are tested locally before a pull request to `staging`. Publishing releases and merging to
`main` remain separate actions.
