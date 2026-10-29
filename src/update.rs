//! Manual self-update from GitHub Releases.
//!
//! The exe is downloaded into memory, checked against the release's SHA-256 file, and swapped
//! in by renaming the running exe aside and writing the new one in its place. The renamed
//! `<exe>.old` is deleted by the next start. Nothing else is ever written.
//!
//! The checksum comes from the same release as the exe, so it guards against corruption, not
//! against someone who controls the release; that rests on HTTPS and the GitHub account.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

use sha2::{Digest, Sha256};

use crate::winhttp::{self, HttpError};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Gusseti/toolboks/releases/latest";
const EXE_SUFFIX: &str = "-x86_64-pc-windows-msvc.exe";
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;
const MIN_EXE_BYTES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<String>,
}

impl Version {
    /// Parses `1.2.3`, `v1.2.3` and `1.2.3-rc.1`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (text, None),
        };
        let mut parts = core.split('.');
        let version = Self {
            major: parts.next()?.parse().ok()?,
            minor: parts.next()?.parse().ok()?,
            patch: parts.next()?.parse().ok()?,
            pre,
        };
        parts.next().is_none().then_some(version)
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("Cargo.toml version is valid semver")
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A release is newer than its own pre-releases.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match &self.pre {
            Some(pre) => write!(f, "-{pre}"),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub exe_url: String,
    pub sha_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// HTTP 404: no release, or the repository is private.
    NotFound,
    Network(String),
    BadResponse(&'static str),
    NoAsset,
    ChecksumMismatch,
    Io(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::NotFound => {
                write!(
                    f,
                    "No public release found. The repository may still be private."
                )
            }
            UpdateError::Network(e) => write!(f, "Network problem: {e}"),
            UpdateError::BadResponse(what) => write!(f, "Unexpected response: {what}."),
            UpdateError::NoAsset => write!(f, "The release has no Windows exe attached."),
            UpdateError::ChecksumMismatch => {
                write!(
                    f,
                    "The download did not match its checksum and was discarded."
                )
            }
            UpdateError::Io(e) => write!(f, "Could not replace the exe: {e}"),
        }
    }
}

impl From<HttpError> for UpdateError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Status(404) => UpdateError::NotFound,
            HttpError::Status(code) => UpdateError::Network(format!("server answered {code}")),
            HttpError::Other(msg) => UpdateError::Network(msg),
        }
    }
}

/// Only GitHub's own hosts, over HTTPS, may serve an update.
fn is_trusted_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    host == "github.com"
        || host.ends_with(".github.com")
        || host.ends_with(".githubusercontent.com")
}

/// Reads the `releases/latest` JSON and picks the Windows exe and its checksum file.
pub fn parse_release(json: &[u8]) -> Result<Release, UpdateError> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|_| UpdateError::BadResponse("not valid JSON"))?;
    let tag = value["tag_name"]
        .as_str()
        .ok_or(UpdateError::BadResponse("no tag_name"))?;
    let version = Version::parse(tag).ok_or(UpdateError::BadResponse("tag is not a version"))?;

    let assets = value["assets"]
        .as_array()
        .ok_or(UpdateError::BadResponse("no assets list"))?;
    let url_of = |name: &str| -> Option<String> {
        assets
            .iter()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .filter(|u| is_trusted_url(u))
            .map(str::to_string)
    };

    let exe_name = assets
        .iter()
        .filter_map(|a| a["name"].as_str())
        .find(|n| n.ends_with(EXE_SUFFIX))
        .ok_or(UpdateError::NoAsset)?;
    let exe_url = url_of(exe_name).ok_or(UpdateError::NoAsset)?;
    let sha_url = url_of(&format!("{exe_name}.sha256")).ok_or(UpdateError::NoAsset)?;
    Ok(Release {
        version,
        exe_url,
        sha_url,
    })
}

/// First whitespace-separated token of a `sha256sum`-style file, as 32 bytes.
pub fn parse_sha256(text: &str) -> Option<[u8; 32]> {
    let token = text.split_whitespace().next()?;
    if token.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(token.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn verify(exe: &[u8], expected: &[u8; 32]) -> Result<(), UpdateError> {
    let looks_like_exe = exe.len() >= MIN_EXE_BYTES && exe.starts_with(b"MZ");
    let digest: [u8; 32] = Sha256::digest(exe).into();
    if looks_like_exe && &digest == expected {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch)
    }
}

pub enum CheckOutcome {
    UpToDate,
    Available(Release),
}

/// Asks GitHub for the latest stable release. Blocking: run it off the UI thread.
pub fn check() -> Result<CheckOutcome, UpdateError> {
    let json = winhttp::get(
        LATEST_RELEASE_URL,
        "application/vnd.github+json",
        MAX_JSON_BYTES,
    )?;
    let release = parse_release(&json)?;
    Ok(if release.version > Version::current() {
        CheckOutcome::Available(release)
    } else {
        CheckOutcome::UpToDate
    })
}

/// Downloads, verifies and swaps in the release. Blocking: run it off the UI thread.
/// On success the new exe is in place; the caller restarts the app.
pub fn install(release: &Release) -> Result<(), UpdateError> {
    let exe = winhttp::get(&release.exe_url, "application/octet-stream", MAX_EXE_BYTES)?;
    let sha_text = winhttp::get(&release.sha_url, "text/plain", 4096)?;
    let expected = parse_sha256(&String::from_utf8_lossy(&sha_text))
        .ok_or(UpdateError::BadResponse("unreadable checksum file"))?;
    verify(&exe, &expected)?;

    let current = std::env::current_exe().map_err(|e| UpdateError::Io(e.to_string()))?;
    replace_exe(&current, &exe).map_err(|e| UpdateError::Io(e.to_string()))
}

fn old_path(current: &Path) -> PathBuf {
    let mut name = current.as_os_str().to_owned();
    name.push(".old");
    name.into()
}

/// Renames `current` (even while it runs) to `<current>.old` and writes `new_exe` in its place.
/// Rolls back if the write fails.
fn replace_exe(current: &Path, new_exe: &[u8]) -> std::io::Result<()> {
    let old = old_path(current);
    let _ = fs::remove_file(&old);
    fs::rename(current, &old)?;
    if let Err(e) = fs::write(current, new_exe) {
        let _ = fs::remove_file(current);
        let _ = fs::rename(&old, current);
        return Err(e);
    }
    Ok(())
}

/// After an update the previous exe is still on disk as `<exe>.old`. Deletes it once the old
/// process has exited. Does nothing, and touches nothing, in the normal case.
pub fn cleanup_old_exe() {
    let Ok(current) = std::env::current_exe() else {
        return;
    };
    let old = old_path(&current);
    if !old.exists() {
        return;
    }
    thread::spawn(move || {
        for _ in 0..30 {
            if fs::remove_file(&old).is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_versions() {
        assert_eq!(v("v1.2.3").to_string(), "1.2.3");
        assert_eq!(v("0.2.0-rc.1").to_string(), "0.2.0-rc.1");
        assert!(Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
        assert!(Version::parse("a.b.c").is_none());
    }

    #[test]
    fn orders_versions() {
        assert!(v("0.2.0") > v("0.1.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.2.0") > v("0.2.0-rc.1"), "release beats its own rc");
        assert!(v("0.2.0-rc.2") > v("0.2.0-rc.1"));
        assert!(v("0.2.0-rc.1") > v("0.1.0"));
        assert_eq!(v("0.1.0"), v("v0.1.0"));
    }

    #[test]
    fn current_version_parses() {
        let _ = Version::current();
    }

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v0.3.0",
        "assets": [
            {"name": "other.txt", "browser_download_url": "https://github.com/x/y/releases/download/v0.3.0/other.txt"},
            {"name": "toolboks-v0.3.0-x86_64-pc-windows-msvc.exe",
             "browser_download_url": "https://github.com/Gusseti/toolboks/releases/download/v0.3.0/toolboks-v0.3.0-x86_64-pc-windows-msvc.exe"},
            {"name": "toolboks-v0.3.0-x86_64-pc-windows-msvc.exe.sha256",
             "browser_download_url": "https://github.com/Gusseti/toolboks/releases/download/v0.3.0/toolboks-v0.3.0-x86_64-pc-windows-msvc.exe.sha256"}
        ]
    }"#;

    #[test]
    fn picks_the_exe_and_checksum_assets() {
        let r = parse_release(RELEASE_JSON.as_bytes()).unwrap();
        assert_eq!(r.version, v("0.3.0"));
        assert!(r.exe_url.ends_with("msvc.exe"));
        assert!(r.sha_url.ends_with("msvc.exe.sha256"));
    }

    #[test]
    fn rejects_bad_releases() {
        assert_eq!(
            parse_release(b"not json"),
            Err(UpdateError::BadResponse("not valid JSON"))
        );
        assert_eq!(
            parse_release(br#"{"tag_name":"v1.0.0","assets":[]}"#),
            Err(UpdateError::NoAsset)
        );
        assert!(matches!(
            parse_release(br#"{"tag_name":"nightly","assets":[]}"#),
            Err(UpdateError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_assets_from_untrusted_hosts() {
        let json =
            RELEASE_JSON.replace("https://github.com/Gusseti", "https://evil.example/Gusseti");
        assert_eq!(parse_release(json.as_bytes()), Err(UpdateError::NoAsset));
        assert!(is_trusted_url("https://objects.githubusercontent.com/a"));
        assert!(!is_trusted_url("http://github.com/a"));
        assert!(!is_trusted_url("https://github.com.evil.example/a"));
    }

    #[test]
    fn parses_checksum_files() {
        let hex = "ab".repeat(32);
        assert_eq!(
            parse_sha256(&format!("{hex}  file.exe\n")),
            Some([0xab; 32])
        );
        assert_eq!(parse_sha256("short"), None);
        assert_eq!(parse_sha256(&"zz".repeat(32)), None);
        assert_eq!(parse_sha256(""), None);
    }

    #[test]
    fn verifies_checksum_and_exe_shape() {
        let mut exe = vec![0u8; MIN_EXE_BYTES];
        exe[..2].copy_from_slice(b"MZ");
        let digest: [u8; 32] = Sha256::digest(&exe).into();
        assert_eq!(verify(&exe, &digest), Ok(()));
        assert_eq!(verify(&exe, &[0; 32]), Err(UpdateError::ChecksumMismatch));

        // Matching checksum but not an exe: still refused.
        let junk = vec![1u8; MIN_EXE_BYTES];
        let junk_digest: [u8; 32] = Sha256::digest(&junk).into();
        assert_eq!(
            verify(&junk, &junk_digest),
            Err(UpdateError::ChecksumMismatch)
        );
    }

    #[test]
    fn swaps_the_exe_and_keeps_the_old_one_aside() {
        let dir = std::env::temp_dir().join(format!("toolboks-swap-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app.exe");
        fs::write(&exe, b"old").unwrap();

        replace_exe(&exe, b"new").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(fs::read(old_path(&exe)).unwrap(), b"old");

        // A second update replaces the leftover .old instead of failing.
        replace_exe(&exe, b"newer").unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"newer");
        assert_eq!(fs::read(old_path(&exe)).unwrap(), b"new");

        let _ = fs::remove_dir_all(&dir);
    }
}
