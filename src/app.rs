use eframe::egui;

use crate::tools::{self, Tool};

pub struct App {
    tools: Vec<Box<dyn Tool>>,
    selected: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            tools: tools::all(),
            selected: 0,
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for tool in &mut self.tools {
            tool.tick(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(160.0)
            .show(ui, |ui| {
                ui.heading("toolboks");
                ui.separator();
                for (i, tool) in self.tools.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, i, tool.name());
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(tool) = self.tools.get_mut(self.selected) {
                ui.heading(tool.name());
                ui.separator();
                tool.ui(ui);
            }
        });
    }
}
