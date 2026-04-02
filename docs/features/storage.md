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

## Automatic Library Updates

On local storage (SD, USB, NVMe), the app monitors the `roms/` directory for changes. New, modified, or deleted ROMs trigger an automatic library update for the affected system.

On NFS, automatic detection is not available (filesystem notifications do not work across network mounts). Use the "Rebuild Game Library" button to pick up changes.

## Storage Change Detection

The app monitors the RePlayOS configuration file for changes. When the storage mode or path changes:

- The library is refreshed automatically
- All connected browsers are notified and reload to reflect the new state

This also applies to skin/theme changes, which are pushed to all browsers instantly.

## Filesystem Adaptation

The app automatically adapts its database configuration to the underlying filesystem:

- **POSIX-capable filesystems** (ext4, btrfs, xfs) -- optimized for concurrent access
- **exFAT/FAT32** (common on USB drives) -- adapted for filesystem limitations
- **NFS** -- adapted for network storage constraints

No user configuration is needed.

## Corruption Recovery

If the metadata database becomes corrupted (e.g., due to unexpected power loss), the app detects it at runtime and shows a recovery banner:

- **Metadata database** -- can be fully rebuilt from the ROM files (no data loss)
- **User data database** -- restored from automatic backups taken at each healthy startup

## App Data Directory

The app stores its data in `.replay-control/` on the ROM storage device, separate from the RePlayOS configuration. This directory contains:

- **Metadata database** -- game library index, imported metadata, thumbnail index (rebuildable)
- **User data database** -- box art overrides, saved videos (persistent)
- **Settings** -- region preference, font size, skin override
- **Media** -- downloaded box art, screenshots, and title screen images

This data travels with the ROM collection if you move the storage device to another Pi.

## Config Boundary

Two configuration files serve different purposes:

- **RePlayOS config** (on the SD card) -- system-level settings (Wi-Fi, NFS, storage mode, skin). Managed by RePlayOS and read by the app. Lives only on the SD card regardless of ROM storage location.
- **App settings** (on ROM storage) -- app-specific preferences (region, language, font size). Travels with the ROM collection.
