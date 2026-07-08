//! Image thumbnail support via libretro-thumbnails.
//!
//! Maps RePlayOS system folder names to libretro-thumbnails repo names,
//! normalizes filenames, and provides fuzzy matching utilities.

pub mod manifest;
pub mod matching;
pub mod resolution;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::fs_walk;
use crate::library_db::{GameEntry, LibraryDb};
use replay_control_core::error::{Error, Result};

/// Percent-encode one URI path segment, keeping only the RFC-3986 unreserved
/// set (`A-Za-z0-9-._~`). Shared by the manifest and resolution modules, which
/// each previously carried a byte-identical copy.
pub(crate) fn percent_encode_uri_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

/// Kind of thumbnail image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailKind {
    Boxart,
    Snap,
    Title,
}

/// All thumbnail kinds, for iteration.
pub const ALL_THUMBNAIL_KINDS: &[ThumbnailKind] = &[
    ThumbnailKind::Boxart,
    ThumbnailKind::Title,
    ThumbnailKind::Snap,
];

/// Persistable thumbnail media counters for one system.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThumbnailMediaStats {
    pub system: String,
    pub total_size_bytes: u64,
    pub file_count: usize,
    pub boxart_file_count: usize,
    pub snap_file_count: usize,
    pub title_file_count: usize,
}

impl ThumbnailKind {
    /// Subdirectory name in the libretro-thumbnails repo.
    pub fn repo_dir(&self) -> &'static str {
        match self {
            ThumbnailKind::Boxart => "Named_Boxarts",
            ThumbnailKind::Snap => "Named_Snaps",
            ThumbnailKind::Title => "Named_Titles",
        }
    }

    /// Subdirectory name in our media storage.
    pub fn media_dir(&self) -> &'static str {
        match self {
            ThumbnailKind::Boxart => "boxart",
            ThumbnailKind::Snap => "snap",
            ThumbnailKind::Title => "title",
        }
    }

    /// Parse a repo directory name back to a `ThumbnailKind`.
    pub fn from_repo_dir(s: &str) -> Option<Self> {
        match s {
            "Named_Boxarts" => Some(ThumbnailKind::Boxart),
            "Named_Snaps" => Some(ThumbnailKind::Snap),
            "Named_Titles" => Some(ThumbnailKind::Title),
            _ => None,
        }
    }
}

/// Convert a libretro-thumbnails repo display name to its URL-safe form.
///
/// Replaces spaces with underscores, e.g.,
/// `"Nintendo - Super Nintendo Entertainment System"` → `"Nintendo_-_Super_Nintendo_Entertainment_System"`.
pub fn repo_url_name(display_name: &str) -> String {
    display_name.replace(' ', "_")
}

/// Build a `data_sources` key from a repo display name.
///
/// Returns `"libretro:{url_name}"`, e.g., `"libretro:Nintendo_-_Super_Nintendo_Entertainment_System"`.
pub fn libretro_source_name(display_name: &str) -> String {
    format!("libretro:{}", repo_url_name(display_name))
}

/// Check if any system has downloaded thumbnail images on disk.
/// Scans `<rc_dir>/media/*/boxart/` for valid PNG files (>= 200 bytes).
pub fn any_images_on_disk(rc_dir: &std::path::Path) -> bool {
    let media_dir = rc_dir.join("media");
    let Ok(entries) = std::fs::read_dir(&media_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let boxart_dir = entry.path().join(ThumbnailKind::Boxart.media_dir());
        if boxart_dir.is_dir()
            && let Ok(mut files) = std::fs::read_dir(&boxart_dir)
            && files.any(|f| {
                f.ok()
                    .map(|f| f.path())
                    .is_some_and(|p| is_valid_image_sync(&p))
            })
        {
            return true;
        }
    }
    false
}

/// Scan a system's media directories and collect all valid image filenames
/// as thumbnail index entries (kind, filename_stem, None).
pub fn scan_system_images(
    media_system_dir: &std::path::Path,
) -> Vec<(String, String, Option<String>)> {
    let mut entries = Vec::new();
    for kind in ALL_THUMBNAIL_KINDS {
        let dir = media_system_dir.join(kind.media_dir());
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stem) = strip_image_ext(&name_str)
                && is_valid_image_sync(&file.path())
            {
                entries.push((kind.repo_dir().to_string(), stem.to_string(), None));
            }
        }
    }
    entries
}

/// Normalize a ROM filename stem to match libretro-thumbnails naming.
///
/// libretro-thumbnails replaces `&*/:`\<>?\\|` with `_` in filenames.
pub fn thumbnail_filename(rom_stem: &str) -> String {
    rom_stem
        .chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            _ => c,
        })
        .collect()
}

// Re-export title utilities from their canonical home in `title_utils`.
// These were originally defined here but moved to `title_utils` for broader reuse.
pub use replay_control_core::title_utils::{
    base_title, filename_stem, strip_n64dd_prefix, strip_tags, strip_version,
};

/// Strip a `.png` or `.jpg` extension from a filename, returning the stem.
pub fn strip_image_ext(name: &str) -> Option<&str> {
    name.strip_suffix(".png")
        .or_else(|| name.strip_suffix(".jpg"))
}

/// Quick check that a file is likely a real image (not a git fake-symlink text file).
pub(crate) fn is_valid_image_sync(path: &Path) -> bool {
    path.metadata().map(|m| m.len() >= 200).unwrap_or(false)
}

/// Try to resolve a small file as a git fake-symlink artifact.
/// Reads its text content (a relative filename), checks if that file exists in
/// `parent_dir` and passes `is_valid_image()`. Returns the target filename on
/// success, `None` otherwise.
pub(crate) fn try_resolve_fake_symlink_sync(path: &Path, parent_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let target_name = std::str::from_utf8(&bytes).ok()?.trim();
    if target_name.is_empty() || !target_name.ends_with(".png") {
        return None;
    }
    let target_path = parent_dir.join(target_name);
    if target_path.exists() && is_valid_image_sync(&target_path) {
        Some(target_name.to_string())
    } else {
        None
    }
}

/// The tiers a `thumbnail_filename`-normalized ROM name matches an image by, in
/// priority order: exact stem, case-insensitive, base-title (tags stripped),
/// version-stripped, and slash-split dual-title parts.
///
/// Single source of truth for those tiers, shared by the per-request resolver
/// [`find_image_on_disk`] (walks tiers in priority order over one directory) and
/// the bulk orphan-detection key-set (`ThumbnailKeys`, which unions the tiers
/// across many ROMs and scans once). One definition keeps the two from drifting.
struct ThumbMatchKeys {
    exact: String,
    lower: String,
    base: String,
    version: Option<String>,
    slash_parts: Vec<String>,
}

fn thumb_match_keys(thumb_name: &str) -> ThumbMatchKeys {
    let base = base_title(thumb_name);
    let stripped = strip_version(&base);
    let version = (stripped.len() < base.len()).then(|| stripped.to_string());
    let search_base = version.as_deref().unwrap_or(&base);
    let sep = if search_base.contains(" / ") {
        Some(" / ")
    } else if search_base.contains(" _ ") {
        Some(" _ ")
    } else {
        None
    };
    let slash_parts = sep
        .map(|sep| {
            search_base
                .split(sep)
                .map(str::trim)
                .filter(|p| p.len() >= 5)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    ThumbMatchKeys {
        lower: thumb_name.to_lowercase(),
        exact: thumb_name.to_string(),
        base,
        version,
        slash_parts,
    }
}

/// Try to find an image file on disk for a ROM, checking exact and fuzzy name matches.
///
/// `media_base` is the system media directory (e.g., `.replay-control/media/sega_smd`).
/// `kind` is the subdirectory name (e.g., `"boxart"` or `"snap"`).
///
/// Returns a relative path like `"boxart/Sonic The Hedgehog 3 (USA).png"` on match.
pub fn find_image_on_disk(media_base: &Path, kind: &str, rom_filename: &str) -> Option<String> {
    let kind_dir = media_base.join(kind);
    if !kind_dir.exists() {
        return None;
    }

    let stem = filename_stem(rom_filename);
    let stem = strip_n64dd_prefix(stem);
    let thumb_name = thumbnail_filename(stem);

    // 1. Exact match
    let exact = kind_dir.join(format!("{thumb_name}.png"));
    if exact.exists() {
        if is_valid_image_sync(&exact) {
            return Some(format!("{kind}/{thumb_name}.png"));
        }
        if let Some(resolved) = try_resolve_fake_symlink_sync(&exact, &kind_dir) {
            return Some(format!("{kind}/{resolved}"));
        }
    }

    let keys = thumb_match_keys(&thumb_name);

    if let Ok(entries) = std::fs::read_dir(&kind_dir) {
        let mut fuzzy_result: Option<String> = None;
        let mut version_result: Option<String> = None;
        let mut slash_result: Option<String> = None;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(img_stem) = strip_image_ext(&name) {
                // 1b. Case-insensitive exact match (preserves region tags)
                if img_stem.to_lowercase() == keys.lower {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        return Some(format!("{kind}/{name}"));
                    }
                    if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        return Some(format!("{kind}/{resolved}"));
                    }
                }

                let img_base = base_title(img_stem);
                // 2. Fuzzy match (strip tags)
                if img_base == keys.base && fuzzy_result.is_none() {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        fuzzy_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        fuzzy_result = Some(format!("{kind}/{resolved}"));
                    }
                }
                // 3. Version-stripped match
                if version_result.is_none() && keys.version.as_deref() == Some(img_base.as_str()) {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        version_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        version_result = Some(format!("{kind}/{resolved}"));
                    }
                }
                // 4. Slash dual-name match
                if slash_result.is_none() && keys.slash_parts.contains(&img_base) {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        slash_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        slash_result = Some(format!("{kind}/{resolved}"));
                    }
                }
            }
        }

        if let Some(result) = fuzzy_result {
            return Some(result);
        }
        if let Some(result) = version_result {
            return Some(result);
        }
        if let Some(result) = slash_result {
            return Some(result);
        }
    }

    None
}

/// Resolve an image file on disk for a ROM, with arcade name translation.
///
/// This is the main entry point for per-file image resolution. For arcade systems,
/// the caller passes `arcade_display` (the MAME display name, e.g.
/// `Golden Axe: The Revenge of Death Adder`) which is tried first; then falls back
/// to the ROM filename. For non-arcade systems, pass `None` — it delegates directly
/// to `find_image_on_disk`.
///
/// Use this instead of calling `find_image_on_disk` directly to avoid forgetting
/// arcade name translation when adding new image types.
pub(crate) fn resolve_image_on_disk_sync(
    arcade_display: Option<&str>,
    media_base: &Path,
    kind: &str,
    rom_filename: &str,
) -> Option<String> {
    if let Some(display) = arcade_display {
        let thumb = thumbnail_filename(display);
        let arcade_filename = format!("{thumb}.zip");
        if let Some(path) = find_image_on_disk(media_base, kind, &arcade_filename) {
            return Some(path);
        }
    }
    find_image_on_disk(media_base, kind, rom_filename)
}

/// Validate an image file on disk. Runs the `metadata` syscall on the
/// blocking pool so async callers don't pin a tokio worker.
pub async fn is_valid_image(path: PathBuf) -> bool {
    tokio::task::spawn_blocking(move || is_valid_image_sync(&path))
        .await
        .unwrap_or(false)
}

/// Resolve an image file on disk for a ROM, with arcade name translation.
/// Runs the `read_dir` + per-entry `metadata` scan on the blocking pool.
pub async fn resolve_image_on_disk(
    arcade_display: Option<String>,
    media_base: PathBuf,
    kind: &'static str,
    rom_filename: String,
) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        resolve_image_on_disk_sync(arcade_display.as_deref(), &media_base, kind, &rom_filename)
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("resolve_image_on_disk panicked: {e}");
        None
    })
}

/// Resolve a single libretro-thumbnails "fake symlink" entry (a tiny text
/// file with the target filename in it). Runs on the blocking pool.
pub async fn try_resolve_fake_symlink(path: PathBuf, parent_dir: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || try_resolve_fake_symlink_sync(&path, &parent_dir))
        .await
        .unwrap_or(None)
}

/// Build a list of ROM filenames for a system from the filesystem.
///
/// Only includes files whose extension matches the system's known ROM
/// extensions (plus `.m3u` which is always accepted). This prevents
/// non-ROM files (`.txt`, `.nfo`, `.jpg`, etc.) from triggering
/// thumbnail downloads.
pub fn list_rom_filenames(storage_root: &Path, system: &str) -> Vec<String> {
    let roms_dir = storage_root.join("roms").join(system);
    let extensions = replay_control_core::systems::find_system(system).map(|s| s.extensions);
    let mut filenames = Vec::new();
    collect_rom_filenames(&roms_dir, &mut filenames, extensions);
    filenames
}

fn collect_rom_filenames(dir: &Path, filenames: &mut Vec<String>, extensions: Option<&[&str]>) {
    let _ = fs_walk::for_each_file(dir, true, |entry, _, _| {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(exts) = extensions {
            let matches = name
                .rsplit_once('.')
                .map(|(_, ext)| {
                    let ext_lower = ext.to_lowercase();
                    ext_lower == "m3u" || exts.iter().any(|e| *e == ext_lower)
                })
                .unwrap_or(false);
            if !matches {
                return;
            }
        }
        filenames.push(name);
    });
}

/// Scan downloaded thumbnail media once and return materializable counters.
/// This is maintenance work; request-time handlers should read the persisted
/// values from `library.db` instead of calling this.
pub fn scan_media_stats(storage_root: &Path) -> Vec<ThumbnailMediaStats> {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    scan_media_stats_dir(&media_dir)
}

fn scan_media_stats_dir(media_dir: &Path) -> Vec<ThumbnailMediaStats> {
    let mut stats_by_system = Vec::new();
    let Ok(systems) = std::fs::read_dir(media_dir) else {
        return stats_by_system;
    };

    for system in systems.flatten() {
        let system_path = system.path();
        if !system_path.is_dir() {
            continue;
        }
        let mut stats = ThumbnailMediaStats {
            system: system.file_name().to_string_lossy().into_owned(),
            ..ThumbnailMediaStats::default()
        };
        for kind in ALL_THUMBNAIL_KINDS {
            scan_kind_media_dir(&system_path.join(kind.media_dir()), *kind, &mut stats);
        }
        if stats.file_count > 0 {
            stats_by_system.push(stats);
        }
    }
    stats_by_system.sort_by(|a, b| a.system.cmp(&b.system));
    stats_by_system
}

fn scan_kind_media_dir(kind_dir: &Path, kind: ThumbnailKind, stats: &mut ThumbnailMediaStats) {
    let Ok(files) = std::fs::read_dir(kind_dir) else {
        return;
    };
    for file in files.flatten() {
        let path = file.path();
        if !path.is_file() || !is_valid_image_sync(&path) {
            continue;
        }
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        stats.total_size_bytes = stats.total_size_bytes.saturating_add(size);
        stats.file_count += 1;
        match kind {
            ThumbnailKind::Boxart => stats.boxart_file_count += 1,
            ThumbnailKind::Snap => stats.snap_file_count += 1,
            ThumbnailKind::Title => stats.title_file_count += 1,
        }
    }
}

/// Delete media files for a single system.
pub fn clear_system_media(storage_root: &Path, system: &str) -> Result<()> {
    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system);
    if media_dir.exists() {
        std::fs::remove_dir_all(&media_dir).map_err(|e| Error::io(&media_dir, e))?;
    }
    Ok(())
}

/// Delete all media files for all systems.
pub fn clear_media(storage_root: &Path) -> Result<()> {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    if media_dir.exists() {
        std::fs::remove_dir_all(&media_dir).map_err(|e| Error::io(&media_dir, e))?;
    }
    Ok(())
}

fn thumbnail_relative_from_media_url(system: &str, url: &str) -> Option<String> {
    let prefix = format!("/media/{system}/");
    let relative = url.strip_prefix(&prefix)?;
    let (kind, filename) = relative.split_once('/')?;
    if filename.contains('/') || strip_image_ext(filename).is_none() {
        return None;
    }
    if ALL_THUMBNAIL_KINDS
        .iter()
        .any(|thumbnail_kind| thumbnail_kind.media_dir() == kind)
    {
        Some(format!("{kind}/{filename}"))
    } else {
        None
    }
}

fn thumbnail_path_from_relative(media_base: &Path, relative: &str) -> Option<PathBuf> {
    let (kind, filename) = relative.split_once('/')?;
    if filename.contains('/') || strip_image_ext(filename).is_none() {
        return None;
    }
    if ALL_THUMBNAIL_KINDS
        .iter()
        .any(|thumbnail_kind| thumbnail_kind.media_dir() == kind)
    {
        Some(media_base.join(kind).join(filename))
    } else {
        None
    }
}

/// The lookup keys a set of ROMs can match a cached thumbnail by — the same
/// tiers [`find_image_on_disk`] resolves through (exact stem, case-insensitive,
/// base-title, version-stripped, slash-split dual titles), collected across many
/// ROMs at once.
///
/// This is the inverse of resolving each ROM to a file: instead of scanning the
/// media directory once *per ROM* (which is O(roms × files) — it pinned the CPU
/// and starved the read pool on large libraries), we gather every ROM's keys
/// once, then scan the directory once and keep any file whose stem matches a
/// key. That is O(roms + files). A file is retained if it *could* be matched by
/// any ROM via any tier — a conservative superset of what per-ROM resolution
/// returns, so cleanup never deletes a thumbnail the runtime would still serve
/// (it may keep a few extra same-base-title files, which is the safe direction).
#[derive(Default)]
struct ThumbnailKeys {
    exact: HashSet<String>,
    ci: HashSet<String>,
    fuzzy: HashSet<String>,
}

impl ThumbnailKeys {
    /// Add the keys for one already-`thumbnail_filename`-normalized name, using
    /// the same tier definition the per-request resolver walks.
    fn add_thumb(&mut self, thumb_name: &str) {
        let keys = thumb_match_keys(thumb_name);
        self.exact.insert(keys.exact);
        self.ci.insert(keys.lower);
        self.fuzzy.insert(keys.base);
        if let Some(version) = keys.version {
            self.fuzzy.insert(version);
        }
        self.fuzzy.extend(keys.slash_parts);
    }

    /// Add the two lookups [`resolve_image_on_disk_sync`] makes for one ROM: the
    /// arcade display name (if any) and the ROM filename.
    fn add_entry(&mut self, rom_filename: &str, display_name: Option<&str>) {
        if let Some(display) = display_name {
            self.add_thumb(&thumbnail_filename(display));
        }
        let stem = strip_n64dd_prefix(filename_stem(rom_filename));
        self.add_thumb(&thumbnail_filename(stem));
    }

    /// Whether a file with this stem matches any collected key.
    fn matches(&self, stem: &str) -> bool {
        self.exact.contains(stem)
            || self.ci.contains(&stem.to_lowercase())
            || self.fuzzy.contains(base_title(stem).as_str())
    }
}

/// Scan each media kind directory once and return the relative paths of files
/// matching any of `keys`. Fake-symlink stubs that match resolve to their target
/// (mirrors [`find_image_on_disk`]).
fn referenced_files_matching(media_base: &Path, keys: &ThumbnailKeys) -> HashSet<String> {
    let mut referenced = HashSet::new();
    for kind in ALL_THUMBNAIL_KINDS {
        let kind_name = kind.media_dir();
        let kind_dir = media_base.join(kind_name);
        let Ok(entries) = std::fs::read_dir(&kind_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            let Some(stem) = strip_image_ext(&filename) else {
                continue;
            };
            if !keys.matches(stem) {
                continue;
            }
            if is_valid_image_sync(&path) {
                referenced.insert(format!("{kind_name}/{filename}"));
            } else if let Some(target) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                referenced.insert(format!("{kind_name}/{target}"));
            }
        }
    }
    referenced
}

fn referenced_thumbnail_paths_for_rom(
    media_base: &Path,
    system: &str,
    rom_filename: &str,
    display_name: Option<&str>,
    box_art_url: Option<&str>,
) -> HashSet<String> {
    let mut keys = ThumbnailKeys::default();
    keys.add_entry(rom_filename, display_name);
    let mut referenced = referenced_files_matching(media_base, &keys);

    if let Some(url) = box_art_url
        && let Some(relative) = thumbnail_relative_from_media_url(system, url)
    {
        referenced.insert(relative);
    }

    referenced
}

fn referenced_thumbnail_paths(
    media_base: &Path,
    system: &str,
    entries: &[GameEntry],
) -> HashSet<String> {
    let mut keys = ThumbnailKeys::default();
    let mut referenced = HashSet::new();
    for entry in entries {
        keys.add_entry(&entry.rom_filename, entry.display_name.as_deref());
        if let Some(url) = &entry.box_art_url
            && let Some(relative) = thumbnail_relative_from_media_url(system, url)
        {
            referenced.insert(relative);
        }
    }
    referenced.extend(referenced_files_matching(media_base, &keys));
    referenced
}

fn media_system_dirs(media_dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(systems) = std::fs::read_dir(media_dir) else {
        return Vec::new();
    };

    let mut result: Vec<(String, PathBuf)> = systems
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            Some((entry.file_name().to_string_lossy().into_owned(), path))
        })
        .collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

/// Return systems that have downloaded thumbnail media on disk.
pub fn media_system_names(storage_root: &Path) -> Vec<String> {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    media_system_dirs(&media_dir)
        .into_iter()
        .map(|(system, _)| system)
        .collect()
}

fn thumbnail_files(system_media: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    for kind in ALL_THUMBNAIL_KINDS {
        let kind_name = kind.media_dir();
        let kind_dir = system_media.join(kind_name);
        let Ok(entries) = std::fs::read_dir(&kind_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_valid_image_sync(&path) {
                continue;
            }
            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            if strip_image_ext(&filename).is_none() {
                continue;
            }
            files.push((format!("{kind_name}/{filename}"), path));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Return cached thumbnail files for a deleted ROM that are no longer used by
/// any remaining ROM in the same system.
pub fn orphaned_thumbnail_files_for_deleted_rom(
    storage_root: &Path,
    system: &str,
    rom_filename: &str,
    display_name: Option<&str>,
    box_art_url: Option<&str>,
    active_entries: &[GameEntry],
) -> Vec<(String, PathBuf)> {
    let media_base = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system);
    let candidates = referenced_thumbnail_paths_for_rom(
        &media_base,
        system,
        rom_filename,
        display_name,
        box_art_url,
    );
    if candidates.is_empty() {
        return Vec::new();
    }

    let retained = referenced_thumbnail_paths(&media_base, system, active_entries);
    let mut orphans: Vec<(String, PathBuf)> = candidates
        .into_iter()
        .filter(|relative| !retained.contains(relative))
        .filter_map(|relative| {
            thumbnail_path_from_relative(&media_base, &relative)
                .filter(|path| path.is_file())
                .map(|path| (system.to_string(), path))
        })
        .collect();
    orphans.sort_by(|a, b| a.1.cmp(&b.1));
    orphans
}

/// Find orphaned thumbnail files that are not referenced by any active ROM.
///
/// A cached thumbnail is considered active when it is the `box_art_url` for a
/// library row or when the normal thumbnail resolver would choose it for that
/// ROM as box art, screenshot, or title image. This mirrors the UI lookup
/// behavior instead of relying on the box-art URL column alone.
///
/// Returns a list of `(system, file_path)` pairs for each orphaned image.
pub fn find_orphaned_thumbnails_from_entries(
    storage_root: &Path,
    entries_by_system: &[(String, Vec<GameEntry>)],
) -> Vec<(String, PathBuf)> {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    if !media_dir.exists() {
        return Vec::new();
    }

    let mut orphans = Vec::new();

    for (system, entries) in entries_by_system {
        let system_media = media_dir.join(system);
        if !system_media.is_dir() {
            continue;
        }
        let referenced = referenced_thumbnail_paths(&system_media, system, entries);
        orphans.extend(
            thumbnail_files(&system_media)
                .into_iter()
                .filter(|(relative, _)| !referenced.contains(relative))
                .map(|(_, path)| (system.clone(), path)),
        );
    }

    orphans
}

/// Find orphaned thumbnail files that are not referenced by any active ROM.
pub fn find_orphaned_thumbnails(
    storage_root: &Path,
    conn: &rusqlite::Connection,
) -> Result<Vec<(String, PathBuf)>> {
    let entries_by_system = media_system_names(storage_root)
        .into_iter()
        .map(|system| {
            let entries = LibraryDb::load_system_entries(conn, &system)?;
            Ok((system, entries))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(find_orphaned_thumbnails_from_entries(
        storage_root,
        &entries_by_system,
    ))
}

/// Delete orphaned thumbnail files and return `(count_deleted, bytes_freed)`.
///
/// Uses [`find_orphaned_thumbnails`] to identify files, then deletes each one.
pub fn delete_orphaned_thumbnails(
    storage_root: &Path,
    conn: &rusqlite::Connection,
) -> Result<(usize, u64)> {
    let orphans = find_orphaned_thumbnails(storage_root, conn)?;
    Ok(delete_thumbnail_files(&orphans))
}

/// Delete thumbnail files previously returned by [`find_orphaned_thumbnails`].
pub fn delete_thumbnail_files(orphans: &[(String, PathBuf)]) -> (usize, u64) {
    let mut deleted = 0usize;
    let mut bytes_freed = 0u64;

    for (_system, path) in orphans {
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        match std::fs::remove_file(path) {
            Ok(()) => {
                deleted += 1;
                bytes_freed += size;
            }
            Err(e) => {
                tracing::debug!(
                    "Failed to delete orphaned thumbnail {}: {e}",
                    path.display()
                );
            }
        }
    }

    (deleted, bytes_freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- thumbnail_filename ---

    #[test]
    fn thumbnail_filename_replaces_special_chars() {
        assert_eq!(thumbnail_filename("Game: The Sequel"), "Game_ The Sequel");
        assert_eq!(thumbnail_filename("A & B"), "A _ B");
        assert_eq!(thumbnail_filename("What?"), "What_");
        assert_eq!(thumbnail_filename("A/B"), "A_B");
        assert_eq!(thumbnail_filename("A*B"), "A_B");
        assert_eq!(thumbnail_filename("A<B>C"), "A_B_C");
        assert_eq!(thumbnail_filename("A|B"), "A_B");
        assert_eq!(thumbnail_filename("A\\B"), "A_B");
        assert_eq!(thumbnail_filename("A`B"), "A_B");
        assert_eq!(thumbnail_filename("A\"B"), "A_B");
    }

    #[test]
    fn thumbnail_filename_preserves_normal_chars() {
        assert_eq!(thumbnail_filename("Super Mario World"), "Super Mario World");
        assert_eq!(
            thumbnail_filename("Sonic The Hedgehog (USA)"),
            "Sonic The Hedgehog (USA)"
        );
    }

    #[test]
    fn thumbnail_filename_empty_string() {
        assert_eq!(thumbnail_filename(""), "");
    }

    #[test]
    fn thumbnail_filename_all_special() {
        assert_eq!(thumbnail_filename("&*/:"), "____");
    }

    #[test]
    fn thumbnail_filename_multiple_colons() {
        assert_eq!(thumbnail_filename("Title: Sub: Part"), "Title_ Sub_ Part");
    }

    #[test]
    fn scan_media_stats_counts_valid_media_by_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let system_dir = tmp
            .path()
            .join(crate::storage::RC_DIR)
            .join("media")
            .join("snes");
        let boxart_dir = system_dir.join(ThumbnailKind::Boxart.media_dir());
        let snap_dir = system_dir.join(ThumbnailKind::Snap.media_dir());
        let title_dir = system_dir.join(ThumbnailKind::Title.media_dir());
        std::fs::create_dir_all(&boxart_dir).unwrap();
        std::fs::create_dir_all(&snap_dir).unwrap();
        std::fs::create_dir_all(&title_dir).unwrap();
        std::fs::write(boxart_dir.join("A.png"), vec![1u8; 201]).unwrap();
        std::fs::write(snap_dir.join("B.jpg"), vec![2u8; 250]).unwrap();
        std::fs::write(title_dir.join("fake.png"), vec![3u8; 50]).unwrap();

        let stats = scan_media_stats(tmp.path());
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].system, "snes");
        assert_eq!(stats[0].file_count, 2);
        assert_eq!(stats[0].boxart_file_count, 1);
        assert_eq!(stats[0].snap_file_count, 1);
        assert_eq!(stats[0].title_file_count, 0);
        assert_eq!(stats[0].total_size_bytes, 451);
    }

    fn media_file(
        storage_root: &Path,
        system: &str,
        kind: ThumbnailKind,
        filename: &str,
    ) -> PathBuf {
        storage_root
            .join(crate::storage::RC_DIR)
            .join("media")
            .join(system)
            .join(kind.media_dir())
            .join(filename)
    }

    fn write_test_image(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![1u8; 201]).unwrap();
    }

    fn test_entry(system: &str, filename: &str) -> GameEntry {
        GameEntry {
            system: system.to_string(),
            rom_filename: filename.to_string(),
            rom_path: format!("/roms/{system}/{filename}"),
            ..GameEntry::default()
        }
    }

    #[test]
    fn find_orphaned_thumbnails_scans_boxart_snap_and_title() {
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let mut conn = LibraryDb::open(db_dir.path()).unwrap();

        for kind in ALL_THUMBNAIL_KINDS {
            write_test_image(&media_file(storage.path(), "snes", *kind, "Kept.png"));
            write_test_image(&media_file(storage.path(), "snes", *kind, "Orphan.png"));
        }

        let mut kept = test_entry("snes", "Kept.sfc");
        kept.box_art_url = Some("/media/snes/boxart/Kept.png".to_string());
        LibraryDb::save_system_entries(&mut conn, "snes", &[kept], None).unwrap();

        let mut orphan_paths: Vec<String> = find_orphaned_thumbnails(storage.path(), &conn)
            .unwrap()
            .into_iter()
            .map(|(_, path)| {
                path.strip_prefix(storage.path())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        orphan_paths.sort();

        assert_eq!(
            orphan_paths,
            vec![
                ".replay-control/media/snes/boxart/Orphan.png",
                ".replay-control/media/snes/snap/Orphan.png",
                ".replay-control/media/snes/title/Orphan.png",
            ]
        );
    }

    #[test]
    fn find_orphaned_thumbnails_includes_system_media_with_no_rows() {
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let conn = LibraryDb::open(db_dir.path()).unwrap();

        for kind in ALL_THUMBNAIL_KINDS {
            write_test_image(&media_file(storage.path(), "snes", *kind, "Removed.png"));
        }

        let orphans = find_orphaned_thumbnails(storage.path(), &conn).unwrap();
        assert_eq!(orphans.len(), 3);
    }

    #[test]
    fn find_orphaned_thumbnails_keeps_fake_symlink_targets_used_by_active_roms() {
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let mut conn = LibraryDb::open(db_dir.path()).unwrap();
        let boxart_dir = storage
            .path()
            .join(crate::storage::RC_DIR)
            .join("media")
            .join("snes")
            .join(ThumbnailKind::Boxart.media_dir());
        std::fs::create_dir_all(&boxart_dir).unwrap();
        std::fs::write(boxart_dir.join("Kept.png"), b"Shared.png").unwrap();
        write_test_image(&boxart_dir.join("Shared.png"));
        write_test_image(&boxart_dir.join("Orphan.png"));

        let kept = test_entry("snes", "Kept.sfc");
        LibraryDb::save_system_entries(&mut conn, "snes", &[kept], None).unwrap();

        let orphans = find_orphaned_thumbnails(storage.path(), &conn).unwrap();

        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].1.file_name().unwrap(), "Orphan.png");
    }

    #[test]
    fn find_orphaned_thumbnails_keeps_base_title_match_for_tagged_rom() {
        // A region-tagged ROM ("Sonic (USA).md") must retain the base-named
        // thumbnail ("Sonic.png") it resolves to via the base_title tier — the
        // O(roms+files) key-set has to cover the fuzzy tiers, not just exact.
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let mut conn = LibraryDb::open(db_dir.path()).unwrap();
        for kind in ALL_THUMBNAIL_KINDS {
            write_test_image(&media_file(storage.path(), "sega_smd", *kind, "Sonic.png"));
            write_test_image(&media_file(
                storage.path(),
                "sega_smd",
                *kind,
                "Unrelated.png",
            ));
        }
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[test_entry("sega_smd", "Sonic (USA).md")],
            None,
        )
        .unwrap();

        let orphans = find_orphaned_thumbnails(storage.path(), &conn).unwrap();

        // Sonic.png retained via base_title; only Unrelated.png (×3 kinds) orphaned.
        assert_eq!(orphans.len(), 3);
        assert!(
            orphans
                .iter()
                .all(|(_, p)| p.file_name().unwrap() == "Unrelated.png")
        );
    }

    #[test]
    fn deleted_rom_thumbnail_cleanup_keeps_shared_variant_media() {
        let storage = tempfile::tempdir().unwrap();
        for kind in ALL_THUMBNAIL_KINDS {
            write_test_image(&media_file(storage.path(), "snes", *kind, "Game.png"));
        }

        let active_entries = vec![test_entry("snes", "Game (Europe).sfc")];
        let orphans = orphaned_thumbnail_files_for_deleted_rom(
            storage.path(),
            "snes",
            "Game (USA).sfc",
            None,
            None,
            &active_entries,
        );

        assert!(orphans.is_empty());
    }

    #[test]
    fn deleted_rom_thumbnail_cleanup_returns_unique_media() {
        let storage = tempfile::tempdir().unwrap();
        for kind in ALL_THUMBNAIL_KINDS {
            write_test_image(&media_file(storage.path(), "snes", *kind, "Deleted.png"));
            write_test_image(&media_file(storage.path(), "snes", *kind, "Other.png"));
        }

        let active_entries = vec![test_entry("snes", "Other.sfc")];
        let mut orphan_paths: Vec<String> = orphaned_thumbnail_files_for_deleted_rom(
            storage.path(),
            "snes",
            "Deleted.sfc",
            None,
            None,
            &active_entries,
        )
        .into_iter()
        .map(|(_, path)| {
            path.strip_prefix(storage.path())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
        orphan_paths.sort();

        assert_eq!(
            orphan_paths,
            vec![
                ".replay-control/media/snes/boxart/Deleted.png",
                ".replay-control/media/snes/snap/Deleted.png",
                ".replay-control/media/snes/title/Deleted.png",
            ]
        );
    }

    // --- strip_tags ---

    #[test]
    fn strip_tags_removes_parenthesized() {
        assert_eq!(strip_tags("Game Name (USA)"), "Game Name");
        assert_eq!(strip_tags("Indiana Jones (Spanish)"), "Indiana Jones");
    }

    #[test]
    fn strip_tags_removes_bracketed() {
        assert_eq!(strip_tags("Game Name [!]"), "Game Name");
    }

    #[test]
    fn strip_tags_no_tags() {
        assert_eq!(strip_tags("Dark Seed"), "Dark Seed");
    }

    #[test]
    fn strip_tags_empty_string() {
        assert_eq!(strip_tags(""), "");
    }

    #[test]
    fn strip_tags_strips_from_first_tag() {
        assert_eq!(strip_tags("Game (USA) (Rev 1)"), "Game");
    }

    #[test]
    fn strip_tags_trims_whitespace() {
        assert_eq!(strip_tags("Game  (USA)"), "Game");
    }

    #[test]
    fn strip_tags_paren_no_space_before() {
        assert_eq!(strip_tags("Game(USA)"), "Game(USA)");
    }

    // --- strip_version ---

    #[test]
    fn strip_version_standard() {
        assert_eq!(
            strip_version("Sonic Adventure 2 v1.008"),
            "Sonic Adventure 2"
        );
    }

    #[test]
    fn strip_version_space_separated() {
        assert_eq!(strip_version("Sega Rally 2 v1 001"), "Sega Rally 2");
    }

    #[test]
    fn strip_version_simple() {
        assert_eq!(strip_version("Game v2"), "Game");
    }

    #[test]
    fn strip_version_with_dots() {
        assert_eq!(strip_version("Game v1.2.3"), "Game");
    }

    #[test]
    fn strip_version_no_version() {
        assert_eq!(strip_version("Super Mario World"), "Super Mario World");
    }

    #[test]
    fn strip_version_empty() {
        assert_eq!(strip_version(""), "");
    }

    #[test]
    fn strip_version_v_without_digit() {
        assert_eq!(strip_version("Game vs Evil"), "Game vs Evil");
    }

    #[test]
    fn strip_version_v_in_middle_of_word() {
        assert_eq!(strip_version("Marvel"), "Marvel");
    }

    #[test]
    fn strip_version_non_version_text_after() {
        assert_eq!(
            strip_version("Game v2 Special Edition"),
            "Game v2 Special Edition"
        );
    }

    #[test]
    fn strip_version_underscore_separated() {
        assert_eq!(strip_version("Game v1_003"), "Game");
    }

    // --- base_title ---

    #[test]
    fn base_title_reorders_trailing_the() {
        assert_eq!(base_title("Legend of Zelda, The"), "the legend of zelda");
    }

    #[test]
    fn base_title_reorders_trailing_a() {
        assert_eq!(base_title("Legend of Zelda, A"), "a legend of zelda");
    }

    #[test]
    fn base_title_reorders_trailing_an() {
        assert_eq!(base_title("NHL 95, An"), "an nhl 95");
    }

    #[test]
    fn base_title_no_article_unchanged() {
        assert_eq!(base_title("Super Mario World"), "super mario world");
    }

    #[test]
    fn base_title_short_name_no_article() {
        assert_eq!(base_title("Contra"), "contra");
    }

    #[test]
    fn base_title_does_not_false_match_article_suffix() {
        assert_eq!(base_title("America"), "america");
    }

    // --- ThumbnailKind ---

    #[test]
    fn thumbnail_kind_repo_dir() {
        assert_eq!(ThumbnailKind::Boxart.repo_dir(), "Named_Boxarts");
        assert_eq!(ThumbnailKind::Snap.repo_dir(), "Named_Snaps");
        assert_eq!(ThumbnailKind::Title.repo_dir(), "Named_Titles");
    }

    #[test]
    fn thumbnail_kind_media_dir() {
        assert_eq!(ThumbnailKind::Boxart.media_dir(), "boxart");
        assert_eq!(ThumbnailKind::Snap.media_dir(), "snap");
        assert_eq!(ThumbnailKind::Title.media_dir(), "title");
    }

    // --- Arcade image matching pipeline ---
    //
    // These tests verify the full ROM-name → thumbnail-filename pipeline
    // for arcade systems, where the ROM zip name (e.g., "avsp") differs
    // from the image filename (e.g., "Alien vs. Predator (Europe 940520).png").
    //
    // The resolution pipeline is:
    //   1. Strip extension from ROM filename → stem
    //   2. Look up arcade_db for display name
    //   3. Apply thumbnail_filename() normalization
    //   4. Try exact match, colon variants, fuzzy (strip_tags), version-stripped

    /// Helper: simulate the full matching pipeline used by resolve_box_art_with_hash.
    /// Returns (thumb_name, base_title) for a given ROM filename + system.
    async fn resolve_pipeline(rom_filename: &str, system: &str) -> (String, String) {
        let stem = filename_stem(rom_filename);
        let is_arcade = replay_control_core::systems::is_arcade_system(system);
        let display_name = if is_arcade {
            crate::arcade_db::lookup_arcade_game(system, stem)
                .await
                .map(|info| info.display_name)
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        let thumb_name = thumbnail_filename(display_name.as_deref().unwrap_or(stem));
        let base = strip_tags(&thumb_name).trim().to_lowercase();
        (thumb_name, base)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_avsp_resolves_to_alien_vs_predator() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        let (thumb_name, base) = resolve_pipeline("avsp.zip", "arcade_fbneo").await;
        assert!(
            thumb_name.starts_with("Alien vs. Predator"),
            "expected 'Alien vs. Predator...', got '{thumb_name}'"
        );
        assert_eq!(base, "alien vs. predator");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_ffight_resolves_to_final_fight() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        let (thumb_name, base) = resolve_pipeline("ffight.zip", "arcade_fbneo").await;
        assert!(
            thumb_name.starts_with("Final Fight"),
            "expected 'Final Fight...', got '{thumb_name}'"
        );
        assert_eq!(base, "final fight");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_dsmbl_resolves_to_deathsmiles() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        let (thumb_name, base) = resolve_pipeline("dsmbl.zip", "arcade_fbneo").await;
        assert!(
            thumb_name.starts_with("Deathsmiles MegaBlack Label"),
            "expected 'Deathsmiles MegaBlack Label...', got '{thumb_name}'"
        );
        assert_eq!(base, "deathsmiles megablack label");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_dmnfrnt_resolves_with_slash_replaced() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        let (thumb_name, _) = resolve_pipeline("dmnfrnt.zip", "arcade_fbneo").await;
        // Display name is "Demon Front / Moyu Zhanxian ..."
        // thumbnail_filename replaces '/' with '_'
        assert!(
            thumb_name.contains("Demon Front _ Moyu Zhanxian"),
            "expected slash replaced with underscore, got '{thumb_name}'"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_sf2_colon_in_display_name() {
        crate::catalog_pool::init_test_catalog().await;
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_fbneo").await;
        // "Street Fighter II: The World Warrior (...)"
        // thumbnail_filename replaces ':' with '_'
        assert!(
            thumb_name.contains("Street Fighter II_ The World Warrior"),
            "expected colon replaced with underscore, got '{thumb_name}'"
        );

        // Colon variant: ": " → " - "
        let info = crate::arcade_db::lookup_arcade_game("arcade_fbneo", "sf2")
            .await
            .unwrap();
        let dash_variant =
            thumbnail_filename(&info.display_name.replace(": ", " - ").replace(':', " -"));
        assert!(
            dash_variant.contains("Street Fighter II - The World Warrior"),
            "expected dash variant, got '{dash_variant}'"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_unknown_rom_falls_back_to_stem() {
        let (thumb_name, base) = resolve_pipeline("zzz_nonexistent.zip", "arcade_fbneo").await;
        assert_eq!(thumb_name, "zzz_nonexistent");
        assert_eq!(base, "zzz_nonexistent");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn non_arcade_system_no_arcade_db_lookup() {
        // For non-arcade systems, the stem is used directly
        let (thumb_name, base) =
            resolve_pipeline("Super Mario World (USA).sfc", "nintendo_snes").await;
        assert_eq!(thumb_name, "Super Mario World (USA)");
        assert_eq!(base, "super mario world");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_mame_system_also_uses_arcade_db() {
        crate::catalog_pool::init_test_catalog().await;
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_mame").await;
        assert!(
            thumb_name.contains("Street Fighter II"),
            "arcade_mame should also use arcade_db, got '{thumb_name}'"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_dc_system_uses_arcade_db() {
        crate::catalog_pool::init_test_catalog().await;
        let (thumb_name, _) = resolve_pipeline("ikaruga.zip", "arcade_dc").await;
        assert!(
            thumb_name.starts_with("Ikaruga"),
            "arcade_dc should use arcade_db, got '{thumb_name}'"
        );
    }

    #[test]
    fn n64dd_prefix_stripped() {
        // N64DD ROMs have a "N64DD - " prefix that should be stripped
        let stem = "N64DD - Mario Artist Paint Studio (Japan).n64";
        let stem = stem.rfind('.').map(|i| &stem[..i]).unwrap_or(stem);
        let stem = strip_n64dd_prefix(stem);
        assert_eq!(stem, "Mario Artist Paint Studio (Japan)");
        let thumb = thumbnail_filename(stem);
        assert_eq!(thumb, "Mario Artist Paint Studio (Japan)");
    }

    #[test]
    fn fuzzy_match_strips_region_tags() {
        // "Alien vs. Predator (Europe 940520)" and "(USA 940520)" should
        // both fuzzy-match to the same base title
        let base_eu = strip_tags("Alien vs. Predator (Europe 940520)")
            .trim()
            .to_lowercase();
        let base_us = strip_tags("Alien vs. Predator (USA 940520)")
            .trim()
            .to_lowercase();
        assert_eq!(base_eu, base_us);
        assert_eq!(base_eu, "alien vs. predator");
    }

    #[test]
    fn version_stripped_match_for_dreamcast_style() {
        // Dreamcast GDI ROMs often have version strings
        let base = strip_tags("Sonic Adventure 2 (USA)").to_lowercase();
        let base_v = strip_tags("Sonic Adventure 2 v1.008 (USA)").to_lowercase();
        // strip_tags removes from first paren, so both → "sonic adventure 2" / "sonic adventure 2 v1.008"
        let v_stripped = strip_version(&base_v);
        assert_eq!(v_stripped, "sonic adventure 2");
        assert_eq!(strip_version(&base), "sonic adventure 2");
    }

    // --- list_rom_filenames extension filtering ---

    #[test]
    fn resolve_image_on_disk_finds_apostrophe_arcade_title() {
        // Regression: arcade ROM galaga88.zip has display "Galaga '88 (set 1)" (MAME 2003+),
        // libretro saves art as "Galaga '88.png". resolve_image_on_disk must find it via
        // the strip_tags fuzzy tier — apostrophes are preserved in both the lookup key
        // and the on-disk filename.
        let tmp = tempfile::tempdir().unwrap();
        let boxart_dir = tmp.path().join("boxart");
        std::fs::create_dir_all(&boxart_dir).unwrap();
        let file = boxart_dir.join("Galaga '88.png");
        std::fs::write(&file, vec![0u8; 1024]).unwrap();

        // Parent: MAME 2003+ display.
        let result = resolve_image_on_disk_sync(
            Some("Galaga '88 (set 1)"),
            tmp.path(),
            "boxart",
            "galaga88.zip",
        );
        assert_eq!(result.as_deref(), Some("boxart/Galaga '88.png"));

        // Clone: FBNeo display.
        let result = resolve_image_on_disk_sync(
            Some("Galaga '88 (02-03-88)"),
            tmp.path(),
            "boxart",
            "galaga88a.zip",
        );
        assert_eq!(result.as_deref(), Some("boxart/Galaga '88.png"));
    }

    #[test]
    fn list_rom_filenames_filters_unsupported_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        let roms_dir = tmp.path().join("roms").join("amstrad_cpc");
        std::fs::create_dir_all(&roms_dir).unwrap();

        // .dsk is a valid Amstrad CPC extension
        std::fs::write(roms_dir.join("Game.dsk"), b"rom").unwrap();
        // .m3u is always accepted
        std::fs::write(roms_dir.join("Playlist.m3u"), b"list").unwrap();
        // .txt and .nfo are not valid ROM extensions
        std::fs::write(roms_dir.join("readme.txt"), b"text").unwrap();
        std::fs::write(roms_dir.join("info.nfo"), b"nfo").unwrap();
        // .jpg is not a ROM extension
        std::fs::write(roms_dir.join("cover.jpg"), b"img").unwrap();

        let mut filenames = list_rom_filenames(tmp.path(), "amstrad_cpc");
        filenames.sort();

        assert_eq!(filenames, vec!["Game.dsk", "Playlist.m3u"]);
    }

    #[test]
    fn list_rom_filenames_unknown_system_returns_all() {
        // For an unknown system (no entry in SYSTEMS), all files are returned.
        let tmp = tempfile::tempdir().unwrap();
        let roms_dir = tmp.path().join("roms").join("unknown_sys");
        std::fs::create_dir_all(&roms_dir).unwrap();

        std::fs::write(roms_dir.join("game.rom"), b"data").unwrap();
        std::fs::write(roms_dir.join("readme.txt"), b"text").unwrap();

        let mut filenames = list_rom_filenames(tmp.path(), "unknown_sys");
        filenames.sort();

        assert_eq!(filenames, vec!["game.rom", "readme.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn list_rom_filenames_does_not_follow_directory_symlink_cycles() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let roms_dir = tmp.path().join("roms").join("amstrad_cpc");
        std::fs::create_dir_all(&roms_dir).unwrap();
        std::fs::write(roms_dir.join("Game.dsk"), b"rom").unwrap();
        symlink(&roms_dir, roms_dir.join("loop")).unwrap();

        let filenames = list_rom_filenames(tmp.path(), "amstrad_cpc");

        assert_eq!(filenames, vec!["Game.dsk"]);
    }
}
