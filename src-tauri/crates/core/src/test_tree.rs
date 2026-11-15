//! A folder of sample files for one test, removed when the test ends. The
//! readers of sysfs, procfs and the key files take the folder they read as
//! an argument, so a test builds the files it needs here.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct Tree {
    root: PathBuf,
}

impl Tree {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "deskmate-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Writes `content` to the file `relative`, creating the folders above it.
    pub fn file(&self, relative: &str, content: &str) -> &Self {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        self
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
