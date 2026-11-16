//! Supervisor tests against a real child process: `examples/fake_addon.rs`, which cargo builds
//! together with the tests. Each test copies it to a folder of its own.

use super::pipe::{current_user_sid, ConnectError, PipePair};
use super::*;
use std::fs;
use std::io::Read;
use std::path::Path;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HANDLE, HLOCAL, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

pub(crate) const ID: &str = "test.fake-addon";
const PATIENCE: Duration = Duration::from_secs(30);

/// A folder with a copy of the fake add-on as `addon.exe`, set to `mode`.
pub(crate) fn fake(test: &str, mode: &str) -> (PathBuf, LaunchSpec) {
    // The example lands in `<target>/<profile>/examples`; where the test exe itself sits below
    // that folder differs between cargo versions, so look upwards from it.
    let test_exe = std::env::current_exe().unwrap();
    let built = test_exe
        .ancestors()
        .map(|folder| folder.join("examples").join("fake_addon.exe"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| {
            panic!(
                "examples/fake_addon.exe was not found above {}; run the tests through cargo                  so the example is built",
                test_exe.display()
            )
        });
    let dir =
        std::env::temp_dir().join(format!("toolboks-supervisor-{test}-{}", std::process::id()));
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
        background: false,
        installed_root: None,
        protocol_version: toolboks_addon_proto::PROTOCOL_VERSION,
    };
    (dir, spec)
}

pub(crate) fn supervisor(timing: Timing) -> Supervisor {
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
    assert!(
        supervisor.take_link(ID).is_none(),
        "the link is handed out once"
    );
    // While it runs, the checked file cannot be replaced.
    assert!(fs::write(&spec.exe, b"swapped").is_err());

    // Opening the page again does not start a second process.
    supervisor.open(spec.clone(), Instant::now());
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
    assert!(
        report.contains("Add-on: Fake add-on (test.fake-addon)"),
        "{report}"
    );
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
    assert_eq!(
        starts(&dir),
        1,
        "a refusal is final, not a crash to restart"
    );
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

/// The `S-1-…` form of a SID given in that form or as an SDDL alias such as `LA`.
fn full_sid(trustee: &str) -> String {
    let mut sid = PSID::default();
    let mut text = PWSTR::null();
    // SAFETY: the input is NUL-terminated; both results are freed below.
    unsafe {
        ConvertStringSidToSidW(&HSTRING::from(trustee), &mut sid).unwrap();
        ConvertSidToStringSidW(sid, &mut text).unwrap();
        let full = text.to_string().unwrap();
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(text.0.cast())));
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(sid.0)));
        full
    }
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
        assert_eq!(list.matches('(').count(), 1, "{list}");
        // Windows may print a well-known account as an alias (`LA` for the built-in
        // administrator), so compare the account itself, not its spelling.
        let trustee = list.rsplit(';').next().unwrap().trim_end_matches(')');
        assert_eq!(full_sid(trustee), sid, "{list}");
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

#[test]
fn hiding_a_foreground_addon_ends_its_process_and_reopening_starts_a_new_one() {
    let (dir, spec) = fake("hide-foreground", "normal");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec.clone(), Instant::now());
    run_until(&mut supervisor, connected);
    let first = watch(supervisor.pid(ID).unwrap());
    supervisor.hide(ID);
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(stop_reason(&events), &StopReason::ByHost);
    assert!(ended_within(&first, Duration::from_secs(5)));
    assert!(!supervisor.is_running(ID));
    supervisor.open(spec, Instant::now());
    run_until(&mut supervisor, connected);
    assert_eq!(starts(&dir), 2);
}

#[test]
fn reopening_while_hidden_addon_is_quitting_restarts_after_exit() {
    let (dir, spec) = fake("hide-reopen", "normal");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec.clone(), Instant::now());
    run_until(&mut supervisor, connected);
    let first = watch(supervisor.pid(ID).unwrap());
    supervisor.hide(ID);
    supervisor.open(spec, Instant::now());
    run_until(&mut supervisor, connected);
    assert!(ended_within(&first, Duration::from_secs(5)));
    assert_eq!(starts(&dir), 2);
}

#[test]
fn a_pipe_that_closes_before_the_process_ends_is_looked_at_again_soon() {
    // A background add-on gets no heartbeat, so without this nothing would wake the host until
    // some other timer fires, and a crash would wait that long to be noticed and restarted.
    let (_dir, mut spec) = fake("drop-pipe", "drop-pipe");
    spec.background = true;
    let ctx = egui::Context::default();
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = asked.clone();
    ctx.set_repaint_observer(move |info| seen.lock().unwrap().push(info.delay));
    let mut supervisor = Supervisor::with_timing(&ctx, Timing::default()).unwrap();
    supervisor.start_background(spec, Instant::now());
    run_until(&mut supervisor, connected);
    // The add-on has closed its pipes and lives on for about 3 s more.
    let give_up = Instant::now() + Duration::from_secs(2);
    loop {
        asked.lock().unwrap().clear();
        supervisor.tick(Instant::now());
        if asked.lock().unwrap().contains(&EXIT_POLL) {
            break;
        }
        assert!(
            Instant::now() < give_up,
            "the closed pipe never asked to be looked at again"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(supervisor.is_running(ID), "the process has not ended yet");
}

#[test]
fn hidden_background_addon_keeps_its_process() {
    let (dir, mut spec) = fake("hide-background", "normal");
    spec.background = true;
    let mut supervisor = supervisor(Timing::default());
    supervisor.start_background(spec.clone(), Instant::now());
    run_until(&mut supervisor, connected);
    let pid = supervisor.pid(ID).unwrap();
    supervisor.open(spec.clone(), Instant::now());
    supervisor.hide(ID);
    supervisor.tick(Instant::now());
    assert_eq!(supervisor.pid(ID), Some(pid));
    supervisor.open(spec, Instant::now());
    assert_eq!(starts(&dir), 1);
}

fn suspends(dir: &Path) -> usize {
    fs::read_to_string(dir.join("suspends.txt"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// `hide` runs on every tick (also from the hidden-window path, #176): a background add-on is
/// told `Suspend` once per change from shown to hidden, not on every call.
#[test]
fn a_hidden_background_addon_is_told_suspend_once_per_hide() {
    let (dir, mut spec) = fake("suspend-once", "normal");
    spec.background = true;
    let mut supervisor = supervisor(Timing::default());
    supervisor.start_background(spec.clone(), Instant::now());
    run_until(&mut supervisor, connected);
    // Connected hidden: told on connect, and repeated hides add nothing.
    let settle = |supervisor: &mut Supervisor| {
        for _ in 0..30 {
            supervisor.hide(ID);
            supervisor.start_background(spec.clone(), Instant::now());
            supervisor.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    settle(&mut supervisor);
    assert_eq!(suspends(&dir), 1);
    // Its page is opened and left: one more, however often hide is called after.
    supervisor.open(spec.clone(), Instant::now());
    supervisor.tick(Instant::now());
    settle(&mut supervisor);
    assert_eq!(suspends(&dir), 2);
    assert_eq!(starts(&dir), 1);
}

#[test]
fn hiding_during_launch_cancels_without_a_hidden_restart() {
    let (dir, spec) = fake("hide-starting", "silent");
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    supervisor.hide(ID);
    assert!(!supervisor.is_running(ID));
    for _ in 0..20 {
        supervisor.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(starts(&dir) <= 1);
    assert!(matches!(
        supervisor.status(ID),
        Some(Status::Stopped(StopReason::ByHost))
    ));
}

#[test]
fn installed_folder_refuses_dlls_extra_files_and_directories() {
    let dir = std::env::temp_dir().join(format!("toolboks-folder-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("addon.exe");
    fs::write(&exe, b"exe").unwrap();
    fs::write(dir.join("entry.json"), b"{}").unwrap();
    assert!(process::verify_folder(&exe).is_ok());
    for name in ["winhttp.dll", "D3DCompiler_47.DLL", "notes.txt"] {
        fs::write(dir.join(name), b"unexpected").unwrap();
        assert!(matches!(
            process::verify_folder(&exe),
            Err(VerifyError::UnexpectedFile(_))
        ));
        fs::remove_file(dir.join(name)).unwrap();
    }
    fs::create_dir(dir.join("nested")).unwrap();
    assert!(process::verify_folder(&exe).is_err());
    fs::remove_dir(dir.join("nested")).unwrap();
    fs::remove_file(&exe).unwrap();
    assert!(process::verify_folder(&exe).is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installed_folder_refuses_junctions_in_its_path() {
    let dir = std::env::temp_dir().join(format!("toolboks-junction-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("real/1.0.0")).unwrap();
    fs::write(dir.join("real/1.0.0/addon.exe"), b"exe").unwrap();
    // Junction creation needs no developer mode or administrator rights.
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(dir.join("redirected"))
        .arg(dir.join("real"))
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(process::verify_folder(&dir.join("redirected/1.0.0/addon.exe")).is_err());
    fs::remove_dir(dir.join("redirected")).unwrap();
    fs::remove_dir_all(dir).unwrap();
}

/// Inspect the built PE, rather than merely checking the Cargo configuration text.
#[cfg(target_env = "msvc")]
#[test]
fn built_executables_limit_implicit_dll_loads_to_system32() {
    let bytes = fs::read(std::env::current_exe().unwrap()).unwrap();
    let word = |offset: usize| u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
    let dword = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let pe = dword(0x3c) as usize;
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
    let optional = pe + 24;
    let (directories, flag_offset) = match word(optional) {
        0x20b => (optional + 112, 0x4e), // PE32+
        0x10b => (optional + 96, 0x36),  // PE32
        magic => panic!("unexpected PE magic {magic:x}"),
    };
    let config_rva = dword(directories + 10 * 8);
    assert_ne!(config_rva, 0, "load configuration directory is missing");
    let sections = optional + word(pe + 20) as usize;
    let config = (0..word(pe + 6) as usize)
        .find_map(|index| {
            let section = sections + index * 40;
            let start = dword(section + 12);
            let size = dword(section + 16);
            (config_rva >= start && config_rva - start < size)
                .then(|| (dword(section + 20) + config_rva - start) as usize)
        })
        .expect("load configuration must be in a section");
    assert!(dword(config) as usize >= flag_offset + 2);
    assert_eq!(
        word(config + flag_offset),
        0x800,
        "System32 must be the only implicit DLL search path"
    );
}

#[test]
fn a_forged_install_record_cannot_launch_an_elevated_process_without_a_signed_catalog() {
    let (root, mut spec) = fake("elevation-forged-record", "normal");
    let version = root.join("addons").join(ID).join("1.0.0");
    fs::create_dir_all(&version).unwrap();
    fs::copy(&spec.exe, version.join("addon.exe")).unwrap();
    fs::write(
        version.parent().unwrap().join("active.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": "1.0.0", "confirmed": true, "launched": false, "sha256": spec.sha256,
        }))
        .unwrap(),
    )
    .unwrap();
    spec.installed_root = Some(root.clone());
    spec.may_elevate = true;
    let supervisor = supervisor(Timing::default());
    // Call the real launch path; refusal must happen before ShellExecute/UAC or any child.
    let result = launch(&supervisor.shared, &spec, true, &AtomicBool::new(false));
    assert!(matches!(
        result,
        Err(LaunchError::Stop(StopReason::Refused(_)))
    ));
    assert_eq!(starts(&root), 0);
    assert!(!version.join("starts.txt").exists());
}

#[test]
fn an_installed_addon_with_a_planted_dll_is_never_started() {
    let (root, mut spec) = fake("planted-dll-launch", "normal");
    let version = root.join("addons").join(ID).join("1.0.0");
    fs::create_dir_all(&version).unwrap();
    fs::copy(&spec.exe, version.join("addon.exe")).unwrap();
    fs::write(version.join("winhttp.dll"), b"planted DLL").unwrap();
    fs::write(
        version.parent().unwrap().join("active.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": "1.0.0", "confirmed": true, "launched": false, "sha256": spec.sha256,
        }))
        .unwrap(),
    )
    .unwrap();
    spec.installed_root = Some(root.clone());
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert!(matches!(stop_reason(&events), StopReason::Refused(_)));
    assert_eq!(starts(&root), 0);
    assert!(!version.join("starts.txt").exists());
}

#[test]
fn refusing_a_helper_without_a_catalog_keeps_the_same_unelevated_drawing_process_alive() {
    let (root, mut spec) = fake("admin-request", "admin-request");
    spec.may_elevate = true;
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    run_until(&mut supervisor, connected);
    let pid = supervisor.pid(ID).unwrap();
    supervisor.send(ID, &HostMessage::Ping { nonce: 5 });
    run_until(&mut supervisor, |_, _| {
        root.join("admin-result.json").is_file()
    });
    let (id, outcome): (u64, toolboks_addon_proto::AdminOutcome) =
        serde_json::from_slice(&fs::read(root.join("admin-result.json")).unwrap()).unwrap();
    assert_eq!(id, 42);
    assert!(matches!(
        outcome,
        toolboks_addon_proto::AdminOutcome::Failed { .. }
    ));
    assert_eq!(supervisor.pid(ID), Some(pid));
    assert_eq!(starts(&root), 1);
    assert_eq!(
        supervisor.status(ID),
        Some(Status::Running { elevated: false })
    );
}

// Protocol versions (#180): the host speaks the installed entry's version, keeps the agreed one
// per add-on, never sends a message newer than it and ignores newer messages from the add-on.

/// The `Findings` and `StatusItems` messages among `events`, in order.
fn findings_and_status(events: &[Event]) -> (Vec<Vec<toolboks_addon_proto::Finding>>, usize) {
    let mut findings = Vec::new();
    let mut status = 0;
    for event in events {
        match event {
            Event::Message {
                message: AddonMessage::Findings(list),
                ..
            } => findings.push(list.iter().cloned().map(Into::into).collect()),
            Event::Message {
                message: AddonMessage::FindingsV5(list),
                ..
            } => findings.push(list.clone()),
            Event::Message {
                message: AddonMessage::StatusItems(_),
                ..
            } => status += 1,
            _ => {}
        }
    }
    (findings, status)
}

fn has_status(events: &[Event], _: &Supervisor) -> bool {
    findings_and_status(events).1 > 0
}

#[test]
fn a_protocol_3_addon_runs_on_this_host_at_protocol_3() {
    let (dir, mut spec) = fake("protocol-3", "normal --protocol 3");
    spec.protocol_version = 3;
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, has_status);
    assert!(connected(&events, &supervisor));
    assert_eq!(supervisor.protocol_version(ID), Some(3));
    assert_eq!(
        supervisor.status(ID),
        Some(Status::Running { elevated: false })
    );
    // Frames: every input batch is answered with its frame.
    let link = supervisor.take_link(ID).unwrap();
    for seq in 1..=3 {
        assert!(supervisor.send(
            ID,
            &HostMessage::Input(toolboks_addon_proto::InputBatch {
                seq,
                size: [400.0, 300.0],
                pixels_per_point: 1.0,
                theme: toolboks_addon_proto::Theme::Dark,
                window_focused: true,
                events: Vec::new(),
            })
        ));
        match link.recv_timeout(PATIENCE) {
            Ok(Ok(AddonMessage::Frame(frame))) => assert_eq!(frame.seq, seq),
            other => panic!("expected frame {seq}, got {other:?}"),
        }
    }
    // Messages of protocol 3 still go out; it answers pings, so it is not seen as stalled.
    assert!(supervisor.send(ID, &HostMessage::Navigate { page: "x".into() }));
    let report = supervisor.report(ID).unwrap();
    assert!(report.contains("connected at protocol 3"), "{report}");
    supervisor.stop(ID, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(stop_reason(&events), &StopReason::ByHost);
    assert_eq!(supervisor.protocol_version(ID), None);
    assert_eq!(starts(&dir), 1);
}

#[test]
fn check_now_is_never_sent_to_a_protocol_3_addon() {
    let (dir, mut spec) = fake("protocol-3-check", "normal --protocol 3");
    spec.protocol_version = 3;
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    run_until(&mut supervisor, has_status);
    let link = supervisor.take_link(ID).unwrap();
    assert!(
        !supervisor.send(ID, &HostMessage::CheckNow),
        "not sent at protocol 3"
    );
    // An input batch after it: the add-on reads in order, so once this frame is back a
    // CheckNow sent before would have reached it (and ended it).
    assert!(supervisor.send(
        ID,
        &HostMessage::Input(toolboks_addon_proto::InputBatch {
            seq: 1,
            size: [400.0, 300.0],
            pixels_per_point: 1.0,
            theme: toolboks_addon_proto::Theme::Dark,
            window_focused: true,
            events: Vec::new(),
        })
    ));
    match link.recv_timeout(PATIENCE) {
        Ok(Ok(AddonMessage::Frame(frame))) => assert_eq!(frame.seq, 1),
        other => panic!("expected the frame, got {other:?}"),
    }
    let events = supervisor.tick(Instant::now());
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Stopped { .. } | Event::Restarting { .. })),
        "{events:?}"
    );
    assert!(!dir.join("checknow.txt").exists(), "CheckNow reached it");
    assert_eq!(
        supervisor.status(ID),
        Some(Status::Running { elevated: false })
    );
    assert_eq!(starts(&dir), 1);
}

#[test]
fn findings_on_a_protocol_3_connection_are_ignored() {
    let (_dir, mut spec) = fake("protocol-3-findings", "findings --protocol 3");
    spec.protocol_version = 3;
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    // The add-on sent Findings before its status item; both are handled in order.
    let events = run_until(&mut supervisor, has_status);
    let (findings, status) = findings_and_status(&events);
    assert_eq!(status, 1);
    assert!(findings.is_empty(), "findings passed on at protocol 3");
    let report = supervisor.report(ID).unwrap();
    assert!(
        report.contains("ignored a message that protocol 3 does not have"),
        "{report}"
    );
}

#[test]
fn an_addon_under_a_protocol_4_entry_sends_the_old_findings_layout() {
    let (_dir, mut spec) = fake("protocol-4-layout", "findings");
    spec.protocol_version = 4;
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, |events, _| {
        !findings_and_status(events).0.is_empty()
    });
    assert_eq!(supervisor.protocol_version(ID), Some(4));
    let layouts: Vec<bool> = events
        .iter()
        .filter_map(|event| match event {
            Event::Message { message, .. } => match message {
                AddonMessage::Findings(_) => Some(true),
                AddonMessage::FindingsV5(_) => Some(false),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(
        !layouts.is_empty() && layouts.iter().all(|old| *old),
        "{layouts:?}"
    );
}

#[test]
fn a_protocol_5_addon_sends_findings_also_while_hidden_and_gets_check_now() {
    let (dir, mut spec) = fake("protocol-5-findings", "findings");
    spec.background = true;
    let mut supervisor = supervisor(Timing::default());
    // Started hidden: suspended on connect, so only background ticks run from then on.
    supervisor.start_background(spec, Instant::now());
    run_until(&mut supervisor, connected);
    assert_eq!(supervisor.protocol_version(ID), Some(5));
    let events = run_until(&mut supervisor, |events, _| {
        findings_and_status(events).0.len() >= 3
    });
    assert!(suspends(&dir) >= 1, "it was suspended");
    let (findings, _) = findings_and_status(&events);
    assert!(
        findings
            .windows(2)
            .all(|pair| pair[0][0].title != pair[1][0].title),
        "each set differs from the last: {findings:?}"
    );
    assert!(findings.last().unwrap()[0].title != "0 ticks");
    // CheckNow goes out at protocol 5, and the changed findings come back.
    assert!(supervisor.send(ID, &HostMessage::CheckNow));
    run_until(&mut supervisor, |events, _| {
        findings_and_status(events)
            .0
            .iter()
            .any(|list| list[1].title == "Checked 1 times")
    });
    assert!(dir.join("checknow.txt").exists());
}

#[test]
fn an_installed_entry_at_a_protocol_this_build_does_not_speak_is_never_started() {
    for (version, text) in [
        (
            6,
            "Add-on speaks protocol 6, Toolboks speaks 5. Update the add-on or Toolboks.",
        ),
        (
            2,
            "Add-on speaks protocol 2, Toolboks speaks 5. Update the add-on or Toolboks.",
        ),
    ] {
        let (dir, mut spec) = fake(&format!("protocol-{version}"), "normal");
        spec.protocol_version = version;
        let mut supervisor = supervisor(Timing::default());
        supervisor.open(spec.clone(), Instant::now());
        let events = run_until(&mut supervisor, stopped);
        assert_eq!(stop_reason(&events), &StopReason::Refused(text.into()));
        assert_eq!(starts(&dir), 0, "the exe must not have run");
        assert_eq!(supervisor.protocol_version(ID), None);
        // It stays stopped when the page is opened again.
        supervisor.open(spec, Instant::now());
        assert_eq!(
            supervisor.status(ID),
            Some(Status::Stopped(StopReason::Refused(text.into())))
        );
        assert_eq!(starts(&dir), 0);
    }
}

#[test]
fn an_entry_at_5_for_a_protocol_3_exe_is_refused_in_plain_words() {
    // The entry says 5, but the exe speaks only 3: it refuses the host's Hello 5.
    let (dir, spec) = fake("protocol-mismatch", "normal --protocol 3");
    assert_eq!(spec.protocol_version, 5);
    let mut supervisor = supervisor(Timing::default());
    supervisor.open(spec, Instant::now());
    let events = run_until(&mut supervisor, stopped);
    assert_eq!(
        stop_reason(&events),
        &StopReason::Refused(
            "Add-on speaks protocol 3, Toolboks speaks 5. Update the add-on or Toolboks.".into()
        )
    );
    assert_eq!(starts(&dir), 1, "refused, not restarted");
}

#[test]
fn messages_newer_than_the_connection_are_held_back_both_ways() {
    use toolboks_addon_proto::Finding;
    let plain = Finding::ok("a", "A", "B");
    let finding = AddonMessage::Findings(vec![(&plain).into()]);
    let finding_v5 = AddonMessage::FindingsV5(vec![plain]);
    assert!(!host_message_allowed(&HostMessage::CheckNow, 3));
    assert!(host_message_allowed(&HostMessage::CheckNow, 4));
    assert!(host_message_allowed(&HostMessage::Suspend, 3));
    assert!(!addon_message_allowed(&finding, 3));
    assert!(addon_message_allowed(&finding, 4));
    assert!(
        addon_message_allowed(&finding, 5),
        "a protocol 4 set is read at 5 too"
    );
    assert!(!addon_message_allowed(&finding_v5, 3));
    assert!(!addon_message_allowed(&finding_v5, 4));
    assert!(addon_message_allowed(&finding_v5, 5));
    assert!(addon_message_allowed(&AddonMessage::Bye, 3));
    assert_eq!(unsupported_protocol(3), None);
    assert_eq!(unsupported_protocol(4), None);
    assert_eq!(unsupported_protocol(5), None);
    assert!(unsupported_protocol(6).is_some());
    assert!(unsupported_protocol(0).is_some());
}
