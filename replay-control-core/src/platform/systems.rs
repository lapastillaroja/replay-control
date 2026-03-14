use serde::Serialize;

/// A supported RePlayOS system (console, arcade, computer, or handheld).
#[derive(Debug, Clone, Serialize)]
pub struct System {
    pub folder_name: &'static str,
    pub display_name: &'static str,
    pub manufacturer: &'static str,
    pub category: SystemCategory,
    pub extensions: &'static [&'static str],
    /// LaunchBox platform names that map to this system folder.
    /// Used by the LaunchBox XML import to match games to systems.
    /// Empty slice means no LaunchBox mapping (system won't get LaunchBox metadata).
    #[serde(skip)]
    pub launchbox_platforms: &'static [&'static str],
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
        launchbox_platforms: &["Arcade"],
    },
    System {
        folder_name: "arcade_mame",
        display_name: "Arcade (MAME)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip"],
        launchbox_platforms: &["Arcade"],
    },
    System {
        folder_name: "arcade_mame_2k3p",
        display_name: "Arcade (MAME 2003+)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip"],
        launchbox_platforms: &["Arcade"],
    },
    System {
        folder_name: "arcade_dc",
        display_name: "Arcade (Atomiswave/Naomi)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        extensions: &["zip", "chd"],
        launchbox_platforms: &["Sammy Atomiswave", "Sega Naomi", "Sega Naomi 2"],
    },
    System {
        folder_name: "atari_2600",
        display_name: "Atari 2600",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a26", "bin"],
        launchbox_platforms: &["Atari 2600"],
    },
    System {
        folder_name: "atari_5200",
        display_name: "Atari 5200",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a52", "bin"],
        launchbox_platforms: &["Atari 5200"],
    },
    System {
        folder_name: "atari_7800",
        display_name: "Atari 7800",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["a78", "bin"],
        launchbox_platforms: &["Atari 7800"],
    },
    System {
        folder_name: "atari_jaguar",
        display_name: "Atari Jaguar",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        extensions: &["j64", "jag", "rom", "abs"],
        launchbox_platforms: &["Atari Jaguar"],
    },
    System {
        folder_name: "atari_lynx",
        display_name: "Atari Lynx",
        manufacturer: "Atari",
        category: SystemCategory::Handheld,
        extensions: &["lnx"],
        launchbox_platforms: &["Atari Lynx"],
    },
    System {
        folder_name: "amstrad_cpc",
        display_name: "Amstrad CPC",
        manufacturer: "Amstrad",
        category: SystemCategory::Computer,
        extensions: &["dsk", "sna", "tap", "cdt"],
        launchbox_platforms: &["Amstrad CPC"],
    },
    System {
        folder_name: "commodore_ami",
        display_name: "Commodore Amiga",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["adf", "hdf", "ipf", "lha"],
        launchbox_platforms: &["Commodore Amiga"],
    },
    System {
        folder_name: "commodore_amicd",
        display_name: "Commodore Amiga CD",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["iso", "cue", "chd"],
        launchbox_platforms: &[],
    },
    System {
        folder_name: "commodore_c64",
        display_name: "Commodore 64",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        extensions: &["d64", "t64", "tap", "prg", "crt"],
        launchbox_platforms: &["Commodore 64"],
    },
    System {
        folder_name: "ibm_pc",
        display_name: "IBM PC (DOS)",
        manufacturer: "IBM",
        category: SystemCategory::Computer,
        extensions: &["zip", "exe", "com", "bat", "conf"],
        launchbox_platforms: &["MS-DOS"],
    },
    System {
        folder_name: "microsoft_msx",
        display_name: "MSX",
        manufacturer: "Microsoft",
        category: SystemCategory::Computer,
        extensions: &["rom", "mx1", "mx2", "dsk"],
        launchbox_platforms: &["Microsoft MSX", "Microsoft MSX2"],
    },
    System {
        folder_name: "nec_pce",
        display_name: "PC Engine / TurboGrafx-16",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        extensions: &["pce", "sgx"],
        launchbox_platforms: &["NEC TurboGrafx-16", "NEC PC Engine"],
    },
    System {
        folder_name: "nec_pcecd",
        display_name: "PC Engine CD",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        extensions: &["cue", "chd", "ccd"],
        launchbox_platforms: &["NEC TurboGrafx-CD", "NEC PC Engine CD-ROM"],
    },
    System {
        folder_name: "nintendo_ds",
        display_name: "Nintendo DS",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["nds"],
        launchbox_platforms: &["Nintendo DS"],
    },
    System {
        folder_name: "nintendo_gb",
        display_name: "Game Boy",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gb"],
        launchbox_platforms: &["Nintendo Game Boy"],
    },
    System {
        folder_name: "nintendo_gba",
        display_name: "Game Boy Advance",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gba"],
        launchbox_platforms: &["Nintendo Game Boy Advance"],
    },
    System {
        folder_name: "nintendo_gbc",
        display_name: "Game Boy Color",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        extensions: &["gbc"],
        launchbox_platforms: &["Nintendo Game Boy Color"],
    },
    System {
        folder_name: "nintendo_n64",
        display_name: "Nintendo 64",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["z64", "n64", "v64"],
        launchbox_platforms: &["Nintendo 64"],
    },
    System {
        folder_name: "nintendo_nes",
        display_name: "Nintendo Entertainment System",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["nes", "unif", "unf"],
        launchbox_platforms: &["Nintendo Entertainment System"],
    },
    System {
        folder_name: "nintendo_snes",
        display_name: "Super Nintendo",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        extensions: &["smc", "sfc"],
        launchbox_platforms: &["Super Nintendo Entertainment System"],
    },
    System {
        folder_name: "panasonic_3do",
        display_name: "3DO",
        manufacturer: "Panasonic",
        category: SystemCategory::Console,
        extensions: &["iso", "chd", "cue"],
        launchbox_platforms: &["3DO Interactive Multiplayer"],
    },
    System {
        folder_name: "philips_cdi",
        display_name: "Philips CD-i",
        manufacturer: "Philips",
        category: SystemCategory::Console,
        extensions: &["chd", "iso", "cue"],
        launchbox_platforms: &["Philips CD-i"],
    },
    System {
        folder_name: "scummvm",
        display_name: "ScummVM",
        manufacturer: "Various",
        category: SystemCategory::Computer,
        extensions: &["scummvm", "svm"],
        launchbox_platforms: &["ScummVM"],
    },
    System {
        folder_name: "sega_32x",
        display_name: "Sega 32X",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["32x", "bin"],
        launchbox_platforms: &["Sega 32X"],
    },
    System {
        folder_name: "sega_cd",
        display_name: "Sega CD / Mega-CD",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "iso"],
        launchbox_platforms: &["Sega CD"],
    },
    System {
        folder_name: "sega_dc",
        display_name: "Sega Dreamcast",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["gdi", "chd", "cdi"],
        launchbox_platforms: &["Sega Dreamcast"],
    },
    System {
        folder_name: "sega_gg",
        display_name: "Sega Game Gear",
        manufacturer: "Sega",
        category: SystemCategory::Handheld,
        extensions: &["gg"],
        launchbox_platforms: &["Sega Game Gear"],
    },
    System {
        folder_name: "sega_sg",
        display_name: "Sega SG-1000",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["sg"],
        launchbox_platforms: &["Sega SG-1000"],
    },
    System {
        folder_name: "sega_smd",
        display_name: "Sega Mega Drive / Genesis",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["md", "bin", "gen", "smd"],
        launchbox_platforms: &["Sega Genesis", "Sega Mega Drive"],
    },
    System {
        folder_name: "sega_sms",
        display_name: "Sega Master System",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["sms"],
        launchbox_platforms: &["Sega Master System"],
    },
    System {
        folder_name: "sega_st",
        display_name: "Sega Saturn",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "iso"],
        launchbox_platforms: &["Sega Saturn"],
    },
    System {
        folder_name: "sharp_x68k",
        display_name: "Sharp X68000",
        manufacturer: "Sharp",
        category: SystemCategory::Computer,
        extensions: &["dim", "hdf", "m3u"],
        launchbox_platforms: &["Sharp X68000"],
    },
    System {
        folder_name: "sinclair_zx",
        display_name: "ZX Spectrum",
        manufacturer: "Sinclair",
        category: SystemCategory::Computer,
        extensions: &["tzx", "tap", "z80", "sna"],
        launchbox_platforms: &["Sinclair ZX Spectrum"],
    },
    System {
        folder_name: "snk_ng",
        display_name: "Neo Geo",
        manufacturer: "SNK",
        category: SystemCategory::Console,
        extensions: &["zip"],
        launchbox_platforms: &["SNK Neo Geo AES", "SNK Neo Geo MVS"],
    },
    System {
        folder_name: "snk_ngcd",
        display_name: "Neo Geo CD",
        manufacturer: "SNK",
        category: SystemCategory::Console,
        extensions: &["chd", "cue"],
        launchbox_platforms: &["SNK Neo Geo CD"],
    },
    System {
        folder_name: "snk_ngp",
        display_name: "Neo Geo Pocket",
        manufacturer: "SNK",
        category: SystemCategory::Handheld,
        extensions: &["ngp", "ngc"],
        launchbox_platforms: &["SNK Neo Geo Pocket", "SNK Neo Geo Pocket Color"],
    },
    System {
        folder_name: "sony_psx",
        display_name: "PlayStation",
        manufacturer: "Sony",
        category: SystemCategory::Console,
        extensions: &["chd", "cue", "bin", "img", "pbp", "m3u"],
        launchbox_platforms: &["Sony Playstation"],
    },
    System {
        folder_name: "alpha_player",
        display_name: "Alpha Player",
        manufacturer: "RePlayOS",
        category: SystemCategory::Utility,
        extensions: &["mkv", "avi", "mp4", "mp3", "flac", "ogg"],
        launchbox_platforms: &[],
    },
];

/// Systems hidden from the UI.
///
/// Alpha Player is a libretro video player core — its "ROMs" are video files
/// (mkv, avi, mp4, etc.), not games.  The current game-centric UI (metadata,
/// box art, "games" labels) doesn't fit, so we hide it until a dedicated
/// media section is built.
const HIDDEN_SYSTEMS: &[&str] = &["alpha_player"];

/// Systems whose ROM sizes should be displayed in Megabit (Mbit/Kbit).
///
/// Cartridge-based and arcade ROM-chip systems used ROM chips whose capacity
/// was measured and marketed in Megabits. Displaying sizes in Mbit matches the
/// original packaging, box art, and enthusiast conventions for these platforms.
const MEGABIT_SYSTEMS: &[&str] = &[
    // --- Atari cartridge systems ---
    // All used ROM cartridges; sizes printed on packaging in Kbit/Mbit.
    "atari_2600",   // 2-64 Kbit ROMs
    "atari_5200",   // 8-128 Kbit ROMs
    "atari_7800",   // 16-1024 Kbit ROMs
    "atari_jaguar", // 8-48 Mbit cartridges
    "atari_lynx",   // 1-4 Mbit cartridge handheld
    // --- Nintendo cartridge systems ---
    // ROM chip sizes on labels: "PRG-ROM: 256 Kbit", "8 Mbit", "64 Mbit", etc.
    // Excludes nintendo_ds: DS era used MB, not Mbit.
    "nintendo_nes",  // 128 Kbit - 4 Mbit, chip sizes on PCB labels
    "nintendo_snes", // 2-48 Mbit, "8 MEGABIT" on Super Mario World box
    "nintendo_n64",  // 32-512 Mbit, "64 Mbit" on Super Mario 64
    "nintendo_gb",   // 256 Kbit - 8 Mbit, "4 Mbit" on Pokemon Red
    "nintendo_gbc",  // 256 Kbit - 16 Mbit, same tradition as GB
    "nintendo_gba",  // 4-256 Mbit, "64 Mbit" standard size
    // --- Sega cartridge systems ---
    // All cart-based; "16 MEGA" on Sonic 3 box, "24 MEGA" on Phantasy Star IV.
    "sega_sg",  // 8-256 Kbit, SG-1000 cartridges
    "sega_sms", // 128 Kbit - 4 Mbit, Master System cartridges
    "sega_smd", // 4-40 Mbit, "16 MEGA" labels on Genesis/MD carts
    "sega_32x", // 8-32 Mbit, cart add-on for Mega Drive
    "sega_gg",  // 256 Kbit - 4 Mbit, same tech as SMS
    // --- NEC ---
    // HuCards were credit-card-format cartridges with ROM chips.
    "nec_pce", // 2-20 Mbit HuCards
    // --- SNK ---
    // Neo Geo AES/MVS had massive cartridges; "330 MEGA" printed on KOF labels.
    "snk_ng",  // 8-688 Mbit, largest cartridges ever made
    "snk_ngp", // 4-16 Mbit, Neo Geo Pocket cartridges
    // --- Arcade (ROM-chip boards) ---
    // Classic arcade boards used ROM chips; board specs stated in Megabits.
    // "CPS2: 160 Mbit", etc. Excludes arcade_dc (GD-ROM/flash = MB).
    "arcade_fbneo",
    "arcade_mame",
    "arcade_mame_2k3p",
];

impl System {
    /// Whether this system should be excluded from UI-facing lists.
    pub fn is_hidden(&self) -> bool {
        HIDDEN_SYSTEMS.contains(&self.folder_name)
    }

    /// Whether this system's ROM sizes should be displayed in Megabit (Mbit/Kbit)
    /// rather than the default KB/MB/GB.
    ///
    /// Returns `true` for cartridge-based and arcade ROM-chip systems whose
    /// original packaging and marketing used Megabit units.
    pub fn uses_megabit(&self) -> bool {
        MEGABIT_SYSTEMS.contains(&self.folder_name)
    }
}

/// Check whether a system (by folder name) should display ROM sizes in Megabit.
///
/// Convenience function for use from the app crate when only a folder name
/// string is available (e.g., from `RomEntry.game.system`).
pub fn find_system_uses_megabit(folder_name: &str) -> bool {
    MEGABIT_SYSTEMS.contains(&folder_name)
}

/// All systems that should be shown in the UI (excludes hidden ones).
pub fn visible_systems() -> impl Iterator<Item = &'static System> {
    SYSTEMS.iter().filter(|s| !s.is_hidden())
}

/// Build a map from LaunchBox platform names to system folder names.
/// Derived from the `launchbox_platforms` field on each `System`.
pub fn launchbox_platform_map() -> std::collections::HashMap<&'static str, Vec<&'static str>> {
    let mut m: std::collections::HashMap<&'static str, Vec<&'static str>> =
        std::collections::HashMap::new();
    for sys in SYSTEMS {
        for &platform in sys.launchbox_platforms {
            m.entry(platform).or_default().push(sys.folder_name);
        }
    }
    m
}

/// Look up a system by its folder name.
pub fn find_system(folder_name: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.folder_name == folder_name)
}

/// Extract the system folder name from a favorite/recent filename.
/// E.g., "sega_smd@Sonic.md.fav" → "sega_smd"
pub fn system_from_fav_filename(filename: &str) -> Option<&str> {
    let (system, rest) = filename.split_once('@')?;
    if system.is_empty() || rest.is_empty() {
        return None;
    }
    Some(system)
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

    #[test]
    fn system_from_fav_rejects_malformed() {
        assert_eq!(system_from_fav_filename("no_at_sign.fav"), None);
        assert_eq!(system_from_fav_filename("@missing_system.fav"), None);
        assert_eq!(system_from_fav_filename("system@"), None);
        assert_eq!(system_from_fav_filename(""), None);
    }

    #[test]
    fn uses_megabit_cartridge_systems() {
        // Nintendo cartridge systems
        assert!(find_system("nintendo_nes").unwrap().uses_megabit());
        assert!(find_system("nintendo_snes").unwrap().uses_megabit());
        assert!(find_system("nintendo_n64").unwrap().uses_megabit());
        assert!(find_system("nintendo_gb").unwrap().uses_megabit());
        assert!(find_system("nintendo_gbc").unwrap().uses_megabit());
        assert!(find_system("nintendo_gba").unwrap().uses_megabit());
        // Sega cartridge systems
        assert!(find_system("sega_smd").unwrap().uses_megabit());
        assert!(find_system("sega_sms").unwrap().uses_megabit());
        assert!(find_system("sega_sg").unwrap().uses_megabit());
        assert!(find_system("sega_gg").unwrap().uses_megabit());
        assert!(find_system("sega_32x").unwrap().uses_megabit());
        // Atari cartridge systems
        assert!(find_system("atari_2600").unwrap().uses_megabit());
        assert!(find_system("atari_5200").unwrap().uses_megabit());
        assert!(find_system("atari_7800").unwrap().uses_megabit());
        assert!(find_system("atari_jaguar").unwrap().uses_megabit());
        assert!(find_system("atari_lynx").unwrap().uses_megabit());
        // NEC / SNK cartridge
        assert!(find_system("nec_pce").unwrap().uses_megabit());
        assert!(find_system("snk_ng").unwrap().uses_megabit());
        assert!(find_system("snk_ngp").unwrap().uses_megabit());
        // Arcade ROM-chip boards
        assert!(find_system("arcade_fbneo").unwrap().uses_megabit());
        assert!(find_system("arcade_mame").unwrap().uses_megabit());
        assert!(find_system("arcade_mame_2k3p").unwrap().uses_megabit());
    }

    #[test]
    fn uses_megabit_non_megabit_systems() {
        // DS uses MB, not Mbit
        assert!(!find_system("nintendo_ds").unwrap().uses_megabit());
        // arcade_dc is disc/flash-based
        assert!(!find_system("arcade_dc").unwrap().uses_megabit());
        // Disc-based systems
        assert!(!find_system("sony_psx").unwrap().uses_megabit());
        assert!(!find_system("sega_dc").unwrap().uses_megabit());
        assert!(!find_system("sega_cd").unwrap().uses_megabit());
        assert!(!find_system("sega_st").unwrap().uses_megabit());
        assert!(!find_system("nec_pcecd").unwrap().uses_megabit());
        assert!(!find_system("snk_ngcd").unwrap().uses_megabit());
        // Computer / floppy-based
        assert!(!find_system("commodore_ami").unwrap().uses_megabit());
        assert!(!find_system("commodore_c64").unwrap().uses_megabit());
        assert!(!find_system("ibm_pc").unwrap().uses_megabit());
        assert!(!find_system("microsoft_msx").unwrap().uses_megabit());
        assert!(!find_system("sinclair_zx").unwrap().uses_megabit());
    }

    #[test]
    fn find_system_uses_megabit_known() {
        assert!(find_system_uses_megabit("nintendo_snes"));
        assert!(find_system_uses_megabit("sega_smd"));
        assert!(find_system_uses_megabit("arcade_fbneo"));
    }

    #[test]
    fn find_system_uses_megabit_non_megabit() {
        assert!(!find_system_uses_megabit("sony_psx"));
        assert!(!find_system_uses_megabit("nintendo_ds"));
        assert!(!find_system_uses_megabit("arcade_dc"));
        assert!(!find_system_uses_megabit("microsoft_msx"));
    }

    #[test]
    fn find_system_uses_megabit_unknown() {
        assert!(!find_system_uses_megabit("totally_unknown"));
        assert!(!find_system_uses_megabit(""));
    }

    #[test]
    fn launchbox_platform_map_covers_non_utility_systems() {
        // Every non-utility, non-hidden system should have at least one
        // LaunchBox platform mapping. This catches forgotten mappings when
        // adding new systems.
        let exceptions = ["commodore_amicd"]; // No LaunchBox platform exists
        for sys in SYSTEMS {
            if sys.category == SystemCategory::Utility || exceptions.contains(&sys.folder_name) {
                continue;
            }
            assert!(
                !sys.launchbox_platforms.is_empty(),
                "System '{}' has no LaunchBox platform mapping. \
                 Add launchbox_platforms to its definition in systems.rs.",
                sys.folder_name
            );
        }
    }

    #[test]
    fn launchbox_platform_map_values_are_valid_systems() {
        let map = launchbox_platform_map();
        for (platform, folders) in &map {
            for folder in folders {
                assert!(
                    find_system(folder).is_some(),
                    "LaunchBox platform '{}' maps to unknown system folder '{}'",
                    platform,
                    folder
                );
            }
        }
    }
}
