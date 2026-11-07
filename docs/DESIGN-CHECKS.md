# Design checks (issue #33)

Written-down checks against `DESIGN.md`. Part (a) is computed. Parts (b) and (c) are **code audits, read from the source, not results seen on screen**: nobody has launched the app at 125 to 200 percent scaling or in an 800 x 560 window for this document. Repeat them on screen before calling #33 done.

## (a) Contrast (computed)

WCAG 2.x relative luminance, ratio = (L1 + 0.05) / (L2 + 0.05). Targets: text 4.5:1, large text and graphics 3:1.

Backgrounds: window `#0e0f11`, card `#15161a`, selected row `#1b1c21`, sidebar `#0a0b0c`, active sidebar item (accent wash over the sidebar, composited) `#241b3b`.

| Foreground | Window | Card | Selected row | Sidebar | Active sidebar item |
|---|---|---|---|---|---|
| Text `#e8e9ec` | 15.80 | 14.89 | 14.01 | 16.23 | 13.36 |
| Muted `#9a9ea8` | 7.15 | 6.74 | 6.34 | 7.34 | 6.05 |
| Accent `#8b5cf6` | 4.53 | **4.27** | **4.02** | 4.65 | **3.83** |
| Accent text `#b39dfa` (Memory data colour) | 8.33 | 7.85 | 7.39 | 8.55 | 7.04 |
| Status ok `#3ecf8e` | 9.61 | 9.06 | 8.52 | 9.87 | not used |
| Status watch `#f0a83a` | 9.47 | 8.93 | 8.40 | 9.72 | not used |
| Status problem `#e5534b` | 5.18 | 4.88 | 4.59 | 5.32 | not used |
| Network `#45b8f0` | 8.55 | 8.06 | 7.59 | 8.79 | not used |

Other pairs: white on the primary button fill `#6d45c4` is 6.37:1. Hairline `#24262c` is 1.27:1 on the window and 1.20:1 on a card.

Findings:

- Every text pair passes 4.5:1, with one exception by design: plain accent `#8b5cf6` as **text** is under 4.5:1 on a card (4.27), the selected row (4.02) and the active sidebar item (3.83). Host code already never uses it for text: add-on badges use `ACCENT_TEXT` (`#b39dfa`, 7.85:1 on a card), and the accent is used for icons, the focus ring and the verified mark, all graphics that need 3:1 (met: 3.83 at the worst). Rule: never set body text in the plain accent; use accent text.
- Muted text is 6.74:1 on a card. DESIGN.md says 6.2:1; the computed value is higher, so the claim is safe. Update it next time DESIGN.md is edited.
- Hairlines and card borders are decorative separators, not the only way to find a control, so the 3:1 rule for component boundaries does not apply to them. Buttons and switches have their own fill. Re-check if a control ever relies on a hairline alone.
- Status colours are always paired with a word ("On", "Off", "Not installed"), so colour is never the only signal.

Nothing needed fixing in the host code.

## (b) Keyboard focus (code audit)

egui makes any widget with `Sense::click()` focusable and part of the Tab order, in creation order. Custom-painted rows must draw their own ring; the shared one is `focus_ring` in `src/app/shell.rs` (1.5 px accent `selection.stroke`, 3.83:1 or better on every row fill).

| Control | File | Focusable | Ring | Notes |
|---|---|---|---|---|
| Sidebar hamburger, nav rows | `src/app/shell.rs` `nav_row` | yes | yes | accessible name set |
| Nested page rows | `src/app/shell.rs` `page_row` | yes | yes | |
| Home list rows | `src/app/shell.rs` `list_row` | yes | yes | |
| Toggle switch (add-on preferences, settings) | `src/ui.rs` `toggle_switch` | yes | yes (line 235) | owned elsewhere, read only |
| Range pills, page pills | `src/ui.rs` `pill` | yes | not audited in detail | owned elsewhere |
| Primary button, Help, Dismiss, Browse, View details, modal buttons | `src/app/shell.rs` | yes | yes, egui default widget focus uses the style's selection stroke | |
| Text inputs | `src/app/shell.rs` | yes | egui default | |
| Tour overlay | `src/app/tutorial.rs` | overlay swallows clicks (`click_and_drag`) while the shell is disabled | n/a | tour has its own buttons |

Gaps:

1. Tab order of the two bottom sidebar items: they are drawn bottom-up, so Tab goes Add-ons then Settings, while on screen Settings sits above Add-ons. Tab moves bottom to top. Small. Fix by drawing Settings first in the bottom-up block would also swap their on-screen positions, which DESIGN.md orders as Add-ons at the very bottom, so it was left alone.
2. The collapsed rail shows the name only as a hover tooltip. A keyboard-only user hears the accessible name (set with `widget_info`) but sees no label when a row has focus. Consider showing the tooltip on focus.
3. Not read in this pass (other builders own them): custom clickable rows in `src/tools/**` and `addons/**`. Check each for `focus_ring`-style drawing.

## (c) Scaling and small windows (code audit)

Minimum window set in `src/main.rs`: `with_min_inner_size([1060.0, 720.0])`. DESIGN.md asks for checks at 800 x 560. egui sizes are logical points, so 200 percent scaling at 1060 px is about 530 points wide; 800 px at 200 percent is 400 points.

What holds:

- Sidebar is fixed 52 or 204 points and the nav list scrolls (`ScrollArea` with `max_height`), so a short window never hides Add-ons and Settings.
- Content, modals (`modal_width`: window width minus 64, floor 240), notes, metrics, add-on text and the Home detail card wrap inside the available width. Add-on pages scroll vertically.
- Add-on text inputs cap at 400 points and shrink to the available width. Browser rows keep a 120 point text column minimum.
- Status bar items are short captions; version is right-aligned.

Fixed in this pass: Home used a 280 point list beside a detail card, which at narrow widths left the card about 150 points wide. Under 520 points of available width it now stacks, with the list above (132 to 220 points tall, scrolling) and the detail card below.

Gaps left:

1. `src/main.rs` minimum inner size (1060 x 720) is larger than the 800 x 560 DESIGN.md target. Lowering it is the owner's call and is outside this slice; once lowered the layout work below matters.
2. Header (46 points, fixed): breadcrumb on the left and, with the rail collapsed, one pill per page plus Help on the right. Neither side wraps or truncates, so a four-page tool at about 400 points of width (800 px at 200 percent) overlaps. Needs a design decision: move the pages into a menu, or truncate the breadcrumb with an ellipsis.
3. Browser rows and the tool pages in `src/tools/**` were not measured; they are other builders' files.
4. Tour step cards and the update prompt use fixed widths in `src/app/tutorial.rs` and `src/app.rs`; not audited in detail.
