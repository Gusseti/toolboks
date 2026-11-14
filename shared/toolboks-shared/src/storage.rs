//! Where Toolboks keeps local data, and how files are written there.

use std::fs;
use std::path::{Path, PathBuf};

/// The folder that holds Toolboks' local data: `%LOCALAPPDATA%/Toolboks`, or
/// `Toolboks Preview` for a build with the `preview` feature.
pub fn data_root() -> PathBuf {
    let name = if cfg!(feature = "preview") {
        "Toolboks Preview"
    } else {
        "Toolboks"
    };
    // TOOLBOKS_DATA_DIR is useful for isolated automated tests and portable developer runs.
    if let Some(path) = std::env::var_os("TOOLBOKS_DATA_DIR") {
        return PathBuf::from(path).join(name);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(Path::new("."))
                .to_owned()
        })
        .join(name)
}

/// Writes `bytes` to a sibling `.pending` file and renames it over `path`, so a reader never
/// sees a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Append to the whole name: `with_extension` would turn `0.12.5` into `0.12.pending`, so
    // two version folders could share one staging name.
    let mut name = path
        .file_name()
        .ok_or_else(|| format!("No file name in {}", path.display()))?
        .to_owned();
    name.push(".pending");
    let temp = path.with_file_name(name);
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            },
        };
        let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    fs::rename(temp, path).map_err(|e| e.to_string())?;
    Ok(())
}
