//! Native confirmation demonstration. Preview only: no UAC and no system changes.
fn main() {
    assert!(
        std::env::var_os("TOOLBOKS_DATA_DIR").is_some(),
        "Use an isolated TOOLBOKS_DATA_DIR"
    );
    let description = "Optimize drive C:\n\nWindows will tell the SSD which unused blocks it can reclaim.\n\nToolboks cannot undo drive optimization.\n\nPreview only. No drive operation will run.";
    let confirmed = toolboks_addon_sdk::admin::preview_dialog(description).expect("native dialog");
    println!(
        "Preview closed: {}. No system change was made.",
        if confirmed { "Confirm" } else { "Cancel" }
    );
}
