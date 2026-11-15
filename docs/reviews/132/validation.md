# Administrator helper validation (#132)

Windows, 2026-11-15. Branch `issue-132-admin-confirm`, PR base `staging`.

## Automated boundary checks

- The SDK runs one immutable, prepared action only after a native confirmation; Cancel
  does not call its write callback, and one confirmation calls it exactly once.
- A forwarded pointer click, Enter key, forged result, or host disconnect cannot confirm
  a pending operation. Input cannot serve as the initial helper request either.
- Missing elevation/capability, invalid selectors and oversized parameters fail closed.
- The drawing runtime rejects helper-operation messages.
- The native PID ownership test exists but is ignored by default: it opens a real Windows
  dialog. It must be scheduled with the owner and was not run in the final headless checks.
- The supervisor test refuses a helper without a saved catalog before UAC while retaining the original
  drawing process's PID and unelevated status. Existing catalog/hash launch checks remain.
- Catalog checks accept v3 and hide v2; the hello executable refuses a v2 handshake with the
  frozen plain-language update message.

## Dialog GUI check and screenshot: pending, not run

The owner explicitly deferred all GUI launches, screenshots, Computer Use and browser
work to avoid focus changes. The orchestrator will schedule GUI validation when the owner approves a
good time. No GUI work is part of the final checks; the native-window test is opt-in.

Before that instruction, the preview example was built and an unelevated native preview was
opened with an isolated `TOOLBOKS_DATA_DIR` (`work-132/preview-dialog-data`). Computer Use
refused app access, so there was no visual inspection, screenshot or interactive button check.
The preview process was stopped. This native `DialogBoxIndirectParamW` window cannot be
captured through egui's screenshot command. No PNG is attached or reconstructed from source.

Pending GUI checks: native helper/window PID ownership, appearance and keyboard focus,
Confirm and Cancel, and a dialog screenshot. The native ownership test can be run with
`--ignored` only after the owner authorizes GUI work. The preview example uses the same SDK
dialog and never performs a system change, even if Confirm is selected.

## Manual checks still required before publication

Real UAC consent and decline, native Confirm and Cancel in an elevated helper, host exit
while that dialog is open, and actual privileged drive analysis/defrag/TRIM were **not run**.
No real UAC prompt was automated or accepted, and no drive optimization was performed.
F4 is implemented with regression coverage; its real elevation integration remains a
release prerequisite. The existing interactive/network-dependent ignored tests stay ignored.

## Required checks

Final results are recorded below after the normal and preview workspace runs complete.
