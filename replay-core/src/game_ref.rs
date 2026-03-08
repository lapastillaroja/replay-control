use serde::{Deserialize, Serialize};

use crate::arcade_db;
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
    /// Human-readable display name resolved from arcade DB.
    pub display_name: Option<String>,
    /// Path to the ROM file relative to the storage root
    pub rom_path: String,
}

impl GameRef {
    /// Create a new GameRef, resolving display_name from the arcade DB for arcade systems.
    pub fn new(system: &str, rom_filename: String, rom_path: String) -> Self {
        let sys_info = systems::find_system(system);

        let system_display = sys_info
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let display_name = sys_info
            .filter(|s| s.category == SystemCategory::Arcade)
            .and_then(|_| {
                let resolved = arcade_db::arcade_display_name(&rom_filename);
                if resolved != rom_filename {
                    Some(resolved.to_string())
                } else {
                    None
                }
            });

        Self {
            system: system.to_string(),
            system_display,
            rom_filename,
            display_name,
            rom_path,
        }
    }
}
