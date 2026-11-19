//! A sample add-on built only against the SDK, driven by a test host over in-memory pipes.

use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use toolboks_addon_sdk::runtime::proto::memory::{duplex, duplex_with_capacity, PipeWriter};
use toolboks_addon_sdk::runtime::proto::mesh::{MeshCache, MeshRef};
use toolboks_addon_sdk::runtime::proto::pacing::{Heartbeat, HeartbeatMonitor};
use toolboks_addon_sdk::runtime::proto::{
    self, egui as wire_egui, AddonMessage, Frame, FrameWriter, Hello, HostMessage, InputBatch,
    InputEvent, ProtocolError, TabDirection, Theme, PROTOCOL_VERSION,
};
use toolboks_addon_sdk::runtime::{run_addon_on, MAX_QUEUED_HOST_MESSAGES};
use toolboks_addon_sdk::{Evidence, Finding, NativeAddon, Severity, StatusItem, TutorialStep};

const WAIT: Duration = Duration::from_secs(10);
const BUTTON: [f32; 2] = [60.0, 25.0];

/// The sample add-on: one button that counts clicks into its preferences.
#[derive(Default)]
struct Counter {
    count: u64,
    /// When set, `ui` blocks until the sender is dropped: a stuck add-on.
    block: Option<mpsc::Receiver<()>>,
}

impl NativeAddon for Counter {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        if let Some(block) = &self.block {
            let _ = block.recv();
        }
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 30.0));
        let clicked = ui.put(rect, egui::Button::new("Add one")).clicked();
        ui.label(format!("Count: {}", self.count));
        if clicked {
            self.count += 1;
            preferences.insert("count".into(), self.count.into());
        }
        clicked
    }

    fn status_items(&self) -> Vec<StatusItem> {
        vec![StatusItem {
            text: format!("{} clicks", self.count),
            tooltip: None,
        }]
    }

    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        vec![TutorialStep::new("main", "add", "Add", "Adds one.")]
    }
}

struct TestHost {
    writer: FrameWriter<PipeWriter>,
    inbox: mpsc::Receiver<Result<AddonMessage, ProtocolError>>,
    addon: JoinHandle<Result<(), ProtocolError>>,
    seq: u64,
}

impl TestHost {
    fn connect(addon: impl NativeAddon + Send + 'static) -> Self {
        Self::connect_at(addon, PROTOCOL_VERSION)
    }

    /// Connects as a host that speaks protocol `version`.
    fn connect_at(addon: impl NativeAddon + Send + 'static, version: u32) -> Self {
        let (host, addon_side) = duplex();
        let addon = std::thread::spawn(move || run_addon_on(addon_side, addon, "counter"));
        let (mut reader, mut writer) = host.split();
        let theirs = proto::host_handshake(
            &mut reader,
            &mut writer,
            &Hello::with_version("counter", "test", version),
        )
        .expect("handshake");
        assert_eq!(
            theirs.protocol_version, version,
            "the add-on agrees on the host's version"
        );
        assert_eq!(theirs.addon_id, "counter");
        assert_eq!(theirs.software_version, env!("CARGO_PKG_VERSION"));
        let (tx, inbox) = mpsc::channel();
        std::thread::spawn(move || loop {
            let message = reader.read::<AddonMessage>();
            let end = message.is_err();
            if tx.send(message).is_err() || end {
                break;
            }
        });
        Self {
            writer,
            inbox,
            addon,
            seq: 0,
        }
    }

    fn send(&mut self, message: HostMessage) {
        self.writer.write(&message).expect("send");
    }

    fn recv(&self) -> AddonMessage {
        self.inbox
            .recv_timeout(WAIT)
            .expect("add-on answered")
            .expect("valid message")
    }

    /// Sends one input batch and returns its frame plus every other message seen meanwhile.
    fn frame(&mut self, events: Vec<InputEvent>) -> (Frame, Vec<AddonMessage>) {
        self.seq += 1;
        let seq = self.seq;
        self.send(HostMessage::Input(InputBatch {
            seq,
            size: [400.0, 300.0],
            pixels_per_point: 1.0,
            theme: Theme::Dark,
            window_focused: true,
            events,
        }));
        let mut others = Vec::new();
        loop {
            match self.recv() {
                AddonMessage::Frame(frame) if frame.seq == seq => return (frame, others),
                other => others.push(other),
            }
        }
    }

    fn quit(mut self) -> Result<(), ProtocolError> {
        self.send(HostMessage::Quit);
        self.addon.join().expect("add-on thread")
    }
}

fn button(pressed: bool) -> InputEvent {
    InputEvent::PointerButton {
        pos: BUTTON,
        button: wire_egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// An add-on with one number and no copy code of its own.
struct Readout;

impl NativeAddon for Readout {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 30.0));
        ui.put(rect, egui::Label::new("CPU 42 %").selectable(false));
        false
    }
}

#[test]
fn ctrl_c_over_a_number_copies_it_without_the_add_on_doing_anything() {
    let mut host = TestHost::connect(Readout);
    host.frame(vec![]);
    // Over the text: the copy arrives with the frame that answers the Copy.
    host.frame(vec![InputEvent::PointerMoved { pos: [110.0, 25.0] }]);
    let (frame, _) = host.frame(vec![InputEvent::Copy]);
    assert_eq!(frame.copied_text.as_deref(), Some("CPU 42 %"));
    // Not over any text: nothing is copied.
    host.frame(vec![InputEvent::PointerMoved {
        pos: [300.0, 250.0],
    }]);
    let (frame, _) = host.frame(vec![InputEvent::Copy]);
    assert_eq!(frame.copied_text, None);
    // No Ctrl+C: nothing is copied either.
    host.frame(vec![InputEvent::PointerMoved { pos: [110.0, 25.0] }]);
    let (frame, _) = host.frame(vec![]);
    assert_eq!(frame.copied_text, None);
    host.quit().unwrap();
}

#[test]
fn hello_first_frame_and_cached_meshes() {
    let mut host = TestHost::connect(Counter::default());
    let mut cache = MeshCache::default();
    let (first, _) = host.frame(vec![]);
    assert!(!first.meshes.is_empty());
    assert!(
        !first.textures_set.is_empty(),
        "the first frame uploads the font atlas"
    );
    assert!(first
        .meshes
        .iter()
        .all(|mesh| matches!(mesh, MeshRef::New { .. })));
    cache.apply(first.meshes).unwrap();
    let mut cached = 0;
    for _ in 0..4 {
        let (frame, _) = host.frame(vec![]);
        cached += frame
            .meshes
            .iter()
            .filter(|mesh| matches!(mesh, MeshRef::Cached { .. }))
            .count();
        let drawn = cache.apply(frame.meshes).unwrap();
        assert!(!drawn.is_empty());
    }
    assert!(cached > 0, "unchanged frames reuse cached meshes");
    host.send(HostMessage::Ping { nonce: 7 });
    assert_eq!(host.recv(), AddonMessage::Pong { nonce: 7 });
    host.quit().unwrap();
}

#[test]
fn click_changes_state() {
    let mut host = TestHost::connect(Counter::default());
    host.send(HostMessage::LoadPreferences { json: "{}".into() });
    host.frame(vec![]);
    host.frame(vec![InputEvent::PointerMoved { pos: BUTTON }]);
    host.frame(vec![button(true)]);
    let (_, others) = host.frame(vec![button(false)]);
    assert!(
        others.contains(&AddonMessage::Preferences {
            json: r#"{"count":1}"#.into(),
            changed: true
        }),
        "{others:?}"
    );
    assert!(
        others.contains(&AddonMessage::StatusItems(vec![proto::StatusItem {
            text: "1 clicks".into(),
            tooltip: None
        }]))
    );
    host.send(HostMessage::SavePreferences);
    assert_eq!(
        host.recv(),
        AddonMessage::Preferences {
            json: r#"{"count":1}"#.into(),
            changed: false
        }
    );
    host.send(HostMessage::TutorialQuery);
    match host.recv() {
        AddonMessage::Tutorial { steps, .. } => assert_eq!(steps[0].target, "add"),
        other => panic!("expected the tutorial, got {other:?}"),
    }
    host.quit().unwrap();
}

#[test]
fn newer_host_is_refused_in_plain_words() {
    let (host, addon_side) = duplex();
    let addon = std::thread::spawn(move || run_addon_on(addon_side, Counter::default(), "counter"));
    let (mut reader, mut writer) = host.split();
    let mut hello = Hello::new("counter", "99.0.0");
    hello.protocol_version = PROTOCOL_VERSION + 1;
    writer.write(&HostMessage::Hello(hello)).unwrap();
    let text = format!(
        "Add-on speaks protocol {PROTOCOL_VERSION}, Toolboks speaks {}. Update the add-on or Toolboks.",
        PROTOCOL_VERSION + 1
    );
    match reader.read::<AddonMessage>().unwrap() {
        AddonMessage::Refuse { reason, .. } => assert_eq!(reason, text),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let error = addon.join().unwrap().unwrap_err();
    assert!(matches!(error, ProtocolError::VersionMismatch { .. }));
    assert_eq!(error.to_string(), text);
}

fn run_with_raw_bytes(bytes: &[u8]) -> ProtocolError {
    let (host, addon_side) = duplex();
    let addon = std::thread::spawn(move || run_addon_on(addon_side, Counter::default(), "counter"));
    let (mut reader, mut writer) = host.split();
    proto::host_handshake(&mut reader, &mut writer, &Hello::new("counter", "test")).unwrap();
    let mut raw = writer.into_inner();
    raw.write_raw(bytes).unwrap();
    drop(raw);
    addon.join().unwrap().unwrap_err()
}

#[test]
fn malformed_messages_end_the_addon_with_an_error() {
    let garbage = run_with_raw_bytes(&[4, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    assert!(
        matches!(garbage, ProtocolError::Malformed(_)),
        "{garbage:?}"
    );
    let oversize = run_with_raw_bytes(&u32::MAX.to_le_bytes());
    assert!(
        matches!(oversize, ProtocolError::FrameTooLarge { .. }),
        "{oversize:?}"
    );
    let truncated = run_with_raw_bytes(&[200, 0, 0, 0, 1, 2, 3]);
    assert!(
        matches!(truncated, ProtocolError::Truncated),
        "{truncated:?}"
    );
    let bad_preferences = {
        let mut host = TestHost::connect(Counter::default());
        host.send(HostMessage::LoadPreferences {
            json: "not json".into(),
        });
        host.addon.join().unwrap().unwrap_err()
    };
    assert!(matches!(bad_preferences, ProtocolError::Malformed(_)));
}

#[test]
fn dropped_pipe_ends_cleanly() {
    let mut host = TestHost::connect(Counter::default());
    host.frame(vec![]);
    let TestHost { writer, addon, .. } = host;
    drop(writer);
    assert!(addon.join().unwrap().is_ok());
}

#[test]
fn stalled_addon_is_detected() {
    let (unblock, block) = mpsc::channel();
    let mut host = TestHost::connect(Counter {
        block: Some(block),
        ..Default::default()
    });
    // The add-on is now stuck inside `ui` and cannot answer pings.
    host.seq += 1;
    let seq = host.seq;
    host.send(HostMessage::Input(InputBatch {
        seq,
        size: [400.0, 300.0],
        pixels_per_point: 1.0,
        theme: Theme::Light,
        window_focused: true,
        events: vec![],
    }));
    let started = Instant::now();
    let mut monitor = HeartbeatMonitor::with_timing(
        started,
        Duration::from_millis(20),
        Duration::from_millis(200),
    );
    let stalled = loop {
        assert!(started.elapsed() < WAIT, "stall not detected");
        match monitor.poll(Instant::now()) {
            Heartbeat::SendPing(nonce) => host.send(HostMessage::Ping { nonce }),
            Heartbeat::Stalled { silent_for } => break silent_for,
            Heartbeat::Alive => {}
        }
        match host.inbox.recv_timeout(Duration::from_millis(5)) {
            Ok(Ok(AddonMessage::Pong { nonce })) => monitor.on_pong(nonce, Instant::now()),
            Ok(Ok(AddonMessage::Frame(_))) => panic!("a stuck add-on sent a frame"),
            Ok(Ok(_)) => monitor.on_message(Instant::now()),
            Ok(Err(error)) => panic!("{error}"),
            Err(_) => {}
        }
    };
    assert!(stalled >= Duration::from_millis(200));
    drop(unblock);
    host.quit().unwrap();
}

/// Buttons stacked bottom to top, so traversal order is not visual order.
#[derive(Default)]
struct Buttons {
    count: usize,
    ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl NativeAddon for Buttons {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        *self.ctx.lock().unwrap() = Some(ui.ctx().clone());
        for index in 0..self.count {
            let y = 200.0 - 40.0 * index as f32;
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, y), egui::vec2(80.0, 30.0));
            ui.put(rect, egui::Button::new(format!("B{index}")));
        }
        false
    }
}

fn tab(direction: TabDirection) -> InputEvent {
    InputEvent::Key {
        key: wire_egui::Key::Tab,
        physical_key: Some(wire_egui::Key::Tab),
        pressed: true,
        repeat: false,
        modifiers: wire_egui::Modifiers {
            shift: direction == TabDirection::Backward,
            ..Default::default()
        },
    }
}

fn walk(count: usize, direction: TabDirection) {
    let label = format!("{count} widgets, {direction:?}");
    let mut host = TestHost::connect(Buttons {
        count,
        ..Default::default()
    });
    host.frame(vec![]);
    let (frame, _) = host.frame(vec![InputEvent::FocusGained {
        via_tab: Some(direction),
    }]);
    assert!(frame.wants_keyboard, "{label}: focus entered");
    assert_eq!(frame.focus_escape, None, "{label}");
    for step in 1..count {
        let (frame, _) = host.frame(vec![tab(direction)]);
        assert_eq!(frame.focus_escape, None, "{label}: step {step}");
        assert!(frame.wants_keyboard, "{label}: step {step}");
    }
    let (frame, _) = host.frame(vec![tab(direction)]);
    assert_eq!(frame.focus_escape, Some(direction), "{label}: escape");
    assert!(!frame.wants_keyboard, "{label}: escape");
    // The host owns focus now: later frames must not take it back.
    for _ in 0..3 {
        let (frame, _) = host.frame(vec![]);
        assert!(!frame.wants_keyboard, "{label}: focus came back");
    }
    host.quit().unwrap();
}

#[test]
fn tab_escapes_at_both_ends() {
    for count in [1, 3] {
        walk(count, TabDirection::Forward);
        walk(count, TabDirection::Backward);
    }
}

#[test]
fn delayed_repaint_between_frames_keeps_its_deadline() {
    let shared = Arc::new(Mutex::new(None));
    let mut host = TestHost::connect(Buttons {
        count: 1,
        ctx: shared.clone(),
    });
    let mut settled = false;
    for _ in 0..20 {
        let (frame, _) = host.frame(vec![]);
        if frame.repaint_after_ms.is_none() {
            settled = true;
            break;
        }
    }
    assert!(settled, "the page never went idle");
    let ctx = shared.lock().unwrap().clone().unwrap();
    ctx.request_repaint_after(Duration::from_millis(500));
    match host.recv() {
        AddonMessage::RequestRepaint { after_ms } => {
            assert!((300..=500).contains(&after_ms), "after_ms {after_ms}")
        }
        other => panic!("expected a repaint request, got {other:?}"),
    }
    // An earlier deadline goes out at once.
    ctx.request_repaint();
    assert_eq!(host.recv(), AddonMessage::RequestRepaint { after_ms: 0 });
    host.quit().unwrap();
}

#[test]
fn flooding_requests_while_answers_back_up_does_not_deadlock() {
    // Small pipes, like OS pipes with a finite buffer. The host writes every request before it
    // reads any answer; a reader that blocked on a bounded queue would deadlock here.
    let (host, addon_side) = duplex_with_capacity(256);
    let addon = std::thread::spawn(move || run_addon_on(addon_side, Counter::default(), "counter"));
    let (mut reader, mut writer) = host.split();
    proto::host_handshake(&mut reader, &mut writer, &Hello::new("counter", "test")).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let flood = std::thread::spawn(move || {
        for _ in 0..500 {
            writer.write(&HostMessage::TutorialQuery).unwrap();
        }
        writer.write(&HostMessage::Quit).unwrap();
        done_tx.send(()).unwrap();
        writer
    });
    done_rx
        .recv_timeout(WAIT)
        .expect("host writes blocked: duplex deadlock");
    let _writer = flood.join().unwrap();
    let mut answers = 0;
    loop {
        match reader.read::<AddonMessage>() {
            Ok(AddonMessage::Tutorial { .. }) => answers += 1,
            Ok(_) => {}
            Err(error) => {
                assert!(error.is_closed(), "{error:?}");
                break;
            }
        }
    }
    assert_eq!(answers, 500);
    addon.join().unwrap().unwrap();
}

#[test]
fn unhandled_flood_past_the_cap_ends_with_a_clear_error() {
    let (unblock, block) = mpsc::channel();
    let mut host = TestHost::connect(Counter {
        block: Some(block),
        ..Default::default()
    });
    // The add-on is stuck in `ui` while the host keeps sending.
    host.seq += 1;
    let seq = host.seq;
    host.send(HostMessage::Input(InputBatch {
        seq,
        size: [400.0, 300.0],
        pixels_per_point: 1.0,
        theme: Theme::Dark,
        window_focused: true,
        events: vec![],
    }));
    for nonce in 0..=MAX_QUEUED_HOST_MESSAGES as u64 + 1 {
        host.send(HostMessage::Ping { nonce });
    }
    drop(unblock);
    let error = host.addon.join().unwrap().unwrap_err();
    assert!(
        matches!(error, ProtocolError::Overloaded { limit } if limit == MAX_QUEUED_HOST_MESSAGES),
        "{error:?}"
    );
    assert!(error.to_string().contains("4096 messages"));
}

#[test]
fn drawing_runtime_refuses_a_helper_operation_without_running_the_addon() {
    let mut host = TestHost::connect(Counter::default());
    host.send(HostMessage::AdminOperation {
        request: proto::AdminRequest {
            id: 1,
            operation: "test".into(),
            json: "{}".into(),
        },
        capabilities: vec!["administrator".into()],
    });
    assert!(matches!(
        host.addon.join().unwrap(),
        Err(ProtocolError::Unexpected(_))
    ));
}

/// Reports health findings: one that counts `check_now` calls, one that counts background ticks.
#[derive(Default)]
struct Health {
    checks: u32,
    ticks: u32,
    background: bool,
}

impl NativeAddon for Health {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut std::collections::BTreeMap<String, serde_json::Value>,
    ) -> bool {
        ui.label("Health");
        false
    }

    fn has_background_work(&self) -> bool {
        self.background
    }

    fn tick_background(&mut self) {
        self.ticks += 1;
    }

    fn check_now(&mut self) {
        self.checks += 1;
    }

    fn status_items(&self) -> Vec<StatusItem> {
        vec![StatusItem {
            text: format!("{} checks", self.checks),
            tooltip: None,
        }]
    }

    fn findings(&self) -> Vec<Finding> {
        vec![
            Finding::attention(
                "checks",
                format!("Checked {} times", self.checks),
                "Counted",
                Severity::Watch,
            )
            .measured_at(1_700_000_000)
            .why("Counting matters.")
            .evidence(Evidence::bar(0.5, "1 used", "1 free")),
            Finding::ok("ticks", format!("{} ticks", self.ticks), "Counted")
                .measured_at(1_700_000_000),
        ]
    }
}

/// The findings among `messages`, from either layout (protocol 4 sets read back without `why`
/// and `evidence`).
fn findings_in(messages: &[AddonMessage]) -> Vec<Vec<Finding>> {
    messages
        .iter()
        .filter_map(|message| match message {
            AddonMessage::Findings(list) => Some(list.iter().cloned().map(Finding::from).collect()),
            AddonMessage::FindingsV5(list) => Some(list.clone()),
            _ => None,
        })
        .collect()
}

/// Every message that arrives within `wait`.
fn drain(host: &TestHost, wait: Duration) -> Vec<AddonMessage> {
    let until = Instant::now() + wait;
    let mut seen = Vec::new();
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        match host.inbox.recv_timeout(left) {
            Ok(message) => seen.push(message.expect("valid message")),
            Err(_) => break,
        }
    }
    seen
}

#[test]
fn findings_reach_a_protocol_5_host_with_why_and_evidence_only_when_they_change() {
    let mut host = TestHost::connect(Health::default());
    let (_, first) = host.frame(vec![]);
    assert!(
        !first.iter().any(|m| matches!(m, AddonMessage::Findings(_))),
        "{first:?}"
    );
    let sent = findings_in(&first);
    assert_eq!(sent.len(), 1, "{first:?}");
    assert_eq!(sent[0][0].title, "Checked 0 times");
    assert_eq!(sent[0][0].why.as_deref(), Some("Counting matters."));
    assert_eq!(
        sent[0][0].evidence,
        Some(Evidence::bar(0.5, "1 used", "1 free"))
    );
    assert_eq!(sent[0][1].title, "0 ticks");
    // Unchanged: nothing is sent again.
    let (_, second) = host.frame(vec![]);
    assert!(findings_in(&second).is_empty(), "{second:?}");
    // CheckNow reaches the add-on, and the changed set follows without a frame.
    host.send(HostMessage::CheckNow);
    let after = loop {
        match host.recv() {
            AddonMessage::FindingsV5(list) => break list,
            AddonMessage::StatusItems(_) => {}
            other => panic!("expected findings, got {other:?}"),
        }
    };
    assert_eq!(after[0].title, "Checked 1 times");
    host.quit().unwrap();
}

#[test]
fn a_host_at_protocol_4_gets_the_old_layout_without_why_and_evidence() {
    let mut host = TestHost::connect_at(Health::default(), 4);
    let (_, first) = host.frame(vec![]);
    assert!(
        !first
            .iter()
            .any(|m| matches!(m, AddonMessage::FindingsV5(_))),
        "{first:?}"
    );
    let old: Vec<_> = first
        .iter()
        .filter_map(|m| match m {
            AddonMessage::Findings(list) => Some(list.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(old.len(), 1, "{first:?}");
    assert_eq!(old[0][0].title, "Checked 0 times");
    host.send(HostMessage::CheckNow);
    loop {
        match host.recv() {
            AddonMessage::Findings(list) => {
                assert_eq!(list[0].title, "Checked 1 times");
                break;
            }
            AddonMessage::StatusItems(_) => {}
            other => panic!("expected protocol 4 findings, got {other:?}"),
        }
    }
    host.quit().unwrap();
}

#[test]
fn findings_are_sent_from_background_ticks_while_suspended() {
    let mut host = TestHost::connect(Health {
        background: true,
        ..Default::default()
    });
    host.frame(vec![]);
    host.send(HostMessage::Suspend);
    // No input at all from here: only background ticks (every 200 ms) change the findings.
    let seen = drain(&host, Duration::from_millis(1500));
    let sent = findings_in(&seen);
    assert!(sent.len() >= 2, "findings from background ticks: {seen:?}");
    assert!(sent
        .iter()
        .all(|list| list.len() == 2 && list[1].title != "0 ticks"));
    assert!(!seen.iter().any(|m| matches!(m, AddonMessage::Frame(_))));
    host.quit().unwrap();
}

#[test]
fn a_host_at_protocol_3_gets_no_findings() {
    let mut host = TestHost::connect_at(
        Health {
            background: true,
            ..Default::default()
        },
        3,
    );
    let (_, mut seen) = host.frame(vec![]);
    assert!(
        seen.iter()
            .any(|m| matches!(m, AddonMessage::StatusItems(_))),
        "the add-on runs as usual: {seen:?}"
    );
    host.frame(vec![]);
    host.send(HostMessage::Suspend);
    // Background ticks change the findings, but none may be sent at protocol 3.
    seen.extend(drain(&host, Duration::from_millis(1000)));
    host.send(HostMessage::Resume);
    let (_, more) = host.frame(vec![]);
    seen.extend(more);
    assert!(findings_in(&seen).is_empty(), "{seen:?}");
    host.quit().unwrap();
}

#[test]
fn check_now_on_a_protocol_3_connection_ends_the_addon() {
    // A protocol 3 host cannot send CheckNow (it does not exist there); a peer that does is
    // broken, as a protocol 3 add-on would see it.
    let mut host = TestHost::connect_at(Health::default(), 3);
    host.frame(vec![]);
    host.send(HostMessage::CheckNow);
    let error = host.addon.join().unwrap().unwrap_err();
    assert!(matches!(error, ProtocolError::Unexpected(_)), "{error:?}");
}

#[test]
fn a_host_at_protocol_6_is_refused_in_plain_words() {
    let (host, addon_side) = duplex();
    let addon = std::thread::spawn(move || run_addon_on(addon_side, Health::default(), "counter"));
    let (mut reader, mut writer) = host.split();
    writer
        .write(&HostMessage::Hello(Hello::with_version(
            "counter", "9.0.0", 6,
        )))
        .unwrap();
    let text = "Add-on speaks protocol 5, Toolboks speaks 6. Update the add-on or Toolboks.";
    match reader.read::<AddonMessage>().unwrap() {
        AddonMessage::Refuse {
            protocol_version,
            reason,
        } => {
            assert_eq!(protocol_version, 5);
            assert_eq!(reason, text);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(addon.join().unwrap().unwrap_err().to_string(), text);
}
