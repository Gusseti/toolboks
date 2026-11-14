# Issue #86 handoff - 2026-11-14

Stopped at the owner's request. Branch: `feat/addon-browser-86`, based on `staging`. No PR has been opened and issue #86 must remain open for orchestrator visual verification. Last pushed implementation before this handoff: `3c26b67`.

## Implemented

- Runtime-catalog marketplace with search, category and installed filters, detail pane, screenshots, access and release notes; landing-page-derived wide rows, icon tiles, verified pills and typography.
- Catalog-based first-run picks with nothing preselected and owner-approved ordering. Multiple installs receive one grouped review; existing catalog verification and installer are reused.
- Install queue/progress/cancel/retry; failures persist across restart until dismissed. Update is the primary action when available. Removal retains data unless "Also delete its data" is checked.
- Installed native add-ons open inside Toolboks through the existing supervisor/view. Local `.tbaddon` developer installation and a browser guided tour remain available.
- Fresh startup loads no builtin manifests or bundled packages; embedded legacy package constants are test-only. Ordering metadata does not provide first-run catalog records.
- Preview-only signed local fixtures and native-window screenshot automation, including first-run installation/opening of two tools, corrupt-download failure, removal choices and update flows.

## Catalog configuration

Single production constant: `src/catalog.rs:31`, `pub const URL`, currently `https://raw.githubusercontent.com/fredhakon/toolboks-addons/main/catalog/index.json`. Signature is adjacent `index.json.sig`; executable assets come from catalog release URLs. Local fixtures and signing keys are preview/test-only, not production fallback content.

## Visual specification and evidence

- Exact source/CSS token table: `visual-spec.md`; implementation primarily `src/app/store.rs`.
- Comparison folder: `C:/Users/fredh/.t3/scratch/2026-11-14-i-need-to-be-able-d4df7aba/work-86-build/compare/`.
- `compare/ref-browser.png` is an 884px-wide live landing-page capture showing Disk Manager; earlier `ref-disk.png` and `initial-side-by-side.png` are exploratory and may include sticky website navigation.
- Native app captures: `docs/reviews/86/native-100/01-first-run.png` through `16-updated-open.png`, plus `browser-crop.png` and `result.txt`. These are actual native-window captures, not HTML mockups.
- Last inspected app crop was 884x608 versus reference 884x597. The latest search-layout adjustment has not been visually confirmed. Latest runner selects Event Watch, so final reference/app selections still need aligning.
- `compare/differences.md` and a final matched-width `compare/app-*.png` pair are NOT complete. Visual parity is NOT certified.

## Verification and known issues

- Latest preview workspace build and workspace/all-targets clippy (`-D warnings`) passed; logs are preserved in the root.
- Last full preview tests: 477 passed, 3 failed, 12 ignored. The three removed-builtin assumptions were subsequently adjusted, but the full suite was not rerun. See `test-preview.log`; do not report a clean suite yet.
- Normal non-preview build/test/clippy remain unrun. Final fmt validation remains outstanding.
- Earlier native automation completed all 16 screens. The final 100%/125%/200% batch was interrupted on the stop instruction and may have partially overwritten the 100% set. Do not claim all scales passed or assume result.txt describes the latest files.
- At handoff inspection, no process with an executable path under this checkout remained running.
- Previously checked production prerequisites: `fredhakon/toolboks-addons` is private with zero catalog entries; production verification public keys remain zero placeholders. Publish signed catalog/release assets and configure real offline keys before release; never bypass verification.
- Legacy tool implementation source remains for extraction work (#88 through #94), although no builtin tools are loaded by fresh startup.
- Browser preview timed out twice; reference capture used Playwright with installed Edge instead.

## Remaining work (only after renewed authorization)

1. Review this WIP and establish fresh, same-width, same-selection reference/app screenshots in `compare/ref-*.png` and `compare/app-*.png`; complete native 100%/125%/200% checks.
2. Write `compare/differences.md`, compare side by side, fix remaining visible differences, and explicitly justify only unavoidable egui rendering differences. Landing page overrides the old sketch for the browser; document resolved differences.
3. Rerun normal and preview build/test/clippy/fmt; resolve remaining task-related failures and record exact commands/results.
4. Document reproducible fresh-data, signed-local-catalog installation and opening of two tools without leaving the app; distinguish preview fixtures from production add-ons.
5. Confirm catalog publication, release assets and real verification keys with owner; retain catalog-only startup and the five owner decisions.
6. Only when authorized, open a PR to `staging` referencing #86, attach native evidence and record catalog constant location and follow-ups. Do NOT close #86; the orchestrator verifies visual match first.

## Files touched

Implementation: `Cargo.toml`, `Cargo.lock`, `src/addons.rs`, `src/app.rs`, `src/app/marketplace.rs`, `src/app/marketplace_check.rs`, `src/app/shell.rs`, `src/app/store.rs`, `src/app/tutorial.rs`, `src/app/tutorial/catalog.rs`, `src/catalog.rs`, `src/installer.rs`, `src/main.rs`, `src/marketplace.rs`, `examples/marketplace_fixture.rs`.

Assets/docs/evidence: `assets/fonts/OFL.txt`, `assets/fonts/SchibstedGrotesk-{400,500,600,700}.ttf`, `assets/fonts/SchibstedGrotesk-variable.ttf`, `visual-spec.md`, `change notes.md`, `docs/reviews/86/native-100/*`, `compare/*`, root `build-fixture.log`, `build-preview.log`, `check.log`, `clippy-preview.log`, `test-preview.log`, and this handoff.

External fixture/data/log workspace (not part of this commit): `C:/Users/fredh/.t3/scratch/2026-11-14-i-need-to-be-able-d4df7aba/validation-86`.

Cargo environment used: `CARGO_HOME=D:/CodexBuilds/aural-20261103/cargo`, `RUSTUP_HOME=D:/CodexBuilds/aural-20261103/rustup`; prepend `$env:CARGO_HOME/bin` to PATH.
