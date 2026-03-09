/// Screen rotation for an arcade game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Horizontal,
    Vertical,
    Unknown,
}

/// Emulation driver status for an arcade game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    Working,
    Imperfect,
    Preliminary,
    Unknown,
}

/// Metadata for an arcade game ROM.
#[derive(Debug, Clone)]
pub struct ArcadeGameInfo {
    /// ROM zip filename without extension (e.g., "mslug6").
    pub rom_name: &'static str,
    /// Human-readable display name (e.g., "Metal Slug 6").
    pub display_name: &'static str,
    /// Release year (e.g., "2006"). May be empty if unknown.
    pub year: &'static str,
    /// Manufacturer/publisher (e.g., "Sega / SNK Playmore"). May be empty.
    pub manufacturer: &'static str,
    /// Maximum simultaneous players. 0 = unknown.
    pub players: u8,
    /// Screen orientation.
    pub rotation: Rotation,
    /// Driver emulation status.
    pub status: DriverStatus,
    /// Whether this ROM is a clone/variant of another game.
    pub is_clone: bool,
    /// Parent ROM name if this is a clone, empty otherwise.
    pub parent: &'static str,
    /// Genre/category (e.g., "Fighter / 2D"). May be empty.
    pub category: &'static str,
    /// Normalized genre from shared taxonomy (e.g., "Fighting"). May be empty.
    pub normalized_genre: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/arcade_db.rs"));

/// Look up arcade game metadata by ROM name (without `.zip` extension).
pub fn lookup_arcade_game(rom_name: &str) -> Option<&'static ArcadeGameInfo> {
    ARCADE_DB.get(rom_name)
}

/// Get the display name for a ROM filename, falling back to the filename itself.
///
/// Accepts filenames with or without the `.zip` extension.
pub fn arcade_display_name(filename: &str) -> &str {
    let rom_name = filename.strip_suffix(".zip").unwrap_or(filename);
    lookup_arcade_game(rom_name)
        .map(|info| info.display_name)
        .unwrap_or(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_known_game() {
        let info = lookup_arcade_game("mslug6").expect("mslug6 should exist in DB");
        assert_eq!(info.display_name, "Metal Slug 6");
        assert_eq!(info.year, "2006");
        assert!(!info.is_clone);
        assert!(info.parent.is_empty());
    }

    #[test]
    fn lookup_clone() {
        let info = lookup_arcade_game("capsnka").expect("capsnka should exist in DB");
        assert!(info.is_clone);
        assert_eq!(info.parent, "capsnk");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup_arcade_game("nonexistent_rom_xyz").is_none());
    }

    #[test]
    fn display_name_with_zip() {
        let name = arcade_display_name("mslug6.zip");
        assert_eq!(name, "Metal Slug 6");
    }

    #[test]
    fn display_name_without_zip() {
        let name = arcade_display_name("mslug6");
        assert_eq!(name, "Metal Slug 6");
    }

    #[test]
    fn display_name_fallback() {
        let name = arcade_display_name("unknown_game.zip");
        assert_eq!(name, "unknown_game.zip");
    }

    #[test]
    fn vertical_rotation_game() {
        // anmlbskt is Animal Basket which has ROT270 (vertical)
        let info = lookup_arcade_game("anmlbskt").expect("anmlbskt should exist");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[test]
    fn horizontal_rotation_game() {
        let info = lookup_arcade_game("crzytaxi").expect("crzytaxi should exist");
        assert_eq!(info.rotation, Rotation::Horizontal);
    }

    #[test]
    fn lookup_gdrom_game() {
        let info = lookup_arcade_game("ikaruga").expect("ikaruga should exist (GD-ROM game)");
        assert!(info.display_name.starts_with("Ikaruga"));
        assert_eq!(info.year, "2001");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[test]
    fn lookup_atomiswave_game() {
        let info = lookup_arcade_game("kofxi").expect("kofxi should exist (Atomiswave game)");
        assert_eq!(info.display_name, "The King of Fighters XI");
        assert_eq!(info.year, "2005");
    }

    // --- FBNeo / MAME 2003+ integration tests ---

    #[test]
    fn lookup_sf2_from_mame() {
        let info = lookup_arcade_game("sf2").expect("sf2 should exist (MAME current)");
        assert_eq!(
            info.display_name,
            "Street Fighter II: The World Warrior (World 910522)"
        );
        assert_eq!(info.year, "1991");
        assert_eq!(info.manufacturer, "Capcom");
        assert_eq!(info.players, 2);
        assert_eq!(info.rotation, Rotation::Horizontal);
        assert_eq!(info.status, DriverStatus::Working);
        assert!(!info.is_clone);
        assert_eq!(info.category, "Fighter / Versus");
    }

    #[test]
    fn lookup_pacman_clone() {
        let info = lookup_arcade_game("pacman").expect("pacman should exist (MAME 2003+)");
        assert_eq!(info.display_name, "Pac-Man (Midway)");
        assert_eq!(info.year, "1980");
        assert!(info.is_clone);
        assert_eq!(info.parent, "puckman");
        assert_eq!(info.category, "Maze / Collect");
    }

    #[test]
    fn lookup_dkong_vertical() {
        let info = lookup_arcade_game("dkong").expect("dkong should exist (MAME 2003+)");
        assert_eq!(info.display_name, "Donkey Kong (US set 1)");
        assert_eq!(info.year, "1981");
        assert_eq!(info.rotation, Rotation::Vertical);
        assert_eq!(info.category, "Platform / Run Jump");
    }

    #[test]
    fn lookup_fbneo_only_game() {
        // 3countba exists in FBNeo but not MAME 2003+ or MAME current
        let info = lookup_arcade_game("3countba").expect("3countba should exist (FBNeo-only)");
        assert_eq!(
            info.display_name,
            "3 Count Bout / Fire Suplex (NGM-043)"
        );
        assert_eq!(info.year, "1993");
        assert_eq!(info.manufacturer, "SNK");
        assert!(info.is_clone);
        assert_eq!(info.parent, "3countb");
        // FBNeo-only games have unknown rotation/status
        assert_eq!(info.rotation, Rotation::Unknown);
        assert_eq!(info.status, DriverStatus::Unknown);
    }

    // --- MAME current (0.285) integration tests ---

    #[test]
    fn lookup_mame_current_only_game() {
        // timecris (Time Crisis) exists in MAME current but not FBNeo or MAME 2003+
        let info =
            lookup_arcade_game("timecris").expect("timecris should exist (MAME current only)");
        assert_eq!(info.display_name, "Time Crisis (World, TS2 Ver.B)");
        assert_eq!(info.year, "1996");
        assert_eq!(info.manufacturer, "Namco");
        assert_eq!(info.players, 1);
        assert_eq!(info.rotation, Rotation::Horizontal);
        assert_eq!(info.status, DriverStatus::Imperfect);
        assert!(!info.is_clone);
    }

    #[test]
    fn lookup_mame_current_overrides_mame2003() {
        // 1941r1 exists in FBNeo and MAME current (but not MAME 2003+).
        // MAME current should override FBNeo, providing rotation and status.
        let info = lookup_arcade_game("1941r1").expect("1941r1 should exist");
        assert_eq!(info.display_name, "1941: Counter Attack (World)");
        assert_eq!(info.year, "1990");
        assert!(info.is_clone);
        assert_eq!(info.parent, "1941");
        // MAME current provides rotation and status data
        assert_eq!(info.rotation, Rotation::Vertical);
        assert_eq!(info.status, DriverStatus::Working);
    }

    #[test]
    fn lookup_mame_current_preserves_flycast() {
        // Flycast hand-curated entries should not be overridden by MAME current
        let info = lookup_arcade_game("ikaruga").expect("ikaruga should still be Flycast entry");
        assert!(info.display_name.starts_with("Ikaruga"));
        assert_eq!(info.year, "2001");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[test]
    fn mame_current_category_overlay() {
        // timecris should have a category from the current MAME catver.ini
        let info = lookup_arcade_game("timecris").expect("timecris should exist");
        assert!(
            !info.category.is_empty(),
            "timecris should have a category from catver-mame-current.ini"
        );
    }

    #[test]
    fn total_entry_count() {
        // After merging Flycast + FBNeo + MAME 2003+ + MAME current, we should have 25K+ entries
        let count = ARCADE_DB.len();
        assert!(
            count >= 25000,
            "Expected 25000+ entries, got {count}"
        );
    }
}
