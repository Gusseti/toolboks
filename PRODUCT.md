# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The product itself is a native Windows desktop app (Rust, egui). This record's platform covers the web surfaces made for it, starting with the landing page.

## Stack

Delegated: static HTML, CSS and JavaScript in `site/`, no build step, deployable to GitHub Pages. Chosen because the repo is a Rust project with no web toolchain and the page needs nothing a framework adds. (The owner was offered the choice and did not pick; revisit if a framework is wanted.)

## Users

Ordinary Windows users with an above-average interest in their system: people who like to see and tune what their PC is doing without being administrators. They open Toolboks for a minute next to a game or work, not to stare at a dashboard. A secondary audience is add-on authors who build tools with the SDK.

## Product Purpose

Toolboks is a platform for system tools. It owns one calm window (sidebar, header, status bar, settings) and an add-on browser; the tools themselves are add-ons that are installed, turned off, updated and removed from that browser. The original built-in tools are being repackaged as add-ons so each appears in the browser and can be downloaded on its own. Success: a visitor understands that Toolboks is the frame and the tools are what they choose to put in it.

## Positioning

A platform, not a bundle. Every tool lives in the same window and follows one design system, so twenty tools feel like one product. First-party tools carry the **Toolboks verified** mark: verification comes from an allowlist embedded in the app, not from a flag a package can set on itself, and verified entries appear first in the browser. Every install and update shows the access a package asks for before it is granted.

## Operating Context

- Windows desktop. One self-updating `toolboks.exe` from GitHub releases, checked against a SHA-256 file before it replaces itself.
- Add-ons are `.tbaddon` packages (validated UTF-8 JSON, max 2 MiB). SDK API 1 renders declarative pages (headings, notes, metrics, preference controls); API 2 activates approved native providers compiled into the host.
- Installation review lists the package's capabilities (for example `host_metrics`, `local_preferences`, `live_audio`, `input_automation`, `system_tuning`). Updates require a higher version and keep preferences; uninstall removes them.
- Preferences and installed packages are stored locally in `%LOCALAPPDATA%/Toolboks/`.

## Capabilities and Constraints

Current first-party add-ons (publisher Toolboks unless noted):

- Memory Cleaner: free up memory and see where it goes.
- Resource Monitor: watch processor, graphics, memory, disk and network.
- Debloater: tune Windows privacy, apps, startup and background tasks.
- Event Watch: understand Windows events and crashes.
- Disk Manager by stratum: background disk analysis, space map, fragmentation reports, drive optimization (native provider).
- Macro Manager: record, edit and repeat keyboard and mouse actions.
- Audio Gainer: check your microphone level, hear yourself, route your mic to other apps.
- System Profiles: review and apply a Windows gaming profile with a restore point, registry backup and activity log.

Not yet true and must not be implied: an online catalog, remote add-on updates, independent third-party native binaries, cryptographic publisher signing. The add-on browser is local today. Guided tours (Help) exist in every workspace.

## Brand Commitments

- Name: Toolboks (lowercase `toolboks` only in code and file names).
- Visual system: DESIGN.md is binding for the app and every add-on.
- Logo: `logo.png` is a glowing cyan toolbox that conflicts with DESIGN.md. Undecided; the owner did not answer. Web surfaces use the app's quiet purple mark until this is settled.
- Voice: plain, non-technical, sentence case; estimates are labelled; never claim a guarantee the tool does not give.

## Evidence on Hand

- Design mockups: `docs/design/h-expanded.png`, `h-collapsed.png`, `h-anatomy.png`, `h-tokens.png`.
- Add-on manifests: `assets/*.json`, `assets/*.tbaddon`, `addons/disk-manager/*.tbaddon`, SDK example `sdk/examples/hello-system.tbaddon`.
- Release facts: version in `Cargo.toml` (0.12.5 at time of writing), MIT license, GitHub releases at `github.com/Gusseti/toolboks`.
- None: no testimonials, user counts, download numbers, press or benchmarks. Do not invent them.

## Product Principles

1. The platform is the product; tools are choices the user makes.
2. Show before change: access is reviewed at install, system changes preview, confirm and offer undo.
3. Trust is earned by mechanism, not badge: say exactly what "verified" means.
4. One window, one system: every tool looks and reads like Toolboks.
5. Plain words first, the detail one click away.

## Accessibility & Inclusion

Text at least 4.5:1, large text 3:1; nothing conveyed by colour alone; full keyboard reach with a visible focus ring (from DESIGN.md).
