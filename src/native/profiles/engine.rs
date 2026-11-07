use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use windows::Win32::{
    System::Com::CoCreateGuid, System::SystemInformation::GetSystemWindowsDirectoryW,
};

use crate::{
    addons,
    tools::debloat::{
        backend::{self, Registry, WinRegistry},
        catalog,
        model::{Edition, Hive, RegValue},
    },
};

#[derive(Clone, Copy)]
pub(super) struct Choice {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
}

pub(super) const CHOICES: &[Choice] = &[
    Choice { id: "game-mode", title: "Turn on Game Mode", detail: "Enable Game Mode for your Windows account." },
    Choice { id: "game-bar-capture", title: "Turn off Game DVR", detail: "Disable Game Bar recording for your account." },
    Choice { id: "consumer-features", title: "Turn off suggested app installs", detail: "Applies to Windows Enterprise and Education." },
    Choice { id: "diagnostic-data-minimum", title: "Limit Windows diagnostic data", detail: "Sets Required on Pro. Keeps a stricter existing setting." },
    Choice { id: "activity-history", title: "Turn off activity history policies", detail: "Stops publishing and uploading activity where Windows supports these policies." },
    Choice { id: "location", title: "Turn off Windows Location", detail: "Blocks Windows location access for all users on Pro, Enterprise, and Education." },
    Choice { id: "storage-sense", title: "Turn off Storage Sense", detail: "Stops automatic storage cleanup for all users on Pro, Enterprise, and Education." },
    Choice { id: "powershell-telemetry", title: "Turn off PowerShell 7 telemetry", detail: "Set the opt-out for your account. Sign out and back in before starting PowerShell 7." },
    Choice { id: "scheduler", title: "Apply gaming scheduler settings", detail: "Set Games scheduling to High, Win32PrioritySeparation to 0x26, and SystemResponsiveness to 10. Changes can affect other apps." },
    Choice { id: "power", title: "Activate Ultimate Performance", detail: "Create and activate a new Ultimate Performance plan. Power use and heat can increase." },
];

const PROFILE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
const GAMES: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games";
const ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
const JOURNAL: &str = "active.json";
pub(super) const LOG: &str = "activity.log";

struct Target {
    hive: Hive,
    path: &'static str,
    name: &'static str,
    desired: RegValue,
    keep_lower: bool,
}

fn targets(id: &str) -> Vec<Target> {
    let entry = |hive, path, name, desired| Target {
        hive,
        path,
        name,
        desired,
        keep_lower: false,
    };
    match id {
        "game-mode" => vec![entry(
            Hive::CurrentUser,
            r"Software\Microsoft\GameBar",
            "AutoGameModeEnabled",
            RegValue::Dword(1),
        )],
        "powershell-telemetry" => vec![entry(
            Hive::CurrentUser,
            "Environment",
            "POWERSHELL_TELEMETRY_OPTOUT",
            RegValue::Sz("1".into()),
        )],
        "location" => vec![entry(
            Hive::LocalMachine,
            r"SOFTWARE\Policies\Microsoft\Windows\LocationAndSensors",
            "DisableLocation",
            RegValue::Dword(1),
        )],
        "storage-sense" => vec![entry(
            Hive::LocalMachine,
            r"SOFTWARE\Policies\Microsoft\Windows\StorageSense",
            "AllowStorageSenseGlobal",
            RegValue::Dword(0),
        )],
        "scheduler" => vec![
            entry(
                Hive::LocalMachine,
                GAMES,
                "Scheduling Category",
                RegValue::Sz("High".into()),
            ),
            entry(
                Hive::LocalMachine,
                r"SYSTEM\CurrentControlSet\Control\PriorityControl",
                "Win32PrioritySeparation",
                RegValue::Dword(0x26),
            ),
            entry(
                Hive::LocalMachine,
                PROFILE,
                "SystemResponsiveness",
                RegValue::Dword(10),
            ),
        ],
        _ => catalog::items()
            .iter()
            .find(|item| item.id == id)
            .into_iter()
            .flat_map(|item| item.settings)
            .map(|setting| Target {
                hive: setting.hive,
                path: setting.path,
                name: setting.name,
                desired: RegValue::Dword(setting.off_value),
                keep_lower: setting.off_at_or_below,
            })
            .collect(),
    }
}

fn required_edition(id: &str) -> Edition {
    if matches!(id, "location" | "storage-sense") {
        return Edition::Pro;
    }
    catalog::items()
        .iter()
        .find(|item| item.id == id)
        .map_or(Edition::Unknown, |item| item.min_edition)
}

impl Target {
    fn read(&self, registry: &impl Registry) -> Result<Option<RegValue>, String> {
        registry.read(self.hive, self.path, self.name)
    }

    fn set(&self, registry: &mut impl Registry, value: Option<&RegValue>) -> Result<(), String> {
        match value {
            Some(value) => registry.write(self.hive, self.path, self.name, value)?,
            None => registry.delete(self.hive, self.path, self.name)?,
        }
        if self.read(registry)?.as_ref() != value {
            return Err(format!(
                "Windows did not retain the requested value for {}.",
                self.name
            ));
        }
        Ok(())
    }

    fn matches(&self, current: Option<&RegValue>) -> bool {
        current == Some(&self.desired)
            || (self.keep_lower
                && matches!((current, &self.desired),
                (Some(RegValue::Dword(current)), RegValue::Dword(desired)) if current < desired))
    }
}

#[derive(Serialize, Deserialize)]
struct Change {
    choice: String,
    name: String,
    original: Option<RegValue>,
    attempted: bool,
}

impl Change {
    fn target(&self) -> Result<Target, String> {
        let choice = CHOICES
            .iter()
            .find(|choice| choice.id == self.choice)
            .ok_or_else(|| format!("Unknown backup group {}.", self.choice))?;
        targets(choice.id)
            .into_iter()
            .find(|target| target.name == self.name)
            .ok_or_else(|| format!("Unknown backup entry {}.", self.name))
    }
}

#[derive(Serialize, Deserialize)]
struct Power {
    original: String,
    created: String,
    attempted: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Journal {
    changes: Vec<Change>,
    power: Option<Power>,
}

pub(super) enum Action {
    Apply {
        selected: Vec<&'static str>,
        enable_protection: bool,
    },
    Restore,
}

pub(super) fn root() -> PathBuf {
    addons::data_root().join("Gaming profile")
}

pub(super) fn has_backup() -> bool {
    root().join(JOURNAL).exists()
}

fn display_value(value: Option<RegValue>) -> String {
    match value {
        None => "Value absent".into(),
        Some(RegValue::Dword(value)) => format!("{value} (0x{value:x})"),
        Some(RegValue::Sz(value)) => value,
    }
}

pub(super) fn preview(selected: &[&str]) -> Vec<String> {
    let env = backend::detect_env();
    let mut lines = Vec::new();
    for choice in CHOICES
        .iter()
        .filter(|choice| selected.contains(&choice.id))
    {
        if env.edition < required_edition(choice.id) {
            lines.push(format!("{} • unavailable on this edition", choice.title));
            continue;
        }
        for target in targets(choice.id) {
            let line = match target.read(&WinRegistry) {
                Ok(current) if target.matches(current.as_ref()) => {
                    format!("{} • {} (keep)", target.name, display_value(current))
                }
                Ok(current) => format!(
                    "{} • {} → {}",
                    target.name,
                    display_value(current),
                    display_value(Some(target.desired))
                ),
                Err(error) => format!("{} • {error}", target.name),
            };
            lines.push(line);
        }
    }
    if selected.contains(&"power") {
        lines.push("Active power plan → Ultimate Performance".into());
    }
    lines
}

pub(super) fn review_backup() -> Result<Vec<String>, String> {
    crate::elevation::require_user_profile()?;
    let journal = load_journal(&root().join(JOURNAL))?;
    review_registry(&journal, &WinRegistry)
}

fn review_registry(journal: &Journal, registry: &impl Registry) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for change in journal.changes.iter().filter(|change| change.attempted) {
        let target = change.target()?;
        let before = display_value(change.original.clone());
        let current = match target.read(registry) {
            Ok(value) => display_value(value),
            Err(error) => format!("Unavailable ({error})"),
        };
        lines.push(format!(
            "{} · Before {before} · Current {current}",
            target.name
        ));
    }
    if lines.is_empty() {
        lines.push("There are no registry edits pending restore.".into());
    }
    Ok(lines)
}

fn load_journal(path: &Path) -> Result<Journal, String> {
    let bytes = fs::read(path).map_err(|e| format!("Open recovery data: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Read recovery data: {e}"))
}

fn save(root: &Path, journal: &Journal) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    addons::write_atomic(&root.join(JOURNAL), &bytes)
}

fn export(journal: &Journal) -> Result<Vec<u8>, String> {
    let mut contents = String::from("Windows Registry Editor Version 5.00\r\n");
    for change in &journal.changes {
        let target = change.target()?;
        let hive = match target.hive {
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
        };
        let value = match &change.original {
            Some(RegValue::Dword(value)) => format!("dword:{value:08x}"),
            Some(RegValue::Sz(value)) => {
                let bytes = value
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("hex(1):{bytes}")
            }
            None => "-".into(),
        };
        contents.push_str(&format!(
            "\r\n[{hive}\\{}]\r\n\"{}\"={value}\r\n",
            target.path, target.name
        ));
    }
    Ok(std::iter::once(0xfeff)
        .chain(contents.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect())
}

pub(super) fn windows_dir() -> Result<PathBuf, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let mut buffer = [0; 260];
    // SAFETY: the API writes at most the slice length supplied by the binding.
    let len = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err("Could not locate the Windows system directory.".into());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
}

fn checked(command: &mut Command, operation: &str) -> Result<String, String> {
    let output = command
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| format!("{operation}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        Ok(stdout.into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("{operation}: {} {}", stdout.trim(), stderr.trim())
            .trim()
            .to_owned())
    }
}

fn new_guid() -> Result<String, String> {
    // SAFETY: the API has no inputs and initializes its output GUID.
    let guid = unsafe { CoCreateGuid() }.map_err(|e| e.to_string())?;
    let hex = format!("{:032x}", guid.to_u128());
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn guids(output: &str) -> impl Iterator<Item = &str> {
    output
        .split(|c: char| !c.is_ascii_hexdigit() && c != '-')
        .filter(|s| {
            s.len() == 36
                && s.bytes().enumerate().all(|(i, c)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        c == b'-'
                    } else {
                        c.is_ascii_hexdigit()
                    }
                })
        })
}

fn power(args: &[&str]) -> Result<String, String> {
    checked(
        Command::new(windows_dir()?.join("System32/powercfg.exe")).args(args),
        "Power plan",
    )
}

fn active_plan() -> Result<String, String> {
    guids(&power(&["/getactivescheme"])?)
        .next()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Could not identify the active power plan.".into())
}

fn activate(id: &str) -> Result<(), String> {
    power(&["/setactive", id])?;
    if active_plan()? != id {
        return Err("Windows did not activate the requested power plan.".into());
    }
    Ok(())
}

fn restore_power(plan: &Power) -> Result<(), String> {
    for id in [&plan.original, &plan.created] {
        if guids(id).next() != Some(id.as_str()) {
            return Err("The backup contains an invalid power plan ID.".into());
        }
    }
    if plan.original == plan.created {
        return Err("The original and created power plan IDs are identical.".into());
    }
    if active_plan()? == plan.created {
        activate(&plan.original)?;
    }
    if guids(&power(&["/list"])?).any(|id| id.eq_ignore_ascii_case(&plan.created)) {
        power(&["/delete", &plan.created])?;
        if guids(&power(&["/list"])?).any(|id| id.eq_ignore_ascii_case(&plan.created)) {
            return Err("Windows did not remove the profile's power plan.".into());
        }
    }
    Ok(())
}

fn restore_point(
    id: &str,
    enable_protection: bool,
    log: &mut impl FnMut(&str),
) -> Result<(), String> {
    let windows = windows_dir()?;
    let drive = windows
        .ancestors()
        .last()
        .ok_or("Could not identify the Windows drive.")?;
    let mut command = Command::new(windows.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .env("TOOLBOKS_RESTORE_DRIVE", drive)
        .env("TOOLBOKS_RESTORE_POINT", format!("Toolboks Gaming {id}"));
    if enable_protection {
        log("Turning on System Protection for the Windows drive.");
    }
    log("Creating and verifying a new Windows restore point.");
    let enable = if enable_protection {
        "Enable-ComputerRestore -Drive $env:TOOLBOKS_RESTORE_DRIVE -ErrorAction Stop;"
    } else {
        ""
    };
    let script = format!(
        "$ErrorActionPreference = 'Stop'; {enable} \
         Checkpoint-Computer -Description $env:TOOLBOKS_RESTORE_POINT -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; \
         $point = Get-ComputerRestorePoint -ErrorAction Stop | Where-Object {{ $_.Description -eq $env:TOOLBOKS_RESTORE_POINT }}; \
         if (-not $point) {{ throw 'No new restore point was found. Check System Protection and the Windows restore-point frequency limit.' }}; \
         Write-Output $point.SequenceNumber"
    );
    let sequence = checked(command.arg(script), "Create restore point")?;
    log(&format!(
        "Windows confirmed restore point {}.",
        sequence.trim()
    ));
    Ok(())
}

fn apply(
    root: &Path,
    selected: &[&str],
    enable_protection: bool,
    log: &mut impl FnMut(&str),
) -> Result<(), String> {
    if root.join(JOURNAL).exists() {
        return Err("Restore the previous Gaming profile before applying another.".into());
    }
    let env = backend::detect_env();
    let mut registry = WinRegistry;
    let mut journal = Journal::default();
    let id = new_guid()?;
    for choice in CHOICES
        .iter()
        .filter(|choice| selected.contains(&choice.id))
    {
        if env.edition < required_edition(choice.id) {
            log(&format!(
                "Skipped {}. This Windows edition does not support its policy.",
                choice.title
            ));
            continue;
        }
        for target in targets(choice.id) {
            let original = target.read(&registry)?;
            if target.matches(original.as_ref()) {
                log(&format!(
                    "{} already has the requested setting.",
                    target.name
                ));
                continue;
            }
            journal.changes.push(Change {
                choice: choice.id.into(),
                name: target.name.into(),
                original,
                attempted: false,
            });
        }
    }
    if selected.contains(&"power") {
        journal.power = Some(Power {
            original: active_plan()?,
            created: id.clone(),
            attempted: false,
        });
    }
    if journal.changes.is_empty() && journal.power.is_none() && !enable_protection {
        log("There are no selected settings to change on this PC.");
        return Ok(());
    }
    if !journal.changes.is_empty() {
        let backup = root.join(&id).join("registry-before.reg");
        addons::write_atomic(&backup, &export(&journal)?)?;
        log(&format!("Registry export saved to {}.", backup.display()));
    }
    restore_point(&id, enable_protection, log)?;
    if journal.changes.is_empty() && journal.power.is_none() {
        return Ok(());
    }
    save(root, &journal)?;
    for index in 0..journal.changes.len() {
        let change = &journal.changes[index];
        let target = change.target()?;
        log(&format!("Applying {}.", target.name));
        if target.read(&registry)? != change.original {
            return Err(format!(
                "{} changed after the backup. The profile stopped.",
                target.name
            ));
        }
        journal.changes[index].attempted = true;
        save(root, &journal)?;
        target.set(&mut registry, Some(&target.desired))?;
        log(&format!("Verified {}.", target.name));
    }
    if let Some(plan) = journal.power.as_mut() {
        if active_plan()? != plan.original {
            return Err("The active power plan changed after the backup.".into());
        }
        plan.attempted = true;
        save(root, &journal)?;
        log("Creating and activating an Ultimate Performance plan.");
        power(&["/duplicatescheme", ULTIMATE, &id])?;
        activate(&id)?;
        log("Verified that Ultimate Performance is active.");
    }
    Ok(())
}

fn restore_value(change: &Change, registry: &mut impl Registry) -> Result<(), String> {
    let target = change.target()?;
    let current = target.read(registry)?;
    if current == change.original {
        return Ok(());
    }
    if current.as_ref() != Some(&target.desired) {
        return Err(format!(
            "{} changed outside Toolboks. Its current value was kept.",
            target.name
        ));
    }
    target.set(registry, change.original.as_ref())
}

fn restore(root: &Path, log: &mut impl FnMut(&str)) -> Result<(), String> {
    let path = root.join(JOURNAL);
    let mut journal = load_journal(&path)?;
    let mut registry = WinRegistry;
    let mut errors = 0;
    if let Some(plan) = journal.power.as_mut().filter(|plan| plan.attempted) {
        match restore_power(plan) {
            Ok(()) => {
                plan.attempted = false;
                save(root, &journal)?;
                log("Removed the Gaming power plan and kept or restored the active plan.");
            }
            Err(error) => {
                log(&error);
                errors += 1;
            }
        }
    }
    for index in (0..journal.changes.len()).rev() {
        let change = &journal.changes[index];
        if !change.attempted {
            continue;
        }
        match restore_value(change, &mut registry) {
            Ok(()) => {
                log(&format!("Verified original value for {}.", change.name));
                journal.changes[index].attempted = false;
                save(root, &journal)?;
            }
            Err(error) => {
                log(&error);
                errors += 1;
            }
        }
    }
    if errors > 0 {
        return Err("Some changes still need attention. Recovery data is available for another restore attempt.".into());
    }
    fs::remove_file(path).map_err(|e| format!("Close recovery data: {e}"))
}

pub(super) fn run(action: Action, mut emit: impl FnMut(String)) -> Result<(), String> {
    if !crate::elevation::is_elevated() {
        return Err("Administrator rights are required.".into());
    }
    crate::elevation::require_user_profile()?;
    let root = root();
    fs::create_dir_all(&root).map_err(|e| format!("Create backup folder: {e}"))?;
    let lock =
        File::create(root.join("run.lock")).map_err(|e| format!("Open profile lock: {e}"))?;
    lock.try_lock()
        .map_err(|e| format!("Lock Gaming profile: {e}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(LOG))
        .map_err(|e| format!("Open activity log: {e}"))?;
    let mut log_error = None;
    let mut log = |message: &str| {
        let line = format!("[{}] {}", crate::win::local_time_hms(), message);
        if let Err(error) = writeln!(file, "{line}") {
            log_error = Some(error);
        }
        emit(line);
    };
    let result = match action {
        Action::Apply {
            selected,
            enable_protection,
        } => {
            log("Gaming profile started.");
            apply(&root, &selected, enable_protection, &mut log)
        }
        Action::Restore => {
            log("Restoring Toolboks changes.");
            restore(&root, &mut log)
        }
    };
    match &result {
        Ok(()) => log("Finished."),
        Err(error) => log(&format!("Stopped. {error}")),
    }
    if let Some(error) = log_error {
        emit(format!("Could not save the full activity log: {error}"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Value(Option<RegValue>);

    impl Registry for Value {
        fn read(&self, _: Hive, _: &str, _: &str) -> Result<Option<RegValue>, String> {
            Ok(self.0.clone())
        }

        fn write(&mut self, _: Hive, _: &str, _: &str, value: &RegValue) -> Result<(), String> {
            self.0 = Some(value.clone());
            Ok(())
        }

        fn delete(&mut self, _: Hive, _: &str, _: &str) -> Result<(), String> {
            self.0 = None;
            Ok(())
        }
    }

    fn change(choice: &str, name: &str, original: Option<RegValue>) -> Change {
        Change {
            choice: choice.into(),
            name: name.into(),
            original,
            attempted: true,
        }
    }

    #[test]
    fn restore_removes_new_values_and_preserves_external_edits() {
        let change = change("game-mode", "AutoGameModeEnabled", None);
        let mut value = Value(Some(RegValue::Dword(1)));
        restore_value(&change, &mut value).unwrap();
        assert_eq!(value.0, None);
        restore_value(&change, &mut value).unwrap();
        value.0 = Some(RegValue::Dword(0));
        assert!(restore_value(&change, &mut value).is_err());
        assert_eq!(value.0, Some(RegValue::Dword(0)));
    }

    #[test]
    fn review_shows_saved_and_live_values_only_for_attempted_edits() {
        let saved = change(
            "scheduler",
            "Win32PrioritySeparation",
            Some(RegValue::Dword(2)),
        );
        let mut unattempted = change(
            "scheduler",
            "SystemResponsiveness",
            Some(RegValue::Dword(20)),
        );
        unattempted.attempted = false;
        let journal = Journal {
            changes: vec![saved, unattempted],
            power: None,
        };
        let live = Value(Some(RegValue::Dword(0x26)));
        assert_eq!(
            review_registry(&journal, &live).unwrap(),
            ["Win32PrioritySeparation · Before 2 (0x2) · Current 38 (0x26)"]
        );
    }

    #[test]
    fn export_contains_original_values_and_can_remove_new_values() {
        let journal = Journal {
            changes: vec![
                change("game-mode", "AutoGameModeEnabled", None),
                change(
                    "scheduler",
                    "Win32PrioritySeparation",
                    Some(RegValue::Dword(2)),
                ),
                change(
                    "scheduler",
                    "Scheduling Category",
                    Some(RegValue::Sz("Low".into())),
                ),
            ],
            power: None,
        };
        let bytes = export(&journal).unwrap();
        assert_eq!(&bytes[..2], &[0xff, 0xfe]);
        let words: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect();
        let text = String::from_utf16(&words).unwrap();
        assert!(text.contains("\"AutoGameModeEnabled\"=-"));
        assert!(text.contains("\"Win32PrioritySeparation\"=dword:00000002"));
        assert!(text.contains("\"Scheduling Category\"=hex(1):4c,00,6f,00,77,00,00,00"));
    }

    #[test]
    fn backup_cannot_select_unrelated_registry_values() {
        assert!(change(
            "edge-shopping-sidebar",
            "EdgeShoppingAssistantEnabled",
            None
        )
        .target()
        .is_err());
        assert!(change("game-mode", "Unrelated", None).target().is_err());
    }

    #[test]
    fn power_plan_ids_can_be_extracted_from_localized_output() {
        let id = "381b4222-f694-41f0-9685-ff5bb260df2e";
        let output = format!("GUID for strømstyringsplan: {id} (Balansert) *");
        assert_eq!(guids(&output).collect::<Vec<_>>(), [id]);
        assert_eq!(guids("381b4222-f694-41f0-9685-ff5bb260df2x").next(), None);
    }
}
