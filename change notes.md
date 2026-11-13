# Unreleased

## Added

- **Macro Manager:** a verified installable add-on for recording and editing keystrokes, mouse movements, button presses/releases, clicks and scrolling inside Toolboks.
- **Mouse-path editor:** place and drag path points, add clicks at points, and use timed or instant movement. Millisecond/second timing scopes and custom inputs accept `ms`, `s` and `sec`.
- **Macro playback:** run once, repeat for a duration, or toggle repeating until stopped, with a configurable delay between completed runs. Configurable recording/playback shortcuts work across tools, and Esc stops active work.
- **Local macro library:** save named macros and settings locally; no recording or playback starts automatically. Recording excludes Toolboks control gestures, and stopping playback releases its held keys/buttons.
- **Audio Gainer settings:** optionally enable your microphone when Toolboks starts, even when another page opens first. The general startup option is off by default; a remembered per-microphone on/off state takes precedence when per-microphone settings are enabled.
- **Per-microphone profiles:** remember microphone on/off state, gain, listening volume, output devices and routing separately for each microphone. Restore activation when switching microphones and when Toolboks launches; saved routes resume with an enabled microphone.
- **Default microphone:** save your preferred microphone by name, keep it remembered while disconnected, and automatically switch back when it reconnects—even while using another tool.
- **Saved-preferences permission:** existing Audio Gainer installations can use Review update to approve saving these settings locally. Older packages remain supported without automatically receiving the new permission.

## Changed

- Removed the duplicate Refresh devices button. Kept Refresh microphones, which updates the microphone list without stopping active audio or resetting output choices.

## Fixed

- Manually disabling the microphone keeps it off without forgetting its saved routing setup. With per-microphone settings enabled, the disabled state is also remembered across app launches.
- Automatic microphone selection rejects ambiguous duplicate names rather than choosing an arbitrary device.
- Audio Gainer's settings walkthrough does not initialize device discovery, resume saved routes or write preferences merely by displaying the tutorial.
