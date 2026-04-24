//! Image thumbnail support via libretro-thumbnails.
//!
//! Maps RePlayOS system folder names to libretro-thumbnails repo names,
//! normalizes filenames, and provides fuzzy matching utilities.

pub mod manifest;
pub mod matching;
pub mod resolution;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use replay_control_core::error::{Error, Result};

/// Kind of thumbnail image.
#[derive(Debug, Clone, Copy)]
pub enum ThumbnailKind {
    Boxart,
    Snap,
    Title,
}

/// All thumbnail kinds, for iteration.
pub const ALL_THUMBNAIL_KINDS: &[ThumbnailKind] = &[
    ThumbnailKind::Boxart,
    ThumbnailKind::Snap,
    ThumbnailKind::Title,
];

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

/// Map RePlayOS system folder names to libretro-thumbnails repo names.
/// Returns one or more repo names (primary first). Multiple repos are tried
/// in order during import, so ROMs not found in the primary repo can be
/// matched against fallback repos.
pub fn thumbnail_repo_names(system: &str) -> Option<&'static [&'static str]> {
    match system {
        "atari_2600" => Some(&["Atari - 2600"]),
        "atari_5200" => Some(&["Atari - 5200"]),
        "atari_7800" => Some(&["Atari - 7800 ProSystem"]),
        "atari_jaguar" => Some(&["Atari - Jaguar"]),
        "atari_lynx" => Some(&["Atari - Lynx"]),
        "amstrad_cpc" => Some(&["Amstrad - CPC"]),
        "commodore_ami" => Some(&["Commodore - Amiga"]),
        // commodore_amicd covers CD32 + CDTV hardware
        "commodore_amicd" => Some(&["Commodore - CD32", "Commodore - CDTV"]),
        "commodore_c64" => Some(&["Commodore - 64"]),
        "ibm_pc" => Some(&["DOS"]),
        "microsoft_msx" => Some(&["Microsoft - MSX"]),
        "nec_pce" => Some(&["NEC - PC Engine - TurboGrafx 16"]),
        "nec_pcecd" => Some(&["NEC - PC Engine CD - TurboGrafx-CD"]),
        "nintendo_ds" => Some(&["Nintendo - Nintendo DS"]),
        "nintendo_gb" => Some(&["Nintendo - Game Boy"]),
        "nintendo_gba" => Some(&["Nintendo - Game Boy Advance"]),
        "nintendo_gbc" => Some(&["Nintendo - Game Boy Color"]),
        "nintendo_n64" => Some(&["Nintendo - Nintendo 64"]),
        "nintendo_nes" => Some(&["Nintendo - Nintendo Entertainment System"]),
        "nintendo_snes" => Some(&["Nintendo - Super Nintendo Entertainment System"]),
        "panasonic_3do" => Some(&["The 3DO Company - 3DO"]),
        "philips_cdi" => Some(&["Philips - CDi"]),
        "sega_32x" => Some(&["Sega - 32X", "Sega - Mega-CD - Sega CD"]),
        "sega_cd" => Some(&["Sega - Mega-CD - Sega CD"]),
        "sega_dc" => Some(&["Sega - Dreamcast"]),
        "sega_gg" => Some(&["Sega - Game Gear"]),
        "sega_sg" => Some(&["Sega - SG-1000"]),
        "sega_smd" => Some(&["Sega - Mega Drive - Genesis"]),
        "sega_sms" => Some(&["Sega - Master System - Mark III"]),
        "sega_st" => Some(&["Sega - Saturn"]),
        "scummvm" => Some(&["ScummVM"]),
        "sharp_x68k" => Some(&["Sharp - X68000"]),
        "sinclair_zx" => Some(&["Sinclair - ZX Spectrum"]),
        "snk_ng" => Some(&["SNK - Neo Geo"]),
        "snk_ngcd" => Some(&["SNK - Neo Geo CD"]),
        "snk_ngp" => Some(&["SNK - Neo Geo Pocket"]),
        "sony_psx" => Some(&["Sony - PlayStation"]),
        // Arcade systems — libretro-thumbnails uses display names as filenames,
        // so the manifest builder translates MAME codenames via arcade_db.
        // MAME primary, FBNeo fallback (each repo has some boxarts the other lacks)
        "arcade_mame" => Some(&["MAME", "FBNeo - Arcade Games"]),
        // FBNeo primary, MAME fallback (FBNeo repo is missing some boxarts that MAME has)
        "arcade_fbneo" => Some(&["FBNeo - Arcade Games", "MAME"]),
        "arcade_mame_2k3p" => Some(&["MAME"]),
        // arcade_dc covers Atomiswave + Naomi + Naomi 2 hardware
        "arcade_dc" => Some(&["Atomiswave", "Sega - Naomi", "Sega - Naomi 2"]),
        _ => None,
    }
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
pub use replay_control_core::title_utils::{base_title, strip_tags, strip_version};

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

    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);
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

    let rom_base = base_title(&thumb_name);
    let rom_base_no_version = strip_version(&rom_base);
    let has_version = rom_base_no_version.len() < rom_base.len();
    let thumb_lower = thumb_name.to_lowercase();

    // Pre-compute slash parts for tier 4 matching.
    let search_base = if has_version {
        rom_base_no_version
    } else {
        &rom_base
    };
    let slash_parts: Vec<&str> = if search_base.contains(" / ") || search_base.contains(" _ ") {
        let sep = if search_base.contains(" / ") {
            " / "
        } else {
            " _ "
        };
        search_base
            .split(sep)
            .map(|p| p.trim())
            .filter(|p| p.len() >= 5)
            .collect()
    } else {
        Vec::new()
    };

    if let Ok(entries) = std::fs::read_dir(&kind_dir) {
        let mut fuzzy_result: Option<String> = None;
        let mut version_result: Option<String> = None;
        let mut slash_result: Option<String> = None;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(img_stem) = strip_image_ext(&name) {
                // 1b. Case-insensitive exact match (preserves region tags)
                if img_stem.to_lowercase() == thumb_lower {
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
                if img_base == rom_base && fuzzy_result.is_none() {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        fuzzy_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        fuzzy_result = Some(format!("{kind}/{resolved}"));
                    }
                }
                // 3. Version-stripped match
                if has_version && img_base == rom_base_no_version && version_result.is_none() {
                    let path = entry.path();
                    if is_valid_image_sync(&path) {
                        version_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink_sync(&path, &kind_dir) {
                        version_result = Some(format!("{kind}/{resolved}"));
                    }
                }
                // 4. Slash dual-name match
                if slash_result.is_none() && slash_parts.iter().any(|part| *part == img_base) {
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
    collect_rom_filenames_recursive(&roms_dir, &mut filenames, extensions);
    filenames
}

fn collect_rom_filenames_recursive(
    dir: &Path,
    filenames: &mut Vec<String>,
    extensions: Option<&[&str]>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with('_') {
                collect_rom_filenames_recursive(&path, filenames, extensions);
            }
        } else {
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
                    continue;
                }
            }
            filenames.push(name);
        }
    }
}

/// Get the total size of the media directory for all systems.
pub fn media_dir_size(storage_root: &Path) -> u64 {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    dir_size(&media_dir)
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
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

/// Find orphaned thumbnail files that are not referenced by any active ROM.
///
/// An image file is considered orphaned if no entry in `game_library` for its system
/// has a `box_art_url` pointing to that file. This approach is simpler and more
/// reliable than trying to replicate the fuzzy matching pipeline.
///
/// Returns a list of `(system, file_path)` pairs for each orphaned image.
pub fn find_orphaned_thumbnails(
    storage_root: &Path,
    conn: &rusqlite::Connection,
) -> Result<Vec<(String, PathBuf)>> {
    let media_dir = storage_root.join(crate::storage::RC_DIR).join("media");
    if !media_dir.exists() {
        return Ok(Vec::new());
    }

    let active_systems = crate::library_db::LibraryDb::active_systems(conn)?;
    let mut orphans = Vec::new();

    // Only check systems that have entries in game_library.
    // Systems without game_library entries may have images from a previous scan
    // that haven't been warmed yet — we don't want to delete those.
    for system in &active_systems {
        let system_media = media_dir.join(system);
        if !system_media.exists() {
            continue;
        }

        // Get all box_art_url values for this system and extract the filesystem-relative
        // image paths. URLs look like `/media/sega_smd/boxart/Sonic.png`, so we strip
        // the `/media/<system>/` prefix to get `boxart/Sonic.png`.
        let prefix = format!("/media/{system}/");
        let referenced: HashSet<String> =
            crate::library_db::LibraryDb::active_box_art_urls(conn, system)?
                .into_iter()
                .filter_map(|url| url.strip_prefix(&prefix).map(|s| s.to_string()))
                .collect();

        // Safety: skip systems where enrichment hasn't run yet.
        // If game_library has entries but no box_art_url is set, enrichment is still
        // in progress — deleting now would wipe all images.
        if referenced.is_empty() {
            continue;
        }

        // Only scan boxart/ — snap images have no corresponding URL column in
        // game_library, so we can't determine which are orphaned.
        let kind = ThumbnailKind::Boxart.media_dir();
        let kind_dir = system_media.join(kind);
        if kind_dir.exists() {
            let entries = match std::fs::read_dir(&kind_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let mut system_orphans = Vec::new();
            let mut total_files = 0usize;

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let filename = entry.file_name();
                let filename = filename.to_string_lossy();
                if !filename.ends_with(".png") {
                    continue;
                }
                total_files += 1;
                let relative = format!("{kind}/{filename}");
                if !referenced.contains(&relative) {
                    system_orphans.push((system.clone(), path));
                }
            }

            // Safety net: if >80% of images would be deleted, something is wrong
            // (likely stale game_library). Skip this system entirely.
            if total_files > 0 && system_orphans.len() * 100 / total_files > 80 {
                continue;
            }

            orphans.extend(system_orphans);
        }
    }

    Ok(orphans)
}

/// Delete orphaned thumbnail files and return `(count_deleted, bytes_freed)`.
///
/// Uses [`find_orphaned_thumbnails`] to identify files, then deletes each one.
pub fn delete_orphaned_thumbnails(
    storage_root: &Path,
    conn: &rusqlite::Connection,
) -> Result<(usize, u64)> {
    let orphans = find_orphaned_thumbnails(storage_root, conn)?;
    let mut deleted = 0usize;
    let mut bytes_freed = 0u64;

    for (_system, path) in &orphans {
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

    Ok((deleted, bytes_freed))
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

    // --- thumbnail_repo_names ---

    #[test]
    fn repo_names_known_systems() {
        assert_eq!(
            thumbnail_repo_names("nintendo_snes"),
            Some(["Nintendo - Super Nintendo Entertainment System"].as_slice())
        );
        assert_eq!(
            thumbnail_repo_names("sega_smd"),
            Some(["Sega - Mega Drive - Genesis"].as_slice())
        );
        assert_eq!(
            thumbnail_repo_names("nintendo_nes"),
            Some(["Nintendo - Nintendo Entertainment System"].as_slice())
        );
    }

    #[test]
    fn repo_names_unknown_system() {
        assert_eq!(thumbnail_repo_names("nonexistent_system"), None);
    }

    #[test]
    fn repo_names_multi_repo_arcade_dc() {
        let repos = thumbnail_repo_names("arcade_dc").unwrap();
        assert_eq!(repos.len(), 3);
        assert!(repos.contains(&"Atomiswave"));
        assert!(repos.contains(&"Sega - Naomi"));
        assert!(repos.contains(&"Sega - Naomi 2"));
    }

    #[test]
    fn repo_names_multi_repo_commodore_amicd() {
        let repos = thumbnail_repo_names("commodore_amicd").unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.contains(&"Commodore - CD32"));
        assert!(repos.contains(&"Commodore - CDTV"));
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
        let stem = replay_control_core::title_utils::filename_stem(rom_filename);
        let is_arcade = replay_control_core::systems::is_arcade_system(system);
        let display_name = if is_arcade {
            crate::arcade_db::lookup_arcade_game(stem)
                .await
                .map(|info| info.display_name)
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
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_fbneo").await;
        // "Street Fighter II: The World Warrior (...)"
        // thumbnail_filename replaces ':' with '_'
        assert!(
            thumb_name.contains("Street Fighter II_ The World Warrior"),
            "expected colon replaced with underscore, got '{thumb_name}'"
        );

        // Colon variant: ": " → " - "
        let info = crate::arcade_db::lookup_arcade_game("sf2").await.unwrap();
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
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_mame").await;
        assert!(
            thumb_name.contains("Street Fighter II"),
            "arcade_mame should also use arcade_db, got '{thumb_name}'"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn arcade_dc_system_uses_arcade_db() {
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
        let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);
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
}
