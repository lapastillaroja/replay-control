//! Image thumbnail support via libretro-thumbnails.
//!
//! Downloads box art and screenshots from the libretro-thumbnails GitHub repos
//! and copies matching images to `<storage>/.replay-control/media/<system>/`.

use std::path::Path;

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
pub fn thumbnail_repo_name(system: &str) -> Option<&'static str> {
    match system {
        "atari_2600" => Some("Atari - 2600"),
        "atari_5200" => Some("Atari - 5200"),
        "atari_7800" => Some("Atari - 7800 ProSystem"),
        "atari_jaguar" => Some("Atari - Jaguar"),
        "atari_lynx" => Some("Atari - Lynx"),
        "amstrad_cpc" => Some("Amstrad - CPC"),
        "commodore_ami" => Some("Commodore - Amiga"),
        "commodore_c64" => Some("Commodore - 64"),
        "ibm_pc" => Some("DOS"),
        "microsoft_msx" => Some("Microsoft - MSX"),
        "nec_pce" => Some("NEC - PC Engine - TurboGrafx 16"),
        "nec_pcecd" => Some("NEC - PC Engine CD - TurboGrafx-CD"),
        "nintendo_ds" => Some("Nintendo - Nintendo DS"),
        "nintendo_gb" => Some("Nintendo - Game Boy"),
        "nintendo_gba" => Some("Nintendo - Game Boy Advance"),
        "nintendo_gbc" => Some("Nintendo - Game Boy Color"),
        "nintendo_n64" => Some("Nintendo - Nintendo 64"),
        "nintendo_nes" => Some("Nintendo - Nintendo Entertainment System"),
        "nintendo_snes" => Some("Nintendo - Super Nintendo Entertainment System"),
        "panasonic_3do" => Some("The 3DO Company - 3DO"),
        "philips_cdi" => Some("Philips - CDi"),
        "sega_32x" => Some("Sega - 32X"),
        "sega_cd" => Some("Sega - Mega-CD - Sega CD"),
        "sega_dc" => Some("Sega - Dreamcast"),
        "sega_gg" => Some("Sega - Game Gear"),
        "sega_sg" => Some("Sega - SG-1000"),
        "sega_smd" => Some("Sega - Mega Drive - Genesis"),
        "sega_sms" => Some("Sega - Master System - Mark III"),
        "sega_st" => Some("Sega - Saturn"),
        "sharp_x68k" => Some("Sharp - X68000"),
        "sinclair_zx" => Some("Sinclair - ZX Spectrum"),
        "snk_ng" => Some("SNK - Neo Geo"),
        "snk_ngcd" => Some("SNK - Neo Geo CD"),
        "snk_ngp" => Some("SNK - Neo Geo Pocket"),
        "sony_psx" => Some("Sony - PlayStation"),
        // Arcade systems use progetto-SNAPS, not libretro-thumbnails
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

/// Build a fuzzy index from a repo image directory.
/// Maps lowercase stripped base names to the actual filenames (without .png).
fn build_fuzzy_index(dir: &Path) -> std::collections::HashMap<String, String> {
    let mut index = std::collections::HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return index,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".png") {
            let key = strip_tags(stem).to_lowercase();
            // Only keep the first match per key (avoid collisions).
            index.entry(key).or_insert_with(|| stem.to_string());
        }
    }
    index
}

/// Try to find a thumbnail file: first exact match, then fuzzy (stripped tags).
fn find_thumbnail(
    repo_subdir: &Path,
    thumb_name: &str,
    fuzzy_index: &std::collections::HashMap<String, String>,
) -> Option<(std::path::PathBuf, String)> {
    // 1. Exact match
    let exact = repo_subdir.join(format!("{thumb_name}.png"));
    if exact.exists() {
        return Some((exact, thumb_name.to_string()));
    }

    // 2. Fuzzy: strip tags from ROM stem, look up in index
    let key = strip_tags(thumb_name).to_lowercase();
    if let Some(repo_stem) = fuzzy_index.get(&key) {
        let path = repo_subdir.join(format!("{repo_stem}.png"));
        if path.exists() {
            return Some((path, repo_stem.clone()));
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
    mut on_progress: impl FnMut(usize, usize),
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

    for (i, rom_filename) in rom_filenames.iter().enumerate() {
        let stem = match rom_filename.rfind('.') {
            Some(i) => &rom_filename[..i],
            None => rom_filename,
        };
        let thumb_name = thumbnail_filename(stem);

        let mut boxart_rel: Option<String> = None;
        let mut snap_rel: Option<String> = None;

        // Try boxart (exact then fuzzy)
        if let Some((src, matched_stem)) = find_thumbnail(&boxart_repo_dir, &thumb_name, &boxart_index) {
            let dst_name = format!("{matched_stem}.png");
            let dst = boxart_dir.join(&dst_name);
            if !dst.exists() {
                if let Err(e) = std::fs::copy(&src, &dst) {
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

        // Try snap (exact then fuzzy)
        if let Some((src, matched_stem)) = find_thumbnail(&snap_repo_dir, &thumb_name, &snap_index) {
            let dst_name = format!("{matched_stem}.png");
            let dst = snap_dir.join(&dst_name);
            if !dst.exists() {
                if let Err(e) = std::fs::copy(&src, &dst) {
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

        if (i + 1) % 100 == 0 {
            on_progress(i + 1, stats.boxart_copied + stats.snap_copied);
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
pub fn clone_thumbnail_repo(repo_name: &str, clone_base: Option<&Path>) -> Result<std::path::PathBuf> {
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
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &url, &dest.to_string_lossy()])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "git clone failed for {repo_name}: {stderr}"
        )));
    }

    Ok(dest)
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
