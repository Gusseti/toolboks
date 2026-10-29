//! Startup entries (Run keys and Startup folders) and the Task Manager style on/off switch
//! (StartupApproved). See docs/RESEARCH-debloat.md section 5.
#![allow(dead_code)]

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
    ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ,
    KEY_SET_VALUE, REG_BINARY, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    UserRun,
    /// HKLM\Software\Microsoft\Windows\CurrentVersion\Run
    MachineRun,
    /// HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run (approval key `Run32`)
    MachineRun32,
    /// %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
    UserFolder,
    /// %ProgramData%\Microsoft\Windows\Start Menu\Programs\StartUp
    CommonFolder,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::UserRun,
        Source::MachineRun,
        Source::MachineRun32,
        Source::UserFolder,
        Source::CommonFolder,
    ];

    /// Changing this source needs administrator rights (HKLM).
    pub fn needs_admin(self) -> bool {
        matches!(
            self,
            Source::MachineRun | Source::MachineRun32 | Source::CommonFolder
        )
    }

    /// Plain label: "Your account", "All users", "All users (32-bit)", "Startup folder", ...
    pub fn label(self) -> &'static str {
        match self {
            Source::UserRun => "Your account",
            Source::MachineRun => "All users",
            Source::MachineRun32 => "All users (32-bit)",
            Source::UserFolder => "Your Startup folder",
            Source::CommonFolder => "All users' Startup folder",
        }
    }
}

/// How the page treats an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Launchers, updaters, chat and cloud apps: fine to switch off.
    Suggest,
    /// Hardware helpers, VPN, unknown: may break something.
    Careful,
    /// Security software and system components: shown read-only, cannot be switched.
    Never,
}

/// An entry as found, before looking at the approval state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    pub source: Source,
    /// Run value name, or the shortcut file name (with extension) for folders.
    pub name: String,
    /// Command line (Run) or the shortcut file path (folders).
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub source: Source,
    pub name: String,
    pub command: String,
    pub enabled: bool,
    pub publisher: Option<String>,
    pub class: Class,
}

/// What a change replaced, so it can be put back. `previous` is the exact 12 bytes of the
/// StartupApproved value before the change, or `None` if it did not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoStep {
    pub source: Source,
    pub name: String,
    pub previous: Option<Vec<u8>>,
}

/// Registry and folder access behind a trait so the logic is tested with a fake.
pub trait StartupStore {
    fn list(&self) -> Result<Vec<RawEntry>, String>;
    /// The StartupApproved value for the entry, `Ok(None)` if there is none.
    fn approval(&self, source: Source, name: &str) -> Result<Option<Vec<u8>>, String>;
    /// `None` deletes the value.
    fn set_approval(
        &mut self,
        source: Source,
        name: &str,
        bytes: Option<&[u8]>,
    ) -> Result<(), String>;
}

/// The real registry and folders. Reads work without admin; writes to HKLM sources need it.
pub struct WinStartup;

const RUN_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32_PATH: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn describe(err: WIN32_ERROR, what: &str) -> String {
    if err == ERROR_ACCESS_DENIED {
        "Needs administrator rights".to_string()
    } else {
        format!("{what}: error {}", err.0)
    }
}

fn is_missing(err: WIN32_ERROR) -> bool {
    err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND
}

fn approval_target(source: Source) -> (HKEY, String) {
    let hive = match source {
        Source::UserRun | Source::UserFolder => HKEY_CURRENT_USER,
        _ => HKEY_LOCAL_MACHINE,
    };
    let sub = match source {
        Source::UserRun | Source::MachineRun => "Run",
        Source::MachineRun32 => "Run32",
        Source::UserFolder | Source::CommonFolder => "StartupFolder",
    };
    (hive, format!(r"{APPROVED_BASE}\{sub}"))
}

fn startup_folder(source: Source) -> Option<std::path::PathBuf> {
    let (var, rest) = match source {
        Source::UserFolder => ("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup"),
        Source::CommonFolder => (
            "ProgramData",
            r"Microsoft\Windows\Start Menu\Programs\StartUp",
        ),
        _ => return None,
    };
    let base = std::env::var_os(var)?;
    Some(std::path::PathBuf::from(base).join(rest))
}

/// All string values of a Run key. A missing or unreadable key gives an empty list.
fn read_run_key(hive: HKEY, path: &str, source: Source, out: &mut Vec<RawEntry>) {
    let path_w = wide(path);
    let mut key = HKEY::default();
    // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
    let err = unsafe { RegOpenKeyExW(hive, PCWSTR(path_w.as_ptr()), None, KEY_READ, &mut key) };
    if err != ERROR_SUCCESS {
        return;
    }
    let mut index = 0u32;
    loop {
        let mut name = vec![0u16; 16384];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; 32768];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        // SAFETY: the buffers are valid for the lengths passed alongside them.
        let err = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        };
        index += 1;
        if err == ERROR_NO_MORE_ITEMS {
            break;
        }
        if err == ERROR_MORE_DATA {
            continue;
        }
        if err != ERROR_SUCCESS {
            break;
        }
        let kind = REG_VALUE_TYPE(kind);
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            continue;
        }
        let units: Vec<u16> = data[..data_len as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
        out.push(RawEntry {
            source,
            name: String::from_utf16_lossy(&name[..name_len as usize]),
            command: String::from_utf16_lossy(&units[..end]),
        });
    }
    // SAFETY: `key` was opened above and is closed exactly once.
    unsafe {
        let _ = RegCloseKey(key);
    }
}

impl StartupStore for WinStartup {
    fn list(&self) -> Result<Vec<RawEntry>, String> {
        let mut out = Vec::new();
        read_run_key(HKEY_CURRENT_USER, RUN_PATH, Source::UserRun, &mut out);
        read_run_key(HKEY_LOCAL_MACHINE, RUN_PATH, Source::MachineRun, &mut out);
        read_run_key(
            HKEY_LOCAL_MACHINE,
            RUN32_PATH,
            Source::MachineRun32,
            &mut out,
        );
        for source in [Source::UserFolder, Source::CommonFolder] {
            let Some(dir) = startup_folder(source) else {
                continue;
            };
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for item in rd.flatten() {
                let path = item.path();
                if !path.is_file() {
                    continue;
                }
                let name = item.file_name().to_string_lossy().into_owned();
                if name.eq_ignore_ascii_case("desktop.ini") {
                    continue;
                }
                out.push(RawEntry {
                    source,
                    name,
                    command: path.to_string_lossy().into_owned(),
                });
            }
        }
        Ok(out)
    }

    fn approval(&self, source: Source, name: &str) -> Result<Option<Vec<u8>>, String> {
        let (hive, path) = approval_target(source);
        let path_w = wide(&path);
        let name_w = wide(name);
        let mut key = HKEY::default();
        // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
        let err = unsafe {
            RegOpenKeyExW(
                hive,
                PCWSTR(path_w.as_ptr()),
                None,
                KEY_QUERY_VALUE,
                &mut key,
            )
        };
        if is_missing(err) {
            return Ok(None);
        }
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot open {path}")));
        }
        let mut kind = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        // SAFETY: size query only; the out-parameters are valid and the name is NUL-terminated.
        let err = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        let result = if is_missing(err) {
            Ok(None)
        } else if err != ERROR_SUCCESS {
            Err(describe(err, &format!("cannot read {name}")))
        } else if kind != REG_BINARY {
            Err(format!("{name} has an unexpected type"))
        } else {
            let mut buf = vec![0u8; size as usize];
            let mut cap = size;
            // SAFETY: `buf` holds `cap` bytes.
            let err = unsafe {
                RegQueryValueExW(
                    key,
                    PCWSTR(name_w.as_ptr()),
                    None,
                    Some(&mut kind),
                    Some(buf.as_mut_ptr()),
                    Some(&mut cap),
                )
            };
            if is_missing(err) {
                Ok(None)
            } else if err != ERROR_SUCCESS {
                Err(describe(err, &format!("cannot read {name}")))
            } else {
                buf.truncate(cap as usize);
                Ok(Some(buf))
            }
        };
        // SAFETY: `key` was opened above and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(key);
        }
        result
    }

    fn set_approval(
        &mut self,
        source: Source,
        name: &str,
        bytes: Option<&[u8]>,
    ) -> Result<(), String> {
        let (hive, path) = approval_target(source);
        let path_w = wide(&path);
        let name_w = wide(name);
        let mut key = HKEY::default();
        let Some(bytes) = bytes else {
            // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
            let err = unsafe {
                RegOpenKeyExW(hive, PCWSTR(path_w.as_ptr()), None, KEY_SET_VALUE, &mut key)
            };
            if is_missing(err) {
                return Ok(());
            }
            if err != ERROR_SUCCESS {
                return Err(describe(err, &format!("cannot open {path}")));
            }
            // SAFETY: `key` is open with set rights and the name is NUL-terminated.
            let err = unsafe { RegDeleteValueW(key, PCWSTR(name_w.as_ptr())) };
            // SAFETY: `key` was opened above and is closed exactly once.
            unsafe {
                let _ = RegCloseKey(key);
            }
            if is_missing(err) || err == ERROR_SUCCESS {
                return Ok(());
            }
            return Err(describe(err, &format!("cannot delete {name}")));
        };
        // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
        let err = unsafe {
            RegCreateKeyExW(
                hive,
                PCWSTR(path_w.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot open {path}")));
        }
        // SAFETY: `key` is open with set rights; the name is NUL-terminated; `bytes` holds the data.
        let err =
            unsafe { RegSetValueExW(key, PCWSTR(name_w.as_ptr()), None, REG_BINARY, Some(bytes)) };
        // SAFETY: `key` was opened above and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(key);
        }
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot write {name}")));
        }
        Ok(())
    }
}

/// Enabled unless the first byte is odd (03 = disabled by the user). Missing, empty or shorter
/// than 4 bytes = enabled.
pub fn is_enabled(approval: Option<&[u8]>) -> bool {
    match approval {
        Some(b) if b.len() >= 4 => b[0] & 1 == 0,
        _ => true,
    }
}

/// `03 00 00 00` followed by `now` as a little-endian FILETIME.
pub fn disabled_bytes(now_filetime: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = 3;
    b[4..].copy_from_slice(&now_filetime.to_le_bytes());
    b
}

/// `02 00 00 00` followed by eight zero bytes.
pub fn enabled_bytes() -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = 2;
    b
}

/// True if `needle` (lowercase) occurs in `hay` (lowercase) not directly after a letter or digit.
fn has_word_start(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = hay[from..].find(needle) {
        let at = from + pos;
        let ok = hay[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        if ok {
            return true;
        }
        from = at + needle.len();
    }
    false
}

const DENY_SUBSTR: &[&str] = &[
    "securityhealth",
    "windows defender",
    "windowsdefender",
    "msmpeng",
    "mpcmdrun",
    "mssense",
    "sense.exe",
    "sensecncproxy",
    "norton",
    "mcafee",
    "kaspersky",
    "bitdefender",
    "avast",
    "sophos",
    "malwarebytes",
    "crowdstrike",
    "sentinelone",
    "cylance",
    "trend micro",
    "trendmicro",
    "webroot",
    "f-secure",
    "devicetrust",
    "appcontrol",
];
/// Short names that must start a word (so "Navigator" does not match "avg").
const DENY_WORD: &[&str] = &["avg", "eset"];

const SUGGEST_SUBSTR: &[&str] = &[
    "onedrive",
    "steam",
    "spotify",
    "discord",
    "utweb",
    "utorrent",
    "curseforge",
    "epicgameslauncher",
    "battle.net",
    "blizzard",
    "docker desktop",
    "everything",
    "citrix workspace",
    "samsungrapidapp",
    "microsoftedgeautolaunch",
    "adobe",
    "armsvc",
    "zoom",
    "skype",
    "dropbox",
    "googledrivefs",
    "ituneshelper",
    "jusched",
    "java update",
    "sunjavaupdate",
];
const SUGGEST_WORD: &[&str] = &["teams"];

/// Classification by name, command line and publisher (case-insensitive). The hard-deny list
/// (SecurityHealth, antivirus/EDR, Windows components, anything signed by Microsoft that is a
/// system component) wins over everything else.
pub fn classify(name: &str, command: &str, publisher: Option<&str>) -> Class {
    let name = name.to_lowercase();
    let command = command.to_lowercase();
    let hay = format!("{name}\n{command}");
    if name == "sense"
        || DENY_SUBSTR.iter().any(|k| hay.contains(k))
        || DENY_WORD.iter().any(|k| has_word_start(&hay, k))
    {
        return Class::Never;
    }
    let in_system = command.contains(r"\system32\")
        || command.contains(r"\syswow64\")
        || command.contains(r"%systemroot%\system32")
        || command.contains(r"%windir%\system32");
    let ms = publisher.is_some_and(|p| p.to_lowercase().contains("microsoft"));
    if in_system && ms {
        return Class::Never;
    }
    if SUGGEST_SUBSTR.iter().any(|k| hay.contains(k))
        || SUGGEST_WORD.iter().any(|k| has_word_start(&hay, k))
    {
        return Class::Suggest;
    }
    Class::Careful
}

fn class_rank(c: Class) -> u8 {
    match c {
        Class::Suggest => 0,
        Class::Careful => 1,
        Class::Never => 2,
    }
}

/// All entries with their on/off state, sorted Suggest, Careful, Never and then by name.
/// `publisher_of` resolves a command line to the company name (the real one reads the file
/// version info; tests pass a closure). A failed approval lookup counts as enabled.
pub fn entries(
    store: &dyn StartupStore,
    publisher_of: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = store
        .list()?
        .into_iter()
        .map(|raw| {
            let approval = store.approval(raw.source, &raw.name).unwrap_or(None);
            let publisher = publisher_of(&raw.command);
            let class = classify(&raw.name, &raw.command, publisher.as_deref());
            Entry {
                source: raw.source,
                name: raw.name,
                command: raw.command,
                enabled: is_enabled(approval.as_deref()),
                publisher,
                class,
            }
        })
        .collect();
    out.sort_by_cached_key(|e| (class_rank(e.class), e.name.to_lowercase()));
    Ok(out)
}

/// Pulls the executable path out of a command line: a quoted path, or an unquoted path (which
/// may contain spaces) up to the first `.exe`, else the first word. Arguments are dropped.
pub fn exe_from_command(command: &str) -> Option<String> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        let p = rest[..end].trim();
        return (!p.is_empty()).then(|| p.to_string());
    }
    let lower = c.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(".exe") {
        let end = from + pos + 4;
        if lower[end..]
            .chars()
            .next()
            .is_none_or(|ch| ch.is_whitespace() || ch == '"')
        {
            return Some(c[..end].to_string());
        }
        from = end;
    }
    c.split_whitespace().next().map(str::to_string)
}

/// Replaces `%NAME%` with the environment value; unknown names are left as they are.
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let var = &after[..end];
                match std::env::var(var) {
                    Ok(v) if !var.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Pulls the executable path out of a command line (handles quotes, arguments, environment
/// variables like %ProgramFiles%) and returns the company name from its version info.
pub fn publisher_of_command(command: &str) -> Option<String> {
    let exe = expand_env(&exe_from_command(command)?);
    let path_w = wide(&exe);
    // SAFETY: the path is NUL-terminated and outlives the call.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` is `size` bytes long.
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(path_w.as_ptr()),
            None,
            size,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
        )
        .ok()?;
    }
    let mut langs: Vec<String> = Vec::new();
    let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len = 0u32;
    let tr = wide(r"\VarFileInfo\Translation");
    // SAFETY: `buf` holds the version block; the out-parameters are valid.
    let found = unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const core::ffi::c_void,
            PCWSTR(tr.as_ptr()),
            &mut ptr,
            &mut len,
        )
    };
    if found.as_bool() && !ptr.is_null() && len >= 4 {
        // SAFETY: the API returned `len` bytes inside `buf`.
        let b = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
        for pair in b.as_chunks::<4>().0.iter() {
            let lang = u16::from_le_bytes([pair[0], pair[1]]);
            let cp = u16::from_le_bytes([pair[2], pair[3]]);
            langs.push(format!("{lang:04X}{cp:04X}"));
        }
    }
    langs.push("040904B0".to_string());
    langs.push("040904E4".to_string());
    for l in langs {
        let q = wide(&format!(r"\StringFileInfo\{l}\CompanyName"));
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut n = 0u32;
        // SAFETY: as above.
        let ok = unsafe {
            VerQueryValueW(
                buf.as_ptr() as *const core::ffi::c_void,
                PCWSTR(q.as_ptr()),
                &mut p,
                &mut n,
            )
        };
        if ok.as_bool() && !p.is_null() && n > 0 {
            // SAFETY: the API returned `n` UTF-16 units inside `buf`.
            let s = unsafe { std::slice::from_raw_parts(p as *const u16, n as usize) };
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            let t = String::from_utf16_lossy(&s[..end]).trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// Switches an entry on or off. Refuses `Class::Never`. Reads the approval back and fails if it
/// does not show the new state (the previous value is then put back). Returns the undo step.
pub fn set_enabled(
    store: &mut dyn StartupStore,
    entry: &Entry,
    enable: bool,
    now_filetime: u64,
) -> Result<UndoStep, String> {
    if entry.class == Class::Never {
        return Err(format!(
            "{} is a security or system component and cannot be changed here",
            entry.name
        ));
    }
    let previous = store.approval(entry.source, &entry.name)?;
    let bytes = if enable {
        enabled_bytes()
    } else {
        disabled_bytes(now_filetime)
    };
    if let Err(e) = store.set_approval(entry.source, &entry.name, Some(&bytes)) {
        let _ = store.set_approval(entry.source, &entry.name, previous.as_deref());
        return Err(e);
    }
    let verified = match store.approval(entry.source, &entry.name) {
        Ok(now) => now.is_some() && is_enabled(now.as_deref()) == enable,
        Err(_) => false,
    };
    if !verified {
        let _ = store.set_approval(entry.source, &entry.name, previous.as_deref());
        return Err(format!(
            "Windows did not keep the change for {}; it was put back",
            entry.name
        ));
    }
    Ok(UndoStep {
        source: entry.source,
        name: entry.name.clone(),
        previous,
    })
}

pub fn undo(store: &mut dyn StartupStore, step: &UndoStep) -> Result<(), String> {
    store.set_approval(step.source, &step.name, step.previous.as_deref())
}

/// Plain text of the steps for the clipboard (one line per step).
pub fn undo_text(steps: &[UndoStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let state = match &s.previous {
                None => "default",
                Some(b) if is_enabled(Some(b)) => "on",
                Some(_) => "off",
            };
            format!("{}: {} -> back to {}\n", s.source.label(), s.name, state)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeStore {
        raw: Vec<RawEntry>,
        approvals: HashMap<(String, String), Vec<u8>>,
        fail_read_for: Option<String>,
        corrupt_next_write: bool,
    }

    fn k(source: Source, name: &str) -> (String, String) {
        (format!("{source:?}"), name.to_string())
    }

    impl StartupStore for FakeStore {
        fn list(&self) -> Result<Vec<RawEntry>, String> {
            Ok(self.raw.clone())
        }
        fn approval(&self, source: Source, name: &str) -> Result<Option<Vec<u8>>, String> {
            if self.fail_read_for.as_deref() == Some(name) {
                return Err("boom".into());
            }
            Ok(self.approvals.get(&k(source, name)).cloned())
        }
        fn set_approval(
            &mut self,
            source: Source,
            name: &str,
            bytes: Option<&[u8]>,
        ) -> Result<(), String> {
            match bytes {
                None => {
                    self.approvals.remove(&k(source, name));
                }
                Some(b) => {
                    let mut v = b.to_vec();
                    if self.corrupt_next_write {
                        self.corrupt_next_write = false;
                        v[0] ^= 1;
                    }
                    self.approvals.insert(k(source, name), v);
                }
            }
            Ok(())
        }
    }

    fn raw(source: Source, name: &str, command: &str) -> RawEntry {
        RawEntry {
            source,
            name: name.into(),
            command: command.into(),
        }
    }

    fn entry(class: Class, name: &str) -> Entry {
        Entry {
            source: Source::UserRun,
            name: name.into(),
            command: String::new(),
            enabled: true,
            publisher: None,
            class,
        }
    }

    #[test]
    fn bytes_are_exact() {
        assert_eq!(enabled_bytes(), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let d = disabled_bytes(0x01DD_37CA_33BE_0C89);
        assert_eq!(
            d,
            [3, 0, 0, 0, 0x89, 0x0C, 0xBE, 0x33, 0xCA, 0x37, 0xDD, 0x01]
        );
    }

    #[test]
    fn is_enabled_table() {
        assert!(is_enabled(None));
        assert!(is_enabled(Some(&[])));
        assert!(is_enabled(Some(&[3, 0])));
        assert!(is_enabled(Some(&[2, 0, 0, 0])));
        assert!(is_enabled(Some(&[6, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        assert!(!is_enabled(Some(&[3, 0, 0, 0])));
        assert!(!is_enabled(Some(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])));
    }

    #[test]
    fn classify_hard_deny() {
        for (n, c) in [
            (
                "SecurityHealth",
                r"%windir%\system32\SecurityHealthSystray.exe",
            ),
            (
                "x",
                r"C:\ProgramData\Microsoft\Windows Defender\MsMpEng.exe",
            ),
            ("x", r"C:\Program Files\Windows Defender\MpCmdRun.exe"),
            ("Sense", "x"),
            ("Norton", "x"),
            ("McAfee Agent", "x"),
            ("Kaspersky", "x"),
            ("Bitdefender", "x"),
            ("Avast", "x"),
            ("AVG UI", r"C:\Program Files\AVG\avgui.exe"),
            ("ESET", r"C:\Program Files\ESET\ESET Security\egui.exe"),
            ("Sophos", "x"),
            ("Malwarebytes", "x"),
            ("CrowdStrike", "x"),
            ("SentinelOne", "x"),
            ("Cylance", "x"),
            ("Trend Micro", "x"),
            ("Webroot", "x"),
            ("F-Secure", "x"),
            ("deviceTRUST", "x"),
            ("AppControl", "x"),
        ] {
            assert_eq!(classify(n, c, None), Class::Never, "{n} {c}");
        }
        assert_eq!(
            classify(
                "Thing",
                r"C:\Windows\System32\thing.exe",
                Some("Microsoft Windows")
            ),
            Class::Never
        );
        assert_eq!(
            classify("Thing", r"C:\Windows\System32\thing.exe", Some("Acme")),
            Class::Careful
        );
    }

    #[test]
    fn classify_suggest_and_careful() {
        for n in [
            "OneDrive",
            "Steam",
            "Spotify",
            "Discord",
            "utweb",
            "EpicGamesLauncher",
            "MicrosoftEdgeAutoLaunch_ABC",
            "Adobe ARM",
            "com.squirrel.Teams.Teams",
            "Docker Desktop",
            "GoogleDriveFS",
        ] {
            assert_eq!(classify(n, "", None), Class::Suggest, "{n}");
        }
        for n in [
            "RazerAppEngine",
            "Creative.SBCommand",
            "RadminVPN",
            "Mystery",
            "Alarm",
        ] {
            assert_eq!(classify(n, "", None), Class::Careful, "{n}");
        }
        assert_eq!(classify("Senseless", "x", None), Class::Careful);
    }

    #[test]
    fn entries_sorted_and_tolerant() {
        let mut s = FakeStore {
            raw: vec![
                raw(Source::UserRun, "Zed", "z.exe"),
                raw(Source::UserRun, "SecurityHealth", "s.exe"),
                raw(Source::MachineRun, "Steam", "steam.exe"),
                raw(Source::UserRun, "Bad", "b.exe"),
                raw(Source::UserRun, "Alpha", "a.exe"),
            ],
            fail_read_for: Some("Bad".into()),
            ..Default::default()
        };
        s.approvals
            .insert(k(Source::MachineRun, "Steam"), disabled_bytes(5).to_vec());
        let e = entries(&s, &|c| (c == "a.exe").then(|| "Acme".to_string())).unwrap();
        let names: Vec<_> = e.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Steam", "Alpha", "Bad", "Zed", "SecurityHealth"]);
        assert!(!e[0].enabled);
        assert!(e[2].enabled);
        assert_eq!(e[1].publisher.as_deref(), Some("Acme"));
        assert_eq!(e[4].class, Class::Never);
    }

    #[test]
    fn set_enabled_round_trip_and_undo() {
        let mut s = FakeStore::default();
        let e = entry(Class::Careful, "App");
        let off = set_enabled(&mut s, &e, false, 7).unwrap();
        assert_eq!(off.previous, None);
        assert_eq!(
            s.approval(Source::UserRun, "App").unwrap().unwrap(),
            disabled_bytes(7)
        );
        let on = set_enabled(&mut s, &e, true, 8).unwrap();
        assert_eq!(on.previous, Some(disabled_bytes(7).to_vec()));
        assert_eq!(
            s.approval(Source::UserRun, "App").unwrap().unwrap(),
            enabled_bytes()
        );
        undo(&mut s, &on).unwrap();
        assert_eq!(
            s.approval(Source::UserRun, "App").unwrap().unwrap(),
            disabled_bytes(7)
        );
        undo(&mut s, &off).unwrap();
        assert_eq!(s.approval(Source::UserRun, "App").unwrap(), None);
    }

    #[test]
    fn set_enabled_refuses_never() {
        let mut s = FakeStore::default();
        let err =
            set_enabled(&mut s, &entry(Class::Never, "SecurityHealth"), false, 1).unwrap_err();
        assert!(err.contains("cannot be changed"));
        assert!(s.approvals.is_empty());
    }

    #[test]
    fn set_enabled_read_back_failure_restores() {
        let mut s = FakeStore {
            corrupt_next_write: true,
            ..Default::default()
        };
        let prev = vec![6, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
        s.approvals.insert(k(Source::UserRun, "App"), prev.clone());
        let r = set_enabled(&mut s, &entry(Class::Suggest, "App"), false, 9);
        assert!(r.is_err());
        assert_eq!(s.approval(Source::UserRun, "App").unwrap(), Some(prev));

        let mut s = FakeStore {
            corrupt_next_write: true,
            ..Default::default()
        };
        assert!(set_enabled(&mut s, &entry(Class::Suggest, "App"), false, 9).is_err());
        assert_eq!(s.approval(Source::UserRun, "App").unwrap(), None);
    }

    #[test]
    fn undo_text_lines() {
        let steps = [
            UndoStep {
                source: Source::UserRun,
                name: "A".into(),
                previous: None,
            },
            UndoStep {
                source: Source::MachineRun,
                name: "B".into(),
                previous: Some(enabled_bytes().to_vec()),
            },
            UndoStep {
                source: Source::UserFolder,
                name: "C.lnk".into(),
                previous: Some(disabled_bytes(1).to_vec()),
            },
        ];
        let t = undo_text(&steps);
        let lines: Vec<_> = t.lines().collect();
        assert_eq!(lines[0], "Your account: A -> back to default");
        assert_eq!(lines[1], "All users: B -> back to on");
        assert_eq!(lines[2], "Your Startup folder: C.lnk -> back to off");
    }

    #[test]
    fn exe_parsing() {
        assert_eq!(
            exe_from_command(r#""C:\Program Files\X\a.exe" -min"#).as_deref(),
            Some(r"C:\Program Files\X\a.exe")
        );
        assert_eq!(
            exe_from_command(r"C:\Program Files\X\a.exe /s /x").as_deref(),
            Some(r"C:\Program Files\X\a.exe")
        );
        assert_eq!(exe_from_command(r"C:\a.exe").as_deref(), Some(r"C:\a.exe"));
        assert_eq!(
            exe_from_command(r"%ProgramFiles%\X\a.EXE --go").as_deref(),
            Some(r"%ProgramFiles%\X\a.EXE")
        );
        assert_eq!(
            exe_from_command("rundll32.exe x,y").as_deref(),
            Some("rundll32.exe")
        );
        assert_eq!(exe_from_command("run.bat now").as_deref(), Some("run.bat"));
        assert_eq!(exe_from_command("  "), None);
        assert_eq!(exe_from_command("\"\""), None);
    }

    #[test]
    fn env_expansion() {
        let v = std::env::var("SystemRoot").unwrap_or_default();
        assert_eq!(
            expand_env("%SystemRoot%\\a %NOPE_Q_Z% 5%"),
            format!("{v}\\a %NOPE_Q_Z% 5%")
        );
    }

    #[test]
    #[ignore = "reads the real registry and folders (read-only)"]
    fn real_listing_smoke() {
        let list = entries(&WinStartup, &publisher_of_command);
        assert!(list.is_ok());
        for e in list.unwrap() {
            println!("{:?} {} {:?} {:?}", e.source, e.name, e.class, e.publisher);
        }
    }
}
