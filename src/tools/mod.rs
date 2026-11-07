use eframe::egui;

mod debloat;
mod events;
mod memory;
mod resources;

/// A utility shown as a tab. Add a module, implement this, and register it in [`all`].
pub trait Tool {
    fn name(&self) -> &'static str;

    /// Pages of this tool (the second tab strip). Empty means a single page named after the tool.
    fn pages(&self) -> &'static [&'static str] {
        &[]
    }

    /// Draws page number `page` (always 0 for a tool without `pages`).
    fn ui(&mut self, ui: &mut egui::Ui, page: usize);
    fn tutorial_location(&self) -> String {
        String::new()
    }
    fn tutorial_navigate(&mut self, _section: &str) {}

    /// Called every frame for every enabled tool, selected or not, and also while the window is
    /// hidden (minimized). A disabled tool is not ticked at all. For background work such as auto-clean; keep it cheap.
    fn tick(&mut self, _ctx: &egui::Context) {}

    /// Short items for the status bar at the bottom of the window (e.g. RAM 63%). Called every
    /// frame, so keep it cheap; return nothing if there is nothing current to show.
    fn status(&self) -> Vec<String> {
        Vec::new()
    }
}

/// How many tools there are. Their order is fixed: it matches the built-in add-on manifests.
pub const COUNT: usize = 4;

/// A fresh, idle instance of tool number `index` (see [`all`] for the order).
pub fn build(index: usize) -> Box<dyn Tool> {
    match index {
        0 => Box::new(memory::Memory::new()),
        1 => Box::new(resources::Resources::new()),
        2 => Box::new(debloat::Debloater::new()),
        _ => Box::new(events::EventWatch::new()),
    }
}

pub fn all() -> Vec<Box<dyn Tool>> {
    (0..COUNT).map(build).collect()
}
