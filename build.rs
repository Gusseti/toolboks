//! Puts the Toolboks logo inside toolboks.exe, so Explorer, pinned taskbar buttons and Alt+Tab
//! show it. The running window sets the same logo itself (see `main.rs`).

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app.ico");
    // A missing resource compiler must not stop a build: the exe just keeps the default icon.
    if let Err(err) = res.compile() {
        println!("cargo:warning=could not embed the app icon: {err}");
    }
}
