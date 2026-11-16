//! Add-on supervisor (#80): starts, watches and stops the processes of native add-ons.
//!
//! - An add-on starts when its page is opened ([`Supervisor::open`]) or, for a background
//!   add-on, when the host asks ([`Supervisor::start_background`]).
//! - Before every start the exe is hashed and held open until the process runs. Normal starts
//!   use the install record; privileged starts require a current catalog entry.
//!   Installed version folders refuse unexpected files or redirected paths.
//! - Each launch gets two named pipes that only the current user can open and a random token
//!   the add-on must present (see `pipe`), then the protocol handshake.
//! - Every add-on process is in one job object that ends with the host, also after a crash of
//!   the host (see `process`).
//! - Hidden background add-ons suspend; other hidden add-ons exit. A crash restarts at most
//!   [`Timing::restart_max`] times per [`Timing::restart_window`], then stays stopped with a
//!   plain reason until the user restarts it ([`Supervisor::restart`]); [`Supervisor::report`]
//!   is the text for a bug report.
//! - Administrator rights are per operation: a separate helper owns the native confirmation
//!   and runs one action. The add-on drawing process stays unelevated and keeps its connection.
//!
//! Nothing here blocks the UI thread: launches run on their own thread, writes go through a
//! writer thread, and [`Supervisor::tick`] only polls.
//!
//! How a page uses it: call `tick` every pass and apply the events to its `AddonView`:
//! `Connected` → `view.attach(supervisor.take_link(id))`, `Restarting` → `view.begin_restart()`,
//! `Stalled` → `view.set_stalled(..)`, `Stopped` → `view.stop(reason.text(name))`.

// The add-on browser and the pilot add-on call into this in later changes (#86, #88); the
// tests drive it today with a fake add-on exe.
#![allow(dead_code)]

pub(crate) mod admin;
mod pipe;
mod process;
#[cfg(test)]
pub(crate) mod tests;

use crate::addon_view::{AddonLink, SharedSender};
use crate::installer;
use eframe::egui;
use process::{ElevateError, Job, Process, VerifyError};
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use toolboks_addon_proto::pacing::{Heartbeat, HeartbeatMonitor, PING_INTERVAL};
use toolboks_addon_proto::{
    host_handshake, AddonMessage, FrameReader, FrameWriter, Hello, HostMessage, HostRequest,
    ProtocolError,
};

/// What to start. Built by the caller from the installed add-on and its catalog entry.
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub id: String,
    /// The name shown to the user.
    pub name: String,
    /// The exe and the SHA-256 recorded for it at install time. Ignored when
    /// `installed_root` is set: the installer's record is then read before every start.
    pub exe: PathBuf,
    pub sha256: String,
    /// The add-on may request a separate administrator helper for an action.
    pub may_elevate: bool,
    /// Background add-ons keep their process while their page is hidden.
    pub background: bool,
    /// `addons::data_root()` for an add-on the installer placed. The supervisor then follows
    /// the installer's start bookkeeping (`launching`, `started`), so a new version that never
    /// starts rolls back.
    pub installed_root: Option<PathBuf>,
}

/// Limits and waits. The defaults are the real ones; tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// How long a started process gets to connect, present its token and say `Hello`.
    pub connect: Duration,
    /// How long an add-on gets to exit after `Quit` before it is ended.
    pub quit_grace: Duration,
    /// How long a stalled add-on is waited for before it is ended and restarted.
    pub stall_kill: Duration,
    /// At most this many automatic restarts inside `restart_window`.
    pub restart_max: usize,
    pub restart_window: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            quit_grace: Duration::from_secs(2),
            stall_kill: Duration::from_secs(30),
            restart_max: 3,
            restart_window: Duration::from_secs(60),
        }
    }
}

/// Allows a limited number of automatic restarts per time window.
#[derive(Debug)]
pub struct RestartPolicy {
    max: usize,
    window: Duration,
    restarts: VecDeque<Instant>,
}

impl RestartPolicy {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            restarts: VecDeque::new(),
        }
    }

    /// Records a restart at `now` and returns true, or returns false when the limit is used up.
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .restarts
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= self.window)
        {
            self.restarts.pop_front();
        }
        if self.restarts.len() >= self.max {
            return false;
        }
        self.restarts.push_back(now);
        true
    }

    pub fn reset(&mut self) {
        self.restarts.clear();
    }
}

/// Why an add-on is not running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The host stopped it.
    ByHost,
    /// The add-on closed itself.
    Closed,
    /// The exe is not the file that was installed. It was not started.
    WrongFile,
    /// The exe is missing or unreadable.
    Missing(String),
    /// The add-on and the host cannot talk (protocol version, wrong add-on, wrong token).
    Refused(String),
    /// The user said no at the administrator prompt.
    Declined,
    /// It ended unexpectedly more often than the restart limit allows.
    Crashed { exit_code: Option<u32> },
}

impl StopReason {
    /// The plain sentence shown to the user after "<Name> stopped.".
    pub fn text(&self) -> String {
        match self {
            Self::ByHost => "It was closed.".into(),
            Self::Closed => "It closed itself.".into(),
            Self::WrongFile => {
                "Its file was changed after it was installed, so Toolboks did not start it. \
                 Remove the add-on and install it again."
                    .into()
            }
            Self::Missing(_) => {
                "Its file is missing. Remove the add-on and install it again.".into()
            }
            Self::Refused(why) => why.clone(),
            Self::Declined => "It needs administrator rights, and the prompt was declined.".into(),
            Self::Crashed { .. } => {
                "It closed unexpectedly several times in a row. You can restart it, or copy a \
                 report for a bug report."
                    .into()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Starting,
    Running {
        elevated: bool,
    },
    /// Running, but it has not answered for a while.
    Stalled,
    Stopping,
    Stopped(StopReason),
}

/// What changed in a [`Supervisor::tick`].
#[derive(Debug)]
pub enum Event {
    /// The add-on is connected; take its link with [`Supervisor::take_link`].
    Connected {
        id: String,
    },
    /// The add-on ended unexpectedly and is being started again.
    Restarting {
        id: String,
    },
    Stalled {
        id: String,
        stalled: bool,
    },
    Stopped {
        id: String,
        reason: StopReason,
    },
    /// A message for the host's services (preferences, status items, tutorial, host requests).
    Message {
        id: String,
        message: AddonMessage,
    },
}

/// A successful launch, handed from the launch thread to `tick`.
struct Started {
    process: Process,
    reader: FrameReader<File>,
    writer: File,
    hello: Hello,
    admin_capabilities: Vec<String>,
}

enum LaunchError {
    /// Final: starting again would give the same result.
    Stop(StopReason),
    /// Counts as a crash: the restart limit decides.
    Failed(String),
}

enum Then {
    Stay,
    Restart,
}

enum Phase {
    Starting {
        result: mpsc::Receiver<Result<Started, LaunchError>>,
        cancel: Arc<AtomicBool>,
    },
    Running {
        process: Process,
        sender: SharedSender,
        side: mpsc::Receiver<AddonMessage>,
        link: Option<AddonLink>,
        heartbeat: HeartbeatMonitor,
        stalled_since: Option<Instant>,
        said_bye: bool,
    },
    Stopping {
        process: Process,
        deadline: Instant,
        then: Then,
    },
    Stopped(StopReason),
}

const LOG_LINES: usize = 50;

/// How soon a running add-on is looked at again when its pipe has closed but Windows does not
/// report the process as ended yet (nothing else may wake the host, for example for a background
/// add-on, which gets no heartbeat).
pub(crate) const EXIT_POLL: Duration = Duration::from_millis(100);

struct Managed {
    spec: LaunchSpec,
    phase: Phase,
    policy: RestartPolicy,
    /// The page is shown. A hidden add-on is suspended and not pinged.
    visible: bool,
    /// One separate administrator operation, coupled to this drawing session.
    admin: Option<admin::AdminJob>,
    log: VecDeque<String>,
    starts: u32,
    last_exit: Option<u32>,
    addon_version: Option<String>,
}

impl Managed {
    fn note(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log
            .push_back(format!("{} {line}", crate::win::local_time_hms()));
    }
}

/// What a launch needs from the supervisor; cloned into the launch thread.
#[derive(Clone)]
struct Shared {
    job: Arc<Job>,
    ctx: egui::Context,
    timing: Timing,
}

pub struct Supervisor {
    shared: Shared,
    addons: BTreeMap<String, Managed>,
    /// False only in tests that imitate a host crash: no `Quit`, the job just closes.
    orderly: bool,
}

impl Supervisor {
    pub fn new(ctx: &egui::Context) -> std::io::Result<Self> {
        Self::with_timing(ctx, Timing::default())
    }

    pub fn with_timing(ctx: &egui::Context, timing: Timing) -> std::io::Result<Self> {
        Ok(Self {
            shared: Shared {
                job: Arc::new(Job::new()?),
                ctx: ctx.clone(),
                timing,
            },
            addons: BTreeMap::new(),
            orderly: true,
        })
    }

    /// The add-on's page was opened: start it if it is not running, wake it if it is.
    /// An add-on that stopped for a reason the user must see stays stopped.
    pub fn open(&mut self, spec: LaunchSpec, now: Instant) {
        self.ensure(spec, true, now);
    }

    /// Starts a background add-on without showing it.
    pub fn start_background(&mut self, spec: LaunchSpec, now: Instant) {
        self.ensure(spec, false, now);
    }

    fn ensure(&mut self, spec: LaunchSpec, visible: bool, now: Instant) {
        let timing = self.shared.timing;
        let managed = self
            .addons
            .entry(spec.id.clone())
            .or_insert_with(|| Managed {
                spec: spec.clone(),
                phase: Phase::Stopped(StopReason::ByHost),
                policy: RestartPolicy::new(timing.restart_max, timing.restart_window),
                visible,
                admin: None,
                log: VecDeque::new(),
                starts: 0,
                last_exit: None,
                addon_version: None,
            });
        managed.spec = spec;
        let was_visible = managed.visible;
        managed.visible |= visible;
        match &mut managed.phase {
            Phase::Stopped(StopReason::ByHost | StopReason::Closed | StopReason::Declined) => {
                managed.policy.reset();
                start(&self.shared, managed);
            }
            Phase::Running {
                sender, heartbeat, ..
            } if visible && !was_visible => {
                // It was not pinged while hidden; start counting from now.
                *heartbeat = HeartbeatMonitor::new(now);
                send(sender, &HostMessage::Resume);
            }
            Phase::Stopping { then, .. } if visible => *then = Then::Restart,
            _ => {}
        }
    }

    /// A background add-on suspends; every other add-on exits when its page is left.
    /// Called on every tick for each add-on whose page is not on screen, so a background add-on
    /// is told `Suspend` only when it goes from shown to hidden (one that connects hidden is
    /// told on connect). Telling it again every tick would wake it, and the host, each time.
    pub fn hide(&mut self, id: &str) {
        if let Some(managed) = self.addons.get_mut(id) {
            let was_visible = std::mem::replace(&mut managed.visible, false);
            if managed.spec.background {
                if let Phase::Running { sender, .. } = &managed.phase {
                    if was_visible {
                        send(sender, &HostMessage::Suspend);
                    }
                }
            } else {
                begin_stop(&self.shared, managed, Then::Stay, Instant::now());
            }
        }
    }

    /// Stops the add-on: `Quit`, then it is ended if it has not exited after the grace time.
    pub fn stop(&mut self, id: &str, now: Instant) {
        if let Some(managed) = self.addons.get_mut(id) {
            begin_stop(&self.shared, managed, Then::Stay, now);
        }
    }

    /// The user asked for a restart. The restart limit starts fresh.
    pub fn restart(&mut self, id: &str, now: Instant) {
        if let Some(managed) = self.addons.get_mut(id) {
            managed.policy.reset();
            begin_stop(&self.shared, managed, Then::Restart, now);
        }
    }

    /// Forgets a stopped add-on (after removal). A running one is stopped first by the caller.
    pub fn forget(&mut self, id: &str) {
        if matches!(
            self.addons.get(id).map(|m| &m.phase),
            Some(Phase::Stopped(_))
        ) {
            self.addons.remove(id);
        }
    }

    /// The connection for the add-on's view, once after each `Connected` event.
    pub fn take_link(&mut self, id: &str) -> Option<AddonLink> {
        match &mut self.addons.get_mut(id)?.phase {
            Phase::Running { link, .. } => link.take(),
            _ => None,
        }
    }

    pub fn status(&self, id: &str) -> Option<Status> {
        Some(match &self.addons.get(id)?.phase {
            Phase::Starting { .. } => Status::Starting,
            Phase::Running {
                stalled_since: Some(_),
                ..
            } => Status::Stalled,
            Phase::Running { process, .. } => Status::Running {
                elevated: process.elevated,
            },
            Phase::Stopping { .. } => Status::Stopping,
            Phase::Stopped(reason) => Status::Stopped(reason.clone()),
        })
    }

    /// A process of this add-on exists or is being started. The installer refuses an update
    /// while this is true.
    pub fn is_running(&self, id: &str) -> bool {
        self.addons
            .get(id)
            .is_some_and(|m| !matches!(m.phase, Phase::Stopped(_)))
    }

    /// Sends a host message (navigation, preferences, capabilities, tutorial) to a running
    /// add-on. Returns false when it is not running.
    pub fn send(&self, id: &str, message: &HostMessage) -> bool {
        match self.addons.get(id).map(|m| &m.phase) {
            Some(Phase::Running { sender, .. }) => send(sender, message),
            _ => false,
        }
    }

    /// A short plain-text report about the add-on, for the clipboard. Nothing is sent anywhere.
    pub fn report(&self, id: &str) -> Option<String> {
        let managed = self.addons.get(id)?;
        let state = match &managed.phase {
            Phase::Stopped(reason) => format!("stopped: {reason:?}"),
            Phase::Starting { .. } => "starting".into(),
            Phase::Running { .. } => "running".into(),
            Phase::Stopping { .. } => "stopping".into(),
        };
        Some(crate::crash::format_addon_report(
            env!("CARGO_PKG_VERSION"),
            &managed.spec.name,
            &managed.spec.id,
            managed.addon_version.as_deref(),
            &state,
            managed.last_exit,
            managed.starts,
            managed.log.iter().map(String::as_str),
        ))
    }

    /// Copies [`Self::report`] to the clipboard.
    pub fn copy_report(&self, id: &str) {
        if let Some(report) = self.report(id) {
            crate::crash::copy_to_clipboard(&report);
        }
    }

    /// Polls every add-on. Call once per UI pass; it never blocks.
    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        for managed in self.addons.values_mut() {
            tick_one(&self.shared, managed, now, &mut events);
        }
        events
    }

    #[cfg(test)]
    fn pid(&self, id: &str) -> Option<u32> {
        match &self.addons.get(id)?.phase {
            Phase::Running { process, .. } => process.pid(),
            _ => None,
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        if !self.orderly {
            return;
        }
        // Ask every add-on to finish, give them a moment, then the job closes and ends the rest.
        for managed in self.addons.values_mut() {
            managed.admin = None;
            match &managed.phase {
                Phase::Starting { cancel, .. } => cancel.store(true, Ordering::SeqCst),
                Phase::Running { sender, .. } => {
                    send(sender, &HostMessage::Quit);
                }
                _ => {}
            }
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        let running = |m: &Managed| match &m.phase {
            Phase::Running { process, .. } | Phase::Stopping { process, .. } => {
                process.exit_code().is_none()
            }
            _ => false,
        };
        while self.addons.values().any(running) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn send(sender: &SharedSender, message: &HostMessage) -> bool {
    sender
        .lock()
        .is_ok_and(|mut sender| sender.send(message).is_ok())
}

/// Starts the launch thread and moves the add-on to `Starting`.
fn start(shared: &Shared, managed: &mut Managed) {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::channel();
    let spec = managed.spec.clone();
    let (thread_shared, thread_cancel) = (shared.clone(), cancel.clone());
    let spawned = std::thread::Builder::new()
        .name("toolboks-addon-launch".into())
        .spawn(move || {
            let outcome = launch(&thread_shared, &spec, false, &thread_cancel);
            // If the supervisor is gone, the process handle drops here and the job ends it.
            let _ = tx.send(outcome);
            thread_shared.ctx.request_repaint();
        });
    managed.starts += 1;
    managed.phase = match spawned {
        Ok(_) => Phase::Starting { result, cancel },
        Err(e) => Phase::Stopped(StopReason::Refused(format!(
            "Toolboks could not start it: {e}"
        ))),
    };
}

/// Runs on the launch thread: check the file, make the pipes, start the process, wait for it
/// to connect and present its token, then do the protocol handshake.
fn launch(
    shared: &Shared,
    spec: &LaunchSpec,
    elevated: bool,
    cancel: &AtomicBool,
) -> Result<Started, LaunchError> {
    let failed = |e: &dyn std::fmt::Display| LaunchError::Failed(e.to_string());
    let (exe, mut sha256) = match &spec.installed_root {
        Some(root) => {
            // May roll back to the previous version when the new one never started.
            installer::launching(root, &spec.id).map_err(|e| failed(&e))?;
            installer::native_exe(root, &spec.id).ok_or_else(|| {
                LaunchError::Stop(StopReason::Missing("no recorded file hash".into()))
            })?
        }
        None => (spec.exe.clone(), spec.sha256.clone()),
    };
    let refused = |why| LaunchError::Stop(StopReason::Refused(why));
    // A drawing process must never inherit administrator rights from an elevated host.
    if !elevated && crate::elevation::is_elevated() {
        return Err(refused(
            "Reopen Toolboks without administrator rights to open add-ons.".into(),
        ));
    }
    let mut admin_capabilities = Vec::new();
    if elevated {
        let root = spec.installed_root.as_ref().ok_or_else(|| refused(
            "Only installed add-ons that the current catalog lists can start with administrator rights.".into()))?;
        let version = exe
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                refused("The installed version is invalid. Reinstall the add-on.".into())
            })?;
        let entry = crate::catalog::elevated_entry(root, &spec.id, version).map_err(refused)?;
        sha256 = entry.sha256;
        admin_capabilities = entry.capabilities;
    }
    if spec.installed_root.is_some() {
        process::verify_folder(&exe).map_err(|e| {
            refused(format!(
                "The add-on folder contains unexpected files. Reinstall the add-on. {e:?}"
            ))
        })?;
    }
    // Held until the process runs: the checked file cannot be swapped in between.
    let _checked = process::verify(&exe, &sha256).map_err(|e| {
        LaunchError::Stop(match e {
            VerifyError::WrongHash => StopReason::WrongFile,
            VerifyError::Missing(why) => StopReason::Missing(why),
            VerifyError::UnexpectedFile(why) => StopReason::Refused(why),
        })
    })?;

    let pipes = pipe::PipePair::create().map_err(|e| failed(&e))?;
    let mut args = vec![
        "--pipe-in",
        pipes.to_addon_path.as_str(),
        "--pipe-out",
        pipes.from_addon_path.as_str(),
        "--token",
        pipes.token.as_str(),
    ];
    if elevated {
        args.push(toolboks_addon_sdk::admin::HELPER_ARG);
    }
    let process = if elevated && !crate::elevation::is_elevated() {
        process::spawn_elevated(&exe, &args).map_err(|e| match e {
            ElevateError::Declined => LaunchError::Stop(StopReason::Declined),
            ElevateError::Failed(why) => failed(&why),
        })?
    } else {
        process::spawn(&shared.job, &exe, &args).map_err(|e| failed(&e))?
    };

    finish_launch(
        pipes,
        process,
        &spec.id,
        shared.timing.connect,
        cancel,
        admin_capabilities,
    )
}

fn finish_launch(
    pipes: pipe::PipePair,
    process: Process,
    id: &str,
    timeout: Duration,
    cancel: &AtomicBool,
    admin_capabilities: Vec<String>,
) -> Result<Started, LaunchError> {
    let failed = |e: &dyn std::fmt::Display| LaunchError::Failed(e.to_string());
    let deadline = Instant::now() + timeout;
    let connected = pipes.connect(process.pid(), deadline, &mut || {
        if cancel.load(Ordering::SeqCst) {
            Err(pipe::ConnectError::Cancelled)
        } else if process.exit_code().is_some() {
            Err(pipe::ConnectError::Exited)
        } else {
            Ok(())
        }
    });
    let (from_addon, to_addon) = match connected {
        Ok(files) => files,
        Err(e) => {
            process.kill();
            return Err(match e {
                pipe::ConnectError::WrongToken | pipe::ConnectError::WrongClient => {
                    LaunchError::Stop(StopReason::Refused(
                        "It did not identify itself to Toolboks, so it was closed.".into(),
                    ))
                }
                other => failed(&format!("the add-on did not connect: {other:?}")),
            });
        }
    };

    let mut reader = FrameReader::new(from_addon);
    let mut writer = FrameWriter::new(to_addon);
    let ours = Hello::new(id, env!("CARGO_PKG_VERSION"));
    // The handshake read blocks. A watchdog ends the process at the deadline, which breaks
    // the pipe and so ends the read.
    let done = AtomicBool::new(false);
    let hello = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                if Instant::now() >= deadline || cancel.load(Ordering::SeqCst) {
                    process.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let hello = host_handshake(&mut reader, &mut writer, &ours);
        done.store(true, Ordering::SeqCst);
        hello
    });
    match hello {
        Ok(hello) => Ok(Started {
            process,
            reader,
            writer: writer.into_inner(),
            hello,
            admin_capabilities,
        }),
        Err(e) => {
            process.kill();
            Err(match e {
                ProtocolError::VersionMismatch { .. }
                | ProtocolError::WrongAddon { .. }
                | ProtocolError::Refused(_) => {
                    LaunchError::Stop(StopReason::Refused(e.to_string()))
                }
                other => failed(&format!("the handshake failed: {other}")),
            })
        }
    }
}

/// The most chunks waiting for the writer thread. Past this the add-on is not reading, and
/// the send fails instead of blocking the UI thread.
const WRITE_QUEUE: usize = 1024;

/// Hands every write to the writer thread, so a full pipe never blocks the caller.
struct QueuedWriter(mpsc::SyncSender<Vec<u8>>);

impl Write for QueuedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};
        match self.0.try_send(bytes.to_vec()) {
            Ok(()) => Ok(bytes.len()),
            Err(mpsc::TrySendError::Full(_)) => Err(Error::new(
                ErrorKind::WouldBlock,
                "the add-on is not reading",
            )),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Wires a connected process to a link for the view and a side channel for the supervisor.
fn connect(shared: &Shared, managed: &mut Managed, started: Started, now: Instant) -> bool {
    let Started {
        process,
        reader,
        mut writer,
        hello,
        ..
    } = started;
    let (chunks, queue) = mpsc::sync_channel::<Vec<u8>>(WRITE_QUEUE);
    let writer_thread = std::thread::Builder::new()
        .name("toolboks-addon-writer".into())
        .spawn(move || {
            // Ends when every sender is gone; the pipe closes with `writer`.
            for chunk in queue {
                if writer.write_all(&chunk).is_err() {
                    break;
                }
            }
        });
    let (side_tx, side) = mpsc::channel();
    let link = writer_thread.and_then(|_| {
        AddonLink::spawn(
            reader,
            FrameWriter::new(QueuedWriter(chunks)),
            &shared.ctx,
            side_tx,
        )
    });
    let link = match link {
        Ok(link) => link,
        Err(e) => {
            process.kill();
            managed.note(format!("could not start a thread: {e}"));
            return false;
        }
    };
    if let Some(root) = &managed.spec.installed_root {
        // The version has started: it is confirmed and the previous one can go.
        if let Err(e) = installer::started(root, &managed.spec.id) {
            managed.note(format!("could not record the start: {e}"));
        }
    }
    let sender = link.sender();
    if !managed.visible {
        send(&sender, &HostMessage::Suspend);
    }
    managed.addon_version = Some(hello.software_version);
    managed.phase = Phase::Running {
        process,
        sender,
        side,
        link: Some(link),
        heartbeat: HeartbeatMonitor::new(now),
        stalled_since: None,
        said_bye: false,
    };
    true
}

/// Sends `Quit` and waits for the exit in `tick`. With `Then::Restart` the add-on starts
/// again afterwards; one that is not running starts right away.
fn begin_stop(shared: &Shared, managed: &mut Managed, then: Then, now: Instant) {
    managed.admin = None;
    let phase = std::mem::replace(&mut managed.phase, Phase::Stopped(StopReason::ByHost));
    let restart = matches!(then, Then::Restart);
    match phase {
        Phase::Running {
            process, sender, ..
        } => {
            send(&sender, &HostMessage::Quit);
            shared.ctx.request_repaint();
            managed.phase = Phase::Stopping {
                process,
                deadline: now + shared.timing.quit_grace,
                then,
            };
        }
        Phase::Stopping {
            process, deadline, ..
        } => {
            managed.phase = Phase::Stopping {
                process,
                deadline,
                then,
            };
        }
        Phase::Starting { cancel, .. } => {
            // The launch thread ends the process it started when it sees this.
            cancel.store(true, Ordering::SeqCst);
            if restart {
                start(shared, managed);
            }
        }
        Phase::Stopped(reason) => {
            if restart {
                start(shared, managed);
            } else {
                managed.phase = Phase::Stopped(reason);
            }
        }
    }
}

/// The add-on ended without being asked to: restart it within the limit, or give up.
fn crashed(shared: &Shared, managed: &mut Managed, now: Instant, events: &mut Vec<Event>) {
    managed.admin = None;
    let id = managed.spec.id.clone();
    if managed.policy.allow(now) {
        events.push(Event::Restarting { id });
        start(shared, managed);
    } else {
        let reason = StopReason::Crashed {
            exit_code: managed.last_exit,
        };
        managed.phase = Phase::Stopped(reason.clone());
        events.push(Event::Stopped { id, reason });
    }
}

fn stopped(managed: &mut Managed, reason: StopReason, events: &mut Vec<Event>) {
    managed.admin = None;
    managed.phase = Phase::Stopped(reason.clone());
    events.push(Event::Stopped {
        id: managed.spec.id.clone(),
        reason,
    });
}

fn tick_one(shared: &Shared, managed: &mut Managed, now: Instant, events: &mut Vec<Event>) {
    let id = managed.spec.id.clone();
    if let Some(job) = &managed.admin {
        if let Some((id, outcome)) = job.poll() {
            if let Phase::Running { sender, .. } = &managed.phase {
                send(sender, &HostMessage::AdminResult { id, outcome });
            }
            managed.admin = None;
        } else {
            shared.ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    // What to do once the borrow of the phase has ended.
    enum Next {
        Nothing,
        Connect(Started),
        Stop(StopReason),
        Crash(Option<u32>, String),
        Admin(toolboks_addon_proto::AdminRequest),
        Finished,
    }
    let mut next = Next::Nothing;
    let mut notes = Vec::new();
    match &mut managed.phase {
        Phase::Starting { result, .. } => match result.try_recv() {
            Ok(Ok(started)) => next = Next::Connect(started),
            Ok(Err(LaunchError::Stop(reason))) => next = Next::Stop(reason),
            Ok(Err(LaunchError::Failed(why))) => next = Next::Crash(None, why),
            Err(mpsc::TryRecvError::Disconnected) => {
                next = Next::Crash(None, "the launch thread ended".into())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        },
        Phase::Running {
            process,
            sender,
            side,
            heartbeat,
            stalled_since,
            said_bye,
            ..
        } => {
            let mut heard = false;
            let mut closed = false;
            loop {
                let message = match side.try_recv() {
                    Ok(message) => message,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                };
                heard = true;
                match message {
                    AddonMessage::Pong { nonce } => heartbeat.on_pong(nonce, now),
                    AddonMessage::Log { level, line } => notes.push(format!("{level:?}: {line}")),
                    AddonMessage::Bye => *said_bye = true,
                    AddonMessage::HostRequest(HostRequest::AdminOperation(request)) => {
                        if managed.admin.is_some() || matches!(next, Next::Admin(_)) {
                            send(
                                sender,
                                &HostMessage::AdminResult {
                                    id: request.id,
                                    outcome: toolboks_addon_proto::AdminOutcome::Failed {
                                        reason: "Another administrator action is still open."
                                            .into(),
                                    },
                                },
                            );
                        } else {
                            next = Next::Admin(request);
                        }
                    }
                    AddonMessage::HostRequest(HostRequest::RestartElevated { .. }) => {
                        notes.push("Whole-interface elevation is no longer supported. Update the add-on to use an administrator helper.".into());
                    }
                    // A drawing connection can never stand in for the helper's result.
                    AddonMessage::AdminResult { .. } => {}
                    message => events.push(Event::Message {
                        id: id.clone(),
                        message,
                    }),
                }
            }
            if heard {
                heartbeat.on_message(now);
                if stalled_since.take().is_some() {
                    events.push(Event::Stalled {
                        id: id.clone(),
                        stalled: false,
                    });
                }
            }
            if let Some(code) = process.exit_code() {
                next = if *said_bye {
                    Next::Stop(StopReason::Closed)
                } else {
                    Next::Crash(Some(code), format!("it ended with exit code {code}"))
                };
            } else {
                if closed {
                    // The pipe closed first: come back soon to see the process end.
                    shared.ctx.request_repaint_after(EXIT_POLL);
                }
                if managed.visible {
                    match heartbeat.poll(now) {
                        Heartbeat::SendPing(nonce) => {
                            send(sender, &HostMessage::Ping { nonce });
                        }
                        Heartbeat::Stalled { .. } if stalled_since.is_none() => {
                            *stalled_since = Some(now);
                            events.push(Event::Stalled {
                                id: id.clone(),
                                stalled: true,
                            });
                        }
                        _ => {}
                    }
                    if stalled_since.is_some_and(|at| {
                        now.saturating_duration_since(at) >= shared.timing.stall_kill
                    }) {
                        process.kill();
                        next = Next::Crash(None, "it stopped answering and was ended".into());
                    }
                    // Come back for the next ping even when nothing else repaints.
                    shared.ctx.request_repaint_after(PING_INTERVAL);
                }
            }
        }
        Phase::Stopping {
            process, deadline, ..
        } => {
            if process.exit_code().is_some() {
                next = Next::Finished;
            } else if now >= *deadline {
                process.kill();
                next = Next::Finished;
            } else {
                shared.ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        Phase::Stopped(_) => {}
    }
    for note in notes {
        managed.note(note);
    }
    match next {
        Next::Nothing => {}
        Next::Connect(started) => {
            if connect(shared, managed, started, now) {
                events.push(Event::Connected { id });
            } else {
                crashed(shared, managed, now, events);
            }
        }
        Next::Stop(reason) => {
            stopped(managed, reason, events);
        }
        Next::Crash(code, why) => {
            managed.last_exit = code;
            managed.note(why);
            crashed(shared, managed, now, events);
        }
        Next::Admin(request) => {
            if let Phase::Running { sender, .. } = &managed.phase {
                if !managed.spec.may_elevate || request.validate().is_err() {
                    send(
                        sender,
                        &HostMessage::AdminResult {
                            id: request.id,
                            outcome: toolboks_addon_proto::AdminOutcome::Failed {
                                reason: "This administrator action is not allowed.".into(),
                            },
                        },
                    );
                } else {
                    managed.admin = Some(admin::AdminJob::installed(
                        shared.clone(),
                        managed.spec.clone(),
                        request,
                    ));
                }
            }
        }
        Next::Finished => {
            let phase = std::mem::replace(&mut managed.phase, Phase::Stopped(StopReason::ByHost));
            match phase {
                Phase::Stopping {
                    then: Then::Restart,
                    ..
                } => {
                    events.push(Event::Restarting { id });
                    start(shared, managed);
                }
                _ => stopped(managed, StopReason::ByHost, events),
            }
        }
    }
}
