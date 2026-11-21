# Make a Toolboks add-on

This guide takes you from nothing to a working add-on: create it, draw a page, save a setting,
test it, and see what publishing involves. The first four steps need no account and no
knowledge of the host's code.

## What an add-on is

A native add-on is a small Windows program (one `.exe`) that draws its page with
[egui](https://docs.rs/egui). Toolboks starts it, shows its page inside the Toolboks window,
and sends it the mouse and keyboard. The add-on never opens a window of its own. Because it is
its own program, a crash or a slow add-on cannot take Toolboks down, and Toolboks can end it at
any time.

There is also a code-free kind, a **declarative** `.tbaddon` (a JSON file with headings, notes,
live readings and toggles). If that is enough for you, read [sdk/README.md](../sdk/README.md).
Everything below is about native add-ons.

You will touch three things:

| File | What it is |
|---|---|
| `src/main.rs` | The add-on: a struct that implements the `NativeAddon` trait. |
| `<name>.tbaddon` | The manifest: name, description, icon, accent colour, what it may do. |
| `icon.png` (optional) | Your own sidebar icon, when the manifest says `"icon": "custom"`. See "Icons" below. |
| `tests/protocol.rs` | A test that starts the built exe and checks it connects, draws and quits. |

## What you need

- Windows, [Rust](https://rustup.rs) (stable) and the Visual C++ build tools.
- A clone of the add-ons repository, `fredhakon/toolboks-addons`. Real add-ons live there, one
  folder each under `addons/`. It already depends on the SDK, so you add nothing.
- For looking at your add-on in a window: a clone of the host repository, `Gusseti/toolboks`
  (step 4).

## 1. Create it

Copy the smallest add-on, `hello`, and give the copy your name. The name is lowercase letters,
digits and hyphens (`counter`, `disk-manager`). The folder, the crate and the exe all use it.

```powershell
git clone https://github.com/fredhakon/toolboks-addons
cd toolboks-addons
Copy-Item -Recurse addons\hello addons\counter
Rename-Item addons\counter\hello.tbaddon counter.tbaddon
```

Then change the name in four places. In each file, search for `hello`:

1. `addons/counter/Cargo.toml`: `name = "counter"`.
2. `addons/counter/counter.tbaddon`: see step 2.
3. `addons/counter/src/main.rs`: the id in `run_addon(…, "toolboks.counter")`.
4. `addons/counter/tests/protocol.rs`: every `hello` and `toolboks.hello` (the file name in
   `include_bytes!("../counter.tbaddon")`, the id the test expects, and `CARGO_BIN_EXE_counter`).

`addons/*` is already a workspace member, so there is no list to add it to. A new crate changes
`Cargo.lock`, and the repo's commands run with `--locked`, so run **one** command without it
first:

```powershell
cargo metadata --format-version 1 > $null
```

Then build and test it. This works before you have written anything:

```powershell
cargo build -p counter
cargo test -p counter
```

The test starts your exe, connects to it as Toolboks would, checks it draws a frame and answers
a heartbeat, and asks it to quit. It opens no window.

## 2. Fill in the manifest

`counter.tbaddon` is JSON. Toolboks reads it to describe your add-on, and the validator rejects
a field it does not know.

```json
{
  "schema_version": 1,
  "manifest": {
    "id": "toolboks.counter",
    "title": "Counter",
    "native_provider": "toolboks.counter",
    "description": "Counts button presses and remembers the number.",
    "long_description": "A small example add-on. It keeps one number and does not change your system.",
    "version": "0.1.0",
    "author": "Your name",
    "publisher": "Your name",
    "category": "System tools",
    "icon": "grid",
    "accent": "#3ecf8e",
    "api_version": 2,
    "min_toolboks_version": "0.12.5",
    "license": "MIT",
    "capabilities": []
  },
  "pages": []
}
```

| Field | Rule |
|---|---|
| `id`, `native_provider` | Lowercase dot-separated names, digits and hyphens. Both are the same text. First-party add-ons use `toolboks.<name>`; for your own use something like `com.yourname.counter`. The id is how Toolboks knows the add-on: it refuses an exe that answers to another one. |
| `title` | Up to 64 characters. The name in the sidebar. |
| `description` | One plain sentence, up to 220 characters. |
| `long_description` | Up to 8000 characters. Say what it does and what it changes. |
| `version` | `major.minor.patch`, and the same as `version` in `Cargo.toml`. A test checks it. |
| `author`, `publisher` | Up to 100 characters. |
| `category` | Free text up to 60 characters. The list groups add-ons by it, for example `System tools`, `Storage`, `Developer tools`. |
| `icon` | `custom` for your own picture (`icon.png`), or one of the twelve names the host draws: `grid`, `activity`, `layers`, `storage`, `wrench`, `leaf`, `macro`, `sliders`, `chip`, `shield`, `mic`, `gauge`. See "Icons" below. |
| `accent` | `#RRGGBB`. |
| `api_version` | `2` for a native add-on. |
| `min_toolboks_version` | Leave `0.12.5` when you use one of the twelve named icons; use `0.21.0` (the first Toolboks that accepts `"icon": "custom"`) when you ship your own `icon.png`. The catalog entry is raised to the first Toolboks that speaks the SDK's protocol automatically. If you set a newer one, change the host version string in the first test of `tests/protocol.rs` to match. |
| `capabilities` | What the user must allow before installing. `[]` for an add-on that only draws a page and keeps its own settings. Anything else (reading files, administrator actions, audio) needs a reason and is reviewed; see the table in [sdk/README.md](../sdk/README.md). |
| `pages` | Must be `[]` for a native add-on. Its pages are drawn by your code. |
| Optional | `byline`, `website`, `support_url`, `screenshots` (HTTPS, at most 8), `data_note` (one line about data you keep on the PC), `checks` (names of the health checks you report, see below). |

**Icons.** The sidebar is scanned by icon, so no two add-ons in the catalog may share one. The
first-party add-ons use the named icons; a new add-on brings its own: set `"icon": "custom"` and
put a file named `icon.png` beside `<name>.tbaddon`. The rules, checked before the picture is
read:

- PNG, at most 32 KiB.
- Square, 16 to 128 pixels on a side (64 x 64 is a good choice).
- Only the transparency (alpha channel) counts: it is the shape. Draw your shape in any colour on
  a transparent background. Toolboks paints it in the colour the other icons have in the same
  place (muted in the sidebar, accent when selected, the add-on's accent on its card), so it looks
  like the rest. Leave a little empty margin, as the built-in icons do; the picture fills an 18 to
  22 pixel square.

The file goes to users through the catalog entry (`icon_url`, `icon_sha256`, `icon_size`; see
[catalog.md](catalog.md)), and Toolboks checks it like the exe: an address on GitHub, the exact
size and SHA-256, and the rules above. If anything is wrong, the default glyph is drawn instead
and the add-on still installs.

## 3. Write the page

Replace `src/main.rs`. This is the complete add-on: a heading, a button, a saved number and a
status bar item.

```rust
// No console window in a release build. The add-on never prints to stdout: use `runtime::log`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use toolboks_addon_sdk::{runtime::run_addon, NativeAddon, StatusItem};

/// The id Toolboks knows this add-on by. It must equal `id` in `counter.tbaddon`.
const ID: &str = "toolboks.counter";
/// The one saved preference. The host stores it under this add-on's id.
const COUNT: &str = "count";

#[derive(Default)]
struct Counter {
    count: u64,
    styled: bool,
}

impl NativeAddon for Counter {
    /// Called once after the host connected, before the first frame.
    fn load_preferences(&mut self, preferences: &BTreeMap<String, serde_json::Value>) {
        self.count = preferences
            .get(COUNT)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
    }

    /// Draws the page. Return true when a saved preference changed.
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        if !self.styled {
            toolboks_shared::ui::apply_style(ui.ctx());
            self.styled = true;
        }
        ui.heading("Counter");
        ui.label(format!("Pressed {} times.", self.count));
        if ui.button("Add one").clicked() {
            self.count += 1;
            preferences.insert(COUNT.into(), self.count.into());
            return true;
        }
        false
    }

    /// Shown in the status bar at the bottom of the Toolboks window.
    fn status_items(&self) -> Vec<StatusItem> {
        vec![StatusItem {
            text: format!("Counter {}", self.count),
            tooltip: None,
        }]
    }
}

fn main() {
    if let Err(error) = run_addon(Counter::default(), ID) {
        eprintln!("Counter stopped: {error}");
        std::process::exit(1);
    }
}
```

Four things to know:

- **`ui` is the only method you must write.** Everything else on `NativeAddon` has a default that
  does nothing. The full list is in the reference table at the end.
- **`ui` runs when something happened, not on a timer.** The host sends input; you draw one
  frame. To animate or show new data, call `ui.ctx().request_repaint()` or
  `request_repaint_after(duration)`; the host limits how often.
- **Saved settings are a JSON map.** Write to `preferences` and return `true`; the host saves
  them under your id and gives them back in `load_preferences` the next time. Keep them under
  256 KiB.
- **Copying is free.** The user points at any text or number you draw and presses Ctrl+C. You
  write no code and add no button for it. Do not draw text outside egui, and do not swallow
  Ctrl+C.

## 4. See it

`cargo test -p counter` already proves the add-on connects, draws and quits. To look at it and
use it, run it in the test host. It is an example in the host repository; build it from a clone
of `Gusseti/toolboks`, giving it the full path to your exe:

```powershell
cd path\to\toolboks
cargo run --example addon_host -- path\to\toolboks-addons\target\debug\counter.exe toolboks.counter
```

A window opens with your page. The bottom line stands in for the Toolboks status bar. Press
the button and the count and the status item go up. **Restart add-on** ends your process and
starts it again, and the count comes back, because the test host hands your saved settings
back. Try Tab through your page and out of it, text fields, copy and paste, and dragging the
window to a display with another scale.

## 5. Add what your add-on needs

Each of these is one more method on `NativeAddon`. Look at the first-party add-ons in
`addons/` for a worked example of each.

- **More than one page.** Implement `navigate(page)` and `page_index()`, and list the page names
  in `addons/<name>/host-services.json`: `{"pages": ["Overview", "Details"]}`. The names are what
  `--open "Title/Page"` links use. Leave the file out for a single page.
- **Work while the page is hidden.** Add `"background": true` to `host-services.json`, then
  implement `has_background_work()` and `tick_background()` (called every 200 ms while hidden).
  Without it, leaving your page ends your process.
- **Health findings on Home.** Implement `findings()`; see "Health findings" below, and list the
  check names in the manifest's `checks`.
- **Tray menu entries.** `tray_items()` and `tray_action(id)`.
- **A guided tour.** `tutorial_steps()`, `tutorial_location()` and friends, with
  `tutorial::region(ui, id, |ui| …)` around each part. A tour only navigates; it never runs an
  action.
- **An administrator action.** Start from `runtime::run_addon_with_admin` and read "Administrator
  actions" below. The UI always stays unelevated, and every change is shown in a Confirm dialog
  the user approves.

## 6. Make it look like Toolboks

Every add-on follows [DESIGN.md](../DESIGN.md) and passes the
[design checklist](ADDON-DESIGN-CHECKLIST.md). In practice:

- Call `toolboks_shared::ui::apply_style(ui.ctx())` once, as the example does. That gives your
  process the host's fonts, colours and spacing.
- Build pages from `toolboks_shared::ui`: `card`, `toggle_switch`, `switch_row`, `pill`,
  `banner`, `graph_card`, `stacked_bar`, and the table helpers `header_cell` and `body_cell`.
  The same module holds the tokens (`ACCENT`, `MUTED`, `GREEN`, `AMBER`, `RED`, the `TEXT_*`
  sizes, the spacing constants). No palette, font, corner radius or icon style of your own; the
  smallest text is 11 px.
- Sentence case, plain words, one primary button per view, estimates labelled as estimates.
- A change to the system shows a preview, asks once and offers undo.

## 7. Rules that come from being a separate program

- **Never print to stdout.** Use `runtime::log(level, line)`; the host keeps the last lines for
  a bug report. `eprintln!` is fine while developing.
- **Your process can be ended at any time**: when Toolboks closes, when the user turns the
  add-on off, after a crash of Toolboks. Keep files consistent at every moment, not only on
  exit. Put files under the add-on data directory.
- **Draw only when asked** (see step 3).
- **Stay small.** `budgets.json` in the add-ons repo limits the first frame to 1000 ms and idle
  memory to 25 MB. Check yours with
  `cargo build --release -p counter` then `cargo run -p addon-repo -- measure counter`.
- **The release exe must not need DLLs.** The workspace's `.cargo/config.toml` already links the C
  runtime statically; keep it.
- What the host does with a misbehaving add-on: one that does not connect within 10 seconds or
  stops answering for 30 seconds is ended; one that crashes is restarted at most 3 times per
  minute and then stays stopped with a message, a Restart action and a report the user can copy.

## 8. Check before you share it

In the add-ons repository:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --locked -p counter
cargo build --locked --release -p counter
$env:ADDON_TEST_EXE = (Resolve-Path target\release\counter.exe).Path
cargo test --locked -p counter --test protocol
cargo run --locked -p addon-repo -- validate
```

The `ADDON_TEST_EXE` run tests the release exe, the one users get. `validate` checks every
manifest and the catalog list as the host would.

## 9. Publish

A release reaches every user, so it is done by the repository owner, not by the add-on's author.
Only first-party add-ons are published today; publishing by others needs its own plan (see
[ADDON-THREAT-MODEL.md](ADDON-THREAT-MODEL.md)).

What a release is, so you know what you are handing over:

1. The add-on's version is raised in `Cargo.toml` and `<name>.tbaddon` and merged to `main`.
2. The exe is built from `main` and uploaded to a GitHub release with its SHA-256.
3. A catalog entry is added (`addon-repo catalog-update`). The app reads the list from the
   `main` branch, so a catalog change is live for everyone about a minute after it lands.

The catalog entry states, among other things:

| Field | Meaning |
|---|---|
| `id`, `title`, `short_description`, `publisher`, `category`, `icon`, `accent` | What the add-on browser shows. |
| `icon_url`, `icon_sha256`, `icon_size` | Only with `"icon": "custom"`: where `icon.png` is and exactly what it must be. |
| `kind` | `native` for an exe, `declarative` for a `.tbaddon`. |
| `version`, `min_toolboks_version` | Plain `x.y.z`. An update must have a higher version. |
| `protocol_version` | The wire protocol your exe speaks (5 with the current SDK). Toolboks speaks the version in the entry, so it must match the exe. |
| `download_url`, `size`, `sha256` | Where the exe is (an HTTPS address on GitHub), and exactly what it must be. Toolboks places nothing that does not match. |
| `capabilities` | What the add-on may do, shown to the user before installing. |
| `pages` | Your page names, from `host-services.json`, so `--open "Title/Page"` links work. |
| `background` | `true` if the add-on keeps working while hidden; Toolboks then starts it at launch. |
| `verified`, `channel` | Set by whoever publishes the catalog. Nothing checks them. |

The full format is in [catalog.md](catalog.md). The catalog is a plain list and is not signed.

## Reference

### The `NativeAddon` trait

Only `ui` is required.

| Method | When it is called | Use it for |
|---|---|---|
| `ui(ui, preferences) -> bool` | Once per frame while your page is shown | Draw the page. Return `true` when you changed `preferences`. |
| `load_preferences(preferences)` | After the host connected | Restore saved settings. |
| `save_preferences(preferences) -> bool` | When the host asks | Report settings changed by background work. |
| `navigate(page)`, `page_index()` | A link or the sidebar chose a page | Add-ons with more than one page. |
| `suspend()`, `resume()` | Your page was left or shown again | Release memory you only need while visible. |
| `has_background_work()`, `tick_background()` | Every 200 ms while hidden | Work that continues while hidden. |
| `status_items()` | After every message | Short items for the status bar. |
| `tray_items()`, `tray_action(id)` | After every message; when the user picks one | Optional entries in the tray menu. |
| `findings()` | After every message and background tick, also while hidden | Health checks for Home (protocol 4). Sent only when they change. |
| `check_now()` | The user pressed "Check again" on Home | Optional: check again when you can. |
| `tutorial_steps()`, `tutorial_location()`, `tutorial_start()`, `tutorial_navigate(page)`, `tutorial_finish()` | During a guided tour | Describe the tour. Mark the parts with `tutorial::mark` or `tutorial::region`. |
| `set_capabilities(capabilities)` | After the host connected | Learn what the user granted. Do nothing that was not granted. |
| `take_host_request()` | After every message | Ask for the package review or one administrator operation. |
| `prepare_admin(request)` | Dedicated elevated helper only | Validate and return a read-only preview plus its exact action. |
| `admin_result(id, outcome)` | Original unelevated process | Show the result; never use it as permission to write. |
| `tick(ctx)` | Every frame while your page is shown | Per-frame housekeeping with the egui context. |
| `cancel()` | Before the add-on exits | Stop work safely. |

### Health findings

Home shows what the installed add-ons measured: no score and no "your PC is healthy". Each
check you make is one `Finding`, built with `Finding::ok`, `Finding::attention(.., Severity)`,
`Finding::unavailable` or `Finding::not_checked`, plus `.stale_after(secs)` and at most one action
(`.open_page(label, route)` or `.instruction(text)`). Findings need protocol 4: the runtime sends
them only to a host that speaks it, and an older host never sees them. Protocol 5 adds two
optional parts that Home shows on the finding it puts first: `.why(text)` (why it matters, at
most 200 characters) and `.evidence(Evidence::bar(fraction, left, right))` or
`.evidence(Evidence::series(values, left, right))`, a small picture of what you measured. A host
at protocol 4 gets the finding without them.

- **Build a finding when you measure, keep it, and return a copy from `findings()`.** The builders
  stamp the current time, so a finding built inside `findings()` changes every second and is sent
  again each time. Keep the check itself a pure function (`fn check(input) -> Finding`) and test
  it on made-up input, as `sdk/examples/hello-addon` does.
- `id` is your own stable name for the check (`memory-pressure`, `drive-health-c`): 1 to 48
  characters of `a-z`, `0-9`, `.` and `-`, unique within your add-on.
- Plain text only, nothing is interpreted: `title` one line of at most 80 characters,
  `observation` at most 240, an `OpenPage` label at most 40, an `Instruction` at most 240. Longer
  text is cut. At most 16 findings per add-on.
- `Ok` says what was measured ("No crashes recorded in the last 7 days"), never "healthy PC".
- `NotChecked`: you could check but have not yet (the first read is still running, the page was
  never opened). `Unavailable`: you cannot check on this PC; say why ("Needs administrator
  rights", "This drive does not report health").
- `Severity::Problem` is for harm to the PC or its data now or soon (a drive "Replace soon", blue
  screens). `Severity::Watch` is for "slows the PC or worth a look" (memory pressure,
  throttling, XMP off). Severity counts only for `Attention`.
- `stale_after_secs`: how long the reading stays current, 60 seconds to 7 days. After that Home
  shows it as out of date, never as OK.
- One action at most, and never one that changes the system from Home. `OpenPage` leads to one of
  your own pages (its first segment must be one of your catalog `pages`, or the finding is
  dropped), where your usual preview, confirm and undo apply. `Instruction` is for something only
  the user can do (BIOS, Windows Settings).
- A background add-on (`background: true`) reports with its page closed: `findings()` is also
  read after every `tick_background`. A foreground add-on reports only while its page is open.
- Evidence is drawn in the finding's status colour, so the two labels must carry the numbers
  ("470 GB used" / "6 GB free", "10 min ago" / "now"), at most 40 characters each. A bar's
  `fraction` and each of a series' values (at most 24, oldest first) are 0 to 1. Evidence without
  labels is dropped.
- Findings stay on the PC: the host keeps them in memory only.

### Administrator actions

Administrator actions use protocol 3 or later and a separate helper. The add-on's entire UI must
stay unelevated. Use `runtime::run_addon_with_admin(create_ui, ID, prepare)` as your entry point;
it selects helper mode before creating UI state. The `hello-addon` sample uses this entry point
and refuses administrator operations, because its counter needs no administrator rights.

- From an explicit action, return `HostRequest::AdminOperation(AdminRequest { id, operation,
  json })` from `take_host_request`. Use a unique id while work is pending; keep parameters under
  32 KiB. Disable duplicate actions until `admin_result` arrives. Tours must never enqueue an
  operation.
- `prepare` runs only in the helper. Match a fixed operation name, parse a strict typed payload,
  validate current OS state, and return `Box<dyn admin::AdminChange>`. Do not change anything
  while preparing. `capability()` names a granted elevation capability; `description()` says
  exactly what will change and any undo limit. Never accept a command line, preview or approval
  flag from the host. Keep the validated target inside this value.
- `AdminChange::apply(self: Box<Self>)` is called once, only after the SDK's native Confirm
  button. Revalidate mutable OS state and refuse if the target changed during confirmation.
  An operation is one reviewed change, not an open-ended script or a batch of future commands.
- Handle `AdminOutcome::{Completed, Cancelled, Failed}` in `admin_result`. This callback is
  informational; privileged writes belong only in `apply`. Cancel makes no change. Closing the
  host cancels a pending dialog; approved work may need to finish safely.
- Installed helper launches require the saved catalog's hash and elevation capability.
  `RestartElevated` no longer restarts an interface; update code using that legacy variant.

The development `addon_host` refuses helper requests for development executables. Inspect the
actual dialog without UAC or writes with a preview build:

```powershell
$env:TOOLBOKS_DATA_DIR = "$PWD\target\admin-dialog-preview-data"
cargo run --features preview --example admin_dialog
```

For real elevation, install a checked add-on from a saved catalog. Manually test both UAC
consent and decline, native Confirm and Cancel, closing the host during confirmation, and the
absence of administrator rights in the drawing process. Never automate the UAC prompt.

### When something goes wrong

| What you see | Why |
|---|---|
| `cannot update the lock file … because --locked was passed` | A new crate changes `Cargo.lock`. Run `cargo metadata --format-version 1` once without `--locked`. |
| The first test fails with "needs Toolboks x.y.z or later" | The manifest's `min_toolboks_version` is newer than the host version string in `tests/protocol.rs`. Raise the string or lower the minimum. |
| The test says the id differs | `ID` in `main.rs`, `id` and `native_provider` in the manifest, and the id in `tests/protocol.rs` must all be the same text. |
| `Unknown sidebar icon` | `icon` is not `custom` or one of the twelve names above. |
| The add-on exits at once on a CI runner | Hosted Windows runners run as administrator, and the drawing runtime refuses to. A normal PC is fine. |
| Toolboks says the add-on did not connect | It has 10 seconds to open its pipes and answer. Look for a panic before `run_addon`, or a slow start-up. |

### More reading

- [ADDON-PROTOCOL.md](ADDON-PROTOCOL.md): the wire protocol, the connection, the supervisor and
  the host services.
- [ADDON-ARCHITECTURE.md](ADDON-ARCHITECTURE.md): how the pieces fit.
- [GUIDED-TOURS.md](GUIDED-TOURS.md): tours in detail.
- [ADDON-THREAT-MODEL.md](ADDON-THREAT-MODEL.md): what the platform defends against.
- [sdk/examples/hello-addon](../sdk/examples/hello-addon/src/main.rs): a longer sample with a
  health finding, a tour and a saved setting.
