//! The Debloater engine without its UI: the curated catalog, the registry backend, the
//! plan/apply logic, and the apps, restore point, services, startup and scheduled task backends. Shared by the Debloater page and the Profiles add-on, so neither has to
//! import the other's host code.

#![cfg(windows)]

pub mod apps;
pub mod backend;
pub mod catalog;
mod change;
pub mod model;
pub mod plan;
pub mod restore;
pub mod services;
pub mod startup;
pub mod tasks;

pub use change::{rollback, ChangeError};
