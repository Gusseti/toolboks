//! Offline tool that creates catalog keys and signs the Toolboks add-on index.
//!
//! Run it on the owner's own machine only, never in CI. The runbook is `docs/catalog-signing.md`.

use ed25519_compact::{KeyPair, Seed};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use toolboks_catalog::{
    check_signature, hex_decode, hex_encode, key_id, rotation_message, verify, Clock, FixedClock,
    Index, Rotation, SignatureFile, SystemClock, TrustState, TrustedKeys,
};

const USAGE: &str = "\
catalog-sign: offline signing for the Toolboks add-on catalog

  catalog-sign keygen <out-dir>
      Create a key pair: <out-dir>/catalog-private.key and <out-dir>/catalog-public.txt.
      Never overwrites. Run it once for the main key and once for the recovery key.

  catalog-sign sign <index.json> <private-key> [rotations.json]
      Check the index and write <index.json>.sig. A rotations.json beside the index is
      picked up automatically.

  catalog-sign rotate <recovery-private-key> <new-main-public-hex> [rotations.json]
      Append a statement, signed by the recovery key, that makes the given key the main key.

  catalog-sign verify <index.json> <main-public-hex> [recovery-public-hex]
      Check <index.json>.sig. With the recovery key, runs the same check as the app,
      rotations included.";

const PRIVATE_FILE: &str = "catalog-private.key";
const PUBLIC_FILE: &str = "catalog-public.txt";
const ROTATIONS_FILE: &str = "rotations.json";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args, SystemClock.now()) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(text) => {
            eprintln!("error: {text}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String], now: u64) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["keygen", dir] => keygen(Path::new(dir)),
        ["sign", index, key] => sign(Path::new(index), Path::new(key), None, now),
        ["sign", index, key, rotations] => sign(
            Path::new(index),
            Path::new(key),
            Some(Path::new(rotations)),
            now,
        ),
        ["rotate", key, new_main] => {
            rotate(Path::new(key), new_main, Path::new(ROTATIONS_FILE), now)
        }
        ["rotate", key, new_main, file] => rotate(Path::new(key), new_main, Path::new(file), now),
        ["verify", index, main] => check(Path::new(index), main, None, now),
        ["verify", index, main, recovery] => check(Path::new(index), main, Some(recovery), now),
        _ => Err(format!("unknown or incomplete command\n\n{USAGE}")),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn sig_path(index: &Path) -> PathBuf {
    // Appended, not `with_extension`: the file is `index.json.sig`.
    let mut name = index.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

fn public_key(hex: &str) -> Result<[u8; 32], String> {
    hex_decode::<32>(hex.trim())
        .ok_or_else(|| "a public key is 64 lowercase hex characters".to_string())
}

fn load_private(path: &Path) -> Result<KeyPair, String> {
    let text = String::from_utf8(read(path)?).map_err(|_| "the private key is not text")?;
    let seed = hex_decode::<32>(text.trim())
        .ok_or_else(|| format!("{} is not a catalog private key", path.display()))?;
    Ok(KeyPair::from_seed(Seed::new(seed)))
}

fn load_rotations(path: &Path) -> Result<Vec<Rotation>, String> {
    serde_json::from_slice(&read(path)?)
        .map_err(|e| format!("{} is not a rotation list: {e}", path.display()))
}

fn create_new(path: &Path, text: &str) -> Result<(), String> {
    // `create_new` fails if the file exists, so a key can never be overwritten, even in a race.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("cannot create {}: {e}", path.display()))
}

fn keygen(dir: &Path) -> Result<String, String> {
    let (private, public) = (dir.join(PRIVATE_FILE), dir.join(PUBLIC_FILE));
    if private.exists() || public.exists() {
        return Err(format!(
            "{} already holds a key; refusing to overwrite it",
            dir.display()
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let pair = KeyPair::generate();
    let public_hex = hex_encode(&pair.pk[..]);
    create_new(&private, &format!("{}\n", hex_encode(&pair.sk.seed()[..])))?;
    create_new(&public, &format!("{public_hex}\n"))?;
    Ok(format!(
        "public key: {public_hex}\nkey id:     {}\n\nprivate key written to {}\n\
         KEEP IT OFFLINE. Anyone who has this file can sign a catalog. Move it to offline\n\
         storage now, keep a second copy somewhere else, and never put it in a repository,\n\
         a cloud folder or CI.",
        key_id(&pair.pk),
        private.display()
    ))
}

fn sign(index: &Path, key: &Path, rotations: Option<&Path>, now: u64) -> Result<String, String> {
    let bytes = read(index)?;
    // The same structural checks the app runs after it has verified the signature.
    let parsed = Index::parse(&bytes).map_err(|e| format!("not signing: {e}"))?;
    if parsed.expires_at <= now {
        return Err("not signing: expires_at is in the past".into());
    }
    let pair = load_private(key)?;
    let beside = index.with_file_name(ROTATIONS_FILE);
    let rotations = match rotations {
        Some(path) => load_rotations(path)?,
        None if beside.exists() => load_rotations(&beside)?,
        None => Vec::new(),
    };
    if let Some(last) = rotations.last() {
        if last.new_key != hex_encode(&pair.pk[..]) {
            return Err(
                "not signing: this is not the main key named by the latest rotation".into(),
            );
        }
    }
    let file = SignatureFile {
        key_id: key_id(&pair.pk),
        signature: hex_encode(&pair.sk.sign(&bytes, None)[..]),
        rotations,
    };
    if !check_signature(&pair.pk, &bytes, &file.signature) {
        return Err("the new signature does not verify; nothing written".into());
    }
    let out = sig_path(index);
    let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    fs::write(&out, text + "\n").map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(format!(
        "signed sequence {} ({} entries, valid for {} more hours) with key {}\nwrote {}",
        parsed.sequence,
        parsed.entries.len(),
        (parsed.expires_at - now) / 3600,
        file.key_id,
        out.display()
    ))
}

fn rotate(key: &Path, new_main: &str, file: &Path, now: u64) -> Result<String, String> {
    let recovery = load_private(key)?;
    let new_key = public_key(new_main)?;
    if new_key == *recovery.pk {
        return Err("the recovery key cannot become the main key".into());
    }
    let mut rotations = if file.exists() {
        load_rotations(file)?
    } else {
        Vec::new()
    };
    let sequence = rotations.last().map_or(1, |last| last.sequence + 1);
    rotations.push(Rotation {
        new_key: hex_encode(&new_key),
        key_id: key_id(&new_key),
        sequence,
        issued_at: now,
        signature: hex_encode(
            &recovery
                .sk
                .sign(rotation_message(&new_key, sequence, now), None)[..],
        ),
    });
    let text = serde_json::to_string_pretty(&rotations).map_err(|e| e.to_string())?;
    fs::write(file, text + "\n").map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    Ok(format!(
        "rotation {sequence}: the main key is now {}\nwrote {}\n\
         Sign every index from now on with the new main key and this file. The old main key is\n\
         refused by each app as soon as it has seen one index signed this way.",
        key_id(&new_key),
        file.display()
    ))
}

fn check(index: &Path, main: &str, recovery: Option<&str>, now: u64) -> Result<String, String> {
    let bytes = read(index)?;
    let signature = read(&sig_path(index))?;
    let main = public_key(main)?;
    let parsed = match recovery {
        Some(recovery) => {
            let keys = TrustedKeys {
                main,
                recovery: public_key(recovery)?,
                rotation_floor: 0,
            };
            let state = TrustState::default();
            verify(&keys, &state, &bytes, Some(&signature), &FixedClock(now))
                .map_err(|e| format!("REFUSED: {e}"))?
                .index
        }
        None => {
            let file = SignatureFile::parse(&signature).map_err(|e| format!("REFUSED: {e}"))?;
            if file.key_id != key_id(&main) || !check_signature(&main, &bytes, &file.signature) {
                return Err("REFUSED: the index is not signed by this key".into());
            }
            let parsed = Index::parse(&bytes).map_err(|e| format!("REFUSED: {e}"))?;
            if parsed.expires_at <= now {
                return Err("REFUSED: the catalog has expired".into());
            }
            parsed
        }
    };
    Ok(format!(
        "OK: sequence {}, {} entries, valid for {} more hours",
        parsed.sequence,
        parsed.entries.len(),
        (parsed.expires_at - now) / 3600
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "toolboks-catalog-sign-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn call(args: &[&str]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        run(&args, NOW)
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    /// Throwaway test key pair in `dir/<name>`: returns (private key path, public hex).
    fn key(dir: &Path, name: &str) -> (String, String) {
        let out = dir.join(name);
        call(&["keygen", &text(&out)]).unwrap();
        let public = fs::read_to_string(out.join(PUBLIC_FILE)).unwrap();
        (text(&out.join(PRIVATE_FILE)), public.trim().to_string())
    }

    fn write_index(dir: &Path, expires_at: u64, download_url: &str) -> String {
        let index = serde_json::json!({
            "format_version": 1,
            "sequence": 3,
            "generated_at": NOW - 60,
            "expires_at": expires_at,
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
                "min_toolboks_version": "0.12.0",
                "protocol_version": 1,
                "download_url": download_url,
                "size": 5,
                "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                "verified": true,
                "channel": "stable"
            }]
        });
        let path = dir.join("index.json");
        fs::write(&path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();
        text(&path)
    }

    const URL: &str = "https://example.com/sample.tbaddon";

    #[test]
    fn keygen_never_overwrites() {
        let dir = temp("keygen");
        let (private, public) = key(&dir, "main");
        let before = fs::read(&private).unwrap();
        assert_eq!(public.len(), 64);
        let again = call(&["keygen", &text(&dir.join("main"))]).unwrap_err();
        assert!(again.contains("refusing to overwrite"), "{again}");
        assert_eq!(fs::read(&private).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sign_then_verify() {
        let dir = temp("sign");
        let (private, public) = key(&dir, "main");
        let (_, other) = key(&dir, "other");
        let index = write_index(&dir, NOW + 3600, URL);
        call(&["sign", &index, &private]).unwrap();
        assert!(dir.join("index.json.sig").exists());
        assert!(call(&["verify", &index, &public])
            .unwrap()
            .starts_with("OK"));
        assert!(call(&["verify", &index, &other]).is_err());
        // One changed byte after signing is caught.
        let mut bytes = fs::read(&index).unwrap();
        bytes.push(b'\n');
        fs::write(&index, bytes).unwrap();
        assert!(call(&["verify", &index, &public]).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sign_refuses_an_expired_or_invalid_index() {
        let dir = temp("refuse");
        let (private, _) = key(&dir, "main");
        let expired = write_index(&dir, NOW, URL);
        let error = call(&["sign", &expired, &private]).unwrap_err();
        assert!(error.contains("in the past"), "{error}");
        let invalid = write_index(&dir, NOW + 3600, "http://example.com/sample.tbaddon");
        let error = call(&["sign", &invalid, &private]).unwrap_err();
        assert!(error.contains("download_url"), "{error}");
        assert!(!dir.join("index.json.sig").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rotate_then_sign_with_the_new_key() {
        let dir = temp("rotate");
        let (old_private, old_public) = key(&dir, "main");
        let (recovery_private, recovery_public) = key(&dir, "recovery");
        let (new_private, new_public) = key(&dir, "next");
        let index = write_index(&dir, NOW + 3600, URL);
        let rotations = text(&dir.join(ROTATIONS_FILE));
        // The main key cannot rotate: the app refuses a statement it signed.
        call(&["rotate", &old_private, &new_public, &rotations]).unwrap();
        call(&["sign", &index, &new_private]).unwrap();
        let refused = call(&["verify", &index, &old_public, &recovery_public]).unwrap_err();
        assert!(refused.contains("recovery key"), "{refused}");
        fs::remove_file(&rotations).unwrap();
        // The recovery key can. The rotations file beside the index is picked up by `sign`.
        call(&["rotate", &recovery_private, &new_public, &rotations]).unwrap();
        let stale = call(&["sign", &index, &old_private]).unwrap_err();
        assert!(stale.contains("latest rotation"), "{stale}");
        call(&["sign", &index, &new_private]).unwrap();
        let ok = call(&["verify", &index, &old_public, &recovery_public]).unwrap();
        assert!(ok.starts_with("OK"), "{ok}");
        // The recovery key itself can never become the main key.
        let error = call(&["rotate", &recovery_private, &recovery_public, &rotations]).unwrap_err();
        assert!(error.contains("cannot become"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }
}
