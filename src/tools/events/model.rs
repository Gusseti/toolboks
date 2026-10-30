//! Data types of Event Watch. Pure data, no Windows calls. FINAL contract: the builders write the
//! source, the catalog, the analysis and the page against these types in parallel; do not change
//! them without telling the orchestrator.

/// Windows event level: 1 Critical, 2 Error, 3 Warning, 4 Information, 5 Verbose, 0 LogAlways.
pub type Level = u8;

/// One Windows Event Log record, kept in memory only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub record_id: u64,
    /// UTC time as milliseconds since 1970-01-01.
    pub time_ms: i64,
    pub channel: String,
    pub provider: String,
    pub id: u32,
    pub level: Level,
    /// Named `EventData` / `UserData` values in order, e.g. `("BugcheckCode", "313")`. Unnamed
    /// values use the name `"Data<N>"`.
    pub data: Vec<(String, String)>,
    /// Localised message text, filled lazily (only when asked for); never used for logic.
    pub message: Option<String>,
}

impl Event {
    pub fn value(&self, name: &str) -> Option<&str> {
        self.data
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    NeedsAttention,
    WorthWatching,
    Harmless,
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Disk,
    Graphics,
    /// WHEA and other hardware error reports.
    Hardware,
    Memory,
    Power,
    Filesystem,
    /// Reserved for driver events once the catalog knows more of them.
    #[allow(dead_code)]
    Driver,
    Software,
    System,
    Other,
}

/// How the page describes an event, from the catalog (provider + id, never the message text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub category: Category,
    pub severity: Severity,
    /// One plain sentence: "Your graphics driver stopped responding and recovered."
    pub headline: &'static str,
    /// What it usually means / what to do, honest and short. May be empty.
    pub hint: &'static str,
    /// True if this entry came from the verified catalog, false for the generic fallback.
    pub known: bool,
}

/// What kind of stop a restart was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrashKind {
    BlueScreen {
        code: u32,
        params: [u64; 4],
    },
    /// The power button was held (Kernel-Power 41 with a PowerButtonTimestamp).
    ForcedRestart,
    /// Everything zero: power loss, a hard hang, or a bug check that could not be written.
    PowerLossOrHang,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crash {
    /// When the system noticed (Kernel-Power 41 `TimeCreated`, i.e. at the next boot).
    pub noticed_ms: i64,
    /// Best estimate of when it stopped (6008 time, else last record before the boot, else
    /// `noticed_ms`).
    pub stopped_ms: i64,
    pub kind: CrashKind,
    /// Dump path from event 1001 when present.
    pub dump_path: Option<String>,
    /// Events in the window before `stopped_ms` (oldest first), warnings and errors only.
    pub before: Vec<Event>,
}

/// Repeated events folded into one row ("Disk 0 retried reads x143").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub provider: String,
    pub id: u32,
    pub subject: Option<String>,
    pub count: usize,
    pub first_ms: i64,
    pub last_ms: i64,
    pub sample: Event,
    pub info: Info,
}

/// One line of the Summary page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestLine {
    pub severity: Severity,
    pub text: String,
}

/// Where a channel stands (for the "log covers from ..." line and access problems).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelStatus {
    pub name: String,
    pub readable: bool,
    pub error: Option<String>,
    pub oldest_ms: Option<i64>,
}
