use serde::Serialize;

/// A supported RePlayOS system (console, arcade, computer, or handheld).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct System {
    pub folder_name: &'static str,
    pub display_name: &'static str,
    pub manufacturer: &'static str,
    pub category: SystemCategory,
    pub extensions: &'static [&'static str],
    /// Short abbreviation for compact display (e.g., "SNES", "MD", "PS1").
    pub abbreviation: &'static str,
    /// CSS hex color for box-art placeholder backgrounds, inspired by original branding.
    pub placeholder_color: &'static str,
    /// LaunchBox platform names that map to this system folder.
    /// Used by the LaunchBox XML import to match games to systems.
    /// Empty slice means no LaunchBox mapping (system won't get LaunchBox metadata).
    #[serde(skip)]
    pub launchbox_platforms: &'static [&'static str],
    /// Hidden from UI-facing lists.
    ///
    /// True for utility cores (e.g., Alpha Player video player) whose "ROMs" are
    /// media files rather than games and don't fit the game-centric UI (metadata,
    /// box art, "games" labels).
    #[serde(skip)]
    pub hidden: bool,
    /// Display ROM sizes in Megabit (Mbit/Kbit) rather than the default KB/MB/GB.
    ///
    /// True for cartridge-based and arcade ROM-chip systems whose original
    /// packaging and marketing used Megabit units:
    /// - Nintendo carts: "8 MEGABIT" on Super Mario World, "64 Mbit" on Mario 64
    /// - Sega carts: "16 MEGA" on Sonic 3, "24 MEGA" on Phantasy Star IV
    /// - SNK Neo Geo: "330 MEGA" on KOF labels (largest cartridges ever made)
    /// - Arcade ROM-chip boards: "CPS2: 160 Mbit"
    ///
    /// False for disc-based (PSX, Saturn, DC, CD), flash/GD-ROM-based arcade
    /// hardware (Naomi/Atomiswave), and modern handhelds (DS uses MB).
    #[serde(skip)]
    pub uses_megabit: bool,
    /// libretro-thumbnails repository names (primary first).
    ///
    /// Multiple repos are tried in order during import, so ROMs not found in
    /// the primary repo can be matched against fallback repos. Empty slice means
    /// no thumbnail source is known for this system.
    #[serde(skip)]
    pub thumbnail_repos: &'static [&'static str],
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
        abbreviation: "ARC",
        placeholder_color: "#1a1a2e",
        extensions: &["zip"],
        launchbox_platforms: &["Arcade"],
        hidden: false,
        uses_megabit: true,
        // FBNeo primary, MAME fallback — each libretro-thumbnails repo has
        // some box arts the other lacks, so two-repo lookup is intentional.
        thumbnail_repos: &["FBNeo - Arcade Games", "MAME"],
    },
    System {
        folder_name: "arcade_mame",
        display_name: "Arcade (MAME)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        abbreviation: "ARC",
        placeholder_color: "#1a1a2e",
        extensions: &["zip"],
        launchbox_platforms: &["Arcade"],
        hidden: false,
        uses_megabit: true,
        // MAME primary, FBNeo fallback — inverse of arcade_fbneo, same reason.
        // libretro-thumbnails uses display names as filenames, so the manifest
        // builder translates MAME codenames via arcade_db.
        thumbnail_repos: &["MAME", "FBNeo - Arcade Games"],
    },
    System {
        folder_name: "arcade_mame_2k3p",
        display_name: "Arcade (MAME 2003+)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        abbreviation: "ARC",
        placeholder_color: "#1a1a2e",
        extensions: &["zip"],
        launchbox_platforms: &["Arcade"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["MAME"],
    },
    System {
        folder_name: "arcade_dc",
        display_name: "Arcade (Atomiswave/Naomi)",
        manufacturer: "Various",
        category: SystemCategory::Arcade,
        abbreviation: "ARC",
        placeholder_color: "#1a1a2e",
        extensions: &["zip", "chd"],
        launchbox_platforms: &["Sammy Atomiswave", "Sega Naomi", "Sega Naomi 2"],
        hidden: false,
        // GD-ROM / flash-based, not ROM chips.
        uses_megabit: false,
        thumbnail_repos: &["Atomiswave", "Sega - Naomi", "Sega - Naomi 2"],
    },
    System {
        folder_name: "arcade_stv",
        display_name: "Sega Titan Video (ST-V)",
        manufacturer: "Sega",
        category: SystemCategory::Arcade,
        abbreviation: "STV",
        placeholder_color: "#1a1a2e",
        extensions: &["zip"],
        // LaunchBox dual-lists most ST-V titles: a sparse `Sega ST-V` entry
        // (often name-only) plus a richer `Arcade` entry that carries the
        // VideoURL, Overview, Wikipedia link, etc. Importing both — combined
        // with the field-level richer-wins merge in `prepare_launchbox_refresh`
        // — fills in the gaps without losing the canonical ST-V tag.
        launchbox_platforms: &["Sega ST-V", "Arcade"],
        hidden: false,
        // Cartridge-based Saturn-derived arcade hardware.
        uses_megabit: true,
        thumbnail_repos: &["MAME"],
    },
    System {
        folder_name: "atari_2600",
        display_name: "Atari 2600",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        abbreviation: "2600",
        placeholder_color: "#8b4513",
        extensions: &["a26", "bin"],
        launchbox_platforms: &["Atari 2600"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Atari - 2600"],
    },
    System {
        folder_name: "atari_5200",
        display_name: "Atari 5200",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        abbreviation: "5200",
        placeholder_color: "#8b4513",
        extensions: &["a52", "bin"],
        launchbox_platforms: &["Atari 5200"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Atari - 5200"],
    },
    System {
        folder_name: "atari_7800",
        display_name: "Atari 7800",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        abbreviation: "7800",
        placeholder_color: "#8b4513",
        extensions: &["a78", "bin", "cdf"],
        launchbox_platforms: &["Atari 7800"],
        hidden: false,
        uses_megabit: true,
        // Slug renamed upstream from "Atari - 7800 ProSystem"; revisit when
        // catalog-build-time slug resolution lands.
        thumbnail_repos: &["Atari - 7800"],
    },
    System {
        folder_name: "atari_jaguar",
        display_name: "Atari Jaguar",
        manufacturer: "Atari",
        category: SystemCategory::Console,
        abbreviation: "JAG",
        placeholder_color: "#8b4513",
        extensions: &["j64", "jag", "rom", "abs"],
        launchbox_platforms: &["Atari Jaguar"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Atari - Jaguar"],
    },
    System {
        folder_name: "atari_lynx",
        display_name: "Atari Lynx",
        manufacturer: "Atari",
        category: SystemCategory::Handheld,
        abbreviation: "LYNX",
        placeholder_color: "#8b4513",
        extensions: &["lnx"],
        launchbox_platforms: &["Atari Lynx"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Atari - Lynx"],
    },
    System {
        folder_name: "amstrad_cpc",
        display_name: "Amstrad CPC",
        manufacturer: "Amstrad",
        category: SystemCategory::Computer,
        abbreviation: "CPC",
        placeholder_color: "#2a4858",
        extensions: &["dsk", "sna", "tap", "cdt", "voc", "cpr", "m3u"],
        launchbox_platforms: &["Amstrad CPC"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Amstrad - CPC"],
    },
    System {
        folder_name: "commodore_ami",
        display_name: "Commodore Amiga",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        abbreviation: "AMI",
        placeholder_color: "#4a3728",
        extensions: &[
            "adf", "hdf", "ipf", "lha", "adz", "dms", "fdi", "raw", "hdz", "slave", "info", "uae",
            "m3u",
        ],
        launchbox_platforms: &["Commodore Amiga"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Commodore - Amiga"],
    },
    System {
        folder_name: "commodore_amicd",
        display_name: "Commodore Amiga CD",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        abbreviation: "ACD",
        placeholder_color: "#4a3728",
        extensions: &["iso", "cue", "chd", "ccd", "nrg", "mds", "m3u"],
        launchbox_platforms: &["Commodore Amiga CD32"],
        hidden: false,
        uses_megabit: false,
        // commodore_amicd covers CD32 + CDTV hardware
        thumbnail_repos: &["Commodore - CD32", "Commodore - CDTV"],
    },
    System {
        folder_name: "commodore_c64",
        display_name: "Commodore 64",
        manufacturer: "Commodore",
        category: SystemCategory::Computer,
        abbreviation: "C64",
        placeholder_color: "#4a3728",
        extensions: &[
            "d64", "t64", "tap", "prg", "crt", "d71", "d80", "d81", "d82", "g64", "g41", "x64",
            "p00", "bin", "gz", "d6z", "d8z", "g6z", "g4z", "x6z", "cmd", "m3u", "vfl", "vs",
        ],
        launchbox_platforms: &["Commodore 64"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Commodore - 64"],
    },
    System {
        folder_name: "ibm_pc",
        display_name: "IBM PC (DOS)",
        manufacturer: "IBM",
        category: SystemCategory::Computer,
        abbreviation: "DOS",
        placeholder_color: "#2a4858",
        extensions: &[
            "zip", "exe", "com", "bat", "dosz", "iso", "cue", "img", "m3u", "m3u8",
        ],
        launchbox_platforms: &["MS-DOS"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["DOS"],
    },
    System {
        folder_name: "microsoft_msx",
        display_name: "MSX",
        manufacturer: "Microsoft",
        category: SystemCategory::Computer,
        abbreviation: "MSX",
        placeholder_color: "#2a4858",
        extensions: &[
            "rom", "mx1", "mx2", "dsk", "ri", "col", "sg", "sc", "sf", "cas", "m3u",
        ],
        launchbox_platforms: &["Microsoft MSX", "Microsoft MSX2"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Microsoft - MSX", "Microsoft - MSX2"],
    },
    System {
        folder_name: "nec_pce",
        display_name: "PC Engine / TurboGrafx-16",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        abbreviation: "PCE",
        placeholder_color: "#cc3300",
        extensions: &["pce", "sgx", "toc"],
        launchbox_platforms: &["NEC TurboGrafx-16", "NEC PC Engine"],
        hidden: false,
        // HuCards were credit-card-format cartridges with ROM chips, 2-20 Mbit.
        uses_megabit: true,
        thumbnail_repos: &["NEC - PC Engine - TurboGrafx 16"],
    },
    System {
        folder_name: "nec_pcecd",
        display_name: "PC Engine CD",
        manufacturer: "NEC",
        category: SystemCategory::Console,
        abbreviation: "PCD",
        placeholder_color: "#cc3300",
        extensions: &["cue", "chd", "ccd", "m3u"],
        launchbox_platforms: &["NEC TurboGrafx-CD", "NEC PC Engine CD-ROM"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["NEC - PC Engine CD - TurboGrafx-CD"],
    },
    System {
        folder_name: "nintendo_ds",
        display_name: "Nintendo DS",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        abbreviation: "NDS",
        placeholder_color: "#888888",
        extensions: &["nds"],
        launchbox_platforms: &["Nintendo DS"],
        hidden: false,
        // DS era used MB sizing, not Mbit.
        uses_megabit: false,
        thumbnail_repos: &["Nintendo - Nintendo DS"],
    },
    System {
        folder_name: "nintendo_gb",
        display_name: "Game Boy",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        abbreviation: "GB",
        placeholder_color: "#8b9c37",
        extensions: &["gb", "sgb"],
        launchbox_platforms: &["Nintendo Game Boy"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Game Boy"],
    },
    System {
        folder_name: "nintendo_gba",
        display_name: "Game Boy Advance",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        abbreviation: "GBA",
        placeholder_color: "#4b0082",
        extensions: &["gba"],
        launchbox_platforms: &["Nintendo Game Boy Advance"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Game Boy Advance"],
    },
    System {
        folder_name: "nintendo_gbc",
        display_name: "Game Boy Color",
        manufacturer: "Nintendo",
        category: SystemCategory::Handheld,
        abbreviation: "GBC",
        placeholder_color: "#6a0dad",
        extensions: &["gbc", "sgbc"],
        launchbox_platforms: &["Nintendo Game Boy Color"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Game Boy Color"],
    },
    System {
        folder_name: "nintendo_n64",
        display_name: "Nintendo 64",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        abbreviation: "N64",
        placeholder_color: "#009e60",
        extensions: &["z64", "n64", "v64", "bin", "u1"],
        launchbox_platforms: &["Nintendo 64"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Nintendo 64"],
    },
    System {
        folder_name: "nintendo_nes",
        display_name: "NES / Famicom",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        abbreviation: "NES",
        placeholder_color: "#c41230",
        extensions: &["nes", "unif", "unf", "fds"],
        launchbox_platforms: &["Nintendo Entertainment System"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Nintendo Entertainment System"],
    },
    System {
        folder_name: "nintendo_snes",
        display_name: "Super Nintendo / Super Famicom",
        manufacturer: "Nintendo",
        category: SystemCategory::Console,
        abbreviation: "SNES",
        placeholder_color: "#6b238e",
        extensions: &["smc", "sfc", "swc", "fig", "bs", "st"],
        launchbox_platforms: &["Super Nintendo Entertainment System"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Nintendo - Super Nintendo Entertainment System"],
    },
    System {
        folder_name: "panasonic_3do",
        display_name: "3DO",
        manufacturer: "Panasonic",
        category: SystemCategory::Console,
        abbreviation: "3DO",
        placeholder_color: "#2a4858",
        extensions: &["iso", "chd", "cue"],
        launchbox_platforms: &["3DO Interactive Multiplayer"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["The 3DO Company - 3DO"],
    },
    System {
        folder_name: "philips_cdi",
        display_name: "Philips CD-i",
        manufacturer: "Philips",
        category: SystemCategory::Console,
        abbreviation: "CDi",
        placeholder_color: "#2a4858",
        extensions: &["chd", "iso", "cue"],
        launchbox_platforms: &["Philips CD-i"],
        hidden: false,
        uses_megabit: false,
        // Slug renamed upstream from "Philips - CDi"; revisit when
        // catalog-build-time slug resolution lands.
        thumbnail_repos: &["Philips - CD-i"],
    },
    System {
        folder_name: "scummvm",
        display_name: "ScummVM",
        manufacturer: "Various",
        category: SystemCategory::Computer,
        abbreviation: "SVM",
        placeholder_color: "#2a4858",
        extensions: &["scummvm", "svm"],
        launchbox_platforms: &["ScummVM"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["ScummVM"],
    },
    System {
        folder_name: "sega_32x",
        display_name: "Sega 32X",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "32X",
        placeholder_color: "#0060a8",
        extensions: &["32x", "bin", "chd", "cue", "iso", "m3u"],
        launchbox_platforms: &["Sega 32X", "Sega CD 32X"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Sega - 32X", "Sega - Mega-CD - Sega CD"],
    },
    System {
        folder_name: "sega_cd",
        display_name: "Sega CD / Mega-CD",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "SCD",
        placeholder_color: "#333355",
        extensions: &["chd", "cue", "iso", "m3u"],
        launchbox_platforms: &["Sega CD"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sega - Mega-CD - Sega CD"],
    },
    System {
        folder_name: "sega_dc",
        display_name: "Sega Dreamcast",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "DC",
        placeholder_color: "#ff6600",
        extensions: &["gdi", "chd", "cdi", "elf", "cue", "lst", "dat", "m3u"],
        launchbox_platforms: &["Sega Dreamcast"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sega - Dreamcast"],
    },
    System {
        folder_name: "sega_gg",
        display_name: "Sega Game Gear",
        manufacturer: "Sega",
        category: SystemCategory::Handheld,
        abbreviation: "GG",
        placeholder_color: "#1a1a2e",
        extensions: &["gg"],
        launchbox_platforms: &["Sega Game Gear"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Sega - Game Gear"],
    },
    System {
        folder_name: "sega_sg",
        display_name: "Sega SG-1000",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "SG",
        placeholder_color: "#0060a8",
        extensions: &["sg"],
        launchbox_platforms: &["Sega SG-1000"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Sega - SG-1000"],
    },
    System {
        folder_name: "sega_smd",
        display_name: "Sega Mega Drive / Genesis",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "MD",
        placeholder_color: "#0060a8",
        extensions: &["md", "bin", "gen", "smd"],
        launchbox_platforms: &["Sega Genesis", "Sega Mega Drive"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Sega - Mega Drive - Genesis"],
    },
    System {
        folder_name: "sega_sms",
        display_name: "Sega Master System",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "SMS",
        placeholder_color: "#cc0000",
        extensions: &["sms"],
        launchbox_platforms: &["Sega Master System"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["Sega - Master System - Mark III"],
    },
    System {
        folder_name: "sega_st",
        display_name: "Sega Saturn",
        manufacturer: "Sega",
        category: SystemCategory::Console,
        abbreviation: "SAT",
        placeholder_color: "#222244",
        extensions: &["chd", "cue", "iso", "ccd", "toc", "m3u"],
        launchbox_platforms: &["Sega Saturn"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sega - Saturn"],
    },
    System {
        folder_name: "sharp_x68k",
        display_name: "Sharp X68000",
        manufacturer: "Sharp",
        category: SystemCategory::Computer,
        abbreviation: "X68K",
        placeholder_color: "#2a4858",
        extensions: &[
            "dim", "hdf", "m3u", "img", "d88", "88d", "hdm", "dup", "2hd", "xdf", "cmd",
        ],
        launchbox_platforms: &["Sharp X68000"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sharp - X68000"],
    },
    System {
        folder_name: "sinclair_zx",
        display_name: "ZX Spectrum",
        manufacturer: "Sinclair",
        category: SystemCategory::Computer,
        abbreviation: "ZX",
        placeholder_color: "#2a4858",
        extensions: &[
            "tzx", "tap", "z80", "sna", "rzx", "scl", "trd", "dsk", "dck", "szx",
        ],
        launchbox_platforms: &["Sinclair ZX Spectrum"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sinclair - ZX Spectrum"],
    },
    System {
        folder_name: "snk_ng",
        display_name: "Neo Geo",
        manufacturer: "SNK",
        category: SystemCategory::Arcade,
        abbreviation: "NG",
        placeholder_color: "#b8860b",
        extensions: &["zip"],
        launchbox_platforms: &["SNK Neo Geo AES", "SNK Neo Geo MVS"],
        hidden: false,
        // "330 MEGA" on KOF labels — largest cartridges ever made.
        uses_megabit: true,
        thumbnail_repos: &["SNK - Neo Geo"],
    },
    System {
        folder_name: "snk_ngcd",
        display_name: "Neo Geo CD",
        manufacturer: "SNK",
        category: SystemCategory::Console,
        abbreviation: "NGCD",
        placeholder_color: "#b8860b",
        extensions: &["chd", "cue"],
        launchbox_platforms: &["SNK Neo Geo CD"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["SNK - Neo Geo CD"],
    },
    System {
        folder_name: "snk_ngp",
        display_name: "Neo Geo Pocket",
        manufacturer: "SNK",
        category: SystemCategory::Handheld,
        abbreviation: "NGP",
        placeholder_color: "#b8860b",
        extensions: &["ngp", "ngc", "ngpc", "npc"],
        launchbox_platforms: &["SNK Neo Geo Pocket", "SNK Neo Geo Pocket Color"],
        hidden: false,
        uses_megabit: true,
        thumbnail_repos: &["SNK - Neo Geo Pocket"],
    },
    System {
        folder_name: "sony_psx",
        display_name: "PlayStation",
        manufacturer: "Sony",
        category: SystemCategory::Console,
        abbreviation: "PS1",
        placeholder_color: "#003791",
        extensions: &[
            "chd", "cue", "bin", "img", "pbp", "m3u", "exe", "psexe", "iso", "ecm", "mds", "psf",
        ],
        launchbox_platforms: &["Sony Playstation"],
        hidden: false,
        uses_megabit: false,
        thumbnail_repos: &["Sony - PlayStation"],
    },
    System {
        folder_name: "alpha_player",
        display_name: "Alpha Player",
        manufacturer: "RePlayOS",
        category: SystemCategory::Utility,
        abbreviation: "VID",
        placeholder_color: "#333333",
        extensions: &[
            "mkv", "avi", "mp4", "mp3", "flac", "ogg", "f4v", "f4f", "3gp", "ogm", "flv", "m4a",
            "webm", "3g2", "mov", "wmv", "mpg", "mpeg", "vob", "asf", "divx", "m2p", "m2ts", "ps",
            "ts", "mxf", "wma", "wav", "s3m", "it", "xm", "mod", "ay", "gbs", "gym", "hes", "kss",
            "nsf", "nsfe", "sap", "spc", "vgm", "vgz",
        ],
        launchbox_platforms: &[],
        // Hidden until a dedicated media section exists — Alpha Player's "ROMs"
        // are video files (mkv, avi, mp4, …), not games, so the game-centric UI
        // (metadata, box art, "games" labels) doesn't fit.
        hidden: true,
        uses_megabit: false,
        thumbnail_repos: &[],
    },
];

impl System {
    /// Whether this system should be excluded from UI-facing lists.
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Whether this system is arcade-class according to the system registry.
    pub fn is_arcade(&self) -> bool {
        self.category == SystemCategory::Arcade
    }

    /// Whether this system's ROM sizes should be displayed in Megabit (Mbit/Kbit)
    /// rather than the default KB/MB/GB.
    pub fn uses_megabit(&self) -> bool {
        self.uses_megabit
    }

    /// GameFAQs title-search URL for `title`, or `None` for utility systems
    /// (e.g. Alpha Player) whose "ROMs" aren't games.
    ///
    /// GameFAQs has no system-scoped search URL that accepts string slugs
    /// (the `platform=` param requires numeric IDs), so this is a global
    /// title-only search.
    pub fn gamefaqs_search_url(&self, title: &str) -> Option<String> {
        if self.category == SystemCategory::Utility {
            return None;
        }
        let encoded: String = url::form_urlencoded::byte_serialize(title.as_bytes()).collect();
        Some(format!(
            "https://gamefaqs.gamespot.com/search?game={encoded}"
        ))
    }
}

/// Check whether a system (by folder name) should display ROM sizes in Megabit.
///
/// Convenience function for use from the app crate when only a folder name
/// string is available (e.g., from `RomEntry.game.system`).
pub fn find_system_uses_megabit(folder_name: &str) -> bool {
    find_system(folder_name).is_some_and(|s| s.uses_megabit)
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

/// Deterministic CRC32 fingerprint of the current LaunchBox platform map.
///
/// Bumps whenever a system is added/removed or its `launchbox_platforms` field
/// changes — gates the LaunchBox auto-import so a fresh deploy that added a
/// new system (e.g. `arcade_stv`) triggers a re-parse even when the upstream
/// XML hash is unchanged.
pub fn launchbox_platform_map_fingerprint() -> String {
    let mut entries: Vec<(&'static str, Vec<&'static str>)> = SYSTEMS
        .iter()
        .filter(|s| !s.launchbox_platforms.is_empty())
        .map(|s| {
            let mut platforms: Vec<&'static str> = s.launchbox_platforms.to_vec();
            platforms.sort_unstable();
            (s.folder_name, platforms)
        })
        .collect();
    entries.sort_unstable_by_key(|(folder, _)| *folder);
    let mut hasher = crc32fast::Hasher::new();
    for (folder, platforms) in &entries {
        hasher.update(folder.as_bytes());
        hasher.update(b"\0");
        for p in platforms {
            hasher.update(p.as_bytes());
            hasher.update(b"\x01");
        }
        hasher.update(b"\n");
    }
    format!("{:08x}", hasher.finalize())
}

/// Look up a system by its folder name.
pub fn find_system(folder_name: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.folder_name == folder_name)
}

/// Resolve a system folder name to its user-facing display name.
/// Falls back to the folder name when unknown.
pub fn system_display_name(folder_name: &str) -> String {
    find_system(folder_name)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| folder_name.to_string())
}

/// libretro-thumbnails repo names for a system folder, or `None` when the
/// system is unknown or has no configured repos. Convenience over
/// `find_system(name).map(|s| s.thumbnail_repos).filter(|r| !r.is_empty())`
/// for callers that only have a `&str` folder name.
pub fn system_thumbnail_repos(folder_name: &str) -> Option<&'static [&'static str]> {
    find_system(folder_name)
        .map(|s| s.thumbnail_repos)
        .filter(|r| !r.is_empty())
}

/// Check whether a system folder name refers to an arcade system.
/// Uses the system registry rather than hardcoded folder name lists.
pub fn is_arcade_system(folder_name: &str) -> bool {
    find_system(folder_name).is_some_and(System::is_arcade)
}

/// Which upstream curates a given arcade ROM's metadata.
///
/// Each upstream (FBNeo DAT, MAME 2003+ XML, MAME current XML, Flycast CSV)
/// has its own row per ROM in `arcade_game`. The runtime merges these by
/// per-system priority — see [`arcade_source_priority`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ArcadeSource {
    Fbneo = 0,
    Mame = 1,
    Mame2k3p = 2,
    Naomi = 3,
}

impl ArcadeSource {
    /// All variants, in deterministic order. Used as the runtime fallback
    /// after a system's priority list is exhausted.
    pub const ALL: [ArcadeSource; 4] = [
        ArcadeSource::Mame,
        ArcadeSource::Mame2k3p,
        ArcadeSource::Fbneo,
        ArcadeSource::Naomi,
    ];

    /// String tag stored in `arcade_game.source` and `arcade_release_date.source`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ArcadeSource::Fbneo => "fbneo",
            ArcadeSource::Mame => "mame",
            ArcadeSource::Mame2k3p => "mame_2k3p",
            ArcadeSource::Naomi => "naomi",
        }
    }

    /// Inverse of `as_str`. Returns `None` for unknown tags.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "fbneo" => Some(ArcadeSource::Fbneo),
            "mame" => Some(ArcadeSource::Mame),
            "mame_2k3p" => Some(ArcadeSource::Mame2k3p),
            "naomi" => Some(ArcadeSource::Naomi),
            _ => None,
        }
    }

    /// 0-based index, matching the position in [`ArcadeSource::ALL`] when used
    /// to index into a fixed-size `[Option<T>; 4]` keyed by source.
    pub const fn idx(self) -> usize {
        self as usize
    }
}

/// Ordered upstream priority for an arcade system. Highest-priority first;
/// later entries are field-by-field fallbacks during merge.
///
/// Mirrors the per-system fallback shape used by the `thumbnail_repos` field
/// on [`System`].
///
/// Returns an empty slice for non-arcade systems.
pub fn arcade_source_priority(folder_name: &str) -> &'static [ArcadeSource] {
    use ArcadeSource::*;
    match folder_name {
        "arcade_fbneo" => &[Fbneo, Mame, Mame2k3p],
        "arcade_mame" => &[Mame, Mame2k3p, Fbneo],
        "arcade_mame_2k3p" => &[Mame2k3p, Mame, Fbneo],
        "arcade_dc" => &[Naomi, Mame, Mame2k3p, Fbneo],
        "arcade_stv" => &[Mame, Mame2k3p, Fbneo],
        "snk_ng" => &[Mame, Mame2k3p, Fbneo],
        _ => &[],
    }
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
        assert_eq!(sys.display_name, "NES / Famicom");
        assert_eq!(sys.category, SystemCategory::Console);
    }

    #[test]
    fn find_unknown_system() {
        assert!(find_system("unknown_system").is_none());
    }

    #[test]
    fn arcade_classification_comes_from_system_registry() {
        assert!(find_system("arcade_fbneo").unwrap().is_arcade());
        assert!(is_arcade_system("arcade_dc"));
        assert!(is_arcade_system("arcade_stv"));
        assert!(!is_arcade_system("nintendo_snes"));
        assert!(!is_arcade_system("unknown_arcade_like_name"));
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
        // STV: cartridge-based Saturn arcade hardware
        assert!(find_system("arcade_stv").unwrap().uses_megabit());
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
        assert!(find_system_uses_megabit("arcade_stv"));
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
    fn alpha_player_is_only_hidden_system() {
        let hidden: Vec<&str> = SYSTEMS
            .iter()
            .filter(|s| s.is_hidden())
            .map(|s| s.folder_name)
            .collect();
        assert_eq!(hidden, vec!["alpha_player"]);
    }

    #[test]
    fn arcade_stv_is_registered() {
        let stv = find_system("arcade_stv").expect("arcade_stv must be in SYSTEMS");
        assert_eq!(stv.display_name, "Sega Titan Video (ST-V)");
        assert_eq!(stv.manufacturer, "Sega");
        assert_eq!(stv.category, SystemCategory::Arcade);
        assert_eq!(stv.abbreviation, "STV");
        assert_eq!(stv.extensions, &["zip"]);
        assert!(stv.uses_megabit);
        assert!(!stv.hidden);
        assert_eq!(stv.thumbnail_repos, &["MAME"]);
    }

    #[test]
    fn arcade_stv_metadata_source_priority() {
        let priority = arcade_source_priority("arcade_stv");
        assert_eq!(
            priority,
            &[
                ArcadeSource::Mame,
                ArcadeSource::Mame2k3p,
                ArcadeSource::Fbneo
            ]
        );
    }

    #[test]
    fn launchbox_platform_map_covers_non_utility_systems() {
        // Every non-utility, non-hidden system should have at least one
        // LaunchBox platform mapping. This catches forgotten mappings when
        // adding new systems.
        let exceptions: [&str; 0] = []; // All non-utility systems should have LaunchBox mappings
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
    fn thumbnail_repos_set_for_non_utility_systems() {
        // Mirrors the LaunchBox-coverage test: every non-utility, non-hidden
        // system should have at least one libretro-thumbnails repo, so new
        // systems don't silently miss out on box-art enrichment.
        for sys in SYSTEMS {
            if sys.category == SystemCategory::Utility || sys.hidden {
                continue;
            }
            assert!(
                !sys.thumbnail_repos.is_empty(),
                "System '{}' has no thumbnail_repos. \
                 Set thumbnail_repos to the libretro-thumbnails repo name(s) \
                 in its definition in systems.rs.",
                sys.folder_name
            );
        }
    }

    #[test]
    fn gamefaqs_search_url_encodes_title_and_skips_utility() {
        let snes = find_system("nintendo_snes").unwrap();
        let url = snes.gamefaqs_search_url("Super Mario World").unwrap();
        assert_eq!(
            url,
            "https://gamefaqs.gamespot.com/search?game=Super+Mario+World"
        );
        // ScummVM (Computer) → link shown; global title search finds the
        // game on its original platform (DOS/Amiga/...).
        assert!(
            find_system("scummvm")
                .unwrap()
                .gamefaqs_search_url("Day of the Tentacle")
                .is_some()
        );
        // Alpha Player (Utility) → no link; "ROMs" are video files.
        assert!(
            find_system("alpha_player")
                .unwrap()
                .gamefaqs_search_url("anything")
                .is_none()
        );
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
