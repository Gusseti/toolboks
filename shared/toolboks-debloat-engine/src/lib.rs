//! The Debloater engine without its UI: the curated catalog, the registry backend and the
//! plan/apply logic. Shared by the Debloater page and the Profiles add-on, so neither has to
//! import the other's host code.

#![cfg(windows)]

pub mod backend;
pub mod catalog;
pub mod model;
pub mod plan;
