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
//! - `wrong-token`: opens the pipes and presents a token that is not the one it was given;
//! - `silent`: opens nothing and waits.
//!
//! Every start appends a line to `starts.txt`, so a test can count launches, and every `Suspend`
//! a line to `suspends.txt`.

use eframe::egui;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use toolboks_addon_sdk::runtime::run_addon;
use toolboks_addon_sdk::{tutorial, NativeAddon, StatusItem, TrayItem, TutorialStep};

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
    let mode = std::fs::read_to_string(folder.join("mode.txt")).unwrap_or_default();
    match mode.trim() {
        "silent" => std::thread::sleep(Duration::from_secs(120)),
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
            } else {
                let _ = run_addon(Fake, ID);
            }
        }
    }
}
