//! Checks the add-on as Toolboks would: the manifest belongs to it, and the built exe connects,
//! draws a page, answers a heartbeat and quits. No window opens.

use toolboks_addon_sdk::testing::{check_exe, check_manifest};

// EDIT: only if you changed the id. It must equal ID in src/main.rs and "id" in the manifest.
const ID: &str = "toolboks.example";

#[test]
fn the_manifest_belongs_to_this_add_on() {
    let manifest = include_bytes!("../addon-template.tbaddon");
    check_manifest(manifest, ID, env!("CARGO_PKG_VERSION")).unwrap();
}

#[test]
fn the_add_on_connects_draws_and_quits() {
    // Set ADDON_TEST_EXE to test another build, such as target\release\addon-template.exe.
    let exe = std::env::var("ADDON_TEST_EXE")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_addon-template").into());
    check_exe(exe.as_ref(), ID).unwrap();
}
