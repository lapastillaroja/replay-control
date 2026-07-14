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

/// Map our system IDs to retrokit folder names (the layout used both by the
/// retrokit-manuals Archive.org collection and by catalog-built manual URLs).
pub fn retrokit_folder_name(system: &str) -> Option<&'static str> {
    Some(match system {
        "nintendo_snes" => "snes",
        "nintendo_nes" => "nes",
        "nintendo_gb" => "gb",
        "nintendo_gba" => "gba",
        "nintendo_gbc" => "gbc",
        "nintendo_n64" => "n64",
        "nintendo_ds" => "nds",
        "sega_smd" => "megadrive",
        "sega_sms" => "mastersystem",
        "sega_gg" => "gamegear",
        "sega_32x" => "sega32x",
        "sega_cd" => "segacd",
        "sega_dc" => "dreamcast",
        "sega_st" => "saturn",
        "sega_sg" => "sg-1000",
        "sony_psx" => "psx",
        "nec_pce" => "pcengine",
        "nec_pcecd" => "pce-cd",
        "atari_2600" => "atari2600",
        "atari_5200" => "atari5200",
        "atari_7800" => "atari7800",
        "atari_jaguar" => "atarijaguar",
        "atari_lynx" => "atarilynx",
        "commodore_c64" => "c64",
        "commodore_ami" => "amiga",
        "snk_ng" => "neogeo",
        "snk_ngcd" => "neogeocd",
        "snk_ngp" => "ngp",
        "panasonic_3do" => "3do",
        "ibm_pc" | "scummvm" => "pc",
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc" | "arcade_stv" => {
            "arcade"
        }
        _ => return None,
    })
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
