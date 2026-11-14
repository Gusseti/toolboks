# Add-on wire protocol

An out-of-process add-on is a separate executable that runs its egui page headless and sends
the host tessellated meshes. The host forwards input and paints those meshes inside the add-on's
page rectangle. This document describes protocol version **1**.

- Messages and framing: crate `sdk/toolboks-addon-proto` (`toolboks-addon-proto`).
- Add-on side loop: `toolboks_addon_sdk::runtime` (SDK feature `runtime`), entry points
  `run_addon(addon, addon_id)` and `run_addon_on(transport, addon, addon_id)`.
- Host side view: #81. Process launch, named pipes and their ACLs: #80.

## Framing

Every message is a little-endian `u32` body length followed by a [postcard](https://docs.rs/postcard)
body. The body length is capped at `MAX_FRAME_BYTES` (16 MiB); the reader checks the cap before it
allocates. A body with trailing bytes is malformed. Every failure is a `ProtocolError`, never a
panic, and every error is fatal for the connection (the stream position is unknown afterwards):
the host ends the add-on, the add-on exits.

A stream that ends at a message boundary is a clean close (`ProtocolError::Closed`, which
`run_addon_on` turns into `Ok(())`). A stream that ends inside a message is `Truncated`.

The transport is two independent byte streams, one per direction (`Transport<R: Read, W: Write>`).
Nothing in the protocol depends on stdin and stdout. Until #80 lands, `run_addon` opens
`--pipe-in <path>` and `--pipe-out <path>` (or `TOOLBOKS_ADDON_PIPE_IN` and
`TOOLBOKS_ADDON_PIPE_OUT`) and falls back to stdin and stdout when neither is given. In the
fallback the add-on must never print to stdout.

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
  `pacing::RepaintLimiter`, so two of them are at least one frame interval apart. The add-on sends
  at most one `RequestRepaint` per input batch and ignores repaint requests made during a pass,
  because those are already in that frame's hint.
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
carries `focus_escape` and the host moves focus to its own next widget. egui wraps focus, so the
escape is detected from widget positions (forward to a widget above, or left on the same row, is a
wrap).

## Designed for, not yet tested in a real host

These paths exist in the messages and in the runtime, but only #81 and the pilot (#88) can test
them against real OS input: text editing and selection, IME (preedit, commit, output rectangle),
clipboard round trip (copy out, paste in), drag released outside the view, focus and Tab routing
including the escape heuristic, and high DPI other than 1.0 (`pixels_per_point` is clamped to
0.25 to 8).
