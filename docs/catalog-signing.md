# Catalog signing (owner runbook)

The add-on catalog is one file, `index.json`, with a detached signature in `index.json.sig`.
The app accepts an index only when the signature over its exact bytes was made by the main
catalog key. This page is for the person who holds that key.

Rules that do not bend:

- Both private keys are created and used **offline, on the owner's own machine**. They never go
  into a repository, a cloud folder, a chat, GitHub Actions secrets or any other CI.
- The app embeds two **public** keys: the main key and the recovery key.
- The main key signs `index.json`. The recovery key signs nothing except a rotation statement
  that names a new main key. An index signed by the recovery key is refused.
- Agents and CI only ever verify. The repository contains test keys only, derived from fixed
  seeds inside test code.

## The tool

`tools/catalog-sign` is a separate program, so none of it ends up in `toolboks.exe`.

```
cargo build --release --locked -p catalog-sign
target\release\catalog-sign.exe
```

| Command | What it does |
| --- | --- |
| `keygen <out-dir>` | Creates `catalog-private.key` and `catalog-public.txt` in a folder. Refuses to overwrite. |
| `sign <index.json> <private-key> [rotations.json]` | Checks the index, then writes `index.json.sig`. |
| `rotate <recovery-private-key> <new-main-public-hex> [rotations.json]` | Appends a rotation statement signed by the recovery key. |
| `verify <index.json> <main-public-hex> [recovery-public-hex]` | Checks `index.json.sig`. With the recovery key it runs the same check as the app. |

## 1. Create both keys (once)

Do this with the network unplugged if you can.

```
catalog-sign keygen E:\catalog-main
catalog-sign keygen F:\catalog-recovery
```

Each run prints the public key (64 hex characters) and its key id (16 hex characters).

- `catalog-private.key` is the secret. It is 32 random bytes as hex, with no password on the file,
  so the storage must protect it: an encrypted USB stick or a password manager entry.
- Keep the main and the recovery key in **different places**. The recovery key is not used day to
  day; it only matters if the main key is lost or leaked.
- Make a second copy of each (second device or paper) and check that the copy reads back.
- `catalog-public.txt` is not secret.

## 2. Put the public keys into the app

In `src/catalog.rs`, replace the two placeholder constants with the public keys, as bytes:

```rust
const MAIN_KEY: [u8; 32] = [0x12, 0x34, /* ... 32 bytes from catalog-public.txt ... */];
const RECOVERY_KEY: [u8; 32] = [/* ... */];
```

Until then both are all zero, which the verifier treats as "no key": every index is refused.
This is an owner-made change of its own. Check the key ids the app derives against the ones
`keygen` printed before releasing.

## 3. Sign a catalog release

The index is built by the catalog repository. Before publishing it:

```
catalog-sign sign index.json E:\catalog-main\catalog-private.key
catalog-sign verify index.json <main-public-hex>
```

`sign` refuses an index that the app would refuse anyway: unknown `format_version`, a bad entry
(id, version, `https` download URL, sha256, size), duplicate id and version, a validity window
longer than 90 days, or an `expires_at` that has already passed.

Things to keep true in the index:

- `sequence` goes up with every release. An app never accepts a lower sequence than the last
  one it accepted, so reusing or lowering a number makes the release invisible.
- `generated_at` and `expires_at` are seconds since the Unix epoch. Keep the window short (a few
  weeks). An expired index is refused by every app, so re-sign before it runs out.
- The signature covers the exact bytes. Do not reformat, re-save or change line endings after
  signing; publish `index.json` and `index.json.sig` exactly as they are. Mark both files
  `-text` in `.gitattributes` if they are committed.

## 4. Rotate the main key (lost or leaked)

1. Create a new main key: `catalog-sign keygen E:\catalog-main-2`.
2. Fetch the recovery key from its storage and sign the statement:

   ```
   catalog-sign rotate F:\catalog-recovery\catalog-private.key <new-main-public-hex> rotations.json
   ```

   This appends to `rotations.json`. The file is public; keep it with the catalog sources.
3. Sign the next index with the **new** main key. `sign` picks up a `rotations.json` that sits
   beside `index.json`, or takes it as the third argument, and embeds it in `index.json.sig`:

   ```
   catalog-sign sign index.json E:\catalog-main-2\catalog-private.key rotations.json
   catalog-sign verify index.json <old-main-public-hex> <recovery-public-hex>
   ```

4. Keep passing `rotations.json` for every later index. An app that has accepted the rotation
   remembers it and refuses the old main key from then on. A fresh install learns it from the
   signature file.
5. Put the recovery key back in its storage.

What a rotation cannot do: it only protects an app once that app has seen it. An app that has
not (a fresh install, or one that was offline) still trusts the old main key, and whoever holds
a leaked key can keep signing new indexes with it. They would also have to get such an index
delivered in place of the real one, but do not rely on that.

So after a **leak**, follow the rotation with a normal app release that builds the new main key
in (step 2 again) and sets `ROTATION_FLOOR` in `src/catalog.rs` to the sequence of the latest
rotation. Statements at or below the floor are ignored, so an old statement cannot bring a
retired key back. After a plain loss (nobody else has the key) the same release can wait for the
next regular one.

If the **recovery** key is lost or leaked, create a new one and ship it in a normal app release.
A leaked recovery key cannot sign an index by itself, but it can name a new main key, so
replace it soon.

## File formats

`index.json` (format 1). Unknown extra fields are ignored; a change in meaning bumps
`format_version`, and older apps then refuse the whole index.

```json
{
  "format_version": 1,
  "sequence": 12,
  "generated_at": 1800000000,
  "expires_at": 1801209600,
  "entries": [
    {
      "id": "toolboks.sample",
      "title": "Sample",
      "short_description": "One line for the list.",
      "long_description": "Longer text for the details page.",
      "publisher": "Toolboks",
      "category": "system",
      "icon": "box",
      "accent": "#4f8cff",
      "screenshots": ["https://example.com/sample/1.png"],
      "capabilities": ["read_system_info"],
      "kind": "declarative",
      "version": "1.0.0",
      "min_toolboks_version": "0.12.5",
      "protocol_version": 1,
      "download_url": "https://example.com/sample/1.0.0/sample.tbaddon",
      "size": 5,
      "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      "release_notes": "First release.",
      "verified": true,
      "channel": "stable"
    }
  ]
}
```

- One entry per add-on version. `kind` is `declarative` or `native`; `channel` is `stable` or
  `preview` (preview entries are shown by preview builds only).
- `version` and `min_toolboks_version` are plain `x.y.z`.
- `publisher_key_id` (optional) is reserved for per-publisher keys and is not checked yet.

`index.json.sig`:

```json
{
  "key_id": "0123456789abcdef",
  "signature": "<128 hex characters: ed25519 over the bytes of index.json>",
  "rotations": [
    {
      "new_key": "<64 hex characters>",
      "key_id": "<16 hex characters>",
      "sequence": 1,
      "issued_at": 1800000000,
      "signature": "<128 hex characters, by the recovery key>"
    }
  ]
}
```

A key id is the first 8 bytes of the SHA-256 of the public key, in hex. All hex is lowercase.
A rotation signature covers the text
`toolboks-catalog-rotation-v1\n<key_id>\n<new_key>\n<sequence>\n<issued_at>\n`.
