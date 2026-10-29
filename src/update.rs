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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

use sha2::{Digest, Sha256};

use crate::winhttp::{self, HttpError};

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Gusseti/toolboks/releases/latest";
/// The release asset is the plain exe; its checksum file is `toolboks.exe.sha256`.
const EXE_ASSET: &str = "toolboks.exe";
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;
const MIN_EXE_BYTES: usize = 100_000;
/// Whole-transfer limits; a server that drips bytes forever cannot stall the app.
const JSON_DEADLINE: Duration = Duration::from_secs(30);
const EXE_DEADLINE: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<String>,
}

impl Version {
    /// Parses `1.2.3`, `v1.2.3` and `1.2.3-rc.1`. Build metadata (`+abc`) is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next().unwrap_or(text);
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
                (Some(a), Some(b)) => cmp_pre(a, b),
            })
    }
}

/// SemVer pre-release order: dot-separated identifiers, numbers compared as numbers (so
/// `rc.10 > rc.9`), numbers before words, and a shorter list before a longer one.
fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        let ord = match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match (x.parse::<u64>(), y.parse::<u64>()) {
                (Ok(m), Ok(n)) => m.cmp(&n),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => x.cmp(y),
            },
        };
        if ord != Ordering::Equal {
            return ord;
        }
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

    let exe_url = url_of(EXE_ASSET).ok_or(UpdateError::NoAsset)?;
    let sha_url = url_of(&format!("{EXE_ASSET}.sha256")).ok_or(UpdateError::NoAsset)?;
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

/// GET that also checks where redirects ended up: the body is discarded unless the final URL is
/// still one of GitHub's hosts over HTTPS.
fn fetch(
    url: &str,
    accept: &str,
    max_bytes: usize,
    deadline: Duration,
) -> Result<Vec<u8>, UpdateError> {
    let response = winhttp::get(url, accept, max_bytes, deadline)?;
    if is_trusted_url(&response.final_url) {
        Ok(response.body)
    } else {
        Err(UpdateError::BadResponse("redirected away from GitHub"))
    }
}

/// Asks GitHub for the latest stable release. Blocking: run it off the UI thread.
pub fn check() -> Result<CheckOutcome, UpdateError> {
    let json = fetch(
        LATEST_RELEASE_URL,
        "application/vnd.github+json",
        MAX_JSON_BYTES,
        JSON_DEADLINE,
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
    let exe = fetch(
        &release.exe_url,
        "application/octet-stream",
        MAX_EXE_BYTES,
        EXE_DEADLINE,
    )?;
    let sha_text = fetch(&release.sha_url, "text/plain", 4096, JSON_DEADLINE)?;
    let expected = parse_sha256(&String::from_utf8_lossy(&sha_text))
        .ok_or(UpdateError::BadResponse("unreadable checksum file"))?;
    verify(&exe, &expected)?;

    let current = std::env::current_exe().map_err(|e| UpdateError::Io(e.to_string()))?;
    replace_exe(&current, &exe).map_err(|e| UpdateError::Io(e.to_string()))
}

fn sibling(current: &Path, suffix: &str) -> PathBuf {
    let mut name = current.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn old_path(current: &Path) -> PathBuf {
    sibling(current, ".old")
}

fn new_path(current: &Path) -> PathBuf {
    sibling(current, ".new")
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    // Make sure the bytes are on disk before the exe is swapped, so a crash or power loss
    // cannot leave a truncated toolboks.exe.
    file.sync_all()
}

/// Puts `new_exe` in place of `current`, which may be running:
/// 1. write and flush `<current>.new`, so `current` is never half-written,
/// 2. rename `current` to `<current>.old` (allowed for a running exe),
/// 3. rename `<current>.new` to `current`.
///
/// Any failure restores the original exe.
fn replace_exe(current: &Path, new_exe: &[u8]) -> std::io::Result<()> {
    let (old, new) = (old_path(current), new_path(current));
    let _ = fs::remove_file(&old);
    if let Err(e) = write_and_sync(&new, new_exe) {
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    if let Err(e) = fs::rename(current, &old) {
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    if let Err(e) = fs::rename(&new, current) {
        let _ = fs::rename(&old, current);
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    Ok(())
}

/// After an update the previous exe is still on disk as `<exe>.old` (and a crash mid-update
/// could leave `<exe>.new`). Deletes them once the old process has exited. Does nothing, and
/// touches nothing, in the normal case.
pub fn cleanup_old_exe() {
    let Ok(current) = std::env::current_exe() else {
        return;
    };
    let leftovers: Vec<PathBuf> = [old_path(&current), new_path(&current)]
        .into_iter()
        .filter(|p| p.exists())
        .collect();
    if leftovers.is_empty() {
        return;
    }
    thread::spawn(move || {
        for path in leftovers {
            for _ in 0..30 {
                if fs::remove_file(&path).is_ok() {
                    break;
                }
                thread::sleep(Duration::from_millis(500));
            }
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
    fn orders_prerelease_numbers_numerically() {
        assert!(v("0.2.0-rc.10") > v("0.2.0-rc.9"));
        assert!(
            v("0.2.0-rc.1") > v("0.2.0-beta.7"),
            "words compare alphabetically"
        );
        assert!(v("0.2.0-rc.1.1") > v("0.2.0-rc.1"), "longer list is newer");
        assert!(v("0.2.0-1") < v("0.2.0-alpha"), "numbers sort before words");
    }

    #[test]
    fn ignores_build_metadata() {
        assert_eq!(v("1.2.3+abc"), v("1.2.3"));
        assert_eq!(v("1.2.3-rc.1+build.5").to_string(), "1.2.3-rc.1");
    }

    #[test]
    fn current_version_parses() {
        let _ = Version::current();
    }

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v0.3.0",
        "assets": [
            {"name": "other.txt", "browser_download_url": "https://github.com/x/y/releases/download/v0.3.0/other.txt"},
            {"name": "toolboks.exe",
             "browser_download_url": "https://github.com/Gusseti/toolboks/releases/download/v0.3.0/toolboks.exe"},
            {"name": "toolboks.exe.sha256",
             "browser_download_url": "https://github.com/Gusseti/toolboks/releases/download/v0.3.0/toolboks.exe.sha256"}
        ]
    }"#;

    #[test]
    fn picks_the_exe_and_checksum_assets() {
        let r = parse_release(RELEASE_JSON.as_bytes()).unwrap();
        assert_eq!(r.version, v("0.3.0"));
        assert!(r.exe_url.ends_with("/v0.3.0/toolboks.exe"));
        assert!(r.sha_url.ends_with("/v0.3.0/toolboks.exe.sha256"));
    }

    #[test]
    fn a_release_without_the_plain_exe_has_no_usable_asset() {
        // The old long-named files are not used any more.
        let json = r#"{"tag_name":"v0.3.0","assets":[
            {"name":"toolboks-v0.3.0-x86_64-pc-windows-msvc.exe",
             "browser_download_url":"https://github.com/Gusseti/toolboks/releases/download/v0.3.0/toolboks-v0.3.0-x86_64-pc-windows-msvc.exe"}]}"#;
        assert_eq!(parse_release(json.as_bytes()), Err(UpdateError::NoAsset));
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

    /// Needs internet and a published release; run with `cargo test -- --ignored`.
    /// The whole download path against the real latest release, minus the swap.
    #[test]
    #[ignore]
    fn the_published_release_downloads_and_verifies() {
        let json = fetch(
            LATEST_RELEASE_URL,
            "application/vnd.github+json",
            MAX_JSON_BYTES,
            JSON_DEADLINE,
        )
        .expect("latest release JSON");
        let release = parse_release(&json).expect("release has an exe and a checksum");
        let exe = fetch(
            &release.exe_url,
            "application/octet-stream",
            MAX_EXE_BYTES,
            EXE_DEADLINE,
        )
        .expect("exe download");
        let sha = fetch(&release.sha_url, "text/plain", 4096, JSON_DEADLINE).expect("checksum");
        let expected = parse_sha256(&String::from_utf8_lossy(&sha)).expect("checksum file parses");
        assert_eq!(verify(&exe, &expected), Ok(()));
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

        // The staging file never survives a swap.
        assert!(!new_path(&exe).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_swap_leaves_the_original_exe_untouched() {
        let dir = std::env::temp_dir().join(format!("toolboks-swap-fail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app.exe");
        fs::write(&exe, b"original").unwrap();
        // A directory squatting on the .new name makes writing the new exe fail.
        fs::create_dir_all(new_path(&exe)).unwrap();

        assert!(replace_exe(&exe, b"new").is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"original");
        assert!(!old_path(&exe).exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
