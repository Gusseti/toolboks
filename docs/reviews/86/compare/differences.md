# Add-on browser: landing page versus the app

Reference: `ref-browser.png`, the browser on https://gusseti.github.io/toolboks/ at 884 px wide, Disk Manager selected.
App: `app-browser.png`, the same area of the real Toolboks window at 100 % scale, same width and selection, captured by `--marketplace-check` (see `docs/LOCAL-DEVELOPMENT.md`).

## Measured result

Text and box positions were measured in both images. These match to the pixel: search box and its text, row titles, descriptions, Verified badges, icon tiles, row state text, the selected row, the column divider, the detail title, meta line, description, trust card, "It can" heading, access lines, and the size of the Install button.

## Differences fixed in this round

| Was | Now |
| --- | --- |
| Search placeholder drawn at 14 px and 2 px too high | 15 px, centred in the box |
| Category buttons 6 px too low and 33 px tall | Centred on the search box, 37 px tall |
| Category buttons drifted left, 10 px by the last one | Within 1 px of the reference |
| Row title 3 px and description 2 px too high, state 1 px too low | Match |
| Verified badge 3 px too high | Match |
| Detail column 1 px too far right, trust card 7 px too narrow | Match |
| "It can" 7 px too low (a default separator was used) | Match |
| Install button 44 px tall and 88 px wide | 46 by 90, as the reference |
| No "by stratum" in the meta line | Shown from the new optional catalog field `byline` |
| List clipped 8 px above the bottom edge | Runs to the bottom edge |

## Differences that remain

| Difference | Reason |
| --- | --- |
| Words are up to 1 px narrower | egui snaps each glyph advance to a whole pixel; the browser uses fractional advances. The gap between category buttons is 6 px, not 4, to make up for it. |
| The selected category's background sits 1 px lower than the reference, its text matches | egui centres text on whole pixels; the source line box is 22.4 px. |
| The selected row is 1 px narrower on the right | Rounding of the list column width. |
| Access lines read differently | The reference shows Disk Manager's real access. The app shows what the local test package asks for, because the review must show what will be installed. |
| The app's browser is taller, and Install sits lower | The app's browser fills the window down to the bottom edge, as the owner asked. The reference has a fixed height. Everything above the button matches. |
| No hover highlight on Event Watch | The reference was captured with the mouse over that row. |

## Other screens

The first-run picker, details page, install review, removal dialog and sidebar states use the host style in `DESIGN.md`. They were not restyled to the landing-page look in this change.
