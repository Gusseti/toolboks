use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize)]
pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub bytes: u64,
    pub files: u64,
    pub modified: u64,
}
#[derive(Clone, Default, Serialize)]
pub struct Extension {
    pub name: String,
    pub bytes: u64,
    pub files: u64,
}
#[derive(Clone, Default, Serialize)]
pub struct Progress {
    pub running: bool,
    pub cancelled: bool,
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub skipped: u64,
    pub current: String,
    pub elapsed: f64,
    pub errors: Vec<String>,
    pub extensions: Vec<Extension>,
    pub root: String,
    pub generation: u64,
}
#[derive(Default)]
pub struct State {
    pub progress: Progress,
    pub nodes: Arc<Vec<Node>>,
}
fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

const MAX_ERRORS: usize = 100;
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
const POLL: Duration = Duration::from_millis(50);

/// One kept directory entry, fully prepared by a worker thread.
struct Entry {
    name: String,
    path: String,
    /// Set for directories: the path to queue for reading.
    dir_path: Option<PathBuf>,
    bytes: u64,
    modified: u64,
    /// Lowercased extension key for files (empty for directories).
    ext: String,
}
/// The result of reading one directory.
struct Batch {
    parent: usize,
    path: PathBuf,
    entries: Vec<Entry>,
    /// Entries skipped on purpose (links, exclusions, special files).
    skipped: u64,
    /// Entries or the directory itself that could not be read.
    error_count: u64,
    errors: Vec<String>,
}
struct Job {
    path: PathBuf,
    parent: usize,
}

/// Disk kind of the volume holding a scan root, used to pick the thread count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Fast,
    Hdd,
    Network,
}
fn thread_count(root: &Path) -> usize {
    match volume_kind(root) {
        Kind::Hdd => 2,
        Kind::Network => 4,
        Kind::Fast => {
            let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
            (cores * 2).clamp(8, 16)
        }
    }
}
#[cfg(windows)]
fn volume_kind(root: &Path) -> Kind {
    use std::path::{Component, Prefix};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::CloseHandle,
            Storage::FileSystem::{
                CreateFileW, GetDriveTypeW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ,
                FILE_SHARE_WRITE, OPEN_EXISTING,
            },
            System::{Ioctl::*, IO::DeviceIoControl},
        },
    };
    let letter = match root.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(b) | Prefix::VerbatimDisk(b) => b as char,
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return Kind::Network,
            _ => return Kind::Fast,
        },
        _ => return Kind::Fast,
    };
    let wide = |s: String| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let drive = wide(format!(r"{letter}:\"));
    // DRIVE_REMOTE
    if unsafe { GetDriveTypeW(PCWSTR(drive.as_ptr())) } == 4 {
        return Kind::Network;
    }
    let path = wide(format!(r"\\.\{letter}:"));
    let Ok(handle) = (unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }) else {
        return Kind::Fast;
    };
    let q = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceSeekPenaltyProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut bytes = [0u8; 64];
    let mut returned = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some((&q as *const STORAGE_PROPERTY_QUERY).cast()),
            std::mem::size_of_val(&q) as u32,
            Some(bytes.as_mut_ptr().cast()),
            bytes.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .is_ok();
    let _ = unsafe { CloseHandle(handle) };
    // Byte 8 is the "incurs seek penalty" flag of DEVICE_SEEK_PENALTY_DESCRIPTOR.
    if ok && returned >= 9 && bytes[8] != 0 {
        Kind::Hdd
    } else {
        Kind::Fast
    }
}
#[cfg(not(windows))]
fn volume_kind(_: &Path) -> Kind {
    Kind::Fast
}

pub fn scan(
    root: PathBuf,
    exclude: Vec<String>,
    shared: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
) {
    let threads = thread_count(&root);
    scan_with(root, exclude, shared, cancel, threads);
}

fn read_dir_job(job: Job, exclusions: &[String], cancel: &AtomicBool) -> Batch {
    let mut b = Batch {
        parent: job.parent,
        path: job.path,
        entries: Vec::new(),
        skipped: 0,
        error_count: 0,
        errors: Vec::new(),
    };
    let fail = |b: &mut Batch, e: String| {
        b.error_count += 1;
        if b.errors.len() < MAX_ERRORS {
            b.errors.push(e);
        }
    };
    let rd = match fs::read_dir(&b.path) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("{}: {}", b.path.display(), e);
            fail(&mut b, msg);
            return b;
        }
    };
    for entry in rd {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let entry = match entry {
            Ok(v) => v,
            Err(e) => {
                fail(&mut b, e.to_string());
                continue;
            }
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                let msg = format!("{}: {}", entry.path().display(), e);
                fail(&mut b, msg);
                continue;
            }
        };
        let is_dir = meta.is_dir();
        if linked(&meta) || !(is_dir || meta.is_file()) {
            b.skipped += 1;
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_dir && !exclusions.is_empty() {
            let lower = name.to_lowercase();
            if exclusions.contains(&lower) {
                b.skipped += 1;
                continue;
            }
        }
        let path = entry.path();
        let ext = if is_dir {
            String::new()
        } else {
            path.extension()
                .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                .unwrap_or_else(|| "No extension".into())
        };
        b.entries.push(Entry {
            name,
            path: path.to_string_lossy().into_owned(),
            bytes: if is_dir { 0 } else { meta.len() },
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_secs()),
            ext,
            dir_path: if is_dir { Some(path) } else { None },
        });
    }
    b
}

fn scan_with(
    root: PathBuf,
    exclude: Vec<String>,
    shared: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    threads: usize,
) {
    let started = Instant::now();
    let generation = shared.lock().unwrap().progress.generation;
    let mut p = Progress {
        running: true,
        root: root.to_string_lossy().into(),
        generation,
        ..Default::default()
    };
    let mut nodes = vec![Node {
        id: 0,
        parent: None,
        name: root
            .file_name()
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .into(),
        path: p.root.clone(),
        dir: true,
        bytes: 0,
        files: 0,
        modified: 0,
    }];
    nodes.reserve(1024);
    let mut extensions: HashMap<String, (u64, u64)> = HashMap::new();
    let exclusions: Vec<String> = exclude.iter().map(|s| s.to_lowercase()).collect();
    let mut last_update = Instant::now();
    let mut last_dir = PathBuf::new();
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let job_rx = Mutex::new(job_rx);
    let (res_tx, res_rx) = mpsc::channel::<Batch>();
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            let res_tx = res_tx.clone();
            let (job_rx, exclusions, cancel) = (&job_rx, &exclusions, &*cancel);
            scope.spawn(move || loop {
                // Holding the lock while blocked is fine: the others wait for the next job.
                let job = match job_rx.lock().unwrap().recv() {
                    Ok(j) if !cancel.load(Ordering::Relaxed) => j,
                    _ => break,
                };
                let batch = read_dir_job(job, exclusions, cancel);
                if res_tx.send(batch).is_err() {
                    break;
                }
            });
        }
        drop(res_tx);
        let mut outstanding = 0usize;
        if !cancel.load(Ordering::Relaxed) {
            let _ = job_tx.send(Job {
                path: root.clone(),
                parent: 0,
            });
            outstanding = 1;
        }
        loop {
            if cancel.load(Ordering::Relaxed) {
                p.cancelled = true;
                break;
            }
            if outstanding == 0 {
                break;
            }
            match res_rx.recv_timeout(POLL) {
                Ok(b) => {
                    outstanding -= 1;
                    p.folders += 1;
                    p.skipped += b.skipped + b.error_count;
                    for e in b.errors {
                        if p.errors.len() < MAX_ERRORS {
                            p.errors.push(e);
                        }
                    }
                    nodes.reserve(b.entries.len());
                    for e in b.entries {
                        let id = nodes.len();
                        let dir = e.dir_path.is_some();
                        if let Some(path) = e.dir_path {
                            let _ = job_tx.send(Job { path, parent: id });
                            outstanding += 1;
                        } else {
                            p.files += 1;
                            p.bytes = p.bytes.saturating_add(e.bytes);
                            let x = extensions.entry(e.ext).or_default();
                            x.0 = x.0.saturating_add(e.bytes);
                            x.1 += 1;
                        }
                        nodes.push(Node {
                            id,
                            parent: Some(b.parent),
                            name: e.name,
                            path: e.path,
                            dir,
                            bytes: e.bytes,
                            files: if dir { 0 } else { 1 },
                            modified: e.modified,
                        });
                    }
                    last_dir = b.path;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if last_update.elapsed() >= PROGRESS_EVERY {
                p.elapsed = started.elapsed().as_secs_f64();
                p.current = last_dir.to_string_lossy().into_owned();
                shared.lock().unwrap().progress = p.clone();
                last_update = Instant::now();
            }
        }
        // Closing the job queue makes idle workers exit; busy ones stop at the cancel check.
        drop(job_tx);
    });
    for id in (1..nodes.len()).rev() {
        // The parent was added before this node, so its totals can be updated in place.
        let (parents, children) = nodes.split_at_mut(id);
        let child = &children[0];
        if let Some(parent) = child.parent.and_then(|p| parents.get_mut(p)) {
            parent.bytes = parent.bytes.saturating_add(child.bytes);
            parent.files = parent.files.saturating_add(child.files);
        }
    }
    p.running = false;
    p.elapsed = started.elapsed().as_secs_f64();
    p.current = last_dir.to_string_lossy().into_owned();
    p.extensions = extensions
        .into_iter()
        .map(|(name, (bytes, files))| Extension { name, bytes, files })
        .collect();
    p.extensions.sort_by_key(|a| std::cmp::Reverse(a.bytes));
    let mut state = shared.lock().unwrap();
    state.nodes = Arc::new(nodes);
    state.progress = p;
}

#[derive(Deserialize, Serialize, Default)]
pub struct Query {
    pub parent: Option<usize>,
    pub mode: Option<String>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub min_bytes: Option<u64>,
    pub older_days: Option<u64>,
}
#[derive(Clone, Serialize)]
pub struct Listing {
    pub rows: Vec<Node>,
    pub total: usize,
    pub parent: Option<Node>,
    pub breadcrumbs: Vec<Node>,
}
pub fn listing(nodes: &[Node], q: &Query) -> Listing {
    let parent_id = q.parent.unwrap_or(0);
    let mode = q.mode.as_deref().unwrap_or("folders");
    let search = q.search.as_deref().unwrap_or("").to_lowercase();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut rows: Vec<&Node> = nodes
        .iter()
        .filter(|n| {
            (if mode == "files" {
                !n.dir
            } else if !search.is_empty() {
                n.id != 0
            } else {
                n.parent == Some(parent_id)
            }) && (search.is_empty() || n.path.to_lowercase().contains(&search))
                && n.bytes >= q.min_bytes.unwrap_or(0)
                && q.older_days
                    .unwrap_or(0)
                    .checked_mul(86400)
                    .is_some_and(|age| {
                        age == 0
                            || (!n.dir && n.modified > 0 && now.saturating_sub(n.modified) >= age)
                    })
        })
        .collect();
    match q.sort.as_deref().unwrap_or("size") {
        "name" => rows.sort_by_cached_key(|n| n.name.to_lowercase()),
        "files" => rows.sort_by_key(|a| std::cmp::Reverse(a.files)),
        "modified" => rows.sort_by_key(|a| std::cmp::Reverse(a.modified)),
        _ => rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.id.cmp(&b.id))),
    }
    let total = rows.len();
    let rows = rows
        .into_iter()
        .skip(q.offset.unwrap_or(0))
        .take(q.limit.unwrap_or(100).min(500))
        .cloned()
        .collect();
    let mut breadcrumbs = vec![];
    let mut next = nodes.get(parent_id);
    while let Some(n) = next {
        breadcrumbs.push(n.clone());
        next = n.parent.and_then(|p| nodes.get(p));
    }
    breadcrumbs.reverse();
    Listing {
        rows,
        total,
        parent: nodes.get(parent_id).cloned(),
        breadcrumbs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static SEQ: AtomicUsize = AtomicUsize::new(0);
    fn temp(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("stratum-{tag}-{}-{n}", std::process::id()))
    }
    fn run(root: &Path, exclude: &[&str], threads: usize) -> Arc<Mutex<State>> {
        let state = Arc::new(Mutex::new(State::default()));
        scan_with(
            root.to_path_buf(),
            exclude.iter().map(|s| s.to_string()).collect(),
            state.clone(),
            Arc::new(AtomicBool::new(false)),
            threads,
        );
        state
    }
    /// The old single-threaded algorithm, reduced to totals.
    #[derive(Default, PartialEq, Debug)]
    struct Reference {
        files: u64,
        folders: u64,
        bytes: u64,
        skipped: u64,
        ext: Vec<(String, u64, u64)>,
    }
    fn reference(root: &Path, exclude: &[&str]) -> Reference {
        let mut r = Reference::default();
        let mut ext: HashMap<String, (u64, u64)> = HashMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            r.folders += 1;
            let Ok(rd) = fs::read_dir(&dir) else {
                r.skipped += 1;
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().to_lowercase();
                let Ok(m) = fs::symlink_metadata(&path) else {
                    r.skipped += 1;
                    continue;
                };
                if linked(&m) || (m.is_dir() && exclude.contains(&name.as_str())) {
                    r.skipped += 1;
                } else if m.is_dir() {
                    stack.push(path);
                } else if m.is_file() {
                    r.files += 1;
                    r.bytes += m.len();
                    let k = path
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                        .unwrap_or("No extension".into());
                    let x = ext.entry(k).or_default();
                    x.0 += m.len();
                    x.1 += 1;
                } else {
                    r.skipped += 1;
                }
            }
        }
        r.ext = ext.into_iter().map(|(k, (b, f))| (k, b, f)).collect();
        r.ext.sort();
        r
    }
    fn totals(s: &State) -> Reference {
        let p = &s.progress;
        let mut ext: Vec<_> = p
            .extensions
            .iter()
            .map(|e| (e.name.clone(), e.bytes, e.files))
            .collect();
        ext.sort();
        Reference {
            files: p.files,
            folders: p.folders,
            bytes: p.bytes,
            skipped: p.skipped,
            ext,
        }
    }
    fn generate(root: &Path) {
        for a in 0..6 {
            for b in 0..4 {
                let d = root.join(format!("d{a}/s{b}"));
                fs::create_dir_all(&d).unwrap();
                for f in 0..9 {
                    let ext = ["txt", "BIN", "rs", ""][f % 4];
                    let name = if ext.is_empty() {
                        format!("f{f}")
                    } else {
                        format!("f{f}.{ext}")
                    };
                    fs::write(d.join(name), vec![0u8; a * 100 + b * 10 + f]).unwrap();
                }
            }
            fs::create_dir_all(root.join(format!("d{a}/Omit/deep"))).unwrap();
            fs::write(root.join(format!("d{a}/Omit/deep/x")), [0; 99]).unwrap();
            fs::create_dir_all(root.join(format!("d{a}/empty"))).unwrap();
        }
    }
    #[test]
    fn aggregates_excludes_and_queries() {
        let root = temp("agg");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::create_dir_all(root.join("omit")).unwrap();
        fs::write(root.join("one.bin"), [0; 17]).unwrap();
        fs::write(root.join("nested/two.bin"), [0; 33]).unwrap();
        fs::write(root.join("omit/ignored"), [0; 100]).unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        scan(
            root.clone(),
            vec!["omit".into()],
            state.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let s = state.lock().unwrap();
        assert_eq!(s.progress.bytes, 50);
        assert_eq!(s.progress.files, 2);
        assert_eq!(s.nodes[0].bytes, 50);
        assert_eq!(s.nodes[0].files, 2);
        assert_eq!(s.progress.extensions[0].bytes, 50);
        let l = listing(
            &s.nodes,
            &Query {
                mode: Some("files".into()),
                min_bytes: Some(20),
                ..Default::default()
            },
        );
        assert_eq!(l.total, 1);
        assert_eq!(l.rows[0].bytes, 33);
        drop(s);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn matches_reference_for_any_thread_count() {
        let root = temp("ref");
        generate(&root);
        let want = reference(&root, &["omit"]);
        assert!(want.files > 100 && want.skipped == 6);
        for threads in [1, 2, 8] {
            let state = run(&root, &["Omit"], threads);
            let s = state.lock().unwrap();
            assert_eq!(totals(&s), want, "threads={threads}");
            assert_eq!(s.nodes[0].bytes, want.bytes);
            assert_eq!(s.nodes[0].files, want.files);
            assert_eq!(s.nodes.len() as u64, 1 + want.files + want.folders - 1);
            assert!(!s.progress.running && !s.progress.cancelled);
            for n in s.nodes.iter().skip(1) {
                assert!(n.parent.unwrap() < n.id, "parent must precede child");
                assert!(s.nodes[n.parent.unwrap()].dir);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn thread_count_is_bounded() {
        let n = thread_count(&std::env::temp_dir());
        assert!((2..=16).contains(&n));
    }
    #[test]
    fn missing_root_is_one_error() {
        let root = temp("missing");
        let state = run(&root, &[], 4);
        let s = state.lock().unwrap();
        assert_eq!(s.progress.skipped, 1);
        assert_eq!(s.progress.errors.len(), 1);
        assert_eq!(s.progress.folders, 1);
    }
    #[test]
    fn cancellation_returns_partial_result() {
        let state = Arc::new(Mutex::new(State::default()));
        scan(
            std::env::temp_dir(),
            vec![],
            state.clone(),
            Arc::new(AtomicBool::new(true)),
        );
        let s = state.lock().unwrap();
        assert!(s.progress.cancelled);
        assert!(!s.progress.running);
        assert_eq!(s.progress.files, 0);
    }
    #[test]
    fn cancellation_mid_scan_stops_promptly() {
        let root = temp("cancel");
        generate(&root);
        let state = Arc::new(Mutex::new(State::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let canceller = std::thread::spawn(move || flag.store(true, Ordering::Relaxed));
        scan_with(root.clone(), vec![], state.clone(), cancel, 4);
        canceller.join().unwrap();
        let s = state.lock().unwrap();
        assert!(!s.progress.running);
        assert_eq!(s.nodes[0].files, s.progress.files);
        drop(s);
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn reparse_points_are_not_followed() {
        use std::os::windows::fs::symlink_dir;
        let root = temp("link");
        fs::create_dir_all(&root).unwrap();
        if symlink_dir(&root, root.join("loop")).is_ok() {
            let state = run(&root, &[], 4);
            assert_eq!(state.lock().unwrap().progress.skipped, 1);
            fs::remove_dir(root.join("loop")).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
    #[test]
    #[ignore]
    fn bench_scan_env_path() {
        let Ok(path) = std::env::var("TOOLBOKS_SCAN_BENCH") else {
            println!("set TOOLBOKS_SCAN_BENCH=<path>");
            return;
        };
        let root = PathBuf::from(path);
        let state = Arc::new(Mutex::new(State::default()));
        let t = Instant::now();
        scan(
            root.clone(),
            vec![],
            state.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let s = state.lock().unwrap();
        println!(
            "scan {} threads={} files={} folders={} bytes={} skipped={} in {:.3}s",
            root.display(),
            thread_count(&root),
            s.progress.files,
            s.progress.folders,
            s.progress.bytes,
            s.progress.skipped,
            t.elapsed().as_secs_f64()
        );
    }
}
