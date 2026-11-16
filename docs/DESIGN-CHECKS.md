# Design checks (issue #33)

Written-down checks against `DESIGN.md`. Part (a) is computed. Parts (b) and (c) are **code audits, read from the source, not results seen on screen**: nobody has launched the app at 125 to 200 percent scaling or in an 800 x 560 window for this document. Repeat them on screen before calling #33 done. Part (d) is the host shell re-read after the #35 follow-ups.

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
| Home Health actions (Check again, row actions, "more" links) | `src/app/health.rs` | yes | yes, `focus_ring` over each | Tab order Check again, then each row's action top to bottom (`tab_goes_check_again_then_each_action_top_to_bottom`). The Home list rows (`list_row`) left Home with #182. |
| Toggle switch (add-on preferences, settings) | `shared/toolboks-shared/src/ui.rs` `toggle_switch` | yes | yes (line 235) | owned elsewhere, read only |
| Range pills, page pills | `shared/toolboks-shared/src/ui.rs` `pill` | yes | yes (line 247), see (d) | |
| Primary button, Help, Dismiss, Browse, View details, modal buttons | `src/app/shell.rs` | yes | yes, egui default widget focus uses the style's selection stroke | |
| Text inputs | `src/app/shell.rs` | yes | egui default | |
| Tour overlay | `src/app/tutorial.rs` | overlay swallows clicks (`click_and_drag`) while the shell is disabled | n/a | tour has its own buttons |

Gaps found in the first pass, since fixed (see (d)): the two bottom sidebar items tabbed bottom to top, and the collapsed rail showed names only on hover.

## (c) Scaling and small windows (code audit)

Minimum window set in `src/main.rs`: `with_min_inner_size([1060.0, 720.0])`. DESIGN.md asks for checks at 800 x 560. egui sizes are logical points, so 200 percent scaling at 1060 px is about 530 points wide; 800 px at 200 percent is 400 points.

What holds:

- Sidebar is fixed 52 or 204 points and the nav list scrolls (`ScrollArea` with `max_height`), so a short window never hides Add-ons and Settings.
- Content, modals (`modal_width`: window width minus 64, floor 240), notes, metrics, add-on text and the Home detail card wrap inside the available width. Add-on pages scroll vertically.
- Add-on text inputs cap at 400 points and shrink to the available width. Browser rows keep a 120 point text column minimum.
- Status bar items are short captions; version is right-aligned.

Fixed in this pass: Home used a 280 point list beside a detail card, which at narrow widths left the card about 150 points wide. Under 520 points of available width it now stacks, with the list above (132 to 220 points tall, scrolling) and the detail card below.

Gaps left from the first pass: items 1 and 2 are resolved in (d). Browser rows (`store::row`) and the tour step cards and update prompt (`src/app/tutorial.rs`, `src/app.rs`) use fixed widths and were not measured. The built-in tools (`src/tools/**`) no longer exist.

## (d) Host shell: focus and scaling (source audit, 2026-11-16)

**This is a source audit, not a measured pass.** It was read from the code and covered by headless egui unit tests (`src/app/shell.rs`, `mod tests`). Nobody opened the app at 125 to 200 percent display scaling for it. A preview build was also looked at once in a real window at 100 percent scaling (see the end of this section); that is a spot check, not a scaling test.

Scaling first: `with_min_inner_size` takes logical points, so Windows display scaling does not shrink the window below 1060 x 720 points. The "400 points at 200 percent" case in (c) does not happen. Egui's own zoom factor is only used by the preview checks, never in a normal run. The supported minimum is now written in DESIGN.md and held in one constant (`MIN_INNER_SIZE`).

| Part | Focus order and ring | Small windows |
|---|---|---|
| Sidebar | Tab goes hamburger, Home, add-ons with their pages, then the foot top to bottom: Settings, Add-ons, Pick your tools. The foot used to be drawn bottom-up, which reversed Tab; it is now created top to bottom and placed by its known height (`bottom_group`). Every row draws `focus_ring`. A collapsed row shows its name as a tooltip on keyboard focus as well as on hover (`focus_tip`, used by `nav` and by the rows of catalog-installed add-ons in `market_sidebar`). Tests: `tab_goes_down_the_foot_of_the_sidebar_in_the_order_shown`, `collapsed_rail_row_shows_its_name_while_it_has_keyboard_focus`. | The list scrolls and reserves the real height of the foot; at 720 points tall there is room for the foot plus several rows (`shell_fits_the_minimum_window`). |
| Header | Buttons are egui widgets with the default focus ring. Page pills use `ui::pill`. | The add-on browser's buttons and an add-on's page tabs switch to a menu ("Add-on actions", "Pages") when they do not fit the width left after the breadcrumb. Before, the buttons switched at a fixed 850 point window width and the page tabs never did. The width estimates are tested to be at least the real widget widths (`header_estimates_are_not_smaller_than_the_widgets`). The breadcrumb itself is not truncated: a very long add-on name beside Help could still crowd it; not measured. |
| Home | Since #182 Home shows the Health card, not the list; its actions draw `focus_ring`. | A row narrower than 560 points puts its action under the text; Home scrolls. Checked in a real window at 1060 x 720 at 100 and 150 percent (`--health-check`). |
| Picker and browser | `store::row` draws its own 1 px accent ring. | Rows keep a minimum text column; not measured. |
| `ui::pill` | Focusable, draws the same ring as the sidebar rows (`shared/toolboks-shared/src/ui.rs`), 3.83:1 or better on every fill. | Width is text plus 18 points, never wrapped; callers must fit it (the header now does). |

Gaps found and not fixed here:

1. `store::status_pill` (the status word under an add-on in the wide sidebar) is clickable and focusable but draws no focus ring.
2. The collapsed rail's status dot (`market_sidebar_status`) is a click target made with `Id::unique`, so its id changes every frame and keyboard focus cannot stay on it. It also has no ring. It is reachable by mouse only.
3. Scaling at 125 to 200 percent has not been looked at on screen.

Spot check in a real window (preview build, own data folder, Resource Monitor installed from a test catalog, 100 percent scaling): at 1540 x 960 and at the 1060 x 720 minimum, with the sidebar open and collapsed, and on the Add-ons page. Nothing overlapped or was cut off: at the minimum the header showed the breadcrumb, the page pills and Help (collapsed) or Help alone (open), and the Add-ons page showed all six action buttons inline. Tab moved the focus ring down the sidebar and, collapsed, the focused row showed its name ("Resource Monitor - Installed") beside it. Settings, Add-ons and Pick your tools sat at the foot in that order. A long add-on name, other add-ons and other fonts were not tried.
