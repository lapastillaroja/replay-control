use serde::Serialize;

/// A supported RePlayOS system (console, arcade, computer, or handheld).
#[derive(Debug, Clone, Serialize)]
pub struct System {
    pub folder_name: &'static str,
    pub display_name: &'static str,
    pub manufacturer: &'static str,
    pub category: SystemCategory,
    pub extensions: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemCategory {
    Arcade,
    Console,
    Computer,
    Handheld,
    Utility,
}

/// All systems supported by RePlayOS, mapped from the actual folder names
/// found on the SD card / USB drive.
pub static SYSTEMS: &[System] = &[
    System {
        folder_name: "arcade_fbneo",
        display_name: "Arcade (FBNeo)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip"],
    },
    System {
        folder_name: "arcade_mame",
        display_name: "Arcade (MAME)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip"],
    },
    System {
        folder_name: "arcade_mame_2k3p",
        display_name: "Arcade (MAME 2003+)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip"],
    },
    System {
        folder_name: "arcade_dc",
        display_name: "Arcade (Atomiswave/Naomi)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip", "chd"],
    },
    System {
        folder_name: "atari_2600",
        display_name: "Atari 2600",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a26", "bin"],
    },
    System {
        folder_name: "atari_5200",
        display_name: "Atari 5200",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a52", "bin"],
    },
    System {
        folder_name: "atari_7800",
        display_name: "Atari 7800",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a78", "bin"],
    },
    System {
        folder_name: "atari_jaguar",
        display_name: "Atari Jaguar",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["j64", "jag", "rom", "abs"],
    },
    System {
        folder_name: "atari_lynx",
        display_name: "Atari Lynx",
        manufacturer: "Atari",
        category: SystemCategory::Handheld,
        extensions: &["lnx"],
    },
    System {
        folder_name: "amstrad_cpc",
        display_name: "Amstrad CPC",
        manufacturer: "Amstrad",
        category: SystemCategory::Computer,
        extensions: &["dsk", "sna", "tap", "cdt"],
    },
    System {
        folder_name: "commodore_ami",
        display_name: "Commodore Amiga",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["adf", "hdf", "ipf", "lha"],
    },
    System {
        folder_name: "commodore_amicd",
        display_name: "Commodore Amiga CD",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["iso", "cue", "chd"],
    },
    System {
        folder_name: "commodore_c64",
        display_name: "Commodore 64",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["d64", "t64", "tap", "prg", "crt"],
    },
    System {
        folder_name: "ibm_pc",
        display_name: "IBM PC (DOS)",
        manufacturer: "IBM",
        category: SystemCategory::Computer,
        extensions: &["zip", "exe", "com", "bat", "conf"],
    },
    System {
        folder_name: "microsoft_msx",
        display_name: "MSX",
        manufacturer: "Microsoft",
        category: SystemCategory::Computer,
        extensions: &["rom", "mx1", "mx2", "dsk"],
    },
    System {
        folder_name: "nec_pce",
        display_name: "PC Engine / TurboGrafx-16",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        extensions: &["pce", "sgx"],
    },
    System {
        folder_name: "nec_pcecd",
        display_name: "PC Engine CD",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        extensions: &["cue", "chd", "ccd"],
    },
    System {
        folder_name: "nintendo_ds",
        display_name: "Nintendo DS",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["nds"],
    },
    System {
        folder_name: "nintendo_gb",
        display_name: "Game Boy",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gb"],
    },
    System {
        folder_name: "nintendo_gba",
        display_name: "Game Boy Advance",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gba"],
    },
    System {
        folder_name: "nintendo_gbc",
        display_name: "Game Boy Color",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gbc"],
    },
    System {
        folder_name: "nintendo_n64",
        display_name: "Nintendo 64",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["z64", "n64", "v64"],
    },
    System {
        folder_name: "nintendo_nes",
        display_name: "Nintendo Entertainment System",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["nes", "unif", "unf"],
    },
    System {
        folder_name: "nintendo_snes",
        display_name: "Super Nintendo",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["smc", "sfc"],
    },
    System {
        folder_name: "panasonic_3do",
        display_name: "3DO",
        manufacturer: "Panasonic",
        category: SystemCategory::Console,
        extensions: &["iso", "chd", "cue"],
    },
    System {
        folder_name: "philips_cdi",
        display_name: "Philips CD-i",
        manufacturer: "Philips",
        category: SystemCategory::Console,
        extensions: &["chd", "iso", "cue"],
    },
    System {
        folder_name: "scummvm",
        display_name: "ScummVM",
        manufacturer: "Various",
        category: SystemCategory::Computer,
        extensions: &["scummvm"],
    },
    System {
        folder_name: "sega_32x",
        display_name: "Sega 32X",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["32x", "bin"],
    },
    System {
        folder_name: "sega_cd",
        display_name: "Sega CD / Mega-CD",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "iso"],
    },
    System {
        folder_name: "sega_dc",
        display_name: "Sega Dreamcast",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["gdi", "chd", "cdi"],
    },
    System {
        folder_name: "sega_gg",
        display_name: "Sega Game Gear",
        manufacturer: "Sega",
        category: SystemCategory::Handheld,
        extensions: &["gg"],
    },
    System {
        folder_name: "sega_sg",
        display_name: "Sega SG-1000",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["sg"],
    },
    System {
        folder_name: "sega_smd",
        display_name: "Sega Mega Drive / Genesis",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["md", "bin", "gen", "smd"],
    },
    System {
        folder_name: "sega_sms",
        display_name: "Sega Master System",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["sms"],
    },
    System {
        folder_name: "sega_st",
        display_name: "Sega Saturn",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "iso"],
    },
    System {
        folder_name: "sharp_x68k",
        display_name: "Sharp X68000",
        manufacturer: "Sharp",
        category: SystemCategory::Computer,
        extensions: &["dim", "hdf", "m3u"],
    },
    System {
        folder_name: "sinclair_zx",
        display_name: "ZX Spectrum",
        manufacturer: "Sinclair",
        category: SystemCategory::Computer,
        extensions: &["tzx", "tap", "z80", "sna"],
    },
    System {
        folder_name: "snk_ng",
        display_name: "Neo Geo",
        manufacturer: "SNK",
        category: SystemCategory::Console,
        extensions: &["zip"],
    },
    System {
        folder_name: "snk_ngcd",
        display_name: "Neo Geo CD",
        manufacturer: "SNK",
        category: SystemCategory::Console,
        extensions: &["chd", "cue"],
    },
    System {
        folder_name: "snk_ngp",
        display_name: "Neo Geo Pocket",
        manufacturer: "SNK",
        category: SystemCategory::Handheld,
        extensions: &["ngp", "ngc"],
    },
    System {
        folder_name: "sony_psx",
        display_name: "PlayStation",
        manufacturer: "Sony",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "bin", "img", "pbp"],
    },
    System {
        folder_name: "alpha_player",
        display_name: "Alpha Player",
        manufacturer: "RePlayOS",
        category: SystemCategory::Utility,
        extensions: &["mkv", "avi", "mp4", "mp3", "flac", "ogg"],
    },
];

/// Look up a system by its folder name.
pub fn find_system(folder_name: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.folder_name == folder_name)
}

/// Extract the system folder name from a favorite/recent filename.
/// E.g., "sega_smd@Sonic.md.fav" → "sega_smd"
pub fn system_from_fav_filename(filename: &str) -> Option<&str> {
    filename.split('@').next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_known_system() {
        let sys = find_system("nintendo_nes").unwrap();
        assert_eq!(sys.display_name, "Nintendo Entertainment System");
        assert_eq!(sys.category, SystemCategory::Console);
    }

    #[test]
    fn find_unknown_system() {
        assert!(find_system("unknown_system").is_none());
    }

    #[test]
    fn parse_system_from_fav() {
        assert_eq!(
            system_from_fav_filename("sega_smd@Sonic.md.fav"),
            Some("sega_smd")
        );
        assert_eq!(
            system_from_fav_filename("arcade_fbneo@ffight.zip.fav"),
            Some("arcade_fbneo")
        );
    }
}
