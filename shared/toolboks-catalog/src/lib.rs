//! Signed add-on catalog: the `index.json` format, its detached signature file and the verifier.
//!
//! Trust model:
//! - The index is signed as the exact bytes of the file (no JSON canonicalisation) with the main
//!   ed25519 key. The signature lives beside it in `index.json.sig` ([`SignatureFile`]).
//! - A second, recovery key can only sign a [`Rotation`] statement that names a new main key. An
//!   index signed by the recovery key itself is refused.
//! - Nothing in the index is parsed before the signature over its bytes has been checked.
//! - A short validity window (`expires_at`) and a monotonic `sequence` stop an old index from being
//!   replayed to push an old, vulnerable add-on version.
//!
//! This crate only verifies. Keys are created and used offline by `tools/catalog-sign`; see
//! `docs/catalog-signing.md`. The entry point is [`verify`].

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use toolboks_addon_sdk::{valid_id, version};

/// The only index format this build understands. Additive fields do not bump it (unknown fields are
/// ignored); a change in meaning does, and older hosts then refuse the whole index.
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: usize = 64 * 1024;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_ROTATIONS: usize = 16;
/// Longest allowed gap between `generated_at` and `expires_at`. Keeps the replay window short even
/// if a far-future expiry is signed by mistake.
pub const MAX_VALIDITY_SECS: u64 = 90 * 24 * 60 * 60;
/// Sanity limit for the `size` an entry may declare.
pub const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
/// First line of the message a rotation signature covers. It is not valid JSON, so a rotation
/// signature can never double as an index signature.
const ROTATION_DOMAIN: &str = "toolboks-catalog-rotation-v1";

/// Lowercase hex.
pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 15)] as char);
    }
    out
}

/// Exactly `N` bytes of lowercase hex. Uppercase, signs, spaces and other lengths are refused, so
/// every value has one spelling.
pub fn hex_decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let text = text.as_bytes();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (byte, [high, low]) in out.iter_mut().zip(text.as_chunks::<2>().0) {
        *byte = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(out)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex_encode(&digest)
}

/// Id of a public key: the first 8 bytes of its SHA-256, in hex. Derived, never chosen, so two
/// keys cannot share an id by mistake and an id cannot be moved to another key.
pub fn key_id(key: &[u8; 32]) -> String {
    let digest: [u8; 32] = Sha256::digest(key).into();
    hex_encode(&digest[..8])
}

/// True when `signature_hex` is a valid ed25519 signature of `message` under `key`. The all-zero
/// placeholder key never verifies anything.
pub fn check_signature(key: &[u8; 32], message: &[u8], signature_hex: &str) -> bool {
    let Some(signature) = hex_decode::<64>(signature_hex) else {
        return false;
    };
    *key != [0u8; 32]
        && ed25519_compact::PublicKey::new(*key)
            .verify(message, &ed25519_compact::Signature::new(signature))
            .is_ok()
}

/// Seconds since the Unix epoch. Injected so tests and tools can pin the time.
pub trait Clock {
    fn now(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        // A clock before 1970 is broken: report the far future so every index counts as expired.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(u64::MAX, |d| d.as_secs())
    }
}

pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now(&self) -> u64 {
        self.0
    }
}

/// The public keys compiled into the verifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedKeys {
    /// Signs `index.json`.
    pub main: [u8; 32],
    /// Signs rotation statements only.
    pub recovery: [u8; 32],
    /// Rotation statements with a sequence at or below this are ignored. Raise it to the latest
    /// rotation's sequence whenever a rotated key becomes the built-in `main`, so an old statement
    /// cannot move trust back to a retired key.
    pub rotation_floor: u64,
}

impl TrustedKeys {
    fn usable(&self) -> bool {
        self.main != [0u8; 32] && self.recovery != [0u8; 32] && self.main != self.recovery
    }
}

/// Why an index was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The built-in keys are placeholders (all zero) or the two keys are the same.
    PlaceholderKeys,
    MissingSignature,
    MalformedSignature,
    /// Signed by a key this host does not know.
    UnknownKey,
    /// Signed by the built-in main key after a rotation replaced it.
    RetiredKey,
    /// The recovery key may only sign rotation statements.
    SignedByRecoveryKey,
    /// The signature does not match the bytes (tampered file or wrong key).
    BadSignature,
    RotationMalformed,
    RotationSignedByMainKey,
    RotationBadSignature,
    RotationOutOfOrder,
    UnknownFormat(u32),
    Malformed(String),
    Expired,
    /// The index is older than one this host already accepted.
    Replayed {
        last: u64,
        got: u64,
    },
    /// The index was good but the new trust state could not be saved (set by the host).
    NotPersisted(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlaceholderKeys => write!(f, "no catalog keys are configured"),
            Self::MissingSignature => write!(f, "the catalog signature is missing"),
            Self::MalformedSignature => write!(f, "the catalog signature file is malformed"),
            Self::UnknownKey => write!(f, "the catalog is signed by an unknown key"),
            Self::RetiredKey => write!(f, "the catalog is signed by a retired key"),
            Self::SignedByRecoveryKey => {
                write!(f, "the recovery key may not sign the catalog")
            }
            Self::BadSignature => write!(f, "the catalog signature does not match"),
            Self::RotationMalformed => write!(f, "a key rotation statement is malformed"),
            Self::RotationSignedByMainKey => {
                write!(f, "a key rotation must be signed by the recovery key")
            }
            Self::RotationBadSignature => {
                write!(
                    f,
                    "a key rotation statement is not signed by the recovery key"
                )
            }
            Self::RotationOutOfOrder => write!(f, "key rotation statements are out of order"),
            Self::UnknownFormat(v) => write!(f, "catalog format {v} is not supported"),
            Self::Malformed(why) => write!(f, "the catalog is malformed: {why}"),
            Self::Expired => write!(f, "the catalog has expired"),
            Self::Replayed { last, got } => {
                write!(
                    f,
                    "the catalog is older than the last one accepted ({got} < {last})"
                )
            }
            Self::NotPersisted(why) => write!(f, "the catalog state could not be saved: {why}"),
        }
    }
}

impl std::error::Error for Refused {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A `.tbaddon` package rendered by the host.
    Declarative,
    /// A native executable.
    Native,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Stable,
    Preview,
}

/// One add-on version in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub short_description: String,
    #[serde(default)]
    pub long_description: String,
    pub publisher: String,
    /// Reserved for per-publisher keys. Format 1 trusts the catalog key alone and does not check
    /// this; when present it must be shaped like a [`key_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_key_id: Option<String>,
    pub category: String,
    pub icon: String,
    pub accent: String,
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// Capability names as plain text, so a name this host does not know yet does not make the
    /// whole index unreadable. The installer decides what to do with unknown ones.
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub kind: Kind,
    pub version: String,
    pub min_toolboks_version: String,
    pub protocol_version: u32,
    pub download_url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the download.
    pub sha256: String,
    #[serde(default)]
    pub release_notes: String,
    /// Drives the verified checkmark and first position. Required: there is no default.
    pub verified: bool,
    pub channel: Channel,
}

/// What the running host can use. Built by the host from its own version and settings.
#[derive(Clone, Copy, Debug)]
pub struct Host<'a> {
    /// Strict `x.y.z`, normally `env!("CARGO_PKG_VERSION")`.
    pub version: &'a str,
    pub protocols: &'a [u32],
    /// Whether preview-channel entries are shown.
    pub preview: bool,
}

/// Why an entry of an accepted index is not offered on this host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hidden {
    NeedsNewerHost,
    UnknownProtocol,
    PreviewChannel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadMismatch {
    Size { expected: u64, got: u64 },
    Sha256,
}

/// Checks downloaded bytes against the size and SHA-256 the signed index declares.
pub fn check_bytes(bytes: &[u8], size: u64, sha256: &str) -> Result<(), DownloadMismatch> {
    let got = bytes.len() as u64;
    if got != size {
        return Err(DownloadMismatch::Size {
            expected: size,
            got,
        });
    }
    if sha256_hex(bytes) != sha256 {
        return Err(DownloadMismatch::Sha256);
    }
    Ok(())
}

fn text(value: &str, min: usize, max: usize, multiline: bool) -> bool {
    (min..=max).contains(&value.chars().count())
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && c == '\n'))
}

/// `https://host/...` with no user info and no port, the same shape the host's downloader accepts.
fn https_url(value: &str) -> bool {
    value.len() <= 2048
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
        && value.strip_prefix("https://").is_some_and(|rest| {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            !host.is_empty() && !host.contains(['@', ':'])
        })
}

impl Entry {
    /// Structural checks. Returns the name of the first bad field.
    pub fn validate(&self) -> Result<(), &'static str> {
        let ok = |pass: bool, field: &'static str| if pass { Ok(()) } else { Err(field) };
        ok(valid_id(&self.id), "id")?;
        ok(text(&self.title, 1, 80, false), "title")?;
        ok(
            text(&self.short_description, 1, 200, false),
            "short_description",
        )?;
        ok(
            text(&self.long_description, 0, 8000, true),
            "long_description",
        )?;
        ok(text(&self.publisher, 1, 80, false), "publisher")?;
        ok(
            self.publisher_key_id
                .as_deref()
                .is_none_or(|id| hex_decode::<8>(id).is_some()),
            "publisher_key_id",
        )?;
        ok(text(&self.category, 1, 40, false), "category")?;
        ok(text(&self.icon, 0, 64, false), "icon")?;
        ok(text(&self.accent, 0, 64, false), "accent")?;
        ok(
            self.screenshots.len() <= 8 && self.screenshots.iter().all(|url| https_url(url)),
            "screenshots",
        )?;
        let capability = |name: &String| {
            (1..=40).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        ok(
            self.capabilities.len() <= 32 && self.capabilities.iter().all(capability),
            "capabilities",
        )?;
        ok(version(&self.version).is_some(), "version")?;
        ok(
            version(&self.min_toolboks_version).is_some(),
            "min_toolboks_version",
        )?;
        ok(self.protocol_version >= 1, "protocol_version")?;
        ok(https_url(&self.download_url), "download_url")?;
        ok((1..=MAX_DOWNLOAD_BYTES).contains(&self.size), "size")?;
        ok(hex_decode::<32>(&self.sha256).is_some(), "sha256")?;
        ok(text(&self.release_notes, 0, 8000, true), "release_notes")?;
        Ok(())
    }

    /// Whether this host may offer the entry. Versions use the same strict `x.y.z` rule as add-on
    /// packages (`toolboks_addon_sdk::version`), and like there a host version that does not parse
    /// (a pre-release build) is treated as too old.
    pub fn check(&self, host: &Host) -> Result<(), Hidden> {
        match (version(host.version), version(&self.min_toolboks_version)) {
            (Some(have), Some(need)) if have >= need => {}
            _ => return Err(Hidden::NeedsNewerHost),
        }
        if !host.protocols.contains(&self.protocol_version) {
            return Err(Hidden::UnknownProtocol);
        }
        if self.channel == Channel::Preview && !host.preview {
            return Err(Hidden::PreviewChannel);
        }
        Ok(())
    }

    /// Checks a finished download against this entry's `size` and `sha256`.
    pub fn check_download(&self, bytes: &[u8]) -> Result<(), DownloadMismatch> {
        check_bytes(bytes, self.size, &self.sha256)
    }
}

/// The parsed `index.json`. Times are seconds since the Unix epoch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub format_version: u32,
    /// Goes up with every published index; a host never goes back to a lower one.
    pub sequence: u64,
    pub generated_at: u64,
    pub expires_at: u64,
    pub entries: Vec<Entry>,
}

impl Index {
    /// Parses and structurally validates index bytes. This does NOT check the signature: hosts go
    /// through [`verify`]; the signing tool uses this to refuse a bad index before signing it.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refused> {
        #[derive(Deserialize)]
        struct Head {
            format_version: u32,
        }
        if bytes.len() > MAX_INDEX_BYTES {
            return Err(Refused::Malformed("the index is too large".into()));
        }
        let malformed = |e: serde_json::Error| Refused::Malformed(e.to_string());
        // The version is read on its own first, so a future format with other fields is reported
        // as unknown rather than as broken.
        let head: Head = serde_json::from_slice(bytes).map_err(malformed)?;
        if head.format_version != FORMAT_VERSION {
            return Err(Refused::UnknownFormat(head.format_version));
        }
        let index: Self = serde_json::from_slice(bytes).map_err(malformed)?;
        index.validate()?;
        Ok(index)
    }

    pub fn validate(&self) -> Result<(), Refused> {
        let bad = |why: String| Err(Refused::Malformed(why));
        if self.format_version != FORMAT_VERSION {
            return Err(Refused::UnknownFormat(self.format_version));
        }
        if self.sequence == 0 {
            return bad("sequence must be at least 1".into());
        }
        if self.expires_at <= self.generated_at
            || self.expires_at - self.generated_at > MAX_VALIDITY_SECS
        {
            return bad("expires_at must be after generated_at, by at most 90 days".into());
        }
        if self.entries.len() > MAX_ENTRIES {
            return bad("too many entries".into());
        }
        let mut seen = HashSet::new();
        for (position, entry) in self.entries.iter().enumerate() {
            if let Err(field) = entry.validate() {
                return bad(format!("entry {position}: bad {field}"));
            }
            if !seen.insert((entry.id.as_str(), entry.version.as_str())) {
                return bad(format!("entry {position}: duplicate id and version"));
            }
        }
        Ok(())
    }

    /// What the add-on list shows: one entry per add-on (the newest version this host can use),
    /// verified add-ons first, otherwise in index order. This is the replacement for the host's
    /// hardcoded `OFFICIAL_ADDONS` list.
    pub fn listing(&self, host: &Host) -> Vec<&Entry> {
        let mut picked: Vec<&Entry> = Vec::new();
        for entry in self.entries.iter().filter(|e| e.check(host).is_ok()) {
            match picked.iter_mut().find(|p| p.id == entry.id) {
                Some(slot) => {
                    if version(&entry.version) > version(&slot.version) {
                        *slot = entry;
                    }
                }
                None => picked.push(entry),
            }
        }
        // Stable sort: index order is kept inside each group.
        picked.sort_by_key(|e| !e.verified);
        picked
    }

    /// The verified flag of the version [`Index::listing`] offers for `id`.
    pub fn verified(&self, id: &str, host: &Host) -> bool {
        self.listing(host).iter().any(|e| e.id == id && e.verified)
    }
}

/// Recovery-key statement that makes `new_key` the main key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rotation {
    /// The new main public key, hex.
    pub new_key: String,
    /// [`key_id`] of `new_key`.
    pub key_id: String,
    /// Starts at 1 and goes up; a host never goes back to a lower rotation.
    pub sequence: u64,
    pub issued_at: u64,
    /// Hex signature by the recovery key over [`rotation_message`].
    pub signature: String,
}

/// The exact bytes a rotation signature covers. Built from the fields, which each have a single
/// spelling, so no JSON canonicalisation is involved.
pub fn rotation_message(new_key: &[u8; 32], sequence: u64, issued_at: u64) -> Vec<u8> {
    format!(
        "{ROTATION_DOMAIN}\n{}\n{}\n{sequence}\n{issued_at}\n",
        key_id(new_key),
        hex_encode(new_key)
    )
    .into_bytes()
}

impl Rotation {
    fn shaped(&self) -> bool {
        hex_decode::<32>(&self.new_key).is_some()
            && hex_decode::<8>(&self.key_id).is_some()
            && hex_decode::<64>(&self.signature).is_some()
    }

    /// The new main key, if the recovery key really signed this statement.
    fn trusted_key(&self, keys: &TrustedKeys) -> Result<[u8; 32], Refused> {
        let key = hex_decode::<32>(&self.new_key).ok_or(Refused::RotationMalformed)?;
        // The recovery key must never end up signing indexes, not even by naming itself.
        if self.sequence == 0
            || key == [0u8; 32]
            || key == keys.recovery
            || self.key_id != key_id(&key)
        {
            return Err(Refused::RotationMalformed);
        }
        let message = rotation_message(&key, self.sequence, self.issued_at);
        if check_signature(&keys.recovery, &message, &self.signature) {
            Ok(key)
        } else if check_signature(&keys.main, &message, &self.signature) {
            Err(Refused::RotationSignedByMainKey)
        } else {
            Err(Refused::RotationBadSignature)
        }
    }
}

/// The detached `index.json.sig`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureFile {
    /// [`key_id`] of the key that signed the index.
    pub key_id: String,
    /// Hex ed25519 signature over the exact bytes of `index.json`.
    pub signature: String,
    /// Every rotation so far, oldest first. Carried by each signature file after a rotation so a
    /// fresh install learns the current main key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rotations: Vec<Rotation>,
}

impl SignatureFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, Refused> {
        if bytes.len() > MAX_SIGNATURE_BYTES {
            return Err(Refused::MalformedSignature);
        }
        let file: Self = serde_json::from_slice(bytes).map_err(|_| Refused::MalformedSignature)?;
        let shaped = hex_decode::<8>(&file.key_id).is_some()
            && hex_decode::<64>(&file.signature).is_some()
            && file.rotations.len() <= MAX_ROTATIONS
            && file.rotations.iter().all(Rotation::shaped);
        if shaped {
            Ok(file)
        } else {
            Err(Refused::MalformedSignature)
        }
    }
}

/// What a host remembers between runs. The host persists it after every accepted index.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustState {
    /// Sequence of the last accepted index.
    #[serde(default)]
    pub last_sequence: u64,
    /// The newest accepted rotation. Kept whole, with its signature, and checked again on every
    /// use, so a damaged or edited state file cannot introduce a key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
}

/// An accepted index and the trust state to persist with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub index: Index,
    pub state: TrustState,
}

/// The newest rotation that counts and the main key in force. `offered` is the chain from the
/// signature file; every statement in it must be good, even ones this host is already past.
fn current_main(
    keys: &TrustedKeys,
    state: &TrustState,
    offered: &[Rotation],
) -> Result<(Option<Rotation>, [u8; 32]), Refused> {
    // A stored rotation only counts while the built-in recovery key still vouches for it and it is
    // above the floor. After a host update with new built-in keys it simply drops out.
    let mut current = state
        .rotation
        .as_ref()
        .filter(|r| r.sequence > keys.rotation_floor)
        .and_then(|r| Some((r.clone(), r.trusted_key(keys).ok()?)));
    let mut previous = 0;
    for rotation in offered {
        let key = rotation.trusted_key(keys)?;
        if rotation.sequence <= previous {
            return Err(Refused::RotationOutOfOrder);
        }
        previous = rotation.sequence;
        let seen = current
            .as_ref()
            .map_or(keys.rotation_floor, |(r, _)| r.sequence);
        if rotation.sequence > seen {
            current = Some((rotation.clone(), key));
        }
    }
    Ok(match current {
        Some((rotation, key)) => (Some(rotation), key),
        None => (None, keys.main),
    })
}

/// Verifies `index` (the exact bytes of `index.json`) against `signature` (the bytes of
/// `index.json.sig`, `None` when it could not be fetched).
///
/// On success the caller MUST persist `Accepted::state` before using the index; it carries the
/// replay floor and any accepted key rotation. Nothing is remembered when an index is refused.
pub fn verify(
    keys: &TrustedKeys,
    state: &TrustState,
    index: &[u8],
    signature: Option<&[u8]>,
    clock: &dyn Clock,
) -> Result<Accepted, Refused> {
    if !keys.usable() {
        return Err(Refused::PlaceholderKeys);
    }
    let signature = match signature {
        Some(bytes) if !bytes.is_empty() => SignatureFile::parse(bytes)?,
        _ => return Err(Refused::MissingSignature),
    };
    let (rotation, main) = current_main(keys, state, &signature.rotations)?;
    if signature.key_id != key_id(&main) {
        return Err(if signature.key_id == key_id(&keys.recovery) {
            Refused::SignedByRecoveryKey
        } else if rotation.is_some() && signature.key_id == key_id(&keys.main) {
            Refused::RetiredKey
        } else {
            Refused::UnknownKey
        });
    }
    if !check_signature(&main, index, &signature.signature) {
        // Refused either way; this only tells the owner which mistake was made.
        return Err(
            if check_signature(&keys.recovery, index, &signature.signature) {
                Refused::SignedByRecoveryKey
            } else {
                Refused::BadSignature
            },
        );
    }
    // Only now, with the bytes authenticated, is the JSON looked at.
    let index = Index::parse(index)?;
    if clock.now() >= index.expires_at {
        return Err(Refused::Expired);
    }
    if index.sequence < state.last_sequence {
        return Err(Refused::Replayed {
            last: state.last_sequence,
            got: index.sequence,
        });
    }
    let state = TrustState {
        last_sequence: index.sequence,
        rotation,
    };
    Ok(Accepted { index, state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    // TEST KEYS ONLY: derived from fixed seeds, never trusted outside this test module.
    const MAIN: [u8; 32] = [1; 32];
    const RECOVERY: [u8; 32] = [2; 32];
    const NEXT: [u8; 32] = [3; 32];
    const STRANGER: [u8; 32] = [4; 32];
    const NOW: u64 = 1_800_000_000;

    fn public(seed: [u8; 32]) -> [u8; 32] {
        *KeyPair::from_seed(Seed::new(seed)).pk
    }

    fn sign(seed: [u8; 32], message: &[u8]) -> String {
        hex_encode(&KeyPair::from_seed(Seed::new(seed)).sk.sign(message, None)[..])
    }

    fn keys() -> TrustedKeys {
        TrustedKeys {
            main: public(MAIN),
            recovery: public(RECOVERY),
            rotation_floor: 0,
        }
    }

    fn entry(id: &str, version: &str) -> Entry {
        Entry {
            id: id.into(),
            title: "Sample".into(),
            short_description: "A sample add-on".into(),
            long_description: String::new(),
            publisher: "Toolboks".into(),
            publisher_key_id: None,
            category: "system".into(),
            icon: "box".into(),
            accent: "#4f8cff".into(),
            screenshots: vec![],
            capabilities: vec!["read_system_info".into()],
            kind: Kind::Declarative,
            version: version.into(),
            min_toolboks_version: "0.12.0".into(),
            protocol_version: 1,
            download_url: format!("https://example.com/{id}/{version}/package.tbaddon"),
            size: 5,
            sha256: sha256_hex(b"hello"),
            release_notes: String::new(),
            verified: true,
            channel: Channel::Stable,
        }
    }

    fn index(sequence: u64) -> Index {
        Index {
            format_version: FORMAT_VERSION,
            sequence,
            generated_at: NOW - 100,
            expires_at: NOW + 1000,
            entries: vec![entry("toolboks.sample", "1.0.0")],
        }
    }

    fn bytes(index: &Index) -> Vec<u8> {
        serde_json::to_vec_pretty(index).unwrap()
    }

    fn signed(seed: [u8; 32], bytes: &[u8], rotations: Vec<Rotation>) -> Vec<u8> {
        serde_json::to_vec(&SignatureFile {
            key_id: key_id(&public(seed)),
            signature: sign(seed, bytes),
            rotations,
        })
        .unwrap()
    }

    fn rotation(signer: [u8; 32], new: [u8; 32], sequence: u64) -> Rotation {
        let new_key = public(new);
        Rotation {
            new_key: hex_encode(&new_key),
            key_id: key_id(&new_key),
            sequence,
            issued_at: NOW - 50,
            signature: sign(signer, &rotation_message(&new_key, sequence, NOW - 50)),
        }
    }

    fn check(
        state: &TrustState,
        index: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<Accepted, Refused> {
        verify(&keys(), state, index, signature, &FixedClock(NOW))
    }

    fn fresh() -> TrustState {
        TrustState::default()
    }

    fn host() -> Host<'static> {
        Host {
            version: "0.12.5",
            protocols: &[1],
            preview: false,
        }
    }

    #[test]
    fn accepts_a_good_index() {
        let index = index(7);
        let bytes = bytes(&index);
        let accepted = check(&fresh(), &bytes, Some(&signed(MAIN, &bytes, vec![]))).unwrap();
        assert_eq!(accepted.index, index);
        assert_eq!(accepted.state.last_sequence, 7);
        assert_eq!(accepted.state.rotation, None);
    }

    #[test]
    fn refuses_a_missing_signature() {
        let bytes = bytes(&index(1));
        assert_eq!(
            check(&fresh(), &bytes, None),
            Err(Refused::MissingSignature)
        );
        assert_eq!(
            check(&fresh(), &bytes, Some(b"")),
            Err(Refused::MissingSignature)
        );
    }

    #[test]
    fn refuses_a_malformed_signature() {
        let bytes = bytes(&index(1));
        let good: SignatureFile = serde_json::from_slice(&signed(MAIN, &bytes, vec![])).unwrap();
        let short = SignatureFile {
            signature: good.signature[..126].to_string(),
            ..good.clone()
        };
        let upper = SignatureFile {
            signature: good.signature.to_uppercase(),
            ..good.clone()
        };
        let not_hex = SignatureFile {
            signature: "zz".repeat(64),
            ..good.clone()
        };
        let bad_id = SignatureFile {
            key_id: "main".into(),
            ..good
        };
        let mut cases: Vec<Vec<u8>> = [short, upper, not_hex, bad_id]
            .iter()
            .map(|file| serde_json::to_vec(file).unwrap())
            .collect();
        cases.push(b"not json".to_vec());
        cases.push(b"{}".to_vec());
        cases.push(sign(MAIN, &bytes).into_bytes());
        for case in cases {
            assert_eq!(
                check(&fresh(), &bytes, Some(&case)),
                Err(Refused::MalformedSignature),
                "{}",
                String::from_utf8_lossy(&case)
            );
        }
    }

    #[test]
    fn refuses_a_wrong_key() {
        let bytes = bytes(&index(1));
        // A stranger signing under their own key id.
        assert_eq!(
            check(&fresh(), &bytes, Some(&signed(STRANGER, &bytes, vec![]))),
            Err(Refused::UnknownKey)
        );
        // A stranger claiming the main key's id.
        let forged = serde_json::to_vec(&SignatureFile {
            key_id: key_id(&public(MAIN)),
            signature: sign(STRANGER, &bytes),
            rotations: vec![],
        })
        .unwrap();
        assert_eq!(
            check(&fresh(), &bytes, Some(&forged)),
            Err(Refused::BadSignature)
        );
    }

    #[test]
    fn refuses_an_index_signed_by_the_recovery_key() {
        let bytes = bytes(&index(1));
        assert_eq!(
            check(&fresh(), &bytes, Some(&signed(RECOVERY, &bytes, vec![]))),
            Err(Refused::SignedByRecoveryKey)
        );
        // The same signature filed under the main key's id.
        let relabelled = serde_json::to_vec(&SignatureFile {
            key_id: key_id(&public(MAIN)),
            signature: sign(RECOVERY, &bytes),
            rotations: vec![],
        })
        .unwrap();
        assert_eq!(
            check(&fresh(), &bytes, Some(&relabelled)),
            Err(Refused::SignedByRecoveryKey)
        );
    }

    #[test]
    fn refuses_tampered_bytes() {
        let original = bytes(&index(1));
        let signature = signed(MAIN, &original, vec![]);
        // Point the entry at another hash: the signature was made over the original bytes.
        let mut changed = index(1);
        changed.entries[0].sha256 = sha256_hex(b"evil!");
        assert_eq!(
            check(&fresh(), &bytes(&changed), Some(&signature)),
            Err(Refused::BadSignature)
        );
        // Even a change that keeps the JSON equal (trailing space) is refused: bytes are signed.
        let mut padded = original.clone();
        padded.push(b' ');
        assert_eq!(
            check(&fresh(), &padded, Some(&signature)),
            Err(Refused::BadSignature)
        );
    }

    #[test]
    fn refuses_an_expired_index() {
        let index = index(1);
        let bytes = bytes(&index);
        let signature = signed(MAIN, &bytes, vec![]);
        for now in [index.expires_at, index.expires_at + 1, u64::MAX] {
            assert_eq!(
                verify(
                    &keys(),
                    &fresh(),
                    &bytes,
                    Some(&signature),
                    &FixedClock(now)
                ),
                Err(Refused::Expired)
            );
        }
        let last_second = FixedClock(index.expires_at - 1);
        assert!(verify(&keys(), &fresh(), &bytes, Some(&signature), &last_second).is_ok());
    }

    #[test]
    fn refuses_a_replayed_lower_sequence() {
        let state = TrustState {
            last_sequence: 5,
            rotation: None,
        };
        let old = bytes(&index(4));
        assert_eq!(
            check(&state, &old, Some(&signed(MAIN, &old, vec![]))),
            Err(Refused::Replayed { last: 5, got: 4 })
        );
        // The same index again is fine, and a newer one moves the floor.
        let same = bytes(&index(5));
        assert!(check(&state, &same, Some(&signed(MAIN, &same, vec![]))).is_ok());
        let newer = bytes(&index(6));
        let accepted = check(&state, &newer, Some(&signed(MAIN, &newer, vec![]))).unwrap();
        assert_eq!(accepted.state.last_sequence, 6);
    }

    #[test]
    fn refuses_an_unknown_format_version() {
        let mut future = index(1);
        future.format_version = 2;
        let bytes = bytes(&future);
        assert_eq!(
            check(&fresh(), &bytes, Some(&signed(MAIN, &bytes, vec![]))),
            Err(Refused::UnknownFormat(2))
        );
        // A future format with a different shape is still reported as unknown, not as broken.
        let other = br#"{"format_version": 3, "packages": {}}"#;
        assert_eq!(
            check(&fresh(), other, Some(&signed(MAIN, other, vec![]))),
            Err(Refused::UnknownFormat(3))
        );
    }

    #[test]
    fn refuses_a_structurally_bad_index() {
        let refused = |change: fn(&mut Index)| {
            let mut index = index(1);
            change(&mut index);
            let bytes = bytes(&index);
            let result = check(&fresh(), &bytes, Some(&signed(MAIN, &bytes, vec![])));
            assert!(matches!(result, Err(Refused::Malformed(_))), "{result:?}");
        };
        refused(|i| i.sequence = 0);
        refused(|i| i.expires_at = i.generated_at);
        refused(|i| i.expires_at = i.generated_at + MAX_VALIDITY_SECS + 1);
        refused(|i| i.entries[0].id = "Not An Id".into());
        refused(|i| i.entries[0].version = "1.0".into());
        refused(|i| i.entries[0].min_toolboks_version = "newest".into());
        refused(|i| i.entries[0].download_url = "http://example.com/a".into());
        refused(|i| i.entries[0].download_url = "https://user@example.com/a".into());
        refused(|i| i.entries[0].sha256 = "abc".into());
        refused(|i| i.entries[0].size = 0);
        refused(|i| i.entries[0].title = String::new());
        refused(|i| i.entries[0].capabilities = vec!["Run Anything".into()]);
        refused(|i| i.entries[0].publisher_key_id = Some("someone".into()));
        refused(|i| i.entries.push(i.entries[0].clone()));
        // A missing `verified` flag is an error, never a silent default.
        let mut value = serde_json::to_value(index(1)).unwrap();
        value["entries"][0]
            .as_object_mut()
            .unwrap()
            .remove("verified");
        let bytes = serde_json::to_vec(&value).unwrap();
        let result = check(&fresh(), &bytes, Some(&signed(MAIN, &bytes, vec![])));
        assert!(matches!(result, Err(Refused::Malformed(_))), "{result:?}");
    }

    #[test]
    fn placeholder_keys_refuse_everything() {
        let bytes = bytes(&index(1));
        let signature = signed(MAIN, &bytes, vec![]);
        let zero = TrustedKeys {
            main: [0; 32],
            recovery: [0; 32],
            rotation_floor: 0,
        };
        let half = TrustedKeys {
            recovery: [0; 32],
            ..keys()
        };
        let same = TrustedKeys {
            recovery: public(MAIN),
            ..keys()
        };
        for keys in [zero, half, same] {
            assert_eq!(
                verify(&keys, &fresh(), &bytes, Some(&signature), &FixedClock(NOW)),
                Err(Refused::PlaceholderKeys)
            );
        }
        assert!(!check_signature(&[0; 32], &bytes, &"0".repeat(128)));
    }

    #[test]
    fn hides_an_addon_that_needs_a_newer_host() {
        let mut entry = entry("toolboks.sample", "1.0.0");
        entry.min_toolboks_version = "0.12.6".into();
        assert_eq!(entry.check(&host()), Err(Hidden::NeedsNewerHost));
        entry.min_toolboks_version = "1.0.0".into();
        assert_eq!(entry.check(&host()), Err(Hidden::NeedsNewerHost));
        entry.min_toolboks_version = "0.12.5".into();
        assert_eq!(entry.check(&host()), Ok(()));
        // 0.9.0 is older than 0.12.5: numbers are compared, not text.
        entry.min_toolboks_version = "0.9.0".into();
        assert_eq!(entry.check(&host()), Ok(()));
        // A host version that cannot be read is treated as too old.
        let odd = Host {
            version: "0.13.0-rc.1",
            ..host()
        };
        assert_eq!(entry.check(&odd), Err(Hidden::NeedsNewerHost));
    }

    #[test]
    fn hides_an_unknown_protocol_version() {
        let mut entry = entry("toolboks.sample", "1.0.0");
        entry.protocol_version = 2;
        assert_eq!(entry.check(&host()), Err(Hidden::UnknownProtocol));
        let newer = Host {
            protocols: &[1, 2],
            ..host()
        };
        assert_eq!(entry.check(&newer), Ok(()));
    }

    #[test]
    fn hides_a_preview_entry_on_stable() {
        let mut entry = entry("toolboks.sample", "1.0.0");
        entry.channel = Channel::Preview;
        assert_eq!(entry.check(&host()), Err(Hidden::PreviewChannel));
        let preview = Host {
            preview: true,
            ..host()
        };
        assert_eq!(entry.check(&preview), Ok(()));
    }

    #[test]
    fn refuses_a_rotation_signed_by_the_wrong_key() {
        let bytes = bytes(&index(1));
        for (signer, expected) in [
            (MAIN, Refused::RotationSignedByMainKey),
            (STRANGER, Refused::RotationBadSignature),
            (NEXT, Refused::RotationBadSignature),
        ] {
            let chain = vec![rotation(signer, NEXT, 1)];
            assert_eq!(
                check(&fresh(), &bytes, Some(&signed(NEXT, &bytes, chain.clone()))),
                Err(expected.clone())
            );
            // A bad statement spoils the file even when the main key signed the index itself.
            assert_eq!(
                check(&fresh(), &bytes, Some(&signed(MAIN, &bytes, chain))),
                Err(expected)
            );
        }
    }

    #[test]
    fn refuses_a_bad_rotation_statement() {
        let bytes = bytes(&index(1));
        // The recovery key may not name itself as the main key.
        let to_recovery = vec![rotation(RECOVERY, RECOVERY, 1)];
        assert_eq!(
            check(
                &fresh(),
                &bytes,
                Some(&signed(RECOVERY, &bytes, to_recovery))
            ),
            Err(Refused::RotationMalformed)
        );
        // A signed statement cannot be re-pointed at another key or sequence.
        let mut moved = rotation(RECOVERY, NEXT, 1);
        moved.new_key = hex_encode(&public(STRANGER));
        moved.key_id = key_id(&public(STRANGER));
        assert_eq!(
            check(
                &fresh(),
                &bytes,
                Some(&signed(STRANGER, &bytes, vec![moved]))
            ),
            Err(Refused::RotationBadSignature)
        );
        let mut bumped = rotation(RECOVERY, NEXT, 1);
        bumped.sequence = 9;
        assert_eq!(
            check(&fresh(), &bytes, Some(&signed(NEXT, &bytes, vec![bumped]))),
            Err(Refused::RotationBadSignature)
        );
        let backwards = vec![rotation(RECOVERY, NEXT, 2), rotation(RECOVERY, STRANGER, 1)];
        assert_eq!(
            check(&fresh(), &bytes, Some(&signed(NEXT, &bytes, backwards))),
            Err(Refused::RotationOutOfOrder)
        );
    }

    #[test]
    fn accepts_a_good_index_after_rotation() {
        let chain = vec![rotation(RECOVERY, NEXT, 1)];
        let first = bytes(&index(1));
        let accepted = check(&fresh(), &first, Some(&signed(NEXT, &first, chain.clone()))).unwrap();
        assert_eq!(accepted.state.rotation, Some(chain[0].clone()));
        // The remembered rotation is enough: later files need not repeat the chain.
        let second = bytes(&index(2));
        let again = check(
            &accepted.state,
            &second,
            Some(&signed(NEXT, &second, vec![])),
        )
        .unwrap();
        assert_eq!(again.state.rotation, Some(chain[0].clone()));
        assert_eq!(again.state.last_sequence, 2);
    }

    #[test]
    fn refuses_the_old_main_key_after_rotation() {
        let chain = vec![rotation(RECOVERY, NEXT, 1)];
        let first = bytes(&index(1));
        let state = check(&fresh(), &first, Some(&signed(NEXT, &first, chain.clone())))
            .unwrap()
            .state;
        let later = bytes(&index(2));
        // With the chain left out (what a holder of the leaked old key would send) and with it.
        for rotations in [vec![], chain] {
            assert_eq!(
                check(&state, &later, Some(&signed(MAIN, &later, rotations))),
                Err(Refused::RetiredKey)
            );
        }
    }

    #[test]
    fn never_goes_back_to_an_older_rotation() {
        let first = rotation(RECOVERY, NEXT, 1);
        let second = rotation(RECOVERY, STRANGER, 2);
        let bytes = bytes(&index(1));
        let chain = vec![first.clone(), second.clone()];
        let state = check(&fresh(), &bytes, Some(&signed(STRANGER, &bytes, chain)))
            .unwrap()
            .state;
        assert_eq!(state.rotation, Some(second));
        // Replaying only the first statement does not bring its key back.
        let replay = signed(NEXT, &bytes, vec![first.clone()]);
        assert_eq!(
            check(&state, &bytes, Some(&replay)),
            Err(Refused::UnknownKey)
        );
        // The built-in floor does the same for a fresh install of a newer host.
        let raised = TrustedKeys {
            rotation_floor: 1,
            ..keys()
        };
        let clock = FixedClock(NOW);
        let replay = signed(NEXT, &bytes, vec![first.clone()]);
        assert_eq!(
            verify(&raised, &fresh(), &bytes, Some(&replay), &clock),
            Err(Refused::UnknownKey)
        );
        // A stored rotation at or below the floor, or one the recovery key no longer vouches for,
        // drops out and the built-in main key is in force again.
        let stale = TrustState {
            last_sequence: 0,
            rotation: Some(first),
        };
        let by_main = signed(MAIN, &bytes, vec![]);
        assert!(verify(&raised, &stale, &bytes, Some(&by_main), &clock).is_ok());
        let edited = TrustState {
            last_sequence: 0,
            rotation: Some(rotation(STRANGER, STRANGER, 5)),
        };
        assert!(check(&edited, &bytes, Some(&by_main)).is_ok());
        let by_stranger = signed(STRANGER, &bytes, vec![]);
        assert_eq!(
            check(&edited, &bytes, Some(&by_stranger)),
            Err(Refused::UnknownKey)
        );
    }

    #[test]
    fn checks_download_size_and_sha256() {
        let entry = entry("toolboks.sample", "1.0.0");
        assert_eq!(
            entry.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(entry.check_download(b"hello"), Ok(()));
        assert_eq!(
            entry.check_download(b"hello!"),
            Err(DownloadMismatch::Size {
                expected: 5,
                got: 6
            })
        );
        assert_eq!(
            entry.check_download(b"jello"),
            Err(DownloadMismatch::Sha256)
        );
        assert_eq!(check_bytes(b"", 0, &sha256_hex(b"")), Ok(()));
        assert_eq!(
            check_bytes(b"hello", 5, &entry.sha256.to_uppercase()),
            Err(DownloadMismatch::Sha256)
        );
    }

    #[test]
    fn lists_verified_first_and_the_newest_usable_version() {
        let mut community = entry("community.widget", "1.0.0");
        community.verified = false;
        let old = entry("toolboks.sample", "1.9.0");
        let new = entry("toolboks.sample", "1.10.0");
        let mut too_new = entry("toolboks.sample", "2.0.0");
        too_new.min_toolboks_version = "9.0.0".into();
        let mut preview = entry("toolboks.preview-only", "0.1.0");
        preview.channel = Channel::Preview;
        let mut index = index(1);
        index.entries = vec![community, old, too_new, new, preview];
        assert_eq!(index.validate(), Ok(()));
        let listed: Vec<(&str, &str)> = index
            .listing(&host())
            .iter()
            .map(|e| (e.id.as_str(), e.version.as_str()))
            .collect();
        assert_eq!(
            listed,
            [("toolboks.sample", "1.10.0"), ("community.widget", "1.0.0")]
        );
        assert!(index.verified("toolboks.sample", &host()));
        assert!(!index.verified("community.widget", &host()));
        assert!(!index.verified("toolboks.preview-only", &host()));
        assert!(!index.verified("toolboks.missing", &host()));
    }

    #[test]
    fn hex_has_one_spelling() {
        assert_eq!(hex_encode(&[0, 15, 16, 255]), "000f10ff");
        assert_eq!(hex_decode::<4>("000f10ff"), Some([0, 15, 16, 255]));
        for bad in [
            "000F10FF",
            "000f10f",
            "000f10ff00",
            "+00f10ff",
            "000f10f ",
            "000f10fg",
        ] {
            assert_eq!(hex_decode::<4>(bad), None, "{bad}");
        }
        assert_eq!(key_id(&public(MAIN)).len(), 16);
        assert_ne!(key_id(&public(MAIN)), key_id(&public(RECOVERY)));
    }
}
