//! Not part of a generated project: tests that `toolboks_addon_sdk::testing::check_exe` fails
//! when it should, using this example's exe.

use toolboks_addon_sdk::testing::{cannot_start_here, check_exe};

#[test]
fn check_exe_refuses_an_exe_that_answers_to_another_id() {
    if cannot_start_here() {
        return;
    }
    let exe = env!("CARGO_BIN_EXE_addon-template");
    let error = check_exe(exe.as_ref(), "toolboks.someone-else").unwrap_err();
    assert!(error.contains("did not connect"), "{error}");
}
