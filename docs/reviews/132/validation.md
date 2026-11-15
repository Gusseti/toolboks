# Administrator helper validation (#132)

Windows, 2026-11-15. Branch `issue-132-admin-confirm`, PR base `staging`.

## Automated boundary checks

- The SDK runs one immutable, prepared action only after a native confirmation; Cancel
  does not call its write callback, and one confirmation calls it exactly once.
- A forwarded pointer click, Enter key, forged result, or host disconnect cannot confirm
  a pending operation. Input cannot serve as the initial helper request either.
- Missing elevation/capability, invalid selectors and oversized parameters fail closed.
- The drawing runtime rejects helper-operation messages.
- A real Windows dialog is created by the SDK test; `GetWindowThreadProcessId` equals the
  calling process. A cancelled connection closes that actual native dialog through its timer.
  This window-ownership check does not invoke UAC or prove the elevated launch integration.
- The supervisor test refuses an helper without a saved catalog before UAC while retaining the original
  drawing process's PID and unelevated status. Existing catalog/hash launch checks remain.
- Catalog checks accept v3 and hide v2; the hello executable refuses a v2 handshake with the
  frozen plain-language update message.

## Native preview and screenshot limitation

Built `cargo build --locked --features preview --example admin_dialog` and started that
executable with the isolated `TOOLBOKS_DATA_DIR`:
`C:/Users/fredh/.t3/scratch/2026-11-14-i-need-to-be-able-d4df7aba/work-132/preview-dialog-data`.
The Windows window inventory returned the executable's actual window titled
**Confirm administrator change**. This example opens the same SDK dialog and never performs
an administrator action, even if Confirm is selected.

Computer Use rejected app access with **"Computer Use was not approved to use admin_dialog"**.
No screenshot, visual inspection, or interactive Confirm/Cancel check could be obtained.
The owner explicitly approved documenting the screenshot limitation and proceeding. This is
an OS-native `DialogBoxIndirectParamW` window, so egui's `ViewportCommand::Screenshot` does
not capture it. The preview process was stopped after the access refusal; no button was
claimed as exercised. No PNG is attached or reconstructed from source.

## Manual checks still required before publication

Real UAC consent and decline, native Confirm and Cancel in an elevated helper, host exit
while that dialog is open, and actual privileged drive analysis/defrag/TRIM were **not run**.
No real UAC prompt was automated or accepted, and no drive optimization was performed.
F4 is implemented with regression coverage; its real elevation integration remains a
release prerequisite. The existing interactive/network-dependent ignored tests stay ignored.

## Required checks

Final results are recorded below after the normal and preview workspace runs complete.
