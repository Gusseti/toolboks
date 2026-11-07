mod backend;
use super::*;
use backend::{Action, Analysis, Media, Metadata, Shared, Volume};
use toolboks_addon_sdk::{Capability, HostRequest};

pub(super) struct Panel {
    service: backend::Service,
    selected: String,
    loaded: bool,
    allowed: bool,
    confirm: Option<(String, String, Action)>,
    request: Option<HostRequest>,
    tour_selection: Option<String>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            service: backend::service(),
            selected: String::new(),
            loaded: false,
            allowed: false,
            confirm: None,
            request: None,
            tour_selection: None,
        }
    }
}
impl Panel {
    pub fn tutorial_start(&mut self) {
        self.tour_selection = Some(self.selected.clone());
    }
    pub fn tutorial_finish(&mut self) {
        if let Some(selected) = self.tour_selection.take() {
            self.selected = selected;
        }
    }
    pub fn capabilities(&mut self, caps: &[Capability]) {
        self.allowed = caps.contains(&Capability::DriveOptimize);
    }
    pub fn request(&mut self) -> Option<HostRequest> {
        self.request.take()
    }
    pub fn dismiss_confirmation(&mut self) {
        self.confirm = None;
    }
    pub fn busy(&self) -> bool {
        let s = self.service.lock().unwrap();
        s.job.is_some() || s.refreshing
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut BTreeMap<String, serde_json::Value>,
    ) -> bool {
        let tutorial = guide::active(ui.ctx());
        if !self.loaded && !tutorial {
            self.loaded = true;
            self.selected = prefs
                .get("optimize_selected_drive")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into();
            if let Some(value) = prefs.get("fragmentation_reports") {
                if let Ok(reports) =
                    serde_json::from_value::<BTreeMap<String, Analysis>>(value.clone())
                {
                    let mut s = self.service.lock().unwrap();
                    for (id, r) in reports {
                        if id == r.device_id {
                            s.reports.entry(id).or_insert(r);
                        }
                    }
                }
            }
            backend::refresh(&self.service);
        }
        let state = if tutorial {
            sample()
        } else {
            self.service.lock().unwrap().clone()
        };
        if state.refreshing || state.job.is_some() {
            ui.request_repaint_after(Duration::from_millis(250));
        }
        theme::title(ui, "Drive optimization");
        weak(
            ui,
            "Check how fragmented a drive is, then defragment a hard drive or run TRIM on an SSD. Nothing starts until you confirm.",
        );
        if tutorial {
            weak(ui, "Tutorial sample. No disk operation runs.");
        }
        ui.add_space(theme::SPACE_L);
        if !self.allowed {
            card().show(ui,|ui|{ui.set_width(ui.available_width());theme::section(ui, "Optimization is turned off");weak(ui,"This version of Disk Manager can only read. Review the add-on update to allow analysis and optimization. You can still say no.");if ui.add(theme::outlined("Review Disk Manager update")).clicked(){self.request=Some(HostRequest::ReviewUpdate);}});
            ui.add_space(theme::SPACE_L);
        }
        if let Some(m) = &state.metadata {
            if !m.volumes.iter().any(|v| v.letter == self.selected) {
                self.selected = m
                    .volumes
                    .first()
                    .map(|v| v.letter.clone())
                    .unwrap_or_default();
            }
            guide::region(ui, "optimize-drive", |ui| {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("optimize-volume")
                        .width(270.0)
                        .selected_text(
                            m.volumes
                                .iter()
                                .find(|v| v.letter == self.selected)
                                .map_or("Select a drive".into(), |v| {
                                    format!("{}  {}", v.letter, v.label)
                                }),
                        )
                        .show_ui(ui, |ui| {
                            for v in &m.volumes {
                                if ui
                                    .selectable_value(
                                        &mut self.selected,
                                        v.letter.clone(),
                                        format!("{}  {}", v.letter, v.label),
                                    )
                                    .changed()
                                {
                                    self.confirm = None;
                                }
                            }
                        });
                    if ui
                        .add_enabled(
                            !state.refreshing && state.job.is_none(),
                            theme::outlined("Refresh drive information"),
                        )
                        .clicked()
                    {
                        backend::refresh(&self.service);
                    }
                })
            });
            if let Some(v) = m.volumes.iter().find(|v| v.letter == self.selected) {
                ui.add_space(theme::SPACE_M);
                weak(
                    ui,
                    format!(
                        "{} · {} · {} free of {}",
                        v.media.label(),
                        v.filesystem,
                        size(v.free),
                        size(v.capacity)
                    ),
                );
                weak(
                    ui,
                    format!("Drive information checked {} · local time", m.checked_at),
                );
                if v.media == Media::Unknown {
                    weak(ui,"The hardware did not expose a reliable media type. Windows will choose its appropriate optimization.");
                }
                ui.add_space(theme::SPACE_L);
                let report = state.reports.get(&v.device_id);
                guide::region(ui, "optimize-fragmentation", |ui| {
                    card().show(ui,|ui|{
                    ui.set_width(ui.available_width());theme::section(ui,"Fragmentation");ui.add_space(theme::SPACE_S);
                    ui.horizontal_wrapped(|ui|{for(label,value)in [("total fragmentation",report.and_then(|a|a.total_percent)),("file fragmentation",report.and_then(|a|a.file_percent)),("free-space fragmentation",report.and_then(|a|a.free_percent))]{theme::stat(ui,value.map_or("Not analyzed".into(),|n|format!("{n:.0}%")),label);}});
                    if let Some(r)=report{weak(ui,format!("Analyzed {} · local time · snapshot, not a live reading",r.checked_at));weak(ui,format!("{} fragmented files of {}",r.fragmented_files.map_or("Unknown".into(),|n|n.to_string()),r.total_files.map_or("unknown".into(),|n|n.to_string())));if v.media==Media::Hdd{weak(ui,if r.recommended{"Windows recommended defragmentation at the time of analysis."}else{"Windows did not recommend defragmentation at the time of analysis."});}}
                    else{weak(ui,"Analyze this drive to get a numeric report from Windows. Unavailable measurements are never shown as zero.");}
                    if v.media==Media::Ssd{weak(ui,"Fragmentation is not an SSD health or wear score. TRIM tells the SSD which unused blocks can be reclaimed.");}
                });
                });
                ui.add_space(theme::SPACE_L);
                guide::region(ui, "optimize-history", |ui| {
                    card().show(ui,|ui|{
                    ui.set_width(ui.available_width());theme::section(ui,"Last recorded optimization");
                    ui.columns(3,|cols|{for(col,(title,record))in cols.iter_mut().zip([("Latest Windows record",v.history.first()),("Last defrag",v.history.iter().find(|h|h.kind()=="Defrag")),("Last TRIM",v.history.iter().find(|h|h.kind()=="TRIM"))]){weak(col,title);if let Some(h)=record{col.label(&h.time);weak(col,format!("{} · {}",h.kind(),h.operation));col.label(RichText::new(&h.volume).small().weak()).on_hover_text(&h.utc);}else{col.label("No available record");}}});
                    weak(ui,"Successful Windows optimization events for this drive letter · local time. Missing records do not mean the drive was never optimized. Available history is limited by Windows log retention.");
                    if let Some(w)=&m.history_warning{theme::caption(ui,format!("Windows history could not be read: {w}"));}
                });
                });
                ui.add_space(theme::SPACE_L);
                guide::region(ui, "optimize-actions", |ui| {
                    card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        theme::section(ui, "Maintain this drive");
                        if v.dirty {
                            weak(ui, "Windows marked this filesystem as needing a check. Optimization is unavailable.");
                        } else if !v.supported() {
                            weak(ui, "Optimization is unavailable for this filesystem or drive state.");
                        }
                        if v.media == Media::Hdd && v.free as f64 / (v.capacity.max(1) as f64) < 0.15 {
                            weak(ui, "Less than 15% free space. Windows may only partially defragment this drive.");
                        }
                        ui.add_space(theme::SPACE_M);
                        let enabled = self.allowed && m.elevated && v.supported() && !self.busy() && !tutorial;
                        if let Some((letter, device_id, action)) = self.confirm.clone().filter(|(letter, id, _)| letter == &v.letter && id == &v.device_id && !tutorial) {
                            theme::section(ui, &format!("{} {letter}?", action.label()));
                            weak(ui, match action {
                                Action::Trim => "Windows will tell the SSD which unused blocks it can reclaim.",
                                Action::Defrag => "Windows will rearrange fragmented files. The drive may be busy while this runs.",
                                _ => "Windows will choose the appropriate optimization for this drive.",
                            });
                            weak(ui, "Toolboks cannot undo drive optimization.");
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled_ui(enabled, |ui| theme::primary(ui, "Start optimization")).inner.clicked() {
                                    backend::start(&self.service, &letter, &device_id, action);
                                    self.confirm = None;
                                }
                                if ui.add(theme::outlined("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    self.confirm = None;
                                }
                            });
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                if !m.elevated && self.allowed {
                                    if ui.add_enabled_ui(!self.busy() && !tutorial, |ui| theme::primary(ui, "Restart with administrator access")).inner.clicked() {
                                        self.request = Some(HostRequest::RestartElevated { page: "4".into() });
                                    }
                                } else if ui.add_enabled_ui(enabled, |ui| theme::primary(ui, "Analyze fragmentation")).inner.clicked() {
                                    backend::start(&self.service, &v.letter, &v.device_id, Action::Analyze);
                                }
                                if ui.add_enabled(enabled, theme::outlined(v.action().label())).clicked() {
                                    self.confirm = Some((v.letter.clone(), v.device_id.clone(), v.action()));
                                }
                            });
                            weak(ui, "Windows runs maintenance in the background. It may continue after Toolboks closes.");
                        }
                    });
                });
            } else {
                weak(
                    ui,
                    "No local fixed drives with drive letters are available.",
                );
            }
        } else {
            if state.refreshing {
                ui.horizontal(|ui| {
                    ui.label("Reading drive information and Windows history...");
                });
            }
            if ui
                .add_enabled(
                    !state.refreshing,
                    theme::outlined("Retry drive information"),
                )
                .clicked()
            {
                backend::refresh(&self.service);
            }
        }
        ui.add_space(theme::SPACE_L);
        guide::region(ui, "optimize-progress", |ui| {
            card().show(ui,|ui|{
            ui.set_width(ui.available_width());theme::section(ui,"Activity and Windows report");
            if let Some(job)=&state.job{ui.label(format!("{} · {} · {}s",job.action.label(),job.letter,job.started.elapsed().as_secs()));if let Some(p)=job.phase_percent{ui.add(egui::ProgressBar::new(p/100.0).text(format!("Windows-reported percentage · {p:.0}%")));weak(ui,"Windows may report several phases; this is not an overall completion estimate.");}
if !job.output.is_empty(){egui::ScrollArea::vertical().id_salt("optimize-output").max_height(180.0).show(ui,|ui|{ui.label(RichText::new(&job.output).monospace().size(12.0));});}}
            else{weak(ui,"No Toolboks optimization is running.");}
            if let Some(message)=&state.message{ui.label(message);}
            if !state.last_output.is_empty(){egui::CollapsingHeader::new("Last Windows report").show(ui,|ui|{ui.label(RichText::new(&state.last_output).monospace().size(12.0));});}
        });
        });
        if tutorial {
            return false;
        }
        let reports = serde_json::to_value(&state.reports).unwrap_or_default();
        let selected = serde_json::Value::String(self.selected.clone());
        let changed = prefs.get("fragmentation_reports") != Some(&reports)
            || prefs.get("optimize_selected_drive") != Some(&selected);
        if changed {
            prefs.insert("fragmentation_reports".into(), reports);
            prefs.insert("optimize_selected_drive".into(), selected);
        }
        changed
    }
}
fn sample() -> Shared {
    let v = Volume {
        letter: "C:".into(),
        device_id: "tutorial-sample".into(),
        label: "Sample SSD".into(),
        filesystem: "NTFS".into(),
        capacity: 512 * 1024 * 1024 * 1024,
        free: 180 * 1024 * 1024 * 1024,
        media: Media::Ssd,
        history: vec![backend::History {
            operation: "retrim".into(),
            volume: "(C:)".into(),
            time: "Sample · yesterday".into(),
            utc: "Illustrative record".into(),
        }],
        ..Default::default()
    };
    let r = Analysis {
        device_id: v.device_id.clone(),
        letter: v.letter.clone(),
        total_percent: Some(3.0),
        file_percent: Some(3.0),
        free_percent: Some(1.0),
        fragmented_files: Some(120),
        total_files: Some(4000),
        checked_at: "Sample analysis".into(),
        ..Default::default()
    };
    Shared {
        metadata: Some(Metadata {
            elevated: false,
            volumes: vec![v],
            checked_at: "Sample metadata".into(),
            ..Default::default()
        }),
        reports: BTreeMap::from([(r.device_id.clone(), r)]),
        ..Default::default()
    }
}
