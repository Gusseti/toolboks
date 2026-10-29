#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod app;
mod tools;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(concat!("toolboks ", env!("CARGO_PKG_VERSION")))
            .with_inner_size([720.0, 480.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "toolboks",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
