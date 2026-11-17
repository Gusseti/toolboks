//! Data types of the debloater. Pure data, no Windows calls. FINAL contract: the engine and the
//! page are written against these types in parallel, do not change them without telling the
//! orchestrator.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Safe,
    Careful,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Edition {
    Unknown,
    Home,
    Pro,
    Enterprise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RegValue {
    Dword(u32),
    Sz(String),
}

/// One registry value that turns an unwanted feature off when it holds `off_value`.
/// A missing value means the Windows default, which counts as "feature still on".
#[derive(Clone, Copy, Debug)]
pub struct RegSetting {
    pub hive: Hive,
    pub path: &'static str,
    pub name: &'static str,
    pub off_value: u32,
    /// Lower values are already stricter than the requested setting.
    pub off_at_or_below: bool,
}

impl RegSetting {
    pub fn is_off(&self, value: Option<&RegValue>) -> bool {
        matches!(value, Some(RegValue::Dword(v))
            if *v == self.off_value || (self.off_at_or_below && *v < self.off_value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    PrivacyAds,
    /// Apps are handled by `apps.rs`, not by registry items, so no `Item` uses this yet.
    #[allow(dead_code)]
    Apps,
}

/// How well a speed or performance benefit is backed (issue #33). Only a named source or our own
/// controlled measurement may raise it above `Unproven`; never invent a benefit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// Our own controlled measurement shows a gain. Nothing qualifies yet.
    Measured,
    /// A named source or our own measurement shows a mixed result. Nothing qualifies yet.
    Mixed,
    /// The change claims or implies a speed or performance benefit and no controlled measurement
    /// shows one.
    Unproven,
    /// Chosen for privacy, fewer ads or housekeeping. It makes no speed claim.
    NotSpeed,
}

impl Evidence {
    /// The badge text.
    pub fn label(self) -> &'static str {
        match self {
            Evidence::Measured => "Measured",
            Evidence::Mixed => "Mixed evidence",
            Evidence::Unproven => "Unproven",
            Evidence::NotSpeed => "Not a speed tweak",
        }
    }

    /// Unproven and mixed changes are never ticked for the user.
    pub fn may_default_on(self) -> bool {
        !matches!(self, Evidence::Unproven | Evidence::Mixed)
    }
}

/// The evidence for one catalog entry: the level, one plain sentence why, and where it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceInfo {
    pub level: Evidence,
    /// One plain sentence.
    pub note: &'static str,
    /// A link (`https://...`) or a plain pointer such as a section of docs/RESEARCH-debloat.md.
    /// `None` means no source was found. Never a made-up address.
    pub source: Option<&'static str>,
    /// True only where we read the source against this claim. A link we only confirmed to exist
    /// is `false` and the page says so.
    pub source_checked: bool,
}

/// One thing the user can turn off.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    /// Plain words: what changes.
    pub what_changes: &'static str,
    /// Plain words: what could stop working, or "Nothing noticeable".
    pub could_break: &'static str,
    pub risk: Risk,
    /// Lowest Windows edition on which the change has an effect.
    pub min_edition: Edition,
    pub settings: &'static [RegSetting],
}

impl Item {
    /// What backs a speed claim for this item.
    pub fn evidence(&self) -> EvidenceInfo {
        crate::evidence::for_id(self.id)
    }

    /// True if any of its settings lives in HKEY_LOCAL_MACHINE.
    pub fn needs_admin(&self) -> bool {
        self.settings.iter().any(|s| s.hive == Hive::LocalMachine)
    }
}

/// Where an item stands right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The unwanted feature is still on (some setting is missing or not at its off value).
    Active,
    /// Already off (every setting holds its off value).
    Off,
    /// Could not be read.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicable {
    Yes,
    NeedsAdmin,
    EditionNotSupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub id: &'static str,
    pub state: State,
    pub applicable: Applicable,
}

/// What we know about the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env {
    pub edition: Edition,
    pub elevated: bool,
}
