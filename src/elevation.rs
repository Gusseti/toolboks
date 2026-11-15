//! Admin rights: detection lives in `toolboks_shared::elevation` and is re-exported here.
//! Toolboks itself never restarts as administrator; an add-on asks for rights through its own
//! separate helper.

pub use toolboks_shared::elevation::*;
