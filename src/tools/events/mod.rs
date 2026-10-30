//! Event Watch: reads the Windows event logs and shows live errors and what happened before a
//! bluescreen, a hard reset or a hardware fault. All Event Log work runs on worker threads.

pub mod analysis;
pub mod bugchecks;
pub mod catalog;
pub mod model;
mod page;
pub mod source;

use eframe::egui;

use super::Tool;

pub struct EventWatch {
    page: page::Page,
}

impl EventWatch {
    pub fn new() -> Self {
        Self {
            page: page::Page::new(),
        }
    }
}

impl Tool for EventWatch {
    fn name(&self) -> &'static str {
        "Event Watch"
    }

    fn pages(&self) -> &'static [&'static str] {
        &["Live", "Crashes", "Summary"]
    }

    fn ui(&mut self, ui: &mut egui::Ui, page: usize) {
        self.page.ui(ui, page);
    }
}

#[cfg(test)]
mod real_log_tests {
    use super::analysis::{detect_crashes, digest, group, ms_to_text, DEFAULT_WINDOW_MS};
    use super::source::{load, WinEvents};

    /// Read-only: runs the whole pipeline on this PC's real System and Application logs.
    #[test]
    #[ignore]
    fn real_pipeline_smoke() {
        let since = {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            now - 30 * 24 * 3600 * 1000
        };
        let (events, statuses) = load(&WinEvents, since);
        let groups = group(&events);
        let crashes = detect_crashes(&events, DEFAULT_WINDOW_MS);
        println!(
            "events={} groups={} crashes={}",
            events.len(),
            groups.len(),
            crashes.len()
        );
        for s in &statuses {
            println!(
                "channel {} readable={} oldest={:?}",
                s.name,
                s.readable,
                s.oldest_ms.map(ms_to_text)
            );
        }
        for c in &crashes {
            println!(
                "crash noticed={} stopped={} kind={:?} dump={:?} before={}",
                ms_to_text(c.noticed_ms),
                ms_to_text(c.stopped_ms),
                c.kind,
                c.dump_path,
                c.before.len()
            );
        }
        for line in digest(&groups, &crashes) {
            println!("digest [{:?}] {}", line.severity, line.text);
        }
        for g in groups.iter().take(8) {
            println!(
                "group x{} {} {} {:?} -> {}",
                g.count, g.provider, g.id, g.subject, g.info.headline
            );
        }
        assert!(!statuses.is_empty());
    }
}
