//! Custom add-on icons: an add-on whose catalog entry says `"icon": "custom"` ships a small PNG
//! (`icon_url`, `icon_sha256`, `icon_size`). The host downloads it with the same checks as the
//! add-on itself and keeps it in `<data>/icons/<id>-<sha256>.png`, outside the add-on's version
//! folders (those may hold only the exe or package and `entry.json`).
//!
//! Only the PNG's alpha channel is used: it is the shape, drawn in the colour the named icons use
//! in the same place. Anything wrong with the file (size, format, sides, hash) means the default
//! glyph is drawn and one line is logged; it never stops an install.

use crate::addons::write_atomic;
use crate::catalog::Entry;
use crate::installer::Fetch;
use crate::update::is_trusted_url;
use crate::winhttp::{Download, HttpError};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use toolboks_catalog::{
    check_bytes, check_icon_header, DownloadMismatch, IconProblem, MAX_ICON_SIDE,
};

/// The prefix of an icon key that names a custom icon file (see [`key`]).
pub const KEY_PREFIX: &str = "custom:";
const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(30);
const STALE_PART: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconError {
    /// The entry has no usable custom icon fields.
    NotCustom,
    Untrusted,
    Download(String),
    Mismatch(DownloadMismatch),
    Bad(IconProblem),
    /// The header was fine but the picture could not be decoded.
    Decode(String),
    Io(String),
}

impl std::fmt::Display for IconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCustom => f.write_str("the entry has no custom icon"),
            Self::Untrusted => f.write_str("the icon address is not trusted"),
            Self::Download(e) => write!(f, "the icon download failed: {e}"),
            Self::Mismatch(m) => write!(f, "the icon does not match the catalog: {m:?}"),
            Self::Bad(problem) => problem.fmt(f),
            Self::Decode(e) => write!(f, "the icon could not be read: {e}"),
            Self::Io(e) => f.write_str(e),
        }
    }
}

/// The folder that holds every cached custom icon.
pub fn dir(root: &Path) -> PathBuf {
    root.join("icons")
}

fn file_name(id: &str, sha256: &str) -> String {
    format!("{id}-{sha256}.png")
}

/// Where the custom icon of `entry` is kept, when it has one.
pub fn path(root: &Path, entry: &Entry) -> Option<PathBuf> {
    let (_, sha256, _) = entry.custom_icon()?;
    toolboks_addon_sdk::valid_id(&entry.id).then(|| dir(root).join(file_name(&entry.id, sha256)))
}

/// What the drawing code is given for `entry`: `custom:<id>-<sha256>` for a custom icon (the name
/// of its cached file), otherwise the icon name itself. A custom icon whose fields are missing
/// stays `custom`, which is drawn as the default glyph.
pub fn key(entry: &Entry) -> String {
    match entry.custom_icon() {
        Some((_, sha256, _)) if toolboks_addon_sdk::valid_id(&entry.id) => {
            format!(
                "{KEY_PREFIX}{}",
                file_name(&entry.id, sha256).trim_end_matches(".png")
            )
        }
        _ => entry.icon.clone(),
    }
}

/// The cached file a key from [`key`] names, under `root`. `None` for a named icon or a key that
/// is not a plain file name.
pub fn key_path(root: &Path, key: &str) -> Option<PathBuf> {
    let stem = key.strip_prefix(KEY_PREFIX)?;
    let plain = !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && !stem.starts_with('.');
    plain.then(|| dir(root).join(format!("{stem}.png")))
}

/// Checks a custom icon file and returns its side in pixels and its alpha channel, one byte per
/// pixel, row by row. The size, signature and header are checked before anything is decoded, so
/// a file that announces a huge picture is never inflated.
pub fn alpha_mask(bytes: &[u8]) -> Result<(u32, Vec<u8>), IconError> {
    let side = check_icon_header(bytes).map_err(IconError::Bad)?;
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_ICON_SIDE);
    limits.max_image_height = Some(MAX_ICON_SIDE);
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let picture = reader
        .decode()
        .map_err(|e| IconError::Decode(e.to_string()))?;
    if picture.width() != side || picture.height() != side {
        return Err(IconError::Decode("the size differs from the header".into()));
    }
    let alpha = picture.into_rgba8().pixels().map(|p| p.0[3]).collect();
    Ok((side, alpha))
}

/// Downloads the custom icon of `entry` into the cache, unless a good copy is already there.
/// Nothing is placed unless the address is trusted, the bytes match `icon_size` and
/// `icon_sha256`, and the PNG passes [`alpha_mask`].
pub fn download(root: &Path, entry: &Entry, fetch: &dyn Fetch) -> Result<PathBuf, IconError> {
    let (url, sha256, size) = entry.custom_icon().ok_or(IconError::NotCustom)?;
    let target = path(root, entry).ok_or(IconError::NotCustom)?;
    if let Ok(bytes) = fs::read(&target) {
        if check_bytes(&bytes, size, sha256).is_ok() && alpha_mask(&bytes).is_ok() {
            return Ok(target);
        }
        let _ = fs::remove_file(&target);
    }
    if !is_trusted_url(url) {
        return Err(IconError::Untrusted);
    }
    let folder = dir(root);
    fs::create_dir_all(&folder).map_err(|e| IconError::Io(e.to_string()))?;
    // Its own name, so the install and the add-on list never write into the same file.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let part = folder.join(format!(
        ".{}.{}-{}.part",
        file_name(&entry.id, sha256),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let cancel = AtomicBool::new(false);
    let saved = fetch.fetch(
        url,
        &part,
        Download {
            resume_from: 0,
            max_bytes: size,
            deadline: DOWNLOAD_DEADLINE,
            cancel: &cancel,
            progress: &mut |_| {},
        },
    );
    let placed = (|| {
        match saved {
            Ok(saved) if !is_trusted_url(&saved.final_url) => return Err(IconError::Untrusted),
            Ok(_) => {}
            Err(HttpError::Status(code)) => {
                return Err(IconError::Download(format!("server answered {code}")))
            }
            Err(HttpError::Cancelled) => return Err(IconError::Download("cancelled".into())),
            Err(HttpError::Other(e)) => return Err(IconError::Download(e)),
        }
        let bytes = fs::read(&part).map_err(|e| IconError::Io(e.to_string()))?;
        check_bytes(&bytes, size, sha256).map_err(IconError::Mismatch)?;
        alpha_mask(&bytes)?;
        write_atomic(&target, &bytes).map_err(IconError::Io)?;
        Ok(target)
    })();
    let _ = fs::remove_file(&part);
    placed
}

/// [`download`] for an install or the add-on list: a failure is one log line and the default
/// glyph, never an error for the caller.
pub fn download_or_log(root: &Path, entry: &Entry, fetch: &dyn Fetch) {
    if entry.custom_icon().is_none() {
        return;
    }
    if let Err(e) = download(root, entry, fetch) {
        eprintln!("Toolboks: icon of {} not used: {e}", entry.id);
    }
}

/// Deletes the cached icons of `id` except the files named in `keep`.
fn remove_except(root: &Path, id: &str, keep: &BTreeSet<String>) {
    let Ok(files) = fs::read_dir(dir(root)) else {
        return;
    };
    let prefix = format!("{id}-");
    for file in files.flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        let ours = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".png"))
            .is_some_and(|sha| toolboks_catalog::hex_decode::<32>(sha).is_some());
        if ours && !keep.contains(&name) {
            let _ = fs::remove_file(file.path());
        }
    }
}

/// Deletes every cached icon of `id` (the add-on was removed).
pub fn remove(root: &Path, id: &str) {
    if toolboks_addon_sdk::valid_id(id) {
        remove_except(root, id, &BTreeSet::new());
    }
}

/// Deletes the cached icons of `id` that none of `entries` uses (the add-on was replaced).
pub fn keep_only(root: &Path, id: &str, entries: &[Entry]) {
    if !toolboks_addon_sdk::valid_id(id) {
        return;
    }
    let keep = entries
        .iter()
        .filter(|entry| entry.id == id)
        .filter_map(|entry| path(root, entry))
        .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
        .collect();
    remove_except(root, id, &keep);
}

/// Deletes every cached icon that none of `entries` uses: the add-on list and the installed
/// versions. Called after the list is read, so icons of add-ons that left the list or changed
/// their picture do not pile up. A download left by a crash goes once it is an hour old (a
/// running one is far younger: it has 30 seconds).
pub fn prune(root: &Path, entries: &[&Entry]) {
    let keep: BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| path(root, entry))
        .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
        .collect();
    let Ok(files) = fs::read_dir(dir(root)) else {
        return;
    };
    for file in files.flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        let stale_part = name.starts_with('.')
            && name.ends_with(".part")
            && file
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age >= STALE_PART);
        if (name.ends_with(".png") && !keep.contains(&name)) || stale_part {
            let _ = fs::remove_file(file.path());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_dir::TestDir;
    use crate::winhttp::Saved;
    use std::sync::Mutex;

    /// A `size` x `size` PNG: a transparent square with an opaque disc in red.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut picture = image::RgbaImage::new(width, height);
        let centre = (width as f32 / 2.0, height as f32 / 2.0);
        for (x, y, pixel) in picture.enumerate_pixels_mut() {
            let (dx, dy) = (x as f32 + 0.5 - centre.0, y as f32 + 0.5 - centre.1);
            let inside = dx * dx + dy * dy <= (width.min(height) as f32 / 3.0).powi(2);
            *pixel = image::Rgba([220, 30, 30, if inside { 255 } else { 0 }]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        picture.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// Hands out `body` as the download of any URL, and records the URLs asked for.
    pub(crate) struct Serve {
        pub body: Vec<u8>,
        pub final_url: Option<String>,
        pub asked: Mutex<Vec<String>>,
    }

    impl Serve {
        pub(crate) fn new(body: Vec<u8>) -> Self {
            Self {
                body,
                final_url: None,
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl Fetch for Serve {
        fn fetch(&self, url: &str, file: &Path, options: Download) -> Result<Saved, HttpError> {
            self.asked.lock().unwrap().push(url.into());
            if self.body.len() as u64 > options.max_bytes {
                return Err(HttpError::Other("larger than allowed".into()));
            }
            fs::write(file, &self.body).map_err(|e| HttpError::Other(e.to_string()))?;
            Ok(Saved {
                final_url: self.final_url.clone().unwrap_or_else(|| url.into()),
                resumed: false,
                len: self.body.len() as u64,
            })
        }
    }

    pub(crate) fn with_icon(entry: &mut Entry, bytes: &[u8]) {
        entry.icon = toolboks_catalog::CUSTOM_ICON.into();
        entry.icon_url = Some(format!(
            "https://github.com/test/{}/icon.png",
            entry.version
        ));
        entry.icon_sha256 = Some(toolboks_catalog::sha256_hex(bytes));
        entry.icon_size = Some(bytes.len() as u64);
    }

    fn entry(bytes: &[u8]) -> Entry {
        let mut entry: Entry = serde_json::from_value(serde_json::json!({
            "id": "com.example.counter",
            "title": "Counter",
            "short_description": "Counts",
            "publisher": "Example",
            "category": "developer",
            "icon": "grid",
            "accent": "#8B5CF6",
            "kind": "native",
            "version": "1.0.0",
            "min_toolboks_version": "0.12.5",
            "protocol_version": 3,
            "download_url": "https://github.com/test/addon.exe",
            "size": 1,
            "sha256": toolboks_catalog::sha256_hex(b"x"),
            "verified": false,
            "channel": "stable"
        }))
        .unwrap();
        with_icon(&mut entry, bytes);
        entry
    }

    #[test]
    fn accepts_a_good_64_pixel_icon_and_reads_its_alpha() {
        let bytes = png(64, 64);
        let (side, alpha) = alpha_mask(&bytes).unwrap();
        assert_eq!(side, 64);
        assert_eq!(alpha.len(), 64 * 64);
        // The corner is transparent, the centre is the shape.
        assert_eq!(alpha[0], 0);
        assert_eq!(alpha[32 * 64 + 32], 255);
        for side in [16, 128] {
            assert_eq!(alpha_mask(&png(side, side)).unwrap().0, side);
        }
    }

    #[test]
    fn refuses_bad_icon_files() {
        let bad = |bytes: &[u8]| alpha_mask(bytes).unwrap_err();
        assert_eq!(
            bad(&png(64, 48)),
            IconError::Bad(IconProblem::NotSquare {
                width: 64,
                height: 48
            })
        );
        assert_eq!(
            bad(&png(256, 256)),
            IconError::Bad(IconProblem::BadSide(256))
        );
        assert_eq!(bad(&png(8, 8)), IconError::Bad(IconProblem::BadSide(8)));
        assert_eq!(
            bad(b"not a png at all"),
            IconError::Bad(IconProblem::NotPng)
        );
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(32, 32)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(bad(jpeg.get_ref()), IconError::Bad(IconProblem::NotPng));
        // Too big a file, whatever it holds.
        let mut big = png(64, 64);
        big.resize(toolboks_catalog::MAX_ICON_BYTES as usize + 1, 0);
        assert_eq!(bad(&big), IconError::Bad(IconProblem::TooLarge));
        // Cut short: in the header, and after it (the picture data is missing).
        let good = png(64, 64);
        assert_eq!(bad(&good[..20]), IconError::Bad(IconProblem::Truncated));
        assert!(matches!(bad(&good[..40]), IconError::Decode(_)));
        assert!(matches!(bad(&good[..good.len() / 2]), IconError::Decode(_)));
    }

    #[test]
    fn downloads_a_matching_icon_once_and_refuses_a_mismatch() {
        let root = TestDir::new("icon-download");
        let bytes = png(64, 64);
        let entry = entry(&bytes);
        let server = Serve::new(bytes.clone());
        let placed = download(&root, &entry, &server).unwrap();
        assert_eq!(placed, path(&root, &entry).unwrap());
        assert_eq!(fs::read(&placed).unwrap(), bytes);
        assert_eq!(key_path(&root, &key(&entry)), Some(placed.clone()));
        // A good copy is not fetched again.
        download(&root, &entry, &server).unwrap();
        assert_eq!(server.asked.lock().unwrap().len(), 1);
        // Other bytes than the catalog says: nothing is placed.
        let other_root = TestDir::new("icon-mismatch");
        let mut tampered = png(64, 64);
        tampered[40] ^= 1;
        assert!(matches!(
            download(&other_root, &entry, &Serve::new(tampered)),
            Err(IconError::Mismatch(DownloadMismatch::Sha256))
        ));
        assert!(!path(&other_root, &entry).unwrap().exists());
        assert_eq!(fs::read_dir(dir(&other_root)).unwrap().count(), 0);
        // A file that matches its hash but is not a usable icon is not placed either.
        let wide = png(64, 32);
        let wide_entry = super::tests::entry(&wide);
        assert!(matches!(
            download(&other_root, &wide_entry, &Serve::new(wide)),
            Err(IconError::Bad(IconProblem::NotSquare { .. }))
        ));
        assert_eq!(fs::read_dir(dir(&other_root)).unwrap().count(), 0);
    }

    #[test]
    fn refuses_untrusted_addresses() {
        let root = TestDir::new("icon-untrusted");
        let bytes = png(32, 32);
        let mut entry = entry(&bytes);
        entry.icon_url = Some("https://example.com/icon.png".into());
        let server = Serve::new(bytes.clone());
        assert_eq!(download(&root, &entry, &server), Err(IconError::Untrusted));
        assert!(server.asked.lock().unwrap().is_empty());
        // A redirect away from GitHub.
        let entry = super::tests::entry(&bytes);
        let mut moved = Serve::new(bytes);
        moved.final_url = Some("https://example.com/icon.png".into());
        assert_eq!(download(&root, &entry, &moved), Err(IconError::Untrusted));
        assert!(!path(&root, &entry).unwrap().exists());
    }

    #[test]
    fn keys_name_the_cached_file_and_nothing_else() {
        let root = Path::new("C:/data");
        let bytes = png(32, 32);
        let entry = entry(&bytes);
        let sha = toolboks_catalog::sha256_hex(&bytes);
        assert_eq!(key(&entry), format!("custom:com.example.counter-{sha}"));
        assert_eq!(
            key_path(root, &key(&entry)),
            Some(
                root.join("icons")
                    .join(format!("com.example.counter-{sha}.png"))
            )
        );
        let mut named = entry.clone();
        named.icon = "chip".into();
        assert_eq!(key(&named), "chip");
        let mut broken = entry.clone();
        broken.icon_sha256 = None;
        assert_eq!(key(&broken), "custom");
        for bad in [
            "chip",
            "custom",
            "custom:",
            "custom:../x",
            "custom:a\\b",
            "custom:A",
            "custom:.x",
        ] {
            assert_eq!(key_path(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn removal_and_replacement_delete_cached_icons() {
        let root = TestDir::new("icon-remove");
        let old_bytes = png(32, 32);
        let new_bytes = png(48, 48);
        let old = entry(&old_bytes);
        let mut new = entry(&new_bytes);
        new.version = "1.1.0".into();
        with_icon(&mut new, &new_bytes);
        let mut other = entry(&old_bytes);
        other.id = "com.example.other".into();
        for (entry, bytes) in [(&old, &old_bytes), (&new, &new_bytes), (&other, &old_bytes)] {
            download(&root, entry, &Serve::new(bytes.clone())).unwrap();
        }
        // Replaced: only the icon the remaining versions use stays.
        keep_only(&root, &old.id, std::slice::from_ref(&new));
        assert!(!path(&root, &old).unwrap().exists());
        assert!(path(&root, &new).unwrap().exists());
        assert!(path(&root, &other).unwrap().exists());
        // Removed: all of its icons go, other add-ons keep theirs.
        remove(&root, &new.id);
        assert!(!path(&root, &new).unwrap().exists());
        assert!(path(&root, &other).unwrap().exists());
        // The list no longer has it: pruned.
        // A download in progress stays; one left by a crash an hour ago goes.
        let running = dir(&root).join(".a.png.1-1.part");
        let left = dir(&root).join(".b.png.1-2.part");
        fs::write(&running, b"in progress").unwrap();
        fs::write(&left, b"left over").unwrap();
        fs::File::options()
            .write(true)
            .open(&left)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - STALE_PART - Duration::from_secs(5))
            .unwrap();
        prune(&root, &[]);
        assert!(running.exists() && !left.exists());
        fs::remove_file(&running).unwrap();
        assert_eq!(fs::read_dir(dir(&root)).unwrap().count(), 0);
    }
}
