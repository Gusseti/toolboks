//! Host side of the add-on catalog: where it is read from, what this build can run, and the copy
//! kept for starting add-ons with administrator rights. The format and the checks live in
//! `toolboks-catalog`.
//!
//! The catalog is a plain list that anyone can publish. It is not signed. Every download is still
//! checked against the size and SHA-256 its entry declares.

// Nothing calls some of this yet: parts move over in later changes.
#![allow(dead_code, unused_imports)]

use crate::addons;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
pub use toolboks_catalog::{
    check_bytes, Channel, DownloadMismatch, Entry, Hidden, Host, Index, Kind, Refused,
};

/// Add-on protocol versions this build can talk to. The supervisor speaks the version of the
/// installed entry, and only one of these. Protocol 4 adds health findings.
pub const PROTOCOL_VERSIONS: &[u32] = &[3, 4];

pub const URL: &str =
    "https://raw.githubusercontent.com/fredhakon/toolboks-addons/main/catalog/index.json";

/// The same list through GitHub's API. GitHub keeps the plain file address (`URL`) in its cache
/// for up to five minutes, so a release made a moment ago is missing after a refresh; the API is
/// at most a minute behind. It is asked first and `URL` is the fallback (a failure, or the API's
/// limit of 60 requests an hour per address being used up).
pub const FRESH_URL: &str =
    "https://api.github.com/repos/fredhakon/toolboks-addons/contents/catalog/index.json";

/// What this build can run. Preview-channel entries are shown by preview builds only.
pub fn host() -> Host<'static> {
    Host {
        version: env!("CARGO_PKG_VERSION"),
        protocols: PROTOCOL_VERSIONS,
        preview: cfg!(feature = "preview"),
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Serialises writes of the saved catalog.
static STATE: Mutex<()> = Mutex::new(());

/// The last catalog read, as received. Kept so an add-on that asks for administrator rights is
/// started from the catalog entry and not from install records the user account can edit.
fn cache_path(root: &Path) -> PathBuf {
    root.join("catalog").join("accepted.json")
}

/// The saved catalog, read again and checked at the time of use.
fn cached(root: &Path) -> Result<Index, String> {
    use std::io::Read;
    let limit = toolboks_catalog::MAX_INDEX_BYTES as u64 + 1;
    let mut bytes = Vec::new();
    std::fs::File::open(cache_path(root))
        .and_then(|file| file.take(limit).read_to_end(&mut bytes))
        .map_err(|_| {
            "Refresh the add-on catalog before starting with administrator rights.".to_string()
        })?;
    if bytes.len() as u64 >= limit {
        return Err("The saved catalog is too large. Refresh the add-on catalog.".into());
    }
    Index::parse(&bytes).map_err(|e| {
        format!("Refresh the add-on catalog before starting with administrator rights: {e}")
    })
}

/// The catalog entry, never `active.json` or `entry.json`, authorizes an elevated start.
/// A version that is not in the current catalog cannot be elevated.
pub fn elevated_entry(root: &Path, id: &str, version: &str) -> Result<Entry, String> {
    let _guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = cached(root)?
        .entries
        .into_iter()
        .find(|entry| entry.id == id && entry.version == version)
        .ok_or_else(|| {
            "This installed version is absent from the catalog. Update the add-on.".to_string()
        })?;
    if entry.kind != Kind::Native
        || !entry.verified
        || entry.check(&host()).is_err()
        || !entry
            .capabilities
            .iter()
            .any(|name| crate::addon_services::ELEVATING_CAPABILITIES.contains(&name.as_str()))
    {
        return Err("The catalog does not permit this add-on to use administrator rights.".into());
    }
    Ok(entry)
}

/// Checks a downloaded `index.json` (exact bytes) and remembers it. `root` is
/// `addons::data_root()`.
pub fn accept(root: &Path, index: &[u8]) -> Result<Index, Refused> {
    let _guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let parsed = Index::parse(index)?;
    addons::write_atomic(&cache_path(root), index).map_err(Refused::NotPersisted)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn index() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "entries": [{
                "id": "toolboks.sample",
                "title": "Sample",
                "short_description": "A sample add-on",
                "publisher": "Toolboks",
                "category": "system",
                "icon": "box",
                "accent": "#4f8cff",
                "kind": "declarative",
                "version": "1.0.0",
                "min_toolboks_version": env!("CARGO_PKG_VERSION"),
                "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
                "download_url": "https://example.com/sample.tbaddon",
                "size": 5,
                "sha256": SHA,
                "verified": true,
                "channel": "preview"
            }]
        }))
        .unwrap()
    }

    fn native_index(payload: &[u8]) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(&index()).unwrap();
        let entry = &mut value["entries"][0];
        entry["kind"] = "native".into();
        entry["channel"] = "stable".into();
        entry["capabilities"] = serde_json::json!(["administrator"]);
        entry["sha256"] = toolboks_catalog::sha256_hex(payload).into();
        entry["size"] = payload.len().into();
        serde_json::to_vec(&value).unwrap()
    }

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("toolboks-catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn accepts_an_unsigned_index_and_remembers_it() {
        let root = root("accept");
        let bytes = index();
        assert_eq!(accept(&root, &bytes).unwrap().entries.len(), 1);
        assert_eq!(std::fs::read(cache_path(&root)).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_a_broken_index_and_remembers_nothing() {
        let root = root("broken");
        assert!(accept(&root, b"{ not json").is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&index()).unwrap();
        value["format_version"] = 2.into();
        assert_eq!(
            accept(&root, &serde_json::to_vec(&value).unwrap()),
            Err(Refused::UnknownFormat(2))
        );
        assert!(!cache_path(&root).exists());
    }

    #[test]
    fn elevated_start_is_authorized_by_the_saved_catalog_entry() {
        let root = root("elevated-cache");
        assert!(
            elevated_entry(&root, "toolboks.sample", "1.0.0").is_err(),
            "no saved catalog"
        );
        accept(&root, &native_index(b"original")).unwrap();
        assert_eq!(
            elevated_entry(&root, "toolboks.sample", "1.0.0")
                .unwrap()
                .sha256,
            toolboks_catalog::sha256_hex(b"original")
        );
        assert!(elevated_entry(&root, "toolboks.other", "1.0.0").is_err());
        assert!(elevated_entry(&root, "toolboks.sample", "0.9.0").is_err());
        std::fs::write(cache_path(&root), b"damaged").unwrap();
        assert!(elevated_entry(&root, "toolboks.sample", "1.0.0").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swapped_exe_and_install_records_do_not_authorize_elevation() {
        let root = root("elevated-records");
        let bytes = native_index(b"original");
        accept(&root, &bytes).unwrap();
        let dir = root.join("addons/toolboks.sample");
        std::fs::create_dir_all(dir.join("1.0.0")).unwrap();
        let malicious = toolboks_catalog::sha256_hex(b"attacker");
        std::fs::write(
            dir.join("active.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": "1.0.0", "confirmed": true, "launched": false, "sha256": malicious,
            }))
            .unwrap(),
        )
        .unwrap();
        let mut forged: Entry = Index::parse(&bytes).unwrap().entries.remove(0);
        forged.sha256 = malicious.clone();
        std::fs::write(
            dir.join("1.0.0/entry.json"),
            serde_json::to_vec(&forged).unwrap(),
        )
        .unwrap();
        let exe = dir.join("1.0.0/addon.exe");
        std::fs::write(&exe, b"attacker").unwrap();
        assert_eq!(
            crate::installer::native_exe(&root, "toolboks.sample")
                .unwrap()
                .1,
            malicious
        );
        let trusted = elevated_entry(&root, "toolboks.sample", "1.0.0").unwrap();
        assert!(trusted
            .check_download(&std::fs::read(exe).unwrap())
            .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn catalog_must_authorize_native_elevation_and_host_compatibility() {
        for (field, value) in [
            ("kind", serde_json::json!("declarative")),
            ("verified", serde_json::json!(false)),
            ("capabilities", serde_json::json!([])),
            ("min_toolboks_version", serde_json::json!("999.0.0")),
            ("protocol_version", serde_json::json!(999)),
        ] {
            let root = root(field);
            let mut index: serde_json::Value =
                serde_json::from_slice(&native_index(b"original")).unwrap();
            index["entries"][0][field] = value;
            accept(&root, &serde_json::to_vec(&index).unwrap()).unwrap();
            assert!(
                elevated_entry(&root, "toolboks.sample", "1.0.0").is_err(),
                "{field}"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn host_describes_this_build() {
        let host = host();
        // If the package version ever stops being plain x.y.z, every entry would be hidden.
        assert!(toolboks_addon_sdk::version(host.version).is_some());
        assert_eq!(host.preview, cfg!(feature = "preview"));
        let parsed = Index::parse(&index()).unwrap();
        let entry = &parsed.entries[0];
        // A preview-channel entry that needs exactly this version.
        let expected = if cfg!(feature = "preview") {
            Ok(())
        } else {
            Err(Hidden::PreviewChannel)
        };
        assert_eq!(entry.check(&host), expected);
        assert_eq!(parsed.verified("toolboks.sample", &host), expected.is_ok());
        assert_eq!(entry.check_download(b"hello"), Ok(()));
    }
    #[test]
    fn this_host_accepts_protocols_three_and_four_and_hides_the_others() {
        assert_eq!(PROTOCOL_VERSIONS, &[3, 4]);
        // Every version this build lists is one the protocol crate speaks.
        assert!(PROTOCOL_VERSIONS.iter().all(|v| {
            (toolboks_addon_proto::MIN_PROTOCOL_VERSION..=toolboks_addon_proto::PROTOCOL_VERSION)
                .contains(v)
        }));
        let mut index: Index = serde_json::from_slice(&index()).unwrap();
        let entry = &mut index.entries[0];
        entry.channel = Channel::Stable;
        for hidden in [0, 1, 2, 5, 99] {
            entry.protocol_version = hidden;
            assert_eq!(
                entry.check(&host()),
                Err(Hidden::UnknownProtocol),
                "{hidden}"
            );
        }
        for shown in [3, 4] {
            entry.protocol_version = shown;
            assert_eq!(entry.check(&host()), Ok(()), "{shown}");
        }
    }
}
