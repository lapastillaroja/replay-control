# The `.replay-control/` Folder

> **Note**: See `docs/features/storage.md` for current-state documentation of storage and the `.replay-control/` directory.
> Location: `<rom_storage>/.replay-control/`
> Defined as: `RC_DIR` constant in `replay-control-core/src/platform/storage.rs`

The `.replay-control/` directory is the companion app's data folder on the ROM storage device (SD card, USB drive, or NFS mount). It stores all app-specific data — metadata, images, settings, and temporary files — separate from both the ROM files and the RePlayOS system configuration.

## Config Boundary

**`replay.cfg`** lives on the SD card at `/media/sd/config/replay.cfg` -- ALWAYS. Even when ROM storage is on USB (`/media/usb`) or NFS (`/media/nfs`), `replay.cfg` remains on the SD card. It is NOT on the ROM storage device. The companion app:
- **Reads** it freely for OS-level settings (skin, storage mode, wifi, video, NFS)
- **May write** parameters that RePlayOS does NOT provide its own UI to modify (e.g., `system_skin`, `wifi_name`, `nfs_server`) — the companion app serves as a UI for these
- **Must NOT** write app-specific settings to it (e.g., region preference) — those go in `.replay-control/settings.cfg` (on the ROM storage device)

## Directory Structure

```
<rom_storage>/
├── roms/                          # ROM files organized by system
│   ├── nintendo_snes/
│   ├── sega_smd/
│   └── ...
├── _favorites/                    # RePlayOS favorites (symlinks managed by our app)
│
└── .replay-control/               # Companion app data directory
    ├── settings.cfg               # App-specific settings (region preference, etc.)
    ├── metadata.db                # SQLite database — game metadata cache
    ├── user_data.db               # SQLite database — user customizations (box art overrides, saved videos)
    ├── launchbox-metadata.xml     # LaunchBox XML dump (downloaded or manually placed)
    │
    ├── media/                     # Game images (box art, screenshots, title screens)
    │   ├── nintendo_snes/
    │   │   ├── boxart/
    │   │   │   ├── Super Mario World (USA).png
    │   │   │   └── ...
    │   │   ├── snap/
    │   │   │   ├── Super Mario World (USA).png
    │   │   │   └── ...
    │   │   └── title/
    │   │       ├── Super Mario World (USA).png
    │   │       └── ...
    │   ├── sega_smd/
    │   │   ├── boxart/
    │   │   ├── snap/
    │   │   └── title/
    │   ├── arcade_mame/
    │   │   ├── boxart/
    │   │   ├── snap/
    │   │   └── title/
    │   └── ...
    │
    └── tmp/                       # Cached files (git clones for image import)
        └── libretro-thumbnails/   # Shallow clones of libretro-thumbnails repos
            ├── Sega - Mega Drive - Genesis/
            └── ...                # Auto-deleted after matching; "Clear Cache" button available
```

## Files and Their Purpose

### `settings.cfg`
App-specific user settings in `key = "value"` format (same syntax as `replay.cfg`). Uses the existing `ReplayConfig` parser.

**Current keys:**
| Key | Values | Default | Description |
|-----|--------|---------|-------------|
| `region_preference` | `"usa"`, `"europe"`, `"japan"`, `"world"` | `"usa"` | Primary preferred ROM region for sort/search ranking |
| `region_secondary` | `"usa"`, `"europe"`, `"japan"`, `"world"` | (none) | Secondary region preference (two-tier sort: Primary > Secondary > World > others) |
| `text_size` | `"normal"`, `"large"` | `"normal"` | UI text size (normal=110%, large=140%) |

This file is created on first write. Missing keys use defaults.

### `metadata.db`
SQLite database caching external game metadata. Stores:
- **Game metadata** (`game_metadata` table): descriptions, ratings, publishers, genres, player counts (from LaunchBox XML import)
- **Game library** (`game_library` / `game_library_meta` tables): L2 persistent cache for ROM listings with box art URLs and ratings
- **Image paths**: relative paths to box art and screenshot files per ROM
- **Thumbnail index** (`thumbnail_index` table): manifest of all available libretro-thumbnails images across ~40 repos
- **Data sources** (`data_sources` table): version tracking for LaunchBox imports and per-repo libretro thumbnail index freshness

Uses `nolock` VFS fallback on NFS mounts (NFS doesn't support SQLite file locking).

**Source code**: `replay-control-core/src/metadata/metadata_db/`

### `user_data.db`
SQLite database for persistent user customizations. Unlike `metadata.db` (which is a rebuildable cache), this file stores deliberate user choices that cannot be reconstructed from external sources.

**Current tables:**
| Table | Purpose |
|---|---|
| `box_art_overrides` | User-chosen region variant for a game's cover art. Keyed by `(system, rom_filename)`. |
| `game_videos` | Saved/pinned video links (YouTube, Twitch, etc.). Keyed by `(system, base_title, video_id)`. Shared across regional variants via `base_title`. |

Uses `nolock` VFS fallback on NFS mounts (same pattern as `metadata.db`).

**Key invariant:** This file is never touched by any "Clear Metadata" or "Clear Images" operation. It survives all cache rebuilds.

**Source code**: `replay-control-core/src/metadata/user_data_db.rs`

### `launchbox-metadata.xml`
The LaunchBox metadata XML dump (~460 MB, ~78K game entries). Either:
- Downloaded automatically via the metadata management UI
- Placed manually by the user (the old name `Metadata.xml` is accepted as a fallback)

Parsed during import to populate `metadata.db`. Kept on disk for re-imports.

### `media/<system>/boxart/`, `media/<system>/snap/`, and `media/<system>/title/`
PNG image files imported from [libretro-thumbnails](https://github.com/libretro-thumbnails) GitHub repos. One subdirectory per system.

- **boxart/**: Box art / cover art images (`Named_Boxarts`)
- **snap/**: In-game screenshot images (`Named_Snaps`)
- **title/**: Title screen images (`Named_Titles`)

Filenames follow the libretro-thumbnails convention: display name with `&*/:\`<>?\\|` replaced by `_`.

Served to the browser at `/media/<system>/boxart/<file>.png` via the Axum media handler in `main.rs`.

**Source code**: `replay-control-core/src/metadata/thumbnails.rs`

### `tmp/libretro-thumbnails/`
Shallow git clones of libretro-thumbnails repos, created during the legacy image import path. Each system's repo is cloned, images are matched and copied to `media/`, and the clone is **auto-deleted after successful matching** to prevent disk from filling up.

**Note:** The new manifest-based thumbnail system (`thumbnail_manifest.rs`) downloads images directly from `raw.githubusercontent.com` via the `thumbnail_index` table, eliminating the need for git clones entirely. This directory is only used by the legacy git-clone import path.

A "Clear Cache" button on the metadata page removes any remaining repos in this directory. Safe to delete manually at any time.

## Size Considerations

On a typical collection:
- `metadata.db`: ~5-20 MB (includes thumbnail_index with ~200K entries across 40 systems)
- `launchbox-metadata.xml`: ~460 MB (can be deleted after import to save space)
- `media/`: 200 MB - 2 GB depending on how many systems have images
- `tmp/`: 0 bytes in normal operation (legacy git-clone path only); safe to delete

## Code References

| Constant/Function | File | Purpose |
|---|---|---|
| `RC_DIR` | `platform/storage.rs` | The `.replay-control` directory name |
| `SETTINGS_FILE` | `platform/storage.rs` | `"settings.cfg"` filename constant |
| `LAUNCHBOX_XML` | `metadata/metadata_db/` | `"launchbox-metadata.xml"` filename constant |
| `METADATA_DB_FILE` | `metadata/metadata_db/` | `"metadata.db"` filename constant |
| `USER_DATA_DB_FILE` | `metadata/user_data_db.rs` | `"user_data.db"` filename constant |
| `UserDataDb::open()` | `metadata/user_data_db.rs` | Opens/creates `user_data.db` |
| `UserDataDb::add_game_video()` | `metadata/user_data_db.rs` | Saves a video link to `game_videos` table |
| `StorageLocation::rc_dir()` | `platform/storage.rs` | Returns `<root>/.replay-control` path |
| `MetadataDb::open()` | `metadata/metadata_db/` | Opens/creates `metadata.db` |
| `import_system_thumbnails()` | `metadata/thumbnails.rs` | Copies images from cloned repo to `media/` |
| `clone_thumbnail_repo()` | `metadata/thumbnails.rs` | Clones a repo into `tmp/` (reuses existing if not stale) |
| `is_repo_stale()` | `metadata/thumbnails.rs` | Checks if local HEAD differs from remote HEAD |
| `media_dir_size()` | `metadata/thumbnails.rs` | Calculates total `media/` size |
| `clear_media()` | `metadata/thumbnails.rs` | Deletes all `media/` content |
| Media HTTP handler | `main.rs` | Serves `media/` files at `/media/` URL path |
