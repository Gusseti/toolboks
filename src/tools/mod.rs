use eframe::egui;

mod debloat;
mod events;
mod memory;
mod resources;

/// A utility shown as a tab. Add a module, implement this, and register it in [`all`].
pub trait Tool {
    /// Draws page number `page` (always 0 for a tool without `pages`).
    fn ui(&mut self, ui: &mut egui::Ui, page: usize);
    fn tutorial_location(&self) -> String {
        String::new()
    }
    fn tutorial_navigate(&mut self, _section: &str) {}

    /// Called while this tool is the active workspace.
    fn tick(&mut self, _ctx: &egui::Context) {}

    /// Release page data. Return true only to preserve lightweight session state.
    fn suspend(&mut self) -> bool {
        false
    }

    fn resume(&mut self) {}

    /// Complete already-started operations for suspended tools without sampling or starting work.
    fn finish_pending(&mut self, _ctx: &egui::Context) {}

    /// Short items for the status bar at the bottom of the window (e.g. RAM 63%). Called every
    /// frame, so keep it cheap; return nothing if there is nothing current to show.
    fn status(&self) -> Vec<String> {
        Vec::new()
    }
}

/// How many tools there are. Their order is fixed: it matches the built-in add-on manifests.
pub const COUNT: usize = 4;
/// Navigation metadata stays available without constructing a tool.
pub const WORKSPACES: [(&str, &[&str]); COUNT] = [
    ("Memory Cleaner", &["Home", "Advanced"]),
    (
        "Resource Monitor",
        &["Overview", "Details", "Processes", "Connections"],
    ),
    (
        "Debloater",
        &["Privacy and ads", "Apps", "Startup", "Tasks", "Services"],
    ),
    ("Event Watch", &["Live", "Crashes", "Summary"]),
];

/// A fresh, idle instance of tool number `index` (see [`all`] for the order).
pub fn build(index: usize) -> Box<dyn Tool> {
    match index {
        0 => Box::new(memory::Memory::new()),
        1 => Box::new(resources::Resources::new()),
        2 => Box::new(debloat::Debloater::new()),
        _ => Box::new(events::EventWatch::new()),
    }
}

#[cfg(test)]
pub fn all() -> Vec<Box<dyn Tool>> {
    (0..COUNT).map(build).collect()
}
