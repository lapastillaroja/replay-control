# `.replay-control/` Directory Improvements

Analysis of the current `.replay-control/` directory structure with proposed naming and organizational improvements.

## Current Structure

```
<rom_storage>/.replay-control/
├── config.cfg                    # App-specific settings (region preference, etc.)
├── metadata.db                   # SQLite database — game metadata cache
├── Metadata.xml                  # LaunchBox XML dump (~460 MB)
├── videos.json                   # User-saved video links per game
│
├── media/                        # Game images (box art + screenshots)
│   └── <system>/
│       ├── boxart/
│       │   └── *.png
│       └── snap/
│           └── *.png
│
└── tmp/                          # Cached working data
    └── libretro-thumbnails/      # Shallow git clones (kept between imports)
        └── <repo_name>/
```

## Proposed Changes

### 1. `config.cfg` → `settings.cfg`

| Aspect | Detail |
|--------|--------|
| Current | `config.cfg` |
| Proposed | `settings.cfg` |
| Rationale | `config.cfg` is ambiguous — the codebase already has `replay.cfg` (OS config) and the directory `config/` on the storage root. The word "config" appears in at least three different contexts: RePlayOS config (`replay.cfg`), storage config dir (`config/`), and now app config (`config.cfg`). **`settings.cfg`** makes the purpose immediately clear — these are user-facing app settings, not system configuration. It also avoids confusion with the `config/` directory at the storage root level. |
| Convention | Underscore vs hyphen is moot here since it's a single word. The `.cfg` extension is kept for consistency with `replay.cfg`. |
| Status | **Not yet implemented** — `config.cfg` is referenced only in docs and design documents (region-preference analysis, game-videos plan). No Rust code reads/writes it yet. This is the ideal time to rename. |

**Why not `replay-control.cfg` or `replay_control.cfg`?** The file already lives inside the `.replay-control/` directory, so repeating the directory name in the filename is redundant. `settings.cfg` is shorter, clearer, and follows the principle that files should describe their *content*, not their *owner* (the directory already identifies the owner).

### 2. `Metadata.xml` → `launchbox-metadata.xml`

| Aspect | Detail |
|--------|--------|
| Current | `Metadata.xml` |
| Proposed | `launchbox-metadata.xml` |
| Rationale | `Metadata.xml` is the upstream filename inside LaunchBox's `Metadata.zip`, but on its own it's completely generic — it says nothing about what kind of metadata or from which source. As the app grows to support additional metadata sources, this becomes increasingly confusing. **`launchbox-metadata.xml`** immediately identifies both the format and the source. |
| Convention | Lowercase with hyphens (`launchbox-metadata.xml`) is the dominant convention for data files on Linux. The current `Metadata.xml` uses PascalCase because it was extracted verbatim from the LaunchBox zip — but there's no reason to preserve the upstream convention in our own data directory. |

**Code locations that hardcode `"Metadata.xml"`:**

| File | Line(s) | Context |
|------|---------|---------|
| `replay-control-core/src/launchbox.rs` | 353, 376 | `download_metadata()` — extracts from zip, names the output file |
| `replay-control-app/src/api/import.rs` | 66 | `regenerate_metadata()` — looks for existing XML |
| `replay-control-app/src/api/background.rs` | 15 | `spawn_auto_import()` — checks if XML exists on startup |

**Note:** The upstream zip contains `Metadata.xml` internally, so `unzip` will extract it with that name. The `download_metadata()` function would need to rename after extraction, or use `unzip -j` followed by `mv`. This is a minor change (one extra `std::fs::rename` call).

**A constant should be introduced** (e.g., `const LAUNCHBOX_XML: &str = "launchbox-metadata.xml"` in `launchbox.rs`) to centralize the filename, matching the existing pattern with `DB_FILE` and `VIDEOS_FILE`.

### 3. Duplicate `RC_DIR` constant

| Aspect | Detail |
|--------|--------|
| Current | `RC_DIR` defined in **two places**: `metadata_db.rs:12` (public) and `videos.rs:6` (private) |
| Proposed | Remove the duplicate in `videos.rs`, import from `metadata_db.rs` |
| Rationale | The duplicate string `".replay-control"` in `videos.rs` is a maintenance hazard. If the directory name ever changes, both locations must be updated. The `metadata_db.rs` version is already `pub const` and used everywhere else via `replay_control_core::metadata_db::RC_DIR`. |

### 4. Add `rc_dir()` method to `StorageLocation`

| Aspect | Detail |
|--------|--------|
| Current | Every call site constructs `storage.root.join(RC_DIR)` manually |
| Proposed | Add `pub fn rc_dir(&self) -> PathBuf { self.root.join(RC_DIR) }` to `StorageLocation` in `storage.rs` |
| Rationale | `StorageLocation` already has accessor methods for `roms_dir()`, `saves_dir()`, `config_dir()`, `captures_dir()`, `favorites_dir()`, and `recents_dir()`. The `.replay-control` directory is notably absent from this list, forcing every consumer to import `RC_DIR` and manually join it. This is inconsistent and error-prone. |

**Call sites that would benefit** (currently doing `storage_root.join(RC_DIR)` or `storage.root.join(metadata_db::RC_DIR)`):

| File | Approximate count |
|------|-------------------|
| `replay-control-app/src/api/import.rs` | 6 |
| `replay-control-app/src/server_fns/mod.rs` | 4 |
| `replay-control-app/src/server_fns/search.rs` | 1 |
| `replay-control-app/src/main.rs` | 1 |
| `replay-control-core/src/thumbnails.rs` | 4 |
| `replay-control-core/src/metadata_db.rs` | 1 |
| `replay-control-core/src/videos.rs` | 2 |

### 5. Introduce filename constants for all data files

| Aspect | Detail |
|--------|--------|
| Current | `metadata.db` has a constant (`DB_FILE`), `videos.json` has a constant (`VIDEOS_FILE`), but `Metadata.xml` and `config.cfg` are string literals |
| Proposed | Add constants for every data file in a central location |
| Rationale | Centralizing all filenames as constants makes renames trivial and greppable. |

Proposed constants (could live in a new `replay-control-core/src/paths.rs` or alongside `RC_DIR` in `metadata_db.rs`):

```rust
pub const RC_DIR: &str = ".replay-control";        // already exists
pub const METADATA_DB_FILE: &str = "metadata.db";  // rename from DB_FILE for clarity
pub const LAUNCHBOX_XML: &str = "launchbox-metadata.xml";
pub const VIDEOS_FILE: &str = "videos.json";        // move from videos.rs
pub const SETTINGS_FILE: &str = "settings.cfg";
```

### 6. Documentation bug: `known-issues.md` has wrong video path

| Aspect | Detail |
|--------|--------|
| Current | `docs/known-issues.md:11` says: `".replay-control/videos/{system}/{old_filename}.json"` |
| Actual | Videos are stored in a flat file: `.replay-control/videos.json` (keyed internally by `"{system}/{rom_filename}"`) |
| Fix | Update the table row to reference `videos.json` and note that the key inside the JSON becomes orphaned |

## Summary Table

| Current | Proposed | Type | Rationale |
|---------|----------|------|-----------|
| `config.cfg` | `settings.cfg` | Rename | Disambiguate from `replay.cfg` and `config/` dir |
| `Metadata.xml` | `launchbox-metadata.xml` | Rename | Identify source + format; lowercase convention |
| `RC_DIR` in `videos.rs` | Remove (import from `metadata_db`) | Code cleanup | Eliminate duplicate constant |
| (none) | `StorageLocation::rc_dir()` | New method | Consistency with other `*_dir()` accessors |
| `DB_FILE` (private) | `METADATA_DB_FILE` (public, centralized) | Rename + move | More descriptive; centralized with other constants |
| Scattered string literals | Centralized constants | Refactor | Single source of truth for all filenames |
| Wrong path in `known-issues.md` | Fix to `videos.json` | Doc fix | Incorrect path reference |

## Migration Considerations

### Backwards Compatibility

- **`settings.cfg`**: No migration needed — the file does not exist yet (no code reads/writes it). Pure rename of the planned filename.
- **`launchbox-metadata.xml`**: Users who manually placed `Metadata.xml` in `.replay-control/` would need to rename it. The app should check for the old name as a fallback during a transition period:
  ```rust
  let xml_path = rc_dir.join(LAUNCHBOX_XML);
  let xml_path = if xml_path.exists() {
      xml_path
  } else {
      // Fallback: check old name
      let old_path = rc_dir.join("Metadata.xml");
      if old_path.exists() { old_path } else { xml_path }
  };
  ```
- **`metadata.db`**: No rename proposed — the name is already descriptive enough within context. No migration needed.
- **`videos.json`**: No rename proposed — the name is clear. No migration needed.
- **`media/`**, **`tmp/`**: No changes proposed — these names are fine.

### What Code Needs to Change

#### For `Metadata.xml` → `launchbox-metadata.xml`:

| File | Change |
|------|--------|
| `replay-control-core/src/launchbox.rs:353` | Change `dest_dir.join("Metadata.xml")` to use new constant |
| `replay-control-core/src/launchbox.rs:376` | Change `.arg("Metadata.xml")` — note: this is the filename *inside* the zip, which stays as `Metadata.xml`. After extraction, add `std::fs::rename` to rename to `launchbox-metadata.xml` |
| `replay-control-app/src/api/import.rs:66` | Use new constant |
| `replay-control-app/src/api/import.rs:68` | Update error message string |
| `replay-control-app/src/api/background.rs:15` | Use new constant |

#### For duplicate `RC_DIR` removal:

| File | Change |
|------|--------|
| `replay-control-core/src/videos.rs:6` | Remove `const RC_DIR` line, add `use crate::metadata_db::RC_DIR;` |

#### For `StorageLocation::rc_dir()`:

| File | Change |
|------|--------|
| `replay-control-core/src/storage.rs` | Add `pub fn rc_dir()` method, add `use crate::metadata_db::RC_DIR;` |
| All call sites doing `.join(RC_DIR)` | Replace with `storage.rc_dir()` where a `StorageLocation` is available |

#### Documentation updates:

| File | Change |
|------|--------|
| `docs/reference/replay-control-folder.md` | Update tree view, file descriptions, code references table |
| `docs/known-issues.md:11` | Fix video path from `videos/{system}/{old_filename}.json` to `videos.json` |
| `docs/reference/game-videos-plan.md` | Update filename reference |
| `docs/reference/region-preference-analysis.md` | Update `config.cfg` → `settings.cfg` references |
| `docs/reference/source-code-analysis.md` | Update filename references |
| `docs/game-metadata.md` | Update `Metadata.xml` references |
| `README.md` | Update if any filenames mentioned |
| `MEMORY.md` (user memory) | Update `.replay-control/config.cfg` reference |

## Implementation Checklist

1. [x] **Centralize constants** — Created `RC_DIR` in `storage.rs` (canonical), re-exported from `metadata_db.rs`. Added `METADATA_DB_FILE`, `LAUNCHBOX_XML`, `VIDEOS_FILE`, `SETTINGS_FILE` in `metadata_db.rs`. Removed duplicate `RC_DIR` from `videos.rs`.
2. [x] **Add `StorageLocation::rc_dir()`** — Added the method, migrated all app-crate call sites that had access to a `StorageLocation`.
3. [x] **Rename `Metadata.xml` → `launchbox-metadata.xml`** — Updated `download_metadata()` to rename after extraction. Added old-name fallback in `spawn_auto_import()` and `regenerate_metadata()`.
4. [x] **Update `config.cfg` → `settings.cfg`** references in all design docs — Updated features.md, game-videos-plan.md, region-preference-analysis.md, replay-control-folder.md, search-improvement-analysis.md, skin-theming-analysis.md, source-code-analysis.md.
5. [x] **Fix `known-issues.md`** — Corrected video path from `videos/{system}/{old_filename}.json` to `videos.json` with orphaned key note.
6. [x] **Update all documentation** — Grepped for all old filenames and updated references across docs.
7. **Test** — Verify auto-import on startup still works (checks for XML file), verify manual re-import works, verify `download_metadata` produces the correctly named file.
