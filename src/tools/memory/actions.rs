//! Safe memory actions engine: lower the memory priority of idle background apps, then trim
//! individual idle apps. Every action verifies the target (pid + start time + image name)
//! immediately before acting, and everything lives in memory (nothing is written to disk).
//!
//! Deliberately absent: system-wide "empty all working sets". Only per-app, per-pid actions.
//!
//! Windows access is behind [`ProcessOps`]; [`WindowsOps`] is the real implementation and the
//! tests use an in-memory fake. Eligibility ([`check_eligibility`]) and [`plan`] are pure.
//!
//! Integration entry points: [`plan`], [`run`], [`UndoLog::restore_all`], [`Cooldowns`],
//! [`foreground_pid`], [`WindowsOps`].

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{HRESULT, PWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, FILETIME, HANDLE};
use windows::Win32::System::ProcessStatus::{
    EmptyWorkingSet, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows::Win32::System::Threading::{
    GetProcessInformation, GetProcessTimes, OpenProcess, ProcessMemoryPriority,
    QueryFullProcessImageNameW, SetProcessInformation, MEMORY_PRIORITY,
    MEMORY_PRIORITY_INFORMATION, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::trim::{skip_reason, Trimmed};
use crate::win::Handle;

/// Memory priority applied to idle background apps (MEMORY_PRIORITY_LOW).
pub const LOW_PRIORITY: u32 = 2;
/// Windows default memory priority.
#[cfg(test)]
pub const NORMAL_PRIORITY: u32 = windows::Win32::System::Threading::MEMORY_PRIORITY_NORMAL.0;

const MB: u64 = 1024 * 1024;

/// Security software: never touched. Lowercase image names.
const SECURITY: &[&str] = &[
    "msmpeng.exe",
    "nissrv.exe",
    "mpdefendercoreservice.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "sense.exe",
    "senseir.exe",
    "mssense.exe",
    "avastsvc.exe",
    "avgsvc.exe",
    "avp.exe",
    "bdservicehost.exe",
    "mbamservice.exe",
    "mbam.exe",
    "ekrn.exe",
    "sophosfilescanner.exe",
    "csfalconservice.exe",
    "csfalconcontainer.exe",
];

/// More Windows shell hosts that are not worth touching (on top of `trim::skip_reason`).
const EXTRA_PROTECTED: &[&str] = &[
    "svchost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "runtimebroker.exe",
    "taskhostw.exe",
];

/// Known game launchers and anti-cheat services. Matched against the app and its parent.
const LAUNCHERS: &[&str] = &[
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "battle.net.exe",
    "eadesktop.exe",
    "origin.exe",
    "upc.exe",
    "ubisoftconnect.exe",
    "galaxyclient.exe",
    "riotclientservices.exe",
    "easyanticheat.exe",
    "beservice.exe",
    "vgc.exe",
];

/// Who a process is. A pid alone is not enough: pids get reused.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    pub pid: u32,
    /// Process creation time as a FILETIME value.
    pub created: u64,
    /// Image file name such as `chrome.exe`, compared case-insensitively.
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    /// Protected process or one owned by another user.
    AccessDenied,
    /// The pid now belongs to another process (or it exited).
    IdentityChanged,
    Failed,
}

/// Everything the filter needs to know about one running app.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub identity: Identity,
    /// Working set in bytes.
    pub working_set: u64,
    /// Time since the last user input in this app; `None` when unknown (then not eligible).
    pub idle_for: Option<Duration>,
    pub plays_audio: bool,
    pub is_fullscreen: bool,
    /// Image name of the parent process, if known (to catch children of game launchers).
    pub parent_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub own_pid: u32,
    pub foreground_pid: Option<u32>,
    /// Lowercase image names the user excluded.
    pub exclusions: HashSet<String>,
    /// Minimum working set for a trim.
    pub min_trim_bytes: u64,
    /// Minimum working set for a priority change.
    pub min_priority_bytes: u64,
    /// Required time without input.
    pub min_idle: Duration,
    pub trim_cooldown: Duration,
    pub max_trims: usize,
    pub max_priority: usize,
}

impl Policy {
    pub fn new(own_pid: u32, foreground_pid: Option<u32>) -> Self {
        Policy {
            own_pid,
            foreground_pid,
            exclusions: HashSet::new(),
            min_trim_bytes: 200 * MB,
            min_priority_bytes: 50 * MB,
            min_idle: Duration::from_secs(5 * 60),
            trim_cooldown: Duration::from_secs(45 * 60),
            max_trims: 8,
            max_priority: 32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    Own,
    System,
    Excluded,
    Security,
    Foreground,
    /// Input recently, or idle time unknown.
    NotIdle,
    PlaysAudio,
    Fullscreen,
    Launcher,
    TooSmall,
    Cooldown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LowerPriority,
    Trim,
}

/// Per-app trim cooldown table. The clock is injected: callers pass `now`.
#[derive(Debug, Default)]
pub struct Cooldowns {
    last: HashMap<(u32, u64), Instant>,
}

impl Cooldowns {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, id: &Identity, now: Instant) {
        self.last.insert((id.pid, id.created), now);
    }

    pub fn is_cooling(&self, id: &Identity, now: Instant, window: Duration) -> bool {
        self.last
            .get(&(id.pid, id.created))
            .is_some_and(|t| now.saturating_duration_since(*t) < window)
    }

    /// Drops entries older than `window`.
    pub fn prune(&mut self, now: Instant, window: Duration) {
        self.last
            .retain(|_, t| now.saturating_duration_since(*t) < window);
    }
}

/// Why this app must be left alone for `kind`, or `Ok(())` if it may be acted on. Pure.
pub fn check_eligibility(
    c: &Candidate,
    kind: Kind,
    policy: &Policy,
    cooldowns: &Cooldowns,
    now: Instant,
) -> Result<(), Skip> {
    let name = c.identity.name.to_lowercase();
    if c.identity.pid == policy.own_pid {
        return Err(Skip::Own);
    }
    if skip_reason(c.identity.pid, &name, policy.own_pid, &HashSet::new()).is_some()
        || EXTRA_PROTECTED.contains(&name.as_str())
    {
        return Err(Skip::System);
    }
    if policy.exclusions.contains(&name) {
        return Err(Skip::Excluded);
    }
    if SECURITY.contains(&name.as_str()) {
        return Err(Skip::Security);
    }
    if policy.foreground_pid == Some(c.identity.pid) {
        return Err(Skip::Foreground);
    }
    if c.is_fullscreen {
        return Err(Skip::Fullscreen);
    }
    let parent_is_launcher = c
        .parent_name
        .as_ref()
        .is_some_and(|p| LAUNCHERS.contains(&p.to_lowercase().as_str()));
    if LAUNCHERS.contains(&name.as_str()) || parent_is_launcher {
        return Err(Skip::Launcher);
    }
    if c.plays_audio {
        return Err(Skip::PlaysAudio);
    }
    match c.idle_for {
        Some(idle) if idle >= policy.min_idle => {}
        _ => return Err(Skip::NotIdle),
    }
    let min = match kind {
        Kind::LowerPriority => policy.min_priority_bytes,
        Kind::Trim => policy.min_trim_bytes,
    };
    if c.working_set < min {
        return Err(Skip::TooSmall);
    }
    if kind == Kind::Trim && cooldowns.is_cooling(&c.identity, now, policy.trim_cooldown) {
        return Err(Skip::Cooldown);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub kind: Kind,
    pub identity: Identity,
    /// Working set when planned, in bytes.
    pub working_set: u64,
}

/// The ordered safe actions: priority changes first, then trims of the largest eligible
/// apps, both capped by the policy. Duplicate pids are acted on once.
pub fn plan(
    candidates: &[Candidate],
    policy: &Policy,
    cooldowns: &Cooldowns,
    now: Instant,
) -> Vec<Action> {
    let mut seen = HashSet::new();
    let mut unique: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| seen.insert(c.identity.pid))
        .collect();
    // Largest first; pid breaks ties so the plan is deterministic.
    unique.sort_by(|a, b| {
        b.working_set
            .cmp(&a.working_set)
            .then(a.identity.pid.cmp(&b.identity.pid))
    });

    let pick = |kind: Kind, cap: usize| -> Vec<Action> {
        unique
            .iter()
            .filter(|c| check_eligibility(c, kind, policy, cooldowns, now).is_ok())
            .take(cap)
            .map(|c| Action {
                kind,
                identity: c.identity.clone(),
                working_set: c.working_set,
            })
            .collect()
    };
    let mut actions = pick(Kind::LowerPriority, policy.max_priority);
    actions.extend(pick(Kind::Trim, policy.max_trims));
    actions
}

/// Access to other processes. Implementations must verify the identity right before acting
/// and return [`ActionError::IdentityChanged`] on a mismatch.
pub trait ProcessOps {
    fn memory_priority(&self, id: &Identity) -> Result<u32, ActionError>;
    fn set_memory_priority(&self, id: &Identity, value: u32) -> Result<(), ActionError>;
    fn trim(&self, id: &Identity) -> Result<Trimmed, ActionError>;
}

/// What was changed, so it can be undone. In memory only.
#[derive(Debug, Default)]
pub struct UndoLog {
    entries: Vec<(Identity, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restored {
    pub identity: Identity,
    pub result: Result<(), ActionError>,
}

impl UndoLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remembers the value before our first change; later changes keep the original.
    fn remember(&mut self, id: &Identity, previous: u32) {
        if !self.entries.iter().any(|(i, _)| i == id) {
            self.entries.push((id.clone(), previous));
        }
    }

    /// Puts every remembered process back to the value it had before (normally 5, NORMAL),
    /// but only if pid, start time and image name still match. Empties the log.
    pub fn restore_all(&mut self, ops: &dyn ProcessOps) -> Vec<Restored> {
        std::mem::take(&mut self.entries)
            .into_iter()
            .map(|(identity, previous)| {
                let result = ops.set_memory_priority(&identity, previous);
                Restored { identity, result }
            })
            .collect()
    }
}

/// What happened to one app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionResult {
    pub identity: Identity,
    pub kind: Kind,
    /// Working set bytes before and after (trim only).
    pub before: Option<u64>,
    pub after: Option<u64>,
    /// Memory priority before the change (priority only).
    pub previous_priority: Option<u32>,
    pub error: Option<ActionError>,
}

impl ActionResult {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }

    #[cfg(test)]
    pub fn freed(&self) -> u64 {
        match (self.before, self.after) {
            (Some(b), Some(a)) => b.saturating_sub(a),
            _ => 0,
        }
    }
}

/// Executes the plan in order. Errors are per app and never stop the run. Successful priority
/// changes go into `undo`; successful trims start their cooldown.
pub fn run(
    plan: &[Action],
    ops: &dyn ProcessOps,
    undo: &mut UndoLog,
    cooldowns: &mut Cooldowns,
    now: Instant,
) -> Vec<ActionResult> {
    plan.iter()
        .map(|a| {
            let mut r = ActionResult {
                identity: a.identity.clone(),
                kind: a.kind,
                before: None,
                after: None,
                previous_priority: None,
                error: None,
            };
            match a.kind {
                Kind::LowerPriority => {
                    let outcome = ops.memory_priority(&a.identity).and_then(|prev| {
                        ops.set_memory_priority(&a.identity, LOW_PRIORITY)?;
                        Ok(prev)
                    });
                    match outcome {
                        Ok(prev) => {
                            undo.remember(&a.identity, prev);
                            r.previous_priority = Some(prev);
                        }
                        Err(e) => r.error = Some(e),
                    }
                }
                Kind::Trim => match ops.trim(&a.identity) {
                    Ok(t) => {
                        cooldowns.record(&a.identity, now);
                        r.before = Some(t.before);
                        r.after = Some(t.after);
                    }
                    Err(e) => r.error = Some(e),
                },
            }
            r
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// Real implementation
// ---------------------------------------------------------------------------------------

/// Pid of the window the user is working in, or `None` if there is none.
pub fn foreground_pid() -> Option<u32> {
    // SAFETY: plain FFI calls; the out-parameter is a valid u32 for the call's duration.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

/// Whether the app has an active audio session.
/// Stub: real detection needs the COM audio session APIs (IAudioSessionManager2), which this
/// slice does not pull in. Returns `false`, so callers must also rely on other signals.
pub fn plays_audio(_pid: u32) -> bool {
    false
}

fn classify(e: &windows::core::Error) -> ActionError {
    if e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        ActionError::AccessDenied
    } else {
        ActionError::Failed
    }
}

fn filetime(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

fn creation_time(handle: HANDLE) -> windows::core::Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: the handle has query access and all four outputs are valid FILETIME buffers.
    unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }?;
    Ok(filetime(created))
}

/// Identity of a running process (pid, creation time, given image name), or `None` if it cannot
/// be opened (protected, other user) or has no creation time.
pub fn identity_of(pid: u32, name: &str) -> Option<Identity> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map(Handle)
        .ok()?;
    let created = creation_time(handle.0).ok()?;
    Some(Identity {
        pid,
        created,
        name: name.to_string(),
    })
}

/// Opens the process and verifies start time and image name before returning the handle.
fn open_verified(id: &Identity, access: PROCESS_ACCESS_RIGHTS) -> Result<Handle, ActionError> {
    // SAFETY: plain FFI call; the returned handle is wrapped for closing right away.
    let handle = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, id.pid) }
        .map(Handle)
        .map_err(|e| classify(&e))?;
    if creation_time(handle.0).ok() != Some(id.created) {
        return Err(ActionError::IdentityChanged);
    }
    let mut image = vec![0u16; 32_768];
    let mut len = image.len() as u32;
    // SAFETY: the handle has query access and the buffer holds `len` UTF-16 units.
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(image.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(|_| ActionError::IdentityChanged)?;
    let image = String::from_utf16_lossy(&image[..len as usize]);
    let name = image.rsplit('\\').next().unwrap_or_default();
    if !name.eq_ignore_ascii_case(&id.name)
        || skip_reason(id.pid, name, std::process::id(), &HashSet::new()).is_some()
    {
        return Err(ActionError::IdentityChanged);
    }
    Ok(handle)
}

fn working_set(handle: &Handle) -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    counters.cb = size;
    // SAFETY: `counters` is a valid, correctly sized out-parameter for the call's duration.
    unsafe { GetProcessMemoryInfo(handle.0, &mut counters, size) }.ok()?;
    Some(counters.WorkingSetSize as u64)
}

/// Real Windows implementation. Needs no admin rights for the user's own processes.
pub struct WindowsOps;

impl ProcessOps for WindowsOps {
    fn memory_priority(&self, id: &Identity) -> Result<u32, ActionError> {
        let handle = open_verified(id, PROCESS_SET_INFORMATION)?;
        let mut info = MEMORY_PRIORITY_INFORMATION::default();
        // SAFETY: `info` is a valid MEMORY_PRIORITY_INFORMATION of the size passed; the handle
        // has query access.
        unsafe {
            GetProcessInformation(
                handle.0,
                ProcessMemoryPriority,
                &mut info as *mut _ as *mut c_void,
                size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
            )
        }
        .map_err(|e| classify(&e))?;
        Ok(info.MemoryPriority.0)
    }

    fn set_memory_priority(&self, id: &Identity, value: u32) -> Result<(), ActionError> {
        let handle = open_verified(id, PROCESS_SET_INFORMATION)?;
        let info = MEMORY_PRIORITY_INFORMATION {
            MemoryPriority: MEMORY_PRIORITY(value),
        };
        // SAFETY: `info` is a valid MEMORY_PRIORITY_INFORMATION of the size passed; the handle
        // was opened with PROCESS_SET_INFORMATION.
        unsafe {
            SetProcessInformation(
                handle.0,
                ProcessMemoryPriority,
                &info as *const _ as *const c_void,
                size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
            )
        }
        .map_err(|e| classify(&e))
    }

    fn trim(&self, id: &Identity) -> Result<Trimmed, ActionError> {
        let handle = open_verified(id, PROCESS_SET_QUOTA)?;
        let before = working_set(&handle).ok_or(ActionError::Failed)?;
        // SAFETY: the handle is valid and was opened with PROCESS_SET_QUOTA.
        unsafe { EmptyWorkingSet(handle.0) }.map_err(|e| classify(&e))?;
        let after = working_set(&handle).ok_or(ActionError::Failed)?;
        Ok(Trimmed { before, after })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn id(pid: u32, name: &str) -> Identity {
        Identity {
            pid,
            created: 1000 + u64::from(pid),
            name: name.to_string(),
        }
    }

    fn cand(pid: u32, name: &str, mb: u64) -> Candidate {
        Candidate {
            identity: id(pid, name),
            working_set: mb * MB,
            idle_for: Some(Duration::from_secs(3600)),
            plays_audio: false,
            is_fullscreen: false,
            parent_name: None,
        }
    }

    fn policy() -> Policy {
        Policy::new(1, Some(2))
    }

    fn check(c: &Candidate, kind: Kind) -> Result<(), Skip> {
        check_eligibility(c, kind, &policy(), &Cooldowns::new(), Instant::now())
    }

    struct FakeProc {
        created: u64,
        name: String,
        priority: u32,
        ws: u64,
    }

    #[derive(Default)]
    struct Fake {
        procs: RefCell<HashMap<u32, FakeProc>>,
        errors: RefCell<HashMap<u32, ActionError>>,
        trims: RefCell<Vec<u32>>,
    }

    impl Fake {
        fn add(&self, i: &Identity, ws: u64) {
            self.procs.borrow_mut().insert(
                i.pid,
                FakeProc {
                    created: i.created,
                    name: i.name.clone(),
                    priority: 5,
                    ws,
                },
            );
        }
        fn priority(&self, pid: u32) -> u32 {
            self.procs.borrow()[&pid].priority
        }
        fn verify(&self, i: &Identity) -> Result<(), ActionError> {
            if let Some(e) = self.errors.borrow().get(&i.pid) {
                return Err(*e);
            }
            match self.procs.borrow().get(&i.pid) {
                Some(p) if p.created == i.created && p.name.eq_ignore_ascii_case(&i.name) => Ok(()),
                _ => Err(ActionError::IdentityChanged),
            }
        }
    }

    impl ProcessOps for Fake {
        fn memory_priority(&self, i: &Identity) -> Result<u32, ActionError> {
            self.verify(i)?;
            Ok(self.procs.borrow()[&i.pid].priority)
        }
        fn set_memory_priority(&self, i: &Identity, v: u32) -> Result<(), ActionError> {
            self.verify(i)?;
            self.procs.borrow_mut().get_mut(&i.pid).unwrap().priority = v;
            Ok(())
        }
        fn trim(&self, i: &Identity) -> Result<Trimmed, ActionError> {
            self.verify(i)?;
            self.trims.borrow_mut().push(i.pid);
            let mut procs = self.procs.borrow_mut();
            let p = procs.get_mut(&i.pid).unwrap();
            let before = p.ws;
            p.ws /= 10;
            Ok(Trimmed {
                before,
                after: p.ws,
            })
        }
    }

    #[test]
    fn plain_idle_big_app_is_eligible() {
        let c = cand(10, "app.exe", 500);
        assert_eq!(check(&c, Kind::Trim), Ok(()));
        assert_eq!(check(&c, Kind::LowerPriority), Ok(()));
    }

    #[test]
    fn skips_own_foreground_and_excluded() {
        assert_eq!(
            check(&cand(1, "toolboks.exe", 500), Kind::Trim),
            Err(Skip::Own)
        );
        assert_eq!(
            check(&cand(2, "app.exe", 500), Kind::Trim),
            Err(Skip::Foreground)
        );
        let mut p = policy();
        p.exclusions.insert("app.exe".into());
        let r = check_eligibility(
            &cand(10, "App.exe", 500),
            Kind::Trim,
            &p,
            &Cooldowns::new(),
            Instant::now(),
        );
        assert_eq!(r, Err(Skip::Excluded));
    }

    #[test]
    fn skips_system_security_and_launchers() {
        assert_eq!(
            check(&cand(8, "Explorer.EXE", 500), Kind::Trim),
            Err(Skip::System)
        );
        assert_eq!(
            check(&cand(8, "svchost.exe", 500), Kind::Trim),
            Err(Skip::System)
        );
        assert_eq!(
            check(&cand(8, "MsMpEng.exe", 500), Kind::Trim),
            Err(Skip::Security)
        );
        assert_eq!(
            check(&cand(8, "steam.exe", 500), Kind::Trim),
            Err(Skip::Launcher)
        );
        let mut child = cand(8, "game.exe", 500);
        child.parent_name = Some("EpicGamesLauncher.exe".into());
        assert_eq!(check(&child, Kind::Trim), Err(Skip::Launcher));
    }

    #[test]
    fn skips_audio_fullscreen_and_recent_or_unknown_input() {
        let mut c = cand(10, "a.exe", 500);
        c.plays_audio = true;
        assert_eq!(check(&c, Kind::Trim), Err(Skip::PlaysAudio));
        let mut c = cand(10, "a.exe", 500);
        c.is_fullscreen = true;
        assert_eq!(check(&c, Kind::Trim), Err(Skip::Fullscreen));
        let mut c = cand(10, "a.exe", 500);
        c.idle_for = Some(Duration::from_secs(30));
        assert_eq!(check(&c, Kind::Trim), Err(Skip::NotIdle));
        c.idle_for = None;
        assert_eq!(check(&c, Kind::Trim), Err(Skip::NotIdle));
    }

    #[test]
    fn minimum_working_set_differs_by_kind() {
        let c = cand(10, "a.exe", 100);
        assert_eq!(check(&c, Kind::Trim), Err(Skip::TooSmall));
        assert_eq!(check(&c, Kind::LowerPriority), Ok(()));
        assert_eq!(
            check(&cand(10, "a.exe", 10), Kind::LowerPriority),
            Err(Skip::TooSmall)
        );
        assert_eq!(check(&cand(10, "a.exe", 200), Kind::Trim), Ok(()));
    }

    #[test]
    fn cooldown_blocks_trim_for_45_minutes_then_expires() {
        let base = Instant::now();
        let c = cand(10, "a.exe", 500);
        let mut cd = Cooldowns::new();
        cd.record(&c.identity, base);
        let p = policy();
        let at =
            |m: u64| check_eligibility(&c, Kind::Trim, &p, &cd, base + Duration::from_secs(m * 60));
        assert_eq!(at(44), Err(Skip::Cooldown));
        assert_eq!(at(45), Ok(()));
        // Priority is not subject to the trim cooldown.
        assert_eq!(
            check_eligibility(&c, Kind::LowerPriority, &p, &cd, base),
            Ok(())
        );
        // A reused pid (different start time) does not inherit the cooldown.
        let mut other = c.clone();
        other.identity.created += 1;
        assert_eq!(check_eligibility(&other, Kind::Trim, &p, &cd, base), Ok(()));
        cd.prune(base + Duration::from_secs(46 * 60), p.trim_cooldown);
        assert!(cd.last.is_empty());
    }

    #[test]
    fn plan_orders_priority_first_and_caps_trims() {
        let cands: Vec<Candidate> = (10..30u32)
            .map(|n| cand(n * 4, "a.exe", 300 + u64::from(n)))
            .collect();
        let actions = plan(&cands, &policy(), &Cooldowns::new(), Instant::now());
        let prio = actions
            .iter()
            .filter(|a| a.kind == Kind::LowerPriority)
            .count();
        let trims: Vec<&Action> = actions.iter().filter(|a| a.kind == Kind::Trim).collect();
        assert_eq!(prio, 20);
        assert_eq!(trims.len(), 8);
        let first_trim = actions.iter().position(|a| a.kind == Kind::Trim).unwrap();
        assert!(actions[..first_trim]
            .iter()
            .all(|a| a.kind == Kind::LowerPriority));
        // Largest first.
        assert!(trims[0].working_set >= trims[7].working_set);
        assert_eq!(trims[0].identity.pid, 29 * 4);
    }

    #[test]
    fn plan_caps_priority_and_skips_ineligible_and_duplicates() {
        let mut p = policy();
        p.max_priority = 3;
        let mut cands: Vec<Candidate> = (10..20u32).map(|n| cand(n * 4, "a.exe", 300)).collect();
        cands.push(cand(40, "a.exe", 300)); // duplicate pid
        cands.push(cand(2, "fg.exe", 900)); // foreground
        let actions = plan(&cands, &p, &Cooldowns::new(), Instant::now());
        assert_eq!(
            actions
                .iter()
                .filter(|a| a.kind == Kind::LowerPriority)
                .count(),
            3
        );
        assert!(actions.iter().all(|a| a.identity.pid != 2));
        let trim_pids: HashSet<u32> = actions
            .iter()
            .filter(|a| a.kind == Kind::Trim)
            .map(|a| a.identity.pid)
            .collect();
        assert_eq!(trim_pids.len(), 8);
    }

    #[test]
    fn run_lowers_priority_trims_and_undo_restores() {
        let fake = Fake::default();
        let a = id(40, "a.exe");
        fake.add(&a, 400 * MB);
        let now = Instant::now();
        let actions = plan(&[cand(40, "a.exe", 400)], &policy(), &Cooldowns::new(), now);
        let mut undo = UndoLog::new();
        let mut cd = Cooldowns::new();
        let results = run(&actions, &fake, &mut undo, &mut cd, now);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.ok()));
        assert_eq!(results[0].previous_priority, Some(5));
        assert_eq!(fake.priority(40), LOW_PRIORITY);
        assert_eq!(results[1].before, Some(400 * MB));
        assert_eq!(results[1].after, Some(40 * MB));
        assert_eq!(results[1].freed(), 360 * MB);
        assert_eq!(undo.len(), 1);
        assert!(cd.is_cooling(&a, now, Duration::from_secs(60)));

        let restored = undo.restore_all(&fake);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].result, Ok(()));
        assert_eq!(fake.priority(40), NORMAL_PRIORITY);
        assert!(undo.is_empty());
    }

    #[test]
    fn repeated_lowering_keeps_the_original_priority_for_undo() {
        let fake = Fake::default();
        let a = id(40, "a.exe");
        fake.add(&a, 400 * MB);
        let act = Action {
            kind: Kind::LowerPriority,
            identity: a.clone(),
            working_set: 400 * MB,
        };
        let mut undo = UndoLog::new();
        let mut cd = Cooldowns::new();
        run(
            std::slice::from_ref(&act),
            &fake,
            &mut undo,
            &mut cd,
            Instant::now(),
        );
        run(&[act], &fake, &mut undo, &mut cd, Instant::now());
        assert_eq!(undo.len(), 1);
        undo.restore_all(&fake);
        assert_eq!(fake.priority(40), 5);
    }

    #[test]
    fn identity_mismatch_blocks_actions_and_undo() {
        let fake = Fake::default();
        let a = id(40, "a.exe");
        fake.add(&a, 400 * MB);
        let lower = Action {
            kind: Kind::LowerPriority,
            identity: a.clone(),
            working_set: 0,
        };
        let trim = Action {
            kind: Kind::Trim,
            identity: a.clone(),
            working_set: 0,
        };
        let mut undo = UndoLog::new();
        let mut cd = Cooldowns::new();
        run(&[lower], &fake, &mut undo, &mut cd, Instant::now());
        assert_eq!(fake.priority(40), LOW_PRIORITY);

        // The pid is reused by another process (new start time and name).
        let newcomer = Identity {
            pid: 40,
            created: 9999,
            name: "other.exe".into(),
        };
        fake.add(&newcomer, 400 * MB);
        let restored = undo.restore_all(&fake);
        assert_eq!(restored[0].result, Err(ActionError::IdentityChanged));
        assert_eq!(fake.priority(40), 5, "newcomer must stay untouched");

        let results = run(&[trim], &fake, &mut undo, &mut cd, Instant::now());
        assert_eq!(results[0].error, Some(ActionError::IdentityChanged));
        assert!(fake.trims.borrow().is_empty());
        assert!(!cd.is_cooling(&a, Instant::now(), Duration::from_secs(60)));
    }

    #[test]
    fn same_pid_and_start_but_other_name_is_a_mismatch() {
        let fake = Fake::default();
        let a = id(40, "a.exe");
        fake.add(&a, MB);
        let wrong = Identity {
            name: "b.exe".into(),
            ..a
        };
        assert_eq!(fake.trim(&wrong), Err(ActionError::IdentityChanged));
    }

    #[test]
    fn errors_are_per_app_and_do_not_stop_the_run() {
        let fake = Fake::default();
        let (a, b) = (id(40, "a.exe"), id(44, "b.exe"));
        fake.add(&a, 400 * MB);
        fake.add(&b, 300 * MB);
        fake.errors
            .borrow_mut()
            .insert(40, ActionError::AccessDenied);
        let now = Instant::now();
        let actions = plan(
            &[cand(40, "a.exe", 400), cand(44, "b.exe", 300)],
            &policy(),
            &Cooldowns::new(),
            now,
        );
        let mut undo = UndoLog::new();
        let mut cd = Cooldowns::new();
        let results = run(&actions, &fake, &mut undo, &mut cd, now);
        assert_eq!(results.len(), 4);
        let denied: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
        assert_eq!(denied.len(), 2);
        assert!(denied
            .iter()
            .all(|r| r.error == Some(ActionError::AccessDenied)));
        assert_eq!(undo.len(), 1);
        assert!(!cd.is_cooling(&a, now, Duration::from_secs(60)));
        assert!(cd.is_cooling(&b, now, Duration::from_secs(60)));
    }

    #[test]
    fn real_priority_round_trip_on_own_child() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 > nul"])
            .spawn()
            .expect("spawn child");
        let pid = child.id();
        // SAFETY: plain FFI call; the handle is wrapped for closing.
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map(Handle);
        let identity = handle.ok().and_then(|h| {
            Some(Identity {
                pid,
                created: creation_time(h.0).ok()?,
                name: "cmd.exe".into(),
            })
        });
        let outcome = identity.map(|i| {
            let ops = WindowsOps;
            let prev = ops.memory_priority(&i);
            let set = ops.set_memory_priority(&i, LOW_PRIORITY);
            let now = ops.memory_priority(&i);
            (prev, set, now)
        });
        let _ = child.kill();
        let _ = child.wait();
        let (prev, set, now) = outcome.expect("identity of child");
        assert!(prev.is_ok() && set.is_ok(), "{prev:?} {set:?}");
        assert_eq!(now, Ok(LOW_PRIORITY));
    }
}
