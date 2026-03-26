# Storage

How storage detection, filesystem watching, and the config boundary work.

## Storage Detection

`StorageLocation::detect()` reads `storage_mode` from `replay.cfg` and resolves the storage root:

| Mode | Path | Filesystem | SQLite Journal | Notes |
|------|------|------------|---------------|-------|
| `sd` | `/media/sd` | ext4 on SD card | WAL | Default |
| `usb` | `/media/usb` | ext4 or exFAT on USB | WAL (ext4) or DELETE (exFAT) | Most common for large collections |
| `nvme` | `/media/nvme` | ext4 on NVMe | WAL | Pi 5 PCIe support |
| `nfs` | `/media/nfs` | NFS v4 mount | DELETE (nolock VFS) | Network share from desktop/NAS |

The `--storage-path` CLI flag bypasses detection entirely (used for local development).

## StorageKind

`StorageKind` enum (`Sd`, `Usb`, `Nvme`, `Nfs`) affects behavior:

- **`is_local()` = true** (Sd, Usb, Nvme): inotify filesystem watcher enabled. SQLite journal mode depends on the filesystem (detected via `/proc/mounts`): WAL mode on POSIX-capable filesystems (ext4, btrfs, xfs, f2fs, tmpfs) for concurrent reads; DELETE mode on exFAT/FAT32 (WAL's shared memory doesn't work reliably on these filesystems).
- **`is_local()` = false** (Nfs): No filesystem watcher, SQLite uses `nolock` VFS with DELETE journal mode (NFS does not support file locking)

## Config File Watcher

`spawn_storage_watcher()` monitors `replay.cfg` using two mechanisms:
1. **inotify** via `notify` crate: instant notification on config file changes
2. **60-second poll**: fallback timer that re-reads config and re-detects storage

On config change, `refresh_storage()` re-reads `replay.cfg`, re-detects storage, and invalidates caches if the storage root or kind changed. Storage changes are pushed to all connected browsers via broadcast SSE (`/sse/config`) to trigger client reload.

## Broadcast SSE

The app uses broadcast Server-Sent Events for real-time push notifications:

- **`/sse/config`** — pushes skin changes and storage changes to all connected browsers. Skin changes update the app's color scheme instantly; storage changes trigger a full client reload.
- **Activity SSE** — converted from polling to broadcast; background operations (scanning, importing) push progress updates to connected clients instead of clients polling for status.

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

Database access uses a `deadpool-sqlite` connection pool (`DbPool`) with separate read and write pools and a 10-second pool wait timeout. The SQLite journal mode is chosen based on the filesystem (detected via `/proc/mounts`): WAL mode on POSIX-capable filesystems (ext4, btrfs, xfs, f2fs, tmpfs) allows multiple concurrent read connections alongside a single write connection. DELETE mode is used on exFAT/FAT32 USB drives (WAL shared memory doesn't work reliably) and NFS (`nolock` VFS). Both modes use 3 read + 1 write connections (SQLite DELETE mode supports concurrent readers when no writer is active). Pool access is fully async (`pool.get().await` + `conn.interact().await`) to avoid pinning tokio worker threads. A `scanning` flag prevents race conditions between background operations (import, thumbnail download, enrichment). At startup, `user_data.db` is backed up to `.bak` if healthy; runtime corruption is detected via `SQLITE_CORRUPT` error codes and surfaced with a recovery banner.

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/platform/storage.rs` | StorageLocation, StorageKind, detect(), RC_DIR |
| `replay-control-core/src/platform/config.rs` | ReplayConfig parser |
| `replay-control-app/src/api/mod.rs` | AppState, DbPool, refresh_storage() |
| `replay-control-app/src/api/background.rs` | Config watcher, ROM watcher, storage poll |
