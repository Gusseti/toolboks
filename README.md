# toolboks

A lightweight toolbox of system utilities, written in Rust.

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

The workflow fails if a tag and the `Cargo.toml` version disagree. Each release has two assets: `toolboks-vX.Y.Z-x86_64-pc-windows-msvc.exe` and the same name plus `.sha256`.

## Updates

On launch the app makes one request to GitHub for the latest stable release (pre-releases are ignored). If a newer version exists it asks whether to update; **Update now** downloads the exe into memory, verifies the SHA-256, swaps itself in place and restarts, and **Not now** keeps the **Install and restart** button in the sidebar. Offline or no release: it says nothing. There is also a **Check for updates** button. No files are left behind. It needs the repository to be public, since it cannot carry a token.

## Design rules

toolboks is one self-contained exe: it creates no folders, config, logs or caches, and keeps all state in memory only. See [docs/PLAN.md](docs/PLAN.md).

## License

[MIT](LICENSE)
