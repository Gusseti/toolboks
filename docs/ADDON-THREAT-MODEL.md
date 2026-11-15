# Add-on platform: threat model and security review

Issue #95. This covers the platform that downloads add-ons and runs them as separate programs:
the signed catalog (`shared/toolboks-catalog`, `src/catalog.rs`), the installer
(`src/installer.rs`), the supervisor and its pipes (`src/supervisor/`), the host view and host
services (`src/addon_view/`, `src/addon_services.rs`), and local `.tbaddon` packages
(`src/addons.rs`). It was written from a read of that code on 2026-11-14. Nothing here was tested
by attacking a running build.

> **Owner decision, 2026-11-15: nothing is signed.** The catalog is a plain list that anyone can
> publish, and neither it nor the exes are signed. The catalog keys, rotation, expiry and replay
> checks and the signing tool were removed (the catalog-signing text below is kept as the record
> of the earlier design and no longer describes the code). The community catalog model is the
> reason: Toolboks does not decide who may publish.
>
> What is still checked: the shape of the list, and for every download the size and SHA-256 the
> entry declares, so a damaged or swapped download is thrown away. What is **not** protected any
> more, all **accepted**: whoever can change the catalog file can list any add-on, set `verified`
> and grant a native add-on the right to ask for administrator rights; an old list can be served
> again; a replaced release asset together with a replaced list is accepted. F2 now means only
> that an elevated start reads the saved catalog entry, not the install records the user account
> can edit. F5 and F6 (release and code signing) are accepted risks with no work planned.
> Administrator rights still need the confirmation dialog owned by the elevated process (#132).
> Revisit before there are real users.

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
  rights it does. **Covered** for the launch-record attack (F2): the host keeps the exact
  accepted index bytes and signature together in `catalog/accepted.json`, verifies them again
  with the current clock and trust state before every privileged launch (including children of
  an elevated host), and takes the hash and elevation capability from that signed entry. A
  missing, damaged, expired, replayed or incompatible catalog refuses the launch. An installed
  rollback absent from that catalog cannot elevate. `entry.json` cannot grant elevation.
- **Covered** for DLLs planted before a launch (F3): the MSVC workspace builds use
  `/DEPENDENTLOADFLAG:0x800` (System32 for implicit DLL loads). Installed version folders may
  contain only regular `addon.exe` and `entry.json` files; extra files, subdirectories and
  reparse points in the folder or its ancestors refuse the launch. The built PE's loader flag
  is checked by a test. Add-on builds outside this workspace must use the same linker flag;
  DLLs loaded explicitly by add-on code must use System32 or an absolute trusted path. A folder
  scan is not an atomic defense against a same-user writer adding a file after the scan; the
  image's loader restriction supplies the protection for implicit dependencies.

### 4. Another program connecting to the pipe

A program tries to talk to an add-on, or to pose as one.

- Each launch has two pipes with random names that only the current Windows user can open. Remote
  clients are refused, there is one instance per pipe and the host creates it first. **Covered**
  for other accounts (the access list is tested; it was not tried with a second account).
- The process on each pipe must be the one the host started, and it must present a random token
  that is new for every launch. **Covered** (`supervisor` tests). For an add-on started through
  the UAC prompt the host may not learn the process id; then only the token is checked.
- A program of the same user is not kept out by design. **Accepted** for normal add-ons. For an
  administrator helper it must never be consent: **implemented, real UAC validation pending**
  (F4). The helper accepts one action, builds its own preview and owns a native Confirm/Cancel
  dialog. Forwarded host input cannot approve it. The drawing process stays unelevated.
  No privileged native add-on may ship until the real UAC integration is checked.

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
  covered: its checksum comes from the same release as the exe (#13). **Accepted risk pending
  owner decision**, finding F5: release-signing key ownership, custody and recovery are not
  agreed. This remains a first-public-add-on-release blocker.
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
- A hidden non-background add-on is sent `Quit` and ended after the grace period if needed;
  hiding during launch cancels that launch. Reopening starts a new process (or waits for the
  old one to finish quitting). Only declared background add-ons retain a suspended process.
  **Covered** (`supervisor` process lifecycle tests), finding F7.
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

**Accepted risk pending owner decision**: owner decision 2 in #95 (finding F6). The owner
must decide whether to buy a code-signing certificate and how to keep its signing credentials;
no certificate or private key is generated by this change.

## Findings

F1 is unchanged. The implementation follow-up records these statuses for F2-F7; an
"accepted risk pending owner decision" is a documented deferral, not release clearance.

| # | Status | Mitigation or reason for deferral |
|---|---|---|
| F2 | **Fixed** | Cache the accepted index and signature atomically; reverify signature, expiry, replay floor, compatibility, native kind and signed elevation capability before a privileged start. Hash the locked exe against the signed entry, never the writable install record. Tests cover missing/expired/tampered/replayed catalogs, recovery-key misuse, mismatched ids/versions, incompatible entries and swapped exe plus records. |
| F3 | **Fixed** | Build MSVC exes with `/DEPENDENTLOADFLAG:0x800`; refuse installed version folders with extra files, directories or reparse points. Tests inspect the built PE loader flag and reject planted DLLs, other files, directories and junctions. Outside-workspace builds must carry the same loader restriction. |
| F4 | **Implemented; real UAC validation pending** | Owner chose per-change confirmation in #118 (2026-11-15), implemented in #132. Protocol 3 starts a separate checked helper; the UI stays unelevated. The SDK boundary applies to conforming add-ons, not arbitrary unsigned code that ignores it. SDK validates each action, creates its own native Confirm/Cancel dialog and checks the window PID. Host clicks/keys, forged results, Cancel and disconnect cannot authorize a write; regression tests cover these and a real native window PID. Disk Manager uses the boundary without moving out of the app. The native preview was opened without elevation; Computer Use app access prevented inspection/capture (owner approved documenting this limitation). Real UAC consent/decline and privileged execution still require a manual check before publication. |
| F5 | **Accepted risk pending owner decision** | Host releases need an offline signing key with owner-approved custody and recovery; the catalog decision authorizes signing indexes only, so its key is not silently reused for host executables. The existing checksum is still only a corruption check. #13/#14 and release signature enforcement remain release blockers. |
| F6 | **Accepted risk pending owner decision** | Owner decision 2 in #95: whether to purchase a code-signing certificate before the first add-on release, and who holds its signing credentials. #50 remains open. |
| F7 | **Fixed** | Hidden non-background add-ons quit and are ended after the grace period; an in-flight launch is cancelled. Reopening starts again, also when the previous process is still quitting. Background add-ons alone remain suspended. Process tests cover each case. |
| F1 | **Fixed previously** | PR #112: the download URL host ends at `/`, `?` or `#`; `@` and `:` in it are refused. No change in this follow-up. |

## Owner decisions

1. **Who holds the catalog signing key, where, and the recovery plan.** Answered in #95 on
   2026-11-14: an offline ed25519 key held by the owner and used locally; an offline recovery key
   that can only rotate the main key; nothing in CI. The built-in public keys are still
   placeholders, so no index is accepted until the owner swaps in the real ones.
2. **Whether to buy a code-signing certificate before the first add-on release.** Not answered
   yet. See F6.
3. **Privileged UI boundary for native add-ons.** Decided in #118 on 2026-11-15: each change
   requires a native dialog owned by the small elevated helper; the add-on UI stays unelevated.
   Protocol 3 migration is owner-approved. Implemented in #132; real UAC validation remains.
4. **Host release-signing key custody and recovery.** Pending (F5): choose the release key and
   who holds it offline. Catalog key ownership does not authorize a second signing purpose.
   No real signing keys were created or changed in this implementation follow-up.

## Before the first public add-on release

- [ ] Real catalog public keys are built in (owner).
- [ ] F5: the host's own update verifies a signature.
- [x] F1 is fixed (PR #112).
- [x] F2 and F3 are fixed, with regression tests.
- [x] F7: hidden non-background add-ons hold no process after shutdown completes.
- [x] F4: owner approved the helper boundary; protocol 3 and regression tests implement it.
- [ ] F4 integration: manually try UAC consent/decline, native Confirm/Cancel and a real
      privileged action before publishing any add-on with administrator rights.
- [ ] Owner decision 2 is recorded.
- [ ] The key rotation was practised once with test keys.
- [ ] The elevated start and a second Windows account on the pipe were tried on a real PC.

### F4 scope under the unsigned catalog decision

Protocol 3 enforces per-action confirmation inside the SDK helper and the bundled Disk
Manager. It is not a sandbox for arbitrary elevated native code. Under the accepted #134
catalog model, a publisher can supply an executable that ignores the SDK and the confirmation
contract altogether. After a user grants that executable UAC rights, Toolboks cannot force it
to confirm each OS write. The catalog's `verified` flag and hash do not authenticate a publisher.
This inherited limitation is not fixed or hidden by the helper implementation.
