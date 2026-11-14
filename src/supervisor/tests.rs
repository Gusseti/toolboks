//! Supervisor tests against a real child process: `examples/fake_addon.rs`, which cargo builds
//! together with the tests. Each test copies it to a folder of its own.

use super::pipe::{current_user_sid, ConnectError, PipePair};
use super::*;
use std::fs;
use std::io::Read;
use std::path::Path;
use windows::core::PWSTR;
use windows::Win32::Foundation::{HANDLE, HLOCAL, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

const ID: &str = "test.fake-addon";
const PATIENCE: Duration = Duration::from_secs(30);

/// A folder with a copy of the fake add-on as `addon.exe`, set to `mode`.
fn fake(test: &str, mode: &str) -> (PathBuf, LaunchSpec) {
    let built = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("examples")
        .join("fake_addon.exe");
    assert!(
        built.is_file(),
        "{} is missing; run the tests through cargo so the example is built",
        built.display()
    );
    let dir = std::env::temp_dir().join(format!(
        "toolboks-supervisor-{test}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("addon.exe");
    fs::copy(&built, &exe).unwrap();
    fs::write(dir.join("mode.txt"), mode).unwrap();
    let spec = LaunchSpec {
        id: ID.into(),
        name: "Fake add-on".into(),
        sha256: toolboks_catalog::sha256_hex(&fs::read(&exe).unwrap()),
        exe,
        may_elevate: false,
        installed_root: None,
    };
    (dir, spec)
}

fn supervisor(timing: Timing) -> Supervisor {
    Supervisor::with_timing(&egui::Context::default(), timing).unwrap()
}

/// Ticks until `done` says so, and returns every event seen.
fn run_until(
    supervisor: &mut Supervisor,
    mut done: impl FnMut(&[Event], &Supervisor) -> bool,
) -> Vec<Event> {
    let mut events = Vec::new();
    let give_up = Instant::now() + PATIENCE;
    loop {
        events.extend(supervisor.tick(Instant::now()));
        if done(&events, supervisor) {
            return events;
        }
        assert!(Instant::now() < give_up, "timed out; events: {events:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn connected(events: &[Event], _: &Supervisor) -> bool {
    events.iter().any(|e| matches!(e, Event::Connected { .. }))
}

fn stopped(events: &[Event], _: &Supervisor) -> bool {
    events.iter().any(|e| matches!(e, Event::Stopped { .. }))
}

fn stop_reason(events: &[Event]) -> &StopReason {
    events
        .iter()
        .find_map(|e| match e {
            Event::Stopped { reason, .. } => Some(reason),
            _ => None,
        })
        .unwrap()
}

fn starts(dir: &Path) -> usize {
    fs::read_to_string(dir.join("starts.txt"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// A handle that becomes signalled when process `pid` ends.
fn watch(pid: u32) -> crate::win::Handle {
    // SAFETY: only the right to wait is asked for; the handle is wrapped for closing.
    crate::win::Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.unwrap())
}

fn ended_within(process: &crate::win::Handle, wait: Duration) -> bool {
    // SAFETY: the handle is open.
    unsafe { WaitForSingleObject(process.0, wait.as_millis() as u32) == WAIT_OBJECT_0 }
}

#[test]
fn restart_policy_allows_a_few_per_window() {
    let start = Instant::now();
    let at = |secs: u64| start + Duration::from_secs(secs);
    let mut policy = RestartPolicy::new(3, Duration::from_secs(60));
    assert!(policy.allow(at(0)) && policy.allow(at(10)) && policy.allow(at(20)));
    assert!(!policy.allow(at(30)), "the fourth restart inside a minute");
    assert!(!policy.allow(at(59)));
    // The first restart is now more than a minute old.
    assert!(policy.allow(at(60)));
    assert!(!policy.allow(at(61)));
    policy.reset();
    assert!(policy.allow(at(62)));
}

#[test]
fn a_wrong_hash_is_refused_and_nothing_starts() {
    let (dir, mut spec) = fake("hash", "normal");
    spec.sha256 = "0".repeat(64);
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec.clone(), Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(stop_reason(&events), &StopReason::WrongFile);
    assert_eq!(starts(&dir), 0, "the exe must not have run");
    assert!(!supervisor.is_running(ID));
    // It stays stopped: opening the page again does not try again behind the user's back.
    supervisor.open(spec.clone(), Instant::now());
    assert_eq!(
        supervisor.status(ID),
        Some(Status::Stopped(StopReason::WrongFile))
    );

    // A file swapped after install: same recorded hash, different bytes.
    let (dir, spec) = fake("swap", "normal");
    let mut bytes = fs::read(&spec.exe).unwrap();
    bytes.push(0);
    fs::write(&spec.exe, bytes).unwrap();
    supervisor.forget(ID);
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(stop_reason(&events), &StopReason::WrongFile);
    assert_eq!(starts(&dir), 0);
}

#[test]
fn a_missing_exe_is_reported() {
    let (_dir, mut spec) = fake("missing", "normal");
    spec.exe = spec.exe.with_file_name("gone.exe");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert!(matches!(stop_reason(&events), StopReason::Missing(_)));
}

#[test]
fn starts_connects_and_stops_on_request() {
    let (dir, spec) = fake("normal", "normal");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec.clone(), Instant::now());
    assert_eq!(supervisor.status(ID), Some(Status::Starting));
    run_until(&mut supervisor, connected);
    assert_eq!(
        supervisor.status(ID),
        Some(Status::Running { elevated: false })
    );
    assert!(supervisor.is_running(ID));
    assert!(supervisor.take_link(ID).is_some());
    assert!(supervisor.take_link(ID).is_none(), "the link is handed out once");
    // While it runs, the checked file cannot be replaced.
    assert!(fs::write(&spec.exe, b"swapped").is_err());

    // Opening the page again does not start a second process.
    supervisor.open(spec.clone(), Instant::now());
    supervisor.hide(ID);
    supervisor.open(spec, Instant::now());
    assert!(supervisor.send(ID, &HostMessage::Navigate { page: "x".into() }));

    let process = watch(supervisor.pid(ID).unwrap());
    supervisor.stop(ID, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(stop_reason(&events), &StopReason::ByHost);
    assert!(ended_within(&process, Duration::from_secs(5)));
    assert!(!supervisor.is_running(ID));
    assert_eq!(starts(&dir), 1);
    let report = supervisor.report(ID).unwrap();
    assert!(report.contains("Add-on: Fake add-on (test.fake-addon)"), "{report}");
    assert!(report.contains("Starts this session: 1"), "{report}");
}

#[test]
fn a_crash_is_reported_and_restarted_within_the_limit() {
    let (dir, spec) = fake("crash", "crash");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    let restarts = events
        .iter()
        .filter(|e| matches!(e, Event::Restarting { .. }))
        .count();
    assert_eq!(restarts, 3, "{events:?}");
    assert_eq!(
        stop_reason(&events),
        &StopReason::Crashed { exit_code: Some(3) }
    );
    assert_eq!(starts(&dir), 4, "one start and three restarts");
    let report = supervisor.report(ID).unwrap();
    assert!(report.contains("Last exit code: 3"), "{report}");
    assert!(report.contains("it ended with exit code 3"), "{report}");

    // It stays stopped until the user asks; then the limit starts fresh.
    for _ in 0..20 {
        assert!(supervisor.tick(Instant::now()).is_empty());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(starts(&dir), 4);
    supervisor.restart(ID, Instant::now());
    run_until(&mut supervisor, |_, _| starts(&dir) >= 5);
}

#[test]
fn closing_the_host_ends_addons_and_their_children() {
    let (dir, spec) = fake("job", "child");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    run_until(&mut supervisor, connected);
    let addon = watch(supervisor.pid(ID).unwrap());
    let child_pid: u32 = fs::read_to_string(dir.join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let child = watch(child_pid);
    assert!(!ended_within(&addon, Duration::ZERO));
    assert!(!ended_within(&child, Duration::ZERO));

    // Like a crash of the host: nothing is asked to quit, the job handle just closes.
    supervisor.orderly = false;
    drop(supervisor);
    assert!(ended_within(&addon, Duration::from_secs(10)));
    assert!(ended_within(&child, Duration::from_secs(10)));
}

#[test]
fn a_wrong_token_is_refused() {
    let (dir, spec) = fake("token", "wrong-token");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert!(matches!(stop_reason(&events), StopReason::Refused(_)));
    assert_eq!(starts(&dir), 1, "a refusal is final, not a crash to restart");
}

#[test]
fn an_addon_that_never_connects_is_ended() {
    let (dir, spec) = fake("silent", "silent");
    let mut supervisor = supervisor(Timing {
        connect: Duration::from_millis(400),
        restart_max: 1,
        ..Timing::default()
    });
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(
        stop_reason(&events),
        &StopReason::Crashed { exit_code: None }
    );
    assert_eq!(starts(&dir), 2, "one start and the one allowed restart");
}

/// The pipe's access list as text (SDDL).
fn access_list(pipe: HANDLE) -> String {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open; the descriptor is freed below.
    let status = unsafe {
        GetSecurityInfo(
            pipe,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
    };
    assert!(status.is_ok(), "{status:?}");
    let mut text = PWSTR::null();
    // SAFETY: the descriptor is valid; the text is freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
        .unwrap();
        let list = text.to_string().unwrap();
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(text.0.cast())));
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(descriptor.0)));
        list
    }
}

#[test]
fn the_pipes_admit_only_the_current_user() {
    let pipes = PipePair::create().unwrap();
    let sid = current_user_sid().unwrap();
    for pipe in pipes.handles() {
        let list = access_list(pipe);
        // Protected (nothing inherited) and exactly one entry: allow, for this user. Everyone
        // else, a second user on the PC included, has no entry and is denied.
        assert!(list.starts_with("D:P(A;;"), "{list}");
        assert!(list.ends_with(&format!(";;;{sid})")), "{list}");
        assert_eq!(list.matches('(').count(), 1, "{list}");
    }
    assert_ne!(pipes.to_addon_path, pipes.from_addon_path);
    assert_eq!(pipes.token.len(), super::pipe::TOKEN_CHARS);
    assert_ne!(pipes.token, PipePair::create().unwrap().token);
}

/// Opens both pipes from this process, like an add-on, and writes `token`.
fn client(pipes: &PipePair, token: String) -> std::thread::JoinHandle<Option<Vec<u8>>> {
    let (input, output) = (pipes.to_addon_path.clone(), pipes.from_addon_path.clone());
    std::thread::spawn(move || {
        let mut reader = fs::File::open(input).ok()?;
        let mut writer = fs::OpenOptions::new().write(true).open(output).ok()?;
        writer.write_all(token.as_bytes()).ok()?;
        let mut answer = vec![0; 5];
        reader.read_exact(&mut answer).ok()?;
        Some(answer)
    })
}

#[test]
fn only_the_started_process_with_the_token_gets_in() {
    let deadline = || Instant::now() + Duration::from_secs(10);
    let mut wait = || Ok(());

    // The right token from the expected process: bytes flow both ways.
    let pipes = PipePair::create().unwrap();
    let add_on = client(&pipes, pipes.token.clone());
    let (_from, mut to) = pipes
        .connect(Some(std::process::id()), deadline(), &mut wait)
        .unwrap();
    to.write_all(b"hello").unwrap();
    assert_eq!(add_on.join().unwrap().as_deref(), Some(&b"hello"[..]));

    // The right token from a process the host did not start.
    let pipes = PipePair::create().unwrap();
    let stranger = client(&pipes, pipes.token.clone());
    let refused = pipes.connect(Some(1), deadline(), &mut wait).unwrap_err();
    assert_eq!(refused, ConnectError::WrongClient);
    assert_eq!(stranger.join().unwrap(), None);

    // The expected process with a wrong token.
    let pipes = PipePair::create().unwrap();
    let liar = client(&pipes, "f".repeat(pipes.token.len()));
    let refused = pipes
        .connect(Some(std::process::id()), deadline(), &mut wait)
        .unwrap_err();
    assert_eq!(refused, ConnectError::WrongToken);
    assert_eq!(liar.join().unwrap(), None);

    // Nobody connects.
    let pipes = PipePair::create().unwrap();
    let soon = Instant::now() + Duration::from_millis(50);
    assert_eq!(
        pipes.connect(None, soon, &mut wait).unwrap_err(),
        ConnectError::TimedOut
    );
}
