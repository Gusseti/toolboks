# Add-on platform: threat model and security review

Issue #95. This covers the platform that downloads add-ons and runs them as separate programs:
the signed catalog (`shared/toolboks-catalog`, `src/catalog.rs`), the installer
(`src/installer.rs`), the supervisor and its pipes (`src/supervisor/`), the host view and host
services (`src/addon_view/`, `src/addon_services.rs`), and local `.tbaddon` packages
(`src/addons.rs`). It was written from a read of that code on 2026-11-14. Nothing here was tested
by attacking a running build.

Each threat ends in one of three states:

- **Covered**: the code stops it, and a test shows it.
- **Accepted**: it is not stopped, and the reason it is tolerable is given.
- **Open**: it is not stopped and should be before the first public add-on release. These are
  collected under "Findings" with a proposed fix.

## What is protected

1. The user's PC: no program runs that the Toolboks team did not publish.
2. Administrator rights: a program without them must not get them through Toolboks.
3. The user's choices: an add-on does what its listed access says, and no more is granted quietly.

## Who is trusted

- **The catalog signing key.** Whoever holds it decides what every Toolboks installs. Owner
  decision (2026-11-14, in #95): an offline ed25519 key held by the owner, never in CI. A second
  offline recovery key can only name a new main key.
- **The host exe.** It holds the public keys and makes every check. A replaced host makes all of
  this meaningless, so the self-update path (#13, #14) is part of this model.
- **First-party add-ons.** Only the team's own add-ons are published for now. They run with the
  user's rights and are not sandboxed.
- **Programs already running as the same Windows user are not defended against.** Such a program
  can read and write everything Toolboks can. Windows does not separate programs of one user, and
  Toolboks does not try to. The exception is administrator rights (see threat 4 and 6).

## Threats

### 1. A tampered catalog or download

Someone changes `index.json`, an add-on file on GitHub, or the traffic in between.

- The index is checked as exact bytes against an ed25519 signature before any of it is parsed. A
  missing, malformed or wrong signature refuses the whole index. With placeholder (all-zero) keys
  every index is refused. **Covered** (`toolboks-catalog` tests).
- An add-on is placed only after its bytes match the size and SHA-256 in the signed index. A
  mismatch deletes the download. **Covered** (`installer` tests).
- Downloads go over HTTPS and only from GitHub's hosts, also after redirects. **Covered**, with
  one defect in the host check that has since been fixed: see finding F1.
- The recovery key cannot sign an index, and a rotation statement cannot double as an index
  signature. **Covered**.

### 2. A downgrade or a replay of an old index

Someone serves an old, correctly signed index to bring back an add-on version with a known flaw.

- Every index has a `sequence`; the host refuses one lower than the last it accepted
  (`trust.json`). Every index expires (`expires_at`, at most 90 days after it was made).
  **Covered**.
- The installer refuses to install a version that is not newer than the installed one.
  **Covered**.
- `trust.json` is in the user's data folder. A program running as the user can delete it and
  replay any index that has not expired yet. **Accepted**: such a program can already do more
  than that (see "Who is trusted"), and the expiry limits how old the index can be. Keep the
  validity window short (days, not months) when signing.

### 3. A replaced exe in the add-on folder

Add-ons live in `%LOCALAPPDATA%`, which the user can write. Something swaps `addon.exe` after
install.

- The supervisor hashes the exe before every start, compares it with the SHA-256 recorded at
  install, and holds the checked file open without write or delete sharing until the process
  runs. A wrong hash is final: no start, no retry. **Covered** for an accidental or careless
  swap (`supervisor` tests).
- The recorded hash (`active.json`) and the kept catalog entry (`entry.json`) are in the same
  folder as the exe. Whoever can swap the exe can swap them too. For an add-on without
  administrator rights that gains the attacker nothing new. For one that asks for administrator
  rights it does: **Open**, findings F2 and F3.

### 4. Another program connecting to the pipe

A program tries to talk to an add-on, or to pose as one.

- Each launch has two pipes with random names that only the current Windows user can open. Remote
  clients are refused, there is one instance per pipe and the host creates it first. **Covered**
  for other accounts (the access list is tested; it was not tried with a second account).
- The process on each pipe must be the one the host started, and it must present a random token
  that is new for every launch. **Covered** (`supervisor` tests). For an add-on started through
  the UAC prompt the host may not learn the process id; then only the token is checked.
- A program of the same user is not kept out by design. **Accepted** for normal add-ons. For an
  add-on running with administrator rights it is a way up: **Open**, finding F4.

### 5. A malicious local `.tbaddon`

The user is talked into installing a package file from somewhere.

- A `.tbaddon` is JSON only, at most 2 MiB. It contains no code, no script and no executable entry
  point; unknown fields are refused. It can show text, live readings and its own toggles.
  **Covered** (SDK validator tests).
- It cannot claim the verified mark, cannot replace a built-in or first-party add-on, and cannot
  turn on a native provider unless it is the exact package the host already trusts. **Covered**
  (`addons` tests).
- Its links must be HTTPS. Its text is shown as written, so it can still lie in words (a fake
  warning, a link to a bad site). **Accepted**: the install review shows who it claims to be from
  and that it is not verified.

### 6. A compromised publisher or a leaked key

- **A leaked GitHub token or account.** The attacker can replace release files. Add-on downloads
  then fail the hash from the signed index. **Covered** for add-ons. The host's own update is not
  covered: its checksum comes from the same release as the exe (#13). **Open**, finding F5.
- **A leaked catalog key.** The attacker can publish any add-on to every user until the key is
  rotated. The recovery key names a new main key; hosts that see the rotation refuse the old key
  from then on. **Accepted**, with the owner decision above. The rotation steps are in
  `docs/catalog-signing.md`; practise them once with test keys before the first release.
- **Both keys lost or leaked.** Only a host update with new built-in keys helps, which again
  rests on the self-update path (finding F5).
- **Third-party publishers** are out of scope until they have their own plan: per-publisher keys
  (`publisher_key_id` is reserved) and a sandbox discussion.

### 7. A faulty or hostile add-on process against the host

Even a first-party add-on can have a bug, and what it sends is treated as untrusted.

- Message size is capped (16 MiB) before memory is allocated; malformed data ends the connection
  and never panics the host. Textures and meshes are checked before they are used. **Covered**
  (`toolboks-addon-proto` and `addon_view` tests).
- An add-on that hangs or crashes cannot take the host with it: launches, reads and writes run
  off the UI thread, a stalled add-on is ended after 30 seconds, and restarts are limited to 3 per
  minute. **Covered** (`supervisor` tests).
- Status items, tray entries, tour steps and preferences are cut to fixed limits, and preferences
  are written only under the add-on's own id. Links open only for `http`, `https` and `mailto`.
  **Covered** (`addon_services` and `addon_view` tests).
- Every add-on process is in a job object that ends with the host, so nothing is left running
  after Toolboks closes or crashes. **Covered**. An add-on started through the UAC prompt is not
  in the job; it exits when the host closes the pipes.
- An add-on draws inside its own page rectangle only, clipped by the host. It cannot draw over
  the sidebar or a host dialog. **Covered** by clipping in the host view.

### 8. Antivirus and Windows warnings

Unsigned programs that are downloaded and started, and that change system settings, look like
malware to scanners. An unsigned add-on that asks for administrator rights shows "Unknown
publisher" in the UAC prompt, which is also what a swapped exe would show.

**Open**: owner decision 2 in #95 (finding F6).

## Findings

Ordered by how much they matter. F1 is fixed; the others are open.

| # | Finding | Where | Proposed fix |
|---|---|---|---|
| F4 | An add-on with administrator rights takes its input (clicks, keys) from the host, which runs without them. A program running as the user can take over the host and so drive the elevated add-on. Today the whole app restarts elevated and Windows keeps lower programs from sending it input; the new design gives that up. | `src/supervisor/`, protocol `Input` | An elevated add-on asks for confirmation of every system change in a window of its own (a real dialog of the elevated process), not in the remote-drawn page. Decide before Debloater and drive optimization move (#92, #88). |
| F2 | For an add-on that may elevate, the exe's hash is checked against a record in the same user-writable folder. A swapped exe plus a swapped record leads to a UAC prompt for the attacker's file under the add-on's name. | `src/installer.rs` (`active.json`), `src/supervisor/mod.rs` | Before an elevated start, check the exe against a signed index that is still valid (keep the last accepted `index.json` and its signature and verify them again), not against `active.json`. Refuse to elevate when no valid index is at hand. |
| F3 | The hash covers `addon.exe` only. Windows loads DLLs from the exe's own folder before the system folder, so a planted DLL beside the exe runs inside the add-on, also an elevated one. | version folder under `addons/<id>/` | Build add-on exes to load DLLs from System32 only (linker `/DEPENDENTLOADFLAG:0x800`), and have the supervisor refuse to start when the version folder holds anything but the expected files. |
| F5 | The host's self-update trusts a checksum from the same release as the exe. Whoever can edit a release can replace the host, and with it every check in this document. | `src/update.rs`, #13, #14 | Sign host releases with a key kept outside GitHub (the catalog signing tool's key handling can be reused) and refuse an update that does not verify. Finish #14. Do this before the first add-on release. |
| F1 | `is_trusted_url` takes everything up to the first `/` as the host. `https://evil.example?x=.github.com` passes, because the text ends in `.github.com`. The catalog's own URL check does not have this flaw, and a URL reaches this function only from a signed index or from GitHub's answers, so it is hard to reach today. | `src/update.rs` `is_trusted_url` | **Fixed** (PR #112): the host ends at `/`, `?` or `#`, and `@` and `:` in it are refused. |
| F6 | Add-on exes and the host are unsigned. | release process, #50 | Owner decision 2. At the least, sign before an add-on that asks for administrator rights is published, so its UAC prompt names the publisher. Signing options and their limits are compared in `docs/RESEARCH-debloat.md` section 10.3. |
| F7 | A hidden add-on that is not a background add-on keeps its (suspended) process. #97 proposes that it holds none. Not a security flaw, but fewer idle processes is also less to attack. | `src/supervisor/mod.rs` `hide` | Decide in #97; ending the process on `hide` is a small change. |

## Owner decisions

1. **Who holds the catalog signing key, where, and the recovery plan.** Answered in #95 on
   2026-11-14: an offline ed25519 key held by the owner and used locally; an offline recovery key
   that can only rotate the main key; nothing in CI. The built-in public keys are still
   placeholders, so no index is accepted until the owner swaps in the real ones.
2. **Whether to buy a code-signing certificate before the first add-on release.** Not answered
   yet. See F6.

## Before the first public add-on release

- [ ] Real catalog public keys are built in (owner).
- [ ] F5: the host's own update verifies a signature.
- [x] F1 is fixed (PR #112).
- [ ] F2, F3 and F4 are fixed or accepted in writing before any add-on that asks for
      administrator rights is published.
- [ ] Owner decision 2 is recorded.
- [ ] The key rotation was practised once with test keys.
- [ ] The elevated start and a second Windows account on the pipe were tried on a real PC.
