//! The add-on catalog: the `index.json` format, entry checks and download checks.
//!
//! The catalog is a plain list that anyone can publish. It is not signed. What is still checked
//! is the shape of the list and, for every download, the size and SHA-256 the entry declares.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use toolboks_addon_sdk::{valid_data_note, valid_id, version};

/// The only index format this build understands. Additive fields do not bump it (unknown fields are
/// ignored); a change in meaning does, and older hosts then refuse the whole index.
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;
/// Sanity limit for the `size` an entry may declare.
pub const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

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

/// Why an index was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    UnknownFormat(u32),
    Malformed(String),
    /// The index was good but could not be saved (set by the host).
    NotPersisted(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFormat(v) => write!(f, "catalog format {v} is not supported"),
            Self::Malformed(why) => write!(f, "the catalog is malformed: {why}"),
            Self::NotPersisted(why) => write!(f, "the catalog could not be saved: {why}"),
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
    /// The maker's credit shown in the details, such as "by stratum", when it differs from the
    /// publisher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byline: Option<String>,
    pub category: String,
    pub icon: String,
    pub accent: String,
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// Capability names as plain text, so a name this host does not know yet does not make the
    /// whole index unreadable. The installer decides what to do with unknown ones.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The add-on's pages by name, in order. Links (`--open "Title/Page"`) resolve against
    /// them, also while the add-on is not running. Empty for an add-on with one page.
    #[serde(default)]
    pub pages: Vec<String>,
    /// The add-on keeps working while hidden: the host starts it at launch while it is on.
    #[serde(default)]
    pub background: bool,
    /// One plain-language line about data the add-on keeps on this PC, copied from its manifest
    /// and shown in the install review and the details. Same rule as the manifest field
    /// (`toolboks_addon_sdk::valid_data_note`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_note: Option<String>,
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
            self.byline
                .as_deref()
                .is_none_or(|byline| text(byline, 1, 60, false)),
            "byline",
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
        let unique_pages = self.pages.iter().enumerate().all(|(i, page)| {
            !self.pages[..i]
                .iter()
                .any(|earlier| earlier.eq_ignore_ascii_case(page))
        });
        ok(
            self.pages.len() <= 16
                && unique_pages
                && self
                    .pages
                    .iter()
                    .all(|page| text(page, 1, 40, false) && !page.contains('/')),
            "pages",
        )?;
        ok(
            self.data_note.as_deref().is_none_or(valid_data_note),
            "data_note",
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

/// The parsed `index.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub format_version: u32,
    pub entries: Vec<Entry>,
}

impl Index {
    /// Parses and structurally validates index bytes.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, version: &str) -> Entry {
        Entry {
            id: id.into(),
            title: "Sample".into(),
            short_description: "A sample add-on".into(),
            long_description: String::new(),
            publisher: "Toolboks".into(),
            byline: None,
            category: "system".into(),
            icon: "box".into(),
            accent: "#4f8cff".into(),
            screenshots: vec![],
            capabilities: vec!["read_system_info".into()],
            pages: vec![],
            background: false,
            data_note: None,
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

    fn index() -> Index {
        Index {
            format_version: FORMAT_VERSION,
            entries: vec![entry("toolboks.sample", "1.0.0")],
        }
    }

    fn bytes(index: &Index) -> Vec<u8> {
        serde_json::to_vec_pretty(index).unwrap()
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
        let index = index();
        assert_eq!(Index::parse(&bytes(&index)), Ok(index));
    }

    #[test]
    fn old_fields_from_signed_indexes_are_ignored() {
        // Earlier indexes carried a sequence, dates and a publisher key id. They still read.
        let mut value = serde_json::to_value(index()).unwrap();
        value["sequence"] = 9.into();
        value["generated_at"] = 1.into();
        value["expires_at"] = 2.into();
        value["entries"][0]["publisher_key_id"] = "0123456789abcdef".into();
        let parsed = Index::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(parsed, index());
    }

    #[test]
    fn refuses_an_unknown_format_version() {
        let mut future = index();
        future.format_version = 2;
        assert_eq!(
            Index::parse(&bytes(&future)),
            Err(Refused::UnknownFormat(2))
        );
        // A future format with a different shape is still reported as unknown, not as broken.
        let other = br#"{"format_version": 3, "packages": {}}"#;
        assert_eq!(Index::parse(other), Err(Refused::UnknownFormat(3)));
    }

    #[test]
    fn refuses_a_structurally_bad_index() {
        let refused = |change: fn(&mut Index)| {
            let mut index = index();
            change(&mut index);
            let result = Index::parse(&bytes(&index));
            assert!(matches!(result, Err(Refused::Malformed(_))), "{result:?}");
        };
        refused(|i| i.entries[0].id = "Not An Id".into());
        refused(|i| i.entries[0].version = "1.0".into());
        refused(|i| i.entries[0].min_toolboks_version = "newest".into());
        refused(|i| i.entries[0].download_url = "http://example.com/a".into());
        refused(|i| i.entries[0].download_url = "https://user@example.com/a".into());
        refused(|i| i.entries[0].sha256 = "abc".into());
        refused(|i| i.entries[0].size = 0);
        refused(|i| i.entries[0].title = String::new());
        refused(|i| i.entries[0].capabilities = vec!["Run Anything".into()]);
        refused(|i| i.entries.push(i.entries[0].clone()));
        // A missing `verified` flag is an error, never a silent default.
        let mut value = serde_json::to_value(index()).unwrap();
        value["entries"][0]
            .as_object_mut()
            .unwrap()
            .remove("verified");
        let result = Index::parse(&serde_json::to_vec(&value).unwrap());
        assert!(matches!(result, Err(Refused::Malformed(_))), "{result:?}");
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
    fn pages_are_unique_plain_names() {
        let mut entry = entry("toolboks.sample", "1.0.0");
        entry.pages = vec!["Map".into(), "Largest files".into()];
        entry.background = true;
        assert_eq!(entry.validate(), Ok(()));
        // Old indexes have neither field.
        let mut json = serde_json::to_value(&entry).unwrap();
        json.as_object_mut().unwrap().remove("pages");
        json.as_object_mut().unwrap().remove("background");
        let old: Entry = serde_json::from_value(json).unwrap();
        assert!(old.pages.is_empty() && !old.background);
        for bad in [
            vec!["Map", "map"],
            vec!["A/B"],
            vec![""],
            vec!["line\nbreak"],
        ] {
            entry.pages = bad.into_iter().map(String::from).collect();
            assert_eq!(entry.validate(), Err("pages"));
        }
        entry.pages = (0..17).map(|i| format!("Page {i}")).collect();
        assert_eq!(entry.validate(), Err("pages"));
    }

    #[test]
    fn data_note_is_optional_one_short_line() {
        let mut entry = entry("toolboks.sample", "1.0.0");
        // Old indexes have no data note, and none is written when it is absent.
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json.get("data_note").is_none());
        let old: Entry = serde_json::from_value(json).unwrap();
        assert_eq!(old.data_note, None);
        let mut with_note = index();
        with_note.entries[0].data_note =
            Some("Keeps your last scan of each drive on this PC (file and folder names).".into());
        assert_eq!(Index::parse(&bytes(&with_note)), Ok(with_note));
        // The limit counts characters, not bytes.
        for good in ["a".repeat(160), "\u{e5}".repeat(160)] {
            entry.data_note = Some(good);
            assert_eq!(entry.validate(), Ok(()));
        }
        for bad in [
            "a".repeat(161),
            String::new(),
            "  ".into(),
            " Keeps scans.".into(),
            "Keeps scans.\n".into(),
            "Keeps\nscans.".into(),
            "Keeps\u{1b}[31mscans.".into(),
            // Format characters (Cf) and line/paragraph separators: the same rule as the SDK.
            "Keeps \u{202e}snacs.".into(),
            "Keeps\u{2066}scans\u{2069}.".into(),
            "Keeps\u{200b}scans.".into(),
            "Keeps\u{200d}scans.".into(),
            "\u{feff}Keeps scans.".into(),
            "Keeps scans.\u{e0041}".into(),
            "Keeps\u{ad}scans.".into(),
            "Keeps\u{2028}scans.".into(),
            "Keeps\u{2029}scans.".into(),
        ] {
            entry.data_note = Some(bad);
            assert_eq!(entry.validate(), Err("data_note"));
        }
        // A bad note refuses the index, like any other bad field.
        for note in ["x".repeat(161), "Keeps \u{202e}snacs.".into()] {
            let mut bad = index();
            bad.entries[0].data_note = Some(note);
            assert!(matches!(
                Index::parse(&bytes(&bad)),
                Err(Refused::Malformed(_))
            ));
        }
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
        let mut index = index();
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
    }
}
