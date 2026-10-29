//! SKELETON: the backend builder replaces the body of `breakdown` and adds the helpers. Keep the
//! public types and the function signature exactly; the Home card is being written against them.
#![allow(dead_code)]

use super::stats::ProcInfo;

/// One colored piece of the "Where your memory goes" bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// Display name, e.g. "Claude", "Chrome", "Other apps", "Windows and other".
    pub name: String,
    /// Memory in bytes after scaling to fit inside the memory in use.
    pub bytes: u64,
    /// Number of processes merged into this segment (0 for the leftover part).
    pub count: usize,
    /// Share of the memory in use, 0 to 100.
    pub percent: f32,
    /// RGB color.
    pub color: [u8; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    /// Memory in use in bytes (what 100 % of the bar means).
    pub in_use: u64,
    /// Largest first; "Other apps" and "Windows and other" come last, in that order.
    pub segments: Vec<Segment>,
}

/// Groups the running processes into colored segments that fill `in_use` bytes.
pub fn breakdown(_procs: &[ProcInfo], in_use: u64) -> Breakdown {
    Breakdown {
        in_use,
        segments: Vec::new(),
    }
}
