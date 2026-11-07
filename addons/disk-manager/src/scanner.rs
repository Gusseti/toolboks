use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
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
pub fn scan(
    root: PathBuf,
    exclude: Vec<String>,
    shared: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
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
    let mut stack = vec![(root, 0)];
    let mut extensions: HashMap<String, Extension> = HashMap::new();
    let exclusions: Vec<String> = exclude.iter().map(|s| s.to_lowercase()).collect();
    let mut last_update = Instant::now();
    while let Some((path, parent)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            p.cancelled = true;
            break;
        }
        p.folders += 1;
        p.current = path.to_string_lossy().into();
        let entries = match fs::read_dir(&path) {
            Ok(v) => v,
            Err(e) => {
                record_error(&mut p, format!("{}: {}", path.display(), e));
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                p.cancelled = true;
                break;
            }
            let entry = match entry {
                Ok(v) => v,
                Err(e) => {
                    record_error(&mut p, e.to_string());
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => {
                    record_error(&mut p, format!("{}: {}", path.display(), e));
                    continue;
                }
            };
            if linked(&meta) || (meta.is_dir() && exclusions.contains(&name.to_lowercase())) {
                p.skipped += 1;
                continue;
            }
            if !meta.is_file() && !meta.is_dir() {
                p.skipped += 1;
                continue;
            }
            let dir = meta.is_dir();
            let bytes = if dir { 0 } else { meta.len() };
            let id = nodes.len();
            nodes.push(Node {
                id,
                parent: Some(parent),
                name,
                path: path.to_string_lossy().into(),
                dir,
                bytes,
                files: if dir { 0 } else { 1 },
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |t| t.as_secs()),
            });
            if dir {
                stack.push((path, id));
            } else {
                p.files += 1;
                p.bytes = p.bytes.saturating_add(bytes);
                let ext = path
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                    .unwrap_or("No extension".into());
                let e = extensions.entry(ext.clone()).or_insert_with(|| Extension {
                    name: ext,
                    ..Default::default()
                });
                e.bytes = e.bytes.saturating_add(bytes);
                e.files += 1;
            }
            if last_update.elapsed().as_millis() > 150 {
                p.elapsed = started.elapsed().as_secs_f64();
                shared.lock().unwrap().progress = p.clone();
                last_update = Instant::now();
            }
        }
    }
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
    p.extensions = extensions.into_values().collect();
    p.extensions.sort_by_key(|a| std::cmp::Reverse(a.bytes));
    let mut state = shared.lock().unwrap();
    state.nodes = Arc::new(nodes);
    state.progress = p;
}
fn record_error(p: &mut Progress, e: String) {
    p.skipped += 1;
    if p.errors.len() < 100 {
        p.errors.push(e);
    }
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
    #[test]
    fn aggregates_excludes_and_queries() {
        let root = std::env::temp_dir().join(format!("stratum-test-{}", std::process::id()));
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
    #[cfg(windows)]
    #[test]
    fn reparse_points_are_not_followed() {
        use std::os::windows::fs::symlink_dir;
        let root = std::env::temp_dir().join(format!("stratum-link-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        if symlink_dir(&root, root.join("loop")).is_ok() {
            let state = Arc::new(Mutex::new(State::default()));
            scan(
                root.clone(),
                vec![],
                state.clone(),
                Arc::new(AtomicBool::new(false)),
            );
            assert_eq!(state.lock().unwrap().progress.skipped, 1);
            fs::remove_dir(root.join("loop")).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
}
