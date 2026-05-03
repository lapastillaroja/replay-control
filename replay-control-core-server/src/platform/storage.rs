use std::path::{Path, PathBuf};

use crate::config::SystemConfig;
use replay_control_core::error::{Error, Result};

/// Represents the resolved storage location where ROMs, saves, and config live.
#[derive(Debug, Clone)]
pub struct StorageLocation {
    /// Root path of the storage (e.g., `/media/sd` or `/media/usb`)
    pub root: PathBuf,
    /// Storage type
    pub kind: StorageKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageKind {
    Sd,
    Usb,
    Nvme,
    Nfs,
}

impl StorageKind {
    /// Returns `true` for local filesystems where inotify works reliably.
    /// NFS is excluded because inotify does not detect changes made by
    /// other NFS clients (only local VFS operations generate events).
    pub fn is_local(self) -> bool {
        matches!(self, Self::Sd | Self::Usb | Self::Nvme)
    }

    /// Lowercase string tag (`"sd"`, `"usb"`, `"nvme"`, `"nfs"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sd => "sd",
            Self::Usb => "usb",
            Self::Nvme => "nvme",
            Self::Nfs => "nfs",
        }
    }
}

/// Check whether a path is on a filesystem that supports POSIX locking
/// and shared memory (required for SQLite WAL mode).
///
/// Returns `true` for ext4, btrfs, xfs, etc.
/// Returns `false` for exfat, vfat/fat32, nfs, and unknown filesystems.
///
/// WAL mode requires the `-shm` (shared memory) file to be mmap'd by
/// multiple connections. Filesystems like exFAT don't support this
/// reliably, causing SQLITE_IOERR_SHORT_READ (522).
pub fn supports_wal(path: &std::path::Path) -> bool {
    // Read /proc/mounts to find the filesystem type for this path.
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false; // Can't determine — assume unsafe
    };

    // Find the longest mount point that's a prefix of our path.
    let path_str = path.to_string_lossy();
    let mut best_match: Option<(&str, &str)> = None; // (mount_point, fs_type)

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let mount_point = parts[1];
        let fs_type = parts[2];

        if path_str.starts_with(mount_point)
            && (best_match.is_none() || mount_point.len() > best_match.unwrap().0.len())
        {
            best_match = Some((mount_point, fs_type));
        }
    }

    match best_match {
        Some((_, fs_type)) => matches!(
            fs_type,
            "ext2" | "ext3" | "ext4" | "btrfs" | "xfs" | "f2fs" | "tmpfs"
        ),
        None => false,
    }
}

/// Directory name for Replay Control data on ROM storage.
pub const RC_DIR: &str = ".replay-control";
/// Filename for app-specific user settings.
pub const SETTINGS_FILE: &str = "settings.cfg";
/// Filename for the per-storage stable id marker (under `RC_DIR`).
/// Contains a single line: a `<kind>-<8 hex>` id like `usb-1a2b3c4d`.
/// Used to namespace the central `library.db` so ROM-storage swaps preserve
/// library state across reboots and mount-path churn.
pub const STORAGE_ID_FILE: &str = "storage-id";

/// Well-known paths relative to the storage root.
const ROMS_DIR: &str = "roms";
const SAVES_DIR: &str = "saves";
const CONFIG_DIR: &str = "config";
const BIOS_DIR: &str = "bios";
const CAPTURES_DIR: &str = "captures";
const MANUALS_DIR: &str = "manuals";

impl StorageLocation {
    /// Detect the active storage location based on the RePlayOS config.
    pub fn detect(config: &SystemConfig) -> Result<Self> {
        let (root, kind) = match config.storage_mode() {
            "usb" => {
                let path = find_usb_storage()?;
                (path, StorageKind::Usb)
            }
            "nvme" => {
                let path = PathBuf::from("/media/nvme");
                if !path.exists() {
                    return Err(Error::StorageNotFound);
                }
                (path, StorageKind::Nvme)
            }
            "nfs" => {
                let path = PathBuf::from("/media/nfs");
                if !path.exists() {
                    return Err(Error::StorageNotFound);
                }
                (path, StorageKind::Nfs)
            }
            _ => {
                // Default: SD card
                let path = PathBuf::from("/media/sd");
                if !path.exists() {
                    return Err(Error::StorageNotFound);
                }
                (path, StorageKind::Sd)
            }
        };

        Ok(Self { root, kind })
    }

    /// Create a StorageLocation pointing at an arbitrary path.
    /// Useful for testing or when the user provides a custom path.
    pub fn from_path(root: PathBuf, kind: StorageKind) -> Self {
        Self { root, kind }
    }

    /// The `.replay-control/` data directory for the companion app.
    pub fn rc_dir(&self) -> PathBuf {
        self.root.join(RC_DIR)
    }

    pub fn roms_dir(&self) -> PathBuf {
        self.root.join(ROMS_DIR)
    }

    pub fn saves_dir(&self) -> PathBuf {
        self.root.join(SAVES_DIR)
    }

    pub fn config_dir(&self) -> PathBuf {
        self.root.join(CONFIG_DIR)
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join(CONFIG_DIR).join("replay.cfg")
    }

    pub fn bios_dir(&self) -> PathBuf {
        self.root.join(BIOS_DIR)
    }

    pub fn captures_dir(&self) -> PathBuf {
        self.root.join(CAPTURES_DIR)
    }

    pub fn manuals_dir(&self) -> PathBuf {
        self.root.join(MANUALS_DIR)
    }

    pub fn favorites_dir(&self) -> PathBuf {
        self.roms_dir().join("_favorites")
    }

    pub fn recents_dir(&self) -> PathBuf {
        self.roms_dir().join("_recent")
    }

    pub fn system_roms_dir(&self, system_folder: &str) -> PathBuf {
        self.roms_dir().join(system_folder)
    }

    /// Returns the total and available disk space for this storage.
    pub async fn disk_usage(&self) -> Result<DiskUsage> {
        disk_usage_for(&self.root).await
    }

    /// Read or create the persistent storage id marker for this storage.
    pub fn ensure_storage_id(&self) -> Result<crate::storage_id::StorageId> {
        ensure_storage_id_at(&self.rc_dir(), self.kind.as_str())
    }

    /// True when `<root>` is actually the active mount point (not just a
    /// rootfs stub the kernel hasn't finished mounting on top of yet).
    /// Production callers should gate `ensure_storage_id` and friends on
    /// this — when it returns false, treat as no-storage and let the
    /// background re-detection loop pick the mount up later. Dev / test
    /// callers (`--storage-path`) skip the check entirely.
    pub fn is_ready(&self) -> bool {
        is_mount_point(&self.root)
    }
}

/// Read or create the storage-id marker inside `rc_dir`. Public for callers
/// that have the `.replay-control` directory path but not a `StorageLocation`
/// (tests, raw storage_root code paths). Production code routes through
/// [`StorageLocation::ensure_storage_id`] only after gating on
/// [`StorageLocation::is_ready`] to close the rootfs-stub race.
///
/// Resolution order:
/// 1. Existing marker (cache hit).
/// 2. Derive a deterministic id from the filesystem UUID (or
///    `server:/share` for NFS). Same storage → same id forever.
/// 3. If the FS id can't be obtained (tmpfs, tempdir, exotic mount), random
///    fallback.
pub fn ensure_storage_id_at(rc_dir: &Path, kind: &str) -> Result<crate::storage_id::StorageId> {
    use crate::storage_id::StorageId;

    let marker = rc_dir.join(STORAGE_ID_FILE);
    let storage_root = rc_dir.parent().unwrap_or(rc_dir);

    if let Some(id) = read_marker(&marker)? {
        return Ok(id);
    }

    let id = match filesystem_id_for(storage_root) {
        Some(fs_id) => {
            let derived = StorageId::from_filesystem_id(kind, &fs_id);
            tracing::info!("Deriving storage id from filesystem id ({fs_id}) -> {derived}");
            derived
        }
        None => {
            tracing::warn!(
                "Could not determine filesystem id for {}; using random id (will rotate on remount)",
                storage_root.display()
            );
            StorageId::generate(kind)
        }
    };
    write_marker(rc_dir, &marker, &id)?;
    Ok(id)
}

/// Read the marker. `Ok(Some(id))` when valid; `Ok(None)` when missing or
/// malformed (caller falls through to derivation). Any other IO error
/// (permission denied, IO error) propagates — silently treating those as
/// "missing" was the bug that let mount races silently rotate the id.
fn read_marker(marker: &Path) -> Result<Option<crate::storage_id::StorageId>> {
    use crate::storage_id::StorageId;
    match std::fs::read_to_string(marker) {
        Ok(buf) => match StorageId::parse(buf.trim()) {
            Ok(id) => Ok(Some(id)),
            Err(e) => {
                tracing::warn!(
                    "Storage id marker at {} is invalid ({:?}: {e}); regenerating",
                    marker.display(),
                    buf.trim()
                );
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(marker, e)),
    }
}

fn write_marker(rc_dir: &Path, marker: &Path, id: &crate::storage_id::StorageId) -> Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(rc_dir).map_err(|e| Error::io(rc_dir, e))?;
    // tmp + sync + rename keeps the marker crash-safe (no torn write on power
    // loss). Two replay-control processes racing here is last-writer-wins
    // since `rename` overwrites — fine for our single-service deployment, and
    // both writers compute the same id from FS UUID so the race is benign.
    let tmp = rc_dir.join(format!("{STORAGE_ID_FILE}.tmp.{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| Error::io(&tmp, e))?;
        f.write_all(id.as_str().as_bytes())
            .and_then(|_| f.write_all(b"\n"))
            .and_then(|_| f.sync_all())
            .map_err(|e| Error::io(&tmp, e))?;
    }
    std::fs::rename(&tmp, marker).map_err(|e| Error::io(marker, e))?;
    tracing::info!("Wrote storage id {id} ({})", marker.display());
    Ok(())
}

/// Resolved mount entry for a path: the source (block device or NFS export)
/// and filesystem type.
struct MountEntry {
    mount_point: String,
    source: String,
    fs_type: String,
}

/// Find the longest-prefix mount entry whose mount point covers `path`. We
/// prefer `/proc/self/mountinfo` over `/proc/mounts` because the former is
/// namespace-correct (handles bind mounts and chroots). Returns `None` if
/// `/proc/self/mountinfo` is unreadable (non-Linux test environments).
fn mount_entry_for(path: &Path) -> Option<MountEntry> {
    let content = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let path_str = path.to_str()?;
    let mut best: Option<(usize, MountEntry)> = None;
    for line in content.lines() {
        // mountinfo line: id parent dev path mount_point options - fs_type source super_options
        // The " - " separator splits the optional-fields head from the tail.
        let (head, tail) = line.split_once(" - ")?;
        let head_parts: Vec<&str> = head.split(' ').collect();
        let tail_parts: Vec<&str> = tail.split(' ').collect();
        if head_parts.len() < 5 || tail_parts.len() < 2 {
            continue;
        }
        let mount_point = head_parts[4];
        let fs_type = tail_parts[0];
        let source = tail_parts[1];
        if path_str == mount_point
            || path_str.starts_with(&format!("{mount_point}/"))
            || mount_point == "/"
        {
            let len = mount_point.len();
            if best.as_ref().map(|(l, _)| len > *l).unwrap_or(true) {
                best = Some((
                    len,
                    MountEntry {
                        mount_point: mount_point.to_string(),
                        source: source.to_string(),
                        fs_type: fs_type.to_string(),
                    },
                ));
            }
        }
    }
    best.map(|(_, m)| m)
}

/// True if `path` is itself a distinct mount point (its longest-prefix mount
/// entry's mount point equals `path`). Returns `false` when `path` is just a
/// regular directory inside another filesystem — the rootfs-stub case.
fn is_mount_point(path: &Path) -> bool {
    mount_entry_for(path)
        .map(|m| Path::new(&m.mount_point) == path)
        .unwrap_or(false)
}

/// Resolve a stable filesystem identifier for `path`. Block-backed filesystems
/// return the FS UUID (32-bit volume serial on exFAT/FAT32, 128-bit UUID on
/// ext4/btrfs/xfs). NFS returns `server:/share`, which is stable across
/// mounts of the same export. Returns `None` when no stable id is available
/// (tmpfs, overlay, etc.) — caller falls back to random.
fn filesystem_id_for(path: &Path) -> Option<String> {
    let entry = mount_entry_for(path)?;
    if entry.fs_type.starts_with("nfs") {
        return Some(entry.source);
    }
    blkid_uuid(&entry.source).or(Some(entry.source))
}

/// Look up a block device's UUID by walking `/dev/disk/by-uuid/` symlinks.
/// Cleaner than shelling out to `blkid` — no setuid concerns, pure Rust.
fn blkid_uuid(device: &str) -> Option<String> {
    let target = std::fs::canonicalize(device).ok()?;
    let entries = std::fs::read_dir("/dev/disk/by-uuid").ok()?;
    for entry in entries.flatten() {
        if let Ok(canonical) = std::fs::canonicalize(entry.path())
            && canonical == target
            && let Some(name) = entry.file_name().to_str()
        {
            return Some(name.to_string());
        }
    }
    None
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
}

async fn disk_usage_for(path: &Path) -> Result<DiskUsage> {
    // Use statvfs via nix or fall back to parsing df output
    let output = tokio::process::Command::new("df")
        .arg("-B1")
        .arg(path)
        .output()
        .await
        .map_err(|e| Error::io(path, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1).ok_or(Error::StorageNotFound)?;

    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return Err(Error::StorageNotFound);
    }

    let total_bytes: u64 = fields[1].parse().unwrap_or(0);
    let used_bytes: u64 = fields[2].parse().unwrap_or(0);
    let available_bytes: u64 = fields[3].parse().unwrap_or(0);

    Ok(DiskUsage {
        total_bytes,
        available_bytes,
        used_bytes,
    })
}

fn find_usb_storage() -> Result<PathBuf> {
    // RePlayOS mounts USB storage at /media/usb
    let path = PathBuf::from("/media/usb");
    if path.exists() {
        return Ok(path);
    }

    // Fallback: scan /media for any mounted USB
    let media = Path::new("/media");
    if media.exists()
        && let Ok(entries) = std::fs::read_dir(media)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p != Path::new("/media/sd") && p.join(ROMS_DIR).exists() {
                return Ok(p);
            }
        }
    }

    Err(Error::StorageNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_local_returns_true_for_local_storage() {
        assert!(StorageKind::Sd.is_local());
        assert!(StorageKind::Usb.is_local());
        assert!(StorageKind::Nvme.is_local());
    }

    #[test]
    fn is_local_returns_false_for_nfs() {
        assert!(!StorageKind::Nfs.is_local());
    }

    #[test]
    fn ensure_storage_id_creates_marker_on_first_call() {
        let tmp = tempfile::tempdir().unwrap();
        let rc_dir = tmp.path().join(RC_DIR);
        let id = ensure_storage_id_at(&rc_dir, "usb").expect("first call");
        let marker = rc_dir.join(STORAGE_ID_FILE);
        assert!(marker.exists());
        let written = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(written.trim(), id.as_str());
    }

    #[test]
    fn ensure_storage_id_is_stable_across_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let rc_dir = tmp.path().join(RC_DIR);
        let a = ensure_storage_id_at(&rc_dir, "usb").unwrap();
        let b = ensure_storage_id_at(&rc_dir, "usb").unwrap();
        let c = ensure_storage_id_at(&rc_dir, "usb").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn ensure_storage_id_rewrites_invalid_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let rc_dir = tmp.path().join(RC_DIR);
        std::fs::create_dir_all(&rc_dir).unwrap();
        std::fs::write(rc_dir.join(STORAGE_ID_FILE), "garbage-shape\n").unwrap();
        let id = ensure_storage_id_at(&rc_dir, "usb").unwrap();
        let written = std::fs::read_to_string(rc_dir.join(STORAGE_ID_FILE)).unwrap();
        assert_eq!(written.trim(), id.as_str());
    }
}
