# Storage

How storage detection, automatic updates, and the app data directory work.

## Storage Modes

Replay Control reads the storage mode from the [RePlayOS](https://www.replayos.com/) configuration and adapts accordingly:

| Mode | Description |
|------|-------------|
| SD card | Default storage on the RePlayOS SD card |
| USB | Most common for large collections (USB drive) |
| NVMe | Pi 5 PCIe NVMe support |
| NFS | Network share from a desktop or NAS |

The storage mode can be changed in the RePlayOS configuration. When the mode changes, the app detects it automatically and refreshes the library.

RePlayOS can separately keep saves, save states, and captures on its local SD card while ROMs remain on another storage device. Replay Control follows that data-location setting and shows its current value on the RePlayOS settings page. Older RePlayOS versions that do not offer the setting continue using ROM storage.

## Automatic Library Updates

On local storage (SD, USB, NVMe), the app monitors the `roms/` directory for changes. New, modified, or deleted ROMs trigger an automatic library update for the affected system after a short debounce window.

On NFS, live ROM-change detection is intentionally disabled because filesystem notifications do not reliably report changes made by other NFS clients, and periodic probing of a stale hard-mounted share can block. Startup scans still reconcile the library with the current share contents, but changes made while Replay Control is already running should be followed by **Rescan Library** from **Settings > Library & Games > Game Library**.

## Storage Change Detection

The app monitors the RePlayOS configuration file for changes. When the storage mode or path changes:

- The library is refreshed automatically
- All connected browsers are notified and reload to reflect the new state

This also applies to skin/theme changes, which are pushed to all browsers instantly.

If the configured storage target cannot be activated (for example NVMe is selected but no NVMe drive is mounted), Replay Control keeps the last successfully opened storage online when one exists and shows a storage-status banner. The banner names the configured target, the fallback storage if available, and tells the user to insert the device or change the selection in RePlayOS settings. Read-only browsing remains available on the fallback storage, but launch, rebuild/rescan, upload, rename/delete, metadata refresh, media downloads, favorites, and other storage-scoped mutations are blocked until the configured storage is ready. On first boot with no usable storage, the `/waiting` page shows the same configured-storage problem instead of silently waiting on an unspecified device.

Replay Control follows the storage selection saved by RePlayOS, including a change back to SD while a previous USB or NVMe mount is still present. It does not search other media folders when the selected device is unavailable. While storage is unavailable, it retries the selected device automatically. When storage returns at the same path, it checks that the ROM directory is readable and verifies the library again; while storage remains healthy, it does not periodically probe an NFS share that could block.

Long-running startup scans, manual rescans/rebuilds, and ROM-watcher scans are tied to the storage generation that was active when they started. A storage swap or transition into a configured-storage error cancels stale scan work before it writes the next system, preventing fallback or old-storage scan results from landing after the active storage state has changed.

## Filesystem Adaptation

The app automatically adapts its database configuration to the underlying filesystem:

- **POSIX-capable filesystems** (ext4, btrfs, xfs) -- optimized for concurrent access
- **exFAT/FAT32** (common on USB drives) -- adapted for filesystem limitations
- **NFS** -- adapted for network storage constraints

No user configuration is needed.

## Corruption Recovery

If the library database becomes corrupted (e.g., due to unexpected power loss), the app detects it at runtime and shows a recovery banner:

- **Library database** -- can be fully rebuilt from the ROM files (no data loss)
- **User data database** -- restored from automatic backups taken at each healthy startup

## App Data Directory

The app keeps data that belongs to a ROM collection on that storage device:

- **User data** -- favorites, overrides, and other library-specific choices
- **Media** -- downloaded box art, screenshots, and title screen images

This data travels with the ROM collection if you move the storage device to another computer. The library index and downloaded metadata sources stay on the computer running Replay Control.

In standalone mode, the selected folder is remembered on that computer. Changing folders does not erase your app preferences; each library keeps its own favorites, artwork, and other library data. Standalone mode is **work in progress and unsupported**.

On first startup, choose the folder that contains `roms/`. Replay Control then continues to the metadata setup step. You can change the folder later from Settings.

## Config Boundary

Two configuration files serve different purposes:

- **RePlayOS config** (on the SD card) -- system-level settings (Wi-Fi, NFS, storage mode, skin). Managed by RePlayOS and read by the app. Lives only on the SD card regardless of ROM storage location.
- **App settings** -- app-specific preferences such as region, language, and text size.
