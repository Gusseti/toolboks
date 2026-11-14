//! Add-on view tests. No window: egui runs headless, the add-on is either a recorded fake
//! link (deterministic, with made-up instants) or the SDK runtime on a thread over in-memory
//! pipes.

use super::input::InputRouter;
use super::*;
use eframe::egui::{pos2, vec2, Event, Modifiers, PointerButton, RawInput};
use std::collections::BTreeMap;
use std::thread::JoinHandle;
use toolboks_addon_proto::memory::duplex;
use toolboks_addon_proto::{host_handshake, Hello};
use toolboks_addon_sdk::runtime::run_addon_on;
use toolboks_addon_sdk::NativeAddon;

const WAIT: Duration = Duration::from_secs(10);

fn page() -> Rect {
    Rect::from_min_max(pos2(100.0, 50.0), pos2(500.0, 350.0))
}

fn pass(
    ctx: &egui::Context,
    events: Vec<Event>,
    mut body: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| body(ui));
    // No renderer here: drop the texture changes on purpose.
    output.textures_delta.clear();
    output
}

fn button(x: f32, y: f32, pressed: bool) -> Event {
    Event::PointerButton {
        pos: pos2(x, y),
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// Records what the view sends instead of writing to a pipe.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<HostMessage>>>);

impl AddonSender for Recorder {
    fn send(&mut self, message: &HostMessage) -> Result<(), ProtocolError> {
        self.0.lock().unwrap().push(message.clone());
        Ok(())
    }
}

impl Recorder {
    fn batches(&self) -> Vec<InputBatch> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|message| match message {
                HostMessage::Input(batch) => Some(batch.clone()),
                _ => None,
            })
            .collect()
    }
}

type ToView = mpsc::Sender<Result<AddonMessage, ProtocolError>>;

fn fake_link() -> (AddonLink, Recorder, ToView) {
    let recorder = Recorder::default();
    let (tx, rx) = mpsc::channel();
    let sender: Box<dyn AddonSender> = Box::new(recorder.clone());
    (
        AddonLink::new(Arc::new(Mutex::new(sender)), rx),
        recorder,
        tx,
    )
}

fn frame(seq: u64) -> Result<AddonMessage, ProtocolError> {
    Ok(AddonMessage::Frame(Frame {
        seq,
        ..Default::default()
    }))
}

/// The sample add-on: one button at (10, 10)-(110, 40) that counts clicks.
#[derive(Default)]
struct Counter {
    count: u64,
}

impl NativeAddon for Counter {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(100.0, 30.0));
        let clicked = ui.put(rect, egui::Button::new("Add one")).clicked();
        ui.label(format!("Count: {}", self.count));
        if clicked {
            self.count += 1;
            preferences.insert("count".into(), self.count.into());
        }
        clicked
    }
}

fn spawn_counter(
    ctx: &egui::Context,
) -> (
    AddonLink,
    mpsc::Receiver<AddonMessage>,
    JoinHandle<Result<(), ProtocolError>>,
) {
    let (host, addon) = duplex();
    let thread = std::thread::spawn(move || run_addon_on(addon, Counter::default(), "counter"));
    let (mut reader, mut writer) = host.split();
    host_handshake(&mut reader, &mut writer, &Hello::new("counter", "test")).expect("handshake");
    let (side_tx, side_rx) = mpsc::channel();
    let link = AddonLink::spawn(reader, writer, ctx, side_tx).expect("reader thread");
    (link, side_rx, thread)
}

/// Runs passes with the real clock until `until` holds.
fn pump(ctx: &egui::Context, view: &mut AddonView, until: impl Fn(&AddonView) -> bool) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        pass(ctx, Vec::new(), |ui| {
            view.show(ui, page());
        });
        if until(view) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the view stayed {:?}", view.state());
}

fn allocated(ctx: &egui::Context) -> usize {
    ctx.tex_manager().read().num_allocated()
}

#[test]
fn input_is_relative_to_the_page_at_high_dpi() {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(2.0);
    pass(&ctx, Vec::new(), |_| {});
    let mut view = AddonView::new("Demo", &ctx);
    let (link, recorder, _tx) = fake_link();
    view.attach(link);
    let events = vec![
        Event::PointerMoved(pos2(150.0, 80.0)),
        button(150.0, 80.0, true),
    ];
    pass(&ctx, events, |ui| {
        view.show_at(ui, page(), Instant::now());
    });
    let batches = recorder.batches();
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(batch.pixels_per_point, 2.0);
    assert_eq!(batch.size, [400.0, 300.0]);
    assert_eq!(
        batch.events,
        vec![
            InputEvent::FocusGained { via_tab: None },
            InputEvent::PointerMoved { pos: [50.0, 30.0] },
            InputEvent::PointerButton {
                pos: [50.0, 30.0],
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]
    );
    assert!(view.focused());
}

#[test]
fn drag_released_outside_after_a_drag_inside() {
    let mut router = InputRouter::default();
    let mut out = Vec::new();
    let drag = [
        button(150.0, 80.0, true),
        Event::PointerMoved(pos2(600.0, 400.0)),
        button(600.0, 400.0, false),
    ];
    router.route(&drag, page(), false, &mut out);
    assert_eq!(
        out,
        vec![
            InputEvent::PointerButton {
                pos: [50.0, 30.0],
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            InputEvent::PointerMoved {
                pos: [500.0, 350.0]
            },
            InputEvent::DragReleasedOutside {
                pos: [500.0, 350.0],
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            },
        ]
    );
    assert!(!router.captured());

    // A press outside the page is not the add-on's business.
    out.clear();
    let outside = [
        button(10.0, 10.0, true),
        Event::PointerMoved(pos2(20.0, 20.0)),
        button(20.0, 20.0, false),
    ];
    router.route(&outside, page(), false, &mut out);
    assert!(out.is_empty(), "{out:?}");

    // Hover in, then out.
    let hover = [
        Event::PointerMoved(pos2(150.0, 80.0)),
        Event::PointerMoved(pos2(10.0, 10.0)),
    ];
    router.route(&hover, page(), false, &mut out);
    assert_eq!(
        out,
        vec![
            InputEvent::PointerMoved { pos: [50.0, 30.0] },
            InputEvent::PointerLeft
        ]
    );

    // Keyboard and text only while the page has focus.
    out.clear();
    let text = [Event::Text("a".into()), Event::Copy];
    router.route(&text, page(), false, &mut out);
    assert!(out.is_empty());
    router.route(&text, page(), true, &mut out);
    assert_eq!(out, vec![InputEvent::Text("a".into()), InputEvent::Copy]);
}

#[test]
fn pacing_sends_one_batch_in_flight_and_nothing_when_idle() {
    let ctx = egui::Context::default();
    let mut view = AddonView::new("Demo", &ctx);
    let (link, recorder, tx) = fake_link();
    view.attach(link);
    let t0 = Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    let run = |view: &mut AddonView, at: Instant, events: Vec<Event>| {
        pass(&ctx, events, |ui| {
            view.show_at(ui, page(), at);
        })
    };
    let inside = || vec![Event::PointerMoved(pos2(150.0, 80.0))];

    run(&mut view, ms(0), Vec::new());
    assert_eq!(
        recorder.batches().len(),
        1,
        "the first batch asks for a frame"
    );
    run(&mut view, ms(100), inside());
    assert_eq!(recorder.batches().len(), 1, "one batch in flight");

    tx.send(frame(1)).unwrap();
    run(&mut view, ms(200), Vec::new());
    assert_eq!(view.state(), &ViewState::Running);
    assert_eq!(
        recorder.batches().len(),
        2,
        "input queued meanwhile goes now"
    );

    tx.send(frame(2)).unwrap();
    run(&mut view, ms(205), inside());
    assert_eq!(recorder.batches().len(), 2, "60 batches a second at most");
    run(&mut view, ms(220), Vec::new());
    assert_eq!(recorder.batches().len(), 3);

    // The add-on asks for a frame in 100 ms (an animation): not sooner.
    tx.send(Ok(AddonMessage::Frame(Frame {
        seq: 3,
        repaint_after_ms: Some(100),
        ..Default::default()
    })))
    .unwrap();
    run(&mut view, ms(230), Vec::new());
    run(&mut view, ms(300), Vec::new());
    assert_eq!(recorder.batches().len(), 3);
    run(&mut view, ms(331), Vec::new());
    assert_eq!(recorder.batches().len(), 4);

    // Idle: no input, no request, no batch and no repaint.
    tx.send(frame(4)).unwrap();
    run(&mut view, ms(1000), Vec::new());
    let output = run(&mut view, ms(2000), Vec::new());
    assert_eq!(recorder.batches().len(), 4);
    let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    assert_eq!(repaint, Duration::MAX, "an idle add-on costs no repaint");

    // A repaint request from the add-on is honored at its time.
    tx.send(Ok(AddonMessage::RequestRepaint { after_ms: 50 }))
        .unwrap();
    run(&mut view, ms(2010), Vec::new());
    assert_eq!(recorder.batches().len(), 4);
    run(&mut view, ms(2061), Vec::new());
    assert_eq!(recorder.batches().len(), 5);
}

#[test]
fn state_machine_with_a_fake_link() {
    let ctx = egui::Context::default();
    let mut view = AddonView::new("Demo", &ctx);
    assert_eq!(view.state(), &ViewState::Connecting);
    assert_eq!(view.note().as_deref(), Some("Starting Demo…"));
    let (link, _recorder, tx) = fake_link();
    view.attach(link);
    let t0 = Instant::now();
    let run = |view: &mut AddonView, at: Instant| {
        pass(&ctx, Vec::new(), |ui| {
            view.show_at(ui, page(), at);
        });
    };
    run(&mut view, t0);
    tx.send(frame(1)).unwrap();
    run(&mut view, t0 + Duration::from_millis(20));
    assert_eq!(view.state(), &ViewState::Running);
    assert_eq!(view.note(), None);

    // The heartbeat says stuck, then alive again.
    view.set_stalled(true);
    assert_eq!(view.state(), &ViewState::Stalled);
    assert_eq!(
        view.note().as_deref(),
        Some("Demo is not responding. Waiting for it…")
    );
    view.set_stalled(false);
    assert_eq!(view.state(), &ViewState::Running);

    // A batch with no frame for the stall timeout.
    pass(&ctx, vec![Event::PointerMoved(pos2(150.0, 80.0))], |ui| {
        view.show_at(ui, page(), t0 + Duration::from_millis(40));
    });
    run(&mut view, t0 + Duration::from_secs(6));
    assert_eq!(view.state(), &ViewState::Stalled);
    tx.send(frame(2)).unwrap();
    run(&mut view, t0 + Duration::from_secs(7));
    assert_eq!(view.state(), &ViewState::Running);

    // The connection drops and nobody restarts it.
    tx.send(Err(ProtocolError::Closed)).unwrap();
    run(&mut view, t0 + Duration::from_secs(8));
    assert_eq!(
        view.state(),
        &ViewState::Stopped("The connection was closed.".into())
    );
}

#[test]
fn applies_cursor_clipboard_and_only_safe_links() {
    let ctx = egui::Context::default();
    let mut view = AddonView::new("Demo", &ctx);
    let (link, _recorder, tx) = fake_link();
    view.attach(link);
    tx.send(Ok(AddonMessage::Frame(Frame {
        seq: 1,
        cursor: egui::CursorIcon::PointingHand,
        copied_text: Some("copied".into()),
        open_urls: vec!["https://example.com/".into(), "file:///C:/x.exe".into()],
        ..Default::default()
    })))
    .unwrap();
    let output = pass(&ctx, vec![Event::PointerMoved(pos2(150.0, 80.0))], |ui| {
        view.show_at(ui, page(), Instant::now());
    });
    let platform = output.platform_output;
    assert_eq!(platform.cursor_icon, egui::CursorIcon::PointingHand);
    let commands = format!("{:?}", platform.commands);
    assert!(commands.contains("copied"), "{commands}");
    assert!(commands.contains("https://example.com/"), "{commands}");
    assert!(!commands.contains("x.exe"), "{commands}");

    assert!(allowed_url("http://example.com"));
    assert!(allowed_url("mailto:someone@example.com"));
    assert!(!allowed_url("https:///nohost"));
    assert!(!allowed_url("javascript:alert(1)"));
    assert!(!allowed_url("ms-settings:"));
}

#[test]
fn tab_escape_hands_focus_back_to_the_host() {
    let ctx = egui::Context::default();
    let mut view = AddonView::new("Demo", &ctx);
    let (link, recorder, tx) = fake_link();
    view.attach(link);
    let t0 = Instant::now();
    let run = |view: &mut AddonView, at: u64, events: Vec<Event>| {
        pass(&ctx, events, |ui| {
            view.show_at(ui, page(), t0 + Duration::from_millis(at));
            let _ = ui.button("Next host widget");
        });
    };
    run(&mut view, 0, vec![button(150.0, 80.0, true)]);
    run(&mut view, 10, vec![button(150.0, 80.0, false)]);
    assert!(view.focused());
    tx.send(Ok(AddonMessage::Frame(Frame {
        seq: 1,
        focus_escape: Some(TabDirection::Forward),
        ..Default::default()
    })))
    .unwrap();
    run(&mut view, 100, Vec::new());
    run(&mut view, 200, Vec::new());
    assert!(!view.focused());
    let events: Vec<InputEvent> = recorder
        .batches()
        .into_iter()
        .flat_map(|batch| batch.events)
        .collect();
    assert!(events.contains(&InputEvent::FocusLost), "{events:?}");
}

#[test]
fn real_addon_paints_takes_clicks_and_restarts_dimmed() {
    let ctx = egui::Context::default();
    pass(&ctx, Vec::new(), |_| {});
    let baseline = allocated(&ctx);
    let mut view = AddonView::new("Counter", &ctx);
    let (link, side, addon) = spawn_counter(&ctx);
    view.attach(link);
    assert_eq!(view.note().as_deref(), Some("Starting Counter…"));
    pump(&ctx, &mut view, |view| {
        view.state() == &ViewState::Running && view.painted_len() > 0
    });
    assert!(view.texture_count() >= 1, "the font atlas came over");
    assert_eq!(allocated(&ctx), baseline + view.texture_count());

    // Click the add-on's button, given in page coordinates (10..110, 10..40).
    let at = page().min + vec2(60.0, 25.0);
    for pressed in [true, false] {
        pass(
            &ctx,
            vec![Event::PointerMoved(at), button(at.x, at.y, pressed)],
            |ui| {
                view.show(ui, page());
            },
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let deadline = Instant::now() + WAIT;
    let mut clicked = false;
    while !clicked && Instant::now() < deadline {
        pass(&ctx, Vec::new(), |ui| {
            view.show(ui, page());
        });
        while let Ok(message) = side.try_recv() {
            clicked |= matches!(message, AddonMessage::Preferences { changed: true, .. });
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(clicked, "the click reached the add-on's button");

    // Restart: the last frame stays, dimmed, with a note.
    view.begin_restart();
    assert!(addon.join().expect("add-on thread").is_ok());
    pass(&ctx, Vec::new(), |ui| {
        view.show(ui, page());
    });
    assert_eq!(view.state(), &ViewState::Restarting);
    assert!(view.dimmed());
    assert!(view.painted_len() > 0);
    assert_eq!(view.note().as_deref(), Some("Counter is restarting…"));

    let (link, _side, addon) = spawn_counter(&ctx);
    view.attach(link);
    assert_eq!(view.state(), &ViewState::Restarting);
    pump(&ctx, &mut view, |view| view.state() == &ViewState::Running);
    assert_eq!(
        allocated(&ctx),
        baseline + view.texture_count(),
        "the old process's textures are freed"
    );

    // Dropping the view frees every texture and closes the connection.
    drop(view);
    assert_eq!(allocated(&ctx), baseline);
    assert!(addon.join().expect("add-on thread").is_ok());
}

#[test]
fn a_bad_texture_stops_the_view_instead_of_panicking() {
    let ctx = egui::Context::default();
    pass(&ctx, Vec::new(), |_| {});
    let baseline = allocated(&ctx);
    let mut view = AddonView::new("Demo", &ctx);
    let (link, _recorder, tx) = fake_link();
    view.attach(link);
    let image = egui::ColorImage {
        size: [4, 4],
        source_size: vec2(4.0, 4.0),
        pixels: vec![egui::Color32::WHITE; 3],
    };
    tx.send(Ok(AddonMessage::Frame(Frame {
        seq: 1,
        textures_set: vec![toolboks_addon_proto::TextureUpdate {
            id: egui::TextureId::Managed(0),
            delta: egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR),
        }],
        ..Default::default()
    })))
    .unwrap();
    pass(&ctx, Vec::new(), |ui| {
        view.show_at(ui, page(), Instant::now());
    });
    assert_eq!(
        view.state(),
        &ViewState::Stopped(
            "Malformed message: texture Managed(0) says 4x4 but carries 3 pixels".into()
        )
    );
    assert_eq!(
        view.note().as_deref(),
        Some("Demo stopped. Malformed message: texture Managed(0) says 4x4 but carries 3 pixels")
    );
    assert_eq!(allocated(&ctx), baseline, "nothing reached the renderer");
}
