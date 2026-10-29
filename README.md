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

The script bumps `Cargo.toml`/`Cargo.lock`, commits, tags and pushes. The workflow fails if the tag and `Cargo.toml` version disagree. Each release has two assets: `toolboks-vX.Y.Z-x86_64-pc-windows-msvc.exe` and the same name plus `.sha256`.

## Updates

The app has a **Check for updates** button. It asks GitHub for the latest stable release (pre-releases are ignored), downloads the exe into memory, verifies the SHA-256, swaps itself in place and restarts. It never contacts the network until you click, and it leaves no files behind. It needs the repository to be public, since it cannot carry a token.

## Design rules

toolboks is one self-contained exe: it creates no folders, config, logs or caches, and keeps all state in memory only. See [docs/PLAN.md](docs/PLAN.md).

## License

[MIT](LICENSE)
