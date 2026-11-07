# Add-on design checklist

Every add-on, built in or installed, must pass this list before release. The rules come from [DESIGN.md](../DESIGN.md), which wins if the two differ. `tbaddon validate` prints this list after a package passes, and `tbaddon checklist` prints it on its own.

An add-on built from the SDK gets most of this for free: the host draws the sidebar entry, header, breadcrumb, status bar, fonts, colours and focus ring. The list is for what the add-on itself chooses: its words, its pages and, for native add-ons, its own drawing code.

## Checklist

1. **Tokens only.** No own palette, font, corner radius or icon style. Native add-ons copy the token values from DESIGN.md (Tokens) into one small module and nowhere else (Disk Manager: `addons/disk-manager/src/theme.rs`). The accent marks selection only. Data colours come from the data set in DESIGN.md, are distinct from the accent and always sit next to a label.
2. **Sentence case.** Sidebar entry, breadcrumb, page names, buttons and headings use sentence case. No uppercase labels, no emoji, no Unicode glyphs as icons. Page names are one or two words.
3. **Plain wording, labelled estimates.** Say what the thing does for the person first, then offer detail. A measurement is a measurement; an estimate says "estimate" in the label and the tooltip. Say plainly what cannot be measured and why. An unavailable value is never shown as zero.
4. **One primary button per view.** It has the accent fill. Other buttons are outlined. Buttons name their action ("Start optimization", not "OK").
5. **Keyboard focus is visible.** Every control can be reached and used with the keyboard, and the focus ring is not removed or hidden.
6. **Contrast of at least 4.5:1** for text (3:1 for large text). Muted text is never darker than the muted token. Colour is never the only signal: lines and tiles are labelled, status has a word.
7. **Changes to the system show a preview, ask once and offer undo.** Show what will change before it happens, confirm with a button that says what it does, and offer undo or say plainly when a change cannot be undone. Never claim a safety guarantee the add-on does not give.
8. **Nothing is written to disk** except through the host's local preferences (and a file the person asked for, such as an exported report, which never overwrites an existing file). No network unless the manifest declares it.

## Page types

- **Overview**: cards, or master and detail (list on the left, detail on the right).
- **Table**: a list with filters; sticky header, right-aligned numbers.
- **Settings**: labelled controls.
- **Tool**: preview, confirm, apply, undo.

## Declarative packages

A `.tbaddon` package has no styling fields, so it picks up the host look with no change. Your job is wording:

- `heading`: a short noun phrase in sentence case ("Your system at a glance"), no full stop.
- `text` and `note`: one or two plain sentences. A note has a short title and carries an honest limit or a tip, nothing decorative.
- `metric`: a label that says what is measured ("Memory in use"). Do not add units the host already shows.
- `toggle` and `text_input`: the label names the setting, not the control ("Show detailed information").
- Page titles are the sidebar entries: one or two words, sentence case.

## Native add-ons

- Draw with the host's helpers where the host exposes them; otherwise use one local token module that mirrors DESIGN.md.
- Cards have a 1 px hairline and a 10 px radius, no shadow, no coloured edge, no card inside a card.
- Tables: 11 px muted sentence-case header, 26 to 30 px rows, numbers right-aligned.
- Mark guided-tour targets with `tutorial::mark` or `tutorial::region` so highlights follow the layout.
- Test at 125 to 200 percent scaling and at the smallest window size.
