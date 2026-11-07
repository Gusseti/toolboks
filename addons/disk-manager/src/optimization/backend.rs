use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Media {
    Hdd,
    Ssd,
    #[default]
    Unknown,
}
impl Media {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdd => "Hard disk drive",
            Self::Ssd => "SSD / solid-state",
            Self::Unknown => "Media type unavailable",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Analyze,
    Defrag,
    Trim,
    Automatic,
}
impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Analyze => "Analyze fragmentation",
            Self::Defrag => "Defragment",
            Self::Trim => "TRIM",
            Self::Automatic => "Optimize with Windows",
        }
    }
    fn flag(self) -> Option<&'static str> {
        match self {
            Self::Analyze => None,
            Self::Defrag => Some("/D"),
            Self::Trim => Some("/L"),
            Self::Automatic => Some("/O"),
        }
    }
}
#[derive(Clone, Default, Debug, Deserialize)]
pub struct History {
    pub operation: String,
    pub volume: String,
    pub time: String,
    pub utc: String,
}
impl History {
    pub fn kind(&self) -> &'static str {
        match self.operation.to_ascii_lowercase().as_str() {
            "retrim" | "trim" => "TRIM",
            "defragmentation" | "defragment" | "defrag" => "Defrag",
            _ => "Windows optimization",
        }
    }
}
#[derive(Clone, Default, Debug, Deserialize)]
pub struct Volume {
    pub letter: String,
    pub device_id: String,
    pub label: String,
    pub filesystem: String,
    pub capacity: u64,
    pub free: u64,
    pub dirty: bool,
    pub history: Vec<History>,
    #[serde(skip)]
    pub media: Media,
}
impl Volume {
    pub fn action(&self) -> Action {
        match self.media {
            Media::Hdd => Action::Defrag,
            Media::Ssd => Action::Trim,
            Media::Unknown => Action::Automatic,
        }
    }
    pub fn supported(&self) -> bool {
        !self.dirty
            && self.capacity > 0
            && ["NTFS", "REFS", "FAT", "FAT32"]
                .contains(&self.filesystem.to_ascii_uppercase().as_str())
    }
}
#[derive(Clone, Default, Deserialize)]
pub struct Metadata {
    pub elevated: bool,
    pub volumes: Vec<Volume>,
    pub history_warning: Option<String>,
    pub checked_at: String,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Analysis {
    pub device_id: String,
    pub letter: String,
    pub total_percent: Option<f64>,
    pub file_percent: Option<f64>,
    pub free_percent: Option<f64>,
    pub fragmented_files: Option<u64>,
    pub total_files: Option<u64>,
    pub recommended: bool,
    pub checked_at: String,
}
impl Analysis {
    fn validate(&self) -> Result<(), String> {
        if !valid_letter(&self.letter)
            || self.device_id.is_empty()
            || self.checked_at.is_empty()
            || [self.total_percent, self.file_percent, self.free_percent]
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0.0..=100.0).contains(v))
        {
            return Err("Windows returned an invalid fragmentation report.".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Job {
    pub letter: String,
    pub action: Action,
    pub started: Instant,
    pub phase_percent: Option<f32>,
    pub output: String,
}
#[derive(Clone, Default)]
pub struct Shared {
    pub metadata: Option<Metadata>,
    pub refreshing: bool,
    pub job: Option<Job>,
    pub reports: BTreeMap<String, Analysis>,
    pub message: Option<String>,
    pub last_output: String,
}
pub type Service = Arc<Mutex<Shared>>;
pub fn service() -> Service {
    static SERVICE: OnceLock<Service> = OnceLock::new();
    SERVICE
        .get_or_init(|| Arc::new(Mutex::new(Shared::default())))
        .clone()
}
pub fn valid_letter(s: &str) -> bool {
    s.len() == 2 && s.as_bytes()[0].is_ascii_uppercase() && s.as_bytes()[1] == b':'
}
fn system_exe(relative: &str) -> Result<std::path::PathBuf, String> {
    #[cfg(windows)]
    {
        let mut path = [0u16; 32768];
        let len = unsafe {
            windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut path))
        } as usize;
        if len == 0 || len >= path.len() {
            return Err("Windows system directory is unavailable.".into());
        }
        Ok(std::path::PathBuf::from(String::from_utf16_lossy(&path[..len])).join(relative))
    }
    #[cfg(not(windows))]
    {
        let _ = relative;
        Err("Drive optimization requires Windows.".into())
    }
}
fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let _ = command;
}
fn script(script: &str, letter: Option<&str>) -> Result<Vec<u8>, String> {
    if letter.is_some_and(|s| !valid_letter(s)) {
        return Err("Invalid drive letter.".into());
    }
    let mut command = Command::new(system_exe(r"WindowsPowerShell\v1.0\powershell.exe")?);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .stdin(Stdio::null());
    if let Some(letter) = letter {
        command.env("TOOLBOKS_OPTIMIZE_VOLUME", letter);
    }
    hidden(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("Could not start Windows storage services: {e}"))?;
    if !output.status.success() {
        return Err(decode(&output.stderr).trim().chars().take(1600).collect());
    }
    Ok(output.stdout)
}
pub fn metadata() -> Result<Metadata, String> {
    let bytes = script(include_str!("metadata.ps1"), None)?;
    let mut value: Metadata = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Could not read Windows drive information: {e}"))?;
    value.volumes.retain(|v| valid_letter(&v.letter));
    for v in &mut value.volumes {
        v.media = media(&v.letter);
    }
    value.volumes.sort_by(|a, b| a.letter.cmp(&b.letter));
    Ok(value)
}
fn analyze(letter: &str) -> Result<Analysis, String> {
    let bytes = script(include_str!("analyze.ps1"), Some(letter))?;
    let value: Analysis = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Could not read the fragmentation report: {e}"))?;
    value.validate()?;
    if value.letter != letter {
        return Err("Windows returned a report for another drive.".into());
    }
    Ok(value)
}
pub fn refresh(service: &Service) {
    {
        let mut s = service.lock().unwrap();
        if s.refreshing || s.job.is_some() {
            return;
        }
        s.refreshing = true;
    }
    let service = service.clone();
    std::thread::spawn(move || {
        let result = metadata();
        let mut s = service.lock().unwrap();
        s.refreshing = false;
        match result {
            Ok(m) => {
                s.metadata = Some(m);
                s.message = None
            }
            Err(e) => s.message = Some(e),
        }
    });
}
fn validate_request<'a>(
    m: &'a Metadata,
    letter: &str,
    device_id: &str,
    action: Action,
) -> Result<&'a Volume, String> {
    if !valid_letter(letter) {
        return Err("Invalid drive letter.".into());
    }
    if !m.elevated {
        return Err(
            "Restart Toolboks with administrator access before analyzing or optimizing.".into(),
        );
    }
    let v = m
        .volumes
        .iter()
        .find(|v| v.letter == letter && v.device_id == device_id)
        .ok_or(
            "The selected volume changed or is no longer available. Refresh and select it again.",
        )?;
    if !v.supported() {
        return Err(
            "The drive is not ready, needs a filesystem check, or uses an unsupported filesystem."
                .into(),
        );
    }
    if action != Action::Analyze && action != v.action() {
        return Err(
            "Drive information changed. Refresh and review the suggested operation.".into(),
        );
    }
    Ok(v)
}
pub fn start(service: &Service, letter: &str, device_id: &str, action: Action) {
    if !valid_letter(letter) {
        return;
    }
    {
        let mut s = service.lock().unwrap();
        if s.job.is_some() || s.refreshing {
            return;
        }
        s.message = None;
        s.last_output.clear();
        s.job = Some(Job {
            letter: letter.into(),
            action,
            started: Instant::now(),
            phase_percent: None,
            output: String::new(),
        });
    }
    let service = service.clone();
    let letter = letter.to_owned();
    let device_id = device_id.to_owned();
    std::thread::spawn(move || {
        let result = (|| {
            let m = metadata()?;
            let v = validate_request(&m, &letter, &device_id, action)?;
            if action != Action::Analyze {
                optimize(&service, &letter, action)?;
            }
            let report = analyze(&letter).and_then(|r| {
                if r.device_id == v.device_id {
                    Ok(r)
                } else {
                    Err(
                        "The drive identity changed during analysis. Refresh and analyze again."
                            .into(),
                    )
                }
            });
            let mut s = service.lock().unwrap();
            if action != Action::Analyze {
                s.reports.remove(&v.device_id);
            }
            match report {
                Ok(report) => {
                    s.reports.insert(report.device_id.clone(), report);
                    Ok(())
                }
                Err(e) if action != Action::Analyze => {
                    s.message = Some(format!(
                        "{} completed. A fresh fragmentation report is unavailable: {e}",
                        action.label()
                    ));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        })();
        let refreshed = metadata();
        let mut s = service.lock().unwrap();
        if let Ok(m) = refreshed {
            s.metadata = Some(m)
        }
        if let Some(job) = s.job.take() {
            s.last_output = job.output;
        }
        match result {
            Ok(()) => {
                if s.message.is_none() {
                    s.message = Some(format!("{} completed for {letter}.", action.label()));
                }
            }
            Err(e) => s.message = Some(e),
        }
    });
}
fn optimize(service: &Service, letter: &str, action: Action) -> Result<(), String> {
    let flag = action
        .flag()
        .ok_or("An analysis cannot start a write operation.")?;
    let mut command = Command::new(system_exe("defrag.exe")?);
    command.args([letter, flag, "/U", "/V"]);
    hidden(&mut command);
    let status = stream_output(command, |output| {
        if let Some(job) = &mut service.lock().unwrap().job {
            job.phase_percent = phase_percent(&output);
            job.output = output;
        }
    })
    .map_err(|e| format!("Could not run Windows drive optimization: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Windows could not complete {} (code {}). See Windows report below.",
            action.label(),
            status.code().unwrap_or(-1)
        ))
    }
}

const OUTPUT_LIMIT: usize = 32 * 1024;

fn stream_output(
    mut command: Command,
    publish: impl FnMut(String),
) -> io::Result<std::process::ExitStatus> {
    let (reader, writer) = io::pipe()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer.try_clone()?))
        .stderr(Stdio::from(writer));
    let mut child = command.spawn()?;
    // Close the parent's write handles so the reader sees EOF when the child exits.
    drop(command);
    let output = read_output(reader, publish);
    let status = child.wait()?;
    output?;
    Ok(status)
}

fn read_output(mut reader: impl Read, mut publish: impl FnMut(String)) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(OUTPUT_LIMIT);
    let mut chunk = [0; 4096];
    let mut last_update = None::<Instant>;
    let mut utf16 = false;
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        // Keep the UTF-16 code-unit boundary even when a pipe read returns an odd byte count.
        let discard = (bytes.len() + count).saturating_sub(OUTPUT_LIMIT - 1) & !1;
        bytes.drain(..discard);
        bytes.extend_from_slice(&chunk[..count]);
        utf16 |= is_utf16(&bytes);
        if last_update.is_none_or(|time| time.elapsed() >= Duration::from_millis(200)) {
            publish(decode_output(&bytes, utf16));
            last_update = Some(Instant::now());
        }
    }
    publish(decode_output(&bytes, utf16));
    Ok(())
}

fn is_utf16(bytes: &[u8]) -> bool {
    bytes.starts_with(&[255, 254])
        || bytes
            .iter()
            .skip(1)
            .step_by(2)
            .take(30)
            .filter(|b| **b == 0)
            .count()
            > 8
}

fn decode(bytes: &[u8]) -> String {
    decode_output(bytes, is_utf16(bytes))
}

fn decode_output(bytes: &[u8], utf16: bool) -> String {
    if utf16 {
        return String::from_utf16_lossy(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .trim_start_matches('\u{feff}')
        .into();
    }
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .into()
}
fn phase_percent(text: &str) -> Option<f32> {
    let end = text.rfind('%')?;
    let prefix = text[..end].trim_end();
    let count = prefix.bytes().rev().take_while(u8::is_ascii_digit).count();
    let value = prefix[prefix.len() - count..].parse::<f32>().ok()?;
    (0.0..=100.0).contains(&value).then_some(value)
}
#[cfg(windows)]
fn media(letter: &str) -> Media {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::CloseHandle,
            Storage::FileSystem::{
                CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
                OPEN_EXISTING,
            },
            System::{Ioctl::*, IO::DeviceIoControl},
        },
    };
    let path: Vec<u16> = format!(r"\\.\{letter}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let Ok(handle) = (unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }) else {
        return Media::Unknown;
    };
    let query = |property| -> Option<bool> {
        let q = STORAGE_PROPERTY_QUERY {
            PropertyId: property,
            QueryType: PropertyStandardQuery,
            ..Default::default()
        };
        let mut bytes = [0u8; 64];
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                Some((&q as *const STORAGE_PROPERTY_QUERY).cast()),
                std::mem::size_of_val(&q) as u32,
                Some(bytes.as_mut_ptr().cast()),
                bytes.len() as u32,
                Some(&mut returned),
                None,
            )
        }
        .is_ok();
        (ok && returned >= 9).then_some(bytes[8] != 0)
    };
    let seek = query(StorageDeviceSeekPenaltyProperty);
    let trim = query(StorageDeviceTrimProperty);
    let _ = unsafe { CloseHandle(handle) };
    match (seek, trim) {
        (Some(true), _) => Media::Hdd,
        (Some(false), Some(true)) => Media::Ssd,
        _ => Media::Unknown,
    }
}
#[cfg(not(windows))]
fn media(_: &str) -> Media {
    Media::Unknown
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_plan_rechecks_identity_media_permissions_and_drive_state() {
        let mut m = Metadata {
            elevated: true,
            volumes: vec![Volume {
                letter: "C:".into(),
                device_id: "same-volume".into(),
                filesystem: "NTFS".into(),
                capacity: 100,
                media: Media::Ssd,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(validate_request(&m, "C:", "same-volume", Action::Trim).is_ok());
        assert!(validate_request(&m, "C:", "same-volume", Action::Defrag).is_err());
        assert!(validate_request(&m, "C:", "reassigned-letter", Action::Trim).is_err());
        m.elevated = false;
        assert!(validate_request(&m, "C:", "same-volume", Action::Trim).is_err());
        m.elevated = true;
        m.volumes[0].dirty = true;
        assert!(validate_request(&m, "C:", "same-volume", Action::Trim).is_err());
    }
    #[test]
    #[ignore = "read-only check of this machine's Windows inventory and event log"]
    fn real_metadata_read_only() {
        let m = metadata().unwrap();
        assert!(!m.volumes.is_empty());
        for v in m.volumes {
            assert!(valid_letter(&v.letter));
            println!(
                "{} {:?}: last operation {}",
                v.letter,
                v.media,
                v.history.first().map(|h| h.kind()).unwrap_or("unavailable")
            );
        }
    }
    #[test]
    fn never_accepts_paths_or_shell_content() {
        for s in [
            "c:",
            "C:\\",
            "C: & whoami",
            "\\\\server\\share",
            "C:\\folder",
            "1:",
        ] {
            assert!(!valid_letter(s));
        }
        assert!(valid_letter("C:"));
    }
    #[test]
    fn correct_flags_and_media_actions() {
        assert_eq!(Action::Analyze.flag(), None);
        assert_eq!(Action::Trim.flag(), Some("/L"));
        assert_eq!(Action::Defrag.flag(), Some("/D"));
        assert_eq!(
            Volume {
                media: Media::Ssd,
                ..Default::default()
            }
            .action(),
            Action::Trim
        );
        assert_eq!(Volume::default().action(), Action::Automatic);
    }
    #[test]
    fn unknown_history_is_not_called_defrag() {
        assert_eq!(
            History {
                operation: "retrim".into(),
                ..Default::default()
            }
            .kind(),
            "TRIM"
        );
        assert_eq!(
            History {
                operation: "analysis".into(),
                ..Default::default()
            }
            .kind(),
            "Windows optimization"
        );
    }
    #[test]
    fn reports_reject_impossible_percentages() {
        let a = Analysis {
            letter: "C:".into(),
            device_id: "volume".into(),
            checked_at: "now".into(),
            total_percent: Some(0.0),
            ..Default::default()
        };
        assert!(a.validate().is_ok());
        assert!(Analysis {
            file_percent: Some(101.0),
            ..a
        }
        .validate()
        .is_err());
    }
    #[test]
    fn progress_is_phase_only_and_decodes_unicode() {
        assert_eq!(phase_percent("Pass 2: 47% complete"), Some(47.0));
        assert_eq!(phase_percent("進捗47% 完了"), Some(47.0));
        assert_eq!(phase_percent("étape%"), None);
        assert_eq!(phase_percent("Done"), None);
        let bytes: Vec<u8> = "Windows 80%"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode(&bytes), "Windows 80%");
    }

    #[test]
    fn output_keeps_a_bounded_utf16_tail() {
        let tail = "進捗47%";
        let bytes: Vec<u8> = std::iter::once(0xfeffu16)
            .chain(std::iter::repeat_n(0x754cu16, OUTPUT_LIMIT))
            .chain(tail.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut output = String::new();
        read_output(bytes.as_slice(), |text| output = text).unwrap();
        assert!(output.ends_with(tail));
        assert_eq!(output.encode_utf16().count(), OUTPUT_LIMIT / 2);
        assert_eq!(phase_percent(&output), Some(47.0));
    }

    #[cfg(windows)]
    #[test]
    fn command_stream_collects_stdout_and_stderr_and_keeps_exit_status() {
        use std::os::windows::process::CommandExt;

        let mut command = Command::new(system_exe("cmd.exe").unwrap());
        command
            .args(["/d", "/c"])
            .raw_arg("echo stdout & echo stderr 1>&2 & exit /b 7");
        hidden(&mut command);
        let mut output = String::new();
        let status = stream_output(command, |text| output = text).unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(
            output.lines().map(str::trim).collect::<Vec<_>>(),
            ["stdout", "stderr"]
        );
    }
}
