use eframe::egui;

mod memory;

/// A single utility shown in the sidebar. Add a module, implement this, and register it in [`all`].
pub trait Tool {
    fn name(&self) -> &'static str;
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Called every frame for every tool, selected or not, and also while the window is hidden
    /// (minimized). For background work such as auto-clean; keep it cheap.
    fn tick(&mut self, _ctx: &egui::Context) {}
}

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![Box::new(memory::Memory::new())]
}
