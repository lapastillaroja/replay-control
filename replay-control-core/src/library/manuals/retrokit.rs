//! Retrokit manual wire types and the system-to-folder-name mapping shared
//! by manual storage paths and catalog-built manual URLs.

use serde::{Deserialize, Serialize};

/// A manual suggestion discovered via retrokit's manifests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualRecommendation {
    pub source: String,
    pub title: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub language: Option<String>,
    pub source_id: String,
}

/// Manuals source-folder name for a system (retrokit-manuals layout).
/// Reads the centralized [`crate::systems::System::manuals_folder`] field.
pub fn retrokit_folder_name(system: &str) -> Option<&'static str> {
    crate::systems::find_system(system).and_then(|sys| sys.manuals_folder)
}

/// Map our system IDs to manual folder names (for `<storage>/manuals/<folder>/`).
/// Same as retrokit_folder_name but used for local storage.
pub fn manual_folder_name(system: &str) -> &str {
    retrokit_folder_name(system).unwrap_or(system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrokit_folder_snes() {
        assert_eq!(retrokit_folder_name("nintendo_snes"), Some("snes"));
    }

    #[test]
    fn retrokit_folder_unknown() {
        assert_eq!(retrokit_folder_name("unknown_system"), None);
    }

    #[test]
    fn retrokit_folder_scummvm_maps_to_pc() {
        assert_eq!(retrokit_folder_name("scummvm"), Some("pc"));
    }

    #[test]
    fn retrokit_folder_stv_maps_to_arcade() {
        assert_eq!(retrokit_folder_name("arcade_stv"), Some("arcade"));
        assert_eq!(manual_folder_name("arcade_stv"), "arcade");
    }
}
