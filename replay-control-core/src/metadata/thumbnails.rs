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

/// Strip parenthesized tags and trailing whitespace from a name for fuzzy matching.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` → `"Indiana Jones and the Fate of Atlantis"`
/// `"Dark Seed"` → `"Dark Seed"` (unchanged)
pub fn strip_tags(name: &str) -> &str {
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

/// Compute a lowercased base title for fuzzy image matching.
///
/// Handles tilde dual-names (`"Name1 ~ Name2"` → `"Name2"`), strips
/// parenthesized/bracketed tags, lowercases the result, and normalizes
/// trailing articles (`", The"` / `", A"` / `", An"`) to the front.
pub fn base_title(name: &str) -> String {
    let s = name.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(name);
    let lower = strip_tags(s).to_lowercase();
    for article in &[", the", ", an", ", a"] {
        if let Some(title) = lower.strip_suffix(article) {
            let art = &article[2..]; // skip ", "
            return format!("{art} {title}");
        }
    }
    lower
}

/// Quick check that a file is likely a real image (not a git fake-symlink text file).
pub fn is_valid_image(path: &Path) -> bool {
    path.metadata().map(|m| m.len() >= 200).unwrap_or(false)
}

/// Try to resolve a small file as a git fake-symlink artifact.
/// Reads its text content (a relative filename), checks if that file exists in
/// `parent_dir` and passes `is_valid_image()`. Returns the target filename on
/// success, `None` otherwise.
pub fn try_resolve_fake_symlink(path: &Path, parent_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let target_name = std::str::from_utf8(&bytes).ok()?.trim();
    if target_name.is_empty() || !target_name.ends_with(".png") {
        return None;
    }
    let target_path = parent_dir.join(target_name);
    if target_path.exists() && is_valid_image(&target_path) {
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
pub fn find_image_on_disk(
    media_base: &Path,
    kind: &str,
    rom_filename: &str,
) -> Option<String> {
    let kind_dir = media_base.join(kind);
    if !kind_dir.exists() {
        return None;
    }

    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    let stem = stem.strip_prefix("N64DD - ").unwrap_or(stem);
    let thumb_name = thumbnail_filename(stem);

    // 1. Exact match
    let exact = kind_dir.join(format!("{thumb_name}.png"));
    if exact.exists() {
        if is_valid_image(&exact) {
            return Some(format!("{kind}/{thumb_name}.png"));
        }
        if let Some(resolved) = try_resolve_fake_symlink(&exact, &kind_dir) {
            return Some(format!("{kind}/{resolved}"));
        }
    }

    let rom_base = base_title(&thumb_name);
    let rom_base_no_version = strip_version(&rom_base);
    let has_version = rom_base_no_version.len() < rom_base.len();
    let thumb_lower = thumb_name.to_lowercase();

    if let Ok(entries) = std::fs::read_dir(&kind_dir) {
        let mut fuzzy_result: Option<String> = None;
        let mut version_result: Option<String> = None;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(img_stem) = name.strip_suffix(".png") {
                // 1b. Case-insensitive exact match (preserves region tags)
                if img_stem.to_lowercase() == thumb_lower {
                    let path = entry.path();
                    if is_valid_image(&path) {
                        return Some(format!("{kind}/{name}"));
                    }
                    if let Some(resolved) = try_resolve_fake_symlink(&path, &kind_dir) {
                        return Some(format!("{kind}/{resolved}"));
                    }
                }

                let img_base = base_title(img_stem);
                // 2. Fuzzy match (strip tags)
                if img_base == rom_base && fuzzy_result.is_none() {
                    let path = entry.path();
                    if is_valid_image(&path) {
                        fuzzy_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink(&path, &kind_dir) {
                        fuzzy_result = Some(format!("{kind}/{resolved}"));
                    }
                }
                // 3. Version-stripped match
                if has_version && img_base == rom_base_no_version && version_result.is_none() {
                    let path = entry.path();
                    if is_valid_image(&path) {
                        version_result = Some(format!("{kind}/{name}"));
                    } else if let Some(resolved) = try_resolve_fake_symlink(&path, &kind_dir) {
                        version_result = Some(format!("{kind}/{resolved}"));
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
    }

    None
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
    }

    #[test]
    fn thumbnail_kind_media_dir() {
        assert_eq!(ThumbnailKind::Boxart.media_dir(), "boxart");
        assert_eq!(ThumbnailKind::Snap.media_dir(), "snap");
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

    /// Helper: simulate the full matching pipeline used by resolve_box_art.
    /// Returns (thumb_name, base_title) for a given ROM filename + system.
    fn resolve_pipeline(rom_filename: &str, system: &str) -> (String, String) {
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let is_arcade = matches!(
            system,
            "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
        );
        let display_name = if is_arcade {
            crate::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
        } else {
            None
        };
        let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));
        let base = strip_tags(&thumb_name).trim().to_lowercase();
        (thumb_name, base)
    }

    #[test]
    fn arcade_avsp_resolves_to_alien_vs_predator() {
        let (thumb_name, base) = resolve_pipeline("avsp.zip", "arcade_fbneo");
        assert!(
            thumb_name.starts_with("Alien vs. Predator"),
            "expected 'Alien vs. Predator...', got '{thumb_name}'"
        );
        assert_eq!(base, "alien vs. predator");
    }

    #[test]
    fn arcade_ffight_resolves_to_final_fight() {
        let (thumb_name, base) = resolve_pipeline("ffight.zip", "arcade_fbneo");
        assert!(
            thumb_name.starts_with("Final Fight"),
            "expected 'Final Fight...', got '{thumb_name}'"
        );
        assert_eq!(base, "final fight");
    }

    #[test]
    fn arcade_dsmbl_resolves_to_deathsmiles() {
        let (thumb_name, base) = resolve_pipeline("dsmbl.zip", "arcade_fbneo");
        assert!(
            thumb_name.starts_with("Deathsmiles MegaBlack Label"),
            "expected 'Deathsmiles MegaBlack Label...', got '{thumb_name}'"
        );
        assert_eq!(base, "deathsmiles megablack label");
    }

    #[test]
    fn arcade_dmnfrnt_resolves_with_slash_replaced() {
        let (thumb_name, _) = resolve_pipeline("dmnfrnt.zip", "arcade_fbneo");
        // Display name is "Demon Front / Moyu Zhanxian ..."
        // thumbnail_filename replaces '/' with '_'
        assert!(
            thumb_name.contains("Demon Front _ Moyu Zhanxian"),
            "expected slash replaced with underscore, got '{thumb_name}'"
        );
    }

    #[test]
    fn arcade_sf2_colon_in_display_name() {
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_fbneo");
        // "Street Fighter II: The World Warrior (...)"
        // thumbnail_filename replaces ':' with '_'
        assert!(
            thumb_name.contains("Street Fighter II_ The World Warrior"),
            "expected colon replaced with underscore, got '{thumb_name}'"
        );

        // Colon variant: ": " → " - "
        let info = crate::arcade_db::lookup_arcade_game("sf2").unwrap();
        let dash_variant =
            thumbnail_filename(&info.display_name.replace(": ", " - ").replace(':', " -"));
        assert!(
            dash_variant.contains("Street Fighter II - The World Warrior"),
            "expected dash variant, got '{dash_variant}'"
        );
    }

    #[test]
    fn arcade_unknown_rom_falls_back_to_stem() {
        let (thumb_name, base) = resolve_pipeline("zzz_nonexistent.zip", "arcade_fbneo");
        assert_eq!(thumb_name, "zzz_nonexistent");
        assert_eq!(base, "zzz_nonexistent");
    }

    #[test]
    fn non_arcade_system_no_arcade_db_lookup() {
        // For non-arcade systems, the stem is used directly
        let (thumb_name, base) =
            resolve_pipeline("Super Mario World (USA).sfc", "nintendo_snes");
        assert_eq!(thumb_name, "Super Mario World (USA)");
        assert_eq!(base, "super mario world");
    }

    #[test]
    fn arcade_mame_system_also_uses_arcade_db() {
        let (thumb_name, _) = resolve_pipeline("sf2.zip", "arcade_mame");
        assert!(
            thumb_name.contains("Street Fighter II"),
            "arcade_mame should also use arcade_db, got '{thumb_name}'"
        );
    }

    #[test]
    fn arcade_dc_system_uses_arcade_db() {
        let (thumb_name, _) = resolve_pipeline("ikaruga.zip", "arcade_dc");
        assert!(
            thumb_name.starts_with("Ikaruga"),
            "arcade_dc should use arcade_db, got '{thumb_name}'"
        );
    }

    #[test]
    fn n64dd_prefix_stripped() {
        // N64DD ROMs have a "N64DD - " prefix that should be stripped
        let stem = "N64DD - Mario Artist Paint Studio (Japan).n64";
        let stem = stem
            .rfind('.')
            .map(|i| &stem[..i])
            .unwrap_or(stem);
        let stem = stem.strip_prefix("N64DD - ").unwrap_or(stem);
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
}
