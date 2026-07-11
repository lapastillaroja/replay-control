use std::collections::HashSet;
use std::fs::{DirEntry, Metadata};
use std::path::{Path, PathBuf};

use replay_control_core::error::{Error, Result};

/// Recursively walk `root`, following symlinks but descending each real
/// directory at most once (cycle-safe), invoking `on_file` for every non-dir
/// entry with its symlink-resolved metadata. Directories whose name starts with
/// `_` are skipped when `skip_underscore_dirs` is set.
///
/// A dangling symlink (`NotFound`) is skipped; any other filesystem error is
/// returned so callers can preserve stored state instead of treating a partial
/// walk as authoritative.
pub(crate) fn for_each_file(
    root: &Path,
    skip_underscore_dirs: bool,
    mut on_file: impl FnMut(&DirEntry, &Path, &Metadata),
) -> Result<()> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    if let Ok(canon) = std::fs::canonicalize(root) {
        visited.insert(canon);
    }
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let path = entry.path();
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::io(&path, e)),
            };
            if metadata.is_dir() {
                if skip_underscore_dirs && entry.file_name().to_string_lossy().starts_with('_') {
                    continue;
                }
                if let Ok(canon) = std::fs::canonicalize(&path)
                    && visited.insert(canon)
                {
                    pending.push(path);
                }
            } else {
                on_file(&entry, &path, &metadata);
            }
        }
    }

    Ok(())
}
