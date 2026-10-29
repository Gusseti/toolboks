#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod app;
mod elevation;
mod tools;
mod update;
mod win;
mod winhttp;

fn main() -> eframe::Result {
    // Removes the previous exe left behind by an update; does nothing otherwise.
    update::cleanup_old_exe();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("toolboks ", env!("CARGO_PKG_VERSION")))
            .with_inner_size([880.0, 640.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "toolboks",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
