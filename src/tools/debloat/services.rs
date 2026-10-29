//! Windows services behind an exact-name allow-list: only the START TYPE is changed (a running
//! service is never stopped, so a change takes effect at the next restart or stop).
//! See docs/RESEARCH-debloat.md section 3 and 9.1.
#![allow(dead_code)]

use windows::core::PCWSTR;
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, EnumDependentServicesW,
    OpenSCManagerW, OpenServiceW, QueryServiceConfig2W, QueryServiceConfigW, QueryServiceStatusEx,
    ENUM_SERVICE_STATUSW, ENUM_SERVICE_TYPE, QUERY_SERVICE_CONFIGW, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_STATUS_PROCESS_INFO, SERVICE_ACTIVE, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_LAUNCH_PROTECTED,
    SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED,
    SERVICE_ENUMERATE_DEPENDENTS, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_TYPE, SERVICE_STATUS_PROCESS,
};

use super::model::Risk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartType {
    Auto,
    /// Automatic (Delayed Start): start type Auto plus the delayed flag.
    AutoDelayed,
    Manual,
    Disabled,
    /// Boot/System or anything else we never write.
    Other(u32),
}

impl StartType {
    pub fn label(self) -> &'static str {
        match self {
            StartType::Auto => "Automatic",
            StartType::AutoDelayed => "Automatic (delayed)",
            StartType::Manual => "Manual",
            StartType::Disabled => "Disabled",
            StartType::Other(_) => "Other",
        }
    }
}

/// One service we know, matched by its exact service name, never by pattern.
#[derive(Clone, Copy, Debug)]
pub struct ServiceDef {
    pub id: &'static str,
    /// Service name for the SCM, e.g. `DiagTrack`.
    pub name: &'static str,
    pub title: &'static str,
    pub what_changes: &'static str,
    pub could_break: &'static str,
    pub risk: Risk,
    /// What "switch off" sets: `Disabled` for Safe items, `Manual` for Careful items.
    pub target: StartType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceState {
    Present {
        start: StartType,
        running: bool,
        /// Names of dependent services that are running right now.
        active_dependents: Vec<String>,
    },
    /// The service does not exist on this PC (not an error).
    NotPresent,
    /// Launch-protected or the access list refuses changes: shown as "Protected by Windows".
    Locked,
    Unknown(String),
}

/// Service Control Manager access behind a trait so the logic is tested with a fake.
pub trait Services {
    fn state(&self, name: &str) -> ServiceState;
    /// Changes the start type (and the delayed flag) only.
    fn set_start(&mut self, name: &str, start: StartType) -> Result<(), String>;
}

const E_ACCESS_DENIED: u32 = 5;
const E_MORE_DATA: u32 = 234;
const E_INSUFFICIENT_BUFFER: u32 = 122;
const E_SERVICE_DOES_NOT_EXIST: u32 = 1060;

const ACCESS_DENIED_TEXT: &str =
    "Needs administrator rights, or the service is protected by Windows";

/// Closes an SCM handle when dropped.
struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The Win32 error number inside a `windows` error (HRESULT_FROM_WIN32 form).
fn win32(e: &windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

fn open_scm() -> Result<Handle, String> {
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(Handle)
        .map_err(|e| {
            if win32(&e) == E_ACCESS_DENIED {
                ACCESS_DENIED_TEXT.to_string()
            } else {
                format!("Could not reach the Windows service manager: {e}")
            }
        })
}

fn open_service(scm: &Handle, name: &str, access: u32) -> windows::core::Result<Handle> {
    let name_w = wide(name);
    unsafe { OpenServiceW(scm.0, PCWSTR(name_w.as_ptr()), access) }.map(Handle)
}

fn map_start(raw: u32, delayed: bool) -> StartType {
    match raw {
        2 if delayed => StartType::AutoDelayed,
        2 => StartType::Auto,
        3 => StartType::Manual,
        4 => StartType::Disabled,
        other => StartType::Other(other),
    }
}

/// Reads one 4-byte `QueryServiceConfig2W` value, or `None` if the level is unavailable.
fn query_config2_u32(
    h: &Handle,
    level: windows::Win32::System::Services::SERVICE_CONFIG,
) -> Option<u32> {
    let mut buf = [0u8; 4];
    let mut needed = 0u32;
    unsafe { QueryServiceConfig2W(h.0, level, Some(&mut buf), &mut needed) }.ok()?;
    Some(u32::from_le_bytes(buf))
}

fn read_start_type(h: &Handle) -> Result<StartType, windows::core::Error> {
    let mut needed = 0u32;
    let first = unsafe { QueryServiceConfigW(h.0, None, 0, &mut needed) };
    if let Err(e) = first {
        let code = win32(&e);
        if code != E_INSUFFICIENT_BUFFER && code != E_MORE_DATA {
            return Err(e);
        }
    }
    // u64 elements keep the buffer 8-byte aligned for the struct.
    let mut buf = vec![0u64; (needed as usize).max(64).div_ceil(8) + 1];
    let cb = (buf.len() * 8) as u32;
    unsafe {
        QueryServiceConfigW(
            h.0,
            Some(buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
            cb,
            &mut needed,
        )?;
        let cfg = &*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
        let raw = cfg.dwStartType.0;
        let delayed = raw == SERVICE_AUTO_START.0
            && query_config2_u32(h, SERVICE_CONFIG_DELAYED_AUTO_START_INFO).unwrap_or(0) != 0;
        Ok(map_start(raw, delayed))
    }
}

fn read_running(h: &Handle) -> Result<bool, windows::core::Error> {
    let mut buf = [0u64; 8];
    let mut needed = 0u32;
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            buf.as_mut_ptr() as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    unsafe { QueryServiceStatusEx(h.0, SC_STATUS_PROCESS_INFO, Some(bytes), &mut needed)? };
    let status = unsafe { &*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS) };
    Ok(status.dwCurrentState == SERVICE_RUNNING)
}

fn read_active_dependents(h: &Handle) -> Result<Vec<String>, windows::core::Error> {
    let mut needed = 0u32;
    let mut count = 0u32;
    let first =
        unsafe { EnumDependentServicesW(h.0, SERVICE_ACTIVE, None, 0, &mut needed, &mut count) };
    match first {
        Ok(()) => return Ok(Vec::new()), // nothing to list
        Err(e) => {
            if win32(&e) != E_MORE_DATA {
                return Err(e);
            }
        }
    }
    let mut buf = vec![0u64; (needed as usize).div_ceil(8) + 8];
    let cb = (buf.len() * 8) as u32;
    unsafe {
        EnumDependentServicesW(
            h.0,
            SERVICE_ACTIVE,
            Some(buf.as_mut_ptr() as *mut ENUM_SERVICE_STATUSW),
            cb,
            &mut needed,
            &mut count,
        )?;
        let items =
            std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUSW, count as usize);
        Ok(items
            .iter()
            .filter_map(|i| i.lpServiceName.to_string().ok())
            .collect())
    }
}

fn classify(e: &windows::core::Error) -> ServiceState {
    match win32(e) {
        E_SERVICE_DOES_NOT_EXIST => ServiceState::NotPresent,
        E_ACCESS_DENIED => ServiceState::Locked,
        _ => ServiceState::Unknown(e.to_string()),
    }
}

/// The real Service Control Manager. Reads need no administrator rights; the only write is
/// `set_start`, which changes the start type and delayed flag and nothing else.
pub struct WinServices;

impl Services for WinServices {
    fn state(&self, name: &str) -> ServiceState {
        let scm = match open_scm() {
            Ok(h) => h,
            Err(e) => return ServiceState::Unknown(e),
        };
        let access = SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_ENUMERATE_DEPENDENTS;
        let h = match open_service(&scm, name, access) {
            Ok(h) => h,
            Err(e) => return classify(&e),
        };
        if let Some(level) = query_config2_u32(&h, SERVICE_CONFIG_LAUNCH_PROTECTED) {
            if level != 0 {
                return ServiceState::Locked;
            }
        }
        let start = match read_start_type(&h) {
            Ok(s) => s,
            Err(e) => return classify(&e),
        };
        let running = match read_running(&h) {
            Ok(r) => r,
            Err(e) => return classify(&e),
        };
        let active_dependents = match read_active_dependents(&h) {
            Ok(d) => d,
            Err(e) => return classify(&e),
        };
        ServiceState::Present {
            start,
            running,
            active_dependents,
        }
    }

    fn set_start(&mut self, name: &str, start: StartType) -> Result<(), String> {
        let (raw, delayed) = match start {
            StartType::Auto => (SERVICE_AUTO_START, false),
            StartType::AutoDelayed => (SERVICE_AUTO_START, true),
            StartType::Manual => (SERVICE_DEMAND_START, false),
            StartType::Disabled => (SERVICE_DISABLED, false),
            StartType::Other(_) => {
                return Err("That start type is never written by this tool".to_string())
            }
        };
        let scm = open_scm()?;
        let h = open_service(&scm, name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)
            .map_err(|e| describe_write_error(&e))?;
        unsafe {
            ChangeServiceConfigW(
                h.0,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                SERVICE_START_TYPE(raw.0),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                PCWSTR::null(),
                PCWSTR::null(),
                None,
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
                PCWSTR::null(),
            )
        }
        .map_err(|e| describe_write_error(&e))?;
        if raw == SERVICE_AUTO_START {
            let info = SERVICE_DELAYED_AUTO_START_INFO {
                fDelayedAutostart: delayed.into(),
            };
            unsafe {
                ChangeServiceConfig2W(
                    h.0,
                    SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                    Some(&info as *const _ as *const core::ffi::c_void),
                )
            }
            .map_err(|e| describe_write_error(&e))?;
        }
        Ok(())
    }
}

fn describe_write_error(e: &windows::core::Error) -> String {
    match win32(e) {
        E_ACCESS_DENIED => ACCESS_DENIED_TEXT.to_string(),
        E_SERVICE_DOES_NOT_EXIST => "That service is not on this PC".to_string(),
        _ => format!("Windows refused the change: {e}"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceStatus {
    pub id: &'static str,
    pub state: ServiceState,
}

/// What a change replaced, so it can be put back (exact start type and delayed flag).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoStep {
    pub id: &'static str,
    pub name: &'static str,
    pub previous: StartType,
}

const fn def(
    id: &'static str,
    name: &'static str,
    title: &'static str,
    what_changes: &'static str,
    could_break: &'static str,
    risk: Risk,
    target: StartType,
) -> ServiceDef {
    ServiceDef {
        id,
        name,
        title,
        what_changes,
        could_break,
        risk,
        target,
    }
}

const CATALOG: &[ServiceDef] = &[
    // Safe: target Disabled.
    def(
        "svc-diagtrack",
        "DiagTrack",
        "Diagnostics and usage data service",
        "Stops Windows from collecting and sending usage and diagnostics data in the background.",
        "Some Microsoft feedback and troubleshooting tools may report less. Nothing you use every day depends on it.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-fax",
        "Fax",
        "Fax",
        "Turns off the built-in fax service.",
        "You can no longer send or receive faxes from this PC. Almost nobody uses this.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-retaildemo",
        "RetailDemo",
        "Shop demo mode",
        "Turns off the service that runs a PC in shop-window demo mode.",
        "Nothing, unless this PC is used as a shop display.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-mapsbroker",
        "MapsBroker",
        "Downloaded maps",
        "Stops Windows from keeping offline map data up to date.",
        "Apps that use downloaded offline maps may show old maps or none.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-lfsvc",
        "lfsvc",
        "Location service",
        "Turns off the service that tells apps where this PC is.",
        "Apps that use your location may stop getting it, and Find my device may not work.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-phonesvc",
        "PhoneSvc",
        "Phone service",
        "Turns off the service that lets Windows manage phone calls and cellular voice.",
        "Phone-call features in Windows and linked-phone apps may stop working. A normal PC does not use it.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-icssvc",
        "icssvc",
        "Mobile hotspot",
        "Turns off the service behind the Windows mobile hotspot.",
        "You can no longer share this PC's internet connection as a hotspot.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-walletservice",
        "WalletService",
        "Wallet service",
        "Turns off the background service used by wallet and payment apps.",
        "Apps that use the Windows wallet may stop working. Most PCs have none.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-semgrsvc",
        "SEMgrSvc",
        "Payments and NFC manager",
        "Turns off the service that manages payments and near-field (NFC) chips.",
        "Tap-to-pay and NFC features may stop working. Most desktops have no NFC.",
        Risk::Safe,
        StartType::Disabled,
    ),
    def(
        "svc-wecsvc",
        "Wecsvc",
        "Event collector",
        "Turns off the service that collects event logs from other computers.",
        "Collecting event logs from other PCs stops working. Home PCs do not use it.",
        Risk::Safe,
        StartType::Disabled,
    ),
    // Careful: target Manual only.
    def(
        "svc-sysmain",
        "SysMain",
        "Superfetch (SysMain)",
        "Sets it to Manual so Windows no longer preloads apps at startup. Leaving it on is usually best; benchmarks show little gain.",
        "Apps you use often may open a little slower, especially on a hard disk.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-spooler",
        "Spooler",
        "Print spooler",
        "Sets it to Manual so printing is not ready at startup.",
        "Stops ALL printing, including Print to PDF, until the service is started again.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-pcasvc",
        "PcaSvc",
        "Program compatibility assistant",
        "Sets it to Manual so Windows stops watching for old programs that have problems.",
        "You may lose the helpful prompts for older programs that do not run well.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-trkwks",
        "TrkWks",
        "Link tracking",
        "Sets it to Manual so Windows stops tracking moved files for shortcuts and links.",
        "Shortcuts and links to files moved on a network may fail to find them.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-wersvc",
        "WerSvc",
        "Error reporting",
        "Sets it to Manual so Windows sends fewer crash reports to Microsoft.",
        "Crash details in Reliability Monitor may be missing, which makes problems harder to diagnose.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-xblauthmanager",
        "XblAuthManager",
        "Xbox sign-in",
        "Sets it to Manual so Xbox sign-in is not ready at startup.",
        "Xbox sign-in, Game Bar and some Store games may not work until it starts again.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-xblgamesave",
        "XblGameSave",
        "Xbox game saves",
        "Sets it to Manual so Xbox cloud game saves are not ready at startup.",
        "Cloud saves for Xbox and Store games may not sync.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-xboxnetapisvc",
        "XboxNetApiSvc",
        "Xbox network",
        "Sets it to Manual so the Xbox Live networking service is not ready at startup.",
        "Online features in Xbox and some Store games may not work.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-xboxgipsvc",
        "XboxGipSvc",
        "Xbox accessories",
        "Sets it to Manual so the Xbox accessory service is not ready at startup.",
        "Xbox controllers and headsets may not be recognised in some games.",
        Risk::Careful,
        StartType::Manual,
    ),
    def(
        "svc-wsaifabricsvc",
        "WSAIFabricSvc",
        "Windows AI service",
        "Sets it to Manual so the Windows AI background service is not ready at startup. Only shown if it is on this PC.",
        "Windows features that use on-device AI may be slower to start or unavailable.",
        Risk::Careful,
        StartType::Manual,
    ),
];

/// All known services (Safe first, then Careful).
pub fn catalog() -> &'static [ServiceDef] {
    CATALOG
}

const NEVER_OFFERED: &[&str] = &[
    "WinDefend",
    "WdNisSvc",
    "Sense",
    "SecurityHealthService",
    "wscsvc",
    "WdNisDrv",
    "SgrmBroker",
    "wuauserv",
    "UsoSvc",
    "WaaSMedicSvc",
    "TrustedInstaller",
    "mpssvc",
    "BFE",
    "StorSvc",
    "Themes",
    "WpnService",
    "DcomLaunch",
    "RpcSs",
    "PlugPlay",
    "Power",
    "ProfSvc",
    "EventLog",
    "CryptSvc",
    "gpsvc",
    "LSM",
    "StateRepository",
    "CoreMessagingRegistrar",
    "BITS",
    "DoSvc",
    "WSearch",
    "bthserv",
    "SharedAccess",
    "SCardSvr",
    "ScDeviceEnum",
    "TabletInputService",
    "WbioSrvc",
    "DPS",
    "WdiServiceHost",
    "WdiSystemHost",
    "DusmSvc",
    "edgeupdate",
    "edgeupdatem",
    "GamingServices",
    "GamingServicesNet",
    "AppXSvc",
    "ClipSVC",
    "InstallService",
    "WSService",
    "LicenseManager",
];

/// Prefixes that are never offered (Defender family, Edge updaters, Appx plumbing, gaming
/// services, push notification user services).
const NEVER_OFFERED_PREFIXES: &[&str] = &["wd", "appx", "gamingservices", "edgeupdate", "wpn"];

/// Names that are never offered or changed, whatever is asked.
pub fn is_never_offered(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return true;
    }
    // Per-user template services look like `Name_1a2b3`: never touched.
    if lower.contains('_') {
        return true;
    }
    NEVER_OFFERED.iter().any(|n| n.eq_ignore_ascii_case(&lower))
        || NEVER_OFFERED_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// One status per catalog entry, same order.
pub fn statuses(svc: &dyn Services) -> Vec<ServiceStatus> {
    catalog()
        .iter()
        .map(|d| ServiceStatus {
            id: d.id,
            state: svc.state(d.name),
        })
        .collect()
}

fn read_start(svc: &dyn Services, name: &str) -> Result<StartType, String> {
    match svc.state(name) {
        ServiceState::Present { start, .. } => Ok(start),
        ServiceState::NotPresent => Err("That service is not on this PC".to_string()),
        ServiceState::Locked => Err("Protected by Windows".to_string()),
        ServiceState::Unknown(e) => Err(format!("Could not read the service: {e}")),
    }
}

/// Switches a service "off" (to `def.target`). Refuses names that are not in the catalog or never
/// offered, services that are not present, locked or unreadable, an unchanged start type, a start
/// type we never write, and a Disabled target while a dependent service is running. Re-reads the
/// state to confirm; if it did not stick, the previous start type is put back and an error returned.
/// Undo with `undo`, which puts `UndoStep::previous` back.
pub fn set_off(svc: &mut dyn Services, def: &ServiceDef) -> Result<UndoStep, String> {
    let known = catalog()
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(def.name) && d.id == def.id)
        .ok_or_else(|| "That service is not on the list this tool may change".to_string())?;
    if is_never_offered(known.name) {
        return Err("That service is never changed by this tool".to_string());
    }
    let (previous, dependents) = match svc.state(known.name) {
        ServiceState::Present {
            start,
            active_dependents,
            ..
        } => (start, active_dependents),
        ServiceState::NotPresent => return Err("That service is not on this PC".to_string()),
        ServiceState::Locked => return Err("Protected by Windows".to_string()),
        ServiceState::Unknown(e) => return Err(format!("Could not read the service: {e}")),
    };
    if let StartType::Other(_) = previous {
        return Err("This service starts with Windows itself and is left alone".to_string());
    }
    if previous == known.target {
        return Err(format!("Already set to {}", known.target.label()));
    }
    if known.target == StartType::Disabled && !dependents.is_empty() {
        let shown: Vec<&str> = dependents.iter().take(3).map(String::as_str).collect();
        let more = if dependents.len() > 3 {
            " and others"
        } else {
            ""
        };
        return Err(format!(
            "Other running services need it: {}{}",
            shown.join(", "),
            more
        ));
    }
    svc.set_start(known.name, known.target)?;
    match read_start(svc, known.name) {
        Ok(now) if now == known.target => Ok(UndoStep {
            id: known.id,
            name: known.name,
            previous,
        }),
        other => {
            let _ = svc.set_start(known.name, previous);
            Err(match other {
                Ok(now) => format!(
                    "Windows kept it at {} instead of {}; the old setting was put back",
                    now.label(),
                    known.target.label()
                ),
                Err(e) => {
                    format!("Could not confirm the change ({e}); the old setting was put back")
                }
            })
        }
    }
}

pub fn undo(svc: &mut dyn Services, step: &UndoStep) -> Result<(), String> {
    if is_never_offered(step.name) || !catalog().iter().any(|d| d.name == step.name) {
        return Err("That service is not on the list this tool may change".to_string());
    }
    svc.set_start(step.name, step.previous)?;
    let now = read_start(svc, step.name)?;
    if now == step.previous {
        Ok(())
    } else {
        Err(format!(
            "Windows kept it at {} instead of {}",
            now.label(),
            step.previous.label()
        ))
    }
}

pub fn undo_text(steps: &[UndoStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let title = catalog()
                .iter()
                .find(|d| d.id == s.id)
                .map(|d| d.title)
                .unwrap_or(s.name);
            format!("{title}: back to {}", s.previous.label())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    struct FakeServices {
        map: HashMap<String, ServiceState>,
        write_fails: bool,
        write_does_not_stick: bool,
        writes: Vec<(String, StartType)>,
    }

    impl FakeServices {
        fn with(name: &str, start: StartType) -> Self {
            let mut f = FakeServices::default();
            f.map.insert(name.to_string(), present(start, vec![]));
            f
        }
        fn start_of(&self, name: &str) -> StartType {
            match &self.map[name] {
                ServiceState::Present { start, .. } => *start,
                _ => panic!("not present"),
            }
        }
    }

    fn present(start: StartType, deps: Vec<&str>) -> ServiceState {
        ServiceState::Present {
            start,
            running: true,
            active_dependents: deps.into_iter().map(String::from).collect(),
        }
    }

    impl Services for FakeServices {
        fn state(&self, name: &str) -> ServiceState {
            self.map
                .get(name)
                .cloned()
                .unwrap_or(ServiceState::NotPresent)
        }
        fn set_start(&mut self, name: &str, start: StartType) -> Result<(), String> {
            self.writes.push((name.to_string(), start));
            if self.write_fails {
                return Err(ACCESS_DENIED_TEXT.to_string());
            }
            if self.write_does_not_stick {
                return Ok(());
            }
            if let Some(ServiceState::Present { start: s, .. }) = self.map.get_mut(name) {
                *s = start;
            }
            Ok(())
        }
    }

    fn find(name: &str) -> ServiceDef {
        *catalog().iter().find(|d| d.name == name).unwrap()
    }

    #[test]
    fn catalog_is_well_formed() {
        let c = catalog();
        let ids: HashSet<_> = c.iter().map(|d| d.id).collect();
        let names: HashSet<_> = c.iter().map(|d| d.name).collect();
        assert_eq!(ids.len(), c.len());
        assert_eq!(names.len(), c.len());
        for d in c {
            assert!(!d.title.is_empty() && !d.what_changes.is_empty() && !d.could_break.is_empty());
            assert!(d
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
            match d.risk {
                Risk::Safe => assert_eq!(d.target, StartType::Disabled, "{}", d.name),
                Risk::Careful => assert_eq!(d.target, StartType::Manual, "{}", d.name),
            }
            assert!(!is_never_offered(d.name), "{} is never-offered", d.name);
        }
        assert!(c.iter().filter(|d| d.risk == Risk::Safe).count() >= 10);
        assert!(c.iter().filter(|d| d.risk == Risk::Careful).count() >= 6);
        // Safe first, then Careful.
        let first_careful = c.iter().position(|d| d.risk == Risk::Careful).unwrap();
        assert!(c[first_careful..].iter().all(|d| d.risk == Risk::Careful));
    }

    #[test]
    fn never_offered_matches() {
        assert!(is_never_offered("WinDefend"));
        assert!(is_never_offered("windefend"));
        assert!(is_never_offered("BITS"));
        assert!(is_never_offered("WpnUserService_1a2b3"));
        assert!(is_never_offered("wuauserv"));
        assert!(is_never_offered(""));
        assert!(!is_never_offered("DiagTrack"));
    }

    #[test]
    fn statuses_cover_not_present_and_locked() {
        let mut f = FakeServices::with("DiagTrack", StartType::Auto);
        f.map.insert("Fax".to_string(), ServiceState::Locked);
        let s = statuses(&f);
        assert_eq!(s.len(), catalog().len());
        assert_eq!(s[0].id, "svc-diagtrack");
        assert!(matches!(s[0].state, ServiceState::Present { .. }));
        assert_eq!(s[1].state, ServiceState::Locked);
        assert_eq!(s[2].state, ServiceState::NotPresent);
    }

    #[test]
    fn round_trip_restores_delayed_exactly() {
        let mut f = FakeServices::with("SysMain", StartType::AutoDelayed);
        let step = set_off(&mut f, &find("SysMain")).unwrap();
        assert_eq!(step.previous, StartType::AutoDelayed);
        assert_eq!(f.start_of("SysMain"), StartType::Manual);
        undo(&mut f, &step).unwrap();
        assert_eq!(f.start_of("SysMain"), StartType::AutoDelayed);
        assert_eq!(
            undo_text(&[step]),
            "Superfetch (SysMain): back to Automatic (delayed)"
        );
    }

    #[test]
    fn disabled_round_trip_with_plain_auto() {
        let mut f = FakeServices::with("DiagTrack", StartType::Auto);
        let step = set_off(&mut f, &find("DiagTrack")).unwrap();
        assert_eq!(f.start_of("DiagTrack"), StartType::Disabled);
        undo(&mut f, &step).unwrap();
        assert_eq!(f.start_of("DiagTrack"), StartType::Auto);
    }

    #[test]
    fn refusals() {
        let d = find("DiagTrack");
        // Not present.
        let mut f = FakeServices::default();
        assert!(set_off(&mut f, &d).is_err());
        // Locked.
        f.map.insert("DiagTrack".into(), ServiceState::Locked);
        assert!(set_off(&mut f, &d).is_err());
        // Unknown.
        f.map
            .insert("DiagTrack".into(), ServiceState::Unknown("x".into()));
        assert!(set_off(&mut f, &d).is_err());
        // Already at target.
        let mut f = FakeServices::with("DiagTrack", StartType::Disabled);
        assert!(set_off(&mut f, &d).is_err());
        // Other start type.
        let mut f = FakeServices::with("DiagTrack", StartType::Other(1));
        assert!(set_off(&mut f, &d).is_err());
        // Dependents block a Disabled target, naming at most 3.
        let mut f = FakeServices::default();
        f.map.insert(
            "DiagTrack".into(),
            present(StartType::Auto, vec!["A", "B", "C", "D"]),
        );
        let e = set_off(&mut f, &d).unwrap_err();
        assert!(e.contains("A, B, C") && !e.contains('D'));
        assert!(f.writes.is_empty());
        // Dependents do not block a Manual target.
        let mut f = FakeServices::default();
        f.map
            .insert("SysMain".into(), present(StartType::Auto, vec!["A"]));
        assert!(set_off(&mut f, &find("SysMain")).is_ok());
        // Not in the catalog.
        let fake_def = ServiceDef {
            id: "svc-bits",
            name: "BITS",
            ..d
        };
        let mut f = FakeServices::with("BITS", StartType::Manual);
        assert!(set_off(&mut f, &fake_def).is_err());
        assert!(f.writes.is_empty());
        // Name from the catalog but wrong id is also refused.
        let wrong = ServiceDef { id: "nope", ..d };
        let mut f = FakeServices::with("DiagTrack", StartType::Auto);
        assert!(set_off(&mut f, &wrong).is_err());
    }

    #[test]
    fn write_failure_is_reported() {
        let mut f = FakeServices::with("DiagTrack", StartType::Auto);
        f.write_fails = true;
        assert!(set_off(&mut f, &find("DiagTrack")).is_err());
        assert_eq!(f.start_of("DiagTrack"), StartType::Auto);
    }

    #[test]
    fn read_back_mismatch_restores_previous() {
        let mut f = FakeServices::with("DiagTrack", StartType::Auto);
        f.write_does_not_stick = true;
        assert!(set_off(&mut f, &find("DiagTrack")).is_err());
        assert_eq!(f.start_of("DiagTrack"), StartType::Auto);
        // Two writes: the attempt and the restore.
        assert_eq!(f.writes.last().unwrap().1, StartType::Auto);
        assert_eq!(f.writes.len(), 2);
    }

    #[test]
    fn undo_mismatch_is_an_error() {
        let mut f = FakeServices::with("DiagTrack", StartType::Disabled);
        f.write_does_not_stick = true;
        let step = UndoStep {
            id: "svc-diagtrack",
            name: "DiagTrack",
            previous: StartType::Auto,
        };
        assert!(undo(&mut f, &step).is_err());
    }

    #[test]
    fn undo_text_falls_back_to_name() {
        let step = UndoStep {
            id: "unknown-id",
            name: "Foo",
            previous: StartType::Auto,
        };
        assert_eq!(undo_text(&[step]), "Foo: back to Automatic");
    }

    /// Read-only: prints the real state of every catalog service. Never writes.
    #[test]
    #[ignore]
    fn real_states_smoke() {
        let s = statuses(&WinServices);
        assert_eq!(s.len(), catalog().len());
        for st in &s {
            match &st.state {
                ServiceState::Present {
                    start,
                    running,
                    active_dependents,
                } => println!(
                    "{}: {} running={} dependents={:?}",
                    st.id,
                    start.label(),
                    running,
                    active_dependents
                ),
                other => println!("{}: {:?}", st.id, other),
            }
        }
    }
}
