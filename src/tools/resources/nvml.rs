//! SKELETON: the NVML builder replaces the bodies. Keep these public types and signatures.
#![allow(dead_code)]

/// Extras only NVIDIA's driver library can give. Every field is `None` when unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NvmlReading {
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    /// Current graphics and memory clocks in MHz.
    pub clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
}

/// A loaded `nvml.dll` with the first NVIDIA device opened.
pub struct Nvml;

impl Nvml {
    /// `None` when there is no `nvml.dll`, it fails to initialise, or no device exists.
    pub fn open() -> Option<Nvml> {
        None
    }

    pub fn read(&self) -> Option<NvmlReading> {
        None
    }
}
