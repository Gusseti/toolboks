//! Downloads, installs, updates and removes add-ons listed in the catalog.
//!
//! Layout under `addons::data_root()`:
//! - `addons/<id>/<version>/package.tbaddon` (declarative) or `addon.exe` (native): one folder
//!   per installed version.
//! - `addons/<id>/<version>/entry.json`: the catalog entry that version was installed from.
//! - `addons/<id>/active.json`: which version runs, and the previous one kept for rollback.
//! - `addons/<id>/.download/<version>.part` and `.part.json`: an unfinished download and the
//!   size and SHA-256 it must match, so it can be resumed, or restarted when the index changed.
//! - `addon-data/<id>/`: the add-on's own data, kept or deleted on removal as the user chooses.
//! - `icons/<id>-<sha256>.png`: the add-on's own icon, when its entry has one (`crate::icons`).
//!   Never in the version folder, which holds only the package or exe and `entry.json`.
//!
//! Nothing is placed before the bytes match the size and SHA-256 of the signed index entry, and
//! `download_url` (and the URL after redirects) must be a trusted GitHub URL. Every failure
//! leaves the active version as it was.

// The browser and the launcher call into this in later changes; the tests drive it today.
#![allow(dead_code)]

use crate::addons::write_atomic;
use crate::catalog::{DownloadMismatch, Entry, Hidden, Host, Index, Kind};
use crate::update::is_trusted_url;
use crate::winhttp::{self, Download, HttpError, Saved};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use toolboks_addon_sdk::{version, Package};

pub const PACKAGE_FILE: &str = "package.tbaddon";
pub const EXE_FILE: &str = "addon.exe";
const ACTIVE_FILE: &str = "active.json";
/// The catalog entry the version was installed from: title, pages, capabilities and the
/// background mark, so links and start-at-launch work without the catalog.
const ENTRY_FILE: &str = "entry.json";
const DOWNLOADS: &str = ".download";
/// The catalog is asked for updates at launch and then at most once a day.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The entry is not for this build (version, protocol or channel).
    Hidden(Hidden),
    /// `download_url`, or where it redirected to, is not a trusted URL.
    Untrusted,
    /// The same or a newer version is already installed.
    NotNewer,
    /// The add-on is running; it must be closed first.
    Running,
    /// The download stopped early. The partial file is kept and the next attempt resumes it.
    Interrupted(String),
    Cancelled,
    /// The finished download does not match the signed index. It was deleted.
    Mismatch(DownloadMismatch),
    /// The bytes match the index but are not a valid package for this entry. It was deleted.
    BadPackage(String),
    NotInstalled,
    Io(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hidden(h) => write!(f, "This add-on is not available for this version: {h:?}"),
            Self::Untrusted => f.write_str("The download address is not trusted."),
            Self::NotNewer => f.write_str("This version or a newer one is already installed."),
            Self::Running => f.write_str("Close the add-on before updating it."),
            Self::Interrupted(e) => write!(f, "The download was interrupted: {e}"),
            Self::Cancelled => f.write_str("The download was cancelled."),
            Self::Mismatch(m) => write!(f, "The download does not match the catalog: {m:?}"),
            Self::BadPackage(e) => write!(f, "The download is not a valid add-on: {e}"),
            Self::NotInstalled => f.write_str("This add-on is not installed."),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

fn io(e: impl std::fmt::Display) -> InstallError {
    InstallError::Io(e.to_string())
}

/// Which version runs. Written with `write_atomic`, so switching versions is one rename.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Active {
    pub version: String,
    /// Kept until `version` has started once, so a bad update can roll back.
    pub previous: Option<String>,
    /// `version` has started successfully at least once.
    pub confirmed: bool,
    /// A start of the unconfirmed `version` was attempted.
    pub launched: bool,
    /// SHA-256 of the native exe of `version`, as the signed index gave it at install time.
    /// The supervisor checks the exe against it before every start. `None` for declarative
    /// packages and for installs made before this was recorded (those do not start).
    #[serde(default)]
    pub sha256: Option<String>,
    /// The same for `previous`, so a rollback keeps its check.
    #[serde(default)]
    pub previous_sha256: Option<String>,
}

pub fn addon_dir(root: &Path, id: &str) -> PathBuf {
    root.join("addons").join(id)
}

pub fn data_dir(root: &Path, id: &str) -> PathBuf {
    root.join("addon-data").join(id)
}

/// A version string that is safe as a folder name: strict `x.y.z`.
fn folder_version(text: &str) -> Option<&str> {
    version(text).map(|_| text)
}

fn read_active(dir: &Path) -> Option<Active> {
    let active: Active = serde_json::from_slice(&fs::read(dir.join(ACTIVE_FILE)).ok()?).ok()?;
    folder_version(&active.version)?;
    if let Some(previous) = &active.previous {
        folder_version(previous)?;
    }
    Some(active)
}

fn write_active(dir: &Path, active: &Active) -> Result<(), InstallError> {
    let bytes = serde_json::to_vec_pretty(active).map_err(io)?;
    write_atomic(&dir.join(ACTIVE_FILE), &bytes).map_err(InstallError::Io)
}

/// The installed version of `id`, if any. Reads only local files, so it works offline.
pub fn installed_version(root: &Path, id: &str) -> Option<String> {
    read_active(&addon_dir(root, id)).map(|a| a.version)
}

/// The active declarative package in an `addons/<id>` folder, for the registry.
pub fn active_package(dir: &Path) -> Option<PathBuf> {
    let path = dir.join(read_active(dir)?.version).join(PACKAGE_FILE);
    path.is_file().then_some(path)
}

/// The active native exe of `id` and the SHA-256 recorded for it at install time, for the
/// supervisor. `None` when the add-on is not installed, is declarative, or has no recorded hash.
pub fn native_exe(root: &Path, id: &str) -> Option<(PathBuf, String)> {
    let dir = addon_dir(root, id);
    let active = read_active(&dir)?;
    let sha256 = active.sha256?;
    Some((dir.join(active.version).join(EXE_FILE), sha256))
}

/// The catalog entry the active version of `id` was installed from. `None` when the add-on is
/// not installed or the record is missing or does not belong to that version.
pub fn installed_entry(root: &Path, id: &str) -> Option<Entry> {
    let dir = addon_dir(root, id);
    let active = read_active(&dir)?;
    let bytes = fs::read(dir.join(&active.version).join(ENTRY_FILE)).ok()?;
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    (entry.id == id && entry.version == active.version && entry.validate().is_ok()).then_some(entry)
}

/// The catalog entries of the installed versions of `id`: the active one and the one kept for
/// rollback.
pub fn version_entries(root: &Path, id: &str) -> Vec<Entry> {
    let dir = addon_dir(root, id);
    let Some(active) = read_active(&dir) else {
        return Vec::new();
    };
    std::iter::once(active.version)
        .chain(active.previous)
        .filter_map(|version| {
            serde_json::from_slice::<Entry>(&fs::read(dir.join(version).join(ENTRY_FILE)).ok()?)
                .ok()
        })
        .filter(|entry| entry.id == id)
        .collect()
}

/// [`version_entries`] of every installed add-on.
pub fn all_version_entries(root: &Path) -> Vec<Entry> {
    let Ok(folders) = fs::read_dir(root.join("addons")) else {
        return Vec::new();
    };
    folders
        .flatten()
        .filter(|folder| folder.path().is_dir())
        .flat_map(|folder| version_entries(root, &folder.file_name().to_string_lossy()))
        .collect()
}

/// Deletes the cached icons of `id` that its installed versions no longer use.
fn prune_icons(root: &Path, id: &str) {
    crate::icons::keep_only(root, id, &version_entries(root, id));
}

/// Every installed add-on that has a readable entry record, in id order.
pub fn installed(root: &Path) -> Vec<Entry> {
    let Ok(folders) = fs::read_dir(root.join("addons")) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = folders
        .flatten()
        .filter(|folder| folder.path().is_dir())
        .filter_map(|folder| installed_entry(root, &folder.file_name().to_string_lossy()))
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

/// Fetches a URL into a file. The release build uses WinHTTP; tests use a local server.
pub trait Fetch {
    fn fetch(&self, url: &str, file: &Path, options: Download) -> Result<Saved, HttpError>;
}

/// HTTPS on port 443 through the system proxy.
pub struct WinHttp;

impl Fetch for WinHttp {
    fn fetch(&self, url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
        winhttp::download(url, file, options)
    }
}

/// What the unfinished download in `.download` is for.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PartMeta {
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub previous: Option<String>,
}

/// Downloads `entry`, checks it against the signed index and makes it the active version. The
/// previous version stays until the new one has started once (see [`launching`]).
/// `running` says whether the add-on is running now; an update is then refused.
pub fn install(
    root: &Path,
    entry: &Entry,
    host: &Host,
    fetch: &dyn Fetch,
    running: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Installed, InstallError> {
    entry.check(host).map_err(InstallError::Hidden)?;
    if entry.validate().is_err() || !is_trusted_url(&entry.download_url) {
        return Err(InstallError::Untrusted);
    }
    let new = folder_version(&entry.version)
        .ok_or_else(|| InstallError::BadPackage("bad version".into()))?;
    let dir = addon_dir(root, &entry.id);
    let current = read_active(&dir);
    if let Some(current) = &current {
        if version(new) <= version(&current.version) {
            return Err(InstallError::NotNewer);
        }
        if running {
            return Err(InstallError::Running);
        }
    }

    let bytes = download(&dir, entry, new, fetch, cancel, progress)?;
    let downloads = dir.join(DOWNLOADS);
    let discard = || {
        let _ = fs::remove_file(downloads.join(format!("{new}.part")));
        let _ = fs::remove_file(downloads.join(format!("{new}.part.json")));
    };
    let file = match entry.kind {
        Kind::Declarative => {
            let package = Package::parse(&bytes, host.version);
            match package {
                Ok(p) if p.manifest.id == entry.id && p.manifest.version == entry.version => {
                    let actual: std::collections::BTreeSet<String> = p
                        .manifest
                        .capabilities
                        .iter()
                        .filter_map(|capability| {
                            serde_json::to_value(capability)
                                .ok()?
                                .as_str()
                                .map(str::to_string)
                        })
                        .collect();
                    let reviewed: std::collections::BTreeSet<String> =
                        entry.capabilities.iter().cloned().collect();
                    if actual != reviewed || p.manifest.native_provider.is_some() {
                        discard();
                        return Err(InstallError::BadPackage(
                            "its access does not match the installation review".into(),
                        ));
                    }
                }
                Ok(_) => {
                    discard();
                    return Err(InstallError::BadPackage(
                        "the package does not match the catalog entry".into(),
                    ));
                }
                Err(e) => {
                    discard();
                    return Err(InstallError::BadPackage(e));
                }
            }
            PACKAGE_FILE
        }
        Kind::Native => EXE_FILE,
    };

    // Stage the version folder next to its final place, then rename it in.
    let staging = dir.join(format!(".staging-{new}"));
    let target = dir.join(new);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(io)?;
    fs::write(staging.join(file), &bytes).map_err(io)?;
    fs::write(
        staging.join(ENTRY_FILE),
        serde_json::to_vec_pretty(entry).map_err(io)?,
    )
    .map_err(io)?;
    if target.exists() {
        // A leftover from an earlier attempt; it is neither active nor previous (checked above).
        fs::remove_dir_all(&target).map_err(io)?;
    }
    fs::rename(&staging, &target).map_err(io)?;

    let previous_sha256 = current.as_ref().and_then(|c| c.sha256.clone());
    let previous = current.map(|c| c.version);
    let active = Active {
        version: new.to_string(),
        previous: previous.clone(),
        // A first install has nothing to roll back to.
        confirmed: previous.is_none(),
        launched: false,
        // `download` has just checked the bytes against this hash.
        sha256: (entry.kind == Kind::Native).then(|| entry.sha256.clone()),
        previous_sha256,
    };
    write_active(&dir, &active)?;
    discard();
    prune(&dir, &active);
    // The add-on's own icon, when it has one. Best effort: a failure means the default glyph.
    crate::icons::download_or_log(root, entry, fetch);
    prune_icons(root, &entry.id);
    Ok(Installed {
        version: new.to_string(),
        previous,
    })
}

/// Downloads into `.download/<version>.part`, resuming a matching earlier attempt, and returns
/// the bytes once they match the entry's size and SHA-256.
fn download(
    dir: &Path,
    entry: &Entry,
    new: &str,
    fetch: &dyn Fetch,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Vec<u8>, InstallError> {
    let downloads = dir.join(DOWNLOADS);
    fs::create_dir_all(&downloads).map_err(io)?;
    let part = downloads.join(format!("{new}.part"));
    let meta_path = downloads.join(format!("{new}.part.json"));
    let meta = PartMeta {
        url: entry.download_url.clone(),
        size: entry.size,
        sha256: entry.sha256.clone(),
    };
    let same = fs::read(&meta_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<PartMeta>(&b).ok())
        .is_some_and(|m| m == meta);
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut resume_from = if same && have <= entry.size { have } else { 0 };
    if resume_from == 0 {
        // A different entry or an oversized file: start clean.
        let _ = fs::remove_file(&part);
        write_atomic(&meta_path, &serde_json::to_vec(&meta).map_err(io)?)
            .map_err(InstallError::Io)?;
    }

    if resume_from < entry.size {
        let size = entry.size;
        let saved = fetch.fetch(
            &entry.download_url,
            &part,
            Download {
                resume_from,
                max_bytes: size,
                deadline: DOWNLOAD_DEADLINE,
                cancel,
                progress: &mut |done| progress(done, size),
            },
        );
        match saved {
            Ok(saved) if !is_trusted_url(&saved.final_url) => {
                let _ = fs::remove_file(&part);
                return Err(InstallError::Untrusted);
            }
            Ok(saved) => resume_from = saved.len,
            Err(HttpError::Cancelled) => return Err(InstallError::Cancelled),
            Err(HttpError::Status(code)) => {
                return Err(InstallError::Interrupted(format!("server answered {code}")))
            }
            Err(HttpError::Other(e)) => return Err(InstallError::Interrupted(e)),
        }
        if resume_from < entry.size {
            return Err(InstallError::Interrupted("the download ended early".into()));
        }
    }

    let bytes = fs::read(&part).map_err(io)?;
    if let Err(mismatch) = entry.check_download(&bytes) {
        // Never resume from bytes that failed the check.
        let _ = fs::remove_file(&part);
        let _ = fs::remove_file(&meta_path);
        return Err(InstallError::Mismatch(mismatch));
    }
    Ok(bytes)
}

/// Deletes version folders that are neither active nor kept for rollback, and stale staging.
fn prune(dir: &Path, active: &Active) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let keep = name == active.version.as_str()
            || active.previous.as_deref() == Some(&*name)
            || name == DOWNLOADS
            || !entry.path().is_dir();
        if !keep && (version(&name).is_some() || name.starts_with(".staging-")) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    /// Start this version, then call [`started`] once it runs.
    Start(String),
    /// The last start of the new version never finished; it was removed and this one is back.
    RolledBack { to: String, from: String },
}

/// Call before starting an add-on. A new version gets one start: if that start never reaches
/// [`started`] (a crash, a hang, the app closed), the next call rolls back to the previous one.
pub fn launching(root: &Path, id: &str) -> Result<Launch, InstallError> {
    let dir = addon_dir(root, id);
    let mut active = read_active(&dir).ok_or(InstallError::NotInstalled)?;
    if active.confirmed {
        return Ok(Launch::Start(active.version));
    }
    if active.launched {
        if let Some(from) = rollback(root, id)? {
            let to = read_active(&dir).ok_or(InstallError::NotInstalled)?.version;
            return Ok(Launch::RolledBack { to, from });
        }
    }
    active.launched = true;
    write_active(&dir, &active)?;
    Ok(Launch::Start(active.version))
}

/// The active version started successfully: it is confirmed and the previous one is deleted.
pub fn started(root: &Path, id: &str) -> Result<(), InstallError> {
    let dir = addon_dir(root, id);
    let mut active = read_active(&dir).ok_or(InstallError::NotInstalled)?;
    if active.confirmed && active.previous.is_none() {
        return Ok(());
    }
    active.confirmed = true;
    active.launched = false;
    active.previous = None;
    active.previous_sha256 = None;
    write_active(&dir, &active)?;
    prune(&dir, &active);
    prune_icons(root, id);
    Ok(())
}

/// Switches back to the previous version and deletes the failed one. Returns the version that
/// was rolled back, or `None` when there is no previous version.
pub fn rollback(root: &Path, id: &str) -> Result<Option<String>, InstallError> {
    let dir = addon_dir(root, id);
    let active = read_active(&dir).ok_or(InstallError::NotInstalled)?;
    let Some(previous) = active.previous.clone() else {
        return Ok(None);
    };
    if !dir.join(&previous).is_dir() {
        return Ok(None);
    }
    let back = Active {
        version: previous,
        previous: None,
        confirmed: true,
        launched: false,
        sha256: active.previous_sha256.clone(),
        previous_sha256: None,
    };
    write_active(&dir, &back)?;
    prune(&dir, &back);
    prune_icons(root, id);
    Ok(Some(active.version))
}

/// Removes every installed version of `id`. With `keep_data` the add-on's data folder stays.
/// The caller makes sure the add-on is not running.
pub fn remove(root: &Path, id: &str, keep_data: bool) -> Result<(), InstallError> {
    if !toolboks_addon_sdk::valid_id(id) {
        return Err(InstallError::NotInstalled);
    }
    match fs::remove_dir_all(addon_dir(root, id)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(e)),
    }
    crate::icons::remove(root, id);
    if !keep_data {
        remove_data(root, id)?;
    }
    Ok(())
}

/// Deletes the add-on's data folder.
pub fn remove_data(root: &Path, id: &str) -> Result<(), InstallError> {
    if !toolboks_addon_sdk::valid_id(id) {
        return Err(InstallError::NotInstalled);
    }
    match fs::remove_dir_all(data_dir(root, id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(e)),
    }
}

impl From<InstallError> for String {
    fn from(e: InstallError) -> Self {
        e.to_string()
    }
}

fn last_check_path(root: &Path) -> PathBuf {
    root.join("catalog").join("last-update-check")
}

/// Whether to ask the catalog for updates now: always at launch, then once a day. A clock that
/// went backwards counts as due.
pub fn update_check_due(root: &Path, now: u64, at_launch: bool) -> bool {
    let last = fs::read_to_string(last_check_path(root))
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok());
    at_launch || last.is_none_or(|last| now < last || now - last >= CHECK_INTERVAL_SECS)
}

pub fn record_update_check(root: &Path, now: u64) -> Result<(), InstallError> {
    write_atomic(&last_check_path(root), now.to_string().as_bytes()).map_err(InstallError::Io)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Available<'a> {
    pub entry: &'a Entry,
    pub installed: String,
}

/// Installed add-ons with a newer version in a verified index. Only reads local files and the
/// index, so offline it simply finds nothing new.
pub fn updates<'a>(root: &Path, index: &'a Index, host: &Host) -> Vec<Available<'a>> {
    index
        .listing(host)
        .into_iter()
        .filter_map(|entry| {
            let installed = installed_version(root, &entry.id)?;
            (version(&entry.version) > version(&installed))
                .then_some(Available { entry, installed })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::Registry;
    use crate::test_dir::TestDir;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::sync::{Arc, Mutex};

    const HOST: Host<'static> = Host {
        version: env!("CARGO_PKG_VERSION"),
        protocols: &[1],
        preview: false,
    };

    /// How the test server answers the n-th request (0-based).
    #[derive(Clone, Copy)]
    enum Mode {
        /// Every request gets the whole body, or the asked range.
        Normal,
        /// The first request sends this many bytes and then closes the connection.
        CutFirstAt(usize),
        /// Ranges are ignored: every request gets the whole body with 200.
        NoRanges,
    }

    /// A tiny HTTP/1.1 server on 127.0.0.1. Records the `Range` header of each request.
    struct Server {
        port: u16,
        ranges: Arc<Mutex<Vec<Option<u64>>>>,
    }

    fn serve(body: Vec<u8>, mode: Mode) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let seen = ranges.clone();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { return };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&request).to_lowercase();
                let range = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().trim_end_matches('-').parse::<u64>().ok());
                seen.lock().unwrap().push(range);
                let start = match (mode, range) {
                    (Mode::NoRanges, _) | (_, None) => None,
                    (_, Some(start)) => Some(start as usize),
                };
                let (head, slice) = match start {
                    Some(s) => (
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {s}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len() - s,
                            body.len() - 1,
                            body.len()
                        ),
                        &body[s..],
                    ),
                    None => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        &body[..],
                    ),
                };
                let _ = stream.write_all(head.as_bytes());
                match mode {
                    Mode::CutFirstAt(cut) if n == 0 => {
                        let _ = stream.write_all(&slice[..cut]);
                        let _ = stream.flush();
                        std::thread::sleep(Duration::from_millis(300));
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    _ => {
                        let _ = stream.write_all(slice);
                    }
                }
            }
        });
        Server { port, ranges }
    }

    /// Serves `https://github.com/test/<file>` from the local server, and reports the trusted URL
    /// as the final one, the way a real redirect-free GitHub download looks.
    struct Local(u16);

    impl Fetch for Local {
        fn fetch(&self, url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
            let name = url.rsplit('/').next().unwrap();
            let local = format!("http://127.0.0.1:{}/{name}", self.0);
            winhttp::download_local(&local, file, options).map(|saved| Saved {
                final_url: url.to_string(),
                ..saved
            })
        }
    }

    fn root(name: &str) -> TestDir {
        TestDir::unused(&format!("install-{name}"))
    }

    /// The SDK example package at `version`, serialised the way packages are stored.
    fn package(version: &str) -> (String, Vec<u8>) {
        let mut p = Package::parse(crate::addons::SDK_EXAMPLE.as_bytes(), HOST.version).unwrap();
        p.manifest.version = version.into();
        (
            p.manifest.id.clone(),
            serde_json::to_vec_pretty(&p).unwrap(),
        )
    }

    fn entry(id: &str, version: &str, bytes: &[u8]) -> Entry {
        let capabilities = Package::parse(bytes, HOST.version)
            .map(|package| package.manifest.capabilities)
            .unwrap_or_default();
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": "Hello",
            "short_description": "Example",
            "publisher": "Example developer",
            "category": "developer",
            "icon": "leaf",
            "accent": "#8B5CF6",
            "kind": "declarative",
            "version": version,
            "capabilities": capabilities,
            "min_toolboks_version": HOST.version,
            "protocol_version": 1,
            "download_url": format!("https://github.com/test/{version}.tbaddon"),
            "size": bytes.len(),
            "sha256": toolboks_catalog::sha256_hex(bytes),
            "verified": true,
            "channel": "stable"
        }))
        .unwrap()
    }

    fn run(root: &Path, entry: &Entry, port: u16) -> Result<Installed, InstallError> {
        let cancel = AtomicBool::new(false);
        install(
            root,
            entry,
            &HOST,
            &Local(port),
            false,
            &cancel,
            &mut |_, _| {},
        )
    }

    fn registry_version(root: &Path, id: &str) -> Option<String> {
        Registry::load(root.to_path_buf())
            .packages
            .iter()
            .find(|p| p.manifest.id == id)
            .map(|p| p.manifest.version.clone())
    }

    #[test]
    fn installs_a_good_download_and_works_offline() {
        let root = root("good");
        let (id, bytes) = package("1.0.0");
        let server = serve(bytes.clone(), Mode::Normal);
        let cancel = AtomicBool::new(false);
        let mut last = (0, 0);
        let done = install(
            &root,
            &entry(&id, "1.0.0", &bytes),
            &HOST,
            &Local(server.port),
            false,
            &cancel,
            &mut |done, total| last = (done, total),
        )
        .unwrap();
        assert_eq!(done.version, "1.0.0");
        assert_eq!(last, (bytes.len() as u64, bytes.len() as u64));
        let dir = addon_dir(&root, &id);
        assert_eq!(
            fs::read(dir.join("1.0.0").join(PACKAGE_FILE)).unwrap(),
            bytes
        );
        assert!(!dir.join(DOWNLOADS).join("1.0.0.part").exists());
        // The registry finds the version folder with no server at all.
        drop(server);
        assert_eq!(registry_version(&root, &id).as_deref(), Some("1.0.0"));
        assert_eq!(launching(&root, &id), Ok(Launch::Start("1.0.0".into())));
        // Installing the same version again is refused.
        assert_eq!(
            run(&root, &entry(&id, "1.0.0", &bytes), 1),
            Err(InstallError::NotNewer)
        );
    }

    #[test]
    fn catalog_package_cannot_ask_for_access_missing_from_the_review() {
        let root = root("review-access");
        let (id, bytes) = package("1.0.0");
        let server = serve(bytes.clone(), Mode::Normal);
        let mut reviewed = entry(&id, "1.0.0", &bytes);
        reviewed.capabilities.clear();
        assert!(matches!(
            run(&root, &reviewed, server.port),
            Err(InstallError::BadPackage(_))
        ));
        assert!(installed_version(&root, &id).is_none());
    }

    #[test]
    fn rejects_a_corrupted_download() {
        let root = root("corrupt");
        let (id, bytes) = package("1.0.0");
        let mut bad = bytes.clone();
        bad[10] ^= 0xff;
        let server = serve(bad, Mode::Normal);
        assert_eq!(
            run(&root, &entry(&id, "1.0.0", &bytes), server.port),
            Err(InstallError::Mismatch(DownloadMismatch::Sha256))
        );
        let dir = addon_dir(&root, &id);
        assert!(!dir.join("1.0.0").exists());
        assert!(!dir.join(ACTIVE_FILE).exists());
        assert!(!dir.join(DOWNLOADS).join("1.0.0.part").exists());
        assert_eq!(registry_version(&root, &id), None);
    }

    #[test]
    fn refuses_untrusted_urls_and_wrong_packages() {
        let root = root("untrusted");
        let (id, bytes) = package("1.0.0");
        let mut e = entry(&id, "1.0.0", &bytes);
        e.download_url = "https://example.com/1.0.0.tbaddon".into();
        assert_eq!(run(&root, &e, 1), Err(InstallError::Untrusted));
        // Matching bytes for a different id are still refused.
        let other = entry("com.example.other", "1.0.0", &bytes);
        let server = serve(bytes, Mode::Normal);
        assert!(matches!(
            run(&root, &other, server.port),
            Err(InstallError::BadPackage(_))
        ));
        assert!(!addon_dir(&root, "com.example.other").join("1.0.0").exists());
    }

    #[test]
    fn resumes_an_interrupted_download() {
        let root = root("resume");
        let (id, mut bytes) = package("1.0.0");
        // Make the body span several reads: trailing whitespace is still valid JSON.
        bytes.extend(std::iter::repeat_n(b' ', 200_000));
        let e = entry(&id, "1.0.0", &bytes);
        let cut = 120_000;
        let server = serve(bytes.clone(), Mode::CutFirstAt(cut));
        assert!(matches!(
            run(&root, &e, server.port),
            Err(InstallError::Interrupted(_))
        ));
        let part = addon_dir(&root, &id).join(DOWNLOADS).join("1.0.0.part");
        let have = fs::metadata(&part).unwrap().len();
        assert!(have > 0 && have <= cut as u64, "{have}");
        assert_eq!(run(&root, &e, server.port).unwrap().version, "1.0.0");
        assert_eq!(*server.ranges.lock().unwrap(), vec![None, Some(have)]);
        assert!(!part.exists());
    }

    #[test]
    fn restarts_cleanly_when_resume_is_not_possible() {
        let root = root("restart");
        let (id, bytes) = package("1.0.0");
        let e = entry(&id, "1.0.0", &bytes);
        let downloads = addon_dir(&root, &id).join(DOWNLOADS);
        fs::create_dir_all(&downloads).unwrap();
        // A partial file left by a different index entry must not be reused.
        fs::write(downloads.join("1.0.0.part"), b"garbage").unwrap();
        fs::write(
            downloads.join("1.0.0.part.json"),
            br#"{"url":"https://github.com/test/old","size":1,"sha256":"00"}"#,
        )
        .unwrap();
        let server = serve(bytes.clone(), Mode::Normal);
        assert!(run(&root, &e, server.port).is_ok());
        assert_eq!(*server.ranges.lock().unwrap(), vec![None]);

        // A matching partial file, but the server ignores Range: the file starts over.
        let root2 = self::root("restart2");
        let downloads = addon_dir(&root2, &id).join(DOWNLOADS);
        fs::create_dir_all(&downloads).unwrap();
        fs::write(downloads.join("1.0.0.part"), &bytes[..20]).unwrap();
        fs::write(
            downloads.join("1.0.0.part.json"),
            serde_json::to_vec(&PartMeta {
                url: e.download_url.clone(),
                size: e.size,
                sha256: e.sha256.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let server = serve(bytes.clone(), Mode::NoRanges);
        assert!(run(&root2, &e, server.port).is_ok());
        assert_eq!(*server.ranges.lock().unwrap(), vec![Some(20)]);
        assert_eq!(
            fs::read(addon_dir(&root2, &id).join("1.0.0").join(PACKAGE_FILE)).unwrap(),
            bytes
        );
    }

    #[test]
    fn cancel_keeps_the_partial_file_and_the_old_version() {
        let root = root("cancel");
        let (id, bytes) = package("1.0.0");
        let server = serve(bytes.clone(), Mode::Normal);
        let cancel = AtomicBool::new(true);
        let result = install(
            &root,
            &entry(&id, "1.0.0", &bytes),
            &HOST,
            &Local(server.port),
            false,
            &cancel,
            &mut |_, _| {},
        );
        assert_eq!(result, Err(InstallError::Cancelled));
        assert_eq!(installed_version(&root, &id), None);
    }

    #[test]
    fn update_rolls_back_when_the_new_version_never_starts() {
        let root = root("rollback");
        let (id, v1) = package("1.0.0");
        let (_, v2) = package("1.1.0");
        let (_, v3) = package("1.2.0");
        let s1 = serve(v1.clone(), Mode::Normal);
        run(&root, &entry(&id, "1.0.0", &v1), s1.port).unwrap();
        assert_eq!(started(&root, &id), Ok(()));

        // Refused while the add-on runs.
        let s2 = serve(v2.clone(), Mode::Normal);
        let cancel = AtomicBool::new(false);
        assert_eq!(
            install(
                &root,
                &entry(&id, "1.1.0", &v2),
                &HOST,
                &Local(s2.port),
                true,
                &cancel,
                &mut |_, _| {}
            ),
            Err(InstallError::Running)
        );

        let done = run(&root, &entry(&id, "1.1.0", &v2), s2.port).unwrap();
        assert_eq!(done.previous.as_deref(), Some("1.0.0"));
        let dir = addon_dir(&root, &id);
        assert!(dir.join("1.0.0").is_dir() && dir.join("1.1.0").is_dir());
        assert_eq!(registry_version(&root, &id).as_deref(), Some("1.1.0"));

        // The first start of 1.1.0 never reports back; the next launch rolls back.
        assert_eq!(launching(&root, &id), Ok(Launch::Start("1.1.0".into())));
        assert_eq!(
            launching(&root, &id),
            Ok(Launch::RolledBack {
                to: "1.0.0".into(),
                from: "1.1.0".into()
            })
        );
        assert!(!dir.join("1.1.0").exists());
        assert_eq!(registry_version(&root, &id).as_deref(), Some("1.0.0"));

        // A good update: once it has started, the previous version is deleted.
        let s3 = serve(v3.clone(), Mode::Normal);
        run(&root, &entry(&id, "1.2.0", &v3), s3.port).unwrap();
        assert_eq!(launching(&root, &id), Ok(Launch::Start("1.2.0".into())));
        assert_eq!(started(&root, &id), Ok(()));
        assert!(!dir.join("1.0.0").exists());
        assert_eq!(launching(&root, &id), Ok(Launch::Start("1.2.0".into())));
    }

    /// Sends `.../icon.png` to `icon` and everything else to `addon`.
    struct WithIcon<'a> {
        addon: &'a dyn Fetch,
        icon: &'a dyn Fetch,
    }

    impl Fetch for WithIcon<'_> {
        fn fetch(&self, url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
            if url.ends_with("/icon.png") {
                self.icon.fetch(url, file, options)
            } else {
                self.addon.fetch(url, file, options)
            }
        }
    }

    #[test]
    fn custom_icons_are_cached_outside_the_version_folder_and_removed_with_the_addon() {
        use crate::icons::tests::{png, with_icon, Serve};
        let root = root("icons");
        let run_with = |entry: &Entry, port: u16, icon: &Serve| {
            let cancel = AtomicBool::new(false);
            let fetch = WithIcon {
                addon: &Local(port),
                icon,
            };
            install(&root, entry, &HOST, &fetch, false, &cancel, &mut |_, _| {})
        };
        let (id, v1) = package("1.0.0");
        let (_, v2) = package("1.1.0");
        let (icon1, icon2) = (png(64, 64), png(32, 32));
        let mut e1 = entry(&id, "1.0.0", &v1);
        with_icon(&mut e1, &icon1);
        let mut e2 = entry(&id, "1.1.0", &v2);
        with_icon(&mut e2, &icon2);
        let path1 = crate::icons::path(&root, &e1).unwrap();
        let path2 = crate::icons::path(&root, &e2).unwrap();
        assert_ne!(path1, path2);

        let s1 = serve(v1.clone(), Mode::Normal);
        run_with(&e1, s1.port, &Serve::new(icon1.clone())).unwrap();
        assert_eq!(fs::read(&path1).unwrap(), icon1);
        // The version folder holds only what the supervisor allows.
        let mut files: Vec<String> = fs::read_dir(addon_dir(&root, &id).join("1.0.0"))
            .unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, [ENTRY_FILE, PACKAGE_FILE]);

        // An update keeps the old icon while the old version is kept for rollback...
        let s2 = serve(v2.clone(), Mode::Normal);
        run_with(&e2, s2.port, &Serve::new(icon2.clone())).unwrap();
        assert!(path1.exists() && path2.exists());
        // ...and drops it once the new version has started (the add-on was replaced).
        launching(&root, &id).unwrap();
        started(&root, &id).unwrap();
        assert!(!path1.exists() && path2.exists());

        // Uninstalling removes the cached icon.
        remove(&root, &id, false).unwrap();
        assert!(!path2.exists());
        assert_eq!(fs::read_dir(crate::icons::dir(&root)).unwrap().count(), 0);

        // An icon that does not match the catalog is not placed, and the install still works.
        let mut tampered = icon1.clone();
        tampered[40] ^= 1;
        let s3 = serve(v1.clone(), Mode::Normal);
        assert_eq!(
            run_with(&e1, s3.port, &Serve::new(tampered)).map(|done| done.version),
            Ok("1.0.0".to_string())
        );
        assert!(!path1.exists());
        assert_eq!(installed_version(&root, &id).as_deref(), Some("1.0.0"));
    }

    #[test]
    fn removal_keeps_or_deletes_data() {
        let root = root("remove");
        let (id, bytes) = package("1.0.0");
        let server = serve(bytes.clone(), Mode::Normal);
        let e = entry(&id, "1.0.0", &bytes);
        let prefs = |root: &Path| {
            let mut r = Registry::load(root.to_path_buf());
            r.config
                .preferences
                .entry(id.clone())
                .or_default()
                .insert("k".into(), serde_json::json!(1));
            r.save().unwrap();
            fs::create_dir_all(data_dir(root, &id)).unwrap();
            fs::write(data_dir(root, &id).join("state"), b"x").unwrap();
        };

        run(&root, &e, server.port).unwrap();
        prefs(&root);
        Registry::load(root.to_path_buf())
            .remove(&id, true)
            .unwrap();
        assert!(!addon_dir(&root, &id).exists());
        assert!(data_dir(&root, &id).join("state").exists());
        let r = Registry::load(root.to_path_buf());
        assert!(r.config.preferences.contains_key(&id));
        assert!(r.packages.iter().all(|p| p.manifest.id != id));

        run(&root, &e, server.port).unwrap();
        Registry::load(root.to_path_buf())
            .remove(&id, false)
            .unwrap();
        assert!(!addon_dir(&root, &id).exists());
        assert!(!data_dir(&root, &id).exists());
        assert!(!Registry::load(root.to_path_buf())
            .config
            .preferences
            .contains_key(&id));
    }

    /// A catalog add-on (such as Disk Manager) is removed with `remove` directly, the call the
    /// Remove dialog makes for an add-on that is not a local package. Its saved scans live in a
    /// subfolder of `addon-data/<id>`: "Also delete its data" deletes them, otherwise they stay.
    /// Another add-on's data is never touched.
    #[test]
    fn removing_a_catalog_addon_deletes_its_saved_scans_only_when_asked() {
        let id = "toolboks.disk-manager";
        let other = "toolboks.resource-monitor";
        for delete_data in [false, true] {
            let root = root(if delete_data {
                "scans-delete"
            } else {
                "scans-keep"
            });
            let installed = addon_dir(&root, id).join("1.0.0");
            fs::create_dir_all(&installed).unwrap();
            fs::write(installed.join("addon.exe"), b"exe").unwrap();
            let scans = data_dir(&root, id).join("scans");
            fs::create_dir_all(&scans).unwrap();
            fs::write(scans.join("C.json"), b"{\"files\":[\"a.txt\"]}").unwrap();
            fs::write(data_dir(&root, id).join("settings.json"), b"{}").unwrap();
            let other_data = data_dir(&root, other).join("history");
            fs::create_dir_all(&other_data).unwrap();

            remove(&root, id, !delete_data).unwrap();

            assert!(!addon_dir(&root, id).exists());
            assert_eq!(scans.join("C.json").exists(), !delete_data);
            assert_eq!(data_dir(&root, id).exists(), !delete_data);
            assert!(other_data.exists(), "another add-on's data stays");
            // Deleting again, or deleting data that is already gone, is not an error.
            remove(&root, id, false).unwrap();
            assert!(!data_dir(&root, id).exists());
        }
    }

    #[test]
    fn update_checks_at_launch_and_daily() {
        let root = root("check");
        let now = 1_800_000_000;
        assert!(update_check_due(&root, now, false));
        record_update_check(&root, now).unwrap();
        assert!(!update_check_due(&root, now + 60, false));
        assert!(update_check_due(&root, now + 60, true));
        assert!(update_check_due(&root, now + CHECK_INTERVAL_SECS, false));
        assert!(update_check_due(&root, now - 1, false));

        let (id, bytes) = package("1.0.0");
        let (_, newer) = package("1.1.0");
        let server = serve(bytes.clone(), Mode::Normal);
        run(&root, &entry(&id, "1.0.0", &bytes), server.port).unwrap();
        let index = Index {
            format_version: 1,
            entries: vec![entry(&id, "1.1.0", &newer)],
        };
        let found = updates(&root, &index, &HOST);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].installed, "1.0.0");
        assert_eq!(found[0].entry.version, "1.1.0");
    }

    #[test]
    fn native_installs_record_the_exe_hash() {
        let root = root("native-hash");
        let id = "test.native";
        let native = |version: &str, bytes: &[u8]| {
            let mut e = entry(id, version, bytes);
            e.kind = Kind::Native;
            e.pages = vec!["Map".into(), "Largest files".into()];
            e.background = true;
            e
        };
        let (v1, v2) = (b"first exe".to_vec(), b"second exe".to_vec());
        let s1 = serve(v1.clone(), Mode::Normal);
        run(&root, &native("1.0.0", &v1), s1.port).unwrap();
        let (exe, sha256) = native_exe(&root, id).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), v1);
        assert_eq!(sha256, toolboks_catalog::sha256_hex(&v1));
        assert_eq!(started(&root, id), Ok(()));
        // The entry is kept beside the version, so pages and the background mark are known
        // without the catalog.
        let kept = installed_entry(&root, id).unwrap();
        assert_eq!(kept, native("1.0.0", &v1));
        assert_eq!(kept.pages, ["Map", "Largest files"]);
        assert!(kept.background);
        assert_eq!(installed(&root), [kept]);
        assert_eq!(installed_entry(&root, "test.other"), None);

        // An update records the new hash and keeps the old one for the rollback.
        let s2 = serve(v2.clone(), Mode::Normal);
        run(&root, &native("1.1.0", &v2), s2.port).unwrap();
        let (exe, sha256) = native_exe(&root, id).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), v2);
        assert_eq!(sha256, toolboks_catalog::sha256_hex(&v2));
        assert_eq!(launching(&root, id), Ok(Launch::Start("1.1.0".into())));
        assert!(matches!(
            launching(&root, id),
            Ok(Launch::RolledBack { .. })
        ));
        let (exe, sha256) = native_exe(&root, id).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), v1);
        assert_eq!(sha256, toolboks_catalog::sha256_hex(&v1));

        // A record without a hash (written before hashes were recorded) gives nothing to start.
        let dir = addon_dir(&root, id);
        fs::write(
            dir.join(ACTIVE_FILE),
            br#"{"version":"1.0.0","previous":null,"confirmed":true,"launched":false}"#,
        )
        .unwrap();
        assert_eq!(installed_version(&root, id).as_deref(), Some("1.0.0"));
        assert_eq!(native_exe(&root, id), None);

        // A declarative package has no exe.
        let (package_id, bytes) = package("1.0.0");
        let s3 = serve(bytes.clone(), Mode::Normal);
        run(&root, &entry(&package_id, "1.0.0", &bytes), s3.port).unwrap();
        assert_eq!(native_exe(&root, &package_id), None);
        let ids: Vec<String> = installed(&root).into_iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&package_id) && ids.contains(&id.to_string()));
        // A record that belongs to another version is not trusted.
        let stale = serde_json::to_vec(&native("9.9.9", &v1)).unwrap();
        fs::write(dir.join("1.0.0").join(ENTRY_FILE), stale).unwrap();
        assert_eq!(installed_entry(&root, id), None);
    }

    #[test]
    fn write_atomic_keeps_dotted_names_apart() {
        let root = root("atomic");
        write_atomic(&root.join("0.12.5"), b"a").unwrap();
        write_atomic(&root.join("0.12.6"), b"b").unwrap();
        assert_eq!(fs::read(root.join("0.12.5")).unwrap(), b"a");
        assert_eq!(fs::read(root.join("0.12.6")).unwrap(), b"b");
        assert!(!root.join("0.12.pending").exists());
    }
}
