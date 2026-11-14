# B1 spike result: #78, draw an add-on's egui page in the host window from a separate exe

**Verdict: GO** for remote drawing as the add-on transport, with the conditions at the end. The RAM number needs one owner-visible note (see RAM).

Code: branch `spike/addon-remote-ui`, folder `spike/addon-remote-ui/` (its own Cargo workspace, nothing depends on it, no PR).
It was cut from staging a0a6df3. Staging has since moved (#98 moved `src/ui.rs`, `src/format.rs`, `src/win.rs` into shared crates), so the `#[path]` includes of those files no longer resolve on new staging. The branch is a read-only record, not meant to be rebased.

## What was built

- `addon/`: headless exe. Runs the real Resource Monitor module from `src/tools/resources/` (included unchanged by `#[path]`, with the real `ui.rs` theme) in an `egui::Context`, Overview page, no window, no eframe (a one-line `eframe` shim crate re-exports egui).
- `host/`: eframe/glow window with a fake sidebar. It forwards pointer, wheel, key, text and size to the add-on, paints the meshes the add-on sends in a rectangle, maps the font-atlas texture, and applies cursor, copied text and links.
- `proto/`: postcard messages with a u32 length prefix.
- Transport: the child's stdin/stdout (anonymous pipes), not a named pipe. A synchronous duplex named pipe handle cannot be read and written from two threads on Windows, so a named pipe needs two pipes or overlapped I/O; the kernel path is the same, so the numbers should carry over. A named pipe was not measured.
- Backpressure: one frame in flight, events queue and merge while the add-on is busy.
- Verified visually with screenshots: the real Overview page renders correctly in the host window and a synthetic click on a list row switches the detail pane (CPU -> Memory).

## Numbers (release profile like the app: opt-level z, LTO, panic abort; i7-9700K, Windows 11, 15 s window after 12 s warm-up, machine not idle)

| Measure | Result |
|---|---|
| Add-on exe size | 4.7 MB (host 5.2 MB) |
| RAM, add-on with real Resource Monitor page | working set 54 MB, private commit 36.5 MB |
| RAM, add-on with an empty page (egui runtime + fonts only) | working set 16 MB, private commit 4 MB |
| Threads / handles, real page | 8-9 / about 720 |
| CPU, idle, page open (about 3 frames/s from the 1 s sampler) | add-on 2.4 % of one core, host 0.3 % |
| CPU, forced 60 frames/s (worst case for an animating graph) | add-on 7.1 %, host 23 % (host rebuilds and re-uploads every mesh each frame, not optimised) |
| Click to first repainted frame at the host (synthetic click, host to pipe to add-on to pipe to host) | median 1.3-1.8 ms, p95 1.5-3.7 ms, one outlier of 245 ms in about 40 clicks |
| Frames per click | 3 (the click frame plus 2 settling frames) |
| Add-on time to run, tessellate and encode one frame | 0.3-0.5 ms (5 ms average in the window that held the outlier) |
| Bytes per frame, Overview, CPU selected, 1 min range | 140 KB at start, growing to 200 KB as graph lines fill (about 5.5k to 8k vertices, 31 meshes) |
| Bytes per frame, Overview, Memory selected | 84-92 KB |
| Wire rate idle | 0.4-0.6 MB/s; at 60 frames/s 9-12 MB/s |
| First frame | 260-370 KB (font atlas) |
| Empty page frame | 1.2 KB |

Click-to-pixels is these numbers plus one host frame to paint (at most 16.7 ms at 60 Hz). That one-frame wait was not measured on a real display, and real OS mouse input was not used for the timing; the click was injected in the host.
Private working set (the number a reviewer would want for "extra RAM") was not captured: the counter query returned nothing. Working set includes shared DLL pages, commit is an upper bound on private memory, so the real private figure lies between the two.

### RAM against the budget
The real page costs 36.5 MB commit / 54 MB working set, above the 25-30 MB budgets. But 32 MB of that is the page's own data: the sampler, `sysinfo` process table, GPU counters, graph histories and the Segoe UI font set. The same data lives in the host today. What the remote-drawing design itself adds is about the empty-page figure, 4 MB commit / 16 MB working set per process, plus a duplicated font atlas. I did not measure the in-process Resource Monitor for a before/after, so "extra RAM versus today" is an estimate, not a result. The per-add-on budget in #97 should be defined as overhead beyond the page's own data, or heavy add-ons will fail it whatever the transport.

## What breaks or needs work

1. `epaint::Shape` and `ClippedShape` have no serde impl in 0.37 (only meshes, textures, events and input types round-trip). The issue's plan of sending shapes and tessellating in the host does not work as written; the spike sends tessellated meshes. That makes frames large: about 20 bytes per vertex plus indices, no delta.
2. The add-on needs its own clock. Reusing the host's last frame time for repaints the add-on starts itself froze egui animations (scroll-bar fade) and caused unthrottled repaint bursts at 100-140 frames/s. Fixed in the spike by using a clock inside the add-on.
3. Add-on-initiated repaints (sampler data, animation) have no rate limit in the protocol; the host must cap frames (the spike only caps the forced animate mode at 60 Hz).
4. Three frames per click: egui settles over 2 more passes. Fine at 1-4 ms; costs bytes (3 x 90-200 KB per click).
5. One 245 ms stall in an add-on frame was seen once; cause not found (a sampler refresh or the machine's load are suspects).
6. Not tested at all, so unknown: text editing and selection, IME, clipboard round trip, drag that ends outside the view rectangle (release events outside are dropped by the spike), Tab and focus routing between host and add-on (keys are forwarded unconditionally), high DPI other than this machine's, window resize while animating, scroll areas driven by wheel, images other than the font atlas, links, cursor icon changes, two add-ons at once, add-on crash and restart.
7. The startup transient: the first seconds run at 100+ frames/s while egui settles and the sampler starts (partly the clock bug above; not re-measured as a separate case after the fix).

## Conditions for the GO

- Send only changed meshes or a cheaper encoding (cache per-widget meshes, or ship shapes with a custom serde for the few shape kinds used), and cap frames at 60/s on the host; otherwise idle bandwidth stays near 0.5 MB/s and animation near 10 MB/s per add-on.
- Add-on keeps its own clock; host sends only input, size and scale.
- Make the host paint path cheaper (cache the translated meshes) before judging host CPU during animation.
- #97 needs a clear RAM metric (private working set, overhead beyond page data).
- Test the items in 6 on the pilot (#88) before the rest move.

## Fallback if the owner rejects the RAM or bandwidth cost: a separate window per add-on

Each add-on exe owns its eframe window, drawn as today, with the host launching and tracking it. Costs: an own GL context and window per add-on (RAM cost not measured here), no shared sidebar chrome, and window management for the user. Benefits: no protocol, no input forwarding, no wire cost.

## Not done

No Opus review (the one allowed for numbers close to the budgets): the numbers are not close to a budget line, they are above it on one reading and below it on another, which is a definition question for the owner (see RAM), not for a reviewer.
`cargo fmt` was run in the spike workspace. `cargo clippy -D warnings` was not run on it: it includes app files that carry their own dead code, and it is throwaway.
