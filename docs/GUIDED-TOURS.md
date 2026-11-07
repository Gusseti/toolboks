# Guided walkthroughs

Help is available beside the verification badge in every add-on workspace. Nothing
opens automatically. Choose the full add-on tour or the current tab only.

The guide dims the workspace and outlines the current component in purple. Next
and Back navigate pages and supported inner tabs. Left/right arrow keys also move
between steps; Cancel or Escape exits at any point. Jump to lets users choose a
particular explanation. Completing or cancelling restores prior page selections.
Workspace controls are disabled while touring; existing background jobs continue.

Each built-in tool has its own tour, with its Help button in its own workspace:
Memory Cleaner (12 steps: Home and Advanced, including Selected/System/Rules),
Resource Monitor (22 steps: Overview, Details, Processes, Connections, and each
top-process ranking), Debloater (10 steps: Privacy and ads, Apps, Startup, Tasks,
Services), and Event Watch (6 steps: Live, Crashes, Summary). "Just this tab" tours
the page that is open, including its inner sections.

Disk Manager has 21 steps covering drive/path selection, folder browsing, scanning
and cancellation, exclusions, totals, view navigation, map, filters, breadcrumbs,
list actions, largest files, file types, coverage/errors, export and drive optimization. Before a scan
is ready, its tour uses a labeled, in-memory sample; it does not scan or export.
The sample disappears on exit, and actual results and preferences remain intact.
If a component is waiting for data, the guide highlights its empty-state message.

Anchors come from actual rendered rectangles. Scroll containers reveal targets
when a step changes. The overlay uses the visible clipped rectangle, avoiding
fixed screen coordinates. Tests render all first-party steps and verify each has
an anchor; Disk Manager tests verify no worker starts and transient state restores.

SDK 0.3 adds optional declarative tutorial metadata and native source-level hooks.
The host supplies generic page/widget tours when metadata is omitted. Package
validation rejects invalid page/widget references and executable actions. See
sdk/README.md for the authoring contract. Native provider routes remain trusted
host-linked code, not dynamically installed executables.
