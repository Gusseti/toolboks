//! Golden bytes: every message of protocol 3 must encode to exactly the bytes protocol 3 used,
//! so an add-on or a host built for protocol 3 reads them unchanged. New messages are only
//! ever appended at the end of the enums; a change that moves a variant or a field fails here.
//!
//! The expected bytes were captured from the protocol 3 code (before protocol 4 was added).
//! Never update them to make this test pass: a difference means the wire format changed.

use toolboks_addon_proto::egui;
use toolboks_addon_proto::mesh::MeshRef;
use toolboks_addon_proto::*;

fn hex(message: &impl serde::Serialize) -> String {
    let mut bytes = Vec::new();
    FrameWriter::new(&mut bytes).write(message).unwrap();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hello() -> Hello {
    Hello {
        protocol_version: 3,
        addon_id: "a.b".into(),
        software_version: "1.0.0".into(),
    }
}

fn admin_request() -> AdminRequest {
    AdminRequest {
        id: 7,
        operation: "op".into(),
        json: "{}".into(),
    }
}

fn mods() -> egui::Modifiers {
    egui::Modifiers {
        alt: false,
        ctrl: true,
        shift: false,
        mac_cmd: false,
        command: true,
    }
}

fn input_batch() -> InputBatch {
    InputBatch {
        seq: 9,
        size: [400.0, 300.0],
        pixels_per_point: 1.5,
        theme: Theme::Dark,
        window_focused: true,
        events: vec![
            InputEvent::PointerMoved { pos: [1.0, 2.0] },
            InputEvent::PointerButton {
                pos: [3.0, 4.0],
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: mods(),
            },
            InputEvent::PointerLeft,
            InputEvent::DragReleasedOutside {
                pos: [-1.0, 5.0],
                button: egui::PointerButton::Secondary,
                modifiers: mods(),
            },
            InputEvent::Scroll {
                delta: [0.0, -3.0],
                unit: egui::MouseWheelUnit::Line,
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: mods(),
            },
            InputEvent::Zoom(1.25),
            InputEvent::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: mods(),
            },
            InputEvent::Text("x".into()),
            InputEvent::Paste("p".into()),
            InputEvent::Copy,
            InputEvent::Cut,
            InputEvent::ImePreedit("i".into()),
            InputEvent::ImeCommit("c".into()),
            InputEvent::ImeDeleteSurrounding {
                before_chars: 1,
                after_chars: 2,
            },
            InputEvent::FocusGained {
                via_tab: Some(TabDirection::Backward),
            },
            InputEvent::FocusLost,
        ],
    }
}

fn frame() -> Frame {
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 20.0));
    let mut mesh = egui::Mesh::default();
    mesh.vertices.push(egui::epaint::Vertex {
        pos: egui::pos2(1.0, 2.0),
        uv: egui::pos2(0.0, 0.0),
        color: egui::Color32::from_rgb(1, 2, 3),
    });
    mesh.indices.push(0);
    Frame {
        seq: 9,
        meshes: vec![
            MeshRef::New {
                id: 5,
                clip: rect,
                mesh,
            },
            MeshRef::Cached { id: 6 },
        ],
        textures_set: vec![TextureUpdate {
            id: egui::TextureId::Managed(1),
            delta: egui::epaint::ImageDelta::full(
                egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
                egui::TextureOptions::LINEAR,
            ),
        }],
        textures_free: vec![egui::TextureId::User(2)],
        repaint_after_ms: Some(16),
        cursor: egui::CursorIcon::PointingHand,
        copied_text: Some("t".into()),
        open_urls: vec!["https://x".into()],
        ime: Some(ImeOutput {
            rect,
            cursor_rect: rect,
        }),
        wants_keyboard: true,
        focus_escape: Some(TabDirection::Forward),
        page_index: 2,
    }
}

/// One of every protocol 3 host message, in variant order.
fn host_messages() -> Vec<(&'static str, HostMessage)> {
    vec![
        ("Hello", HostMessage::Hello(hello())),
        (
            "Refuse",
            HostMessage::Refuse {
                protocol_version: 3,
                reason: "no".into(),
            },
        ),
        ("Input", HostMessage::Input(input_batch())),
        ("Navigate", HostMessage::Navigate { page: "p".into() }),
        ("Suspend", HostMessage::Suspend),
        ("Resume", HostMessage::Resume),
        (
            "LoadPreferences",
            HostMessage::LoadPreferences { json: "{}".into() },
        ),
        ("SavePreferences", HostMessage::SavePreferences),
        ("Capabilities", HostMessage::Capabilities(vec!["c".into()])),
        ("TutorialQuery", HostMessage::TutorialQuery),
        ("TutorialStart", HostMessage::TutorialStart),
        (
            "TutorialNavigate",
            HostMessage::TutorialNavigate { page: "p".into() },
        ),
        ("TutorialFinish", HostMessage::TutorialFinish),
        ("Ping", HostMessage::Ping { nonce: 300 }),
        ("Quit", HostMessage::Quit),
        ("TrayAction", HostMessage::TrayAction { id: "t".into() }),
        (
            "TutorialTarget",
            HostMessage::TutorialTarget {
                target: Some("a".into()),
                reveal: true,
            },
        ),
        (
            "AdminOperation",
            HostMessage::AdminOperation {
                request: admin_request(),
                capabilities: vec!["administrator".into()],
            },
        ),
        (
            "AdminResult",
            HostMessage::AdminResult {
                id: 7,
                outcome: AdminOutcome::Completed { json: "{}".into() },
            },
        ),
    ]
}

/// One of every protocol 3 add-on message, in variant order.
fn addon_messages() -> Vec<(&'static str, AddonMessage)> {
    vec![
        ("Hello", AddonMessage::Hello(hello())),
        (
            "Refuse",
            AddonMessage::Refuse {
                protocol_version: 3,
                reason: "no".into(),
            },
        ),
        ("Frame", AddonMessage::Frame(frame())),
        (
            "RequestRepaint",
            AddonMessage::RequestRepaint { after_ms: 250 },
        ),
        (
            "Preferences",
            AddonMessage::Preferences {
                json: "{}".into(),
                changed: true,
            },
        ),
        (
            "HostRequest",
            AddonMessage::HostRequest(HostRequest::AdminOperation(admin_request())),
        ),
        (
            "HostRequest.RestartElevated",
            AddonMessage::HostRequest(HostRequest::RestartElevated { page: "p".into() }),
        ),
        (
            "HostRequest.ReviewUpdate",
            AddonMessage::HostRequest(HostRequest::ReviewUpdate),
        ),
        (
            "StatusItems",
            AddonMessage::StatusItems(vec![StatusItem {
                text: "s".into(),
                tooltip: Some("t".into()),
            }]),
        ),
        (
            "Tutorial",
            AddonMessage::Tutorial {
                steps: vec![TutorialStep {
                    page: "p".into(),
                    target: "t".into(),
                    title: "T".into(),
                    description: "d".into(),
                }],
                location: "l".into(),
            },
        ),
        (
            "Log",
            AddonMessage::Log {
                level: LogLevel::Warn,
                line: "w".into(),
            },
        ),
        ("Pong", AddonMessage::Pong { nonce: 300 }),
        ("Bye", AddonMessage::Bye),
        (
            "TrayItems",
            AddonMessage::TrayItems(vec![TrayItem {
                id: "i".into(),
                label: "L".into(),
                enabled: false,
            }]),
        ),
        (
            "TutorialAnchor",
            AddonMessage::TutorialAnchor {
                target: "a".into(),
                rect: Some(egui::Rect::from_min_max(
                    egui::pos2(1.0, 2.0),
                    egui::pos2(3.0, 4.0),
                )),
            },
        ),
        (
            "AdminResult",
            AddonMessage::AdminResult {
                id: 7,
                outcome: AdminOutcome::Failed { reason: "r".into() },
            },
        ),
        (
            "AdminResult.Cancelled",
            AddonMessage::AdminResult {
                id: 8,
                outcome: AdminOutcome::Cancelled,
            },
        ),
    ]
}

const HOST_V3: &[(&str, &str)] = &[
    ("Hello", "0c000000000303612e6205312e302e30"),
    ("Refuse", "050000000103026e6f"),
    ("Input", "7000000002090000c843000096430000c03f010110000000803f00000040010000404000008040000100010000010203000080bf0000a0400100010000010400000000000040c00101010001000001050000a03f0605010501000001000001070178080170090a0b01690c01630d01020e01010f"),
    ("Navigate", "03000000030170"),
    ("Suspend", "0100000004"),
    ("Resume", "0100000005"),
    ("LoadPreferences", "0400000006027b7d"),
    ("SavePreferences", "0100000007"),
    ("Capabilities", "0400000008010163"),
    ("TutorialQuery", "0100000009"),
    ("TutorialStart", "010000000a"),
    ("TutorialNavigate", "030000000b0170"),
    ("TutorialFinish", "010000000c"),
    ("Ping", "030000000dac02"),
    ("Quit", "010000000e"),
    ("TrayAction", "030000000f0174"),
    ("TutorialTarget", "050000001001016101"),
    ("AdminOperation", "170000001107026f70027b7d010d61646d696e6973747261746f72"),
    ("AdminResult", "06000000120700027b7d"),
];

const ADDON_V3: &[(&str, &str)] = &[
    ("Hello", "0c000000000303612e6205312e302e30"),
    ("Refuse", "050000000103026e6f"),
    ("Frame", "8100000002090200050000000000000000000020410000a0410100010000803f000000400000000000000000010203ff000001060100010001010000803f0000803f01ffffffff0101000000010102011004010174010968747470733a2f2f78010000000000000000000020410000a0410000000000000000000020410000a04101010002"),
    ("RequestRepaint", "0300000003fa01"),
    ("Preferences", "0500000004027b7d01"),
    ("HostRequest", "09000000050207026f70027b7d"),
    ("HostRequest.RestartElevated", "0400000005000170"),
    ("HostRequest.ReviewUpdate", "020000000501"),
    ("StatusItems", "0700000006010173010174"),
    ("Tutorial", "0c00000007010170017401540164016c"),
    ("Log", "0400000008010177"),
    ("Pong", "0300000009ac02"),
    ("Bye", "010000000a"),
    ("TrayItems", "070000000b010169014c00"),
    ("TutorialAnchor", "140000000c0161010000803f000000400000404000008040"),
    ("AdminResult", "050000000d07020172"),
    ("AdminResult.Cancelled", "030000000d0801"),
];

fn check<M: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    messages: Vec<(&'static str, M)>,
    golden: &[(&str, &str)],
) {
    assert_eq!(messages.len(), golden.len());
    for ((name, message), (golden_name, bytes)) in messages.into_iter().zip(golden) {
        assert_eq!(name, *golden_name);
        assert_eq!(
            hex(&message),
            *bytes,
            "{name} encodes differently from protocol 3"
        );
        // And protocol 3 bytes still decode to the same message.
        let raw: Vec<u8> = (0..bytes.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
            .collect();
        let decoded: M = FrameReader::new(&raw[..]).read().unwrap();
        assert_eq!(
            decoded, message,
            "{name} decodes differently from protocol 3"
        );
    }
}

#[test]
fn every_protocol_3_host_message_keeps_its_bytes() {
    check(host_messages(), HOST_V3);
}

#[test]
fn every_protocol_3_addon_message_keeps_its_bytes() {
    check(addon_messages(), ADDON_V3);
}

#[test]
fn protocol_4_messages_come_after_every_protocol_3_variant() {
    // Variant indices: 19 host messages and 14 add-on messages existed in protocol 3.
    assert_eq!(hex(&HostMessage::CheckNow), "0100000013");
    let finding = Finding::attention(
        "drive-health-c",
        "Drive C: may fail soon",
        "2 media errors",
        Severity::Problem,
    )
    .measured_at(1_700_000_000)
    .stale_after(86_400)
    .open_page("Open drive health", "Optimization/C:");
    let bytes = hex(&AddonMessage::Findings(vec![finding.clone()]));
    assert_eq!(&bytes[8..10], "0e", "Findings is variant 14: {bytes}");
    let raw: Vec<u8> = (0..bytes.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
        .collect();
    let decoded: AddonMessage = FrameReader::new(&raw[..]).read().unwrap();
    assert_eq!(decoded, AddonMessage::Findings(vec![finding]));
}

/// Prints the current bytes; run with `-- --ignored --nocapture` to see them.
#[test]
#[ignore]
fn print_golden_bytes() {
    for (name, message) in host_messages() {
        println!("(\"{name}\", \"{}\"),", hex(&message));
    }
    println!("---");
    for (name, message) in addon_messages() {
        println!("(\"{name}\", \"{}\"),", hex(&message));
    }
}
