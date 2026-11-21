# {{title}}: get started

This folder is a complete Toolboks add-on. It already builds, passes its test and shows a small
page with a button. Make it yours with the five edits below.

A native add-on is a small Windows program (one exe). Toolboks starts it, shows its page inside
the Toolboks window and sends it the mouse and keyboard. It never opens a window of its own.

## What you got

| File | What it is |
|---|---|
| `src/main.rs` | The add-on: draws the page, keeps one saved setting, shows a status bar item. |
| `{{name}}.tbaddon` | The manifest: id, name, description, author, icon and colour. |
| `icon.png` | The sidebar icon: a placeholder shape you replace. |
| `tests/protocol.rs` | A test that starts the exe as Toolboks would and checks it connects, draws and quits. |
| `Cargo.toml` | The Rust project. The Toolboks SDK comes from GitHub at one fixed commit. |
| `.cargo/config.toml` | Builds an exe that needs no extra DLLs. Leave it as it is. |
| `.gitignore` | Keeps the `target` build folder out of git. |
| `README.md` | This page. |
| `AGENTS.md` | Instructions for an AI coding assistant working in this folder. |
| `CLAUDE.md` | Points Claude at `AGENTS.md`. |

Every place you are expected to change is marked with a comment that starts with `EDIT:`.
Search for it.

## The five edits, in order

1. **Name it.** In `{{name}}.tbaddon` set `title`, `description` (one sentence),
   `long_description` (what it does and what it changes on the PC), `author` and `publisher`.
   The id is `{{id}}`. If you change it, change it in three places, all the same text: `id` and
   `native_provider` in `{{name}}.tbaddon`, `ID` in `src/main.rs`, and `ID` in
   `tests/protocol.rs`. Use something only you would pick, like `com.yourname.{{name}}`.
2. **Your icon.** Replace `icon.png` with your own: a PNG with a transparent background,
   square, 16 to 128 pixels on a side (64 x 64 is good), at most 32 KiB. Only the transparency
   counts: it is the shape. Toolboks paints the shape in its own colours, like its other icons.
   Leave a little empty margin around it.
3. **Draw your page** in `ui()` in `src/main.rs`. Use egui and the helpers in
   `toolboks_shared::ui` (`card`, `toggle_switch`, `switch_row`, `pill`, `banner`, the colour and
   text size constants). `ui()` runs when something happened, not on a timer; to show new data
   call `ui.ctx().request_repaint_after(...)`.
4. **Saved settings.** Read them in `load_preferences()`. To save one, put the new value in
   `preferences` inside `ui()` and return `true`. They are a JSON map, kept by Toolboks under
   your id. Keep them small (under 256 KiB).
5. **The status bar item** in `status_items()`. Return an empty list if you want none.

## Commands

You need Windows, [Rust](https://rustup.rs) (stable), the Visual C++ build tools and git. Run
these in this folder:

```powershell
cargo build                      # builds target\debug\{{name}}.exe
cargo test                       # checks the manifest and starts the exe as Toolboks would
cargo build --release            # the small exe you share: target\release\{{name}}.exe
```

The first build downloads the SDK and creates `Cargo.lock`; keep that file.

To test the release exe: `$env:ADDON_TEST_EXE = (Resolve-Path target\release\{{name}}.exe).Path`
then `cargo test`, then `Remove-Item Env:ADDON_TEST_EXE`.

**See it in a window.** Toolboks has a test window for add-ons. Clone
`https://github.com/Gusseti/toolboks`, and in that clone run:

```powershell
cargo run --example addon_host -- C:\full\path\to\{{name}}\target\debug\{{name}}.exe {{id}}
```

Your page opens in a window. **Restart add-on** ends your exe and starts it again; your saved
settings come back.

## Rules

- Never print to stdout: Toolboks talks to your exe over it. Use
  `toolboks_addon_sdk::runtime::log` (stderr is fine while you develop).
- Your exe can be ended at any time (Toolboks closed, the add-on turned off). Keep your files
  consistent at every moment, not only on exit.
- Draw only with egui, inside `ui()`. No windows, no other UI library.
- Copying is automatic: the user points at any text you draw and presses Ctrl+C. Add no copy
  code and never swallow Ctrl+C.
- Keep `toolboks_shared::ui::apply_style` at the top of `ui()` so the page looks like Toolboks.
- No administrator actions, except through the SDK's helper (`run_addon_with_admin`, see the
  full guide). The page itself never runs as administrator.

## Not possible yet

Publishing to the Toolboks add-on browser is done by the Toolboks owner; there is no way yet
for others to publish. For now, share the release exe and `{{name}}.tbaddon` directly.

## More

The full guide, with multiple pages, background work, health findings for Home, guided tours
and administrator actions:
https://github.com/Gusseti/toolboks/blob/main/docs/ADDON-AUTHOR-GUIDE.md
