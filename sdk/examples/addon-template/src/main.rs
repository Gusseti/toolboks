//! Your add-on. Toolboks starts this exe and shows its page inside the Toolboks window.
//! Lines marked `EDIT:` are the places to change first; README.md lists them in order.

// No console window in a release build. Never print to stdout: Toolboks talks to the add-on
// over it. Use `toolboks_addon_sdk::runtime::log` for log lines.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use toolboks_addon_sdk::{runtime::run_addon, NativeAddon, StatusItem};
use toolboks_shared::ui;

// EDIT: the id Toolboks knows this add-on by. It must equal "id" and "native_provider" in
// addon-template.tbaddon and ID in tests/protocol.rs.
const ID: &str = "toolboks.example";
// EDIT: the name at the top of the page (usually the manifest's "title").
const TITLE: &str = "Example add-on";
// EDIT: your saved settings. Each is one key in a JSON map that Toolboks keeps for you.
const COUNT: &str = "count";

// EDIT: what your add-on remembers while it runs.
#[derive(Default)]
struct Addon {
    count: u64,
    styled: bool,
}

impl NativeAddon for Addon {
    /// Called once after Toolboks connected, before the first frame: read saved settings here.
    fn load_preferences(&mut self, preferences: &BTreeMap<String, serde_json::Value>) {
        // EDIT: read each saved setting, with a default for the first start.
        self.count = preferences
            .get(COUNT)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
    }

    /// Draws the page, once per frame while it is shown. Return true after you changed
    /// `preferences`, and Toolboks saves them.
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        // Keep this: it gives the page the Toolboks fonts, colours and spacing.
        if !self.styled {
            ui::apply_style(ui.ctx());
            self.styled = true;
        }
        let mut changed = false;
        // EDIT: your page. Draw it only with egui and the toolboks_shared::ui helpers.
        ui.heading(TITLE);
        ui.label(
            egui::RichText::new("Your page goes here. Press the button; the number is saved.")
                .color(ui::MUTED)
                .size(ui::TEXT_CAPTION),
        );
        ui.add_space(ui::GAP);
        ui::card(ui, ui.available_width(), 0.0, |ui| {
            ui.label(format!("Pressed {} times.", self.count));
            if ui.button("Add one").clicked() {
                self.count += 1;
                // EDIT: write a changed setting into the map, then return true.
                preferences.insert(COUNT.into(), self.count.into());
                changed = true;
            }
        });
        changed
    }

    /// Short items for the status bar at the bottom of the Toolboks window.
    fn status_items(&self) -> Vec<StatusItem> {
        // EDIT: your status bar item, or return an empty Vec for none.
        vec![StatusItem {
            text: format!("Pressed {}", self.count),
            tooltip: Some(format!("{TITLE}: times the button was pressed")),
        }]
    }
}

fn main() {
    if let Err(error) = run_addon(Addon::default(), ID) {
        // stderr is fine; stdout is not.
        eprintln!("{TITLE} stopped: {error}");
        std::process::exit(1);
    }
}
