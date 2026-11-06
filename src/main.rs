#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

mod addons;
mod app;
mod crash;
mod elevation;
mod format;
mod instance;
mod native;
mod tools;
mod tray;
mod ui;
mod update;
mod win;
mod winhttp;

fn main() -> eframe::Result {
    crash::install();
    // One window per user: a plain second launch shows the first one and exits. Held until exit.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(_instance) = instance::acquire(&args) else {
        return Ok(());
    };
    // Removes the previous exe left behind by an update; does nothing otherwise.
    if !cfg!(feature = "preview") {
        update::cleanup_old_exe();
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "{} {}",
                instance::APP_NAME,
                env!("CARGO_PKG_VERSION")
            ))
            .with_inner_size([1540.0, 960.0])
            .with_min_inner_size([1060.0, 720.0]),
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
