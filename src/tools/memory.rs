use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::System;

use super::Tool;

const REFRESH: Duration = Duration::from_secs(1);
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Live RAM usage. Read-only for now; cleaning actions come later.
pub struct Memory {
    /// `None` if the platform refused to provide system info.
    sys: Option<System>,
    last_refresh: Instant,
}

impl Memory {
    pub fn new() -> Self {
        let sys = System::new().ok().map(|mut sys| {
            sys.refresh_memory();
            sys
        });
        Self {
            sys,
            last_refresh: Instant::now(),
        }
    }
}

impl Tool for Memory {
    fn name(&self) -> &'static str {
        "Memory"
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(sys) = self.sys.as_mut() else {
            ui.label("Memory information is not available on this system.");
            return;
        };

        if self.last_refresh.elapsed() >= REFRESH {
            sys.refresh_memory();
            self.last_refresh = Instant::now();
        }
        ui.request_repaint_after(REFRESH);

        let total = sys.total_memory() as f64;
        let used = sys.used_memory() as f64;
        let fraction = if total > 0.0 {
            (used / total) as f32
        } else {
            0.0
        };

        ui.add(egui::ProgressBar::new(fraction).text(format!(
            "{:.1} / {:.1} GiB ({:.0}%)",
            used / GIB,
            total / GIB,
            fraction * 100.0
        )));
    }
}
