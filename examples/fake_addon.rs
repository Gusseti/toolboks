//! A fake add-on exe for the supervisor tests (#80). It is built with the tests and never
//! shipped. The tests copy it to a folder of their own as `addon.exe`; a `mode.txt` beside the
//! copy picks what it does:
//!
//! - no file or `normal`: a real add-on on the SDK runtime;
//! - `crash`: the same, but the process ends with exit code 3 shortly after it started;
//! - `child`: the same, and it starts a child process that sleeps, to show that the host's job
//!   ends children too (the child's process id goes to `child.pid`);
//! - `services`: a real add-on that uses every host service (status bar items, tray entries,
//!   a tour with an anchor, preferences), for the host services tests;
//! - `findings`: a real add-on (protocol 5) with background work that reports health findings:
//!   one changes on every background tick, one counts `CheckNow`;
//! - `wrong-token`: opens the pipes and presents a token that is not the one it was given;
//! - `silent`: opens nothing and waits.
//!
//! `--protocol 3` after the mode (in `mode.txt` or on the command line) makes it a protocol 3
//! add-on as released before protocol 4: it accepts only a host `Hello` at 3 and refuses any
//! other in plain words, answers every input with a frame and sends a status item. A `CheckNow`
//! is written to `checknow.txt` and ends it, as it would end a real protocol 3 add-on (it cannot
//! read that message). With `findings`, it also sends a `Findings` message it should not send
//! at protocol 3, to show the host ignores it.
//!
//! Every start appends a line to `starts.txt`, so a test can count launches, and every `Suspend`
//! a line to `suspends.txt`.

use eframe::egui;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use toolboks_addon_proto::{AddonMessage, FrameReader, FrameWriter, Hello, HostMessage};
use toolboks_addon_sdk::runtime::run_addon;
use toolboks_addon_sdk::{
    tutorial, Finding, NativeAddon, Severity, StatusItem, TrayItem, TutorialStep,
};

pub const ID: &str = "test.fake-addon";

struct Fake;

impl NativeAddon for Fake {
    fn suspend(&mut self) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder().join("suspends.txt"))
        {
            let _ = writeln!(file, "suspend");
        }
    }

    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        ui.label("Fake add-on");
        false
    }
}

/// Uses every host service. `count` is the one preference; the tray entry "bump" raises it,
/// and so does the button, which a tour must never reach.
#[derive(Default)]
struct Services {
    count: i64,
}

impl NativeAddon for Services {
    fn load_preferences(&mut self, preferences: &BTreeMap<String, serde_json::Value>) {
        self.count = preferences
            .get("count")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
    }

    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        ui.label("Services add-on");
        tutorial::region(ui, "fake-anchor", |ui| {
            if ui.button("Add a hundred").clicked() {
                self.count += 100;
            }
        });
        false
    }

    fn save_preferences(&mut self, preferences: &mut BTreeMap<String, serde_json::Value>) -> bool {
        let count = serde_json::Value::from(self.count);
        if preferences.get("count") == Some(&count) {
            return false;
        }
        preferences.insert("count".into(), count);
        true
    }

    fn status_items(&self) -> Vec<StatusItem> {
        vec![StatusItem {
            text: format!("Fake {}", self.count),
            tooltip: None,
        }]
    }

    fn tray_items(&self) -> Vec<TrayItem> {
        let item = |id: &str, label: &str, enabled| TrayItem {
            id: id.into(),
            label: label.into(),
            enabled,
        };
        vec![item("bump", "Add one", true), item("off", "Not now", false)]
    }

    fn tray_action(&mut self, id: &str) {
        if id == "bump" {
            self.count += 1;
        }
    }

    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        vec![TutorialStep::new(
            "main",
            "fake-anchor",
            "The button",
            "This button adds a hundred.",
        )]
    }

    fn tutorial_location(&self) -> String {
        "main".into()
    }
}

/// Reports health findings from background ticks and on `CheckNow`.
#[derive(Default)]
struct Findings {
    ticks: u32,
    checks: u32,
}

impl NativeAddon for Findings {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut BTreeMap<String, serde_json::Value>) -> bool {
        ui.label("Findings add-on");
        false
    }

    fn has_background_work(&self) -> bool {
        true
    }

    fn suspend(&mut self) {
        append(&folder().join("suspends.txt"), "suspend");
    }

    fn tick_background(&mut self) {
        self.ticks += 1;
    }

    fn check_now(&mut self) {
        self.checks += 1;
        append(&folder().join("checknow.txt"), "check");
    }

    fn findings(&self) -> Vec<Finding> {
        vec![
            Finding::ok("ticks", format!("{} ticks", self.ticks), "Counted").measured_at(1),
            Finding::attention(
                "checks",
                format!("Checked {} times", self.checks),
                "Counted",
                Severity::Watch,
            )
            .measured_at(1),
        ]
    }
}

fn append(path: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// A protocol 3 add-on as released before protocol 4, without the SDK runtime: only `Hello` 3,
/// a frame for every input, one status item, pong for every ping.
fn protocol_3(send_findings: bool) {
    let folder = folder();
    let input = std::fs::File::open(argument("--pipe-in").expect("--pipe-in")).expect("pipe-in");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .open(argument("--pipe-out").expect("--pipe-out"))
        .expect("pipe-out");
    output
        .write_all(argument("--token").expect("--token").as_bytes())
        .expect("token");
    let mut reader = FrameReader::new(input);
    let mut writer = FrameWriter::new(output);
    // Its newest and only version is 3: a host at 4 is refused in plain words.
    if toolboks_addon_proto::addon_handshake(
        &mut reader,
        &mut writer,
        &Hello::with_version(ID, "0.3.0", 3),
    )
    .is_err()
    {
        std::process::exit(1);
    }
    if send_findings {
        // Not allowed at protocol 3. Sent before the status item, so a host that shows the
        // status item has already dealt with this.
        let finding = Finding::attention("rogue", "Rogue", "Not at 3", Severity::Problem);
        let _ = writer.write(&AddonMessage::Findings(vec![(&finding).into()]));
    }
    let _ = writer.write(&AddonMessage::StatusItems(vec![
        toolboks_addon_proto::StatusItem {
            text: "Fake v3".into(),
            tooltip: None,
        },
    ]));
    loop {
        let sent = match reader.read::<HostMessage>() {
            Ok(HostMessage::Input(batch)) => {
                writer.write(&AddonMessage::Frame(toolboks_addon_proto::Frame {
                    seq: batch.seq,
                    ..Default::default()
                }))
            }
            Ok(HostMessage::Ping { nonce }) => writer.write(&AddonMessage::Pong { nonce }),
            Ok(HostMessage::Suspend) => {
                append(&folder.join("suspends.txt"), "suspend");
                Ok(())
            }
            Ok(HostMessage::CheckNow) => {
                // A real protocol 3 add-on cannot read this message and ends with an error.
                append(&folder.join("checknow.txt"), "check");
                std::process::exit(7);
            }
            Ok(HostMessage::Quit) | Err(_) => return,
            Ok(_) => Ok(()),
        };
        if sent.is_err() {
            return;
        }
    }
}

fn folder() -> PathBuf {
    let exe = std::env::current_exe().expect("own path");
    exe.parent().map(Path::to_path_buf).expect("own folder")
}

fn argument(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == name {
            return args.next();
        }
    }
    None
}

struct AdminRequester {
    sent: bool,
}
impl NativeAddon for AdminRequester {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut BTreeMap<String, serde_json::Value>) -> bool {
        ui.label("Administrator request test");
        false
    }
    fn take_host_request(&mut self) -> Option<toolboks_addon_sdk::HostRequest> {
        if self.sent {
            return None;
        }
        self.sent = true;
        Some(toolboks_addon_sdk::HostRequest::AdminOperation(
            toolboks_addon_sdk::AdminRequest {
                id: 42,
                operation: "test_action".into(),
                json: "{}".into(),
            },
        ))
    }
    fn admin_result(&mut self, id: u64, outcome: toolboks_addon_sdk::AdminOutcome) {
        let path = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("admin-result.json");
        std::fs::write(path, serde_json::to_vec(&(id, outcome)).unwrap()).unwrap();
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--sleep") {
        std::thread::sleep(Duration::from_secs(120));
        return;
    }
    let folder = folder();
    if let Ok(mut starts) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("starts.txt"))
    {
        let _ = writeln!(starts, "{}", std::process::id());
    }
    let text = std::fs::read_to_string(folder.join("mode.txt")).unwrap_or_default();
    let mut words: Vec<&str> = text.split_whitespace().collect();
    let protocol = match words.iter().position(|word| *word == "--protocol") {
        Some(at) => {
            let version = words.get(at + 1).and_then(|v| v.parse::<u32>().ok());
            words.drain(at..(at + 2).min(words.len()));
            version
        }
        None => argument("--protocol").and_then(|v| v.parse().ok()),
    };
    let mode = words.first().copied().unwrap_or("normal");
    if protocol == Some(3) {
        protocol_3(mode == "findings");
        return;
    }
    match mode {
        "silent" => std::thread::sleep(Duration::from_secs(120)),
        "drop-pipe" => {
            // Connects, closes its end of both pipes, and ends 3 s later: the host sees the pipe
            // close well before the process ends.
            {
                let input = std::fs::File::open(argument("--pipe-in").expect("--pipe-in"))
                    .expect("pipe-in");
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .open(argument("--pipe-out").expect("--pipe-out"))
                    .expect("pipe-out");
                output
                    .write_all(argument("--token").expect("--token").as_bytes())
                    .expect("token");
                let mut reader = toolboks_addon_proto::FrameReader::new(input);
                let mut writer = toolboks_addon_proto::FrameWriter::new(output);
                toolboks_addon_proto::addon_handshake(
                    &mut reader,
                    &mut writer,
                    &toolboks_addon_proto::Hello::new(ID, "0.0.0"),
                )
                .expect("handshake");
            }
            std::thread::sleep(Duration::from_secs(3));
            std::process::exit(3);
        }
        "wrong-token" => {
            let (input, output) = (argument("--pipe-in"), argument("--pipe-out"));
            let _reader = std::fs::File::open(input.expect("--pipe-in"));
            let mut writer = std::fs::OpenOptions::new()
                .write(true)
                .open(output.expect("--pipe-out"))
                .expect("pipe-out");
            let token = argument("--token").expect("--token");
            let _ = writer.write_all("0".repeat(token.len()).as_bytes());
            std::thread::sleep(Duration::from_secs(120));
        }
        mode => {
            if mode == "crash" {
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(300));
                    std::process::exit(3);
                });
            }
            if mode == "child" {
                // Never waited for on purpose: the test checks that the host's job ends it.
                #[allow(clippy::zombie_processes)]
                let child = std::process::Command::new(std::env::current_exe().expect("own path"))
                    .arg("--sleep")
                    .spawn()
                    .expect("child");
                std::fs::write(folder.join("child.pid"), child.id().to_string())
                    .expect("child.pid");
            }
            if mode == "admin-request" {
                let _ = run_addon(AdminRequester { sent: false }, ID);
            } else if mode == "services" {
                let _ = run_addon(Services::default(), ID);
            } else if mode == "findings" {
                let _ = run_addon(Findings::default(), ID);
            } else {
                let _ = run_addon(Fake, ID);
            }
        }
    }
}
