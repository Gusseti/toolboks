//! Isolated PREVIEW check of Health on Home (#182, the "Focus" design). It installs four fixture
//! add-on records (no exe; nothing is started) into a fresh data folder, feeds fixture findings
//! into the health store the way `Services` does, and saves a screenshot of each state: the focus
//! card (a Problem with a bar, a Watch with a series, nine OK rows from three add-ons), a focus
//! with "Why it matters" and "What to do", all OK, mixed (not checked, unavailable, out of date),
//! nothing reported, "Checking…", a long list that scrolls, and nothing installed (the "Pick
//! your tools" prompt). The fixtures exist only in this preview check, never in release code.
//!
//! `toolboks --health-check <out> [scale] [width height]`: the window is 1060 x 720 points (or
//! `width` x `height`) at `scale` (1.0 or 1.5), so the screenshots are 1060 x 720 pixels at
//! 100 % and 1590 x 1080 at 150 %.
use super::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use toolboks_addon_proto::{Evidence, Finding, Severity};

const DISK: &str = "fixture.disk-manager";
const MEMORY: &str = "fixture.memory-cleaner";
const EVENTS: &str = "fixture.event-watch";
const MONITOR: &str = "fixture.resource-monitor";

/// `(id, title, icon, pages, checks)` of a fixture add-on.
type Fixture = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const ADDONS: [Fixture; 4] = [
    (
        DISK,
        "Disk Manager",
        "storage",
        &["Optimization", "Map"],
        &["Drive health"],
    ),
    (
        MEMORY,
        "Memory Cleaner",
        "chip",
        &["Home", "Settings"],
        &["Memory pressure"],
    ),
    (
        EVENTS,
        "Event Watch",
        "shield",
        &["Crashes", "Events"],
        &["Recent crashes", "Boot time"],
    ),
    (
        MONITOR,
        "Resource Monitor",
        "activity",
        &["Overview", "Checks"],
        &[],
    ),
];

/// Writes the installed records of the fixture add-ons: an `active.json` and the version's
/// `entry.json`, without an exe, so nothing starts.
fn install_fixtures(root: &std::path::Path) {
    for (id, title, icon, pages, checks) in ADDONS {
        let dir = crate::installer::addon_dir(root, id);
        std::fs::create_dir_all(dir.join("1.0.0")).unwrap();
        let entry = serde_json::json!({
            "id": id,
            "title": title,
            "short_description": "Fixture for the Health check",
            "publisher": "Toolboks",
            "category": "system",
            "icon": icon,
            "accent": "#8B5CF6",
            "kind": "native",
            "pages": pages,
            "checks": checks,
            "version": "1.0.0",
            "min_toolboks_version": "0.12.0",
            "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": "https://github.com/test/addon.exe",
            "size": 1,
            "sha256": "00".repeat(32),
            "verified": true,
            "channel": "stable"
        });
        std::fs::write(
            dir.join("1.0.0").join("entry.json"),
            serde_json::to_vec_pretty(&entry).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("active.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": "1.0.0",
                "previous": null,
                "confirmed": true,
                "launched": true
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(
            crate::installer::installed_entry(root, id).is_some(),
            "fixture {id} reads back"
        );
    }
}

fn pages(id: &str) -> Vec<String> {
    ADDONS
        .iter()
        .find(|addon| addon.0 == id)
        .map(|addon| addon.3.iter().map(|page| page.to_string()).collect())
        .unwrap_or_default()
}

pub struct Check {
    app: App,
    output: PathBuf,
    /// The window size in points the run was started with.
    size: [f32; 2],
    stage: usize,
    frames: usize,
    /// When the current stage began: a capture waits for the frames and for animations (such as
    /// a scroll bar fading out) to settle.
    stage_started: std::time::Instant,
    started: std::time::Instant,
    capture: Option<String>,
    saved: Arc<AtomicBool>,
}

impl Check {
    pub fn new(output: PathBuf, size: [f32; 2]) -> Self {
        install_fixtures(&crate::addons::data_root());
        let mut app = App::new();
        // A fresh data folder reads as an update from the old built-in tools; that offer is not
        // what these captures are about.
        app.registry.config.restore_pending.clear();
        app.screen = Screen::Home;
        // Open sidebar: the content is then 820 points wide at the minimum window.
        app.sidebar_expanded = true;
        Self {
            app,
            output,
            size,
            stage: 0,
            frames: 0,
            stage_started: std::time::Instant::now(),
            started: std::time::Instant::now(),
            capture: None,
            saved: Arc::new(AtomicBool::new(true)),
        }
    }
    fn capture(&mut self, name: &str) {
        self.capture = Some(name.into());
        self.saved.store(false, Ordering::Relaxed);
        self.stage += 1;
        self.frames = 0;
        self.stage_started = std::time::Instant::now();
    }
    /// Replaces the store with `sets` received at `now`, then marks `stopped` as not running.
    fn feed(&mut self, sets: Vec<(&str, Vec<Finding>)>, stopped: &[&str]) {
        let now = crate::addon_services::unix_now();
        self.app.services.health = Default::default();
        self.app.health_check = None;
        for (id, findings) in sets {
            let dropped = self
                .app
                .services
                .health
                .replace(id, &findings, &pages(id), now);
            assert!(dropped.is_empty(), "{id}: {dropped:?}");
        }
        for id in stopped {
            self.app.services.health.set_running(id, false);
        }
    }
    /// The board Home draws now.
    fn board(&self) -> super::health::Board {
        let now = crate::addon_services::unix_now();
        let installed = crate::installer::installed(&self.app.registry.root);
        let titles = |id: &str| {
            installed
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.title.clone())
        };
        let view = self.app.services.health.view(now, &titles);
        let waiting: Vec<_> = installed
            .iter()
            .filter(|entry| {
                !entry.checks.is_empty() && !self.app.services.health.reported(&entry.id)
            })
            .map(|entry| super::health::Waiting {
                id: entry.id.clone(),
                title: entry.title.clone(),
                checks: entry.checks.clone(),
                running: false,
            })
            .collect();
        super::health::board(&view, &waiting, now, self.app.health_check.is_some(), false)
    }
}

fn ago(secs: u64) -> u64 {
    crate::addon_services::unix_now() - secs
}

/// Nine OK rows with short values, three from each of three add-ons.
fn fine_rows(id: &str) -> Vec<Finding> {
    let rows: &[(&str, &str, &str)] = match id {
        MEMORY => &[
            ("memory-in-use", "Memory in use", "41% in use"),
            ("page-file", "Page file", "Sized by Windows"),
            ("compressed", "Compressed memory", "1.2 GB"),
        ],
        DISK => &[
            ("drive-health-c", "Drive health of C:", "Good"),
            ("free-space-d", "Free space on D:", "61% free"),
            ("temperature-c", "Drive temperature", "31 °C"),
        ],
        EVENTS => &[
            ("recent-crashes", "Crashes", "None in 30 days"),
            ("boot-time", "Start-up", "42 s"),
            ("restarts", "Restarts", "None unexpected"),
        ],
        _ => &[],
    };
    rows.iter()
        .map(|(id, title, value)| {
            Finding::ok(*id, *title, *value)
                .measured_at(ago(90))
                .stale_after(86_400)
        })
        .collect()
}

/// The focus card's fixture: a Problem with a bar, a Watch with a series, nine OK rows.
fn attention_sets() -> Vec<(&'static str, Vec<Finding>)> {
    let mut disk = vec![Finding::attention(
        "free-space-c",
        "Drive C: is almost full",
        "Only 6 GB is left of 476 GB.",
        Severity::Problem,
    )
    .measured_at(ago(20))
    .stale_after(86_400)
    .why("Windows slows down and updates can fail when a drive drops below about 10 GB free.")
    .evidence(Evidence::bar(470.0 / 476.0, "470 GB used", "6 GB free"))
    .open_page("Open Disk Manager", "Optimization/C:")];
    disk.extend(fine_rows(DISK));
    let mut memory = vec![Finding::attention(
        "memory-pressure",
        "Memory ran short under load",
        "Available memory fell under 1.1 GB 4 times in the last 10 minutes.",
        Severity::Watch,
    )
    .measured_at(ago(120))
    .stale_after(300)
    .evidence(Evidence::series(
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.7, 1.0, 0.0, 0.0, 0.85],
        "10 min ago",
        "now",
    ))
    .open_page("Open Memory Cleaner", "Home")];
    memory.extend(fine_rows(MEMORY));
    vec![(DISK, disk), (MEMORY, memory), (EVENTS, fine_rows(EVENTS))]
}

/// A Watch focus whose action is an instruction: "Why it matters" and "What to do" side by side.
fn instruction_sets() -> Vec<(&'static str, Vec<Finding>)> {
    vec![
        (
            MONITOR,
            vec![Finding::attention(
                "ram-speed",
                "Memory runs slower than it is rated for",
                "Memory runs at 2400 MT/s; it is rated for 3600 MT/s.",
                Severity::Watch,
            )
            .measured_at(ago(600))
            .stale_after(86_400)
            .why(
                "Games and other heavy apps can lose some speed when memory runs below its rating.",
            )
            .evidence(Evidence::bar(
                2400.0 / 3600.0,
                "2400 MT/s now",
                "3600 MT/s rated",
            ))
            .instruction(
                "Turn on XMP or EXPO in the BIOS setup. Toolboks does not change BIOS settings.",
            )],
        ),
        (MEMORY, fine_rows(MEMORY)),
        (DISK, fine_rows(DISK)),
    ]
}

fn all_ok_sets() -> Vec<(&'static str, Vec<Finding>)> {
    vec![
        (
            MEMORY,
            vec![Finding::ok(
                "memory-pressure",
                "Memory: no pressure in the last 30 min",
                "",
            )
            .measured_at(ago(40))
            .stale_after(300)],
        ),
        (
            DISK,
            vec![
                Finding::ok("drive-health-c", "Drive C: no warnings from the drive", "")
                    .measured_at(ago(300))
                    .stale_after(86_400),
                Finding::ok("drive-health-d", "Drive D: no warnings from the drive", "")
                    .measured_at(ago(300))
                    .stale_after(86_400),
            ],
        ),
        (
            MONITOR,
            vec![Finding::ok("power-plan", "Power plan: Balanced", "")
                .measured_at(ago(600))
                .stale_after(86_400)],
        ),
    ]
}

fn mixed_sets() -> Vec<(&'static str, Vec<Finding>)> {
    vec![
        (
            MEMORY,
            vec![Finding::not_checked(
                "memory-pressure",
                "Memory pressure",
                "The first reading is still running",
            )
            .measured_at(ago(5))],
        ),
        (
            DISK,
            vec![
                Finding::ok("drive-health-c", "Drive C: no warnings from the drive", "")
                    .measured_at(ago(300))
                    .stale_after(86_400),
                Finding::unavailable(
                    "drive-health-d",
                    "Drive health of D:",
                    "This drive does not report health",
                )
                .measured_at(ago(300))
                .stale_after(86_400),
            ],
        ),
        (
            EVENTS,
            vec![
                Finding::ok("boot-time", "Boot time: last start took 38 s", "")
                    .measured_at(ago(3 * 3_600))
                    .stale_after(3_600),
                Finding::ok(
                    "recent-crashes",
                    "No crashes recorded in the last 7 days",
                    "",
                )
                .measured_at(ago(3 * 3_600))
                .stale_after(3_600),
            ],
        ),
        (
            MONITOR,
            vec![Finding::attention(
                "power-plan",
                "Power saver plan is on",
                "Windows is set to save power while the PC is plugged in.",
                Severity::Watch,
            )
            .measured_at(ago(2 * 86_400))
            .stale_after(86_400)
            .instruction("Open Settings > System > Power and choose Balanced.")],
        ),
    ]
}

/// More than three attention rows from one add-on, and enough rows to scroll.
fn long_sets() -> Vec<(&'static str, Vec<Finding>)> {
    let mut sets = attention_sets();
    sets.extend(instruction_sets().into_iter().take(1));
    let disk = &mut sets[0].1;
    for (letter, minutes) in [("e", 7), ("f", 9), ("g", 11)] {
        disk.push(
            Finding::attention(
                format!("drive-health-{letter}"),
                format!("Drive {}: is wearing", letter.to_uppercase()),
                "The drive reports 85% of its rated writes used.",
                Severity::Watch,
            )
            .measured_at(ago(minutes * 60))
            .stale_after(86_400)
            .open_page("Open drive health", format!("Optimization/{letter}:")),
        );
    }
    sets
}

impl eframe::App for Check {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(100));
        assert!(
            self.started.elapsed() < Duration::from_secs(120),
            "health check timed out at stage {}",
            self.stage
        );
        self.frames += 1;
        if self.frames < 12 || self.stage_started.elapsed() < Duration::from_millis(800) {
            return;
        }
        if let Some(name) = self.capture.take() {
            let path = self.output.join(format!("{name}.png"));
            let saved = self.saved.clone();
            let _ = std::fs::create_dir_all(&self.output);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    let pixels: Vec<_> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    image::save_buffer(
                        path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    saved.store(true, Ordering::Relaxed);
                }),
            ));
            self.frames = 0;
            return;
        }
        if !self.saved.load(Ordering::Relaxed) {
            return;
        }
        match self.stage {
            0 => {
                let size = ctx.content_rect().size();
                let [width, height] = self.size;
                assert!(
                    (size.x - width).abs() < 2.0 && (size.y - height).abs() < 2.0,
                    "the window is {size:?} points, not {width} x {height}"
                );
                self.feed(attention_sets(), &[]);
                let board = self.board();
                let focus = board.focus.as_ref().expect("a focus card");
                assert_eq!(focus.title, "Drive C: is almost full");
                assert!(focus.button.as_ref().unwrap().primary);
                assert!(matches!(focus.evidence, Some(Evidence::Bar { .. })));
                assert_eq!(board.others.len(), 1);
                assert!(matches!(
                    board.others[0].evidence,
                    Some(Evidence::Series { .. })
                ));
                assert_eq!(board.fine.len(), 3);
                assert!(board.fine.iter().all(|group| group.facts.len() == 3
                    && group.facts.iter().all(|fact| !fact.value.is_empty())));
                assert_eq!(board.fine_heading, "Everything else is fine");
                assert!(board.quiet.is_empty());
                assert!(board.caption.starts_with("Health, 11 checks, last result"));
                self.capture("01-focus");
            }
            1 => {
                self.feed(instruction_sets(), &[]);
                let board = self.board();
                let focus = board.focus.as_ref().expect("a focus card");
                assert!(focus.why.is_some() && focus.instruction.is_some());
                assert_eq!(focus.button, None);
                self.capture("02-focus-instruction");
            }
            2 => {
                self.feed(all_ok_sets(), &[]);
                let board = self.board();
                assert_eq!(
                    board.headline.as_deref(),
                    Some("Nothing needs attention in what was checked.")
                );
                assert!(board.footer && board.focus.is_none());
                self.capture("03-all-ok");
            }
            3 => {
                self.feed(mixed_sets(), &[EVENTS]);
                let board = self.board();
                let words: Vec<_> = board.quiet.iter().map(|l| l.status.word()).collect();
                assert_eq!(
                    words,
                    ["Not checked", "Unavailable", "Out of date", "Out of date"]
                );
                assert_eq!(board.fine.len(), 1);
                self.capture("04-mixed");
            }
            4 => {
                self.feed(Vec::new(), &[]);
                let board = self.board();
                assert!(board.nothing_reported);
                assert_eq!(
                    board.headline.as_deref(),
                    Some("No add-on has reported a check yet. Disk Manager, Event Watch and Memory Cleaner report checks here.")
                );
                self.capture("05-nothing-reported");
            }
            5 => {
                self.feed(attention_sets(), &[]);
                self.app.health_check = Some(super::health::CheckAgain::new(
                    std::time::Instant::now(),
                    self.app.services.health.received(),
                ));
                assert!(self.board().caption.ends_with("Checking…"));
                self.capture("06-checking");
            }
            6 => {
                self.feed(long_sets(), &[]);
                let board = self.board();
                assert_eq!(
                    board.more,
                    [(DISK.to_string(), "1 more from Disk Manager".to_string())]
                );
                self.capture("07-long-list");
            }
            7 => {
                self.feed(Vec::new(), &[]);
                std::fs::remove_dir_all(self.app.registry.root.join("addons")).unwrap();
                self.capture("08-nothing-installed");
            }
            8 => {
                std::fs::write(
                    self.output.join("result.txt"),
                    "PASS: Health on Home drawn in eight states from fixture findings (focus, focus with what to do, all OK, mixed, nothing reported, checking, long list, nothing installed). Inspect the captures.\n",
                )
                .unwrap();
                self.app.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}
