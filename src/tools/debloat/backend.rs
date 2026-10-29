//! Registry access behind a trait, plus the real (read-only in phase 1) implementation and
//! edition / elevation detection.
#![allow(dead_code)]

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};

use super::model::{Edition, Env, Hive, RegValue};

/// Registry access behind a trait so the engine can be tested with an in-memory fake.
pub trait Registry {
    /// `Ok(None)` if the value does not exist.
    fn read(&self, hive: Hive, path: &str, name: &str) -> Result<Option<RegValue>, String>;
    fn write(&mut self, hive: Hive, path: &str, name: &str, value: &RegValue)
        -> Result<(), String>;
    fn delete(&mut self, hive: Hive, path: &str, name: &str) -> Result<(), String>;
}

/// The real registry. Phase 1: reads only; `write` and `delete` return an error.
pub struct WinRegistry;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn root(hive: Hive) -> HKEY {
    match hive {
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
    }
}

impl Registry for WinRegistry {
    fn read(&self, hive: Hive, path: &str, name: &str) -> Result<Option<RegValue>, String> {
        let path_w = wide(path);
        let name_w = wide(name);
        let mut key = HKEY::default();
        // SAFETY: both strings are NUL-terminated and outlive the call; `key` is a valid out-parameter.
        let err = unsafe {
            RegOpenKeyExW(
                root(hive),
                PCWSTR(path_w.as_ptr()),
                None,
                KEY_READ,
                &mut key,
            )
        };
        if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if err != ERROR_SUCCESS {
            return Err(format!("cannot open {path}: error {}", err.0));
        }

        let result = query_value(key, &name_w, name);
        // SAFETY: `key` was opened above and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(key);
        }
        result
    }

    fn write(
        &mut self,
        hive: Hive,
        path: &str,
        name: &str,
        value: &RegValue,
    ) -> Result<(), String> {
        let path_w = wide(path);
        let name_w = wide(name);
        let mut key = HKEY::default();
        // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
        let err = unsafe {
            RegCreateKeyExW(
                root(hive),
                PCWSTR(path_w.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot open {path}")));
        }

        let (kind, bytes): (_, Vec<u8>) = match value {
            RegValue::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            RegValue::Sz(s) => (
                REG_SZ,
                wide(s).iter().flat_map(|c| c.to_le_bytes()).collect(),
            ),
        };
        // SAFETY: `key` is open with set rights; the name is NUL-terminated; `bytes` holds the data.
        let err = unsafe { RegSetValueExW(key, PCWSTR(name_w.as_ptr()), None, kind, Some(&bytes)) };
        // SAFETY: `key` was opened above and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(key);
        }
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot write {name}")));
        }
        Ok(())
    }

    fn delete(&mut self, hive: Hive, path: &str, name: &str) -> Result<(), String> {
        let path_w = wide(path);
        let name_w = wide(name);
        let mut key = HKEY::default();
        // SAFETY: the path is NUL-terminated and outlives the call; `key` is a valid out-parameter.
        let err = unsafe {
            RegOpenKeyExW(
                root(hive),
                PCWSTR(path_w.as_ptr()),
                None,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot open {path}")));
        }
        // SAFETY: `key` is open with set rights and the name is NUL-terminated.
        let err = unsafe { RegDeleteValueW(key, PCWSTR(name_w.as_ptr())) };
        // SAFETY: `key` was opened above and is closed exactly once.
        unsafe {
            let _ = RegCloseKey(key);
        }
        if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        if err != ERROR_SUCCESS {
            return Err(describe(err, &format!("cannot delete {name}")));
        }
        Ok(())
    }
}

fn describe(err: WIN32_ERROR, what: &str) -> String {
    if err == ERROR_ACCESS_DENIED {
        "Needs administrator rights".to_string()
    } else {
        format!("{what}: error {}", err.0)
    }
}

fn query_value(key: HKEY, name_w: &[u16], name: &str) -> Result<Option<RegValue>, String> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    // SAFETY: size query only; the out-parameters are valid and the name is NUL-terminated.
    let err = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if err != ERROR_SUCCESS {
        return Err(format!("cannot read {name}: error {}", err.0));
    }

    // Round up to whole u16 units so both value types fit; extra room for a missing terminator.
    let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
    let mut cap = (buf.len() * 2) as u32;
    // SAFETY: `buf` is at least `cap` bytes long and `cap` is passed as its size.
    let err = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut cap),
        )
    };
    if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if err != ERROR_SUCCESS {
        return Err(format!("cannot read {name}: error {}", err.0));
    }

    let bytes = cap as usize;
    if kind == REG_DWORD {
        if bytes < 4 {
            return Err(format!("{name} is not a valid number"));
        }
        let v = buf[0] as u32 | ((buf[1] as u32) << 16);
        Ok(Some(RegValue::Dword(v)))
    } else if kind == REG_SZ {
        let units = (bytes / 2).min(buf.len());
        let text = &buf[..units];
        let end = text.iter().position(|&c| c == 0).unwrap_or(units);
        Ok(Some(RegValue::Sz(String::from_utf16_lossy(&text[..end]))))
    } else {
        Err(format!("{name} has an unsupported type"))
    }
}

/// Edition from the `EditionID` text of Windows.
pub(crate) fn edition_from_id(id: &str) -> Edition {
    let id = id.trim().to_ascii_lowercase();
    if id.starts_with("core") || id == "home" {
        Edition::Home
    } else if id.starts_with("professional") {
        Edition::Pro
    } else if id.starts_with("enterprise") || id.starts_with("education") {
        Edition::Enterprise
    } else {
        Edition::Unknown
    }
}

/// Edition and elevation of this machine.
pub fn detect_env() -> Env {
    let edition = match WinRegistry.read(
        Hive::LocalMachine,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "EditionID",
    ) {
        Ok(Some(RegValue::Sz(id))) => edition_from_id(&id),
        _ => Edition::Unknown,
    };
    Env {
        edition,
        elevated: crate::elevation::is_elevated(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions_map() {
        assert_eq!(edition_from_id("Core"), Edition::Home);
        assert_eq!(edition_from_id("CoreSingleLanguage"), Edition::Home);
        assert_eq!(edition_from_id("Professional"), Edition::Pro);
        assert_eq!(edition_from_id("ProfessionalWorkstation"), Edition::Pro);
        assert_eq!(edition_from_id("Enterprise"), Edition::Enterprise);
        assert_eq!(edition_from_id("EnterpriseS"), Edition::Enterprise);
        assert_eq!(edition_from_id("Education"), Edition::Enterprise);
        assert_eq!(edition_from_id("ServerStandard"), Edition::Unknown);
        assert_eq!(edition_from_id(""), Edition::Unknown);
    }

    #[test]
    fn real_read_of_missing_value_is_none() {
        let r = WinRegistry.read(Hive::CurrentUser, r"Software\Toolboks\DoesNotExist", "Nope");
        assert_eq!(r, Ok(None));
    }

    use crate::tools::debloat::model::{Group, Item, RegSetting, Risk};
    use crate::tools::debloat::plan;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    /// Deletes the whole scratch key on drop, even if an assert failed.
    struct Scratch(String);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let p = wide(&self.0);
            // SAFETY: NUL-terminated path under HKCU; only ever a toolboks-test key.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()));
            }
        }
    }

    fn scratch(tag: &str) -> Scratch {
        let path = format!(r"Software\toolboks-test-{tag}-{}", std::process::id());
        // Clean any leftovers first.
        drop(Scratch(path.clone()));
        Scratch(path)
    }

    #[test]
    fn real_write_read_delete_roundtrip() {
        let s = scratch("rw");
        let mut r = WinRegistry;
        let h = Hive::CurrentUser;
        r.write(h, &s.0, "D", &RegValue::Dword(7)).unwrap();
        r.write(h, &s.0, "S", &RegValue::Sz("hello ø".into()))
            .unwrap();
        assert_eq!(r.read(h, &s.0, "D"), Ok(Some(RegValue::Dword(7))));
        assert_eq!(
            r.read(h, &s.0, "S"),
            Ok(Some(RegValue::Sz("hello ø".into())))
        );
        r.write(h, &s.0, "D", &RegValue::Dword(0)).unwrap();
        r.write(h, &s.0, "S", &RegValue::Sz(String::new())).unwrap();
        assert_eq!(r.read(h, &s.0, "D"), Ok(Some(RegValue::Dword(0))));
        assert_eq!(r.read(h, &s.0, "S"), Ok(Some(RegValue::Sz(String::new()))));
        r.delete(h, &s.0, "D").unwrap();
        assert_eq!(r.read(h, &s.0, "D"), Ok(None));
        assert_eq!(r.delete(h, &s.0, "D"), Ok(()));
        assert_eq!(
            r.delete(h, r"Software\toolboks-test-no-such-key", "X"),
            Ok(())
        );
    }

    #[test]
    fn real_apply_then_undo_through_engine() {
        let s = scratch("engine");
        let path: &'static str = Box::leak(s.0.clone().into_boxed_str());
        let settings: &'static [RegSetting] = Box::leak(Box::new([RegSetting {
            hive: Hive::CurrentUser,
            path,
            name: "Flag",
            off_value: 0,
        }]));
        let items = [Item {
            id: "scratch",
            group: Group::PrivacyAds,
            title: "t",
            what_changes: "w",
            could_break: "c",
            risk: Risk::Safe,
            min_edition: Edition::Unknown,
            settings,
        }];
        let env = Env {
            edition: Edition::Pro,
            elevated: false,
        };
        let mut r = WinRegistry;
        r.write(Hive::CurrentUser, path, "Flag", &RegValue::Dword(1))
            .unwrap();
        let run = plan::apply(&items, &["scratch"], &mut r, env);
        assert_eq!(run.applied.len(), 1);
        assert_eq!(
            r.read(Hive::CurrentUser, path, "Flag"),
            Ok(Some(RegValue::Dword(0)))
        );
        let results = plan::undo(&run, &mut r);
        assert!(results.iter().all(|(_, res)| res.is_ok()));
        assert_eq!(
            r.read(Hive::CurrentUser, path, "Flag"),
            Ok(Some(RegValue::Dword(1)))
        );
    }
}
