# Instructions for AI coding assistants

This folder is a Toolboks add-on called "{{title}}" (crate and exe `{{name}}`, id `{{id}}`).
Read this file before changing anything. The human-facing guide is `README.md`.

## What an add-on is

Toolboks is a Windows app. A native add-on is a separate exe that Toolboks starts with its
stdin and stdout connected to Toolboks. The SDK (`toolboks_addon_sdk::runtime::run_addon`)
speaks the binary protocol over those pipes: Toolboks sends input, the add-on draws one egui
frame and sends back the shapes, and Toolboks shows them inside its own window. The add-on has
no window, no event loop and no console of its own. Toolboks may end the process at any moment.

## Files

| File | Purpose | May change | Must not change |
|---|---|---|---|
| `src/main.rs` | The add-on: a struct implementing `NativeAddon`, and `main` calling `run_addon`. | Everything marked `EDIT:`; new structs, functions and modules. | The `apply_style` call; `ID` without also changing the manifest and the test. |
| `{{name}}.tbaddon` | The manifest (JSON). | `title`, `description`, `long_description`, `author`, `publisher`, `category`, `accent`, `version` (with Cargo.toml). | Field names (unknown fields are refused); `api_version` 2; `pages` must stay `[]`; `capabilities` only when the human asks. |
| `icon.png` | Sidebar icon; only its alpha channel is used. | Replace it with another valid icon. | The rules under Invariants. |
| `tests/protocol.rs` | Checks the manifest and starts the built exe as Toolboks would. | `ID`, when the id changes. | Anything else. |
| `Cargo.toml` | The crate. The SDK comes from git at a fixed `rev`. | `version` (with the manifest); new dependencies that do not draw UI. | `rev` (unless asked), the SDK features, `[workspace]`, `[profile.release]`. |
| `.cargo/config.toml` | Static C runtime and safe DLL search. | Nothing. | All of it. |
| `README.md`, `AGENTS.md`, `CLAUDE.md` | Documentation. | Keep them true when the code changes. | |

## Invariants (the test or Toolboks refuses the add-on otherwise)

- `ID` in `src/main.rs` == `id` == `native_provider` in `{{name}}.tbaddon` == `ID` in
  `tests/protocol.rs`.
- `version` in `Cargo.toml` == `version` in the manifest, `major.minor.patch`.
- Manifest text limits: `title` 64, `description` 220 (one line), `long_description` 8000,
  `author` and `publisher` 100, `category` 60 characters; `accent` is `#RRGGBB`; `icon` stays
  `"custom"`.
- `icon.png`: PNG, square, 16 to 128 pixels, at most 32 KiB, shape in the alpha channel on a
  transparent background.
- Never write to stdout (`println!`, `print!`, `dbg!` to stdout, a library that prints). It
  corrupts the protocol. Use `toolboks_addon_sdk::runtime::log(LogLevel, line)`; `eprintln!` is
  allowed.
- Draw only through egui inside `ui()` (and `tick()`). No windows, dialogs or other UI crates.
- Keep `toolboks_shared::ui::apply_style(ui.ctx())` at the top of `ui()`.
- No threads that touch egui or the add-on struct. A worker thread may compute and send results
  over a channel; read the channel in `ui()` or `tick()` and call `request_repaint`.
- Saved settings are the JSON map passed to `ui()` and `load_preferences()`, under 256 KiB.
  Return `true` from `ui()` after changing it.
- Never consume or block Ctrl+C: copying any drawn text is done by the SDK.
- No administrator operations, no registry or system changes, no elevation. Those need the
  SDK's reviewed helper (`run_addon_with_admin`) and the human's decision.
- No network access and no file access outside the add-on's own data unless the manifest's
  `capabilities` list it and the human asked for it.
- Look like Toolboks: build from `toolboks_shared::ui` (`card`, `toggle_switch`, `switch_row`,
  `pill`, `banner`, `graph_card`, `header_cell`, `body_cell`, the `MUTED`/`ACCENT`/`TEXT_*`
  constants). Sentence case. Smallest text 11 px.

## Commands

Run in this folder (Windows, Rust stable, Visual C++ build tools, git):

```powershell
cargo build
cargo test
cargo build --release
```

A passing `cargo test` ends with `test result: ok. 2 passed; 0 failed` for `tests/protocol.rs`
(`the_manifest_belongs_to_this_add_on`, `the_add_on_connects_draws_and_quits`). Run `cargo test`
after every change. If it prints `skipped: this process is elevated`, the exe test did not run.
Also run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before you finish.

## The NativeAddon methods

These are all the methods of `toolboks_addon_sdk::NativeAddon` at the pinned SDK. Only `ui` is
required. Do not call or implement any other name; if you need something not listed, stop and
ask the human.

- `ui(ui, preferences) -> bool`: draw the page, once per frame while shown; return true after changing `preferences`.
- `load_preferences(preferences)`: read saved settings once after Toolboks connected.
- `save_preferences(preferences) -> bool`: report settings changed by background work.
- `status_items() -> Vec<StatusItem>`: short items for the Toolboks status bar.
- `tick(ctx)`: per-frame housekeeping before `ui`, while the page is shown.
- `suspend()`: the page was left; free memory only needed while visible.
- `resume()`: the page is shown again.
- `navigate(page)`: a link or the sidebar chose one of the pages listed in `host-services.json`.
- `page_index() -> usize`: index of the page being shown.
- `has_background_work() -> bool`: true while work continues with the page hidden (needs `"background": true` in `host-services.json`).
- `tick_background()`: called about every 200 ms while hidden and `has_background_work` is true.
- `cancel()`: Toolboks is ending the add-on; stop work safely.
- `set_capabilities(capabilities)`: what the user granted; do nothing that was not granted.
- `findings() -> Vec<Finding>`: health checks shown on Toolboks Home.
- `check_now()`: the user asked Home to check again.
- `tray_items() -> Vec<TrayItem>`: entries for the tray menu.
- `tray_action(id)`: the user chose one of those entries.
- `tutorial_steps() -> Vec<TutorialStep>`: the guided tour's steps.
- `tutorial_location() -> String`: the route of the page being shown, for the tour.
- `tutorial_start()`: a tour started; save transient UI state.
- `tutorial_navigate(page)`: the tour moved to a page; navigate only.
- `tutorial_finish()`: the tour ended; restore the saved state.
- `take_host_request() -> Option<HostRequest>`: ask Toolboks for one administrator operation.
- `prepare_admin(request)`: in the elevated helper only; validate and describe the change.
- `admin_result(id, outcome)`: the result of an administrator operation; informational only.

Multiple pages, background work, findings, tours and administrator actions all need more than
this file explains. Read https://github.com/Gusseti/toolboks/blob/main/docs/ADDON-AUTHOR-GUIDE.md
(and the SDK source at the `rev` in `Cargo.toml`) before using them.

## What not to do

- Do not edit `.cargo/config.toml`, `[profile.release]`, `[workspace]` or the SDK `rev`.
- Do not add UI crates (eframe, winit, iced, native-windows-gui, any webview) or a second egui.
- Do not change `tests/protocol.rs` except `ID`. Do not weaken or skip a test to make it pass.
- Do not invent SDK functions, types or manifest fields. If the compiler says one does not
  exist, it does not.
- Do not add `unsafe`, network calls, process launches or writes outside the add-on's data.

## When something is not supported

Say so plainly to the human: what they asked, why the SDK or these rules do not allow it, and
the closest thing that is possible. Do not work around a rule above without their explicit
answer. Publishing to the Toolboks add-on browser is not something you or the author can do; it
is done by the Toolboks owner.
