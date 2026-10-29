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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    PrivacyAds,
    /// Apps are handled by `apps.rs`, not by registry items, so no `Item` uses this yet.
    #[allow(dead_code)]
    Apps,
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
