//! Code shared by the Toolboks host and its add-ons, so add-ons built as separate exes do not
//! import host code.

#[cfg(windows)]
pub mod elevation;
pub mod format;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(windows)]
pub mod win;
