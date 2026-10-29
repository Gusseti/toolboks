use eframe::egui;

mod memory;
mod resources;

/// A utility shown in the sidebar. Add a module, implement this, and register it in [`all`].
pub trait Tool {
    fn name(&self) -> &'static str;

    /// Sidebar pages of this tool. Empty means a single page named after the tool.
    fn pages(&self) -> &'static [&'static str] {
        &[]
    }

    /// Draws page number `page` (always 0 for a tool without `pages`).
    fn ui(&mut self, ui: &mut egui::Ui, page: usize);

    /// Called every frame for every tool, selected or not, and also while the window is hidden
    /// (minimized). For background work such as auto-clean; keep it cheap.
    fn tick(&mut self, _ctx: &egui::Context) {}
}

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![Box::new(memory::Memory::new())]
}
