//! Starts the built add-on exe the way the test host does (over stdin and stdout) and checks
//! that it answers, draws a frame, reports its status item and exits when asked.

use std::process::{Command, Stdio};
use toolboks_addon_proto::{
    host_handshake, AddonMessage, FrameReader, FrameWriter, Hello, HostMessage, InputBatch, Theme,
};

#[test]
fn the_sample_add_on_draws_and_quits() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-addon"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start hello-addon");
    let mut reader = FrameReader::new(child.stdout.take().unwrap());
    let mut writer = FrameWriter::new(child.stdin.take().unwrap());
    let theirs = host_handshake(
        &mut reader,
        &mut writer,
        &Hello::new("example.hello", "test"),
    )
    .expect("handshake");
    assert_eq!(theirs.addon_id, "example.hello");
    assert_eq!(theirs.protocol_version, 3);

    writer
        .write(&HostMessage::LoadPreferences {
            json: r#"{"count":41}"#.into(),
        })
        .unwrap();
    writer
        .write(&HostMessage::Input(InputBatch {
            seq: 1,
            size: [600.0, 400.0],
            pixels_per_point: 1.0,
            theme: Theme::Dark,
            window_focused: true,
            events: Vec::new(),
        }))
        .unwrap();
    let mut status = None;
    let frame = loop {
        match reader.read::<AddonMessage>().expect("a message") {
            AddonMessage::StatusItems(items) => status = items.into_iter().next(),
            AddonMessage::Frame(frame) => break frame,
            _ => {}
        }
    };
    assert_eq!(frame.seq, 1);
    assert!(!frame.meshes.is_empty(), "the page was drawn");
    assert_eq!(status.expect("a status item").text, "Hello 41");

    writer.write(&HostMessage::Quit).unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn the_sample_refuses_protocol_two_with_a_plain_update_message() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hello-addon"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = FrameReader::new(child.stdout.take().unwrap());
    let mut writer = FrameWriter::new(child.stdin.take().unwrap());
    let mut hello = Hello::new("example.hello", "old-host");
    hello.protocol_version = 2;
    writer.write(&HostMessage::Hello(hello)).unwrap();
    match reader.read::<AddonMessage>().unwrap() {
        AddonMessage::Refuse {
            protocol_version,
            reason,
        } => {
            assert_eq!(protocol_version, 3);
            assert!(reason.contains("Update the add-on or Toolboks."));
        }
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!child.wait().unwrap().success());
}
