use serde::{Deserialize, Serialize};

use crate::arcade_db;
use crate::game_db;
use crate::rom_tags;
use crate::systems::{self, SystemCategory};

/// A reference to a game — the common identity shared across ROM listings,
/// favorites, and recents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRef {
    /// System folder name (e.g., "nintendo_nes")
    pub system: String,
    /// Display name of the system (e.g., "Nintendo Entertainment System")
    pub system_display: String,
    /// ROM filename (e.g., "Super Mario Bros (USA).nes")
    pub rom_filename: String,
    /// Human-readable display name resolved from arcade DB or game DB.
    pub display_name: Option<String>,
    /// Path to the ROM file relative to the storage root
    pub rom_path: String,
}

/// Strip parenthesized/bracketed tags from a filename stem.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` → `"Indiana Jones and the Fate of Atlantis"`
fn strip_filename_tags(stem: &str) -> &str {
    stem.find(" (")
        .or_else(|| stem.find(" ["))
        .map(|i| stem[..i].trim())
        .unwrap_or(stem)
}

impl GameRef {
    /// Create a new GameRef, resolving display_name from the appropriate DB.
    ///
    /// For arcade systems, uses the arcade DB (zip filename lookup).
    /// For non-arcade systems with game DB coverage, uses the game DB
    /// (No-Intro filename lookup).
    pub fn new(system: &str, rom_filename: String, rom_path: String) -> Self {
        let sys_info = systems::find_system(system);

        let system_display = sys_info
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let display_name = if sys_info.is_some_and(|s| s.category == SystemCategory::Arcade) {
            // Arcade: use arcade DB (zip filename without extension)
            let resolved = arcade_db::arcade_display_name(&rom_filename);
            if resolved != rom_filename {
                Some(resolved.to_string())
            } else {
                None
            }
        } else {
            // Non-arcade: try game DB first, then append useful filename tags.
            // Fall back to deriving a clean name from the filename for systems
            // without game DB coverage (e.g., ibm_pc, commodore_ami) or unknown systems.
            game_db::game_display_name(system, &rom_filename)
                .map(|name| rom_tags::display_name_with_tags(name, &rom_filename))
                .or_else(|| {
                    let stem = rom_filename
                        .rfind('.')
                        .map(|i| &rom_filename[..i])
                        .unwrap_or(&rom_filename);
                    // Always return a display name from the stem (without extension).
                    // Use the tag-stripped base as the display name, or the full
                    // stem if there are no tags to strip.
                    let base = strip_filename_tags(stem);
                    let name = if base.is_empty() { stem } else { base };
                    Some(rom_tags::display_name_with_tags(name, &rom_filename))
                })
        };

        Self {
            system: system.to_string(),
            system_display,
            rom_filename,
            display_name,
            rom_path,
        }
    }
}
