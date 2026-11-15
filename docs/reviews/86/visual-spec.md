# Add-on browser visual specification

Source: the live `https://gusseti.github.io/toolboks/` browser (`#store`) and its `styles.css`, inspected November 14, 2026. Source lives on `main:site/`, not a `gh-pages` branch. CSS values below are copied, not estimated. Root font size is 16 CSS px. The landing page overrides the approved sketch for this surface.

| Element | Exact source values (CSS px unless noted) |
| --- | --- |
| Palette | window `#0e0f11`; toolbar `#0a0b0c`; card/icon/trust `#15161a`; selected `#1b1c21`; border `#24262c`; divider `#1c1e22`; text `#e8e9ec`; muted `#9a9ea8` |
| Accent | fill `#7a4be8`; hover `#6c3fdc`; focus `#8b5cf6`; text `#b39dfa`; badge wash `#8b5cf633`; installed `#3ecf8e`; hover border `#33363e` |
| Font | Schibsted Grotesk, fallback Segoe UI Variable / Segoe UI; regular 400, medium 500, semibold 600, heading 700; inherited line height 1.6 |
| Store | 1 px border, 12 radius, overflow hidden; screenshot reference store width 1036 px |
| Toolbar | padding 14 vertical / 16 horizontal; gap 12 vertical / 16 horizontal; bottom divider 1 px; flex-wrap |
| Search | card fill, 1 px border, radius 8; horizontal padding 12; icon 17, gap 8; font 15/24 regular; vertical input padding 10; flex basis 220, max width 320 |
| Categories | gap 4; button padding 7 vertical / 12 horizontal; font 14/22.4 regular; radius 7; transparent muted, selected fill/text |
| Columns | `minmax(0,1fr) 340px`; minimum body height 440; below viewport 860 use one column, list above detail |
| List | padding 8; row gap 2; max height 520; right divider 1 px; thin scrollbar `#2c2e35` |
| Row/card | grid `40px minmax(0,1fr) auto`; gap 14; padding 12; radius 8; no border; selected fill, hover card fill; computed height 73.984375 |
| Icon box | 40 square; radius 10; 1 px border; icon 20; stroke 1.7 in 24-unit SVG grid; selected border `#33363e` |
| Row title | 15/24 semibold; publisher 13/20.8 muted, left margin 6; description 13/20.8 muted, margin top 2, single-line ellipsis |
| Verified row badge | margin left 6; padding top/bottom 2, left 4, right 7; radius 99; 11/13.2 medium; icon 13; gap 5; accent wash/text; computed height 17.1875 |
| General verified badge | margin left 8; padding 3 vertical, left 6, right 9; radius 99; 12/14.4 medium; icon 15; gap 5 |
| Row state | 13/20.8 regular muted; gap 6; icon 15; installed green plus check and word |
| List footer | padding 12; font 13/20.8 muted; top divider 1 px; margin top 6 |
| Detail | width 340 including padding 20; gap 16; body 14/22.4; title 22/35.2 bold, letter spacing -0.02em; meta 13/20.8, margin top 4 |
| Trust card | padding 10 vertical / 12 horizontal; radius 8; 1 px border; icon 17 accent text, gap 10; text 13/20.8 muted |
| Access | top divider 1 px; padding top 14; gap 10; heading 13/16.9 semibold; line icon 16, gap 10; text 14/22.4 muted |
| Primary button | padding 14 vertical / 20 horizontal; gap 10; 16/16 semibold; radius 8; 1 px matching fill border; white text; bottom of detail; hover fill above |
| Secondary/small | transparent, 1 px line border, same radius; small padding 9/14, font 14, radius 6; disabled opacity .45 |

## Reconciliation

- Replace the sketch's narrow 280 px master column, separate card frames and category dropdown with the landing page's broad compact rows, 340 px detail column, icon tiles, verified pills and inline category buttons.
- Keep search, installed filtering, refresh, full details and grouped installation; utility actions sit outside the reference store so they do not displace its layout.
- First-run picks are live verified catalog records only; no embedded packages or synthetic catalog records. The owner-approved order is ordering metadata, not a list of packages to load.
- Preserve the five owner decisions. Updates replace Install as primary; removal retains data unless explicitly checked; failures remain until dismissed.
- Use the same palette and typography on first-run, review, details and removal. Native scrolling/focus and live operational state remain real, not a painted mockup.
