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
use replay_control_core::systems::{find_system, is_arcade_system};

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

/// Inverse of [`percent_encode_uri_segment`]: decode `%XX` escapes back to the
/// original bytes, then interpret as UTF-8. A `%` not followed by two hex
/// digits is left literal. Used to match a stored (percent-encoded)
/// `box_art_url` filename against the decoded names on disk.
pub(crate) fn percent_decode_uri_segment(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

/// `<storage_root>/.replay-control/media` — root of all downloaded thumbnail
/// media. Central definition of the media layout; every non-test path below
/// derives from these helpers instead of re-spelling the joins.
pub fn media_root(storage_root: &Path) -> PathBuf {
    storage_root.join(crate::storage::RC_DIR).join("media")
}

/// `<storage_root>/.replay-control/media/<system>`
pub fn system_media_dir(storage_root: &Path, system: &str) -> PathBuf {
    media_root(storage_root).join(system)
}

/// `<storage_root>/.replay-control/media/<system>/<kind dir>`
pub fn media_kind_dir(storage_root: &Path, system: &str, kind: ThumbnailKind) -> PathBuf {
    system_media_dir(storage_root, system).join(kind.media_dir())
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

/// Supported thumbnail image extensions, in resolution-priority order: a stem
/// present under more than one extension resolves to the first listed. `.png`
/// wins over `.jpg` — it is the native download format and the extension the
/// on-disk fast path (`exact_image_on_disk`) probes, so the exact tier's
/// tie-break and the fast path agree. Extending image support means adding here.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg"];

/// Strip a supported image extension from a filename, returning the stem.
pub fn strip_image_ext(name: &str) -> Option<&str> {
    IMAGE_EXTENSIONS
        .iter()
        .find_map(|ext| name.strip_suffix(*ext))
}

/// Quick check that a file is likely a real image (not a git fake-symlink text
/// file), by size. Follows symlinks (uses `metadata`); for classifying a
/// directory entry as a servable image use [`is_real_image_file`], which does
/// not, so serve and the scanner agree.
pub(crate) fn is_valid_image_sync(path: &Path) -> bool {
    path.metadata().map(|m| m.len() >= 200).unwrap_or(false)
}

/// True when `path` is a real, non-symlink regular file holding image bytes.
///
/// Uses `symlink_metadata`, so an OS symlink is rejected here just as the
/// directory scanner rejects it (`DirEntry::file_type` does not follow links).
/// This is the shared predicate the scanner ([`matching::scan_dir`]) and the
/// on-disk fast path use to decide a directory entry is a servable image — as
/// opposed to a libretro fake-symlink stub (a tiny text file) or an OS symlink —
/// so runtime serve and orphan cleanup classify every file identically.
pub(crate) fn is_real_image_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.is_file() && meta.len() >= 200)
        .unwrap_or(false)
}

/// Try to resolve a small file as a git fake-symlink artifact.
/// Reads its text content (a relative filename), checks that it names a supported
/// image that exists in `parent_dir` and passes [`is_valid_image_sync`]. Returns
/// the target filename on success, `None` otherwise.
pub(crate) fn try_resolve_fake_symlink_sync(path: &Path, parent_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let target_name = std::str::from_utf8(&bytes).ok()?.trim();
    // Accept any supported image extension as a target, not just .png — the
    // resolver, cleanup, and this stub check must agree on what counts as an
    // image (see IMAGE_EXTENSIONS), or a stub's .jpg/.webp target would go
    // unreferenced and be pruned.
    strip_image_ext(target_name)?;
    let target_path = parent_dir.join(target_name);
    if target_path.exists() && is_valid_image_sync(&target_path) {
        Some(target_name.to_string())
    } else {
        None
    }
}

/// Resolve an image file on disk for a ROM, with arcade name translation.
///
/// The single per-file resolution entry point, used by both runtime serving and
/// (through the same [`matching::resolve_thumbnail`]) orphan cleanup, so a file
/// cleanup would delete is exactly a file serving could never show. Builds the
/// shared symlink-aware directory index once, then resolves through the one
/// multi-tier matcher. For arcade systems the caller passes `arcade_display`
/// (the MAME display name, e.g. `Golden Axe: The Revenge of Death Adder`), tried
/// first, then the ROM filename stem; for non-arcade systems, pass `None`.
pub(crate) fn resolve_image_on_disk_sync(
    arcade_display: Option<&str>,
    media_base: &Path,
    kind: &str,
    rom_filename: &str,
) -> Option<String> {
    let kind_dir = media_base.join(kind);
    if !kind_dir.exists() {
        return None;
    }
    // Fast path: an exact `{thumbnail_filename}.png` match — art named after the
    // arcade display or the ROM stem — is the overwhelmingly common case. Check
    // it directly (one stat) before the fuzzy resolver, which builds a full
    // directory index (read_dir + stat every file). On a large media dir
    // (thousands of files) on NFS that scan costs hundreds of ms per lookup, and
    // the game-detail page does two per view (snap + title).
    if let Some(hit) = exact_image_on_disk(&kind_dir, kind, arcade_display, rom_filename) {
        return Some(hit);
    }
    let index = matching::build_dir_index_with_symlinks(&kind_dir, kind);
    matching::resolve_thumbnail(&index, rom_filename, arcade_display)
}

/// Fast-path the exact match for a ROM: probe `{thumbnail_filename}.<ext>` on
/// disk for the single top-priority key (the arcade display-exact key, or the
/// stem-exact key for non-arcade — see [`matching::exact_thumbnail_key`]). Using
/// only that key is what keeps the fast path resolver-equivalent: it can never
/// jump a stem match ahead of a higher-priority display-name (colon/fuzzy) match.
///
/// Extensions are probed in [`IMAGE_EXTENSIONS`] priority order and the first
/// real, valid file wins — which is exactly the exact tier's pick, because that
/// tier prefers a self-named `{key}.<ext>` file by the same extension order (see
/// `matching::exact_pref`). Deriving the probe order from `IMAGE_EXTENSIONS`
/// rather than hard-coding `.png` means a new supported format is picked up here
/// automatically and the fast path can't drift from the tier's preference.
///
/// A `.jpg`/other-only stem still fast-paths (a later probe hits); only a
/// fake-symlink stub, an OS symlink, or a genuine lower-tier (fuzzy) match misses
/// every probe (via [`is_real_image_file`], which the scanner uses too) and falls
/// through to the full scan, which resolves it. Returns `None` on a miss.
fn exact_image_on_disk(
    kind_dir: &Path,
    kind: &str,
    arcade_display: Option<&str>,
    rom_filename: &str,
) -> Option<String> {
    let thumb = matching::exact_thumbnail_key(rom_filename, arcade_display);
    IMAGE_EXTENSIONS.iter().find_map(|ext| {
        let candidate = kind_dir.join(format!("{thumb}{ext}"));
        is_real_image_file(&candidate).then(|| format!("{kind}/{thumb}{ext}"))
    })
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

/// Build a list of ROM filenames for a system from the filesystem.
///
/// Only includes files whose extension matches the system's known ROM
/// extensions (plus `.m3u` which is always accepted). This prevents
/// non-ROM files (`.txt`, `.nfo`, `.jpg`, etc.) from triggering
/// thumbnail downloads.
pub fn list_rom_filenames(storage_root: &Path, system: &str) -> Vec<String> {
    let roms_dir = storage_root.join("roms").join(system);
    let extensions = find_system(system).map(|s| s.extensions);
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
    let media_dir = media_root(storage_root);
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

/// Delete all media files for all systems.
pub fn clear_media(storage_root: &Path) -> Result<()> {
    let media_dir = media_root(storage_root);
    if media_dir.exists() {
        std::fs::remove_dir_all(&media_dir).map_err(|e| Error::io(&media_dir, e))?;
    }
    Ok(())
}

fn thumbnail_relative_from_media_url(system: &str, url: &str) -> Option<String> {
    let prefix = format!("/media/{system}/");
    let relative = url.strip_prefix(&prefix)?;
    let (kind, filename) = relative.split_once('/')?;
    // box_art_url is percent-encoded per segment (format_box_art_url); decode so
    // the filename matches the decoded names on disk (thumbnail_files). Without
    // this, any box art whose filename has spaces/apostrophes/parens (i.e. most
    // arcade names) never matched its URL reference and was deleted as orphan.
    let filename = percent_decode_uri_segment(filename);
    if filename.contains('/') || strip_image_ext(&filename).is_none() {
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

/// Arcade display name to feed the resolver for one entry: `Some` only for
/// arcade systems (where art is filed under the MAME display name), mirroring
/// exactly what the runtime serve path passes. `None` for non-arcade, so cleanup
/// resolves by the ROM stem just as serving does.
fn cleanup_arcade_display(is_arcade: bool, display_name: Option<&str>) -> Option<&str> {
    is_arcade.then_some(display_name).flatten()
}

/// The set of on-disk media files the runtime could serve for `entries` — the
/// referenced set for orphan cleanup. Built with the **same** resolver runtime
/// serving uses ([`matching::resolve_thumbnail`] over the shared
/// [`matching::build_dir_index_with_symlinks`]), so a file absent from this set
/// is exactly a file serving could never show — cleanup and serving cannot
/// disagree. Each kind directory is scanned once (O(roms + files)).
fn thumbnail_dir_scans(media_base: &Path) -> Vec<(&'static str, matching::DirScan)> {
    ALL_THUMBNAIL_KINDS
        .iter()
        .map(|kind| {
            let kind_name = kind.media_dir();
            let scan = matching::scan_dir_with_symlinks(&media_base.join(kind_name), kind_name);
            (kind_name, scan)
        })
        .collect()
}

fn referenced_files_for_entries_from_scans(
    system: &str,
    entries: &[GameEntry],
    scans: &[(&'static str, matching::DirScan)],
) -> HashSet<String> {
    let is_arcade = is_arcade_system(system);
    let mut referenced = HashSet::new();
    for (_, scan) in scans {
        for entry in entries {
            let arcade_display = cleanup_arcade_display(is_arcade, entry.display_name.as_deref());
            if let Some(relative) =
                matching::resolve_thumbnail(&scan.index, &entry.rom_filename, arcade_display)
            {
                referenced.insert(relative);
            }
        }
    }
    referenced
}

fn referenced_thumbnail_paths_for_rom_from_scans(
    system: &str,
    rom_filename: &str,
    display_name: Option<&str>,
    box_art_url: Option<&str>,
    scans: &[(&'static str, matching::DirScan)],
) -> HashSet<String> {
    let arcade_display = cleanup_arcade_display(is_arcade_system(system), display_name);
    let mut referenced = HashSet::new();
    for (_, scan) in scans {
        if let Some(relative) =
            matching::resolve_thumbnail(&scan.index, rom_filename, arcade_display)
        {
            referenced.insert(relative);
        }
    }

    if let Some(url) = box_art_url
        && let Some(relative) = thumbnail_relative_from_media_url(system, url)
    {
        referenced.insert(relative);
    }

    referenced
}

fn referenced_thumbnail_paths_from_scans(
    system: &str,
    entries: &[GameEntry],
    scans: &[(&'static str, matching::DirScan)],
) -> HashSet<String> {
    let mut referenced = referenced_files_for_entries_from_scans(system, entries, scans);
    for entry in entries {
        if let Some(url) = &entry.box_art_url
            && let Some(relative) = thumbnail_relative_from_media_url(system, url)
        {
            referenced.insert(relative);
        }
    }
    referenced
}

#[cfg(test)]
fn referenced_thumbnail_paths(
    media_base: &Path,
    system: &str,
    entries: &[GameEntry],
) -> HashSet<String> {
    let scans = thumbnail_dir_scans(media_base);
    referenced_thumbnail_paths_from_scans(system, entries, &scans)
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
    let media_dir = media_root(storage_root);
    media_system_dirs(&media_dir)
        .into_iter()
        .map(|(system, _)| system)
        .collect()
}

fn thumbnail_files_from_scans(
    scans: &[(&'static str, matching::DirScan)],
) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = scans
        .iter()
        .flat_map(|(_, scan)| scan.valid_files.iter().cloned())
        .collect();
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
    let media_base = system_media_dir(storage_root, system);
    let scans = thumbnail_dir_scans(&media_base);
    let candidates = referenced_thumbnail_paths_for_rom_from_scans(
        system,
        rom_filename,
        display_name,
        box_art_url,
        &scans,
    );
    if candidates.is_empty() {
        return Vec::new();
    }

    let retained = referenced_thumbnail_paths_from_scans(system, active_entries, &scans);
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
    let media_dir = media_root(storage_root);
    if !media_dir.exists() {
        return Vec::new();
    }

    let mut orphans = Vec::new();

    for (system, entries) in entries_by_system {
        let system_media = media_dir.join(system);
        if !system_media.is_dir() {
            continue;
        }
        let scans = thumbnail_dir_scans(&system_media);
        let referenced = referenced_thumbnail_paths_from_scans(system, entries, &scans);
        orphans.extend(
            thumbnail_files_from_scans(&scans)
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

    fn system_media_base(storage_root: &Path, system: &str) -> PathBuf {
        storage_root
            .join(crate::storage::RC_DIR)
            .join("media")
            .join(system)
    }

    /// The set of files the runtime serve path would show for `entries`, resolved
    /// per entry per kind via the same `resolve_image_on_disk_sync` GetInfo uses.
    fn served_files(media_base: &Path, system: &str, entries: &[GameEntry]) -> HashSet<String> {
        let is_arcade = is_arcade_system(system);
        let mut served = HashSet::new();
        for entry in entries {
            let arcade_display = cleanup_arcade_display(is_arcade, entry.display_name.as_deref());
            for kind in ALL_THUMBNAIL_KINDS {
                if let Some(relative) = resolve_image_on_disk_sync(
                    arcade_display,
                    media_base,
                    kind.media_dir(),
                    &entry.rom_filename,
                ) {
                    served.insert(relative);
                }
            }
        }
        served
    }

    fn orphan_filenames(
        storage_root: &Path,
        system: &str,
        entries: &[GameEntry],
    ) -> HashSet<String> {
        let by_system = vec![(system.to_string(), entries.to_vec())];
        find_orphaned_thumbnails_from_entries(storage_root, &by_system)
            .into_iter()
            .map(|(_, path)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    /// The core invariant of the unification: runtime serving
    /// (`resolve_image_on_disk_sync`) and cleanup (`referenced_thumbnail_paths`)
    /// resolve every ROM through the SAME shared `resolve_thumbnail` over the same
    /// index, so they must agree file-for-file — cleanup can never delete a file
    /// serving would show, nor keep a true orphan. Exercises exact (Mario),
    /// fuzzy + regional tie-break (Zelda → alphabetically-first Europe), and slash
    /// dual-title (Sonic) resolution, with pure orphans mixed in.
    #[test]
    fn matcher_and_cleanup_resolve_identically() {
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        for f in [
            "Zelda (USA).png",
            "Zelda (Europe).png",
            "Mario (USA).png",
            "Boxart Orphan.png",
        ] {
            write_test_image(&media_file(
                storage.path(),
                system,
                ThumbnailKind::Boxart,
                f,
            ));
        }
        for f in ["Sonic 3.png", "Snap Orphan.png"] {
            write_test_image(&media_file(storage.path(), system, ThumbnailKind::Snap, f));
        }

        let entries = vec![
            test_entry(system, "Mario (USA).sfc"),
            test_entry(system, "Zelda.sfc"),
            test_entry(system, "Sonic & Knuckles _ Sonic 3 (World).sfc"),
        ];
        let media_base = system_media_base(storage.path(), system);

        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);

        // The invariant: serve and cleanup resolve to exactly the same files.
        assert_eq!(
            served, referenced,
            "serve and cleanup must resolve identically"
        );

        let expected: HashSet<String> = [
            "boxart/Mario (USA).png",
            "boxart/Zelda (Europe).png",
            "snap/Sonic 3.png",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(referenced, expected);

        // Cleanup deletes exactly the files no ROM resolves to — never a served one.
        let expected_orphans: HashSet<String> =
            ["Zelda (USA).png", "Boxart Orphan.png", "Snap Orphan.png"]
                .into_iter()
                .map(str::to_string)
                .collect();
        assert_eq!(
            orphan_filenames(storage.path(), system, &entries),
            expected_orphans
        );
    }

    /// Same invariant for the arcade path: art filed under the MAME display name
    /// resolves via the display tier, art filed under the ROM short name via the
    /// stem fallback — serve and cleanup both try both, so both are retained.
    #[test]
    fn matcher_and_cleanup_agree_on_arcade_display_and_stem() {
        let storage = tempfile::tempdir().unwrap();
        let system = "arcade_fbneo"; // is_arcade_system == true
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Street Fighter II' - Champion Edition.png",
        ));
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Snap,
            "sf2ce.png",
        ));
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Arcade Orphan.png",
        ));

        let mut entry = test_entry(system, "sf2ce.zip");
        entry.display_name = Some("Street Fighter II' - Champion Edition".to_string());
        let entries = vec![entry];
        let media_base = system_media_base(storage.path(), system);

        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        assert_eq!(served, referenced, "arcade serve and cleanup must agree");

        let expected: HashSet<String> = [
            "boxart/Street Fighter II' - Champion Edition.png",
            "snap/sf2ce.png",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(
            referenced, expected,
            "display-named and stem-named art both kept"
        );

        assert_eq!(
            orphan_filenames(storage.path(), system, &entries),
            ["Arcade Orphan.png"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    /// Regression: the on-disk fast path must not jump a stem-exact file ahead of
    /// a higher-priority display-name match. Here the display name has a colon, so
    /// the resolver matches the art via its colon-variant tier (`": " -> " - "`),
    /// which outranks the ROM stem. A competing stem-exact file sits in the SAME
    /// kind. A fast path that probed display-exact THEN stem-exact would serve the
    /// stem file (display-exact misses the colon-variant name), while cleanup's
    /// full resolver keeps the colon-variant file — so cleanup would delete the
    /// served stem file. Probing only the top-priority key keeps them in agreement.
    #[test]
    fn matcher_and_cleanup_agree_when_display_colon_variant_outranks_stem() {
        let storage = tempfile::tempdir().unwrap();
        let system = "arcade_fbneo"; // is_arcade_system == true
        // The resolver's colon tier turns "II: Champion" into "II - Champion".
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Street Fighter II - Champion Edition.png",
        ));
        // Competing stem-exact art in the same kind — must lose to the display tier.
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "sf2ce.png",
        ));

        let mut entry = test_entry(system, "sf2ce.zip");
        entry.display_name = Some("Street Fighter II: Champion Edition".to_string());
        let entries = vec![entry];
        let media_base = system_media_base(storage.path(), system);

        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        // The core invariant: the fast path and the full-scan cleanup must agree.
        assert_eq!(
            served, referenced,
            "fast path must not outrank display colon-variant with stem-exact"
        );
        // Both must pick the colon-variant display art, never the stem file.
        assert_eq!(
            referenced,
            ["boxart/Street Fighter II - Champion Edition.png"]
                .into_iter()
                .map(str::to_string)
                .collect::<HashSet<String>>()
        );
        // The stem file is a genuine orphan under resolver semantics, safe to prune
        // precisely because the fast path never serves it.
        assert_eq!(
            orphan_filenames(storage.path(), system, &entries),
            ["sf2ce.png"].into_iter().map(str::to_string).collect()
        );
    }

    /// Regression: a stem present as both `.png` and `.jpg` must resolve the same
    /// way for runtime serve (the fast path, which probes `.png`) and orphan
    /// cleanup (the full scan). Before the exact tier gained a deterministic
    /// `.png` > `.jpg` preference, the full scan's last-writer-wins could land on
    /// the `.jpg` depending on readdir order, so cleanup would delete the `.png`
    /// the fast path just served. Both must pick the `.png`, and only the `.jpg`
    /// twin is pruned.
    #[test]
    fn matcher_and_cleanup_agree_on_png_over_jpg_duplicate_stem() {
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Sonic.png",
        ));
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Sonic.jpg",
        ));

        let entries = vec![test_entry(system, "Sonic.sfc")];
        let media_base = system_media_base(storage.path(), system);

        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        assert_eq!(
            served, referenced,
            "serve and cleanup must agree on the .png/.jpg twin"
        );
        assert_eq!(
            referenced,
            ["boxart/Sonic.png"]
                .into_iter()
                .map(str::to_string)
                .collect::<HashSet<String>>(),
            "the .png twin is the served/referenced file"
        );
        // Only the unreferenced .jpg twin is an orphan; the served .png survives.
        assert_eq!(
            orphan_filenames(storage.path(), system, &entries),
            ["Sonic.jpg"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn serve_resolves_non_png_art_and_prefers_png_when_both_exist() {
        // The image served for a ROM: a jpg-only cover is served as-is, and when
        // both a .png and .jpg exist the .png is served. (A new format added to
        // IMAGE_EXTENSIONS is served the same way — nothing else hard-codes .png.)
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        let media_base = system_media_base(storage.path(), system);

        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Mario.jpg",
        ));
        assert_eq!(
            resolve_image_on_disk_sync(None, &media_base, "boxart", "Mario.sfc").as_deref(),
            Some("boxart/Mario.jpg"),
        );

        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Mario.png",
        ));
        assert_eq!(
            resolve_image_on_disk_sync(None, &media_base, "boxart", "Mario.sfc").as_deref(),
            Some("boxart/Mario.png"),
        );
    }

    /// Regression: a real self-named `{stem}.png` must win the exact tier over a
    /// same-stem fake-symlink stub that redirects elsewhere — otherwise the full
    /// scan could reference the stub's (alphabetically-earlier) target while the
    /// fast path serves the real `{stem}.png`, and cleanup would delete the served
    /// file. Serve and cleanup must both resolve the ROM to its real `.png`.
    #[test]
    fn matcher_and_cleanup_agree_when_real_png_competes_with_samestem_stub() {
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        // Real self-named art for the ROM (creates the boxart dir).
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Sonic.png",
        ));
        // A same-stem fake-symlink stub: a tiny text file whose content is the
        // target filename, redirecting stem "Sonic" to a differently-named image.
        std::fs::write(
            media_file(storage.path(), system, ThumbnailKind::Boxart, "Sonic.jpg"),
            b"Aardvark.png",
        )
        .unwrap();
        // The stub's target: real, but named for a different game (sorts first).
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Aardvark.png",
        ));

        let entries = vec![test_entry(system, "Sonic.sfc")];
        let media_base = system_media_base(storage.path(), system);

        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        assert_eq!(
            served, referenced,
            "serve and cleanup must agree despite the same-stem stub"
        );
        assert_eq!(
            referenced,
            ["boxart/Sonic.png"]
                .into_iter()
                .map(str::to_string)
                .collect::<HashSet<String>>(),
            "the real self-named .png wins over the stub's redirect"
        );
        // The served Sonic.png must NOT be pruned; only the unreferenced target is.
        let orphans = orphan_filenames(storage.path(), system, &entries);
        assert!(
            !orphans.contains("Sonic.png"),
            "served file must never be deleted, got {orphans:?}"
        );
        assert_eq!(
            orphans,
            ["Aardvark.png"].into_iter().map(str::to_string).collect()
        );
    }

    /// Regression: an OS symlink in a media dir (which the app never writes) must
    /// be treated the same by serve and cleanup. `is_file()` follows symlinks but
    /// the scanner does not, so a fast path using `is_file()` would serve the
    /// symlink while cleanup left its target an orphan — deleting a served file.
    /// Both must ignore it (the scanner never indexes it), so nothing is served
    /// via the link and nothing it points at is protected by it.
    #[cfg(unix)]
    #[test]
    fn serve_and_cleanup_ignore_os_symlinks_identically() {
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Aardvark.png",
        ));
        let boxart = system_media_base(storage.path(), system).join("boxart");
        std::os::unix::fs::symlink("Aardvark.png", boxart.join("Sonic.png")).unwrap();

        let entries = vec![test_entry(system, "Sonic.sfc")];
        let media_base = system_media_base(storage.path(), system);
        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        assert_eq!(
            served, referenced,
            "serve and cleanup must treat the OS symlink identically"
        );
        assert!(
            served.is_empty(),
            "the OS symlink is never indexed, so nothing is served for it"
        );
    }

    /// Regression: a fake-symlink stub may point at any supported extension, not
    /// only `.png`. A stub resolving to a `.jpg` target must resolve (serve) and
    /// keep that target (cleanup) — matching each other.
    #[test]
    fn serve_resolves_fake_symlink_stub_to_non_png_target() {
        let storage = tempfile::tempdir().unwrap();
        let system = "nintendo_snes";
        // The real target, named for the game, as a .jpg.
        write_test_image(&media_file(
            storage.path(),
            system,
            ThumbnailKind::Boxart,
            "Real Cover.jpg",
        ));
        // The stub, named for the ROM: a tiny text file whose content is the target.
        std::fs::write(
            media_file(storage.path(), system, ThumbnailKind::Boxart, "Doom.png"),
            b"Real Cover.jpg",
        )
        .unwrap();

        let entries = vec![test_entry(system, "Doom.sfc")];
        let media_base = system_media_base(storage.path(), system);
        let served = served_files(&media_base, system, &entries);
        let referenced = referenced_thumbnail_paths(&media_base, system, &entries);
        assert_eq!(served, referenced, "serve and cleanup must agree");
        assert_eq!(
            referenced,
            ["boxart/Real Cover.jpg"]
                .into_iter()
                .map(str::to_string)
                .collect::<HashSet<String>>(),
            "the stub resolves to its .jpg target",
        );
        assert!(
            !orphan_filenames(storage.path(), system, &entries).contains("Real Cover.jpg"),
            "the stub's referenced target must not be pruned"
        );
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
    fn find_orphaned_thumbnails_keeps_special_char_box_art_url_target() {
        // Regression: a game whose stored box_art_url is percent-encoded and
        // points at a special-char filename that does NOT match its own
        // base_title (arcade art named after another system's display name)
        // must be retained via the URL — the URL was compared encoded against
        // decoded on-disk names, so it was wrongly deleted.
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let mut conn = LibraryDb::open(db_dir.path()).unwrap();
        write_test_image(&media_file(
            storage.path(),
            "arcade_mame_2k3p",
            ThumbnailKind::Boxart,
            "Fighters' Impact A (Ver 2.00J).png",
        ));
        write_test_image(&media_file(
            storage.path(),
            "arcade_mame_2k3p",
            ThumbnailKind::Boxart,
            "Unrelated.png",
        ));

        let mut entry = test_entry("arcade_mame_2k3p", "ftimpcta.zip");
        entry.display_name = Some("Fighter's Impact Ace (JAPAN)".to_string());
        entry.box_art_url = Some(
            "/media/arcade_mame_2k3p/boxart/Fighters%27%20Impact%20A%20%28Ver%202.00J%29.png"
                .to_string(),
        );
        LibraryDb::save_system_entries(&mut conn, "arcade_mame_2k3p", &[entry], None).unwrap();

        let orphans = find_orphaned_thumbnails(storage.path(), &conn).unwrap();

        // The box_art_url target is retained; only Unrelated.png is orphaned.
        assert!(
            orphans
                .iter()
                .all(|(_, p)| p.file_name().unwrap() == "Unrelated.png"),
            "box_art_url target should be retained, got {orphans:?}"
        );
    }

    #[test]
    fn find_orphaned_thumbnails_keeps_active_variant_and_removes_others() {
        // The user's selected cover (box_art_url) survives cleanup even when it
        // is NOT the resolver's default pick, while a third non-active,
        // non-default regional variant is removed. This is the accepted cleanup
        // semantic: the active cover is kept; alternates are pruned (and remain
        // re-downloadable via the picker's manifest layer — see the
        // find_boxart_variants Layer 2 test).
        let storage = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let mut conn = LibraryDb::open(db_dir.path()).unwrap();
        for f in ["Zelda (USA).png", "Zelda (Europe).png", "Zelda (Japan).png"] {
            write_test_image(&media_file(
                storage.path(),
                "nintendo_snes",
                ThumbnailKind::Boxart,
                f,
            ));
        }
        // Resolver default for "Zelda.sfc" is the alphabetically-first variant
        // (Europe); the user selected USA, stored percent-encoded in box_art_url.
        let mut entry = test_entry("nintendo_snes", "Zelda.sfc");
        entry.box_art_url = Some("/media/nintendo_snes/boxart/Zelda%20%28USA%29.png".to_string());
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[entry], None).unwrap();

        let orphans: HashSet<String> = find_orphaned_thumbnails(storage.path(), &conn)
            .unwrap()
            .into_iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        // USA kept (active/box_art_url), Europe kept (resolver default), Japan removed.
        assert_eq!(
            orphans,
            ["Zelda (Japan).png"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[test]
    fn percent_decode_round_trips_encode() {
        for s in [
            "Fighters' Impact A (Ver 2.00J)",
            "Simple",
            "A & B / C",
            "50% Off",
            "Pokémon (日本)",
        ] {
            assert_eq!(
                percent_decode_uri_segment(&percent_encode_uri_segment(s)),
                s
            );
        }
        // A lone or truncated `%` is left literal.
        assert_eq!(percent_decode_uri_segment("100%"), "100%");
        assert_eq!(percent_decode_uri_segment("ab%2"), "ab%2");
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
        let is_arcade = is_arcade_system(system);
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
