# Macro Manager

Install **Macro Manager** from Add-ons, review its input-automation and local-preferences permissions, then open it in the Toolboks sidebar.

## Record and edit

The library starts empty. Choose New and type into Give it a name. Confirm with the white checkmark or Enter; cancel with X or Escape. The checkmark turns green on hover; X turns red. Changes save automatically; there is no Save button. Selecting a macro shows one options bar with independently collapsible Record and Playback sections. Selecting a step replaces those controls with that step's settings.

Edit delays or delete steps directly in the step list. Add key immediately waits for your key without inserting a placeholder. Delete removes the selected step without confirmation, or asks for confirmation when the macro itself is selected. The last macro can be deleted too.

Choose keystrokes, mouse movements, clicks and/or scrolls, then press Record. Stop recording with the same button, your configured shortcut or Esc. Toolboks control gestures and injected playback input are excluded from recordings. New recordings append to the selected macro.

Recorded delays preserve the elapsed time between captured actions. Fixed delays apply the chosen default between events. Custom delays retain recorded timings for individual editing in Selected step. Recorded key/button presses and releases stay separate so held keys and drags can be replayed.

## Mouse editor

Use Add path, then Draw path to place screen positions. Select and drag a point to adjust it. Add click places a click at the nearest point; the inspector edits its count. Movement uses a duration, not pixels per second; Instant uses zero movement duration.

Milliseconds covers 0–1,000 ms; Seconds covers 0–10 s with decimal values and whole-second marks. Click a checkpoint for an exact whole second or 100 ms. Drag between checkpoints for finer values, or edit the displayed duration directly using `ms`, `s` or `sec`. Bare numbers use the selected scope. There is no separate Custom timing field.

## Repeat and shortcuts

Play once, repeat for a duration, or repeat until stopped (toggle). The delay between repeats is a pause after each completed run; it does not speed up the actions within a run. Stop with the playback button, its configured shortcut or Esc. The run duration also stops active work when it expires.

Use Play macro in the Playback section to play the selected macro. Assign each macro its own activation key in the Playback controls; the library shows its saved key. An activation key runs its assigned macro even when another macro is selected. Press it again to stop that macro. Existing global playback shortcuts migrate to the previously selected macro.

The recording shortcut remains shared. Esc is reserved for stopping; shortcuts must be different and available in Windows. No input is recorded or played simply by opening Toolboks. Assigned shortcuts remain usable while working in another tool; active recording/playback continues without rendering Macro Manager's page.

Shortcut assignment uses the native Windows key, so `Keypad 4` and top-row `4` stay separate. Reassign any keypad shortcut saved by an older build, which stored it as a top-row number. With Num Lock off, Windows reports keypad navigation keys instead of numbers.

Click the shortcut button, then press your key or combination. Assignment has no timeout: Ctrl, Shift, Alt and Windows alone keep it waiting, and the first non-modifier key completes the combination. Esc cancels. Fn is generally handled by keyboard firmware and is not exposed as an independent Windows modifier.

Macros and preferences are stored locally with the add-on's namespaced preferences. Playback sends real input to the active Windows desktop; Windows may reject input to an application with higher privileges. Stop releases keys/buttons held by playback.

Recorded movements are grouped into one Mouse movement step per consecutive run. Their full trajectory is visible, with Recorded mouse movements are not editable shown in the editor. Geometry and internal timing remain read-only; delay before the run, reordering and deletion remain available. Clicks, scrolling and keys retain their sequence. Manual paths remain editable. Existing movement rows are grouped on load without discarding samples. The grouped local action format requires this build or newer when reopening saved recordings.
