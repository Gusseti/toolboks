//! A protocol-4 sample Toolboks add-on that is its own program. Toolboks starts it, it draws its page with
//! egui, and the SDK runtime sends the drawing to the Toolboks window and brings input back.
//!
//! Build and try it (see `docs/ADDON-AUTHOR-GUIDE.md`):
//!
//! ```text
//! cargo build -p hello-addon
//! cargo run --example addon_host -- target\debug\hello-addon.exe example.hello
//! ```

// No console window in a release build. The add-on never prints to stdout: use `runtime::log`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use toolboks_addon_sdk::runtime::{log, proto::LogLevel, run_addon_with_admin};
use toolboks_addon_sdk::{tutorial, Finding, NativeAddon, Severity, StatusItem, TutorialStep};

/// The id the host knows this add-on by. It must match the catalog entry.
pub const ID: &str = "example.hello";
/// The one saved preference, namespaced by the host under the add-on's id.
const COUNT: &str = "count";

#[derive(Default)]
struct Hello {
    count: i64,
    styled: bool,
    /// The last check, made when the count changes. Kept, not rebuilt in `findings`: a
    /// finding built there would carry a new time every second and be sent again each time.
    finding: Option<Finding>,
}

/// The sample's one health check, a pure function so it is tested without a host. A real
/// add-on checks something it measured (memory, a drive, the event log).
pub fn check(count: i64) -> Finding {
    if count >= 10 {
        Finding::attention(
            "hello-count",
            "The count is high",
            format!("Add one was pressed {count} times."),
            Severity::Watch,
        )
        .open_page("Open Hello", "main")
    } else {
        Finding::ok(
            "hello-count",
            "The count is low",
            format!("Add one was pressed {count} times."),
        )
    }
    .stale_after(3600)
}

impl NativeAddon for Hello {
    /// Called once after the host connected, before the first frame.
    fn load_preferences(&mut self, preferences: &BTreeMap<String, serde_json::Value>) {
        self.count = preferences
            .get(COUNT)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        self.finding = Some(check(self.count));
    }

    /// Draws the page. Return true when a saved preference changed.
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        if !self.styled {
            // The host's fonts, colours and spacing (DESIGN.md), applied to this process once.
            toolboks_shared::ui::apply_style(ui.ctx());
            self.styled = true;
        }
        let mut changed = false;
        ui.heading("Hello from an add-on");
        ui.label("This page is drawn by a separate program and shown inside Toolboks.");
        ui.add_space(8.0);
        // `region` marks this part for the guided tour (see `tutorial_steps`).
        tutorial::region(ui, "hello-counter", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add one").clicked() {
                    self.count += 1;
                    self.finding = Some(check(self.count));
                    preferences.insert(COUNT.into(), self.count.into());
                    changed = true;
                    log(LogLevel::Info, format!("count is now {}", self.count));
                }
                ui.label(format!("Count: {}", self.count));
            });
        });
        changed
    }

    /// Shown in the status bar at the bottom of the Toolboks window while the add-on runs.
    fn status_items(&self) -> Vec<StatusItem> {
        vec![StatusItem {
            text: format!("Hello {}", self.count),
            tooltip: Some("How often Add one was pressed".into()),
        }]
    }

    /// Health findings for Home (protocol 4). Sent when they change, never to an older host.
    fn findings(&self) -> Vec<Finding> {
        self.finding.iter().cloned().collect()
    }

    /// The guided tour. The host draws the overlay and its buttons; a step only navigates.
    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        vec![TutorialStep::new(
            "main",
            "hello-counter",
            "The counter",
            "Add one raises the count. The count is saved and shown in the status bar.",
        )]
    }

    fn tutorial_location(&self) -> String {
        "main".into()
    }
}

fn main() {
    // Runs until the host says quit or closes the connection.
    if let Err(error) = run_addon_with_admin(Hello::default, ID, |_| {
        Err("The hello sample has no administrator actions.".into())
    }) {
        eprintln!("hello-addon stopped: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolboks_addon_sdk::{FindingAction, FindingStatus};

    #[test]
    fn the_check_says_what_was_counted() {
        let low = check(3);
        assert_eq!(low.status, FindingStatus::Ok);
        assert_eq!(low.observation, "Add one was pressed 3 times.");
        assert_eq!(low.action, None);
        let high = check(10);
        assert_eq!(high.status, FindingStatus::Attention);
        assert_eq!(high.severity, Severity::Watch);
        assert!(
            matches!(high.action, Some(FindingAction::OpenPage { ref route, .. }) if route == "main")
        );
        assert!(Finding::valid_id(&high.id));
    }
}
