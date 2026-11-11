mod engine;

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsStr,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::windows::ffi::OsStrExt,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, RichText};
use toolboks_addon_sdk::{tutorial, Capability, HostRequest, NativeAddon, TutorialStep};
use windows::{core::PCWSTR, Win32::UI::Shell::ShellExecuteW};

use crate::{elevation, ui as theme};
use engine::{Action, CHOICES};

const PAGE: &str = "Gaming";
const LOG_LINES: usize = 120;

enum Event {
    Line(String),
    Done(Result<(), String>),
}

#[derive(Default)]
pub(super) struct Gaming {
    allowed: bool,
    initialized: bool,
    selected: BTreeSet<&'static str>,
    enable_protection: bool,
    request: Option<HostRequest>,
    receiver: Option<Receiver<Event>>,
    log: VecDeque<String>,
    status: Option<Result<(), String>>,
    message: Option<String>,
    preview: Option<Vec<String>>,
}

struct Guide {
    title: &'static str,
    instructions: &'static str,
    link: Option<(&'static str, &'static str)>,
}

const GUIDES: &[Guide] = &[
    Guide {
        title: "Windowed optimizations and GPU scheduling",
        instructions: "Open System → Display → Graphics. Turn on Optimizations for windowed games and Hardware-accelerated GPU scheduling if available. Restart Windows after changing GPU scheduling.",
        link: Some(("Open Graphics settings", "ms-settings:display-advancedgraphics")),
    },
    Guide {
        title: "NVIDIA global graphics settings",
        instructions: "Open NVIDIA Control Panel → Manage 3D Settings → Global Settings. Note your current values. Set Shader Cache Size to 10 GB, Vertical sync to Off, and Power management mode to Prefer maximum performance. Apply. These settings affect other 3D apps too.",
        link: Some(("NVIDIA settings reference", "https://www.nvidia.com/content/Control-Panel-Help/vLatest/en-us/mergedProjects/3D%20Settings/Manage_3D_Settings_%28reference%29.htm")),
    },
    Guide {
        title: "Memory Integrity",
        instructions: "Windows Security → Device security → Core isolation details. Turning off Memory integrity weakens Windows protection against malicious drivers. If you choose to change it, restart Windows. Return here to turn it back on.",
        link: Some(("Open Windows Security", "windowsdefender:")),
    },
    Guide {
        title: "Temporary files and Disk Cleanup",
        instructions: "Open Settings → System → Storage → Temporary files, or search Start for Disk Cleanup. Review the selected categories before deleting. Deleted files cannot be recovered using this profile.",
        link: Some(("Open Storage settings", "ms-settings:storagesense")),
    },
    Guide {
        title: "Storage Sense",
        instructions: "Open Settings → System → Storage and switch Storage Sense off to stop automatic cleanup.",
        link: Some(("Open Storage Sense", "ms-settings:storagepolicies")),
    },
    Guide {
        title: "Location",
        instructions: "Open Privacy & security → Location. Turn off Location services to disable location access for the device.",
        link: Some(("Open Location settings", "ms-settings:privacy-location")),
    },
    Guide {
        title: "Start and search app-launch tracking",
        instructions: "Under Privacy & security, open General or Recommendations & offers. Turn off the option that improves Start and search results by tracking app launches.",
        link: Some(("Open privacy settings", "ms-settings:privacy-general")),
    },
    Guide {
        title: "Notifications",
        instructions: "Open System → Notifications and switch Notifications off.",
        link: Some(("Open Notifications settings", "ms-settings:notifications")),
    },
    Guide {
        title: "Taskbar End Task",
        instructions: "Search Windows Settings for End Task and enable it under Advanced or For developers, depending on your Windows version. Right-click an app in the taskbar to find End task.",
        link: Some(("Open developer settings", "ms-settings:developers")),
    },
    Guide {
        title: "Windows Terminal default profile",
        instructions: "Install PowerShell 7 if needed. In Windows Terminal, open Settings → Startup, select PowerShell as the default profile, and save. Windows PowerShell is the older profile.",
        link: Some(("Windows Terminal setup", "https://learn.microsoft.com/windows/terminal/install")),
    },
    Guide {
        title: "Recall",
        instructions: "On a Copilot+ PC, open Privacy & security → Recall & snapshots and turn off Save snapshots. Removing Recall through Windows Features also permanently deletes its snapshots.",
        link: Some(("Microsoft Recall settings", "https://support.microsoft.com/en-us/windows/privacy/privacy-and-control-over-your-recall-experience")),
    },
    Guide {
        title: "Explorer automatic folder discovery",
        instructions: "To fix the view type of a folder, open its Properties → Customize and choose a template under Optimize this folder for. The global registry override is undocumented.",
        link: None,
    },
    Guide {
        title: "HomeGroup and Wi-Fi Sense",
        instructions: "Current Windows versions have removed these features. There is no setting to change.",
        link: None,
    },
];

fn open(target: &OsStr) -> Result<(), String> {
    let target: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
    // SAFETY: target is a NUL-terminated path or a fixed settings URI. No arguments are passed.
    let code = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if (code.0 as isize) <= 32 {
        Err("Windows could not open that location.".into())
    } else {
        Ok(())
    }
}

fn heading(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(theme::TEXT_SECTION).strong());
}

impl Gaming {
    fn initialize(&mut self, preferences: &BTreeMap<String, serde_json::Value>) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        let selected = preferences
            .get("selected")
            .and_then(|value| value.as_array());
        for choice in CHOICES {
            if selected.map_or(choice.id == "game-mode", |ids| {
                ids.iter().any(|id| id.as_str() == Some(choice.id))
            }) {
                self.selected.insert(choice.id);
            }
        }
        self.enable_protection = preferences
            .get("enable_protection")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if let Ok(mut file) = File::open(engine::root().join(engine::LOG)) {
            let len = file.metadata().map_or(0, |meta| meta.len());
            let offset = len.saturating_sub(32 * 1024);
            let mut bytes = Vec::new();
            if file.seek(SeekFrom::Start(offset)).is_ok()
                && file.take(32 * 1024).read_to_end(&mut bytes).is_ok()
            {
                let text = String::from_utf8_lossy(&bytes);
                let log = if offset > 0 {
                    text.split_once('\n').map_or("", |(_, rest)| rest)
                } else {
                    &text
                };
                let mut recent: Vec<_> = log.lines().rev().take(LOG_LINES).collect();
                recent.reverse();
                self.log.extend(recent.into_iter().map(str::to_owned));
            }
        }
    }

    fn start(&mut self, action: Action, ctx: &egui::Context) {
        if !self.allowed || self.receiver.is_some() {
            return;
        }
        self.status = None;
        self.message = None;
        self.preview = None;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let thread = std::thread::Builder::new()
            .name("gaming-profile".into())
            .spawn(move || {
                let result = engine::run(action, |line| {
                    let _ = tx.send(Event::Line(line));
                    ctx.request_repaint();
                });
                let _ = tx.send(Event::Done(result));
                ctx.request_repaint();
            });
        match thread {
            Ok(_) => self.receiver = Some(rx),
            Err(error) => self.status = Some(Err(format!("Could not start the profile: {error}"))),
        }
    }

    fn poll(&mut self) {
        let Some(rx) = &self.receiver else { return };
        loop {
            match rx.try_recv() {
                Ok(Event::Line(line)) => {
                    if self.log.len() == LOG_LINES {
                        self.log.pop_front();
                    }
                    self.log.push_back(line);
                }
                Ok(Event::Done(result)) => {
                    self.status = Some(result);
                    self.receiver = None;
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.status = Some(Err(
                        "The profile was interrupted. Check the log and recovery data.".into(),
                    ));
                    self.receiver = None;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
    }

    fn configuration(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let busy = self.receiver.is_some();
        let recovery = engine::has_backup();
        let elevated = elevation::is_elevated();
        heading(ui, "Profile changes");
        ui.label(RichText::new("Review the options you want to apply. Windows must confirm a new restore point before Toolboks changes the profile settings.").color(theme::MUTED));
        ui.add_enabled_ui(!busy && !recovery, |ui| {
            changed |= ui.checkbox(&mut self.enable_protection, "Turn on System Protection for the Windows drive").changed();
            ui.label(RichText::new("Enable this if System Protection is off. Windows keeps it enabled after a profile restore.").color(theme::MUTED));
            ui.separator();
            for choice in CHOICES {
                let mut on = self.selected.contains(choice.id);
                if ui.checkbox(&mut on, choice.title).changed() {
                    changed = true;
                    if on {
                        self.selected.insert(choice.id);
                    } else {
                        self.selected.remove(choice.id);
                    }
                }
                ui.label(RichText::new(choice.detail).color(theme::MUTED));
            }
        });
        if changed {
            self.preview = None;
        }
        let review_label = if recovery {
            "Review registry edits"
        } else {
            "Review current values"
        };
        if ui
            .add_enabled(
                !busy && (recovery || !self.selected.is_empty()),
                egui::Button::new(review_label),
            )
            .clicked()
        {
            self.preview = Some(if recovery {
                engine::review_backup().unwrap_or_else(|error| vec![error])
            } else {
                let selected: Vec<_> = CHOICES
                    .iter()
                    .filter(|choice| self.selected.contains(choice.id))
                    .map(|choice| choice.id)
                    .collect();
                engine::preview(&selected)
            });
        }
        if let Some(lines) = &self.preview {
            for line in lines {
                ui.label(RichText::new(line).color(theme::MUTED));
            }
        }
        ui.add_space(theme::SPACE_XS);
        ui.horizontal_wrapped(|ui| {
            if !elevated {
                if ui
                    .add_enabled(!busy, primary("Restart as administrator"))
                    .clicked()
                {
                    self.request = Some(HostRequest::RestartElevated { page: PAGE.into() });
                }
            } else if !recovery {
                let ready = !busy && (!self.selected.is_empty() || self.enable_protection);
                if ui
                    .add_enabled(ready, primary("Apply Gaming profile"))
                    .clicked()
                {
                    let selected = CHOICES
                        .iter()
                        .filter(|choice| self.selected.contains(choice.id))
                        .map(|choice| choice.id)
                        .collect();
                    self.start(
                        Action::Apply {
                            selected,
                            enable_protection: self.enable_protection,
                        },
                        ui.ctx(),
                    );
                }
            }
            if ui
                .add_enabled(!busy && recovery && elevated, egui::Button::new("Restore"))
                .clicked()
            {
                self.start(Action::Restore, ui.ctx());
            }
            if ui
                .add_enabled(
                    engine::root().is_dir(),
                    egui::Button::new("Open backup location"),
                )
                .clicked()
            {
                self.message = open(engine::root().as_os_str()).err();
            }
        });
        if recovery {
            ui.label(RichText::new("Restore uses the saved values for registry and power plan changes made by Toolboks. Guided changes are reversed in their original apps.").color(theme::MUTED));
        }
        changed
    }

    fn activity(&self, ui: &mut egui::Ui) {
        heading(ui, "Activity log");
        if self.receiver.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working in Windows…");
            });
        }
        if let Some(status) = &self.status {
            match status {
                Ok(()) => {
                    ui.label(RichText::new("Finished").color(theme::GREEN));
                }
                Err(error) => {
                    ui.label(RichText::new(error).color(theme::AMBER));
                }
            }
        }
        if let Some(message) = &self.message {
            ui.label(RichText::new(message).color(theme::AMBER));
        }
        if self.log.is_empty() {
            ui.label(
                RichText::new("Backup, apply, and restore activity will appear here.")
                    .color(theme::MUTED),
            );
        } else {
            egui::ScrollArea::vertical()
                .id_salt("gaming-log")
                .max_height(220.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.log {
                        ui.label(RichText::new(line).monospace().size(theme::TEXT_CAPTION));
                    }
                });
        }
    }

    fn guides(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Guided settings");
        ui.label(RichText::new("Follow these steps in Windows or NVIDIA Control Panel. Record your original settings before changing them.").color(theme::MUTED));
        for guide in GUIDES {
            ui.collapsing(guide.title, |ui| {
                ui.label(guide.instructions);
                if let Some((label, link)) = guide.link {
                    if link.starts_with("https://") {
                        ui.hyperlink_to(label, link);
                    } else if ui.button(label).clicked() {
                        self.message = open(OsStr::new(link)).err();
                    }
                }
                if guide.title == "Temporary files and Disk Cleanup"
                    && ui.button("Open Disk Cleanup").clicked()
                {
                    self.message = crate::win::windows_dir()
                        .and_then(|dir| open(dir.join("System32/cleanmgr.exe").as_os_str()))
                        .err();
                }
            });
        }
    }
}

fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(egui::Color32::WHITE))
        .fill(theme::HOST_ACCENT_FILL)
}

impl NativeAddon for Gaming {
    fn set_capabilities(&mut self, capabilities: &[Capability]) {
        self.allowed = capabilities.contains(&Capability::SystemTuning);
    }

    fn take_host_request(&mut self) -> Option<HostRequest> {
        self.request.take()
    }

    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        preferences: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        if !self.allowed {
            ui.label("The System tuning capability is required.");
            return false;
        }
        self.initialize(preferences);
        ui.label(RichText::new(PAGE).size(theme::TEXT_TITLE).strong());
        ui.label(
            RichText::new(
                "Choose Windows settings for your gaming setup and keep a recovery record.",
            )
            .color(theme::MUTED),
        );
        ui.add_space(theme::SPACE_XS);
        let mut changed = false;
        egui::ScrollArea::vertical()
            .id_salt("gaming-profile")
            .show(ui, |ui| {
                let width = ui.available_width();
                tutorial::region(ui, "gaming-activity", |ui| {
                    theme::card(ui, width, 0.0, |ui| self.activity(ui));
                });
                ui.add_space(theme::GAP);
                tutorial::region(ui, "gaming-changes", |ui| {
                    theme::card(ui, width, 0.0, |ui| {
                        changed = self.configuration(ui);
                    });
                });
                ui.add_space(theme::GAP);
                tutorial::region(ui, "gaming-guides", |ui| {
                    theme::card(ui, width, 0.0, |ui| self.guides(ui));
                });
            });
        if changed {
            preferences.insert("selected".into(), serde_json::json!(self.selected));
            preferences.insert(
                "enable_protection".into(),
                serde_json::json!(self.enable_protection),
            );
        }
        changed
    }

    fn tick(&mut self, _ctx: &egui::Context) {
        self.poll();
    }

    fn has_background_work(&self) -> bool {
        self.receiver.is_some()
    }

    fn tick_background(&mut self) {
        self.poll();
    }

    fn tutorial_location(&self) -> String {
        PAGE.into()
    }

    fn tutorial_steps(&self) -> Vec<TutorialStep> {
        vec![
            TutorialStep::new(PAGE, "gaming-changes", "Choose changes", "Select the settings you want and enable System Protection if it is off. Toolboks checks for a new restore point before applying changes."),
            TutorialStep::new(PAGE, "gaming-activity", "Watch the activity log", "The log shows the registry export location, changes applied, and verification results."),
            TutorialStep::new(PAGE, "gaming-guides", "Follow guided settings", "Open Windows and NVIDIA settings from this list. Reverse these changes in their original apps."),
            TutorialStep::new(PAGE, "gaming-changes", "Restore earlier settings", "Use Restore for settings Toolboks applied and Open backup location to view the saved registry export."),
        ]
    }
}
