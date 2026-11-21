//! Checks for an add-on's own tests, so its `tests/protocol.rs` stays a few lines long.
//!
//! [`check_manifest`] reads the `.tbaddon` as Toolboks does and compares it with the add-on.
//! [`check_exe`] starts the built exe the way Toolboks does (over stdin and stdout), connects,
//! asks for one frame, sends a heartbeat and asks it to quit. Neither opens a window.

use crate::Package;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use toolboks_addon_proto::{
    host_handshake, AddonMessage, FrameReader, FrameWriter, Hello, HostMessage, InputBatch, Theme,
};

/// How long the exe may take to connect, draw and answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the exe may take to exit after it was asked to quit.
const QUIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Reads a manifest as Toolboks does and checks that it belongs to the add-on: `id` and
/// `native_provider` equal `id`, and its `version` equals `version` (pass
/// `env!("CARGO_PKG_VERSION")`). The manifest's own `min_toolboks_version` is used as the host
/// version, so every other rule is checked whatever minimum it names.
pub fn check_manifest(manifest: &[u8], id: &str, version: &str) -> Result<Package, String> {
    let package: Package = serde_json::from_slice(manifest)
        .map_err(|e| format!("The manifest is not a valid add-on package: {e}"))?;
    package.validate(&package.manifest.min_toolboks_version)?;
    let m = &package.manifest;
    if m.id != id {
        return Err(format!(
            "The manifest's id is {:?}, the add-on's is {id:?}.",
            m.id
        ));
    }
    if m.native_provider.as_deref() != Some(id) {
        return Err(format!(
            "The manifest's native_provider is {:?}, it must be {id:?}.",
            m.native_provider
        ));
    }
    if m.version != version {
        return Err(format!(
            "The manifest's version is {}, Cargo.toml says {version}.",
            m.version
        ));
    }
    Ok(package)
}

/// True when the exe cannot be started here: this process runs as administrator (GitHub's
/// Windows runners do) and the drawing runtime refuses that by design.
pub fn cannot_start_here() -> bool {
    crate::admin::elevated()
        && std::env::var_os("TOOLBOKS_TEST_ALLOW_ELEVATED").is_none_or(|v| v != "1")
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the exe at `exe` as Toolboks would and checks that it answers to `id` at this SDK's
/// protocol version, draws a frame that is not empty, answers a heartbeat and exits cleanly when
/// asked. The exe is ended if anything fails. Skipped (returns `Ok`) when [`cannot_start_here`].
pub fn check_exe(exe: &Path, id: &str) -> Result<(), String> {
    if cannot_start_here() {
        eprintln!(
            "skipped: this process is elevated and the add-on runtime refuses to run elevated"
        );
        return Ok(());
    }
    let mut child = KillOnDrop(
        Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start {}: {e}", exe.display()))?,
    );
    let (stdout, stdin) = match (child.0.stdout.take(), child.0.stdin.take()) {
        (Some(stdout), Some(stdin)) => (stdout, stdin),
        _ => return Err("The exe's stdin or stdout is missing.".into()),
    };
    let host_hello = Hello::new(id, "test");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let talk = || -> Result<(), String> {
            let mut reader = FrameReader::new(stdout);
            let mut writer = FrameWriter::new(stdin);
            host_handshake(&mut reader, &mut writer, &host_hello)
                .map_err(|e| format!("The add-on did not connect: {e}"))?;
            writer
                .write(&HostMessage::Input(InputBatch {
                    seq: 1,
                    size: [600.0, 400.0],
                    pixels_per_point: 1.0,
                    theme: Theme::Dark,
                    window_focused: true,
                    events: Vec::new(),
                }))
                .map_err(|e| e.to_string())?;
            loop {
                match reader.read::<AddonMessage>() {
                    Ok(AddonMessage::Frame(frame)) if frame.seq == 1 => {
                        if frame.meshes.is_empty() {
                            return Err("The add-on drew an empty frame.".into());
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => return Err(format!("The add-on stopped before drawing: {e}")),
                }
            }
            writer
                .write(&HostMessage::Ping { nonce: 7 })
                .map_err(|e| e.to_string())?;
            loop {
                match reader.read::<AddonMessage>() {
                    Ok(AddonMessage::Pong { nonce: 7 }) => break,
                    Ok(_) => {}
                    Err(e) => return Err(format!("The add-on did not answer a heartbeat: {e}")),
                }
            }
            writer
                .write(&HostMessage::Quit)
                .map_err(|e| e.to_string())?;
            // Read until it closes, so a GUI-subsystem exe has flushed its answers.
            while reader.read::<AddonMessage>().is_ok() {}
            Ok(())
        };
        let _ = tx.send(talk());
    });
    rx.recv_timeout(ANSWER_TIMEOUT)
        .map_err(|_| "The add-on did not answer within 120 seconds.".to_string())??;
    let deadline = Instant::now() + QUIT_TIMEOUT;
    loop {
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("The add-on exited with {status} after Quit."))
            };
        }
        if Instant::now() > deadline {
            return Err("The add-on did not exit within 30 seconds of Quit.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, provider: &str, version: &str) -> Vec<u8> {
        serde_json::json!({
            "schema_version": 1,
            "manifest": {
                "id": id, "title": "Counter", "native_provider": provider,
                "description": "Counts.", "long_description": "Counts.", "version": version,
                "author": "A", "publisher": "A", "category": "Tools", "icon": "custom",
                "accent": "#3ecf8e", "api_version": 2, "min_toolboks_version": "0.30.0",
                "license": "MIT", "capabilities": []
            },
            "pages": []
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn a_matching_manifest_passes_whatever_minimum_it_names() {
        let bytes = manifest("com.me.counter", "com.me.counter", "0.1.0");
        let package = check_manifest(&bytes, "com.me.counter", "0.1.0").unwrap();
        assert_eq!(package.manifest.title, "Counter");
    }

    #[test]
    fn a_different_id_provider_or_version_is_named() {
        let good = "com.me.counter";
        let bytes = manifest("com.me.other", good, "0.1.0");
        assert!(check_manifest(&bytes, good, "0.1.0")
            .unwrap_err()
            .contains("id"));
        let bytes = manifest(good, "com.me.other", "0.1.0");
        assert!(check_manifest(&bytes, good, "0.1.0")
            .unwrap_err()
            .contains("native_provider"));
        let bytes = manifest(good, good, "0.2.0");
        assert!(check_manifest(&bytes, good, "0.1.0")
            .unwrap_err()
            .contains("Cargo.toml"));
    }

    #[test]
    fn an_invalid_manifest_is_refused() {
        assert!(check_manifest(b"{}", "com.me.counter", "0.1.0").is_err());
        let bytes = manifest("Bad Id", "Bad Id", "0.1.0");
        assert!(check_manifest(&bytes, "Bad Id", "0.1.0").is_err());
    }

    #[test]
    fn a_missing_exe_is_an_error() {
        if cannot_start_here() {
            return;
        }
        let error = check_exe(Path::new("no-such-add-on-exe.exe"), "com.me.counter").unwrap_err();
        assert!(error.starts_with("Could not start"), "{error}");
    }
}
