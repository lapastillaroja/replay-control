# Storage

How storage detection, filesystem watching, and the config boundary work.

## Storage Detection

`StorageLocation::detect()` reads `storage_mode` from `replay.cfg` and resolves the storage root:

| Mode | Path | Filesystem | Notes |
|------|------|------------|-------|
| `sd` | `/media/sd` | ext4 on SD card | Default |
| `usb` | `/media/usb` | ext4/exFAT on USB | Most common for large collections |
| `nvme` | `/media/nvme` | ext4 on NVMe | Pi 5 PCIe support |
| `nfs` | `/media/nfs` | NFS v4 mount | Network share from desktop/NAS |

The `--storage-path` CLI flag bypasses detection entirely (used for local development).

## StorageKind

`StorageKind` enum (`Sd`, `Usb`, `Nvme`, `Nfs`) affects behavior:

- **`is_local()` = true** (Sd, Usb, Nvme): inotify filesystem watcher enabled, SQLite WAL mode for concurrent reads
- **`is_local()` = false** (Nfs): No filesystem watcher, SQLite uses `nolock` VFS with DELETE journal mode (NFS does not support file locking)

## Config File Watcher

`spawn_storage_watcher()` monitors `replay.cfg` using two mechanisms:
1. **inotify** via `notify` crate: instant notification on config file changes
2. **60-second poll**: fallback timer that re-reads config and re-detects storage

On config change, `refresh_storage()` re-reads `replay.cfg`, re-detects storage, and invalidates caches if the storage root or kind changed.

## ROM Directory Watcher

`spawn_rom_watcher()` (local storage only) sets up a recursive `notify` watcher on the `roms/` directory:
- Debounce window: 3 seconds (batches rapid changes from bulk copies)
- Extracts affected system name from event path
- Triggers cache invalidation + re-enrichment for affected systems
- Detects new system directories when `roms/` top-level changes

## Config Boundary

Two config files serve different purposes:

### `replay.cfg` (RePlayOS, on SD card)
- Path: `/media/sd/config/replay.cfg`
- Belongs to RePlayOS, lives ONLY on the SD card regardless of ROM storage location
- App reads it freely; may write parameters RePlayOS has no UI for (skin, wifi, nfs settings)

### `.replay-control/settings.cfg` (App, on ROM storage)
- Path: `<rom_storage>/.replay-control/settings.cfg`
- App-specific settings that travel with the ROM collection
- Currently: `region_preference`
- Same `key = "value"` syntax, parsed by `ReplayConfig`

## `.replay-control/` Directory

The app's data directory on ROM storage. Full structure documented in `docs/reference/replay-control-folder.md`.

Key files:
- `metadata.db` -- Rebuildable cache (game metadata, game library, thumbnail index, aliases, series)
- `user_data.db` -- User customizations that survive cache clears (box art overrides, saved videos)
- `settings.cfg` -- App-specific settings (region preference, secondary region, text size)
- `media/` -- Downloaded box art, screenshot, and title screen images

Database access uses a `deadpool-sqlite` connection pool (`DbPool`) with separate read and write pools. On local storage (WAL mode), multiple concurrent read connections are allowed alongside a single write connection. On NFS (`nolock` VFS), pools are limited to 1 connection each. A `metadata_operation_in_progress` busy flag prevents race conditions between background operations (import, thumbnail download, enrichment).

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/platform/storage.rs` | StorageLocation, StorageKind, detect(), RC_DIR |
| `replay-control-core/src/platform/config.rs` | ReplayConfig parser |
| `replay-control-app/src/api/mod.rs` | AppState, DbPool, refresh_storage() |
| `replay-control-app/src/api/background.rs` | Config watcher, ROM watcher, storage poll |
