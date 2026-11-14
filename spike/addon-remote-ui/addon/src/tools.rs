//! Stub of the app's `tools` module: only the trait, plus the real Resource Monitor included from
//! the app source unchanged.
use eframe::egui;

pub trait Tool {
    fn ui(&mut self, ui: &mut egui::Ui, page: usize);
    fn tutorial_location(&self) -> String {
        String::new()
    }
    fn tutorial_navigate(&mut self, _section: &str) {}
    fn tick(&mut self, _ctx: &egui::Context) {}
    fn suspend(&mut self) -> bool {
        false
    }
    fn resume(&mut self) {}
    fn finish_pending(&mut self, _ctx: &egui::Context) {}
    fn status(&self) -> Vec<String> {
        Vec::new()
    }
}

#[path = "../../../../src/tools/resources/mod.rs"]
pub mod resources;
