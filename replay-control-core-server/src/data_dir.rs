//! Per-host directory for ROM-storage-keyed databases.
//!
//! On Pi production this defaults to `/var/lib/replay-control`. The root
//! contains a `storages/` subdir with one folder per storage id, each
//! holding that storage's `library.db` (plus its WAL/SHM sidecars).
//! User data (`user_data.db`) stays on the ROM storage and is not
//! tracked here.

use std::path::{Path, PathBuf};

use crate::storage_id::StorageId;
use replay_control_core::error::{Error, Result};

/// Default data root on a real ReplayOS install. Override with
/// `--data-dir` on the CLI when running in dev or pointing at NVMe.
pub const DEFAULT_DATA_DIR: &str = "/var/lib/replay-control";

const STORAGES_SUBDIR: &str = "storages";

/// Mirrors `library::db::LIBRARY_DB_FILE`. Re-stated here so non-`library`-
/// feature callers (path resolution only) don't need to enable the feature.
pub const LIBRARY_DB_FILE: &str = "library.db";

/// Cheap to clone.
#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Create a `DataDir` rooted at `root`. Does not create the directory —
    /// see [`Self::ensure_storage_dir`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> Self {
        Self::new(DEFAULT_DATA_DIR)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn storage_dir(&self, id: &StorageId) -> PathBuf {
        self.root.join(STORAGES_SUBDIR).join(id.as_str())
    }

    pub fn library_db_path(&self, id: &StorageId) -> PathBuf {
        self.storage_dir(id).join(LIBRARY_DB_FILE)
    }

    pub fn ensure_storage_dir(&self, id: &StorageId) -> Result<PathBuf> {
        let dir = self.storage_dir(id);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> StorageId {
        StorageId::parse(s).expect("valid id")
    }

    #[test]
    fn paths_are_namespaced_by_id() {
        let d = DataDir::new("/tmp/x");
        assert_eq!(
            d.library_db_path(&id("usb-1a2b3c4d")),
            PathBuf::from("/tmp/x/storages/usb-1a2b3c4d/library.db")
        );
    }

    #[test]
    fn ensure_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDir::new(tmp.path());
        let dir = d.ensure_storage_dir(&id("usb-1a2b3c4d")).unwrap();
        assert!(dir.exists());
        d.ensure_storage_dir(&id("usb-1a2b3c4d")).unwrap();
    }
}
