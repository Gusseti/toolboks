# Product

<!-- impeccable:product-schema 1 -->

## Platform

web (this record covers the Toolboks landing page; the app itself is native Windows, egui)

## Users

Ordinary Windows 11 users with an above-average interest in their system: people who open a small tool for a minute next to a game or work. Secondary: add-on authors who want to build on the SDK. *(Assumed from docs/ADDON-VISION.md; not yet confirmed by the owner for the landing page.)*

## Product Purpose

Toolboks is a platform for Windows system tools: one window, written in Rust and shipped as a single `toolboks.exe`, that tools plug into as add-ons. Every tool is an add-on, including the team's own (Memory Cleaner, Resource Monitor, Debloater, Event Watch, System Profiles, Disk Manager by stratum, Macro Manager, Audio Gainer); separating the original tools into add-ons is in progress (owner, 2026-11-14). Add-ons are found and installed through the add-on browser, which is meant to become a marketplace. Success for the landing page: a visitor understands that Toolboks is a home for tools, sees the verified tools on offer, and downloads it.

## Positioning

A platform, not a single utility: one window, one look and one set of rules for every tool. Tools made by the Toolboks team carry the **Toolboks verified** mark and are listed first; a package cannot claim that mark itself. Every add-on follows the honesty rules: measured numbers shown as measured, estimates labelled, limits stated, and anything that changes the system shows a preview, asks once and offers undo.

## Operating Context

Downloaded from GitHub Releases (`toolboks.exe` plus `.sha256`). Updates are checked once on launch against the latest stable release, verified by SHA-256 and swapped in place. Preferences stay in a local data folder. Admin rights are only requested for actions that need them.

## Capabilities and Constraints

- Current stable version: 0.16.0 (2026-11-16).
- Add-on SDK 0.4 developer preview. Signed publisher identity, the online catalog (marketplace) and remote add-on updates are the direction but not built yet; the landing page may show the add-on browser as the place to find tools, but must not claim third-party publishing is open today.
- Code signing (SignPath) is not done yet; SmartScreen warnings are likely.
- Whether the repository is public is an open decision in docs/PLAN.md.

## Brand Commitments

- Name: Toolboks (one word, capital T).
- DESIGN.md is the app's visual system: dark graphite, one purple accent `#8b5cf6`, plain language, no glow, no gradients.
- The owner asked for the landing page to be dark, sleek and intriguing, with some page effects (2026-11-14).
- logo.png is a cyan-lit toolbox; its cyan does not match the app's purple accent (unresolved).

## Evidence on Hand

- Design mockups: docs/design/h-expanded.png, h-collapsed.png, h-anatomy.png, h-tokens.png.
- CHANGELOG.md, README.md, MIT license.
- No user counts, download numbers, testimonials or press exist. Do not invent them.

## Product Principles

1. Plain words first, the raw number one click away.
2. Measured is measured, estimated says so.
3. Nothing changes the system without preview, confirmation and undo.
4. One window, one look, for every tool and add-on.
