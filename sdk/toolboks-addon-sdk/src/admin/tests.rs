use super::*;
use proto::memory::duplex;
use proto::{InputBatch, InputEvent, Theme};
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

struct Change(Arc<AtomicUsize>);
impl AdminChange for Change {
    fn capability(&self) -> &str {
        "administrator"
    }
    fn description(&self) -> String {
        "Change this test counter once.".into()
    }
    fn apply(self: Box<Self>) -> Result<String, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("{}".into())
    }
}
fn request() -> AdminRequest {
    AdminRequest {
        id: 7,
        operation: "test_change".into(),
        json: "{}".into(),
    }
}
fn caps() -> Vec<String> {
    vec!["administrator".into()]
}
fn input(events: Vec<InputEvent>) -> HostMessage {
    HostMessage::Input(InputBatch {
        seq: 1,
        size: [600.0, 400.0],
        pixels_per_point: 1.0,
        theme: Theme::Dark,
        window_focused: true,
        events,
    })
}
#[test]
fn cancel_performs_no_change_and_one_confirmation_runs_exactly_once() {
    let writes = Arc::new(AtomicUsize::new(0));
    for answer in [false, true] {
        let outcome = perform(
            &request(),
            &caps(),
            true,
            &AtomicBool::new(false),
            |_| Ok(Box::new(Change(writes.clone()))),
            |_, _| Ok(answer),
        );
        assert_eq!(writes.load(Ordering::SeqCst), usize::from(answer));
        assert_eq!(
            outcome,
            if answer {
                AdminOutcome::Completed { json: "{}".into() }
            } else {
                AdminOutcome::Cancelled
            }
        );
    }
}
#[test]
fn missing_rights_or_capability_never_opens_a_confirmation_or_applies() {
    for (elevated, capabilities) in [(false, caps()), (true, vec![])] {
        let outcome = perform(
            &request(),
            &capabilities,
            elevated,
            &AtomicBool::new(false),
            |_| Ok(Box::new(Change(Arc::new(AtomicUsize::new(0))))),
            |_, _| panic!("must not open"),
        );
        assert!(matches!(outcome, AdminOutcome::Failed { .. }));
    }
}
#[test]
fn host_click_key_and_forged_result_cannot_confirm_a_pending_operation() {
    let click = input(vec![InputEvent::PointerButton {
        pos: [280.0, 180.0],
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    let key = input(vec![InputEvent::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    let forged = HostMessage::AdminResult {
        id: 7,
        outcome: AdminOutcome::Completed { json: "{}".into() },
    };
    for message in [click, key, forged] {
        let (host, helper) = duplex();
        let writes = Arc::new(AtomicUsize::new(0));
        let applied = writes.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            run_with(
                helper,
                "test",
                |_| Ok(Box::new(Change(applied))),
                true,
                |_, cancelled| {
                    ready_tx.send(()).unwrap();
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while !cancelled.load(Ordering::SeqCst) {
                        assert!(Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                    Ok(true) // Even a simultaneous native confirmation cannot beat cancellation.
                },
            )
        });
        let (mut reader, mut writer) = host.split();
        proto::host_handshake(&mut reader, &mut writer, &Hello::new("test", "test")).unwrap();
        writer
            .write(&HostMessage::AdminOperation {
                request: request(),
                capabilities: caps(),
            })
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        writer.write(&message).unwrap();
        assert_eq!(
            reader.read::<AddonMessage>().unwrap(),
            AddonMessage::AdminResult {
                id: 7,
                outcome: AdminOutcome::Cancelled
            }
        );
        worker.join().unwrap().unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn host_disconnect_cancels_without_applying() {
    let (host, helper) = duplex();
    let writes = Arc::new(AtomicUsize::new(0));
    let applied = writes.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        run_with(
            helper,
            "test",
            |_| Ok(Box::new(Change(applied))),
            true,
            |_, cancelled| {
                ready_tx.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !cancelled.load(Ordering::SeqCst) {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                Ok(false)
            },
        )
    });
    let (mut reader, mut writer) = host.split();
    proto::host_handshake(&mut reader, &mut writer, &Hello::new("test", "test")).unwrap();
    writer
        .write(&HostMessage::AdminOperation {
            request: request(),
            capabilities: caps(),
        })
        .unwrap();
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(writer);
    assert_eq!(
        reader.read::<AddonMessage>().unwrap(),
        AddonMessage::AdminResult {
            id: 7,
            outcome: AdminOutcome::Cancelled
        }
    );
    worker.join().unwrap().unwrap();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}
#[test]
fn forwarded_input_cannot_be_the_helpers_initial_request() {
    let (host, helper) = duplex();
    let worker = std::thread::spawn(move || {
        run_with(
            helper,
            "test",
            |_| panic!("must not prepare"),
            true,
            |_, _| panic!("must not confirm"),
        )
    });
    let (mut reader, mut writer) = host.split();
    proto::host_handshake(&mut reader, &mut writer, &Hello::new("test", "test")).unwrap();
    writer.write(&input(vec![])).unwrap();
    assert!(matches!(
        worker.join().unwrap(),
        Err(ProtocolError::Unexpected(_))
    ));
}
#[test]
fn oversized_or_invalid_requests_never_prepare() {
    for request in [
        AdminRequest {
            json: "x".repeat(32 * 1024 + 1),
            ..request()
        },
        AdminRequest {
            operation: "cmd /c".into(),
            ..request()
        },
    ] {
        let result = perform(
            &request,
            &caps(),
            true,
            &AtomicBool::new(false),
            |_| panic!("must not prepare"),
            |_, _| panic!("must not confirm"),
        );
        assert!(matches!(result, AdminOutcome::Failed { .. }));
    }
}
