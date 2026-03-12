//! Image thumbnail support via libretro-thumbnails.
//!
//! Maps RePlayOS system folder names to libretro-thumbnails repo names,
//! normalizes filenames, and provides fuzzy matching utilities.

use std::path::Path;

use crate::error::{Error, Result};

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
        "sega_32x" => Some(&["Sega - 32X"]),
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

/// Strip parenthesized tags and trailing whitespace from a name for fuzzy matching.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` → `"Indiana Jones and the Fate of Atlantis"`
/// `"Dark Seed"` → `"Dark Seed"` (unchanged)
pub(crate) fn strip_tags(name: &str) -> &str {
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
pub fn strip_version(name: &str) -> &str {
    // Look for " v" followed by a digit, then optional digits/dots/spaces/underscores
    let bytes = name.as_bytes();
    let mut i = 0;
    let mut last_version_start = None;
    while i + 2 < bytes.len() {
        if bytes[i] == b' '
            && bytes[i + 1] == b'v'
            && bytes.get(i + 2).is_some_and(|b| b.is_ascii_digit())
        {
            // Check that everything after " v\d" is digits, dots, spaces, or underscores
            let rest = &bytes[i + 2..];
            if rest
                .iter()
                .all(|b| b.is_ascii_digit() || *b == b'.' || *b == b' ' || *b == b'_')
            {
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
    }

    #[test]
    fn thumbnail_kind_media_dir() {
        assert_eq!(ThumbnailKind::Boxart.media_dir(), "boxart");
        assert_eq!(ThumbnailKind::Snap.media_dir(), "snap");
    }
}
