# Add-on wire protocol

An out-of-process add-on is a separate executable that runs its egui page headless and sends
the host tessellated meshes. The host forwards input and paints those meshes inside the add-on's
page rectangle. This document describes protocol version **2**
(version 1 plus the tray and tour-anchor messages).

- Messages and framing: crate `sdk/toolboks-addon-proto` (`toolboks-addon-proto`).
- Add-on side loop: `toolboks_addon_sdk::runtime` (SDK feature `runtime`), entry points
  `run_addon(addon, addon_id)` and `run_addon_on(transport, addon, addon_id)`.
- Host side view: `src/addon_view/`. Process launch, named pipes and their access lists:
  `src/supervisor/` (see "Connection" and "Supervisor" below).

## Framing

Every message is a little-endian `u32` body length followed by a [postcard](https://docs.rs/postcard)
body. The body length is capped at `MAX_FRAME_BYTES` (16 MiB); the reader checks the cap before it
allocates. A body with trailing bytes is malformed. Every failure is a `ProtocolError`, never a
panic, and every error is fatal for the connection (the stream position is unknown afterwards):
the host ends the add-on, the add-on exits.

A stream that ends at a message boundary is a clean close (`ProtocolError::Closed`, which
`run_addon_on` turns into `Ok(())`). A stream that ends inside a message is `Truncated`.

The transport is two independent byte streams, one per direction (`Transport<R: Read, W: Write>`).
Nothing in the protocol depends on stdin and stdout.

## Connection

The host starts an add-on with `--pipe-in <path> --pipe-out <path> --token <text>` (`run_addon`
also reads `TOOLBOKS_ADDON_PIPE_IN`, `TOOLBOKS_ADDON_PIPE_OUT` and `TOOLBOKS_ADDON_TOKEN`).

- **Two named pipes per launch**, one per direction, with a random name
  (`\\.\pipe\toolboks-addon-<random>-in` and `-out`). The add-on reads host messages from
  `pipe-in` and writes its own to `pipe-out`. Two pipes, because a blocked read on a synchronous
  pipe handle holds up a write on the same handle.
- **Only the current user can open them.** Each pipe has a protected access list with one entry
  that allows the user Toolboks runs as; other accounts on the PC and remote clients are refused.
  There is one instance per pipe and the host creates it first.
- **The token** is 32 random bytes as 64 hex characters, new for every launch. The add-on writes
  it to `pipe-out` before anything else; `run_addon` does this. A wrong token ends the add-on.
  The token is not a protocol message, so it does not change `PROTOCOL_VERSION`.
- **The process must be the one the host started.** The host compares the process id on each pipe
  with the process it launched and refuses any other.
- The add-on has `Timing::connect` (10 seconds) to open both pipes, present the token and answer
  `Hello`; after that the host ends it.

With no paths, `run_addon` falls back to stdin and stdout (for tools and tests); the add-on must
then never print to stdout.

## Handshake and versions

1. The host sends `Hello { protocol_version, addon_id, software_version }`.
2. The add-on checks the version. On a mismatch it sends `Refuse { protocol_version, reason }` and
   exits with `ProtocolError::VersionMismatch`.
3. Otherwise it answers with its own `Hello` (its id and the SDK version).
4. The host checks version and id the same way and refuses with `Refuse` on a mismatch.

The refusal text is the same on both sides: "Add-on speaks protocol X, Toolboks speaks Y. Update
the add-on or Toolboks." `Hello` (variant 0) and `Refuse` (variant 1) are frozen in both
directions, so any two versions can refuse each other in plain words. Any other change to a
message bumps `PROTOCOL_VERSION`.

## Messages

Host to add-on (`HostMessage`):

| Message | Meaning |
|---|---|
| `Hello`, `Refuse` | Handshake (frozen). |
| `Input(InputBatch)` | Input and view state for exactly one frame: `seq`, page size in points, `pixels_per_point`, theme (light or dark), window focus and events. |
| `Navigate { page }` | Calls `NativeAddon::navigate`. |
| `Suspend`, `Resume` | Page left or entered the foreground. |
| `LoadPreferences { json }` | Namespaced preferences as a JSON object. |
| `SavePreferences` | Asks for preferences; answered by `Preferences`. |
| `Capabilities(names)` | Granted capabilities by manifest name; unknown names are ignored. |
| `TutorialQuery`, `TutorialStart`, `TutorialNavigate { page }`, `TutorialFinish` | Tour hooks; the query is answered by `Tutorial`. |
| `Ping { nonce }` | Liveness check, answered by `Pong` from the add-on's main loop. |
| `Quit` | Orderly shutdown: the add-on calls `cancel` and exits. |
| `TrayAction { id }` | The user chose one of the add-on's tray entries. The runtime passes on only an entry the add-on offers and has enabled. |
| `TutorialTarget { target, reveal }` | The tour anchor to highlight, or none. `reveal` scrolls it into view once. |

Input events (`InputEvent`): pointer moved, pointer button, pointer left, drag released outside,
scroll (unit, phase, source), zoom, key (logical and physical key, repeat, modifiers), text,
paste (clipboard text the host read), copy, cut, IME preedit, IME commit, IME delete-surrounding,
focus gained (optionally by Tab, with direction) and focus lost. Positions are in the view's
logical points with the origin at the page rectangle's top-left corner; positions outside the
view are valid during a drag. The host sends no time: the add-on keeps its own clock, because
reusing host time froze egui animations in the spike.

Add-on to host (`AddonMessage`):

| Message | Meaning |
|---|---|
| `Hello`, `Refuse` | Handshake (frozen). |
| `Frame(Frame)` | The answer to one `Input`, and its acknowledgement (`seq` echoes the batch). |
| `RequestRepaint { after_ms }` | Something asked for a repaint between frames (background data, animation). |
| `Preferences { json, changed }` | Preferences as a JSON object. Sent on `SavePreferences` and when `ui` reports a change. |
| `HostRequest` | `RestartElevated { page }` or `ReviewUpdate`. |
| `StatusItems(items)` | The full set of status bar items; sent when it changes. |
| `Tutorial { steps, location }` | Answer to `TutorialQuery`. |
| `Log { level, line }` | A log line queued with `runtime::log`. |
| `Pong { nonce }` | Answer to `Ping`. |
| `Bye` | The add-on exits on its own. |
| `TrayItems(items)` | The full set of entries (`id`, `label`, `enabled`) the add-on offers in the host's tray menu; sent when it changes. Optional. |
| `TutorialAnchor { target, rect }` | The rectangle of the anchor asked for with `TutorialTarget`, in the view's points, or none while it is not visible. Sent before the frame it belongs to, when it changes. |

A `Frame` holds the meshes (see below), texture uploads (`TextureUpdate { id, delta }`) and frees,
a repaint-after hint, the cursor icon, copied text, URLs to open, the IME output rectangle, whether
the add-on wants the keyboard, a Tab focus escape, and the page index. Messages that an input batch
causes (changed preferences, host requests, status items, log lines) are sent before the frame that
acknowledges that batch.

Managed texture ids are in the add-on's own namespace; the host maps them to its own textures.
Paint callbacks cannot cross the process boundary and are dropped.

## Cached meshes

Each tessellated primitive gets an id: a 64-bit hash of its clip rectangle, texture id, vertices and
indices. The add-on sends a primitive in full (`MeshRef::New`) the first time and as a bare id
(`MeshRef::Cached`) while it stays unchanged. Both sides follow one rule: after a frame, the cache
holds exactly the ids that frame referenced. No extra messages keep the caches in step. A cached id
the host does not hold is `ProtocolError::UnknownMesh`. Helpers: `mesh::MeshEncoder` (add-on side)
and `mesh::MeshCache` (host side).

## Frame pacing

- **One frame in flight.** The host sends the next `Input` only after the `Frame` that
  acknowledges the previous one. Input that arrives meanwhile is queued and merged: consecutive
  pointer moves collapse into one (`pacing::FramePacer`).
- **60 fps cap.** The host sends at most one batch per `pacing::FRAME_INTERVAL`.
- **Add-on-initiated repaints.** `RequestRepaint` and the frame's `repaint_after_ms` go through
  `pacing::RepaintLimiter`, so two of them are at least one frame interval apart. `after_ms` is the
  time left until the deadline the add-on asked for. Between two input batches the add-on sends
  another `RequestRepaint` only for an earlier deadline, and it ignores repaint requests made
  during a pass, because those are already in that frame's hint. The add-on's reader never
  blocks on its queue (a blocked reader would stop draining the pipe while the host may be
  blocked writing to it: a duplex deadlock). Past `MAX_QUEUED_HOST_MESSAGES` (4096) unhandled
  host messages the add-on ends the session with `ProtocolError::Overloaded`.
- The host drives every frame. The add-on renders only when an `Input` arrives.

## What may block

- **The add-on never blocks the host.** The host reads on its own thread and never waits for the
  add-on on the UI thread. It decides that an add-on is stuck with `pacing::HeartbeatMonitor` (any
  message proves the add-on's main loop is alive; after `PING_INTERVAL` of silence the host pings;
  a ping unanswered for `STALL_TIMEOUT` means stalled) and `FramePacer::stalled` (a batch with no
  frame for the timeout). Both take the current time as an argument, so they are tested with
  made-up instants.
- **The add-on may block on the host.** Its main loop waits for the next message; its reader runs
  on a separate thread. Pongs come from the main loop, so a `ui` call that never returns shows up as
  a stall.
- While suspended with background work, the add-on calls `tick_background` every 200 ms without
  waiting for the host.

## Focus and keyboard

The host sends `FocusGained` and `FocusLost` and routes keys to the add-on only while the last frame
said `wants_keyboard`. Focus that arrives by Tab carries the direction; the add-on then focuses its
first (or last) widget. When Tab moves focus past the add-on's last (or first) widget, the frame
carries `focus_escape` and the host moves focus to its own next widget. The runtime places an
invisible focusable sentinel before and after the page, so the ends of egui's own traversal order
are exact: focus on the end sentinel is a forward escape, focus on the start sentinel a backward
one. The sentinel gives focus back at once, so the add-on never takes it back on a later frame.

## Designed for, not yet tested in a real host

These paths exist in the messages and in the runtime, but only #81 and the pilot (#88) can test
them against real OS input: text editing and selection, IME (preedit, commit, output rectangle),
clipboard round trip (copy out, paste in), drag released outside the view, focus and Tab routing
against real input, and high DPI other than 1.0 (`pixels_per_point` is clamped to
0.25 to 8).

## Host view

`src/addon_view/` is the host side of a page: `AddonView` paints the add-on's frames inside the page
rectangle and forwards input back. It does not start or watch processes; the supervisor does
the handshake, then hands the view an `AddonLink` (a shared sender plus the inbox of a reader
thread). `AddonLink::spawn` sends frames and repaint requests to the view and every other message
(pong, log, preferences, host requests, status items, tutorial, bye) to a side channel for the
supervisor. Pongs and logs never wake the host's UI.

- **Painting.** Meshes come out of `mesh::MeshCache`, get host texture ids and are translated to the
  page origin once per frame. A mesh the add-on sent as cached is not translated again, and an
  unchanged frame paints the translated meshes as they are. Each mesh is clipped to its own clip
  rectangle and the page.
- **Textures.** Each view owns its host textures (add-on texture id to host id) and frees all of
  them when it is dropped. On a restart the old textures stay until the new process paints its first
  frame, so the last frame can still be shown.
- **Input.** Positions are in points relative to the page's top-left corner; the batch carries
  `pixels_per_point`, the page size, the theme and window focus. Pointer events go while the
  pointer is over the page, or while a drag that started there is held: moves outside are forwarded
  and the release outside arrives as `DragReleasedOutside`. Keyboard, text, clipboard and IME events
  go while the page has the host's keyboard focus (a click inside takes it, a click outside drops
  it). While the page has focus, Tab and arrows belong to the add-on; when a frame carries
  `focus_escape`, the host moves focus to its own next (or previous) widget and sends `FocusLost`.
  Focus that arrives by Tab sends `FocusGained` with the direction.
- **Pacing.** One batch in flight, at most 60 a second, add-on repaint hints (`repaint_after_ms` and
  `RequestRepaint`) through `RepaintLimiter`. With no input and no hint the host neither sends nor
  repaints, so an idle add-on costs nothing.
- **Applied back.** The cursor icon while the pointer is over the page, copied text to the
  clipboard, links through the host's link handling (only `http`, `https` and `mailto`), and the IME
  rectangle moved to the page.
- **States.** `Connecting` until the first frame ("Starting <Name>…"), `Running`, `Stalled`
  (heartbeat from the supervisor, or a batch without a frame for `STALL_TIMEOUT`), `Restarting` and
  `Stopped`. Stalled, restarting and stopped show the last frame dimmed under a plain note, for
  example "<Name> is restarting…", never a blank page.

To try the view in a real window (debug only, not reachable from the sidebar), run
`cargo run --example addon_view_demo`. It runs a sample add-on on a thread over in-memory pipes and
has a "Restart add-on" button for the restart state.

## Supervisor

`src/supervisor/` starts, watches and stops add-on processes. It is not reachable from the app yet:
the add-on browser (#86) and the pilot add-on (#88) call it; until then its tests drive it with a
fake add-on exe (`examples/fake_addon.rs`).

- **When an add-on runs.** `open` starts it when its page is opened; `start_background` starts a
  background add-on without showing it. A page that is left gets `Suspend` (`hide`) and keeps its
  process; `Resume` follows when it is shown again. `stop` sends `Quit` and ends the process if it
  has not exited after 2 seconds.
- **The file check.** Before every start the exe is hashed and compared with the SHA-256 the
  installer recorded from the signed catalog (`active.json`, see `src/installer.rs`). The checked
  file is held open without write or delete sharing until the process runs, so it cannot be swapped
  in between. A wrong hash is final: the add-on is not started and is not retried.
- **The job object.** Every add-on process starts suspended, joins one job object and then runs.
  The job ends all its processes when its last handle closes, and only the host holds one: closing
  Toolboks, or a crash of Toolboks, ends every add-on and every child an add-on started.
- **Crashes.** An add-on that ends without being asked to, never connects, or stops answering for
  30 seconds after the heartbeat called it stalled, is restarted: at most 3 times per minute. After
  that it stays stopped with a plain reason until the user restarts it (`restart`, which starts the
  limit fresh). `report` gives a short plain-text report (state, exit code, the last 50 log lines
  the add-on sent) and `copy_report` puts it on the clipboard. Nothing is written to disk or sent.
- **Refusals are final.** A wrong file, a missing file, a protocol mismatch, a wrong token or a
  declined administrator prompt stop the add-on without a retry.
- **Administrator rights per add-on.** When an add-on that may elevate (`LaunchSpec::may_elevate`)
  sends `HostRequest::RestartElevated { page }`, only that add-on is restarted through the UAC
  prompt and then sent `Navigate { page }`; the host keeps running as it is. A declined prompt
  leaves it stopped, and the next start is a normal one. An elevated add-on is not in the job (the
  host may not have the right to put it there or to end it); it exits when the host closes the
  pipes, which `run_addon` treats as a clean close. This path has not been tried on a real PC yet.
- **Nothing blocks the UI.** Launches run on their own thread, writes go through a writer thread
  with a bounded queue, and `tick` only polls. While an add-on is shown, `tick` asks for a repaint
  once a second for the heartbeat ping; a hidden add-on is not pinged.
- **Events.** `tick` returns what changed, for the page to apply to its `AddonView`: `Connected`
  (attach the link from `take_link`), `Restarting`, `Stalled`, `Stopped`, and `Message` for the
  host services (preferences, status items, tutorial, other host requests).

The Restart and Report buttons on a stopped page are drawn by the page that hosts the view; they
come with the first real add-on page (#88).

## Host services

`src/addon_services.rs` is the host's side of what a built-in tool gets by calling the host. It
takes the supervisor's events (`Services::apply`) and is, like the supervisor, not reachable from
the app until the add-on pages exist (#86, #88). Everything is optional for an add-on, and what an
add-on sends is cut to fixed limits before the host keeps or shows it.

- **Status bar items.** `NativeAddon::status_items` → `StatusItems`. Shown while the add-on runs
  and is turned on; they disappear when it stops, restarts or is turned off. At most 8 items of 48
  characters.
- **Links.** `resolve_link("Title/Page", ...)` matches the title and the pages the add-on declares
  in its catalog entry (`pages`), case-insensitively. The result is: open (on that page, or the
  first page when the page is unknown), turned off (the overview with a message, as today), not
  installed but in the catalog (offer to install), or unknown.
- **Guided tours.** `start_tour` sends `TutorialQuery` and `TutorialStart`; the steps arrive with
  `Tutorial`. `show_step` sends `TutorialNavigate` (navigation only) and `TutorialTarget`; the
  add-on marks the anchor with `tutorial::mark` or `tutorial::region` as before and the runtime
  reports its rectangle with `TutorialAnchor`. The host draws the overlay, Next, Back and Cancel.
  While a tour runs the page calls `AddonView::set_interactive(false)`: no click, key or text
  reaches the add-on, so a tour cannot run an action. `finish_tour` ends it, also on Cancel.
- **Tray.** `NativeAddon::tray_items` → `TrayItems`; the host shows the entries of running,
  turned-on add-ons and sends `TrayAction` for the one the user chose (`NativeAddon::tray_action`).
  The tray's memory reading does not depend on any add-on. At most 8 entries per add-on.
- **Background add-ons.** A catalog entry with `background: true` is started at launch while the
  add-on is turned on (`background(...)` gives the list for `Supervisor::start_background`). A
  turned-off add-on is never started.
- **Preferences.** After `Connected` the host sends `Capabilities` (from the installed entry) and
  `LoadPreferences` with the add-on's own preferences. `Preferences { changed: true }` is written
  into `config.json` under the add-on's id only (`Effect::SaveConfig`); the host asks a background
  add-on with `SavePreferences`. At most 256 KiB.
- **Installed entries.** The installer keeps the catalog entry beside each installed version
  (`entry.json`), so the title, pages, capabilities and the background mark are known offline.
