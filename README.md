# toolboks

A lightweight toolbox of system utilities, written in Rust.

## Branches

- `main` – stable; every stable release is cut from here.
- `staging` – integration branch; pre-releases (`-rc.N`) are cut from here. Merge `staging` into `main` when ready.

## Releasing

Versions follow [SemVer](https://semver.org) and live in `Cargo.toml`. A release is a `vX.Y.Z` git tag; pushing it triggers `.github/workflows/release.yml`, which builds `toolboks.exe` and publishes a GitHub release with a zip and SHA-256 checksum.

```powershell
.\scripts\release.ps1 0.2.0-rc.1   # from staging: pre-release
.\scripts\release.ps1 0.2.0        # from main: stable release
```

The script bumps `Cargo.toml`/`Cargo.lock`, commits, tags and pushes. The workflow fails if the tag and `Cargo.toml` version disagree. Release assets are named `toolboks-vX.Y.Z-x86_64-pc-windows-msvc.zip`, which is the layout the `self_update` crate expects for auto-update.
