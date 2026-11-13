# Unreleased

## Added

- **Macro Manager:** a verified installable add-on for recording and editing keystrokes, mouse movements, button presses/releases, clicks and scrolling inside Toolboks.
- **Mouse-path editor:** place and drag path points, add clicks at points, and use timed or instant movement. The full-width timing slider reaches 10 seconds with clickable checkpoints; edit its duration directly using `ms`, `s` or `sec`.
- **Macro playback:** use the prominent Play macro button, run once, repeat for a duration, or toggle repeating until stopped, with a configurable delay between completed runs. Each macro has its own saved activation key that triggers it even when another macro is selected. Recording shortcuts work across tools, and Esc stops active work.
- **Local macro library:** starts empty, requires a name for each new macro, and saves changes automatically. The last macro can be deleted. No recording or playback starts automatically; recording excludes Toolboks control gestures, and stopping playback releases its held keys/buttons.
- **Audio Gainer settings:** optionally enable your microphone when Toolboks starts, even when another page opens first. The general startup option is off by default; a remembered per-microphone on/off state takes precedence when per-microphone settings are enabled.
- **Per-microphone profiles:** remember microphone on/off state, gain, listening volume, output devices and routing separately for each microphone. Restore activation when switching microphones and when Toolboks launches; saved routes resume with an enabled microphone.
- **Default microphone:** save your preferred microphone by name, keep it remembered while disconnected, and automatically switch back when it reconnects—even while using another tool.
- **Saved-preferences permission:** existing Audio Gainer installations can use Review update to approve saving these settings locally. Older packages remain supported without automatically receiving the new permission.

## Changed

- Macro Manager uses one unified options bar with collapsible Record and Playback sections. Selecting a step shows its settings instead; the rename field is removed.
- Record Mouse captures movement, mouse buttons and wheel activity, including over Toolboks. Record keystrokes captures keyboard input separately; recording checkboxes are removed. The recording shortcut uses the most recently chosen recording mode. Only clicks on the mouse recorder's Stop button are excluded from mouse recordings.
- Macro Manager follows the approved editor layout: aligned full-height library and options panels, quiet full-width macro rows, spaced step controls, a separate canvas toolbar, and a full-width timing track with an editable duration. Record and Playback expand without indenting their controls; recorded paths show labelled start and end points.
- New macro creation happens inline: Give it a name, confirm with the checkmark or Enter, or cancel with X or Escape. White icons turn green or red on hover.
- Consecutive recorded mouse movements become one read-only Mouse movement step, with its full trajectory displayed. Coordinates, timing and input order are preserved, including older recordings grouped on load. Grouped recordings use a new local action format that older builds cannot read.
- The steps list, mouse editor, timing and macro controls remain visible with disabled actions when the library is empty, including after deleting its last macro.
- Step rows include editable delays and immediate deletion. Delete removes the selected step; deleting a macro requires confirmation. Add key starts key capture without inserting a default key. Selected buttons use readable light text.
- Removed the duplicate Refresh devices button. Kept Refresh microphones, which updates the microphone list without stopping active audio or resetting output choices.

## Fixed

- Esc stops mouse recording independently of keyboard-hook delivery and immediately refreshes the recorder controls.

- Macro activation and recording shortcuts preserve keypad keys instead of binding the number row. Keypad shortcuts are labelled explicitly; previously assigned keypad shortcuts need to be reassigned once.
- Shortcut assignment waits without a timeout for the first non-modifier key, with Ctrl/Shift/Alt/Windows supported in combinations. A dedicated key-state listener avoids recording-hook waits on the editor thread.
- Macro shortcuts wake the active editor immediately instead of waiting for its 200 ms background poll.
- Manually disabling the microphone keeps it off without forgetting its saved routing setup. With per-microphone settings enabled, the disabled state is also remembered across app launches.
- Automatic microphone selection rejects ambiguous duplicate names rather than choosing an arbitrary device.
- Audio Gainer's settings walkthrough does not initialize device discovery, resume saved routes or write preferences merely by displaying the tutorial.
