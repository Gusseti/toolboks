# The add-on catalog

The catalog is one file, `index.json`, at the address in `src/catalog.rs` (`URL`). Anyone can run
a catalog: it is a plain list and it is **not signed**. The app checks the shape of the list and,
for every download, the size and SHA-256 the entry declares. A download that does not match is
thrown away.

What this means: whoever can change the file can list any add-on. Toolboks does not decide who
may publish. The "verified" flag in an entry is whatever the list says.

Older indexes carried a `sequence`, `generated_at`, `expires_at` and a `publisher_key_id`. They
are ignored, so those indexes still read.

## `index.json` (format 1)

 Unknown extra fields are ignored; a change in meaning bumps
`format_version`, and older apps then refuse the whole index.

```json
{
  "format_version": 1,
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
      "pages": ["Overview", "Details"],
      "background": false,
      "kind": "declarative",
      "version": "1.0.0",
      "min_toolboks_version": "0.12.5",
      "protocol_version": 3,
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
- `protocol_version` must be one the app speaks (3 today; v2 entries are hidden), also for a declarative package;
  an entry with another version is hidden.
- `pages` (optional) lists the add-on's page names in order: at most 16, unique, 1 to 40
  characters, no `/`. Links such as `--open "Title/Page"` resolve against them.
- `background` (optional, default false) marks a native add-on that keeps working while hidden.
  The app starts it at launch while it is turned on.
- `capabilities`: `administrator`, `drive_optimize` or `system_tuning` let a native add-on ask for
  a separate helper for one administrator action. The helper owns its native Confirm/Cancel
  window; the add-on UI stays unelevated. See [ADDON-PROTOCOL.md](ADDON-PROTOCOL.md).
- `byline` (optional, at most 60 characters) is the maker's credit shown in the details, such as "by stratum".
- `data_note` (optional, 1 to 160 characters, one line, no control characters, no Unicode format
  characters such as bidi overrides or zero-width characters, no line or paragraph separators, no
  spaces at either end; `toolboks_addon_sdk::valid_data_note`) is one plain sentence about data the add-on keeps on this PC, such as "Keeps your last scan
  of each drive on this PC (file and folder names)." It is shown under the capabilities in the
  install review and in the details. Copy it from the add-on's manifest (`data_note`, same rule).
  A bad note makes the whole index refused, like any other bad field. Older apps ignore it.
