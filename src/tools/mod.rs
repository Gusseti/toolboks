use eframe::egui;

mod debloat;
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

    /// Called every frame for every tool, selected or not, and also while the window is hidden
    /// (minimized). For background work such as auto-clean; keep it cheap.
    fn tick(&mut self, _ctx: &egui::Context) {}

    /// Short items for the status bar at the bottom of the window (e.g. RAM 63%). Called every
    /// frame, so keep it cheap; return nothing if there is nothing current to show.
    fn status(&self) -> Vec<String> {
        Vec::new()
    }
}

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(memory::Memory::new()),
        Box::new(resources::Resources::new()),
        Box::new(debloat::Debloater::new()),
    ]
}
