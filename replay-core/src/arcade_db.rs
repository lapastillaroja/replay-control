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
}
