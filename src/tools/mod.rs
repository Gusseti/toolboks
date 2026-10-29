use eframe::egui;

mod memory;

/// A single utility shown in the sidebar. Add a module, implement this, and register it in [`all`].
pub trait Tool {
    fn name(&self) -> &'static str;
    fn ui(&mut self, ui: &mut egui::Ui);
}

pub fn all() -> Vec<Box<dyn Tool>> {
    vec![Box::new(memory::Memory::new())]
}
