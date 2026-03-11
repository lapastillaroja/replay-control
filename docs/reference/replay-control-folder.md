# The `.replay-control/` Folder

> Location: `<rom_storage>/.replay-control/`
> Defined as: `RC_DIR` constant in `replay-control-core/src/storage.rs`

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
    ├── launchbox-metadata.xml     # LaunchBox XML dump (downloaded or manually placed)
    ├── videos.json                # User-saved video links per game
    │
    ├── media/                     # Game images (box art + screenshots)
    │   ├── nintendo_snes/
    │   │   ├── boxart/
    │   │   │   ├── Super Mario World (USA).png
    │   │   │   └── ...
    │   │   └── snap/
    │   │       ├── Super Mario World (USA).png
    │   │       └── ...
    │   ├── sega_smd/
    │   │   ├── boxart/
    │   │   └── snap/
    │   ├── arcade_mame/
    │   │   ├── boxart/
    │   │   └── snap/
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

**Current/planned keys:**
| Key | Values | Default | Description |
|-----|--------|---------|-------------|
| `region_preference` | `"usa"`, `"europe"`, `"japan"`, `"world"` | `"usa"` | Preferred ROM region for sort/search ranking |

This file is created on first write. Missing keys use defaults.

### `metadata.db`
SQLite database caching external game metadata. Stores:
- **Game metadata**: descriptions, ratings, publishers, genres (from LaunchBox XML import)
- **Image paths**: relative paths to box art and screenshot files per ROM
- **Per-system coverage stats**: how many ROMs have metadata/images

Uses `nolock` VFS fallback on NFS mounts (NFS doesn't support SQLite file locking).

**Source code**: `replay-control-core/src/metadata_db.rs`

### `launchbox-metadata.xml`
The LaunchBox metadata XML dump (~460 MB, ~78K game entries). Either:
- Downloaded automatically via the metadata management UI
- Placed manually by the user (the old name `Metadata.xml` is accepted as a fallback)

Parsed during import to populate `metadata.db`. Kept on disk for re-imports.

### `videos.json`
User-saved video links per game. JSON format with a `games` map keyed by `"{system}/{rom_filename}"`. Each entry stores URL, platform, video ID, title, tag (trailer/gameplay/1cc), and timestamp.

Separate from `metadata.db` so video data survives metadata clears. Written atomically (write `.tmp` then `rename`). Mutex-guarded via AppState.

**Source code**: `replay-control-core/src/videos.rs`, `replay-control-core/src/video_url.rs`

### `media/<system>/boxart/` and `media/<system>/snap/`
PNG image files imported from [libretro-thumbnails](https://github.com/libretro-thumbnails) GitHub repos. One subdirectory per system.

- **boxart/**: Box art / cover art images
- **snap/**: In-game screenshot images

Filenames follow the libretro-thumbnails convention: display name with `&*/:\`<>?\\|` replaced by `_`.

Served to the browser at `/media/<system>/boxart/<file>.png` via the Axum media handler in `main.rs`.

**Source code**: `replay-control-core/src/thumbnails.rs`

### `tmp/libretro-thumbnails/`
Shallow git clones of libretro-thumbnails repos, created during image import. Each system's repo is cloned, images are matched and copied to `media/`, and the clone is **auto-deleted after successful matching** to prevent disk from filling up (repos previously caused ~10:1 overhead vs useful image data).

A "Clear Cache" button on the metadata page removes any remaining repos in this directory. The "Re-match All" feature works only with repos already on disk (truly offline -- no staleness check, no network access).

Safe to delete manually at any time -- repos will be re-cloned on the next download.

## Size Considerations

On a typical collection:
- `metadata.db`: ~5-15 MB
- `launchbox-metadata.xml`: ~460 MB (can be deleted after import to save space)
- `media/`: 200 MB – 2 GB depending on how many systems have images
- `tmp/`: 0 bytes initially; grows as repos are cached (several GB if all systems imported); safe to delete

## Code References

| Constant/Function | File | Purpose |
|---|---|---|
| `RC_DIR` | `storage.rs` | The `.replay-control` directory name |
| `SETTINGS_FILE` | `storage.rs` | `"settings.cfg"` filename constant |
| `LAUNCHBOX_XML` | `metadata_db.rs` | `"launchbox-metadata.xml"` filename constant |
| `METADATA_DB_FILE` | `metadata_db.rs` | `"metadata.db"` filename constant |
| `VIDEOS_FILE` | `storage.rs` | `"videos.json"` filename constant |
| `StorageLocation::rc_dir()` | `storage.rs` | Returns `<root>/.replay-control` path |
| `MetadataDb::open()` | `metadata_db.rs:83` | Opens/creates `metadata.db` |
| `import_system_thumbnails()` | `thumbnails.rs:294` | Copies images from cloned repo to `media/` |
| `clone_thumbnail_repo()` | `thumbnails.rs:565` | Clones a repo into `tmp/` (reuses existing if not stale) |
| `is_repo_stale()` | `thumbnails.rs:511` | Checks if local HEAD differs from remote HEAD |
| `media_dir_size()` | `thumbnails.rs:726` | Calculates total `media/` size |
| `clear_media()` | `thumbnails.rs:759` | Deletes all `media/` content |
| Media HTTP handler | `main.rs:158` | Serves `media/` files at `/media/` URL path |
