//! Scheduled tasks (telemetry and consumer-experience tasks) behind an exact-path allow-list.
//! See docs/RESEARCH-debloat.md section 4 and 9.2.
#![allow(dead_code)]

use super::model::Risk;
use windows::core::BSTR;
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, VARIANT_BOOL};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{IRegisteredTask, ITaskService, TaskScheduler};
use windows::Win32::System::Variant::VARIANT;

/// One task we know, matched by its exact full path, never by pattern.
#[derive(Clone, Copy, Debug)]
pub struct TaskDef {
    pub id: &'static str,
    /// Full path, e.g. `\Microsoft\Windows\Customer Experience Improvement Program\Consolidator`.
    pub path: &'static str,
    pub title: &'static str,
    pub what_changes: &'static str,
    pub could_break: &'static str,
    pub risk: Risk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Enabled,
    Disabled,
    /// The task does not exist on this PC (not an error).
    NotPresent,
    /// Windows refused access (task ACL); skipped, never taken over.
    Protected,
    Unknown(String),
}

/// Task Scheduler access behind a trait so the logic is tested with a fake. The real one uses the
/// Task Scheduler 2.0 COM API and must be called from a thread that may initialise COM (a worker
/// thread, not the UI thread).
pub trait Tasks {
    fn state(&self, path: &str) -> TaskState;
    fn set_enabled(&mut self, path: &str, enabled: bool) -> Result<(), String>;
}

pub struct WinTasks;

/// Balances CoInitializeEx with CoUninitialize only when this call initialised COM.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> ComGuard {
        // S_FALSE (already initialised on this thread) is a success and still needs balancing.
        // RPC_E_CHANGED_MODE means another apartment model is active: use it, do not balance.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ComGuard(hr.is_ok() && hr != RPC_E_CHANGED_MODE)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

enum Fail {
    NotPresent,
    Protected,
    Other(String),
}

fn classify(e: &windows::core::Error) -> Fail {
    match e.code().0 as u32 {
        0x8007_0002 | 0x8007_0003 => Fail::NotPresent,
        0x8007_0005 => Fail::Protected,
        _ => Fail::Other(e.message()),
    }
}

fn get_task(path: &str) -> Result<IRegisteredTask, Fail> {
    let svc: ITaskService = unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| Fail::Other(e.message()))?;
    let empty = VARIANT::default();
    unsafe { svc.Connect(&empty, &empty, &empty, &empty) }.map_err(|e| classify(&e))?;
    let root = unsafe { svc.GetFolder(&BSTR::from("\\")) }.map_err(|e| classify(&e))?;
    unsafe { root.GetTask(&BSTR::from(path)) }.map_err(|e| classify(&e))
}

impl Tasks for WinTasks {
    fn state(&self, path: &str) -> TaskState {
        let _com = ComGuard::new();
        let task = match get_task(path) {
            Ok(t) => t,
            Err(Fail::NotPresent) => return TaskState::NotPresent,
            Err(Fail::Protected) => return TaskState::Protected,
            Err(Fail::Other(m)) => return TaskState::Unknown(m),
        };
        match unsafe { task.Enabled() } {
            Ok(v) if v.as_bool() => TaskState::Enabled,
            Ok(_) => TaskState::Disabled,
            Err(e) => match classify(&e) {
                Fail::NotPresent => TaskState::NotPresent,
                Fail::Protected => TaskState::Protected,
                Fail::Other(m) => TaskState::Unknown(m),
            },
        }
    }

    fn set_enabled(&mut self, path: &str, enabled: bool) -> Result<(), String> {
        let _com = ComGuard::new();
        let task = match get_task(path) {
            Ok(t) => t,
            Err(Fail::NotPresent) => return Err("Not on this PC".to_string()),
            Err(Fail::Protected) => return Err("Protected by Windows, skipped".to_string()),
            Err(Fail::Other(m)) => return Err(m),
        };
        let v = VARIANT_BOOL(if enabled { -1 } else { 0 });
        unsafe { task.SetEnabled(v) }.map_err(|e| match classify(&e) {
            Fail::Protected => "Protected by Windows, skipped".to_string(),
            Fail::NotPresent => "Not on this PC".to_string(),
            Fail::Other(m) => m,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStatus {
    pub id: &'static str,
    pub state: TaskState,
}

/// What a change replaced, so it can be put back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoStep {
    pub id: &'static str,
    pub path: &'static str,
    pub was_enabled: bool,
}

static CATALOG: &[TaskDef] = &[
    // ---- Safe ----
    TaskDef {
        id: "ceip-consolidator",
        path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\Consolidator",
        title: "Usage data upload (customer experience program)",
        what_changes: "Stops Windows from collecting and sending usage statistics to Microsoft.",
        could_break: "Nothing you will notice. Only the usage data upload stops.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "ceip-usb",
        path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\UsbCeip",
        title: "USB usage data upload",
        what_changes: "Stops Windows from sending data about the USB devices you plug in.",
        could_break: "Nothing you will notice.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "siuf-dmclient",
        path: "\\Microsoft\\Windows\\Feedback\\Siuf\\DmClient",
        title: "Feedback prompt helper",
        what_changes: "Stops a background job that prepares Windows feedback questions.",
        could_break: "Windows may no longer ask you for feedback. Nothing else.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "siuf-dmclient-scenario",
        path: "\\Microsoft\\Windows\\Feedback\\Siuf\\DmClientOnScenarioDownload",
        title: "Feedback prompt downloader",
        what_changes: "Stops a background job that downloads Windows feedback questions.",
        could_break: "Windows may no longer ask you for feedback. Nothing else.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "feedback-toast",
        path: "\\Microsoft\\Windows\\PerformanceTrace\\ShowFeedbackToast",
        title: "Feedback pop-up",
        what_changes: "Stops the pop-up that asks how Windows is performing.",
        could_break: "Nothing. You just see fewer pop-ups.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "maps-toast",
        path: "\\Microsoft\\Windows\\Maps\\MapsToastTask",
        title: "Maps notifications",
        what_changes: "Stops notifications from the Maps app about downloaded maps.",
        could_break: "You will not get Maps pop-up notifications.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "wer-queue-reporting",
        path: "\\Microsoft\\Windows\\Windows Error Reporting\\QueueReporting",
        title: "Crash report upload",
        what_changes: "Stops Windows from sending queued crash reports to Microsoft.",
        could_break: "Crash reports are no longer sent, so Windows may not offer fixes for them.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "device-info",
        path: "\\Microsoft\\Windows\\Device Information\\Device",
        title: "Device information upload",
        what_changes: "Stops Windows from sending details about your PC hardware.",
        could_break: "Nothing you will notice.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "device-info-user",
        path: "\\Microsoft\\Windows\\Device Information\\Device User",
        title: "Device information upload (per user)",
        what_changes: "Stops Windows from sending details about your PC hardware for your account.",
        could_break: "Nothing you will notice.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "disk-footprint-diagnostics",
        path: "\\Microsoft\\Windows\\DiskFootprint\\Diagnostics",
        title: "Disk usage data collection",
        what_changes: "Stops Windows from collecting disk usage statistics.",
        could_break: "Nothing you will notice.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "appexp-mare-backup",
        path: "\\Microsoft\\Windows\\Application Experience\\MareBackup",
        title: "Compatibility data backup",
        what_changes: "Stops a background job that backs up app compatibility data.",
        could_break: "Very little. Some app compatibility hints may be less up to date.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "appexp-pca-patch-db",
        path: "\\Microsoft\\Windows\\Application Experience\\PcaPatchDbTask",
        title: "App compatibility database update",
        what_changes: "Stops a background job that refreshes the app compatibility database.",
        could_break: "Very little. Windows may warn less often about old programs.",
        risk: Risk::Safe,
    },
    TaskDef {
        id: "appexp-startup-app",
        path: "\\Microsoft\\Windows\\Application Experience\\StartupAppTask",
        title: "Startup app notification check",
        what_changes: "Stops the check that tells you when a new app has added itself to startup.",
        could_break: "You will not get a notice when an app adds itself to startup.",
        risk: Risk::Safe,
    },
    // ---- Careful ----
    TaskDef {
        id: "appexp-appraiser",
        path: "\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser",
        title: "Compatibility appraiser",
        what_changes: "Stops the job that scans your PC and sends compatibility data to Microsoft.",
        could_break: "Windows Update may have less information when deciding whether to offer you a big feature update. Some PCs may be offered updates later, or not at all.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "autochk-proxy",
        path: "\\Microsoft\\Windows\\Autochk\\Proxy",
        title: "System quality data proxy",
        what_changes: "Stops a helper that gathers system quality data. It does not affect disk checks.",
        could_break: "Some diagnostic data is no longer collected. Nothing you will notice day to day.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "xbl-game-save",
        path: "\\Microsoft\\XblGameSave\\XblGameSaveTask",
        title: "Xbox game saves sync",
        what_changes: "Stops the background sync of Xbox game saves.",
        could_break: "Games that use Xbox cloud saves may not sync your progress between PCs.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "xbl-game-save-logon",
        path: "\\Microsoft\\XblGameSave\\XblGameSaveTaskLogon",
        title: "Xbox game saves sync at sign-in",
        what_changes: "Stops the Xbox game saves sync that runs when you sign in.",
        could_break: "Games that use Xbox cloud saves may not sync your progress between PCs.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "family-safety-monitor",
        path: "\\Microsoft\\Windows\\Shell\\FamilySafetyMonitor",
        title: "Family Safety monitor",
        what_changes: "Stops the job that enforces Microsoft Family Safety rules.",
        could_break: "Parental controls and screen time limits from Family Safety stop working. Fine on a PC that does not use them.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "family-safety-refresh",
        path: "\\Microsoft\\Windows\\Shell\\FamilySafetyRefreshTask",
        title: "Family Safety refresh",
        what_changes: "Stops the job that refreshes Microsoft Family Safety settings.",
        could_break: "Parental controls and screen time limits from Family Safety stop updating. Fine on a PC that does not use them.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "flighting-usage-reporting",
        path: "\\Microsoft\\Windows\\Flighting\\FeatureConfig\\UsageDataReporting",
        title: "Feature usage reporting",
        what_changes: "Stops reports on which Windows features you use.",
        could_break: "Not fully known. Microsoft may roll out new features to your PC differently.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "flighting-usage-receiver",
        path: "\\Microsoft\\Windows\\Flighting\\FeatureConfig\\UsageDataReceiver",
        title: "Feature usage collector",
        what_changes: "Stops the collector of Windows feature usage data.",
        could_break: "Not fully known. Microsoft may roll out new features to your PC differently.",
        risk: Risk::Careful,
    },
    TaskDef {
        id: "flighting-usage-flushing",
        path: "\\Microsoft\\Windows\\Flighting\\FeatureConfig\\UsageDataFlushing",
        title: "Feature usage data sender",
        what_changes: "Stops sending of collected Windows feature usage data.",
        could_break: "Not fully known. Microsoft may roll out new features to your PC differently.",
        risk: Risk::Careful,
    },
];

/// All known tasks (Safe first, then Careful).
pub fn catalog() -> &'static [TaskDef] {
    CATALOG
}

/// Paths (and path prefixes) that are never offered or changed, whatever is asked.
pub fn is_never_offered(path: &str) -> bool {
    const DENY: &[&str] = &[
        "\\updateorchestrator",
        "\\windowsupdate",
        "\\waasmedic",
        "\\windows defender",
        "\\systemrestore",
        "\\time synchronization",
        "\\servicing",
        "\\tpm",
        "\\bitlocker",
        "\\defrag",
        "\\storagesense",
        "\\createobjecttask",
        "\\reconcileconfigs",
        "\\reconcilefeatures",
        "\\safeguardsreconciliation",
        "\\onesettings",
        "\\cloudexperiencehost",
    ];
    let p = path.to_lowercase();
    DENY.iter().any(|d| p.contains(d))
}

/// One status per catalog entry, same order.
pub fn statuses(tasks: &dyn Tasks) -> Vec<TaskStatus> {
    catalog()
        .iter()
        .map(|d| TaskStatus {
            id: d.id,
            state: tasks.state(d.path),
        })
        .collect()
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

fn state_for(on: bool) -> TaskState {
    if on {
        TaskState::Enabled
    } else {
        TaskState::Disabled
    }
}

/// Switches one catalog task on or off. Refuses tasks not in the catalog or never offered, tasks
/// that are not present or protected, and re-reads the state to confirm.
pub fn set_enabled(tasks: &mut dyn Tasks, def: &TaskDef, enable: bool) -> Result<UndoStep, String> {
    let known = catalog()
        .iter()
        .find(|d| d.path == def.path)
        .ok_or_else(|| "This task is not on the list, so it was left alone".to_string())?;
    if is_never_offered(known.path) {
        return Err("Windows needs this task, so it was left alone".to_string());
    }
    let was_enabled = match tasks.state(known.path) {
        TaskState::Enabled => true,
        TaskState::Disabled => false,
        TaskState::NotPresent => return Err("Not on this PC".to_string()),
        TaskState::Protected => return Err("Protected by Windows, skipped".to_string()),
        TaskState::Unknown(m) => return Err(format!("Could not read the task: {m}")),
    };
    if was_enabled == enable {
        return Err(format!("Already {}", on_off(enable)));
    }
    tasks.set_enabled(known.path, enable)?;
    if tasks.state(known.path) != state_for(enable) {
        let _ = tasks.set_enabled(known.path, was_enabled);
        return Err("Windows did not keep the change, so it was put back".to_string());
    }
    Ok(UndoStep {
        id: known.id,
        path: known.path,
        was_enabled,
    })
}

pub fn undo(tasks: &mut dyn Tasks, step: &UndoStep) -> Result<(), String> {
    tasks.set_enabled(step.path, step.was_enabled)?;
    if tasks.state(step.path) != state_for(step.was_enabled) {
        return Err("Windows did not keep the change".to_string());
    }
    Ok(())
}

pub fn undo_text(steps: &[UndoStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let title = catalog()
                .iter()
                .find(|d| d.id == s.id)
                .map(|d| d.title)
                .unwrap_or(s.path);
            format!("{title}: back to {}", on_off(s.was_enabled))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct FakeTasks {
        map: HashMap<String, TaskState>,
        fail_writes: bool,
        /// Number of writes silently ignored before writes stick.
        ignore_writes: u32,
    }

    impl FakeTasks {
        fn with(path: &str, st: TaskState) -> FakeTasks {
            let mut f = FakeTasks::default();
            f.map.insert(path.to_string(), st);
            f
        }
    }

    impl Tasks for FakeTasks {
        fn state(&self, path: &str) -> TaskState {
            self.map.get(path).cloned().unwrap_or(TaskState::NotPresent)
        }
        fn set_enabled(&mut self, path: &str, enabled: bool) -> Result<(), String> {
            if self.fail_writes {
                return Err("denied".to_string());
            }
            if self.ignore_writes > 0 {
                self.ignore_writes -= 1;
                return Ok(());
            }
            self.map.insert(path.to_string(), state_for(enabled));
            Ok(())
        }
    }

    fn first() -> &'static TaskDef {
        &catalog()[0]
    }

    #[test]
    fn catalog_is_sane() {
        let c = catalog();
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for d in c {
            assert!(ids.insert(d.id), "dup id {}", d.id);
            assert!(paths.insert(d.path.to_lowercase()), "dup path {}", d.path);
            assert!(!d.title.is_empty() && !d.what_changes.is_empty() && !d.could_break.is_empty());
            assert!(d.path.starts_with("\\Microsoft\\"));
            assert!(!is_never_offered(d.path), "{} is never-offered", d.path);
            assert!(d
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
        }
        assert!(c.iter().any(|d| d.risk == Risk::Careful));
        assert!(c.iter().any(|d| d.risk == Risk::Safe));
        let first_careful = c.iter().position(|d| d.risk == Risk::Careful).unwrap();
        assert!(c[first_careful..].iter().all(|d| d.risk == Risk::Careful));
    }

    #[test]
    fn never_offered_list() {
        for p in [
            "\\Microsoft\\Windows\\UpdateOrchestrator\\Schedule Scan",
            "\\Microsoft\\Windows\\WindowsUpdate\\Scheduled Start",
            "\\Microsoft\\Windows\\WaaSMedic\\PerformRemediation",
            "\\Microsoft\\Windows\\Windows Defender\\Windows Defender Scheduled Scan",
            "\\Microsoft\\Windows\\SystemRestore\\SR",
            "\\Microsoft\\Windows\\Time Synchronization\\SynchronizeTime",
            "\\Microsoft\\Windows\\Servicing\\StartComponentCleanup",
            "\\Microsoft\\Windows\\TPM\\Tpm-Maintenance",
            "\\Microsoft\\Windows\\BitLocker\\BitLocker MDM policy Refresh",
            "\\Microsoft\\Windows\\Defrag\\ScheduledDefrag",
            "\\Microsoft\\Windows\\DiskFootprint\\StorageSense",
            "\\Microsoft\\Windows\\CloudExperienceHost\\CreateObjectTask",
            "\\Microsoft\\Windows\\Shell\\CreateObjectTask",
            "\\Microsoft\\Windows\\Flighting\\ReconcileConfigs",
            "\\Microsoft\\Windows\\Flighting\\ReconcileFeatures",
            "\\Microsoft\\Windows\\Flighting\\SafeguardsReconciliation",
            "\\Microsoft\\Windows\\Flighting\\OneSettings\\RefreshCache",
            "\\microsoft\\windows\\updateorchestrator\\x",
        ] {
            assert!(is_never_offered(p), "{p}");
        }
        assert!(!is_never_offered(
            "\\Microsoft\\Windows\\Maps\\MapsToastTask"
        ));
    }

    #[test]
    fn statuses_cover_states() {
        let mut f = FakeTasks::default();
        f.map
            .insert(catalog()[0].path.to_string(), TaskState::Enabled);
        f.map
            .insert(catalog()[1].path.to_string(), TaskState::Disabled);
        f.map
            .insert(catalog()[2].path.to_string(), TaskState::Protected);
        let s = statuses(&f);
        assert_eq!(s.len(), catalog().len());
        assert_eq!(s[0].state, TaskState::Enabled);
        assert_eq!(s[1].state, TaskState::Disabled);
        assert_eq!(s[2].state, TaskState::Protected);
        assert_eq!(s[3].state, TaskState::NotPresent);
        assert_eq!(s[0].id, catalog()[0].id);
    }

    #[test]
    fn round_trip_and_undo() {
        let d = first();
        let mut f = FakeTasks::with(d.path, TaskState::Enabled);
        let step = set_enabled(&mut f, d, false).unwrap();
        assert!(step.was_enabled);
        assert_eq!(f.state(d.path), TaskState::Disabled);
        undo(&mut f, &step).unwrap();
        assert_eq!(f.state(d.path), TaskState::Enabled);
        assert_eq!(undo_text(&[step]), format!("{}: back to on", d.title));
    }

    #[test]
    fn undo_text_off_and_fallback() {
        let t = undo_text(&[
            UndoStep {
                id: "nope",
                path: "\\X\\Y",
                was_enabled: false,
            },
            UndoStep {
                id: catalog()[0].id,
                path: catalog()[0].path,
                was_enabled: false,
            },
        ]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "\\X\\Y: back to off");
        assert_eq!(lines[1], format!("{}: back to off", catalog()[0].title));
    }

    #[test]
    fn refusals() {
        let d = first();
        let mut f = FakeTasks::with(d.path, TaskState::Enabled);
        let stranger = TaskDef {
            path: "\\Microsoft\\Windows\\Maps\\Other",
            ..*d
        };
        assert!(set_enabled(&mut f, &stranger, false).is_err());
        let denied = TaskDef {
            path: "\\Microsoft\\Windows\\WindowsUpdate\\Scheduled Start",
            ..*d
        };
        assert!(set_enabled(&mut f, &denied, false).is_err());
        let mut f = FakeTasks::default();
        assert!(set_enabled(&mut f, d, false).is_err());
        let mut f = FakeTasks::with(d.path, TaskState::Protected);
        assert_eq!(
            set_enabled(&mut f, d, false).unwrap_err(),
            "Protected by Windows, skipped"
        );
        let mut f = FakeTasks::with(d.path, TaskState::Unknown("x".into()));
        assert!(set_enabled(&mut f, d, false).is_err());
        let mut f = FakeTasks::with(d.path, TaskState::Disabled);
        assert!(set_enabled(&mut f, d, false)
            .unwrap_err()
            .starts_with("Already"));
        let mut f = FakeTasks::with(d.path, TaskState::Enabled);
        assert!(set_enabled(&mut f, d, true)
            .unwrap_err()
            .starts_with("Already"));
        let mut f = FakeTasks::with(d.path, TaskState::Enabled);
        f.fail_writes = true;
        assert!(set_enabled(&mut f, d, false).is_err());
        assert_eq!(f.state(d.path), TaskState::Enabled);
    }

    #[test]
    fn read_back_mismatch_restores() {
        let d = first();
        let mut f = FakeTasks::with(d.path, TaskState::Enabled);
        f.ignore_writes = 1;
        assert!(set_enabled(&mut f, d, false).is_err());
        assert_eq!(f.state(d.path), TaskState::Enabled);
    }

    #[test]
    #[ignore]
    fn real_statuses_smoke() {
        let s = statuses(&WinTasks);
        assert_eq!(s.len(), catalog().len());
        for st in &s {
            println!("{:<28} {:?}", st.id, st.state);
        }
    }
}
