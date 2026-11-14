//! A fake add-on exe for the supervisor tests (#80). It is built with the tests and never
//! shipped. The tests copy it to a folder of their own as `addon.exe`; a `mode.txt` beside the
//! copy picks what it does:
//!
//! - no file or `normal`: a real add-on on the SDK runtime;
//! - `crash`: the same, but the process ends with exit code 3 shortly after it started;
//! - `child`: the same, and it starts a child process that sleeps, to show that the host's job
//!   ends children too (the child's process id goes to `child.pid`);
//! - `wrong-token`: opens the pipes and presents a token that is not the one it was given;
//! - `silent`: opens nothing and waits.
//!
//! Every start appends a line to `starts.txt`, so a test can count launches.

use eframe::egui;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use toolboks_addon_sdk::runtime::run_addon;
use toolboks_addon_sdk::NativeAddon;

pub const ID: &str = "test.fake-addon";

struct Fake;

impl NativeAddon for Fake {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        ui.label("Fake add-on");
        false
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
                let child = std::process::Command::new(std::env::current_exe().expect("own path"))
                    .arg("--sleep")
                    .spawn()
                    .expect("child");
                std::fs::write(folder.join("child.pid"), child.id().to_string())
                    .expect("child.pid");
            }
            let _ = run_addon(Fake, ID);
        }
    }
}
