//! A scratch folder for tests that removes itself, also when the test fails or panics.
//!
//! Tests used to build a folder in `%TEMP%` by hand and delete it as the last line, which a
//! failing test never reached; and the process id in the name meant the next run never reused it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// How long `Drop` keeps retrying: a child exe can still be shutting down and hold its file.
const REMOVE_PATIENCE: Duration = Duration::from_secs(3);

/// A unique folder under the temp folder, deleted with everything in it when dropped.
pub(crate) struct TestDir(PathBuf);

impl TestDir {
    /// A fresh, empty folder named after `label`.
    pub(crate) fn new(label: &str) -> Self {
        let dir = Self::unused(label);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The path of a folder that does not exist yet (for code that creates it itself). It is
    /// still removed on drop.
    pub(crate) fn unused(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("toolboks-{label}-{}-{n}", std::process::id())))
    }
}

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let give_up = Instant::now() + REMOVE_PATIENCE;
        loop {
            match std::fs::remove_dir_all(&self.0) {
                Ok(()) => return,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                // Errors are ignored in the end: a cleanup problem must not fail the test.
                Err(_) if Instant::now() >= give_up => return,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
