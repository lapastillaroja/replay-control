/// Embedded game metadata database for non-arcade systems.
///
/// Provides canonical game titles and metadata for cartridge-based systems
/// by embedding No-Intro DAT data cross-referenced with TheGamesDB and
/// libretro-database metadata at build time.
///
/// The data model uses a two-level structure:
/// - `CanonicalGame`: one per unique game, holds shared metadata
/// - `GameEntry`: one per ROM filename variant, references a CanonicalGame

/// Shared metadata for a canonical game (one per unique game title per system).
#[derive(Debug, Clone)]
pub struct CanonicalGame {
    /// Clean display title (e.g., "Super Mario World").
    pub display_name: &'static str,
    /// Release year (0 = unknown).
    pub year: u16,
    /// Genre/category (e.g., "Platform"). Empty if unknown.
    pub genre: &'static str,
    /// Developer name (e.g., "Nintendo"). Empty if unknown.
    pub developer: &'static str,
    /// Max players (0 = unknown).
    pub players: u8,
    /// Normalized genre from shared taxonomy (e.g., "Platform"). Empty if unknown.
    pub normalized_genre: &'static str,
}

/// Metadata for a specific ROM file variant.
#[derive(Debug, Clone)]
pub struct GameEntry {
    /// Canonical filename stem from No-Intro (without extension).
    pub canonical_name: &'static str,
    /// Region code (e.g., "USA", "Europe", "Japan"). Empty if unknown.
    pub region: &'static str,
    /// CRC32 of the ROM file.
    pub crc32: u32,
    /// Reference to the shared canonical game metadata.
    pub game: &'static CanonicalGame,
}

// Include the build-generated game database code.
// This provides per-system PHF maps and canonical game tables.
include!(concat!(env!("OUT_DIR"), "/game_db.rs"));

/// Look up game metadata by filename stem (without extension) for a system.
///
/// The filename stem should match the No-Intro canonical naming convention,
/// e.g., `"Super Mario World (USA)"` (not `"Super Mario World (USA).sfc"`).
pub fn lookup_game(system: &str, filename_stem: &str) -> Option<&'static GameEntry> {
    get_system_db(system).and_then(|db| db.get(filename_stem))
}

/// Look up game metadata by CRC32 hash for a system.
///
/// Falls back to CRC32-based identification when filename matching fails
/// (e.g., for ROMs with non-standard filenames).
pub fn lookup_by_crc(system: &str, crc32: u32) -> Option<&'static GameEntry> {
    let crc_index = get_system_crc_index(system)?;
    let filename_stem = crc_index.get(&crc32)?;
    lookup_game(system, filename_stem)
}

/// Look up a canonical game by normalized title for a system.
///
/// This is used as a fallback when exact filename matching fails.
/// The normalized title strips parenthesized tags, lowercases, removes
/// punctuation, and collapses whitespace.
pub fn lookup_by_normalized_title(
    system: &str,
    normalized: &str,
) -> Option<&'static CanonicalGame> {
    let norm_index = get_system_norm_index(system)?;
    let game_id = norm_index.get(normalized)?;
    let games = get_system_games(system)?;
    games.get(*game_id as usize)
}

/// Normalize a ROM filename stem for fuzzy title matching.
///
/// This mirrors the `normalize_title()` function used at build time to generate
/// the normalized title index. The normalization:
/// 1. Strips everything from the first `(` or `[` onward (removes tags)
/// 2. Lowercases
/// 3. Removes non-alphanumeric characters (except spaces)
/// 4. Collapses whitespace
///
/// Examples:
/// - `"Super Mario World (USA)"` -> `"super mario world"`
/// - `"Legend of Zelda, The (USA) (Rev 1)"` -> `"legend of zelda the"`
/// - `"Sonic The Hedgehog (USA, Europe) (Traducido Es)"` -> `"sonic the hedgehog"`
pub fn normalize_filename(stem: &str) -> String {
    // Strip everything from the first '(' or '[' onward
    let base = stem
        .find(|c| c == '(' || c == '[')
        .map(|i| &stem[..i])
        .unwrap_or(stem)
        .trim();

    // Lowercase, keep only alphanumeric and spaces
    let mut result = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }

    // Collapse whitespace
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

/// Get the display name for a ROM file on a given system.
///
/// Uses a three-step fallback chain:
/// 1. Exact filename stem match (No-Intro canonical name)
/// 2. Normalized title fallback (strips tags, lowercases, fuzzy match)
/// 3. Tilde-split fallback (for multi-title entries like "Title A ~ Title B")
///
/// Note: CRC32 fallback is available via `lookup_by_crc()` but not called here
/// because it requires file I/O to compute the hash, which is outside the scope
/// of filename-based resolution. Callers with file access can use it separately.
pub fn game_display_name(system: &str, filename: &str) -> Option<&'static str> {
    // Strip extension to get the filename stem
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    // 1. Exact stem match (current behavior, fastest path)
    if let Some(entry) = lookup_game(system, stem) {
        return Some(entry.game.display_name);
    }

    // 2. Normalized title fallback
    let normalized = normalize_filename(stem);
    if !normalized.is_empty() {
        if let Some(game) = lookup_by_normalized_title(system, &normalized) {
            return Some(game.display_name);
        }
    }

    // 3. Tilde-split fallback: try each segment of "Title A ~ Title B" separately
    if stem.contains('~') {
        for part in stem.split('~') {
            let part_normalized = normalize_filename(part.trim());
            if !part_normalized.is_empty() {
                if let Some(game) = lookup_by_normalized_title(system, &part_normalized) {
                    return Some(game.display_name);
                }
            }
        }
    }

    None
}

/// Systems that have game DB coverage.
pub fn supported_systems() -> &'static [&'static str] {
    GAME_DB_SYSTEMS
}

/// Check if a system has game DB coverage.
pub fn has_system(system: &str) -> bool {
    GAME_DB_SYSTEMS.contains(&system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_systems_not_empty() {
        let systems = supported_systems();
        assert!(
            !systems.is_empty(),
            "Game DB should have at least one supported system"
        );
        assert!(systems.contains(&"nintendo_nes"));
        assert!(systems.contains(&"nintendo_snes"));
        assert!(systems.contains(&"sega_smd"));
    }

    #[test]
    fn has_system_checks() {
        assert!(has_system("nintendo_nes"));
        assert!(has_system("sega_smd"));
        assert!(!has_system("arcade_fbneo")); // Arcade uses arcade_db, not game_db
        assert!(!has_system("nonexistent_system"));
    }

    #[test]
    fn lookup_nonexistent_system() {
        assert!(lookup_game("nonexistent_system", "anything").is_none());
    }

    #[test]
    fn lookup_nonexistent_rom() {
        assert!(lookup_game("nintendo_nes", "nonexistent_rom_xyz_123").is_none());
    }

    // --- NES tests ---

    #[test]
    fn lookup_nes_super_mario_bros() {
        let entry = lookup_game("nintendo_nes", "Super Mario Bros. (World)")
            .expect("Super Mario Bros. should exist in NES DB");
        assert_eq!(entry.game.display_name, "Super Mario Bros.");
        assert_eq!(entry.region, "World");
        assert!(entry.crc32 != 0, "CRC32 should be set");
    }

    #[test]
    fn lookup_nes_zelda() {
        let entry = lookup_game("nintendo_nes", "Legend of Zelda, The (USA)")
            .expect("Zelda should exist in NES DB");
        assert_eq!(entry.game.display_name, "The Legend of Zelda");
        assert_eq!(entry.region, "USA");
    }

    #[test]
    fn nes_display_name_with_extension() {
        let name = game_display_name("nintendo_nes", "Super Mario Bros. (World).nes");
        assert_eq!(name, Some("Super Mario Bros."));
    }

    // --- SNES tests ---

    #[test]
    fn lookup_snes_super_mario_world() {
        let entry = lookup_game("nintendo_snes", "Super Mario World (USA)")
            .expect("Super Mario World should exist in SNES DB");
        assert_eq!(entry.game.display_name, "Super Mario World");
        assert_eq!(entry.region, "USA");
    }

    #[test]
    fn snes_zelda_link_to_past() {
        let entry = lookup_game(
            "nintendo_snes",
            "Legend of Zelda, The - A Link to the Past (USA)",
        )
        .expect("Zelda ALTTP should exist in SNES DB");
        assert_eq!(
            entry.game.display_name,
            "The Legend of Zelda - A Link to the Past"
        );
    }

    // --- Canonical game grouping tests ---

    #[test]
    fn canonical_game_shared_across_regions() {
        // USA and Europe versions of the same game should share a canonical game
        let usa = lookup_game("nintendo_snes", "Super Mario World (USA)");
        let eur = lookup_game("nintendo_snes", "Super Mario World (Europe)");
        if let (Some(usa), Some(eur)) = (usa, eur) {
            assert_eq!(
                usa.game.display_name, eur.game.display_name,
                "USA and Europe variants should share the same display name"
            );
            // They should point to the same CanonicalGame instance
            assert!(
                std::ptr::eq(usa.game, eur.game),
                "USA and Europe variants should point to the same CanonicalGame"
            );
        }
    }

    // --- Mega Drive/Genesis tests ---

    #[test]
    fn lookup_smd_sonic() {
        let entry = lookup_game("sega_smd", "Sonic The Hedgehog (USA, Europe)")
            .expect("Sonic should exist in SMD DB");
        assert_eq!(entry.game.display_name, "Sonic The Hedgehog");
    }

    // --- Game Boy tests ---

    #[test]
    fn lookup_gb_tetris() {
        // No-Intro has "Tetris (World) (Rev 1)" and "Tetris (Japan) (En)"
        let entry = lookup_game("nintendo_gb", "Tetris (World) (Rev 1)")
            .or_else(|| lookup_game("nintendo_gb", "Tetris (Japan) (En)"))
            .expect("Tetris should exist in GB DB");
        assert_eq!(entry.game.display_name, "Tetris");
    }

    // --- CRC32 lookup tests ---

    #[test]
    fn lookup_by_crc_nes() {
        // Look up Super Mario Bros by its known CRC32
        let entry =
            lookup_game("nintendo_nes", "Super Mario Bros. (World)").expect("SMB should exist");
        let crc = entry.crc32;
        assert!(crc != 0);
        let by_crc =
            lookup_by_crc("nintendo_nes", crc).expect("CRC32 lookup should find Super Mario Bros.");
        assert_eq!(by_crc.game.display_name, "Super Mario Bros.");
    }

    // --- Metadata coverage tests ---

    #[test]
    fn nes_has_genre_data() {
        let entry =
            lookup_game("nintendo_nes", "Super Mario Bros. (World)").expect("SMB should exist");
        // Genre should be populated from libretro-database or TGDB
        assert!(
            !entry.game.genre.is_empty(),
            "Super Mario Bros. should have genre data, got empty"
        );
    }

    #[test]
    fn snes_has_players_data() {
        let entry =
            lookup_game("nintendo_snes", "Super Mario World (USA)").expect("SMW should exist");
        assert!(
            entry.game.players > 0,
            "Super Mario World should have players data"
        );
    }

    #[test]
    fn snes_has_year_data() {
        let entry =
            lookup_game("nintendo_snes", "Super Mario World (USA)").expect("SMW should exist");
        assert!(
            entry.game.year > 0,
            "Super Mario World should have a release year"
        );
    }

    // --- Normalized title fallback tests ---

    #[test]
    fn normalize_filename_strips_tags() {
        assert_eq!(
            normalize_filename("Super Mario World (USA)"),
            "super mario world"
        );
        assert_eq!(
            normalize_filename("Super Mario World (USA) (Rev 1)"),
            "super mario world"
        );
        assert_eq!(
            normalize_filename("Sonic The Hedgehog (USA, Europe)"),
            "sonic the hedgehog"
        );
    }

    #[test]
    fn normalize_filename_strips_punctuation() {
        assert_eq!(
            normalize_filename("Legend of Zelda, The (USA)"),
            "legend of zelda the"
        );
        assert_eq!(
            normalize_filename("Super Mario Bros. (World)"),
            "super mario bros"
        );
    }

    #[test]
    fn normalize_filename_handles_brackets() {
        assert_eq!(
            normalize_filename("Game Name [T-Spa1.0v] (USA)"),
            "game name"
        );
        assert_eq!(normalize_filename("Game Name [!] (USA)"), "game name");
    }

    #[test]
    fn normalize_filename_handles_bare_names() {
        // & is not alphanumeric so it's stripped, whitespace is collapsed
        assert_eq!(
            normalize_filename("Battletoads & Double Dragon"),
            "battletoads double dragon"
        );
        assert_eq!(normalize_filename("Doom Troopers"), "doom troopers");
    }

    #[test]
    fn normalize_filename_collapses_whitespace() {
        assert_eq!(normalize_filename("  Game   Name  (USA)  "), "game name");
    }

    #[test]
    fn normalized_fallback_finds_game_with_extra_tags() {
        // This simulates a ROM file with translation tags that wouldn't exact-match
        // but should match via normalized title fallback.
        // "Super Mario World (USA) (Traducido Es).smc" -> stem -> normalized -> "super mario world"
        let name = game_display_name(
            "nintendo_snes",
            "Super Mario World (USA) (Traducido Es).smc",
        );
        assert_eq!(name, Some("Super Mario World"));
    }

    #[test]
    fn normalized_fallback_finds_game_with_60hz_tag() {
        // "Super Mario World (Europe) (60hz).sfc" should match via normalized fallback
        let name = game_display_name("nintendo_snes", "Super Mario World (Europe) (60hz).sfc");
        assert_eq!(name, Some("Super Mario World"));
    }

    #[test]
    fn normalized_fallback_finds_game_with_fastrom_tag() {
        // "Super Mario World (USA) (FastRom).sfc" should match via normalized fallback
        let name = game_display_name("nintendo_snes", "Super Mario World (USA) (FastRom).sfc");
        assert_eq!(name, Some("Super Mario World"));
    }

    #[test]
    fn normalized_fallback_finds_bare_filename() {
        // A file with no tags at all, just the game title
        let name = game_display_name("nintendo_snes", "Super Mario World.sfc");
        assert_eq!(name, Some("Super Mario World"));
    }

    #[test]
    fn exact_match_still_preferred_over_normalized() {
        // Exact match should still work and be used first
        let name = game_display_name("nintendo_snes", "Super Mario World (USA).sfc");
        assert_eq!(name, Some("Super Mario World"));
    }

    #[test]
    fn normalized_lookup_nonexistent_game() {
        // A game that truly doesn't exist in the DB should still return None
        let name = game_display_name(
            "nintendo_snes",
            "Totally Fake Game That Does Not Exist (USA).sfc",
        );
        assert!(name.is_none());
    }

    #[test]
    fn lookup_by_normalized_title_smd() {
        // Sonic should be findable by normalized title
        let game = lookup_by_normalized_title("sega_smd", "sonic the hedgehog");
        assert!(
            game.is_some(),
            "Sonic should be findable by normalized title"
        );
        assert_eq!(game.unwrap().display_name, "Sonic The Hedgehog");
    }

    // --- Total entry count test ---

    #[test]
    fn total_entry_count() {
        // We should have a substantial number of entries across all systems.
        // Build output shows ~34K ROM entries but some are deduplicated (same filename
        // stem, different ROM format), so the PHF maps contain fewer.
        // This is a smoke test to catch data loading failures.
        let mut total = 0usize;
        for system in supported_systems() {
            if let Some(db) = get_system_db(system) {
                total += db.len();
            }
        }
        assert!(
            total >= 20000,
            "Expected 20000+ total ROM entries across all systems, got {total}"
        );
    }
}
