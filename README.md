# toolboks

A lightweight toolbox of system utilities, written in Rust. One exe, nothing written to disk.

## Tools

- **Memory Cleaner**: one-click Optimize memory (gentle, no admin needed), a colored "Where your memory goes" bar grouped by app, automatic cleaning with a start-percentage slider and an options panel, and an Advanced page with the process list, protection, system-wide cleaning and auto-clean rules.
- **Resource Monitor**: a Task Manager style Overview (CPU with live clock, GPU, memory, disk, network, top processes) and a Details page (per-core CPU, GPU engines, per-drive disk, per-adapter network), with live graphs.

## Branches

- `main` – stable; every stable release is cut from here.
- `staging` – integration branch; pre-releases (`-rc.N`) are cut from here. Merge `staging` into `main` when ready.

## Releasing

Versions follow [SemVer](https://semver.org) and live in `Cargo.toml`.

**Normal way:** bump `version` in `Cargo.toml` (and `Cargo.lock`), merge to `main`. `.github/workflows/release.yml` sees a version with no release yet, builds `toolboks.exe` and publishes release `vX.Y.Z`. A push to `main` that does not change the version releases nothing.

**Pre-releases and tagging by hand:** `scripts/release.ps1` bumps the version, commits, tags and pushes; a pushed `vX.Y.Z-rc.N` tag publishes a pre-release (which the in-app updater ignores).

```powershell
.\scripts\release.ps1 0.2.0-rc.1   # from staging: pre-release
.\scripts\release.ps1 0.2.0        # from main: stable release
```

The workflow fails if a tag and the `Cargo.toml` version disagree. Each release has exactly two files: `toolboks.exe` (what you download and run) and `toolboks.exe.sha256` (its checksum). Versions 0.1.0 and 0.2.0 used long file names and cannot update themselves to a later release; download `toolboks.exe` once from the latest release.

## Updates

On launch the app makes one request to GitHub for the latest stable release (pre-releases are ignored). If a newer version exists it asks whether to update; **Update now** downloads the exe into memory, verifies the SHA-256, swaps itself in place and restarts, and **Not now** keeps the **Install and restart** button in the sidebar. Offline or no release: it says nothing. There is also a **Check for updates** button. No files are left behind. It needs the repository to be public, since it cannot carry a token.

## Design rules

toolboks is one self-contained exe: it creates no folders, config, logs or caches, and keeps all state in memory only. See [docs/PLAN.md](docs/PLAN.md).

## License

[MIT](LICENSE)
