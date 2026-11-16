# Add-on author guide

How to make a Toolboks add-on that is its own program: create it, build it, try it in the test
host, and what publishing will involve. You need Windows, Rust with its native build tools, and
a checkout of this repository. No account is needed.

There are two kinds of add-on:

- **Declarative** (`.tbaddon`): a JSON file with headings, notes, live readings and toggles. No
  code. Start with [sdk/README.md](../sdk/README.md) if that is enough for you.
- **Native**: a separate exe that draws its own page with [egui](https://docs.rs/egui) and is
  shown inside the Toolboks window. This guide is about native add-ons.

Toolboks installs native add-on executables through the catalog and starts them through
the supervisor. The test host below is useful for local drawing checks without publishing.
Disk Manager is the pilot in `toolboks-addons/addons/disk-manager`; it uses protocol 3 and the
administrator helper for its drive actions.

## 1. Run the sample

From the repository root:

```powershell
cargo build -p hello-addon
cargo run --example addon_host -- target\debug\hello-addon.exe example.hello
```

A window opens with the sample's page: a heading, a line of text, an **Add one** button and a
count. The bottom line stands in for the Toolboks status bar and shows `Hello 0`. Press the
button: the count and the status item go up, and the terminal prints the saved preferences and a
log line. Press **Restart add-on**: the add-on is started again and still shows the count, because
the test host gave its preferences back.

The sample is [sdk/examples/hello-addon](../sdk/examples/hello-addon/src/main.rs), about a
hundred and fifty lines, with one health finding as the example. Its test
(`cargo test -p hello-addon`) starts the built exe and checks that it draws a frame, reports its
finding at protocol 4 and none at protocol 3, and quits when asked.

## 2. Create your own

Copy `sdk/examples/hello-addon` to a new folder, add it to `members` in the root `Cargo.toml`
(or keep it in a repository of your own and point the two path dependencies at this one), and
change three things:

1. The package name in its `Cargo.toml`.
2. `ID` in `main.rs`: a lowercase id such as `com.yourname.yourtool`. It is the name Toolboks
   knows the add-on by; the host refuses an add-on that answers with another id.
3. The `NativeAddon` implementation.

`NativeAddon` is the whole contract. Only `ui` is required:

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
| `findings()` | After every message and background tick, also while hidden | Health checks for Home (protocol 4; see "Health findings" below). Sent only when they change. |
| `check_now()` | The user pressed "Check again" on Home | Optional: check again when you can. |
| `tutorial_steps()`, `tutorial_location()`, `tutorial_start()`, `tutorial_navigate(page)`, `tutorial_finish()` | During a guided tour | Describe the tour. Mark the parts with `tutorial::mark` or `tutorial::region`. |
| `set_capabilities(capabilities)` | After the host connected | Learn what the user granted. Do nothing that was not granted. |
| `take_host_request()` | After every message | Ask for the package review or one administrator operation. |
| `prepare_admin(request)` | Dedicated elevated helper only | Validate and return a read-only preview plus its exact action. |
| `admin_result(id, outcome)` | Original unelevated process | Show the result; never use it as permission to write. |
| `cancel()` | Before the add-on exits | Stop work safely. |

Rules that come from being a separate program:

- **Never print to stdout.** Use `runtime::log(level, line)`; the host keeps the last lines for
  a bug report. `eprintln!` is fine while developing.
- **Your process can be ended at any time**: when Toolboks closes, when the user turns the
  add-on off, after a crash of Toolboks. Keep files consistent at every moment, not only on exit.
- **Draw only when asked.** The runtime calls `ui` when the host sends input. To animate or show
  new data, call `ctx.request_repaint()` or `request_repaint_after`; the host limits how often.
- **A tour step navigates, it never runs an action.** The host also forwards no clicks or keys
  to your page while a tour runs.
- **Preferences are yours alone.** The host saves them under your add-on's id. Keep them small
  (the host accepts at most 256 KiB).

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
  it on made-up input, as the sample does.
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

## 3. Make it look like Toolboks

Every add-on follows [DESIGN.md](../DESIGN.md) and passes the
[add-on design checklist](ADDON-DESIGN-CHECKLIST.md). In practice:

- Call `toolboks_shared::ui::apply_style(ui.ctx())` once, as the sample does. That gives your
  process the host's fonts, colours and spacing.
- Build pages from the shared helpers in `toolboks_shared::ui` (`card`, `toggle_switch`, `pill`,
  `graph_card`, `stacked_bar`) and the tokens there. No palette, font, corner radius or icon
  style of your own.
- Sentence case, plain words, one primary button per view, estimates labelled as estimates.
- A change to the system shows a preview, asks once and offers undo.

## 4. Test it

- `cargo test -p <your add-on>`: copy the sample's `tests/stdio.rs`. It starts your exe like the
  test host does and lets you send input and read frames without a window.
- `cargo run --example addon_host -- <your exe> <your id>`: look at it and use it. Try the
  keyboard (Tab through your page and out of it), text fields, copy and paste, and a window
  dragged to a display with another scale.
- The test host talks to your add-on over stdin and stdout. Toolboks itself uses two named pipes
  that only the current Windows user can open, a launch token and a check of your exe's SHA-256
  before every start. `run_addon` handles both ways; nothing changes in your code.

What the host does with a misbehaving add-on, so you know what to expect: an add-on that does not
connect within 10 seconds or stops answering for 30 seconds is ended; one that crashes is
restarted at most 3 times per minute and then stays stopped with a message, a Restart action and
a report the user can copy.

## 5. Package and publish

A native add-on is published as one exe. Build it small:

```powershell
cargo build --release -p <your add-on>
```

The workspace's release profile already optimises for size (`opt-level = "z"`, LTO, stripped).
The exe must not need DLLs beside it; the workspace links the C runtime statically and uses
`/DEPENDENTLOADFLAG:0x800` on MSVC to load implicit DLL dependencies from System32 only. Copy
both settings from `.cargo/config.toml` when building outside this workspace. Explicit DLL
loads must use System32 or an absolute trusted path. Installed version folders hold only
`addon.exe` and `entry.json`; put mutable files under the add-on data directory. Extra files,
subdirectories or redirected paths make the supervisor refuse a start.

Leaving a non-background add-on page ends its process; opening it again starts a new one.
Only add-ons declared as `background` in the catalog keep a suspended process while hidden.

Administrator actions use protocol 3 and a separate helper. The add-on's entire UI must stay
unelevated. Use `runtime::run_addon_with_admin(create_ui, ID, prepare)` as your entry point;
it selects helper mode before creating UI state. The hello sample uses this entry point and
refuses administrator operations, because its counter needs no administrator rights.

- From an explicit action, return `HostRequest::AdminOperation(AdminRequest { id, operation,
  json })`. Use a unique id while work is pending; keep parameters under 32 KiB. Disable duplicate
  actions until `admin_result` arrives. Tours must never enqueue an operation.
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
  Still-bundled providers implement `NativeAddon::prepare_admin` and use the same SDK boundary.

The development `addon_host` speaks v3 but deliberately refuses helper requests for development
executables. Inspect the actual dialog without UAC or writes with a preview build:

```powershell
$env:TOOLBOKS_DATA_DIR = "$PWD\target\admin-dialog-preview-data"
cargo run --features preview --example admin_dialog
```

For real elevation, install a checked add-on from a saved catalog. Manually test both UAC
consent and decline, native Confirm and Cancel, closing the host during confirmation, and the
absence of administrator rights in the drawing process. Never automate the UAC prompt.

To reach users, the add-on needs an entry in the catalog. An entry states, among other
things:

| Field | Meaning |
|---|---|
| `id`, `title`, `short_description`, `publisher`, `category`, `icon`, `accent` | What the add-on browser shows. |
| `kind` | `native` for an exe, `declarative` for a `.tbaddon`. |
| `version`, `min_toolboks_version` | Plain `x.y.z`. An update must have a higher version. |
| `protocol_version` | The wire protocol your exe speaks: 4 with the current SDK, 3 for exes built before it. Toolboks speaks the version in this entry, so it must match the exe (an exe built for 3 refuses a host at 4). An entry at 4 needs a Toolboks that speaks 4: older ones hide it, so set `min_toolboks_version` to the first such release. Rebuild all v2 binaries. |
| `download_url`, `size`, `sha256` | Where the exe is (an HTTPS address on GitHub), and exactly what it must be. Toolboks places nothing that does not match. |
| `capabilities` | What the add-on may do, shown to the user before installing. |
| `pages` | Your page names, so `--open "Title/Page"` links work. |
| `background` | `true` if the add-on keeps working while hidden; Toolboks then starts it at launch. |
| `verified`, `channel` | Set by whoever publishes the catalog. Nothing checks them. |

The full format is in [catalog.md](catalog.md). The catalog is a plain list and is not signed.

The first-party repository, release workflow (#83) and catalog browser (#86) are available.
Releases and catalog entries still require the owner/orchestrator. Only first-party add-ons are published for
now; publishing by others needs its own plan (see
[ADDON-THREAT-MODEL.md](ADDON-THREAT-MODEL.md)).

## Reference

- [ADDON-PROTOCOL.md](ADDON-PROTOCOL.md): the wire protocol, the connection, the supervisor and
  the host services.
- [ADDON-ARCHITECTURE.md](ADDON-ARCHITECTURE.md): how the pieces fit, and what is built.
- [GUIDED-TOURS.md](GUIDED-TOURS.md): tours in detail.
- [ADDON-THREAT-MODEL.md](ADDON-THREAT-MODEL.md): what the platform defends against.
