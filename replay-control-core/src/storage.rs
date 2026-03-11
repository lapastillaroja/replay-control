use std::path::{Path, PathBuf};

use crate::config::ReplayConfig;
use crate::error::{Error, Result};

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
    Nfs,
}

/// Directory name for Replay Control data on ROM storage.
pub const RC_DIR: &str = ".replay-control";
/// Filename for the user-saved video links JSON.
pub const VIDEOS_FILE: &str = "videos.json";
/// Filename for app-specific user settings.
pub const SETTINGS_FILE: &str = "settings.cfg";

/// Well-known paths relative to the storage root.
const ROMS_DIR: &str = "roms";
const SAVES_DIR: &str = "saves";
const CONFIG_DIR: &str = "config";
const BIOS_DIR: &str = "bios";
const CAPTURES_DIR: &str = "captures";

impl StorageLocation {
    /// Detect the active storage location based on the RePlayOS config.
    pub fn detect(config: &ReplayConfig) -> Result<Self> {
        let (root, kind) = match config.storage_mode() {
            "usb" => {
                let path = find_usb_storage()?;
                (path, StorageKind::Usb)
            }
            "nfs" => {
                // NFS is mounted at a fixed path by RePlayOS
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
    pub fn disk_usage(&self) -> Result<DiskUsage> {
        disk_usage_for(&self.root)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiskUsage {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
}

fn disk_usage_for(path: &Path) -> Result<DiskUsage> {
    // Use statvfs via nix or fall back to parsing df output
    let output = std::process::Command::new("df")
        .arg("-B1")
        .arg(path)
        .output()
        .map_err(|e| Error::io(path, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .nth(1)
        .ok_or_else(|| Error::StorageNotFound)?;

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
    if media.exists() {
        if let Ok(entries) = std::fs::read_dir(media) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p != Path::new("/media/sd") && p.join(ROMS_DIR).exists() {
                    return Ok(p);
                }
            }
        }
    }

    Err(Error::StorageNotFound)
}
