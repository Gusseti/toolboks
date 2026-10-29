//! SKELETON: the GPU builder replaces the bodies. Keep these public types and signatures.
#![allow(dead_code)]

/// Basic readings for the main GPU, from sysinfo's `gpu` feature plus the Windows GPU counters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuBasic {
    pub name: String,
    pub vendor: Option<String>,
    /// Busiest engine, 0 to 100 (what Task Manager shows as GPU load).
    pub load: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// Load per engine type, e.g. ("3D", 21.0), ("Copy", 0.0), ("Video Decode", 3.0), 0 to 100.
    pub engines: Vec<(String, f32)>,
}

pub struct GpuReader;

impl GpuReader {
    pub fn new() -> Self {
        GpuReader
    }

    /// One reading, or `None` if there is no usable GPU. Called about once a second from the
    /// sampler thread, never from the UI thread.
    pub fn read(&mut self) -> Option<GpuBasic> {
        None
    }
}
