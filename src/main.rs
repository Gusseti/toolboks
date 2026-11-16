#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
// Shared with add-ons through `toolboks-shared`; re-exported so `crate::<module>` paths keep working.
use toolboks_shared::{ui, win};

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
// Home draws the health view from #182 (H3) on; until then only the tests read it.
#[allow(dead_code)]
mod health;
mod installer;
mod instance;
mod marketplace;
mod own_usage;
mod supervisor;
mod tray;
mod update;
mod winhttp;

fn main() -> eframe::Result {
    crash::install();
    // One window per user: a plain second launch shows the first one and exits. Held until exit.
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The isolated preview checks use their own data folder, so they must not hand over to (or
    // wait for) a Toolboks Preview window that is already open.
    #[cfg(feature = "preview")]
    let isolated_check = args.iter().any(|a| {
        [
            "--addon-check",
            "--disk-manager-check",
            "--health-check",
            "--marketplace-check",
            "--restore-check",
        ]
        .contains(&a.as_str())
    });
    #[cfg(not(feature = "preview"))]
    let isolated_check = false;
    let _instance = if isolated_check {
        None
    } else {
        let Some(guard) = instance::acquire(&args) else {
            return Ok(());
        };
        Some(guard)
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
        } else if let Some(scale) = health_check_scale(&args) {
            // 1060 x 720 points (or the size asked for) at the check's scale (the zoom factor is
            // set to match below).
            let [width, height] = health_check_size(&args);
            [width * scale, height * scale]
        } else {
            [1540.0, 960.0]
        })
        .with_min_inner_size(app::MIN_INNER_SIZE);
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
            if let Some(index) = args.iter().position(|arg| arg == "--restore-check") {
                let output =
                    std::path::PathBuf::from(args.get(index + 1).expect("screenshot folder"));
                let expected = args
                    .get(index + 2)
                    .expect("comma separated ids that should be offered")
                    .split(',')
                    .map(str::to_owned)
                    .collect();
                // No test catalog is needed: without one the check uses the real public list.
                assert!(std::env::var_os("TOOLBOKS_DATA_DIR").is_some());
                std::fs::create_dir_all(&output).expect("create screenshot folder");
                return Ok(Box::new(app::restore_check::Check::new(output, expected)));
            }
            #[cfg(feature = "preview")]
            if let Some(index) = args.iter().position(|arg| arg == "--addon-check") {
                let id = args.get(index + 1).expect("add-on id").clone();
                let output =
                    std::path::PathBuf::from(args.get(index + 2).expect("screenshot folder"));
                assert!(std::env::var_os("TOOLBOKS_TEST_CATALOG").is_some());
                assert!(std::env::var_os("TOOLBOKS_DATA_DIR").is_some());
                // A fresh folder by default. `TOOLBOKS_CHECK_REUSE=1` keeps an earlier run's
                // install and data, for checks that continue where the last one stopped.
                assert!(
                    std::env::var_os("TOOLBOKS_CHECK_REUSE").is_some()
                        || !addons::data_root().exists(),
                    "Use a fresh isolated data folder"
                );
                std::fs::create_dir_all(&output).expect("create screenshot folder");
                return Ok(Box::new(app::addon_check::Check::new(output, id)));
            }
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
            if let Some(index) = args.iter().position(|arg| arg == "--health-check") {
                let output =
                    std::path::PathBuf::from(args.get(index + 1).expect("screenshot folder"));
                assert!(std::env::var_os("TOOLBOKS_DATA_DIR").is_some());
                assert!(
                    !addons::data_root().exists(),
                    "Use a fresh isolated data folder"
                );
                std::fs::create_dir_all(&output).expect("create screenshot folder");
                cc.egui_ctx
                    .set_zoom_factor(health_check_scale(&args).unwrap_or(1.0));
                return Ok(Box::new(app::health_check::Check::new(
                    output,
                    health_check_size(&args),
                )));
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

/// The window size in points of a `--health-check <out> [scale] [width height]` run: 1060 x 720
/// (the supported minimum) unless both are given.
fn health_check_size(args: &[String]) -> [f32; 2] {
    let Some(index) = args.iter().position(|arg| arg == "--health-check") else {
        return [1060.0, 720.0];
    };
    match (args.get(index + 3), args.get(index + 4)) {
        (Some(width), Some(height)) => [
            width.parse().expect("width"),
            height.parse().expect("height"),
        ],
        _ => [1060.0, 720.0],
    }
}

/// The scale of a `--health-check <out> [scale] [width height]` run (preview builds only), else
/// `None`.
fn health_check_scale(args: &[String]) -> Option<f32> {
    if !cfg!(feature = "preview") {
        return None;
    }
    let index = args.iter().position(|arg| arg == "--health-check")?;
    Some(
        args.get(index + 2)
            .map_or(1.0, |value| value.parse().expect("scale")),
    )
}
