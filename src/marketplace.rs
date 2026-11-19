use crate::{catalog, installer, winhttp};
use catalog::{Entry, Index};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub const PICK_ORDER: [&str; 8] = [
    "resource-monitor",
    "memory-cleaner",
    "disk-manager",
    "event-watch",
    "debloater",
    "system-profiles",
    "aural",
    "macro-manager",
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum InstallStatus {
    Waiting,
    Downloading { done: u64, total: u64 },
    Checking,
    Ready,
    Cancelled,
    Failed(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_installed_add_ons_with_a_newer_listed_version_are_pending() {
        let mut newer = entry("test.newer");
        newer.version = "1.2.0".into();
        let mut same = entry("test.same");
        same.version = "1.0.0".into();
        let mut missing = entry("test.missing");
        missing.version = "3.0.0".into();
        let list = [newer, same, missing];
        let installed = |id: &str| match id {
            "test.newer" => Some("1.0.0".to_string()),
            "test.same" => Some("1.0.0".to_string()),
            _ => None,
        };
        let pending = pending_updates(&list, installed);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "test.newer");
        // An older listed version never offers a downgrade.
        assert!(pending_updates(&list, |_| Some("9.0.0".to_string())).is_empty());
    }

    #[test]
    fn the_update_line_names_what_can_be_updated() {
        let one = ["Audio Gainer".to_string()];
        let two = ["Audio Gainer".to_string(), "Disk Manager".to_string()];
        assert_eq!(update_line(&[]), "");
        assert_eq!(update_line(&one), "Update available: Audio Gainer");
        assert_eq!(
            update_line(&two),
            "2 updates available: Audio Gainer, Disk Manager"
        );
    }

    #[test]
    fn both_list_addresses_are_trusted_github_hosts() {
        assert!(crate::update::is_trusted_url(catalog::URL));
        assert!(crate::update::is_trusted_url(catalog::FRESH_URL));
        assert!(catalog::FRESH_URL.ends_with("/catalog/index.json"));
    }

    #[test]
    fn the_list_is_fetched_again_every_five_minutes() {
        let start = std::time::Instant::now();
        let every = REFRESH_EVERY;
        assert_eq!(every.as_secs(), 300);
        // Never asked yet: the first fetch is not the periodic one.
        assert!(!refresh_due(None, start, every));
        assert!(!refresh_due(Some(start), start + every / 2, every));
        assert!(refresh_due(Some(start), start + every, every));
        assert!(refresh_due(Some(start), start + every * 3, every));
    }

    #[test]
    fn the_age_of_the_list_is_said_in_plain_words() {
        let secs = std::time::Duration::from_secs;
        assert_eq!(checked_text(secs(5)), "Checked just now");
        assert_eq!(checked_text(secs(60)), "Checked 1 min ago");
        assert_eq!(checked_text(secs(59 * 60)), "Checked 59 min ago");
        assert_eq!(checked_text(secs(3 * 3600 + 20)), "Checked 3 h ago");
    }

    use super::*;
    use crate::test_dir::TestDir;

    fn entry(id: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "id": id, "title": id, "short_description": "Test tool", "publisher": "Toolboks",
            "category": "system", "icon": "box", "accent": "#8b5cf6", "kind": "declarative",
            "version": "1.0.0", "min_toolboks_version": "0.1.0", "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": "https://github.com/Gusseti/test/releases/download/test/sample.tbaddon",
            "size": 123, "sha256": "00".repeat(32), "verified": true, "channel": "stable"
        })).unwrap()
    }

    fn root(name: &str) -> TestDir {
        TestDir::new(&format!("browser-{name}"))
    }

    #[test]
    fn selection_starts_empty_and_keeps_the_approved_order() {
        let mut browser = Marketplace::default();
        browser
            .entries
            .extend([entry("test.memory-cleaner"), entry("test.resource-monitor")]);
        assert!(browser.selected_entries().is_empty());
        browser.select("test.memory-cleaner", true);
        browser.select("test.resource-monitor", true);
        assert_eq!(
            browser
                .selected_entries()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["test.resource-monitor", "test.memory-cleaner"]
        );
        browser.select("test.resource-monitor", false);
        assert_eq!(browser.selected_entries().len(), 1);
    }

    #[test]
    fn queue_status_and_failure_stay_until_explicitly_dismissed() {
        let mut browser = Marketplace::default();
        browser.queue(vec![entry("test.one"), entry("test.two")]);
        browser.queue(vec![entry("test.one")]);
        assert_eq!(browser.queue.len(), 2);
        assert!(browser.busy());
        browser.dismiss("test.one");
        assert_eq!(browser.jobs.len(), 2);
        browser.jobs.get_mut("test.one").unwrap().status =
            InstallStatus::Failed("Interrupted".into());
        browser.cancel("test.two");
        assert!(!browser.busy());
        assert!(matches!(
            browser.jobs["test.one"].status,
            InstallStatus::Failed(_)
        ));
        browser.dismiss("test.two");
        assert!(browser.jobs.contains_key("test.one"));
        browser.dismiss("test.one");
        assert!(browser.jobs.is_empty());
    }

    #[test]
    fn failed_and_interrupted_installs_survive_restart_until_dismissed() {
        let root = root("remember");
        let mut browser = Marketplace::default();
        browser.queue(vec![entry("test.one"), entry("test.two")]);
        browser.jobs.get_mut("test.one").unwrap().status =
            InstallStatus::Failed("Interrupted".into());
        browser.save_status(&root);
        let mut restored = Marketplace::restore(&root);
        assert!(restored
            .jobs
            .values()
            .all(|job| matches!(job.status, InstallStatus::Failed(_))));
        restored.dismiss("test.one");
        restored.save_status(&root);
        assert!(!Marketplace::restore(&root).jobs.contains_key("test.one"));
        assert!(Marketplace::restore(&root).jobs.contains_key("test.two"));
    }

    #[test]
    fn progress_checking_ready_and_cancelled_are_distinct() {
        let root = root("progress");
        let ctx = eframe::egui::Context::default();
        let mut browser = Marketplace::default();
        browser.jobs.insert(
            "test.one".into(),
            Job {
                entry: entry("test.one"),
                status: InstallStatus::Waiting,
            },
        );
        browser
            .sender
            .send(Message::Progress("test.one".into(), 30, 100))
            .unwrap();
        browser.poll(&root, &ctx, &BTreeSet::new());
        assert_eq!(
            browser.jobs["test.one"].status,
            InstallStatus::Downloading {
                done: 30,
                total: 100
            }
        );
        browser
            .sender
            .send(Message::Progress("test.one".into(), 100, 100))
            .unwrap();
        browser.poll(&root, &ctx, &BTreeSet::new());
        assert_eq!(browser.jobs["test.one"].status, InstallStatus::Checking);
        browser
            .sender
            .send(Message::Installed(
                "test.one".into(),
                Ok(installer::Installed {
                    version: "1.0.0".into(),
                    previous: None,
                }),
            ))
            .unwrap();
        assert!(browser.poll(&root, &ctx, &BTreeSet::new()));
        assert_eq!(browser.jobs["test.one"].status, InstallStatus::Ready);
        browser
            .sender
            .send(Message::Installed(
                "test.one".into(),
                Err(installer::InstallError::Cancelled),
            ))
            .unwrap();
        browser.poll(&root, &ctx, &BTreeSet::new());
        assert_eq!(browser.jobs["test.one"].status, InstallStatus::Cancelled);
    }

    #[test]
    fn installed_process_must_stop_before_an_update_starts() {
        let root = root("running");
        let mut browser = Marketplace::default();
        browser.queue(vec![entry("test.one")]);
        browser.poll(
            &root,
            &eframe::egui::Context::default(),
            &BTreeSet::from(["test.one".into()]),
        );
        assert!(browser.active.is_none());
        assert_eq!(browser.queue.len(), 1);
        assert_eq!(browser.jobs["test.one"].status, InstallStatus::Waiting);
    }

    #[test]
    fn update_is_primary_only_for_a_newer_version_and_unknown_access_blocks_install() {
        let mut sample = entry("test.one");
        assert!(update_available(Some("0.9.0"), &sample));
        assert!(!update_available(Some("1.0.0"), &sample));
        assert!(!update_available(None, &sample));
        sample.capabilities.push("unknown".into());
        assert!(!can_install(&sample));
    }

    #[test]
    fn developer_integration_is_explained_and_installable() {
        let mut sample = entry("test.one");
        sample.capabilities = vec!["developer_integration".into()];
        assert!(can_install(&sample));
        assert!(access("developer_integration").contains("priority and CPU cores"));
        let manifest_text = package_access_text(&toolboks_addon_sdk::Manifest {
            capabilities: vec![toolboks_addon_sdk::Capability::DeveloperIntegration],
            ..toolboks_addon_sdk::Package::parse(
                crate::addons::SDK_EXAMPLE.as_bytes(),
                env!("CARGO_PKG_VERSION"),
            )
            .unwrap()
            .manifest
        });
        assert_eq!(
            manifest_text,
            [AccessLine::Can(access("developer_integration"))]
        );
    }

    #[test]
    fn review_text_lists_the_data_note_under_the_capabilities() {
        let note = "Keeps your last scan of each drive on this PC (file and folder names).";
        let mut sample = entry("test.disk");
        sample.capabilities = vec!["filesystem_read".into(), "local_preferences".into()];
        sample.kind = catalog::Kind::Native;
        // Without a note nothing extra is shown.
        assert_eq!(
            access_text(&sample),
            [
                AccessLine::Can(access("filesystem_read")),
                AccessLine::Can(access("local_preferences")),
                AccessLine::Native,
            ]
        );
        sample.data_note = Some(note.into());
        let lines = access_text(&sample);
        assert_eq!(
            lines,
            [
                AccessLine::Can(access("filesystem_read")),
                AccessLine::Can(access("local_preferences")),
                AccessLine::Data(note.into()),
                AccessLine::Native,
            ]
        );
        assert_eq!(lines[2].text(), note);
        // An add-on with no capabilities still shows its note.
        sample.capabilities.clear();
        sample.kind = catalog::Kind::Declarative;
        assert_eq!(
            access_text(&sample),
            [AccessLine::Nothing, AccessLine::Data(note.into())]
        );
        // A note that breaks the rule (only possible in a file changed on disk) is not shown.
        for bad in ["", "  ", "two\nlines"] {
            sample.data_note = Some(bad.into());
            assert_eq!(access_text(&sample), [AccessLine::Nothing]);
        }
        sample.data_note = Some("x".repeat(161));
        assert_eq!(access_text(&sample), [AccessLine::Nothing]);
        assert_eq!(shown_data_note(Some(note)), Some(note));
        assert_eq!(shown_data_note(None), None);
    }

    #[test]
    fn local_package_review_shows_the_data_note_after_the_capabilities() {
        let note = "Keeps a list of recent files on this PC.";
        let mut package = toolboks_addon_sdk::Package::parse(
            crate::addons::SDK_EXAMPLE.as_bytes(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        let capabilities: Vec<AccessLine> = package_access_text(&package.manifest);
        assert_eq!(capabilities.len(), package.manifest.capabilities.len());
        assert!(capabilities
            .iter()
            .all(|line| matches!(line, AccessLine::Can(_))));
        // A package that passed validation with a note shows it, after the capabilities.
        package.manifest.data_note = Some(note.into());
        let bytes = serde_json::to_vec(&package).unwrap();
        let package =
            toolboks_addon_sdk::Package::parse(&bytes, env!("CARGO_PKG_VERSION")).unwrap();
        let mut expected = capabilities.clone();
        expected.push(AccessLine::Data(note.into()));
        assert_eq!(package_access_text(&package.manifest), expected);
        // No capabilities: only the note.
        let mut bare = package.manifest.clone();
        bare.capabilities.clear();
        assert_eq!(package_access_text(&bare), [AccessLine::Data(note.into())]);
        // A note that breaks the rule is never shown.
        bare.data_note = Some("Keeps \u{202e}selif".into());
        assert!(package_access_text(&bare).is_empty());
    }

    #[test]
    fn a_catalog_entry_with_a_data_note_reads_and_old_ones_still_do() {
        let old = entry("test.old");
        assert_eq!(old.data_note, None);
        let mut json = serde_json::to_value(&old).unwrap();
        json["data_note"] = "Keeps a list of recent files on this PC.".into();
        let new: Entry = serde_json::from_value(json).unwrap();
        assert_eq!(
            new.data_note.as_deref(),
            Some("Keeps a list of recent files on this PC.")
        );
        assert_eq!(new.validate(), Ok(()));
    }

    #[test]
    fn removing_a_local_package_keeps_data_unless_explicitly_requested() {
        for delete_data in [false, true] {
            let root = root(if delete_data {
                "delete-data"
            } else {
                "keep-data"
            });
            let mut registry = crate::addons::Registry::load(root.to_path_buf());
            let package = toolboks_addon_sdk::Package::parse(
                crate::addons::SDK_EXAMPLE.as_bytes(),
                env!("CARGO_PKG_VERSION"),
            )
            .unwrap();
            let id = package.manifest.id.clone();
            registry.install(package).unwrap();
            registry
                .config
                .preferences
                .entry(id.clone())
                .or_default()
                .insert("example".into(), serde_json::json!(true));
            let data = installer::data_dir(&root, &id);
            std::fs::create_dir_all(&data).unwrap();
            std::fs::write(data.join("sample"), b"saved data").unwrap();
            registry.remove(&id, !delete_data).unwrap();
            assert_eq!(data.exists(), !delete_data);
            assert_eq!(
                crate::addons::Registry::load(root.to_path_buf())
                    .config
                    .preferences
                    .contains_key(&id),
                !delete_data
            );
        }
    }
}
impl InstallStatus {
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Waiting | Self::Downloading { .. } | Self::Checking
        )
    }
    pub fn label(&self) -> String {
        match self {
            Self::Waiting => "Waiting to install".into(),
            Self::Downloading { done, total } => {
                format!("Downloading {} of {}", size(*done), size(*total))
            }
            Self::Checking => "Checking and installing".into(),
            Self::Ready => "Ready to open".into(),
            Self::Cancelled => "Download cancelled".into(),
            Self::Failed(_) => "Could not install".into(),
        }
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Job {
    pub entry: Entry,
    pub status: InstallStatus,
}
enum Message {
    Catalog(Result<Index, String>),
    Progress(String, u64, u64),
    Installed(
        String,
        Result<installer::Installed, installer::InstallError>,
    ),
    Image(String, Result<Vec<u8>, String>),
}
/// How often the add-on list is fetched again while the app is open.
pub const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// True when the list was last asked for at least `every` ago. `last_attempt` is when the last
/// fetch started, whether it worked or not, so a failing connection is retried on the same
/// schedule instead of on every frame.
pub fn refresh_due(
    last_attempt: Option<std::time::Instant>,
    now: std::time::Instant,
    every: std::time::Duration,
) -> bool {
    last_attempt.is_some_and(|at| now.saturating_duration_since(at) >= every)
}

/// "Checked just now", "Checked 3 min ago", "Checked 2 h ago" for a list fetched `elapsed` ago.
pub fn checked_text(elapsed: std::time::Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "Checked just now".into(),
        1..=59 => format!("Checked {minutes} min ago"),
        _ => format!("Checked {} h ago", minutes / 60),
    }
}

pub struct Marketplace {
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    pub loading: bool,
    pub checked: bool,
    /// When the list was last fetched successfully.
    pub last_checked: Option<std::time::Instant>,
    /// When the last fetch started (worked or not).
    pub last_attempt: Option<std::time::Instant>,
    /// A background fetch is running; its failure is ignored and keeps the list as it is.
    quiet_pending: bool,
    pub selected: BTreeSet<String>,
    pub category: String,
    pub installed_only: bool,
    pub focused: Option<String>,
    pub details: bool,
    pub review: Option<Vec<Entry>>,
    pub remove: Option<(String, bool)>,
    pub jobs: BTreeMap<String, Job>,
    pub images: BTreeMap<String, Result<Vec<u8>, String>>,
    image_pending: BTreeSet<String>,
    queue: VecDeque<Entry>,
    active: Option<(String, Arc<AtomicBool>)>,
    sender: mpsc::Sender<Message>,
    receiver: mpsc::Receiver<Message>,
    persisted: Vec<u8>,
}
impl Default for Marketplace {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            entries: vec![],
            error: None,
            loading: false,
            checked: false,
            last_checked: None,
            last_attempt: None,
            quiet_pending: false,
            selected: BTreeSet::new(),
            category: "All".into(),
            installed_only: false,
            focused: None,
            details: false,
            review: None,
            remove: None,
            jobs: BTreeMap::new(),
            images: BTreeMap::new(),
            image_pending: BTreeSet::new(),
            queue: VecDeque::new(),
            active: None,
            sender,
            receiver,
            persisted: vec![],
        }
    }
}
impl Drop for Marketplace {
    fn drop(&mut self) {
        if let Some((_, cancel)) = &self.active {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
pub fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
/// The install review line for `developer_integration`, shared by the catalog and local-package reviews.
const DEVELOPER_INTEGRATION_ACCESS: &str = "Developer tools integration: runs a local job queue on this PC, changes the priority and CPU cores of build and test processes or ends them, and installs scripts into Claude Code's settings";
pub fn access(name: &str) -> String {
    match name {
        "host_metrics" => "Read memory use and uptime",
        "local_preferences" => "Keep its own settings on this PC",
        "filesystem_read" => "Read file and folder details in locations you select",
        "user_exports" => "Save reports where you choose",
        "drive_optimize" => {
            "Analyze and optimize drives when you ask (administrator permission required)"
        }
        "live_audio" => "Use your selected microphone and audio outputs",
        "system_tuning" => "Back up and change the Windows settings and power plan you select",
        "input_automation" => "Record and replay keyboard and mouse actions when you start a macro",
        "developer_integration" => DEVELOPER_INTEGRATION_ACCESS,
        "administrator" => "Ask Windows for administrator permission for this add-on",
        _ => return format!("Unrecognized access: {name}. Update Toolboks before installing."),
    }
    .into()
}
/// One line under "It can" in the install review and the add-on details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessLine {
    /// A capability, in plain words (drawn with a check).
    Can(String),
    /// The add-on asks for no capabilities.
    Nothing,
    /// The add-on's own note about data it keeps on this PC (drawn with a drive).
    Data(String),
    /// A native add-on runs a program on the PC.
    Native,
}
impl AccessLine {
    pub fn text(&self) -> &str {
        match self {
            Self::Can(text) | Self::Data(text) => text,
            Self::Nothing => "No extra host access requested.",
            Self::Native => "Runs a program on your PC, not in a security sandbox.",
        }
    }
}
/// A data note as it may be shown: only one that passes the catalog and manifest rule. The
/// catalog and packages are checked when they are read, so this only drops a note in a file that
/// was changed on disk afterwards.
pub fn shown_data_note(note: Option<&str>) -> Option<&str> {
    note.filter(|note| toolboks_addon_sdk::valid_data_note(note))
}
/// What the install review and the details list under "It can", in order: the capabilities, then
/// the add-on's data note when it has one, then the native-program line.
pub fn access_text(entry: &Entry) -> Vec<AccessLine> {
    let mut lines: Vec<AccessLine> = entry
        .capabilities
        .iter()
        .map(|name| AccessLine::Can(access(name)))
        .collect();
    if lines.is_empty() {
        lines.push(AccessLine::Nothing);
    }
    if let Some(note) = shown_data_note(entry.data_note.as_deref()) {
        lines.push(AccessLine::Data(note.into()));
    }
    if entry.kind == catalog::Kind::Native {
        lines.push(AccessLine::Native);
    }
    lines
}
/// What the review before installing a local `.tbaddon` lists under "It asks to": one line per
/// capability (none when it asks for none), then the package's data note when it has one.
pub fn package_access_text(manifest: &toolboks_addon_sdk::Manifest) -> Vec<AccessLine> {
    use toolboks_addon_sdk::Capability;
    let mut lines: Vec<AccessLine> = manifest
        .capabilities
        .iter()
        .map(|capability| {
            AccessLine::Can(
                match capability {
                    Capability::Administrator => "Make administrator changes only after you confirm in a separate window",
                    Capability::HostMetrics => "Read basic memory and uptime readings",
                    Capability::LocalPreferences => "Save its own preferences locally",
                    Capability::FilesystemRead => "Read file and folder metadata for locations you select",
                    Capability::UserExports => "Save reports to a destination you choose",
                    Capability::DriveOptimize => "Analyze fragmentation and run Windows defrag or TRIM only when you request it (administrator access required)",
                    Capability::LiveAudio => "Open the selected microphone and route live audio to outputs you enable",
                    Capability::SystemTuning => "Back up and change the Windows settings and power plan you select",
                    Capability::InputAutomation => "Record keyboard and mouse input and replay it only when you start a macro",
                    Capability::DeveloperIntegration => DEVELOPER_INTEGRATION_ACCESS,
                }
                .into(),
            )
        })
        .collect();
    if let Some(note) = shown_data_note(manifest.data_note.as_deref()) {
        lines.push(AccessLine::Data(note.into()));
    }
    lines
}
pub fn can_install(entry: &Entry) -> bool {
    entry.check(&catalog::host()).is_ok()
        && entry
            .capabilities
            .iter()
            .all(|name| !access(name).starts_with("Unrecognized"))
}
pub fn update_available(installed: Option<&str>, entry: &Entry) -> bool {
    installed.is_some_and(|old| {
        toolboks_addon_sdk::version(old) < toolboks_addon_sdk::version(&entry.version)
    })
}
/// The listed add-ons that have a newer version than the installed one, in list order.
/// `installed` gives the installed version of an id, if it is installed.
pub fn pending_updates(
    entries: &[Entry],
    installed: impl Fn(&str) -> Option<String>,
) -> Vec<Entry> {
    entries
        .iter()
        .filter(|entry| {
            can_install(entry) && update_available(installed(&entry.id).as_deref(), entry)
        })
        .cloned()
        .collect()
}
/// The line at the top of Home when updates wait: "Update available: Audio Gainer" or
/// "2 updates available: Audio Gainer, Disk Manager".
pub fn update_line(titles: &[String]) -> String {
    match titles {
        [] => String::new(),
        [one] => format!("Update available: {one}"),
        many => format!("{} updates available: {}", many.len(), many.join(", ")),
    }
}
pub fn pick_rank(entry: &Entry) -> usize {
    let suffix = entry.id.rsplit('.').next().unwrap_or(&entry.id);
    PICK_ORDER
        .iter()
        .position(|id| *id == suffix)
        .unwrap_or(PICK_ORDER.len())
}

fn fetched(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    fetched_as(url, "application/octet-stream", limit)
}
fn fetched_as(url: &str, accept: &str, limit: usize) -> Result<Vec<u8>, String> {
    if !crate::update::is_trusted_url(url) {
        return Err("The download address is not trusted.".into());
    }
    let response = winhttp::get(url, accept, limit, Duration::from_secs(30)).map_err(|_| {
        "Could not reach the add-on list. Your installed tools still work.".to_string()
    })?;
    if !crate::update::is_trusted_url(&response.final_url) {
        return Err("The download moved to an untrusted address.".into());
    }
    Ok(response.body)
}
fn load_catalog(root: &Path) -> Result<Index, String> {
    #[cfg(feature = "preview")]
    if let Some(folder) = fixture() {
        let bytes = std::fs::read(folder.join("index.json")).map_err(|error| error.to_string())?;
        return catalog::accept(root, &bytes)
            .map_err(|error| format!("Could not read the test add-on list: {error}"));
    }
    // The API first: it is fresh within a minute, the plain file address can be five minutes behind.
    if let Some(index) = fetched_as(
        catalog::FRESH_URL,
        "application/vnd.github.raw",
        toolboks_catalog::MAX_INDEX_BYTES,
    )
    .ok()
    .and_then(|bytes| catalog::accept(root, &bytes).ok())
    {
        return Ok(index);
    }
    let bytes = fetched(catalog::URL, toolboks_catalog::MAX_INDEX_BYTES)?;
    catalog::accept(root, &bytes)
        .map_err(|_| "Could not read the add-on list. Try again later.".into())
}
#[cfg(feature = "preview")]
fn fixture() -> Option<PathBuf> {
    std::env::var_os("TOOLBOKS_TEST_CATALOG").map(PathBuf::from)
}
#[cfg(feature = "preview")]
struct FixtureFetch(PathBuf);
#[cfg(feature = "preview")]
impl installer::Fetch for FixtureFetch {
    fn fetch(
        &self,
        url: &str,
        file: &Path,
        options: winhttp::Download,
    ) -> Result<winhttp::Saved, winhttp::HttpError> {
        use std::io::{Read, Write};
        let name = url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !name.contains(['\\', ':', '%']) && *name != "..")
            .ok_or_else(|| winhttp::HttpError::Other("Invalid fixture file".into()))?;
        let mut source = std::fs::File::open(self.0.join(name))
            .map_err(|error| winhttp::HttpError::Other(error.to_string()))?;
        let mut destination = std::fs::File::create(file)
            .map_err(|error| winhttp::HttpError::Other(error.to_string()))?;
        let mut done = 0;
        let mut chunk = [0; 16384];
        loop {
            if options.cancel.load(Ordering::Relaxed) {
                return Err(winhttp::HttpError::Cancelled);
            }
            let read = source
                .read(&mut chunk)
                .map_err(|error| winhttp::HttpError::Other(error.to_string()))?;
            if read == 0 {
                break;
            }
            done += read as u64;
            if done > options.max_bytes {
                return Err(winhttp::HttpError::Other("Fixture too large".into()));
            }
            destination
                .write_all(&chunk[..read])
                .map_err(|error| winhttp::HttpError::Other(error.to_string()))?;
            (options.progress)(done);
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(winhttp::Saved {
            final_url: url.into(),
            resumed: false,
            len: done,
        })
    }
}
impl Marketplace {
    pub fn restore(root: &Path) -> Self {
        let mut browser = Self::default();
        if let Ok(bytes) = std::fs::read(root.join("addon-install-status.json")) {
            if let Ok(jobs) = serde_json::from_slice::<BTreeMap<String, Job>>(&bytes) {
                browser.jobs = jobs;
                for job in browser.jobs.values_mut() {
                    if job.status.busy() {
                        job.status = InstallStatus::Failed(
                            "The last install was interrupted. Try again or dismiss it.".into(),
                        );
                    }
                }
            }
        }
        browser
    }

    fn save_status(&mut self, root: &Path) {
        let mut jobs = self.jobs.clone();
        for job in jobs.values_mut() {
            if job.status.busy() {
                job.status = InstallStatus::Waiting;
            }
        }
        if let Ok(bytes) = serde_json::to_vec(&jobs) {
            if self.persisted != bytes {
                match crate::addons::write_atomic(&root.join("addon-install-status.json"), &bytes) {
                    Ok(()) => self.persisted = bytes,
                    Err(error) => {
                        self.error = Some(format!("Could not remember install status: {error}"))
                    }
                }
            }
        }
    }
    pub fn refresh(&mut self, root: PathBuf, ctx: &eframe::egui::Context) {
        if self.loading || self.busy() || self.review.is_some() {
            return;
        }
        self.loading = true;
        self.checked = true;
        self.error = None;
        self.last_attempt = Some(std::time::Instant::now());
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Message::Catalog(load_catalog(&root)));
            ctx.request_repaint();
        });
    }
    /// The periodic fetch: nothing on screen changes while it runs, and when it fails the list the
    /// user already has stays exactly as it is (no error, no emptied list, no closed review).
    pub fn refresh_quietly(&mut self, root: PathBuf, ctx: &eframe::egui::Context) {
        if self.loading || self.quiet_pending || self.busy() || self.review.is_some() {
            return;
        }
        self.quiet_pending = true;
        self.last_attempt = Some(std::time::Instant::now());
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Message::Catalog(load_catalog(&root)));
            ctx.request_repaint();
        });
    }
    pub fn refresh_due(&self, now: std::time::Instant) -> bool {
        refresh_due(self.last_attempt, now, REFRESH_EVERY)
    }
    pub fn select(&mut self, id: &str, selected: bool) {
        if selected {
            self.selected.insert(id.into());
        } else {
            self.selected.remove(id);
        }
    }
    pub fn selected_entries(&self) -> Vec<Entry> {
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| self.selected.contains(&entry.id) && can_install(entry))
            .cloned()
            .collect();
        entries.sort_by_key(pick_rank);
        entries
    }
    pub fn busy(&self) -> bool {
        self.jobs.values().any(|job| job.status.busy())
    }
    pub fn queue(&mut self, entries: Vec<Entry>) {
        for entry in entries {
            if self
                .jobs
                .get(&entry.id)
                .is_some_and(|job| job.status.busy())
            {
                continue;
            }
            self.jobs.insert(
                entry.id.clone(),
                Job {
                    entry: entry.clone(),
                    status: InstallStatus::Waiting,
                },
            );
            self.selected.remove(&entry.id);
            self.queue.push_back(entry);
        }
    }
    pub fn dismiss(&mut self, id: &str) {
        if self.jobs.get(id).is_some_and(|job| !job.status.busy()) {
            self.jobs.remove(id);
        }
    }
    pub fn cancel(&mut self, id: &str) {
        if let Some((active, cancel)) = &self.active {
            if active == id {
                cancel.store(true, Ordering::Relaxed);
                return;
            }
        }
        self.queue.retain(|entry| entry.id != id);
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = InstallStatus::Cancelled;
        }
    }
    pub fn image(&mut self, url: &str, ctx: &eframe::egui::Context) {
        if self.images.contains_key(url) || !self.image_pending.insert(url.into()) {
            return;
        }
        let url = url.to_string();
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = fetched(&url, 8 * 1024 * 1024);
            let _ = sender.send(Message::Image(url, result));
            ctx.request_repaint();
        });
    }
    pub fn poll(
        &mut self,
        root: &Path,
        ctx: &eframe::egui::Context,
        running: &BTreeSet<String>,
    ) -> bool {
        let mut changed = false;
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Catalog(result) => {
                    self.loading = false;
                    let quiet = std::mem::take(&mut self.quiet_pending);
                    match result {
                        Err(_) if quiet => {}
                        Ok(index) => {
                            self.last_checked = Some(std::time::Instant::now());
                            self.entries = index
                                .listing(&catalog::host())
                                .into_iter()
                                .cloned()
                                .collect();
                            self.selected
                                .retain(|id| self.entries.iter().any(|entry| &entry.id == id));
                            let _ = installer::record_update_check(root, catalog::now());
                        }
                        Err(error) => {
                            self.entries.clear();
                            self.selected.clear();
                            self.review = None;
                            self.error = Some(error);
                        }
                    }
                }
                Message::Progress(id, done, total) => {
                    if let Some(job) = self.jobs.get_mut(&id) {
                        job.status = if done == total {
                            InstallStatus::Checking
                        } else {
                            InstallStatus::Downloading { done, total }
                        };
                    }
                }
                Message::Installed(id, result) => {
                    self.active = None;
                    if let Some(job) = self.jobs.get_mut(&id) {
                        job.status = match result {
                            Ok(_) => {
                                changed = true;
                                InstallStatus::Ready
                            }
                            Err(installer::InstallError::Cancelled) => InstallStatus::Cancelled,
                            Err(error) => InstallStatus::Failed(install_failure(&error)),
                        };
                    }
                }
                Message::Image(url, bytes) => {
                    self.image_pending.remove(&url);
                    self.images.insert(url, bytes);
                }
            }
        }
        if self.active.is_none() {
            if let Some(entry) = self.queue.pop_front() {
                if running.contains(&entry.id) {
                    self.queue.push_front(entry);
                    self.save_status(root);
                    ctx.request_repaint_after(Duration::from_millis(100));
                    return changed;
                }
                let cancel = Arc::new(AtomicBool::new(false));
                self.active = Some((entry.id.clone(), cancel.clone()));
                self.jobs.get_mut(&entry.id).unwrap().status = InstallStatus::Downloading {
                    done: 0,
                    total: entry.size,
                };
                let sender = self.sender.clone();
                let root = root.to_path_buf();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let mut progress = |done, total| {
                        let _ = sender.send(Message::Progress(entry.id.clone(), done, total));
                        ctx.request_repaint();
                    };
                    let fetch: Box<dyn installer::Fetch> = {
                        #[cfg(feature = "preview")]
                        if let Some(folder) = fixture() {
                            Box::new(FixtureFetch(folder))
                        } else {
                            Box::new(installer::WinHttp)
                        }
                        #[cfg(not(feature = "preview"))]
                        {
                            Box::new(installer::WinHttp)
                        }
                    };
                    let result = installer::install(
                        &root,
                        &entry,
                        &catalog::host(),
                        fetch.as_ref(),
                        false,
                        &cancel,
                        &mut progress,
                    );
                    let _ = sender.send(Message::Installed(entry.id, result));
                    ctx.request_repaint();
                });
            }
        }
        if self.busy() || self.loading {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.save_status(root);
        changed
    }
}

fn install_failure(error: &installer::InstallError) -> String {
    let message = match error {
        installer::InstallError::Mismatch(_) => "The download could not be checked. Download it again.",
        installer::InstallError::Interrupted(_) => "The download was interrupted. Check your connection and try again.",
        installer::InstallError::Untrusted => "The download address is not trusted. Refresh the add-on list and try again.",
        installer::InstallError::BadPackage(_) => "The package does not match the reviewed add-on. Refresh the list and download it again.",
        installer::InstallError::Hidden(_) => "Update Toolboks before installing this add-on.",
        _ => return format!("{error} Your previous version, if installed, is unchanged."),
    };
    format!(
        "{message} Nothing new was installed. Your previous version, if installed, is unchanged."
    )
}
