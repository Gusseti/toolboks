# Add-on wire protocol

An out-of-process add-on is a separate executable that runs its egui page headless and sends
the host tessellated meshes. The host forwards input and paints those meshes inside the add-on's
page rectangle. This document describes protocol version **5**: version 3, plus health findings
in version 4 (`AddonMessage::Findings` and `HostMessage::CheckNow`), plus in version 5 a finding's
optional `why` and `evidence` (`AddonMessage::FindingsV5`). Version 3 and 4 add-ons keep working:
the host speaks the version recorded for the installed add-on (see "Handshake and versions"). Version 2
binaries must be rebuilt; `Hello` and `Refuse` remain frozen so old peers receive a plain update
message.

- New to this? Start with the [add-on author guide](ADDON-AUTHOR-GUIDE.md).
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

The host chooses the version. Each installed add-on's catalog entry records its
`protocol_version`; the host speaks that version, so an add-on built for an older protocol keeps
working on a newer host.

1. Before the exe is started, the host checks that the entry's version is one it speaks
   (`catalog::PROTOCOL_VERSIONS`, today `[3, 4, 5]`). Any other version is refused in the plain
   words below and the exe is never started.
2. The host sends `Hello { protocol_version, addon_id, software_version }` at the entry's version
   (`Hello::with_version`).
3. The add-on accepts a version from `MIN_PROTOCOL_VERSION` (3) up to its own `PROTOCOL_VERSION`
   (5). Any other version is refused: it sends `Refuse { protocol_version, reason }` with its own
   newest version and exits with `ProtocolError::VersionMismatch`.
4. Otherwise it answers with its own `Hello` (its id and the SDK version) **at the host's
   version**. `addon_handshake` returns that agreed version.
5. The host refuses (`Refuse`) an answer at any other version than the one it sent, or with
   another id.

The agreed version is the connection's. Neither side ever sends a message newer than it: the
SDK runtime sends findings only at 4 or later (`Findings` at 4, `FindingsV5` at 5 or later), and
the supervisor keeps the version per add-on (`Supervisor::protocol_version`), sends `CheckNow`
only at 4 or later (`Supervisor::send` returns false otherwise) and ignores `Findings` arriving on
a version 3 connection and `FindingsV5` on a version 3 or 4 connection (one line in the add-on's
report log). A runtime at 4 or later that receives `CheckNow` on a version 3 connection ends with
`ProtocolError::Unexpected`, as a real version 3 add-on would fail to read it.

**Why version 5 and not new fields under 4.** The wire format is postcard, which is not
self-describing: a struct is its fields in order, with no names and no lengths. A field added to
`Finding` changes the bytes of every `Findings` message, so a version 4 peer would misread a list
with the new fields (the next finding starts in the wrong place, and trailing bytes are refused),
and `#[serde(default)]` or `skip_serializing_if` cannot help (postcard has no "field absent").
So the version 4 layout is frozen as `FindingV4` (the body of `Findings`, golden bytes in
`tests/golden.rs`), and the extended `Finding` travels in the new `FindingsV5` message. The new
fields carry `#[serde(default)]` only so that JSON without them (fixtures, tools) still parses.

| Add-on | Host | Result |
|---|---|---|
| version 3 (released before protocol 4) | speaks 3 to 5 | Entry says 3: the host sends `Hello` 3, it runs as before, no findings. |
| version 4 | speaks 3 to 5 | Entry says 4: findings flow without `why` and `evidence`. |
| version 5 | speaks 3 to 5 | Entry says 5: findings flow with `why` and `evidence`. A version 5 exe under an entry that says 4 agrees on 4 and sends the version 4 layout. |
| version 5 | speaks 3 and 4 | The older host's catalog filter hides entries at 5, so it keeps the version it has. |
| version 4 | speaks only 3 | The older host's catalog filter hides entries at 4, so it keeps the version 3 add-on it has. A version 4 exe that does meet a version 3 host agrees on 3 and sends no findings. |
| version 3 exe under an entry that says 4 | any | The add-on refuses `Hello` 4: "Add-on speaks protocol 3, Toolboks speaks 4. …" |

The refusal text is the same on both sides: "Add-on speaks protocol X, Toolboks speaks Y. Update
the add-on or Toolboks." `Hello` (variant 0) and `Refuse` (variant 1) are frozen in both
directions, so any two versions can refuse each other in plain words. Any other change to a
message bumps `PROTOCOL_VERSION`. New messages are appended at the end of the enums, so every
older message keeps its variant index and its bytes; `sdk/toolboks-addon-proto/tests/golden.rs`
holds the version 3 bytes of every message and fails if one changes.

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
| `AdminResult { id, outcome }` | Informational helper result for `NativeAddon::admin_result`; never authorizes a write. |
| `AdminOperation { request, capabilities }` | Dedicated helper connection only (see below); rejected by the drawing runtime. |
| `TutorialTarget { target, reveal }` | The tour anchor to highlight, or none. `reveal` scrolls it into view once. |
| `CheckNow` | Version 4 and later. The user asked Home to check again; calls `NativeAddon::check_now`. Optional to honour. Never sent on a version 3 connection. |

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
| `HostRequest` | `AdminOperation(AdminRequest)` or `ReviewUpdate`. Legacy `RestartElevated` is refused; it never elevates a drawing process. |
| `StatusItems(items)` | The full set of status bar items; sent when it changes. |
| `Tutorial { steps, location }` | Answer to `TutorialQuery`. |
| `Log { level, line }` | A log line queued with `runtime::log`. |
| `Pong { nonce }` | Answer to `Ping`. |
| `Bye` | The add-on exits on its own. |
| `TrayItems(items)` | The full set of entries (`id`, `label`, `enabled`) the add-on offers in the host's tray menu; sent when it changes. Optional. |
| `AdminResult { id, outcome }` | Dedicated helper result only; the host ignores this on a drawing connection. |
| `TutorialAnchor { target, rect }` | The rectangle of the anchor asked for with `TutorialTarget`, in the view's points, or none while it is not visible. Sent before the frame it belongs to, when it changes. |
| `Findings(findings)` | Version 4. The full set of the add-on's health findings (`FindingV4`, see "Host services"); replaces the previous set. Sent when it changes, also from background ticks while suspended. Never sent on a version 3 connection; the host ignores it there. The SDK sends `FindingsV5` instead at 5; a host still accepts this one there. |
| `FindingsV5(findings)` | Version 5. The same as `Findings`, each finding with its optional `why` and `evidence`. Never sent on a version 3 or 4 connection; the host ignores it there. |

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
- **Page switches.** A page picked in the host (sidebar, header, a Home finding) goes as `Navigate`
  followed by an input batch (`addon_view::navigate`), because the add-on draws only in answer to
  input. `NativeAddon::navigate` needs no repaint request of its own.
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
  installer recorded from the saved catalog (`active.json`, see `src/installer.rs`). The checked
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
- **Refusals are final.** A wrong file, a missing file, a protocol mismatch or a wrong token
  stops the add-on without a retry. A declined administrator prompt cancels only the helper;
  its original drawing process keeps running.
- **Administrator actions.** The drawing process stays unelevated. An add-on with an elevation
  capability sends `HostRequest::AdminOperation`; the supervisor starts a separate instance of
  its checked executable with `--toolboks-admin-helper` through UAC. A saved catalog
  authorizes the helper's hash and capabilities before launch. It has separate pipes, token and
  handshake; the drawing link never attaches to it. The original UI keeps its process, view,
  preferences and heartbeat. Only one helper may be open per add-on. There is no automatic
  retry of a helper or operation. Declining UAC returns `Cancelled` to the original UI.
  Native drawing processes, bundled add-on UI initialization and the SDK drawing runtime
  refuse administrator rights: reopen Toolboks normally. Helper mode alone may elevate.
- **Nothing blocks the UI.** Launches run on their own thread, writes go through a writer thread
  with a bounded queue, and `tick` only polls. While an add-on is shown, `tick` asks for a repaint
  once a second for the heartbeat ping; a hidden add-on is not pinged.
- **Events.** `tick` returns what changed, for the page to apply to its `AddonView`: `Connected`
  (attach the link from `take_link`), `Restarting`, `Stalled`, `Stopped`, and `Message` for the
  host services (preferences, status items, tutorial, other host requests).

The Restart and Report buttons on a stopped page are drawn by the page that hosts the view; they
come with the first real add-on page (#88).

## Administrator helper boundary (version 3)

`AdminRequest { id: u64, operation: String, json: String }` carries an add-on-defined action
selector (1 to 80 ASCII letters, digits, `_` or `-`) and at most 32 KiB of untrusted parameters.
The host treats them as opaque data. No request field carries approval or preview text.
The add-on must choose an allowed operation, parse a strict parameter type and construct the
plain-language preview from independently checked state inside the helper.

The host passes `operation` and `json` through without a schema, within the limits above, so
the add-on's `prepare` must validate strictly: reject unknown keys, wrong types and empty
selections. The preview may be up to 8,000 bytes. The confirmation window shows about 2,500
characters before it scrolls and never truncates the text. Enable each Windows privilege only
for the step that needs it and remove it afterwards. Return per-step results in the
`Completed` json; the host does not rewrite them.

1. UI sends `HostRequest::AdminOperation(request)`.
2. Supervisor verifies the installed executable against the saved catalog and starts
   only its helper mode. The helper presents a new token and completes the usual v3 handshake.
3. Host sends exactly one `HostMessage::AdminOperation { request, capabilities }`, using the
   saved entry's capabilities (`administrator`, `drive_optimize` or `system_tuning`).
4. SDK checks that this process is elevated, validates bounds and calls read-only preparation.
   `AdminChange::capability` must match a granted capability. The prepared value owns the exact
   validated action and preview. No new host data is passed to `apply`.
5. SDK creates a native Windows dialog on the helper's thread, with no host HWND as parent.
   Windows reports the window's process id and the SDK requires its own process id. The dialog
   explains the change with **Confirm** and **Cancel**; Cancel has initial/default focus.
   Only that native dialog can authorize `AdminChange::apply`, once. No egui input is used.
6. Helper sends `AddonMessage::AdminResult { id, outcome }` and exits. Outcomes are
   `Completed { json }`, `Cancelled` or `Failed { reason }`. Host matches the request id and
   forwards an informational `HostMessage::AdminResult` to the unelevated UI.

The helper accepts no second operation, navigation, preferences, frames or approval messages.
Any additional message (including forwarded click/key input or a forged result) cancels its
pending dialog; disconnection does the same. Closing/restarting the UI drops its helper job
and connection. Cancellation before approval makes no system change. Once approved, an action
may need to finish safely; Windows drive maintenance may outlive the helper or Toolboks.

For still-bundled source providers, the same SDK loop runs through an early helper-only entry
in the host executable and the existing native-provider registry. It bypasses the host window,
single-instance guard and normal app initialization. No tool is moved out of the app here.
The host launcher and message handling contain no add-on operation names or execution logic.

Regression tests cover forwarded clicks/keys and forged results, Cancel, missing rights or
capabilities, host disconnect, wrong connection roles and v2 refusal. The native-window PID
ownership test is ignored by default because it opens a real window; run it only in an
owner-approved GUI session. The preview `admin_dialog` example can exercise the exact native
dialog without UAC or writes. GUI verification and the screenshot are pending, not run. A real UAC consent/decline and privileged drive operation still need a manual
Windows check; the automated tests do not prove that integration.

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
  (`entry.json`), so the title, pages, capabilities, the background mark and the protocol version
  are known offline.
- **Health findings (version 4; `why` and `evidence` version 5).** `NativeAddon::findings` →
  `Findings` at 4, `FindingsV5` at 5; the runtime diffs it like
  `status_items` and also sends it after background ticks, so a background add-on reports with its
  page closed. A `Finding` is `id` (1 to 48 characters of `a-z`, `0-9`, `.`, `-`, unique within
  the add-on), `title` (one line, at most 80 characters), `observation` (at most 240), `status`
  (`Ok`, `Attention`, `NotChecked`, `Unavailable`), `severity` (`Watch` or `Problem`, used only for
  `Attention`), `measured_at` (Unix seconds), `stale_after_secs` and at most one `action`:
  `OpenPage { label, route }` (a page of the same add-on; label at most 40) or
  `Instruction { text }` (at most 240). Version 5 adds `why` (why it matters, at most 200) and
  `evidence`: `Bar { fraction, left, right }` (a filled bar, 0 to 1) or
  `Series { values, left, right }` (at most 24 columns, oldest first, each 0 to 1), with labels of
  at most 40 that carry the numbers ("470 GB used" / "6 GB free"). The limits are constants in
  `toolboks-addon-proto` (`MAX_FINDINGS` = 16 per message, `MIN_STALE_AFTER_SECS` = 60,
  `MAX_STALE_AFTER_SECS` = 604 800, `MAX_FINDING_WHY_CHARS`, `MAX_EVIDENCE_LABEL_CHARS`,
  `MAX_EVIDENCE_VALUES`).
  The message carries no source: the host files every finding under the id of the connection it
  came from. `CheckNow` asks the add-on to check again (`NativeAddon::check_now`). The host keeps
  findings in memory only, never on disk (`src/health.rs`): each message replaces the add-on's
  set; a finding with a bad or repeated id (the first wins), an empty title or an `OpenPage`
  route whose first segment is not one of the add-on's catalog `pages` is dropped with a line in
  the add-on's report log; text is cleaned like status items, a `measured_at` in the future is
  taken as the receive time and `stale_after_secs` is clamped; `why` and the evidence labels are
  cut, evidence numbers are clamped to 0 to 1 (one that is not a number becomes 0; a series keeps
  its newest 24), and evidence without labels, with a bar that is not a number or a series without
  values is dropped (the finding stays). Findings stay after the add-on stops and go when it is
  turned off or removed. Home shows them as described in `DESIGN.md` ("Focus card").
