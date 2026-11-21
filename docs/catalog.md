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
  `developer_integration` (local job queue, process priority and CPU cores, Claude Code scripts and
  settings) grants no administrator helper; the install review explains it.
- `icon` is one of the names the app draws, or `custom` for an add-on that ships its own picture.
  A custom icon needs all three of `icon_url` (an HTTPS GitHub address, the same rule as
  `download_url`), `icon_sha256` (lowercase hex SHA-256 of the file) and `icon_size` (bytes, 1 to
  32768); an entry with `"icon": "custom"` and any of them missing or bad makes the whole index
  refused. The fields are left out for a named icon (and checked if present). The file is a
  square PNG of 16 to 128 pixels; the app downloads it when it installs or updates the add-on and
  when the add-on list shows it, places nothing that does not match `icon_size` and
  `icon_sha256`, and keeps it in `<data>/icons/<id>-<sha256>.png` (never in the add-on's version
  folder). A file that fails any check is drawn as the default glyph. Older apps ignore the three
  fields and draw the unknown name `custom` as their default glyph (the entry still reads and
  installs), so an entry with a custom icon must set `min_toolboks_version` to the first release
  that draws custom icons (the one after 0.20.0), the same way protocol 4 and 5 entries are kept
  from older apps.
- `byline` (optional, at most 60 characters) is the maker's credit shown in the details, such as "by stratum".
- `data_note` (optional, 1 to 160 characters, one line, no control characters, no Unicode format
  characters such as bidi overrides or zero-width characters, no line or paragraph separators, no
  spaces at either end; `toolboks_addon_sdk::valid_data_note`) is one plain sentence about data the add-on keeps on this PC, such as "Keeps your last scan
  of each drive on this PC (file and folder names)." It is shown under the capabilities in the
  install review and in the details. Copy it from the add-on's manifest (`data_note`, same rule).
  A bad note makes the whole index refused, like any other bad field. Older apps ignore it.
- `checks` (optional, default empty, left out when empty) lists the names of the health checks the
  add-on can report to Home, such as `["Recent crashes", "Disk health"]`: at most 6 names, each 1 to
  40 characters of plain text (same rule as `data_note`: no control characters, no Unicode format
  characters such as bidi overrides or zero-width characters, no line or paragraph separators, no
  spaces at either end; `toolboks_addon_sdk::valid_check_names`), and no two names the same when
  upper and lower case are ignored. Copy it from the add-on's manifest (`checks`, same rule). A bad
  list makes the whole index refused, like any other bad field. Older apps ignore it; a package
  manifest with `checks` is refused by apps older than the release that adds it, so an add-on that
  sets it needs that `min_toolboks_version`.
