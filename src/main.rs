#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
// Shared with add-ons through `toolboks-shared`; re-exported so `crate::<module>` paths keep working.
use toolboks_shared::{format, ui, win};

mod addons;
mod app;
mod crash;
mod elevation;
mod instance;
mod native;
mod tools;
mod tray;
mod update;
mod winhttp;

fn main() -> eframe::Result {
    crash::install();
    // One window per user: a plain second launch shows the first one and exits. Held until exit.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(_instance) = instance::acquire(&args) else {
        return Ok(());
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!(
            "{} {}",
            instance::APP_NAME,
            env!("CARGO_PKG_VERSION")
        ))
        .with_inner_size([1540.0, 960.0])
        .with_min_inner_size([1060.0, 720.0]);
    // The window and taskbar icon. If the embedded image cannot be read, Windows keeps the exe's
    // own icon (see build.rs), so a bad image never stops the app from starting.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        instance::APP_NAME,
        options,
        Box::new(|cc| {
            ui::apply_style(&cc.egui_ctx);
            Ok(Box::new(app::App::new()))
        }),
    )
}
