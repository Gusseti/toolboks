//! Temperature, power and clocks from NVIDIA's management library, loaded at runtime.

use std::ffi::c_void;

use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// Extras only NVIDIA's driver library can give. Every field is `None` when unavailable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NvmlReading {
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    /// Current graphics and memory clocks in MHz.
    pub clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
}

type InitFn = unsafe extern "C" fn() -> u32;
type ShutdownFn = unsafe extern "C" fn() -> u32;
type HandleFn = unsafe extern "C" fn(u32, *mut *mut c_void) -> u32;
type TempFn = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32;
type PowerFn = unsafe extern "C" fn(*mut c_void, *mut u32) -> u32;
type ClockFn = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32;

const SENSOR_GPU: u32 = 0;
const CLOCK_GRAPHICS: u32 = 0;
const CLOCK_MEM: u32 = 2;

/// A loaded `nvml.dll` with the first NVIDIA device opened.
pub struct Nvml {
    lib: HMODULE,
    device: *mut c_void,
    shutdown: ShutdownFn,
    temp: TempFn,
    power: PowerFn,
    clock: ClockFn,
}

/// Resolve one export as a typed function pointer.
///
/// # Safety
/// `T` must be the exact function-pointer type of the export.
unsafe fn export<T: Copy>(lib: HMODULE, name: PCSTR) -> Option<T> {
    // SAFETY: `lib` is a live module handle and `name` is NUL-terminated.
    let p = unsafe { GetProcAddress(lib, name) }?;
    // SAFETY: caller guarantees `T` matches; both are pointer-sized fn pointers.
    Some(unsafe { std::mem::transmute_copy::<unsafe extern "system" fn() -> isize, T>(&p) })
}

fn milliwatts_to_watts(mw: u32) -> f32 {
    mw as f32 / 1000.0
}

impl Nvml {
    /// `None` when there is no `nvml.dll`, it fails to initialise, or no device exists.
    pub fn open() -> Option<Nvml> {
        // SAFETY: loading a system library by name.
        let lib = unsafe { LoadLibraryW(w!("nvml.dll")) }.ok()?;
        let free = |lib: HMODULE| {
            // SAFETY: `lib` was returned by LoadLibraryW and is freed once.
            let _ = unsafe { FreeLibrary(lib) };
        };
        // SAFETY: every alias below matches the documented NVML export signature.
        let fns = unsafe {
            (
                export::<InitFn>(lib, windows::core::s!("nvmlInit_v2")),
                export::<ShutdownFn>(lib, windows::core::s!("nvmlShutdown")),
                export::<HandleFn>(lib, windows::core::s!("nvmlDeviceGetHandleByIndex_v2")),
                export::<TempFn>(lib, windows::core::s!("nvmlDeviceGetTemperature")),
                export::<PowerFn>(lib, windows::core::s!("nvmlDeviceGetPowerUsage")),
                export::<ClockFn>(lib, windows::core::s!("nvmlDeviceGetClockInfo")),
            )
        };
        let (Some(init), Some(shutdown), Some(handle), Some(temp), Some(power), Some(clock)) = fns
        else {
            free(lib);
            return None;
        };
        // SAFETY: `init` is nvmlInit_v2 and takes no arguments.
        if unsafe { init() } != 0 {
            free(lib);
            return None;
        }
        let mut device: *mut c_void = std::ptr::null_mut();
        // SAFETY: NVML is initialised and `device` is a valid out-parameter.
        if unsafe { handle(0, &mut device) } != 0 || device.is_null() {
            // SAFETY: NVML was initialised above.
            unsafe { shutdown() };
            free(lib);
            return None;
        }
        Some(Nvml {
            lib,
            device,
            shutdown,
            temp,
            power,
            clock,
        })
    }

    pub fn read(&self) -> Option<NvmlReading> {
        let mut v = 0u32;
        // SAFETY: `device` is valid while `self` lives; `v` is a valid out-parameter.
        let temp_c =
            (unsafe { (self.temp)(self.device, SENSOR_GPU, &mut v) } == 0).then_some(v as f32);
        // SAFETY: as above.
        let power_w =
            (unsafe { (self.power)(self.device, &mut v) } == 0).then(|| milliwatts_to_watts(v));
        // SAFETY: as above.
        let clock_mhz =
            (unsafe { (self.clock)(self.device, CLOCK_GRAPHICS, &mut v) } == 0).then_some(v);
        // SAFETY: as above.
        let mem_clock_mhz =
            (unsafe { (self.clock)(self.device, CLOCK_MEM, &mut v) } == 0).then_some(v);
        if temp_c.is_none() && power_w.is_none() && clock_mhz.is_none() && mem_clock_mhz.is_none() {
            return None;
        }
        Some(NvmlReading {
            temp_c,
            power_w,
            clock_mhz,
            mem_clock_mhz,
        })
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: NVML was initialised in `open`; the library is freed exactly once.
        unsafe {
            (self.shutdown)();
            let _ = FreeLibrary(self.lib);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milliwatts_convert() {
        assert_eq!(milliwatts_to_watts(0), 0.0);
        assert_eq!(milliwatts_to_watts(1500), 1.5);
        assert_eq!(milliwatts_to_watts(250_000), 250.0);
    }

    #[test]
    #[ignore]
    fn live_read() {
        let nvml = Nvml::open().expect("nvml open");
        for _ in 0..2 {
            println!("{:?}", nvml.read());
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
