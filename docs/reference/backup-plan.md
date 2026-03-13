# Backup Feature -- Design Plan

> **Status**: Planning. Not yet implemented.

Date: 2026-03-13

## Overview

Manual backup of RePlayOS user data to the NFS server that is already configured for RePlayOS. The user triggers a backup from the companion app's NFS settings page. Automatic/scheduled backups are a future consideration.

**Key constraint:** The backup destination is the same NFS share configured in `replay.cfg` (`nfs_server` + `nfs_share`). The user must have a working NFS setup. The backup writes to a dedicated subdirectory on the NFS share, separate from any ROM data.

---

## 1. What to Back Up

### Tier 1: Critical (irreplaceable user data)

These files contain user-generated data that cannot be reconstructed from any external source. Loss of these files means loss of gameplay progress or deliberate user choices.

| Item | Path on Pi | Typical Size | Rationale |
|------|-----------|-------------|-----------|
| Save RAM (SRAM / battery saves) | `<storage>/saves/` (flat or per-system) | 1-500 KB each | In-game save data (battery saves). Irreplaceable gameplay progress. |
| Save states | `<storage>/saves/` (`.state`, `.state1`, etc.) | 100 KB - 50 MB each | Emulator snapshots. Hours of gameplay progress. Can be large for complex systems (N64, PSX). |
| `user_data.db` | `<storage>/.replay-control/user_data.db` | < 1 MB | User's deliberate box art overrides. Small but irreplaceable. |
| `videos.json` | `<storage>/.replay-control/videos.json` | < 1 MB | User-curated video links per game. |
| `settings.cfg` | `<storage>/.replay-control/settings.cfg` | < 1 KB | App-specific user settings (region preference, etc.). |
| `replay.cfg` | `/media/sd/config/replay.cfg` | < 4 KB | RePlayOS system configuration. Always on SD card. Core system settings (video mode, NFS config, Wi-Fi, skin, etc.). |
| Favorites | `<storage>/roms/_favorites/` | Symlinks only | Symlinks to favorited ROMs. Tiny but represent user curation. |
| Captures (screenshots) | `<storage>/captures/` | 7-19 KB each | User-taken screenshots. Small files, sentimental value. |

### Tier 2: Important (user customizations, reconstructable with effort)

These files represent user effort but could be recreated manually if lost.

| Item | Path on Pi | Typical Size | Rationale |
|------|-----------|-------------|-----------|
| Input remaps / core configs | `<storage>/config/` | Small files | Per-game and per-system controller remaps and core option overrides. RePlayOS stores input remaps and core configs here. Recreating per-game button mappings is tedious. |
| Recently played | `<storage>/roms/_recent/` | Symlinks | Managed by RePlayOS. Nice to have for continuity. |

### Tier 3: Rebuildable (cache data, skip by default)

These can be fully regenerated from external sources. Backing them up wastes space and time.

| Item | Path on Pi | Typical Size | Skip Reason |
|------|-----------|-------------|-------------|
| `metadata.db` | `<storage>/.replay-control/metadata.db` | 5-20 MB | Rebuilt from LaunchBox XML + libretro-thumbnails. |
| `launchbox-metadata.xml` | `<storage>/.replay-control/launchbox-metadata.xml` | ~460 MB | Downloaded from LaunchBox. |
| `media/` (box art + snaps) | `<storage>/.replay-control/media/` | 200 MB - 2 GB | Downloaded from libretro-thumbnails GitHub repos. |
| `tmp/` | `<storage>/.replay-control/tmp/` | Variable | Temporary git clones. Deleted after use. |
| BIOS files | `<storage>/bios/` | Variable | User-provided. Not ours to manage and potentially legally sensitive. The user should have their own copies. |
| ROM files | `<storage>/roms/` | Potentially huge | Not a backup target. ROMs are the user's own files. If using NFS, ROMs are already on the NFS server. |

### Summary: default backup set

```
<storage>/saves/                      # Tier 1: save RAM + save states
<storage>/.replay-control/user_data.db  # Tier 1: user customizations DB
<storage>/.replay-control/videos.json   # Tier 1: curated video links
<storage>/.replay-control/settings.cfg  # Tier 1: app settings
/media/sd/config/replay.cfg            # Tier 1: system config (always on SD)
<storage>/roms/_favorites/             # Tier 1: favorite symlinks (targets only)
<storage>/captures/                    # Tier 1: user screenshots
<storage>/config/                      # Tier 2: input remaps + core configs
<storage>/roms/_recent/                # Tier 2: recently played (symlinks)
```

---

## 2. Backup Destination and Structure

### NFS mount point

RePlayOS mounts the NFS share at `/media/nfs`. When `storage_mode=nfs`, this IS the storage root -- so the user's ROM data and saves are already on NFS. In that case, backup makes less sense (data is already on the NFS server).

The primary use case for backup is when `storage_mode=usb` (or `sd`) and the user has also configured NFS in `replay.cfg` as a backup target. However, `storage_mode` controls where RePlayOS looks for ROMs -- if it's set to `usb`, the NFS share is NOT mounted automatically by RePlayOS.

**Approach: mount NFS temporarily for backup.**

The backup server function will:
1. Read `nfs_server` and `nfs_share` from `replay.cfg`
2. Mount the NFS share to a temporary mount point (e.g., `/tmp/replay-backup-nfs`)
3. Perform the backup
4. Unmount

This avoids conflicting with RePlayOS's NFS mount at `/media/nfs` and works regardless of `storage_mode`.

### Destination directory layout

```
<nfs_share>/
└── .replay-backups/
    └── <hostname>_<YYYYMMDD>_<HHMMSS>/
        ├── manifest.json              # Backup metadata
        ├── replay.cfg                 # System config
        ├── saves/                     # Full mirror of saves/ directory
        │   ├── *.srm                  # Battery saves
        │   ├── *.state, *.state1...   # Save states
        │   └── ...
        ├── config/                    # Full mirror of config/ directory
        │   └── ...                    # Input remaps, core options
        ├── captures/                  # Full mirror of captures/ directory
        │   ├── sega_smd/
        │   └── ...
        ├── replay-control/            # .replay-control data (minus caches)
        │   ├── user_data.db
        │   ├── videos.json
        │   └── settings.cfg
        └── favorites.txt              # Newline-separated list of favorite symlink targets
```

### `manifest.json`

```json
{
  "version": 1,
  "hostname": "replaypi",
  "timestamp": "2026-03-13T14:30:00Z",
  "storage_mode": "usb",
  "storage_root": "/media/usb",
  "items": {
    "saves": { "files": 42, "bytes": 15728640 },
    "config": { "files": 8, "bytes": 4096 },
    "captures": { "files": 12, "bytes": 180224 },
    "replay_control": { "files": 3, "bytes": 32768 },
    "replay_cfg": { "files": 1, "bytes": 2048 },
    "favorites": { "files": 15, "bytes": 0 }
  },
  "total_files": 81,
  "total_bytes": 15947776,
  "duration_secs": 12
}
```

### Why not tar.gz?

- **Incremental restores**: Users may want to restore just saves or just config, not everything.
- **Browsability**: Files on NFS are directly browsable -- the user can manually grab a save file from their PC without needing the app.
- **Simplicity**: No need for a tar/compression library dependency. The Pi's CPU is limited; compression would slow backups for marginal space savings on already-small files.
- **NFS semantics**: Writing many small files over NFS is fine for the file counts involved (dozens to hundreds, not millions).

### Favorites handling

`_favorites/` contains symlinks pointing at ROM paths. We cannot copy symlinks to NFS (the targets may not exist on the NFS server). Instead, serialize the symlink targets to `favorites.txt`:

```
roms/sega_smd/Sonic The Hedgehog 2 (World) (Rev A).md
roms/nintendo_snes/Super Mario World (USA).sfc
```

On restore, recreate the symlinks. If a target ROM does not exist on the destination, skip it and report it.

---

## 3. UI Design

### Extend the NFS Settings Page

The current NFS page (`/more/nfs`) has: server address, share path, NFS version, save button, reboot button. Add a **Backup section** below the existing NFS config form, visually separated.

```
┌─────────────────────────────────────────┐
│  ← Back            NFS Share Settings   │
├─────────────────────────────────────────┤
│                                         │
│  Server Address                         │
│  ┌─────────────────────────────────┐    │
│  │ 192.168.1.100                   │    │
│  └─────────────────────────────────┘    │
│                                         │
│  Share Path                             │
│  ┌─────────────────────────────────┐    │
│  │ /export/share                   │    │
│  └─────────────────────────────────┘    │
│                                         │
│  NFS Version                            │
│  ┌─────────────────────────────────┐    │
│  │ NFSv4                       ▼  │    │
│  └─────────────────────────────────┘    │
│                                         │
│  ┌─────────────┐                        │
│  │    Save     │                        │
│  └─────────────┘                        │
│                                         │
│  ┌─────────────┐                        │
│  │   Reboot    │                        │
│  └─────────────┘                        │
│                                         │
│ ──────────── Backup ─────────────────── │
│                                         │
│  Back up saves, settings, and config    │
│  to the NFS share above.               │
│                                         │
│  ┌───────────────────┐                  │
│  │   Back Up Now     │                  │
│  └───────────────────┘                  │
│                                         │
│  Last backup: 2026-03-12 14:30          │
│  42 files, 15 MB, took 12s              │
│                                         │
│  ── or, to see/manage all backups: ──   │
│                                         │
│  ┌───────────────────┐                  │
│  │  View Backups     │                  │
│  └───────────────────┘                  │
│                                         │
└─────────────────────────────────────────┘
```

### "Back Up Now" flow

1. User taps "Back Up Now"
2. Button becomes disabled, shows spinner: "Backing up..."
3. Progress text updates in real-time: "Copying saves... (12/42 files)"
4. On success: "Backup complete. 42 files, 15 MB, took 12s."
5. On error: red error message with details (NFS unreachable, disk full, etc.)

### Backup History / View Backups page

A separate page at `/more/nfs/backups` listing all backup directories found on the NFS share:

```
┌─────────────────────────────────────────┐
│  ← Back             Backups             │
├─────────────────────────────────────────┤
│                                         │
│  replaypi · 2026-03-13 14:30            │
│  42 files, 15 MB                        │
│  ┌──────────┐  ┌──────────┐             │
│  │ Restore  │  │  Delete  │             │
│  └──────────┘  └──────────┘             │
│                                         │
│  replaypi · 2026-03-12 09:15            │
│  38 files, 14 MB                        │
│  ┌──────────┐  ┌──────────┐             │
│  │ Restore  │  │  Delete  │             │
│  └──────────┘  └──────────┘             │
│                                         │
└─────────────────────────────────────────┘
```

### Restore flow

1. User taps "Restore" on a backup
2. Confirmation dialog: "This will overwrite your current saves, settings, and config with the backup from 2026-03-13 14:30. Continue?"
3. On confirm: progress updates similar to backup
4. On success: "Restore complete. Reboot recommended for config changes."
5. Optional: show a reboot button

---

## 4. Server Function Design

### Backup

```rust
#[server(prefix = "/sfn")]
pub async fn start_backup() -> Result<BackupResult, ServerFnError> {
    // 1. Read NFS config from replay.cfg
    // 2. Validate NFS config is set (server + share non-empty)
    // 3. Mount NFS share to temp path
    // 4. Create timestamped backup directory
    // 5. Copy files (saves, config, captures, .replay-control items, replay.cfg)
    // 6. Write manifest.json
    // 7. Unmount NFS
    // 8. Return result with stats
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub backup_name: String,       // e.g., "replaypi_20260313_143000"
    pub total_files: usize,
    pub total_bytes: u64,
    pub duration_secs: u64,
}
```

### List Backups

```rust
#[server(prefix = "/sfn")]
pub async fn list_backups() -> Result<Vec<BackupInfo>, ServerFnError> {
    // 1. Mount NFS share
    // 2. Scan .replay-backups/ for backup directories
    // 3. Read manifest.json from each
    // 4. Unmount NFS
    // 5. Return sorted by timestamp (newest first)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub name: String,              // directory name
    pub hostname: String,
    pub timestamp: String,         // ISO 8601
    pub total_files: usize,
    pub total_bytes: u64,
}
```

### Restore

```rust
#[server(prefix = "/sfn")]
pub async fn restore_backup(backup_name: String) -> Result<RestoreResult, ServerFnError> {
    // 1. Mount NFS share
    // 2. Read manifest.json from the specified backup
    // 3. Copy files back to their original locations
    // 4. For favorites: recreate symlinks from favorites.txt (skip missing targets)
    // 5. Unmount NFS
    // 6. Invalidate caches
    // 7. Return result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub total_files: usize,
    pub total_bytes: u64,
    pub skipped_favorites: Vec<String>,  // favorites whose ROM targets don't exist
    pub duration_secs: u64,
}
```

### Delete Backup

```rust
#[server(prefix = "/sfn")]
pub async fn delete_backup(backup_name: String) -> Result<(), ServerFnError> {
    // 1. Mount NFS share
    // 2. Validate backup_name (no path traversal)
    // 3. rm -rf the backup directory
    // 4. Unmount NFS
}
```

### Progress Reporting

For the initial manual backup, synchronous server functions (blocking until complete) are sufficient. The backup of Tier 1+2 data should complete in seconds to a few minutes on a local network. Progress is shown via the response.

If backups take too long (large saves directories), a future iteration can add SSE-based progress reporting following the existing pattern used by metadata import (`import_progress`).

---

## 5. NFS Mount Strategy

### Temporary mount for backup operations

```rust
fn mount_nfs_for_backup(server: &str, share: &str, version: &str) -> Result<PathBuf> {
    let mount_point = PathBuf::from("/tmp/replay-backup-nfs");
    std::fs::create_dir_all(&mount_point)?;

    let ver_flag = format!("vers={version}");
    let source = format!("{server}:{share}");

    let output = Command::new("mount")
        .args(["-t", "nfs", "-o", &ver_flag, &source, mount_point.to_str().unwrap()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("NFS mount failed: {stderr}"));
    }

    Ok(mount_point)
}

fn unmount_nfs_backup(mount_point: &Path) -> Result<()> {
    let _ = Command::new("umount").arg(mount_point).output();
    let _ = std::fs::remove_dir(mount_point);
    Ok(())
}
```

### When storage_mode is already NFS

If `storage_mode=nfs`, the active storage IS the NFS share. In this case:
- The data to back up (saves, config, captures) is already on NFS
- But the user might still want a backup snapshot (point-in-time copy)
- The backup writes to `.replay-backups/` on the same NFS share
- No additional mount is needed -- use the existing storage root's NFS mount

Detection logic:
```rust
if storage.kind == StorageKind::Nfs {
    // Data is already on NFS. Backup destination is on the same share.
    // No mount/unmount needed.
    let nfs_root = storage.root.clone(); // /media/nfs
    backup_to(nfs_root, &storage)?;
} else {
    // Data is on local storage (USB/SD). Mount NFS temporarily.
    let nfs_root = mount_nfs_for_backup(&nfs_server, &nfs_share, &nfs_version)?;
    backup_to(nfs_root, &storage)?;
    unmount_nfs_backup(&nfs_root)?;
}
```

---

## 6. Core Module Design

New module: `replay-control-core/src/backup.rs`

```rust
pub struct BackupEngine {
    /// Where to write the backup (NFS mount root)
    destination_root: PathBuf,
    /// Source storage location
    source: StorageLocation,
    /// Path to replay.cfg on the SD card
    config_path: PathBuf,
    /// Hostname for backup naming
    hostname: String,
}

impl BackupEngine {
    pub fn new(destination_root: PathBuf, source: StorageLocation, config_path: PathBuf, hostname: String) -> Self;

    /// Run a full backup. Returns stats on completion.
    pub fn backup(&self) -> Result<BackupResult>;

    /// List available backups on the destination.
    pub fn list_backups(&self) -> Result<Vec<BackupInfo>>;

    /// Restore a specific backup to the source storage.
    pub fn restore(&self, backup_name: &str) -> Result<RestoreResult>;

    /// Delete a backup.
    pub fn delete(&self, backup_name: &str) -> Result<()>;
}
```

File copy uses `std::fs::copy` (not async -- runs in a `spawn_blocking` context from the server function). Simple recursive directory walk for `saves/`, `config/`, `captures/`.

---

## 7. Edge Cases

### NFS not configured
If `nfs_server` or `nfs_share` is empty in `replay.cfg`, disable the backup button and show: "Configure an NFS share above to enable backups."

### NFS unreachable
The `mount` command will fail with a timeout. Show the error: "Could not connect to NFS server. Check that the server is running and reachable."

### Backup already in progress
Use an `AtomicBool` guard (same pattern as `metadata_operation_in_progress`) to prevent concurrent backups. If a backup is in progress, the button is disabled and shows "Backup in progress..."

### Disk space on NFS
Before starting backup, check available space on the NFS mount with `df`. Compare against estimated backup size. If insufficient, abort with a clear error: "Not enough space on NFS share (need X MB, have Y MB available)."

### Partial backup (interrupted)
If the backup is interrupted (power loss, NFS timeout mid-copy), the backup directory exists but has no `manifest.json` (written last). On `list_backups`, directories without a valid `manifest.json` are shown with a warning "(incomplete)" and can only be deleted, not restored.

### Backup while game is running
Save files could be written to by the emulator while backup is in progress. Since saves are small and written atomically by most libretro cores, the risk of corruption is low. Document that backup is safest when no game is running, but do not prevent it.

### Large saves directory
Some systems (PS1, N64) can have large save states (10-50 MB each). A user with many save states could have 500 MB+ in `saves/`. The backup will take proportionally longer. Progress reporting (file count) keeps the user informed.

### Path traversal in backup_name
The `delete_backup` and `restore_backup` functions must validate that `backup_name` contains no path separators or `..` components. Sanitize to `[a-zA-Z0-9_-]` only.

### Symlink resolution for favorites
When backing up favorites, read each symlink's target with `std::fs::read_link()`. Store relative paths (relative to storage root) in `favorites.txt`. On restore, recreate symlinks only if the target ROM exists.

---

## 8. Future: Automatic Backup

Not in scope for the initial implementation, but the design should not preclude it.

### Possible approaches

1. **Periodic timer**: A background task (like `spawn_storage_watcher`) that runs every N hours / days. Configurable in `settings.cfg` with a key like `backup_auto_interval = "daily"`.

2. **On-shutdown backup**: Trigger backup when RePlayOS shuts down. Requires integration with the systemd service (ExecStop hook). Risk: delays shutdown if NFS is slow.

3. **Incremental backups**: Compare file mtimes against the last backup's manifest. Only copy changed files. Reduces backup time and NFS traffic significantly.

### Retention policy
When automatic backups are added, implement a retention policy: keep the last N backups, or keep backups from the last N days, and auto-delete older ones. Default: keep last 5 backups.

---

## 9. Implementation Phases

### Phase 1: Manual Backup (MVP)

- Add `backup.rs` module to `replay-control-core` with `BackupEngine`
- Implement `start_backup` server function (mount NFS, copy files, write manifest, unmount)
- Add backup section to the NFS settings page UI (button + status)
- Add `list_backups` server function (mount, scan, unmount)
- Show last backup info on the NFS page
- i18n keys for all backup UI strings

**Estimated effort:** 4-6 hours

### Phase 2: Restore + Backup Management

- Implement `restore_backup` server function
- Implement `delete_backup` server function
- Add `/more/nfs/backups` page with backup list, restore, and delete
- Restore confirmation dialog
- Handle incomplete backups (no manifest)

**Estimated effort:** 3-4 hours

### Phase 3: Polish

- Pre-backup disk space check
- Progress reporting for large backups (SSE, following the metadata import pattern)
- Backup-in-progress guard (`AtomicBool`)
- Edge case handling (NFS timeout, partial writes)

**Estimated effort:** 2-3 hours

### Phase 4: Automatic Backups (future)

- Add `backup_auto_interval` setting to `settings.cfg`
- Background task with configurable schedule
- Retention policy (keep last N)
- Incremental backup support (mtime comparison)

**Estimated effort:** 4-6 hours

---

## 10. Files to Create/Modify

### New files
| File | Purpose |
|------|---------|
| `replay-control-core/src/backup.rs` | Backup engine: copy logic, manifest, list, restore, delete |
| `replay-control-app/src/server_fns/backup.rs` | Server functions: `start_backup`, `list_backups`, `restore_backup`, `delete_backup` |
| `replay-control-app/src/pages/backups.rs` | Backup history/management page at `/more/nfs/backups` |

### Modified files
| File | Changes |
|------|---------|
| `replay-control-core/src/lib.rs` | Add `pub mod backup;` |
| `replay-control-app/src/server_fns/mod.rs` | Add `mod backup;` + `pub use backup::*;` |
| `replay-control-app/src/pages/nfs.rs` | Add backup section below NFS config form |
| `replay-control-app/src/pages/mod.rs` | Add `pub mod backups;` |
| `replay-control-app/src/lib.rs` | Add route for `/more/nfs/backups` |
| `replay-control-app/src/main.rs` | Add `register_explicit` for backup server functions |
| `replay-control-app/src/i18n.rs` | Add i18n keys for backup UI strings |
| `replay-control-app/style/style.css` | Backup section styling (minimal, reuses existing form styles) |
