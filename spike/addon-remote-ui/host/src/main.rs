//! Spike host: a window that shows the page of a separate add-on exe inside a rectangle.
//! It sends input to the add-on and paints the meshes it sends back.
//! Args: `--addon <exe>`  `--empty` (pass to add-on)  `--auto` (inject a click every 500 ms)
//!       `--animate` (ask for a frame at 60 Hz all the time)  `--secs N` (quit after N seconds)
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Pos2, Rect, Shape, TextureId};
use spike_proto::{read_msg, write_msg, FrameOut, FrameRequest, ToAddon};

struct Arrived {
    frame: FrameOut,
    bytes: usize,
    at: Instant,
}

#[derive(Default)]
struct Stats {
    frames: u32,
    bytes: u64,
    addon_us: u64,
    shapes_bytes: u64,
    meshes: u64,
    vertices: u64,
    max_bytes: usize,
    clicks_lat_ms: Vec<f32>,
    click_frames: Vec<u32>,
    host_us: u64,
    window_start: Option<Instant>,
}

struct App {
    to_addon: Sender<ToAddon>,
    from_addon: Receiver<Arrived>,
    child: std::process::Child,
    seq: u64,
    in_flight: bool,
    pending_events: Vec<Event>,
    repaint_due: Option<Instant>,
    meshes: Vec<(Rect, TextureId, Vec<u32>, Vec<egui::epaint::Vertex>)>,
    cursor: egui::CursorIcon,
    tex_map: HashMap<TextureId, TextureId>,
    last_size: [f32; 2],
    last_ppp: f32,
    pointer_inside: bool,
    animate: bool,
    auto: bool,
    started: Instant,
    last_auto: Instant,
    auto_toggle: bool,
    click: Option<(u64, Instant, u32)>,
    stats: Stats,
    quit_after: Option<Duration>,
    sent_at: Instant,
    total_frames: u64,
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let addon = val("--addon").unwrap_or_else(|| "spike-addon.exe".into());
    let mut cmd = Command::new(&addon);
    if flag("--empty") {
        cmd.arg("--empty");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start add-on exe");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    println!("addon pid {}", child.id());

    let (to_tx, to_rx) = mpsc::channel::<ToAddon>();
    std::thread::spawn(move || {
        let mut w = BufWriter::new(stdin);
        while let Ok(m) = to_rx.recv() {
            if write_msg(&mut w, &m).is_err() {
                break;
            }
        }
    });
    let (from_tx, from_rx) = mpsc::channel::<Arrived>();
    let quit_after = val("--secs")
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs);
    let app = App {
        to_addon: to_tx,
        from_addon: from_rx,
        child,
        seq: 0,
        in_flight: false,
        pending_events: Vec::new(),
        repaint_due: None,
        meshes: Vec::new(),
        cursor: egui::CursorIcon::Default,
        tex_map: HashMap::new(),
        last_size: [0.0; 2],
        last_ppp: 0.0,
        pointer_inside: false,
        animate: flag("--animate"),
        auto: flag("--auto"),
        started: Instant::now(),
        last_auto: Instant::now(),
        auto_toggle: false,
        click: None,
        stats: Stats::default(),
        quit_after,
        sent_at: Instant::now(),
        total_frames: 0,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Toolboks spike host",
        opts,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                while let Ok((frame, bytes)) = read_msg::<FrameOut>(&mut r) {
                    let a = Arrived {
                        frame,
                        bytes,
                        at: Instant::now(),
                    };
                    if from_tx.send(a).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            Ok(Box::new(app))
        }),
    )
}

impl App {
    fn absorb(&mut self, ctx: &egui::Context, a: Arrived) {
        let f = a.frame;
        // Texture changes first.
        {
            let tm = ctx.tex_manager();
            let mut tm = tm.write();
            for (remote, delta) in f.textures_set {
                match (delta.pos, self.tex_map.get(&remote)) {
                    (None, None) => {
                        let id = tm.alloc("remote".into(), delta.image, delta.options);
                        self.tex_map.insert(remote, id);
                    }
                    (_, Some(id)) => tm.set(*id, delta),
                    (Some(_), None) => {}
                }
            }
            for remote in f.textures_free {
                if let Some(id) = self.tex_map.remove(&remote) {
                    tm.free(id);
                }
            }
        }
        self.meshes = f
            .meshes
            .into_iter()
            .map(|m| (m.clip, m.texture, m.indices, m.vertices))
            .collect();
        self.cursor = f.cursor;
        if !f.copied_text.is_empty() {
            ctx.copy_text(f.copied_text);
        }
        for u in f.open_urls {
            ctx.open_url(egui::OpenUrl::new_tab(u));
        }
        self.repaint_due = f
            .repaint_after_ms
            .map(|ms| a.at + Duration::from_millis(ms));
        if self.animate {
            let due = self.sent_at + Duration::from_micros(16_667);
            self.repaint_due = Some(self.repaint_due.map_or(due, |d| d.min(due)));
        }
        self.in_flight = false;
        self.total_frames += 1;

        let s = &mut self.stats;
        s.window_start.get_or_insert(a.at);
        s.frames += 1;
        s.bytes += a.bytes as u64;
        s.max_bytes = s.max_bytes.max(a.bytes);
        s.addon_us += f.addon_us;
        s.shapes_bytes += f.shapes_bytes.unwrap_or(0) as u64;
        s.meshes += self.meshes.len() as u64;
        s.vertices += self.meshes.iter().map(|m| m.3.len() as u64).sum::<u64>();
        if let Some((seq, t0, n)) = self.click.as_mut() {
            if f.seq >= *seq {
                if *n == 0 {
                    s.clicks_lat_ms.push((a.at - *t0).as_secs_f32() * 1000.0);
                }
                *n += 1;
            }
        }
        // A new frame the add-on does not ask to follow up ends the click measurement.
        if let Some((_, _, n)) = self.click {
            if n > 0 && f.repaint_after_ms.is_none() {
                s.click_frames.push(n);
                self.click = None;
            }
        }
        if s.window_start
            .is_some_and(|t| a.at - t >= Duration::from_secs(5))
        {
            self.report(a.at);
        }
    }

    fn report(&mut self, now: Instant) {
        let s = &mut self.stats;
        let secs = s
            .window_start
            .map_or(1.0, |t| (now - t).as_secs_f32().max(0.001));
        let n = s.frames.max(1) as f32;
        let mut lat = s.clicks_lat_ms.clone();
        lat.sort_by(|a, b| a.total_cmp(b));
        let pct = |p: f32| {
            lat.get(((lat.len() as f32 - 1.0) * p).round() as usize)
                .copied()
        };
        println!(
            "STATS t={:.0}s frames={} fps={:.1} bytes/frame avg={:.0} max={} B/s={:.0} shapes_bytes/frame={:.0} meshes/frame={:.0} verts/frame={:.0} addon_ms/frame={:.2} host_ms/frame={:.2} clicks={} click_ms p50={:?} p95={:?} max={:?} frames/click={:?}",
            self.started.elapsed().as_secs_f32(),
            s.frames,
            s.frames as f32 / secs,
            s.bytes as f32 / n,
            s.max_bytes,
            s.bytes as f32 / secs,
            s.shapes_bytes as f32 / n,
            s.meshes as f32 / n,
            s.vertices as f32 / n,
            s.addon_us as f32 / n / 1000.0,
            s.host_us as f32 / n / 1000.0,
            lat.len(),
            pct(0.5),
            pct(0.95),
            lat.last(),
            s.click_frames.iter().copied().max()
        );
        *s = Stats::default();
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        let t0 = Instant::now();
        while let Ok(a) = self.from_addon.try_recv() {
            self.absorb(ctx, a);
        }
        if self.quit_after.is_some_and(|d| self.started.elapsed() > d) {
            self.report(Instant::now());
            let _ = self.to_addon.send(ToAddon::Quit);
            std::thread::sleep(Duration::from_millis(100));
            let _ = self.child.kill();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::Panel::left("host_side")
            .exact_size(160.0)
            .show(root, |ui| {
                ui.heading("Toolboks host");
                ui.label("Resource Monitor");
                ui.label("(add-on process)");
                ui.separator();
                ui.label(format!("frames {}", self.total_frames));
            });

        let mut rect = Rect::NOTHING;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
                rect = ui.available_rect_before_wrap();
                ui.allocate_rect(rect, egui::Sense::hover());
            });
        let ppp = ctx.pixels_per_point();
        let size = [rect.width(), rect.height()];

        // Collect the input that belongs to the view, in view coordinates.
        let mut events: Vec<Event> = Vec::new();
        let (raw_events, modifiers, focused) = ctx.input(|i| {
            (
                i.events.clone(),
                i.modifiers,
                i.viewport().focused.unwrap_or(true),
            )
        });
        for ev in raw_events {
            match ev {
                Event::PointerMoved(p) => {
                    if rect.contains(p) {
                        self.pointer_inside = true;
                        events.push(Event::PointerMoved(p - rect.min.to_vec2()));
                    } else if self.pointer_inside {
                        self.pointer_inside = false;
                        events.push(Event::PointerGone);
                    }
                }
                Event::PointerGone => {
                    self.pointer_inside = false;
                    events.push(Event::PointerGone);
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } if rect.contains(pos) => {
                    events.push(Event::PointerButton {
                        pos: pos - rect.min.to_vec2(),
                        button,
                        pressed,
                        modifiers,
                    });
                    if pressed && self.click.is_none() {
                        self.click = Some((self.seq + 1, Instant::now(), 0));
                    }
                }
                e @ Event::MouseWheel { .. } if self.pointer_inside => events.push(e),
                e @ (Event::Key { .. }
                | Event::Text(_)
                | Event::Copy
                | Event::Cut
                | Event::Paste(_)
                | Event::Ime(_)) => events.push(e),
                _ => {}
            }
        }

        // Synthetic clicks for the unattended benchmark: alternate two list rows.
        if self.auto
            && self.started.elapsed() > Duration::from_secs(4)
            && self.last_auto.elapsed() > Duration::from_millis(500)
            && self.click.is_none()
            && !self.in_flight
        {
            self.last_auto = Instant::now();
            self.auto_toggle = !self.auto_toggle;
            let pos = Pos2::new(120.0, if self.auto_toggle { 120.0 } else { 210.0 });
            let m = egui::Modifiers::NONE;
            events.push(Event::PointerMoved(pos));
            events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: m,
            });
            events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: m,
            });
            self.click = Some((self.seq + 1, Instant::now(), 0));
        }
        self.pending_events.append(&mut events);

        let now = Instant::now();
        let due = self.repaint_due.is_some_and(|t| now >= t);
        let changed = size != self.last_size || (ppp - self.last_ppp).abs() > f32::EPSILON;
        let first = self.total_frames == 0 && self.seq == 0;
        if !self.in_flight && (!self.pending_events.is_empty() || due || changed || first) {
            self.seq += 1;
            let req = FrameRequest {
                seq: self.seq,
                size,
                ppp,
                time: ctx.input(|i| i.time),
                modifiers,
                events: std::mem::take(&mut self.pending_events),
                focused,
            };
            self.last_size = size;
            self.last_ppp = ppp;
            self.in_flight = true;
            self.repaint_due = None;
            self.sent_at = Instant::now();
            let _ = self.to_addon.send(ToAddon::Frame(req));
        } else if let Some(t) = self.repaint_due {
            ctx.request_repaint_after(t.saturating_duration_since(now));
        }
        if self.auto || self.animate {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.quit_after.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // Paint what the add-on drew.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("remote_view"),
        ));
        let off = rect.min.to_vec2();
        for (clip, tex, indices, vertices) in &self.meshes {
            let Some(host_tex) = (match tex {
                TextureId::Managed(_) => self.tex_map.get(tex).copied(),
                other => Some(*other),
            }) else {
                continue;
            };
            let mut mesh = egui::epaint::Mesh::with_texture(host_tex);
            mesh.indices = indices.clone();
            mesh.vertices = vertices
                .iter()
                .map(|v| egui::epaint::Vertex {
                    pos: v.pos + off,
                    ..*v
                })
                .collect();
            let c = clip.translate(off).intersect(rect);
            painter.with_clip_rect(c).add(Shape::mesh(mesh));
        }
        if self.pointer_inside {
            ctx.set_cursor_icon(self.cursor);
        }
        self.stats.host_us += t0.elapsed().as_micros() as u64;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
