//! Image thumbnail support via libretro-thumbnails.
//!
//! Downloads box art and screenshots from the libretro-thumbnails GitHub repos
//! and copies matching images to `<storage>/.replay-control/media/<system>/`.

use std::path::Path;

use crate::arcade_db;
use crate::error::{Error, Result};
use crate::metadata_db::MetadataDb;

/// Kind of thumbnail image.
#[derive(Debug, Clone, Copy)]
pub enum ThumbnailKind {
    Boxart,
    Snap,
}

impl ThumbnailKind {
    /// Subdirectory name in the libretro-thumbnails repo.
    pub fn repo_dir(&self) -> &'static str {
        match self {
            ThumbnailKind::Boxart => "Named_Boxarts",
            ThumbnailKind::Snap => "Named_Snaps",
        }
    }

    /// Subdirectory name in our media storage.
    pub fn media_dir(&self) -> &'static str {
        match self {
            ThumbnailKind::Boxart => "boxart",
            ThumbnailKind::Snap => "snap",
        }
    }
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
        "sega_32x" => Some(&["Sega - 32X"]),
        "sega_cd" => Some(&["Sega - Mega-CD - Sega CD"]),
        "sega_dc" => Some(&["Sega - Dreamcast"]),
        "sega_gg" => Some(&["Sega - Game Gear"]),
        "sega_sg" => Some(&["Sega - SG-1000"]),
        "sega_smd" => Some(&["Sega - Mega Drive - Genesis"]),
        "sega_sms" => Some(&["Sega - Master System - Mark III"]),
        "sega_st" => Some(&["Sega - Saturn"]),
        "sharp_x68k" => Some(&["Sharp - X68000"]),
        "sinclair_zx" => Some(&["Sinclair - ZX Spectrum"]),
        "snk_ng" => Some(&["SNK - Neo Geo"]),
        "snk_ngcd" => Some(&["SNK - Neo Geo CD"]),
        "snk_ngp" => Some(&["SNK - Neo Geo Pocket"]),
        "sony_psx" => Some(&["Sony - PlayStation"]),
        // Arcade systems — libretro-thumbnails uses display names as filenames,
        // so import_system_thumbnails() translates MAME codenames via arcade_db.
        "arcade_mame" => Some(&["MAME"]),
        "arcade_fbneo" => Some(&["FBNeo - Arcade Games"]),
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

/// Copy a PNG from a cloned repo, resolving "fake symlinks" created by git on
/// filesystems that don't support symlinks (e.g., exFAT). On such filesystems,
/// git writes the symlink target path as a small text file instead of creating
/// a real symlink. We detect these by checking for the PNG magic bytes.
fn copy_png(src: &Path, dst: &Path) -> std::io::Result<()> {
    let real_src = resolve_fake_symlink(src)?;
    std::fs::copy(&real_src, dst)?;
    Ok(())
}

/// If `path` is a small file without PNG magic bytes, treat it as a git "fake
/// symlink" — read its text content as a relative path and resolve it.
fn resolve_fake_symlink(path: &Path) -> std::io::Result<std::path::PathBuf> {
    const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

    let meta = std::fs::metadata(path)?;
    // Real PNGs are almost always > 200 bytes; fake symlinks are short text.
    if meta.len() < 200 {
        let bytes = std::fs::read(path)?;
        if !bytes.starts_with(&PNG_MAGIC) {
            // Content is the relative target path (utf-8 text).
            let target = std::str::from_utf8(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .trim();
            let resolved = path.parent().unwrap_or(Path::new(".")).join(target);
            if resolved.exists() {
                // Recursively resolve in case of chained symlinks.
                return resolve_fake_symlink(&resolved);
            }
        }
    }
    Ok(path.to_path_buf())
}

/// Stats from an image import operation.
#[derive(Debug, Clone)]
pub struct ImageImportStats {
    pub total_roms: usize,
    pub boxart_copied: usize,
    pub snap_copied: usize,
}

/// Strip parenthesized tags and trailing whitespace from a name for fuzzy matching.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` → `"Indiana Jones and the Fate of Atlantis"`
/// `"Dark Seed"` → `"Dark Seed"` (unchanged)
fn strip_tags(name: &str) -> &str {
    name.find(" (")
        .or_else(|| name.find(" ["))
        .map(|i| &name[..i])
        .unwrap_or(name)
        .trim()
}

/// Strip GDI/TOSEC version strings from a name for fuzzy matching.
/// `"Sonic Adventure 2 v1.008"` → `"Sonic Adventure 2"`
/// `"Sega Rally 2 v1 001"` → `"Sega Rally 2"`
/// Returns the original string if no version pattern is found.
fn strip_version(name: &str) -> &str {
    // Look for " v" followed by a digit, then optional digits/dots/spaces/underscores
    let bytes = name.as_bytes();
    let mut i = 0;
    let mut last_version_start = None;
    while i + 2 < bytes.len() {
        if bytes[i] == b' ' && bytes[i + 1] == b'v' && bytes.get(i + 2).is_some_and(|b| b.is_ascii_digit()) {
            // Check that everything after " v\d" is digits, dots, spaces, or underscores
            let rest = &bytes[i + 2..];
            if rest.iter().all(|b| b.is_ascii_digit() || *b == b'.' || *b == b' ' || *b == b'_') {
                last_version_start = Some(i);
            }
        }
        i += 1;
    }
    match last_version_start {
        Some(pos) => name[..pos].trim(),
        None => name,
    }
}

/// Fuzzy lookup index built from a repo image directory.
/// Contains two tiers: stripped-tags keys and version-stripped keys.
struct FuzzyIndex {
    /// Maps `lowercase(strip_tags(stem))` → original stem
    by_tags: std::collections::HashMap<String, String>,
    /// Maps `lowercase(strip_version(strip_tags(stem)))` → original stem
    by_version: std::collections::HashMap<String, String>,
}

/// Build a fuzzy index from a repo image directory.
fn build_fuzzy_index(dir: &Path) -> FuzzyIndex {
    let mut by_tags = std::collections::HashMap::new();
    let mut by_version = std::collections::HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return FuzzyIndex { by_tags, by_version },
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".png") {
            let stripped = strip_tags(stem);
            let key = stripped.to_lowercase();
            // Only keep the first match per key (avoid collisions).
            by_tags.entry(key.clone()).or_insert_with(|| stem.to_string());
            let version_key = strip_version(&key);
            if version_key.len() < key.len() {
                by_version.entry(version_key.to_string()).or_insert_with(|| stem.to_string());
            }
        }
    }
    FuzzyIndex { by_tags, by_version }
}

/// Try to find a thumbnail file: exact match → fuzzy (stripped tags) → version-stripped.
fn find_thumbnail(
    repo_subdir: &Path,
    thumb_name: &str,
    fuzzy_index: &FuzzyIndex,
) -> Option<(std::path::PathBuf, String)> {
    // 1. Exact match
    let exact = repo_subdir.join(format!("{thumb_name}.png"));
    if exact.exists() {
        return Some((exact, thumb_name.to_string()));
    }

    // 2. Fuzzy: strip tags from ROM stem, look up in index
    let key = strip_tags(thumb_name).to_lowercase();
    if let Some(repo_stem) = fuzzy_index.by_tags.get(&key) {
        let path = repo_subdir.join(format!("{repo_stem}.png"));
        if path.exists() {
            return Some((path, repo_stem.clone()));
        }
    }

    // 3. Version-stripped: handles GDI/TOSEC names like "Sonic Adventure 2 v1.008"
    let version_key = strip_version(&key);
    if version_key.len() < key.len() {
        // Look in both the tags index (repo entry has no version) and version index (repo entry also has a version)
        if let Some(repo_stem) = fuzzy_index.by_tags.get(version_key)
            .or_else(|| fuzzy_index.by_version.get(version_key))
        {
            let path = repo_subdir.join(format!("{repo_stem}.png"));
            if path.exists() {
                return Some((path, repo_stem.clone()));
            }
        }
    }

    None
}

/// Import images for a single system from a cloned libretro-thumbnails repo.
///
/// `repo_dir` is the path to the cloned repo (e.g., `/tmp/libretro-thumbnails/Nintendo - Super Nintendo Entertainment System`).
/// `rom_filenames` are the ROM files on disk for this system.
pub fn import_system_thumbnails(
    repo_dir: &Path,
    system: &str,
    storage_root: &Path,
    db: &mut MetadataDb,
    rom_filenames: &[String],
    mut on_progress: impl FnMut(usize, usize) -> bool,
) -> Result<ImageImportStats> {
    let media_base = storage_root
        .join(crate::metadata_db::RC_DIR)
        .join("media")
        .join(system);

    let boxart_dir = media_base.join("boxart");
    let snap_dir = media_base.join("snap");
    std::fs::create_dir_all(&boxart_dir).map_err(|e| Error::io(&boxart_dir, e))?;
    std::fs::create_dir_all(&snap_dir).map_err(|e| Error::io(&snap_dir, e))?;

    // Build fuzzy indexes for fallback matching.
    let boxart_repo_dir = repo_dir.join("Named_Boxarts");
    let snap_repo_dir = repo_dir.join("Named_Snaps");
    let boxart_index = build_fuzzy_index(&boxart_repo_dir);
    let snap_index = build_fuzzy_index(&snap_repo_dir);

    let mut stats = ImageImportStats {
        total_roms: rom_filenames.len(),
        boxart_copied: 0,
        snap_copied: 0,
    };

    let mut db_updates: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();

    let is_arcade = matches!(system, "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc");

    for (i, rom_filename) in rom_filenames.iter().enumerate() {
        let stem = match rom_filename.rfind('.') {
            Some(i) => &rom_filename[..i],
            None => rom_filename,
        };

        // For arcade systems, ROM filenames are MAME codenames (e.g. "sf2")
        // but libretro-thumbnails uses display names (e.g. "Street Fighter II_ The World Warrior").
        // Translate through arcade_db.
        let display_name = if is_arcade {
            arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
        } else {
            None
        };
        let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));

        // Build colon-variant names for fallback matching.
        // thumbnail_filename() replaces `:` with `_`, but libretro-thumbnails contributors
        // sometimes used ` -` or dropped the colon entirely instead.
        let has_colon = display_name.unwrap_or(stem).contains(':');
        let colon_variants: Vec<String> = if has_colon {
            let source = display_name.unwrap_or(stem);
            vec![
                // Dash variant: "Title: Subtitle" → "Title - Subtitle"
                thumbnail_filename(&source.replace(": ", " - ").replace(':', " -")),
                // Dropped variant: "Title: Subtitle" → "Title Subtitle"
                thumbnail_filename(&source.replace(": ", " ").replace(':', "")),
            ]
        } else {
            Vec::new()
        };

        let mut boxart_rel: Option<String> = None;
        let mut snap_rel: Option<String> = None;

        // Try boxart (exact then fuzzy, with colon-variant fallbacks)
        let boxart_match = find_thumbnail(&boxart_repo_dir, &thumb_name, &boxart_index)
            .or_else(|| {
                colon_variants.iter().find_map(|v| find_thumbnail(&boxart_repo_dir, v, &boxart_index))
            });
        if let Some((src, matched_stem)) = boxart_match {
            let dst_name = format!("{matched_stem}.png");
            let dst = boxart_dir.join(&dst_name);
            if !dst.exists() {
                if let Err(e) = copy_png(&src, &dst) {
                    tracing::debug!("Failed to copy boxart for {rom_filename}: {e}");
                } else {
                    stats.boxart_copied += 1;
                    boxart_rel = Some(format!("boxart/{dst_name}"));
                }
            } else {
                stats.boxart_copied += 1;
                boxart_rel = Some(format!("boxart/{dst_name}"));
            }
        }

        // Try snap (exact then fuzzy, with colon-variant fallbacks)
        let snap_match = find_thumbnail(&snap_repo_dir, &thumb_name, &snap_index)
            .or_else(|| {
                colon_variants.iter().find_map(|v| find_thumbnail(&snap_repo_dir, v, &snap_index))
            });
        if let Some((src, matched_stem)) = snap_match {
            let dst_name = format!("{matched_stem}.png");
            let dst = snap_dir.join(&dst_name);
            if !dst.exists() {
                if let Err(e) = copy_png(&src, &dst) {
                    tracing::debug!("Failed to copy snap for {rom_filename}: {e}");
                } else {
                    stats.snap_copied += 1;
                    snap_rel = Some(format!("snap/{dst_name}"));
                }
            } else {
                stats.snap_copied += 1;
                snap_rel = Some(format!("snap/{dst_name}"));
            }
        }

        if boxart_rel.is_some() || snap_rel.is_some() {
            db_updates.push((
                system.to_string(),
                rom_filename.clone(),
                boxart_rel,
                snap_rel,
            ));
        }

        if (i + 1) % 10 == 0 {
            if !on_progress(i + 1, stats.boxart_copied + stats.snap_copied) {
                // Cancelled — flush what we have so far and return.
                if !db_updates.is_empty() {
                    let _ = db.bulk_update_image_paths(&db_updates);
                }
                return Ok(stats);
            }
        }
    }

    // Batch update DB
    if !db_updates.is_empty() {
        db.bulk_update_image_paths(&db_updates)
            .map_err(|e| Error::Other(format!("Failed to update image paths: {e}")))?;
    }

    on_progress(stats.total_roms, stats.boxart_copied + stats.snap_copied);

    tracing::info!(
        "Image import for {system}: {}/{} boxart, {}/{} snaps",
        stats.boxart_copied,
        stats.total_roms,
        stats.snap_copied,
        stats.total_roms,
    );

    Ok(stats)
}

/// Clone a libretro-thumbnails repo.
///
/// `clone_base` is the parent directory for clones (e.g. `<storage>/.replay-control/tmp`).
/// Falls back to `/tmp` if `None`.
///
/// If `cancel` is provided, the clone subprocess is killed when the flag becomes `true`.
pub fn clone_thumbnail_repo(
    repo_name: &str,
    clone_base: Option<&Path>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<std::path::PathBuf> {
    let url = format!(
        "https://github.com/libretro-thumbnails/{}.git",
        repo_name.replace(' ', "_")
    );
    let base = clone_base
        .map(|b| b.join("libretro-thumbnails"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/libretro-thumbnails"));
    let dest = base.join(repo_name);

    // If already cloned, reuse
    if dest.join("Named_Boxarts").exists() {
        tracing::info!("Reusing existing clone at {}", dest.display());
        return Ok(dest);
    }

    std::fs::create_dir_all(dest.parent().unwrap())
        .map_err(|e| Error::io(&dest, e))?;

    // Remove partial clone if exists
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }

    tracing::info!("Cloning {url} ...");
    let mut child = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &url, &dest.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to run git: {e}")))?;

    // Poll the child process, checking for cancellation every 200ms.
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = child.stderr.take()
                        .map(|mut s| {
                            let mut buf = String::new();
                            use std::io::Read;
                            let _ = s.read_to_string(&mut buf);
                            buf
                        })
                        .unwrap_or_default();
                    return Err(Error::Other(format!(
                        "git clone failed for {repo_name}: {stderr}"
                    )));
                }
                return Ok(dest);
            }
            Ok(None) => {
                // Still running — check cancel flag.
                if cancel.map_or(false, |c| c.load(std::sync::atomic::Ordering::Relaxed)) {
                    tracing::info!("Cancelling git clone for {repo_name}");
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&dest);
                    return Err(Error::Other("Cancelled".to_string()));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => {
                return Err(Error::Other(format!("Failed to wait for git: {e}")));
            }
        }
    }
}

/// Build a list of ROM filenames for a system from the filesystem.
pub fn list_rom_filenames(storage_root: &Path, system: &str) -> Vec<String> {
    let roms_dir = storage_root.join("roms").join(system);
    let mut filenames = Vec::new();
    collect_rom_filenames_recursive(&roms_dir, &mut filenames);
    filenames
}

fn collect_rom_filenames_recursive(dir: &Path, filenames: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with('_') {
                collect_rom_filenames_recursive(&path, filenames);
            }
        } else {
            filenames.push(entry.file_name().to_string_lossy().to_string());
        }
    }
}

/// Get the total size of the media directory for all systems.
pub fn media_dir_size(storage_root: &Path) -> u64 {
    let media_dir = storage_root
        .join(crate::metadata_db::RC_DIR)
        .join("media");
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

/// Delete all media files for all systems.
pub fn clear_media(storage_root: &Path) -> Result<()> {
    let media_dir = storage_root
        .join(crate::metadata_db::RC_DIR)
        .join("media");
    if media_dir.exists() {
        std::fs::remove_dir_all(&media_dir)
            .map_err(|e| Error::io(&media_dir, e))?;
    }
    Ok(())
}
