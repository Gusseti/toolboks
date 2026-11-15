# Local previews

Work on a local development branch. Editing, building, testing, and committing locally
do not change GitHub. Pushes and pull requests are separate, deliberate steps.

## Build and run

From the repository root, on Windows with Rust and its native build tools installed:

```powershell
cargo build --release --locked --features preview
Copy-Item target/release/toolboks.exe 'target/Toolboks Preview.exe'
& '.\target\Toolboks Preview.exe'
```

The `preview` feature provides a separate window title and single-instance identity,
so the preview and the stable app can run side by side. It disables automatic update
checks, the update controls, update installation, and old-executable cleanup. Existing
tools still function normally: system-changing actions affect the real PC if you choose
them. Use a disposable Windows VM for testing those actions.

No release version is bumped for local experiments. Host and external add-on settings
now persist locally. Preview data lives under `%LOCALAPPDATA%/Toolboks Preview`, separate
from stable data under `%LOCALAPPDATA%/Toolboks`. Set `TOOLBOKS_DATA_DIR` to a test folder
to isolate test runs further; Toolboks appends its channel folder to that directory.
Original native-tool settings remain session-only until migrated.

## Checks

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --features preview -- -D warnings
cargo test --locked --workspace --features preview
cargo build --release --locked --workspace --features preview
```

Also check the standard build before proposing changes. Do not run ignored real-system
tests or administrative actions as part of routine preview validation.

Development can stay entirely local until the preview is ready for review. Follow the
repository's staging/main release policy when publishing is explicitly requested.

## Checking the add-on browser in the real window

Preview builds can read a signed local test catalog and step through the add-on screens,
saving a screenshot of each. Release builds do not have this code.

```powershell
cargo build --features preview
cargo run --features preview --example marketplace_fixture -- C:\Temp\cat1 target\debug\hello-addon.exe 1.0.0
cargo run --features preview --example marketplace_fixture -- C:\Temp\cat2 target\debug\hello-addon.exe 1.1.0
$env:TOOLBOKS_TEST_CATALOG = "C:\Temp\cat1"
$env:TOOLBOKS_DATA_DIR = "C:\Temp\fresh-data"   # must not exist yet
target\debug\toolboks.exe --marketplace-check C:\Temp\shots C:\Temp\cat2 1
```

The run picks two tools on the first-run screen, reviews and installs them together, opens
them, then covers the browser, an update, both removal choices and a failed download. The
packages are harmless SDK samples with the tools' names, not the real tools.
`docs/reviews/86/` holds the last set and the comparison with the landing page.
