#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
// Shared with add-ons through `toolboks-shared`; re-exported so `crate::<module>` paths keep working.
use toolboks_shared::{format, ui, win};

mod addon_services;
// The supervisor hands its links to this view. Add-on pages use both from the pilot (#88) on;
// until then only tests and the addon_view_demo example do.
#[allow(dead_code)]
mod addon_view;
mod addons;
mod app;
mod catalog;
mod crash;
mod elevation;
mod installer;
mod instance;
mod marketplace;
mod native;
mod supervisor;
mod tools;
mod tray;
mod update;
mod winhttp;

fn main() -> eframe::Result {
    // Helpers never acquire the app instance, construct its window, or start its tools.
    if toolboks_addon_sdk::admin::is_helper() {
        let args: Vec<_> = std::env::args().collect();
        let Some(provider) = args
            .iter()
            .position(|arg| arg == "--builtin-provider")
            .and_then(|i| args.get(i + 1))
        else {
            return Ok(());
        };
        let mut native = native::NativeHost::default();
        if let Err(error) = toolboks_addon_sdk::admin::run_helper(provider, |request| {
            native.prepare_admin(provider, request)
        }) {
            eprintln!("{error}");
        }
        return Ok(());
    }
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
        .with_inner_size(if args.iter().any(|arg| arg == "--marketplace-check") {
            // The screenshot check runs at the width its reference captures were taken at.
            [1120.0, 760.0]
        } else {
            [1540.0, 960.0]
        })
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
        Box::new(move |cc| {
            ui::apply_style(&cc.egui_ctx);
            app::store::install_fonts(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            #[cfg(feature = "preview")]
            if let Some(index) = args.iter().position(|arg| arg == "--disk-manager-check") {
                let output =
                    std::path::PathBuf::from(args.get(index + 1).expect("screenshot folder"));
                assert!(std::env::var_os("TOOLBOKS_TEST_CATALOG").is_some());
                assert!(std::env::var_os("TOOLBOKS_DATA_DIR").is_some());
                assert!(
                    !addons::data_root().exists(),
                    "Use a fresh isolated data folder"
                );
                std::fs::create_dir_all(&output).expect("create screenshot folder");
                return Ok(Box::new(app::disk_manager_check::Check::new(output)));
            }
            #[cfg(feature = "preview")]
            if let Some(index) = args.iter().position(|arg| arg == "--marketplace-check") {
                let output =
                    std::path::PathBuf::from(args.get(index + 1).expect("screenshot folder"));
                let updates =
                    std::path::PathBuf::from(args.get(index + 2).expect("update catalog folder"));
                assert!(std::env::var_os("TOOLBOKS_TEST_CATALOG").is_some());
                assert!(std::env::var_os("TOOLBOKS_DATA_DIR").is_some());
                assert!(
                    !addons::data_root().exists(),
                    "Use a fresh isolated data folder"
                );
                std::fs::create_dir_all(&output).expect("create screenshot folder");
                let scale = args
                    .get(index + 3)
                    .map_or(1.0, |value| value.parse().expect("scale"));
                cc.egui_ctx.set_zoom_factor(scale);
                return Ok(Box::new(app::marketplace_check::Check::new(
                    output, updates,
                )));
            }
            Ok(Box::new(app::App::new()))
        }),
    )
}
