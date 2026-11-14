//! Spike add-on: runs the real Resource Monitor page in a headless egui `Context` and answers
//! frame requests from the host on stdin/stdout. Args: `--empty` draws one label instead.
#![allow(dead_code, unused_imports)]
use std::io::{BufReader, BufWriter, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::epaint::Primitive;
use spike_proto::{read_msg, write_msg, FrameOut, FrameRequest, ToAddon, WireMesh};

#[path = "../../../../src/format.rs"]
mod format;
mod tools;
#[path = "../../../../src/ui.rs"]
mod ui;
#[path = "../../../../src/win.rs"]
mod win;

use tools::Tool;

enum Wake {
    Request(FrameRequest),
    Repaint,
    Quit,
}

/// Merges a newer request into the unanswered one so one frame answers all of them.
fn merge(pending: &mut Option<FrameRequest>, r: FrameRequest) {
    match pending.as_mut() {
        Some(p) => {
            p.events.extend(r.events);
            p.seq = r.seq;
            p.size = r.size;
            p.ppp = r.ppp;
            p.time = r.time;
            p.modifiers = r.modifiers;
            p.focused = r.focused;
        }
        None => *pending = Some(r),
    }
}

fn same_state(r: &FrameRequest) -> FrameRequest {
    FrameRequest {
        seq: r.seq,
        size: r.size,
        ppp: r.ppp,
        time: r.time,
        modifiers: r.modifiers,
        events: Vec::new(),
        focused: r.focused,
    }
}

fn main() {
    let empty = std::env::args().any(|a| a == "--empty");
    let ctx = egui::Context::default();
    ui::apply_style(&ctx);
    let mut tool = tools::resources::Resources::new();

    let (tx, rx) = mpsc::channel::<Wake>();
    let tx_repaint = tx.clone();
    ctx.set_request_repaint_callback(move |_info| {
        let _ = tx_repaint.send(Wake::Repaint);
    });
    std::thread::spawn(move || {
        let mut stdin = BufReader::new(std::io::stdin().lock());
        loop {
            match read_msg::<ToAddon>(&mut stdin) {
                Ok((ToAddon::Frame(req), _)) => {
                    if tx.send(Wake::Request(req)).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = tx.send(Wake::Quit);
                    break;
                }
            }
        }
    });
    let mut out = BufWriter::new(std::io::stdout().lock());

    let mut pending: Option<FrameRequest> = None;
    let mut repaint_at: Option<Instant> = None;
    let mut last_req = FrameRequest {
        size: [800.0, 600.0],
        ppp: 1.0,
        ..Default::default()
    };
    let mut ppp = 0.0f32;
    // The add-on keeps its own clock: add-on initiated repaints must see time advance too.
    let clock = Instant::now();
    let mut dbg = [0u32; 4]; // request, repaint wake, timeout, delay0
    let mut dbg_at = Instant::now();
    loop {
        let first = match repaint_at {
            Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    dbg[2] += 1;
                    pending.get_or_insert_with(|| same_state(&last_req));
                    None
                }
                Err(_) => break,
            },
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
        };
        for m in first
            .into_iter()
            .chain(std::iter::from_fn(|| rx.try_recv().ok()))
        {
            match m {
                Wake::Quit => return,
                Wake::Request(r) => {
                    dbg[0] += 1;
                    merge(&mut pending, r)
                }
                Wake::Repaint => {
                    dbg[1] += 1;
                    pending.get_or_insert_with(|| same_state(&last_req));
                }
            }
        }
        let Some(req) = pending.take() else { continue };
        let started = Instant::now();
        if (req.ppp - ppp).abs() > f32::EPSILON {
            ppp = req.ppp;
            ctx.set_pixels_per_point(ppp);
        }
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(req.size[0], req.size[1]),
            )),
            time: Some(clock.elapsed().as_secs_f64()),
            events: req.events.clone(),
            focused: req.focused,
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(req.ppp);
        if !empty {
            tool.tick(&ctx);
        }
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                if empty {
                    ui.label("empty add-on");
                } else {
                    tool.ui(ui, 0);
                }
            });
        });
        // `epaint::Shape` and `ClippedShape` have no serde impl in 0.37: only meshes round-trip.
        let shapes_bytes: Option<u32> = None;
        let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
        let meshes: Vec<WireMesh> = prims
            .into_iter()
            .filter_map(|p| match p.primitive {
                Primitive::Mesh(m) => Some(WireMesh {
                    clip: p.clip_rect,
                    texture: m.texture_id,
                    indices: m.indices,
                    vertices: m.vertices,
                }),
                Primitive::Callback(_) => None,
            })
            .collect();
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay);
        let repaint_after_ms = match delay {
            Some(d) if d == Duration::MAX => None,
            Some(d) => Some(d.as_millis() as u64),
            None => None,
        };
        if repaint_after_ms == Some(0) {
            dbg[3] += 1;
            if dbg[3] > 40 && dbg[3] % 37 == 0 {
                eprintln!("HOT causes={:?}", ctx.repaint_causes());
            }
        }
        if std::env::var_os("SPIKE_DBG").is_some() && dbg_at.elapsed() > Duration::from_secs(2) {
            eprintln!(
                "addon wake: request={} repaint_cb={} timeout={} delay0={} causes={:?}",
                dbg[0],
                dbg[1],
                dbg[2],
                dbg[3],
                ctx.repaint_causes()
            );
            dbg = [0; 4];
            dbg_at = Instant::now();
        }
        repaint_at = repaint_after_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
        let tex_set = std::mem::take(&mut output.textures_delta.set);
        let tex_free = std::mem::take(&mut output.textures_delta.free);
        let frame = FrameOut {
            seq: req.seq,
            meshes,
            textures_set: tex_set
                .into_iter()
                .flat_map(|(id, ds)| ds.into_iter().map(move |d| (id, d)))
                .collect(),
            textures_free: tex_free.into_iter().collect(),
            cursor: output.platform_output.cursor_icon,
            copied_text: output
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
                .collect::<String>(),
            open_urls: output
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::OpenUrl(u) => Some(u.url.clone()),
                    _ => None,
                })
                .collect(),
            repaint_after_ms,
            addon_us: started.elapsed().as_micros() as u64,
            shapes_bytes,
        };
        last_req = same_state(&req);
        if write_msg(&mut out, &frame).is_err() {
            break;
        }
    }
    let _ = out.flush();
}
