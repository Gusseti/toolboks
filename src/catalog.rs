//! Host side of the signed add-on catalog: the built-in trusted keys, what this build can run, and
//! the trust state kept between runs. The format and the verifier live in `toolboks-catalog`.
//!
//! [`accept`] is the one way an index gets in. [`Index::listing`] and [`Index::verified`] then give
//! the verified flag and the list order that replace `addons::OFFICIAL_ADDONS`.

// Nothing calls this yet: fetching, installing and the add-on list move over in later changes.
#![allow(dead_code, unused_imports)]

use crate::addons;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
pub use toolboks_catalog::{
    check_bytes, Channel, Clock, DownloadMismatch, Entry, Hidden, Host, Index, Kind, Refused,
    SystemClock, TrustState, TrustedKeys,
};

/// PLACEHOLDER. All zero means "no key": every index is refused until the owner replaces these
/// two constants, in a separate change, with the public keys made offline by
/// `catalog-sign keygen` (see docs/catalog-signing.md). Only public keys ever belong here.
const MAIN_KEY: [u8; 32] = [0; 32];
/// PLACEHOLDER, see [`MAIN_KEY`]. The recovery key can only rotate the main key.
const RECOVERY_KEY: [u8; 32] = [0; 32];
/// Sequence of the last rotation already folded into [`MAIN_KEY`]. Stays 0 until a rotated key is
/// built in; raise it then, so an old rotation statement cannot bring a retired key back.
const ROTATION_FLOOR: u64 = 0;

/// Add-on protocol versions this build can talk to.
pub const PROTOCOL_VERSIONS: &[u32] = &[toolboks_addon_proto::PROTOCOL_VERSION];

pub fn trusted_keys() -> TrustedKeys {
    TrustedKeys {
        main: MAIN_KEY,
        recovery: RECOVERY_KEY,
        rotation_floor: ROTATION_FLOOR,
    }
}

/// What this build can run. Preview-channel entries are shown by preview builds only.
pub fn host() -> Host<'static> {
    Host {
        version: env!("CARGO_PKG_VERSION"),
        protocols: PROTOCOL_VERSIONS,
        preview: cfg!(feature = "preview"),
    }
}

/// Serialises read, verify and save, so two fetches cannot move the sequence backwards.
static STATE: Mutex<()> = Mutex::new(());

/// The last accepted sequence and key rotation, in one file so both change together.
/// `write_atomic` stages the write in the sibling `trust.json.pending`.
fn state_path(root: &Path) -> PathBuf {
    root.join("catalog").join("trust.json")
}

/// A missing or unreadable file gives the same trust as a fresh install: the built-in keys and no
/// replay floor. A stored rotation is checked against the recovery key again on every use, so
/// editing the file cannot add a key.
pub fn load_state(root: &Path) -> TrustState {
    std::fs::read(state_path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Exact signed bytes are stored together, so an interrupted write cannot mix generations.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedIndex {
    index: Vec<u8>,
    signature: Vec<u8>,
}

fn cache_path(root: &Path) -> PathBuf {
    root.join("catalog").join("accepted.json")
}

/// Reverify at the time of use: cached files and install records are user-writable.
fn cached_with(keys: &TrustedKeys, clock: &dyn Clock, root: &Path) -> Result<Index, String> {
    use std::io::Read;
    // JSON byte arrays expand to at most four bytes per original byte.
    let limit =
        4 * (toolboks_catalog::MAX_INDEX_BYTES + toolboks_catalog::MAX_SIGNATURE_BYTES) + 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(cache_path(root))
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| {
            "Refresh the add-on catalog before starting with administrator rights.".to_string()
        })?;
    if bytes.len() > limit {
        return Err("The saved catalog is too large. Refresh the add-on catalog.".into());
    }
    let cached: CachedIndex = serde_json::from_slice(&bytes)
        .map_err(|_| "The saved catalog is damaged. Refresh the add-on catalog.".to_string())?;
    toolboks_catalog::verify(
        keys,
        &load_state(root),
        &cached.index,
        Some(&cached.signature),
        clock,
    )
    .map(|accepted| accepted.index)
    .map_err(|e| {
        format!("Refresh the add-on catalog before starting with administrator rights: {e}")
    })
}

/// The signed entry, never `active.json` or `entry.json`, authorizes an elevated start.
/// Rollbacks absent from the current signed index cannot be elevated.
pub fn elevated_entry(root: &Path, id: &str, version: &str) -> Result<Entry, String> {
    elevated_entry_with(&trusted_keys(), &SystemClock, root, id, version)
}

fn elevated_entry_with(
    keys: &TrustedKeys,
    clock: &dyn Clock,
    root: &Path,
    id: &str,
    version: &str,
) -> Result<Entry, String> {
    let _guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let index = cached_with(keys, clock, root)?;
    let entry = index
        .entries
        .into_iter()
        .find(|entry| entry.id == id && entry.version == version)
        .ok_or_else(|| {
            "This installed version is absent from the signed catalog. Update the add-on."
                .to_string()
        })?;
    if entry.kind != Kind::Native
        || !entry.verified
        || entry.check(&host()).is_err()
        || !entry
            .capabilities
            .iter()
            .any(|name| crate::addon_services::ELEVATING_CAPABILITIES.contains(&name.as_str()))
    {
        return Err(
            "The signed catalog does not permit this add-on to use administrator rights.".into(),
        );
    }
    Ok(entry)
}

/// Verifies a downloaded `index.json` (exact bytes) against its `index.json.sig` and remembers it.
/// `root` is `addons::data_root()`. An index that cannot be remembered is refused, since the
/// replay protection depends on it.
pub fn accept(root: &Path, index: &[u8], signature: Option<&[u8]>) -> Result<Index, Refused> {
    accept_with(&trusted_keys(), &SystemClock, root, index, signature)
}

fn accept_with(
    keys: &TrustedKeys,
    clock: &dyn Clock,
    root: &Path,
    index: &[u8],
    signature: Option<&[u8]>,
) -> Result<Index, Refused> {
    let _guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let state = load_state(root);
    let accepted = toolboks_catalog::verify(keys, &state, index, signature, clock)?;
    if accepted.state != state {
        let bytes = serde_json::to_vec_pretty(&accepted.state)
            .map_err(|e| Refused::NotPersisted(e.to_string()))?;
        addons::write_atomic(&state_path(root), &bytes).map_err(Refused::NotPersisted)?;
    }
    let cached = CachedIndex {
        index: index.to_vec(),
        signature: signature.ok_or(Refused::MissingSignature)?.to_vec(),
    };
    let bytes = serde_json::to_vec(&cached).map_err(|e| Refused::NotPersisted(e.to_string()))?;
    addons::write_atomic(&cache_path(root), &bytes).map_err(Refused::NotPersisted)?;
    Ok(accepted.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};
    use toolboks_catalog::{
        hex_encode, key_id, rotation_message, FixedClock, Rotation, SignatureFile,
    };

    // TEST KEYS ONLY: derived from fixed seeds and trusted by nothing outside this test module.
    const MAIN: [u8; 32] = [11; 32];
    const RECOVERY: [u8; 32] = [12; 32];
    const NEXT: [u8; 32] = [13; 32];
    const NOW: u64 = 1_800_000_000;

    fn public(seed: [u8; 32]) -> [u8; 32] {
        *KeyPair::from_seed(Seed::new(seed)).pk
    }

    fn sign(seed: [u8; 32], message: &[u8]) -> String {
        hex_encode(&KeyPair::from_seed(Seed::new(seed)).sk.sign(message, None)[..])
    }

    fn test_keys() -> TrustedKeys {
        TrustedKeys {
            main: public(MAIN),
            recovery: public(RECOVERY),
            rotation_floor: 0,
        }
    }

    fn index(sequence: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "sequence": sequence,
            "generated_at": NOW - 60,
            "expires_at": NOW + 3600,
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
                "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                "verified": true,
                "channel": "preview"
            }]
        }))
        .unwrap()
    }

    fn signed(seed: [u8; 32], bytes: &[u8], rotations: Vec<Rotation>) -> Vec<u8> {
        serde_json::to_vec(&SignatureFile {
            key_id: key_id(&public(seed)),
            signature: sign(seed, bytes),
            rotations,
        })
        .unwrap()
    }

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("toolboks-catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn accept_test(
        root: &Path,
        seed: [u8; 32],
        bytes: &[u8],
        rotations: Vec<Rotation>,
    ) -> Result<Index, Refused> {
        let signature = signed(seed, bytes, rotations);
        accept_with(
            &test_keys(),
            &FixedClock(NOW),
            root,
            bytes,
            Some(&signature),
        )
    }

    #[test]
    fn built_in_keys_never_trust_a_test_key() {
        let root = root("placeholder");
        let keys = trusted_keys();
        for seed in [MAIN, RECOVERY, NEXT] {
            assert_ne!(keys.main, public(seed));
            assert_ne!(keys.recovery, public(seed));
        }
        // The real entry point, with the real clock, refuses an index signed by a test key.
        let bytes = index(1);
        let refused = accept(&root, &bytes, Some(&signed(MAIN, &bytes, vec![])));
        assert!(refused.is_err());
        if keys.main == [0; 32] {
            assert_eq!(refused, Err(Refused::PlaceholderKeys));
        }
        assert!(!state_path(&root).exists());
    }

    #[test]
    fn remembers_the_accepted_sequence() {
        let root = root("sequence");
        assert!(accept_test(&root, MAIN, &index(5), vec![]).is_ok());
        assert_eq!(load_state(&root).last_sequence, 5);
        assert!(!root.join("catalog").join("trust.json.pending").exists());
        // Read back from disk on the next call, as after a restart.
        assert_eq!(
            accept_test(&root, MAIN, &index(4), vec![]),
            Err(Refused::Replayed { last: 5, got: 4 })
        );
        // A refused index changes nothing.
        assert_eq!(load_state(&root).last_sequence, 5);
        assert!(accept_test(&root, MAIN, &index(6), vec![]).is_ok());
        assert_eq!(load_state(&root).last_sequence, 6);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remembers_an_accepted_rotation() {
        let root = root("rotation");
        let new_key = public(NEXT);
        let rotation = Rotation {
            new_key: hex_encode(&new_key),
            key_id: key_id(&new_key),
            sequence: 1,
            issued_at: NOW - 10,
            signature: sign(RECOVERY, &rotation_message(&new_key, 1, NOW - 10)),
        };
        assert!(accept_test(&root, NEXT, &index(1), vec![rotation.clone()]).is_ok());
        assert_eq!(load_state(&root).rotation, Some(rotation));
        // The old main key is out, and the new one works without repeating the statement.
        assert_eq!(
            accept_test(&root, MAIN, &index(2), vec![]),
            Err(Refused::RetiredKey)
        );
        assert!(accept_test(&root, NEXT, &index(2), vec![]).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_damaged_state_file_is_a_fresh_start() {
        let root = root("damaged");
        std::fs::create_dir_all(root.join("catalog")).unwrap();
        std::fs::write(state_path(&root), b"{ not json").unwrap();
        assert_eq!(load_state(&root), TrustState::default());
        assert!(accept_test(&root, MAIN, &index(3), vec![]).is_ok());
        assert_eq!(load_state(&root).last_sequence, 3);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn native_index(sequence: u64, payload: &[u8]) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(&index(sequence)).unwrap();
        let entry = &mut value["entries"][0];
        entry["kind"] = "native".into();
        entry["channel"] = "stable".into();
        entry["capabilities"] = serde_json::json!(["administrator"]);
        entry["sha256"] = toolboks_catalog::sha256_hex(payload).into();
        entry["size"] = payload.len().into();
        serde_json::to_vec(&value).unwrap()
    }

    fn elevated_test(root: &Path, clock: u64) -> Result<Entry, String> {
        elevated_entry_with(
            &test_keys(),
            &FixedClock(clock),
            root,
            "toolboks.sample",
            "1.0.0",
        )
    }

    #[test]
    fn elevated_start_requires_a_current_untampered_signed_catalog() {
        let root = root("elevated-cache");
        assert!(elevated_test(&root, NOW).is_err(), "missing cache");
        let bytes = native_index(5, b"original");
        accept_test(&root, MAIN, &bytes, vec![]).unwrap();
        assert_eq!(
            elevated_test(&root, NOW).unwrap().sha256,
            toolboks_catalog::sha256_hex(b"original")
        );
        assert!(elevated_test(&root, NOW + 3600).is_err(), "expired index");
        assert!(elevated_entry_with(
            &test_keys(),
            &FixedClock(NOW),
            &root,
            "toolboks.other",
            "1.0.0"
        )
        .is_err());
        assert!(elevated_entry_with(
            &test_keys(),
            &FixedClock(NOW),
            &root,
            "toolboks.sample",
            "0.9.0"
        )
        .is_err());
        let saved = std::fs::read(cache_path(&root)).unwrap();
        let mut cached: CachedIndex = serde_json::from_slice(&saved).unwrap();
        cached.index = native_index(5, b"attacker");
        std::fs::write(cache_path(&root), serde_json::to_vec(&cached).unwrap()).unwrap();
        assert!(
            elevated_test(&root, NOW).is_err(),
            "replaced index without signature"
        );
        cached.signature = signed(RECOVERY, &cached.index, vec![]);
        std::fs::write(cache_path(&root), serde_json::to_vec(&cached).unwrap()).unwrap();
        assert!(
            elevated_test(&root, NOW).is_err(),
            "recovery cannot authorize execution"
        );
        std::fs::write(cache_path(&root), &saved).unwrap();
        accept_test(&root, MAIN, &native_index(6, b"newer"), vec![]).unwrap();
        std::fs::write(cache_path(&root), &saved).unwrap();
        assert!(
            elevated_test(&root, NOW).is_err(),
            "old cache below replay floor"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swapped_exe_and_install_records_do_not_authorize_elevation() {
        let root = root("elevated-records");
        let bytes = native_index(1, b"original");
        accept_test(&root, MAIN, &bytes, vec![]).unwrap();
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
        let trusted = elevated_test(&root, NOW).unwrap();
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
                serde_json::from_slice(&native_index(1, b"original")).unwrap();
            index["entries"][0][field] = value;
            accept_test(&root, MAIN, &serde_json::to_vec(&index).unwrap(), vec![]).unwrap();
            assert!(elevated_test(&root, NOW).is_err(), "{field}");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn host_describes_this_build() {
        let host = host();
        // If the package version ever stops being plain x.y.z, every entry would be hidden.
        assert!(toolboks_addon_sdk::version(host.version).is_some());
        assert_eq!(host.preview, cfg!(feature = "preview"));
        let parsed = Index::parse(&index(1)).unwrap();
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
}
