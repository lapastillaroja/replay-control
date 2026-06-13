//! Curated arcade hardware boards (CPS-2, Neo Geo MVS, Taito F3, …) used as
//! a stable, denormalized value on `arcade_game` and `game_library`.
//!
//! The enum is the single source of truth: it carries every board's display
//! name, manufacturer, and the MAME-driver-sourcefile mapping. There is no
//! companion CSV or database table — adding a new board is one variant + one
//! `from_sourcefile` arm.

use serde::{Deserialize, Serialize};

/// A curated arcade hardware board.
///
/// Storage is via [`Self::as_tag`] (stable, kebab-case ASCII) so enum
/// reordering doesn't invalidate existing catalog/library rows. Variant order
/// is informational only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArcadeBoard {
    // Capcom
    Cps1,
    Cps2,
    Cps3,
    // SNK
    NeoGeoMvs,
    // Sega
    SegaSystemC2,
    SegaSystem16a,
    SegaSystem16b,
    SegaSystem18,
    SegaSystem24,
    SegaSystem32,
    SegaModel1,
    SegaModel2,
    SegaModel3,
    SegaStv,
    SegaNaomi,
    SegaNaomi2,
    SammyAtomiswave,
    // Taito
    TaitoF2,
    TaitoF3,
    TaitoZ,
    // IGS
    IgsPgm,
    IgsPgm2,
    // Cave
    CaveFirstGen,
    CaveCv1000,
    // Midway
    MidwayWolfUnit,
    MidwayTUnit,
    MidwayVUnit,
    MidwayYUnit,
    MidwaySeattle,
    // Namco
    NamcoSystem1,
    NamcoSystem2,
    NamcoSystem11,
    NamcoSystem12,
    NamcoSystem22,
    NamcoSystem23,
    // Konami
    KonamiGx,
    KonamiMysticWarriors,
    KonamiGq,
    KonamiM2,
    // Data East
    DataEastDeco32,
    DataEastDecoCassette,
    // Irem
    IremM72,
    IremM92,
    // Jaleco
    JalecoMegaSystem1,
}

impl ArcadeBoard {
    /// Every variant, deterministic order. Used by the search recognizer and
    /// any future "list all boards" UI.
    pub const ALL: &'static [ArcadeBoard] = &[
        ArcadeBoard::Cps1,
        ArcadeBoard::Cps2,
        ArcadeBoard::Cps3,
        ArcadeBoard::NeoGeoMvs,
        ArcadeBoard::SegaSystemC2,
        ArcadeBoard::SegaSystem16a,
        ArcadeBoard::SegaSystem16b,
        ArcadeBoard::SegaSystem18,
        ArcadeBoard::SegaSystem24,
        ArcadeBoard::SegaSystem32,
        ArcadeBoard::SegaModel1,
        ArcadeBoard::SegaModel2,
        ArcadeBoard::SegaModel3,
        ArcadeBoard::SegaStv,
        ArcadeBoard::SegaNaomi,
        ArcadeBoard::SegaNaomi2,
        ArcadeBoard::SammyAtomiswave,
        ArcadeBoard::TaitoF2,
        ArcadeBoard::TaitoF3,
        ArcadeBoard::TaitoZ,
        ArcadeBoard::IgsPgm,
        ArcadeBoard::IgsPgm2,
        ArcadeBoard::CaveFirstGen,
        ArcadeBoard::CaveCv1000,
        ArcadeBoard::MidwayWolfUnit,
        ArcadeBoard::MidwayTUnit,
        ArcadeBoard::MidwayVUnit,
        ArcadeBoard::MidwayYUnit,
        ArcadeBoard::MidwaySeattle,
        ArcadeBoard::NamcoSystem1,
        ArcadeBoard::NamcoSystem2,
        ArcadeBoard::NamcoSystem11,
        ArcadeBoard::NamcoSystem12,
        ArcadeBoard::NamcoSystem22,
        ArcadeBoard::NamcoSystem23,
        ArcadeBoard::KonamiGx,
        ArcadeBoard::KonamiMysticWarriors,
        ArcadeBoard::KonamiGq,
        ArcadeBoard::KonamiM2,
        ArcadeBoard::DataEastDeco32,
        ArcadeBoard::DataEastDecoCassette,
        ArcadeBoard::IremM72,
        ArcadeBoard::IremM92,
        ArcadeBoard::JalecoMegaSystem1,
    ];

    /// Stable ASCII slug stored in `arcade_game.board` and
    /// `game_library.board`. Never change without a coordinated catalog +
    /// library rebuild.
    pub const fn as_tag(self) -> &'static str {
        match self {
            ArcadeBoard::Cps1 => "cps1",
            ArcadeBoard::Cps2 => "cps2",
            ArcadeBoard::Cps3 => "cps3",
            ArcadeBoard::NeoGeoMvs => "neogeo_mvs",
            ArcadeBoard::SegaSystemC2 => "sega_system_c2",
            ArcadeBoard::SegaSystem16a => "sega_system_16a",
            ArcadeBoard::SegaSystem16b => "sega_system_16b",
            ArcadeBoard::SegaSystem18 => "sega_system_18",
            ArcadeBoard::SegaSystem24 => "sega_system_24",
            ArcadeBoard::SegaSystem32 => "sega_system_32",
            ArcadeBoard::SegaModel1 => "sega_model_1",
            ArcadeBoard::SegaModel2 => "sega_model_2",
            ArcadeBoard::SegaModel3 => "sega_model_3",
            ArcadeBoard::SegaStv => "sega_stv",
            ArcadeBoard::SegaNaomi => "sega_naomi",
            ArcadeBoard::SegaNaomi2 => "sega_naomi_2",
            ArcadeBoard::SammyAtomiswave => "sammy_atomiswave",
            ArcadeBoard::TaitoF2 => "taito_f2",
            ArcadeBoard::TaitoF3 => "taito_f3",
            ArcadeBoard::TaitoZ => "taito_z",
            ArcadeBoard::IgsPgm => "igs_pgm",
            ArcadeBoard::IgsPgm2 => "igs_pgm2",
            ArcadeBoard::CaveFirstGen => "cave_first_gen",
            ArcadeBoard::CaveCv1000 => "cave_cv1000",
            ArcadeBoard::MidwayWolfUnit => "midway_wolf_unit",
            ArcadeBoard::MidwayTUnit => "midway_t_unit",
            ArcadeBoard::MidwayVUnit => "midway_v_unit",
            ArcadeBoard::MidwayYUnit => "midway_y_unit",
            ArcadeBoard::MidwaySeattle => "midway_seattle",
            ArcadeBoard::NamcoSystem1 => "namco_system_1",
            ArcadeBoard::NamcoSystem2 => "namco_system_2",
            ArcadeBoard::NamcoSystem11 => "namco_system_11",
            ArcadeBoard::NamcoSystem12 => "namco_system_12",
            ArcadeBoard::NamcoSystem22 => "namco_system_22",
            ArcadeBoard::NamcoSystem23 => "namco_system_23",
            ArcadeBoard::KonamiGx => "konami_gx",
            ArcadeBoard::KonamiMysticWarriors => "konami_mystic_warriors",
            ArcadeBoard::KonamiGq => "konami_gq",
            ArcadeBoard::KonamiM2 => "konami_m2",
            ArcadeBoard::DataEastDeco32 => "dataeast_deco32",
            ArcadeBoard::DataEastDecoCassette => "dataeast_deco_cassette",
            ArcadeBoard::IremM72 => "irem_m72",
            ArcadeBoard::IremM92 => "irem_m92",
            ArcadeBoard::JalecoMegaSystem1 => "jaleco_mega_system_1",
        }
    }

    /// User-facing label rendered on detail pages and (later) filter pills.
    pub const fn display_name(self) -> &'static str {
        match self {
            ArcadeBoard::Cps1 => "CPS-1",
            ArcadeBoard::Cps2 => "CPS-2",
            ArcadeBoard::Cps3 => "CPS-3",
            ArcadeBoard::NeoGeoMvs => "Neo Geo MVS",
            ArcadeBoard::SegaSystemC2 => "System C-2",
            ArcadeBoard::SegaSystem16a => "System 16A",
            ArcadeBoard::SegaSystem16b => "System 16B",
            ArcadeBoard::SegaSystem18 => "System 18",
            ArcadeBoard::SegaSystem24 => "System 24",
            ArcadeBoard::SegaSystem32 => "System 32",
            ArcadeBoard::SegaModel1 => "Model 1",
            ArcadeBoard::SegaModel2 => "Model 2",
            ArcadeBoard::SegaModel3 => "Model 3",
            ArcadeBoard::SegaStv => "ST-V",
            ArcadeBoard::SegaNaomi => "Naomi",
            ArcadeBoard::SegaNaomi2 => "Naomi 2",
            ArcadeBoard::SammyAtomiswave => "Atomiswave",
            ArcadeBoard::TaitoF2 => "F2 System",
            ArcadeBoard::TaitoF3 => "F3 System",
            ArcadeBoard::TaitoZ => "Z System",
            ArcadeBoard::IgsPgm => "PGM",
            ArcadeBoard::IgsPgm2 => "PGM2",
            ArcadeBoard::CaveFirstGen => "Cave 1st Generation",
            ArcadeBoard::CaveCv1000 => "CV1000",
            ArcadeBoard::MidwayWolfUnit => "Wolf Unit",
            ArcadeBoard::MidwayTUnit => "T-Unit",
            ArcadeBoard::MidwayVUnit => "V-Unit",
            ArcadeBoard::MidwayYUnit => "Y-Unit",
            ArcadeBoard::MidwaySeattle => "Seattle",
            ArcadeBoard::NamcoSystem1 => "System 1",
            ArcadeBoard::NamcoSystem2 => "System 2",
            ArcadeBoard::NamcoSystem11 => "System 11",
            ArcadeBoard::NamcoSystem12 => "System 12",
            ArcadeBoard::NamcoSystem22 => "System 22",
            ArcadeBoard::NamcoSystem23 => "System 23",
            ArcadeBoard::KonamiGx => "GX",
            ArcadeBoard::KonamiMysticWarriors => "Mystic Warriors",
            ArcadeBoard::KonamiGq => "GQ",
            ArcadeBoard::KonamiM2 => "M2",
            ArcadeBoard::DataEastDeco32 => "DECO32",
            ArcadeBoard::DataEastDecoCassette => "DECO Cassette",
            ArcadeBoard::IremM72 => "M72",
            ArcadeBoard::IremM92 => "M92",
            ArcadeBoard::JalecoMegaSystem1 => "Mega System 1",
        }
    }

    /// Manufacturer label used only at display time (board metadata grid,
    /// future tooltip). Never indexed or stored.
    pub const fn manufacturer(self) -> &'static str {
        match self {
            ArcadeBoard::Cps1 | ArcadeBoard::Cps2 | ArcadeBoard::Cps3 => "Capcom",
            ArcadeBoard::NeoGeoMvs => "SNK",
            ArcadeBoard::SegaSystemC2
            | ArcadeBoard::SegaSystem16a
            | ArcadeBoard::SegaSystem16b
            | ArcadeBoard::SegaSystem18
            | ArcadeBoard::SegaSystem24
            | ArcadeBoard::SegaSystem32
            | ArcadeBoard::SegaModel1
            | ArcadeBoard::SegaModel2
            | ArcadeBoard::SegaModel3
            | ArcadeBoard::SegaStv
            | ArcadeBoard::SegaNaomi
            | ArcadeBoard::SegaNaomi2 => "Sega",
            ArcadeBoard::SammyAtomiswave => "Sammy",
            ArcadeBoard::TaitoF2 | ArcadeBoard::TaitoF3 | ArcadeBoard::TaitoZ => "Taito",
            ArcadeBoard::IgsPgm | ArcadeBoard::IgsPgm2 => "IGS",
            ArcadeBoard::CaveFirstGen | ArcadeBoard::CaveCv1000 => "Cave",
            ArcadeBoard::MidwayWolfUnit
            | ArcadeBoard::MidwayTUnit
            | ArcadeBoard::MidwayVUnit
            | ArcadeBoard::MidwayYUnit
            | ArcadeBoard::MidwaySeattle => "Midway",
            ArcadeBoard::NamcoSystem1
            | ArcadeBoard::NamcoSystem2
            | ArcadeBoard::NamcoSystem11
            | ArcadeBoard::NamcoSystem12
            | ArcadeBoard::NamcoSystem22
            | ArcadeBoard::NamcoSystem23 => "Namco",
            ArcadeBoard::KonamiGx
            | ArcadeBoard::KonamiMysticWarriors
            | ArcadeBoard::KonamiGq
            | ArcadeBoard::KonamiM2 => "Konami",
            ArcadeBoard::DataEastDeco32 | ArcadeBoard::DataEastDecoCassette => "Data East",
            ArcadeBoard::IremM72 | ArcadeBoard::IremM92 => "Irem",
            ArcadeBoard::JalecoMegaSystem1 => "Jaleco",
        }
    }

    /// UI label combining the board name with its manufacturer, e.g.
    /// `"F3 System (Taito)"`. Use this anywhere a board is shown to the user
    /// (board page title, search blocks, game-detail metadata). The bare
    /// [`Self::display_name`] stays manufacturer-free for recognizer tokens.
    pub fn display_label(self) -> String {
        format!("{} ({})", self.display_name(), self.manufacturer())
    }

    /// Inverse of [`Self::as_tag`]. Returns `None` for empty or unknown tags.
    pub fn from_tag(tag: &str) -> Option<Self> {
        ArcadeBoard::ALL
            .iter()
            .copied()
            .find(|board| board.as_tag() == tag)
    }

    /// Every emulator-driver sourcefile spelling that identifies this board,
    /// across all upstreams we ingest. This is the **single source of truth**
    /// for board ↔ sourcefile attribution — [`Self::from_sourcefile`] scans it,
    /// so adding a new upstream spelling is a one-line edit here.
    ///
    /// Each entry is one of:
    /// - MAME current canonical `manufacturer/board.cpp` (e.g. `igs/pgm.cpp`)
    /// - FBNeo's `d_`-stripped form, which often uses a different directory or
    ///   basename (e.g. `pgm/pgm.cpp`, `sega/sys16a.cpp`, `taito/taitoz.cpp`,
    ///   `pre90s/namcos1.cpp`)
    /// - MAME 2003+ legacy bare `board.c` (e.g. `pgm.c`, `system16.c`)
    ///
    /// Callers strip the one parser-shape quirk this table doesn't model — the
    /// FBNeo `d_` basename prefix — *before* lookup. (Flycast doesn't go through
    /// here at all: it maps its CSV directly to a variant via `flycast_board`.)
    pub const fn sourcefiles(self) -> &'static [&'static str] {
        match self {
            ArcadeBoard::Cps1 => &["capcom/cps1.cpp", "cps1.c"],
            ArcadeBoard::Cps2 => &["capcom/cps2.cpp", "cps2.c"],
            ArcadeBoard::Cps3 => &["capcom/cps3.cpp", "cps3/cps3.cpp", "cps3.c"],
            ArcadeBoard::NeoGeoMvs => &["neogeo/neogeo.cpp", "neogeo.c"],
            ArcadeBoard::SegaSystemC2 => &["sega/segac2.cpp", "segac2.c"],
            ArcadeBoard::SegaSystem16a => &["sega/segas16a.cpp", "sega/sys16a.cpp", "segas16a.c"],
            ArcadeBoard::SegaSystem16b => &[
                "sega/segas16b.cpp",
                "sega/sys16b.cpp",
                "segas16b.c",
                "system16.c",
            ],
            ArcadeBoard::SegaSystem18 => &["sega/segas18.cpp", "segas18.c"],
            ArcadeBoard::SegaSystem24 => &["sega/segas24.cpp", "segas24.c"],
            ArcadeBoard::SegaSystem32 => &["sega/segas32.cpp"],
            ArcadeBoard::SegaModel1 => &["sega/model1.cpp"],
            ArcadeBoard::SegaModel2 => &["sega/model2.cpp"],
            ArcadeBoard::SegaModel3 => &["sega/model3.cpp"],
            ArcadeBoard::SegaStv => &["sega/stv.cpp"],
            ArcadeBoard::SegaNaomi => &["sega/naomi.cpp"],
            ArcadeBoard::SegaNaomi2 => &["sega/naomi2.cpp"],
            ArcadeBoard::SammyAtomiswave => &["sega/atomiswave.cpp"],
            ArcadeBoard::TaitoF2 => &["taito/taitof2.cpp", "taitof2.c"],
            ArcadeBoard::TaitoF3 => &["taito/taitof3.cpp", "taitof3.c"],
            ArcadeBoard::TaitoZ => &["taito/taito_z.cpp", "taito/taitoz.cpp"],
            ArcadeBoard::IgsPgm => &["igs/pgm.cpp", "pgm/pgm.cpp", "pgm.c"],
            ArcadeBoard::IgsPgm2 => &["igs/pgm2.cpp", "pgm2/pgm2.cpp"],
            ArcadeBoard::CaveFirstGen => &["cave/cave.cpp"],
            ArcadeBoard::CaveCv1000 => &["cave/cv1k.cpp"],
            ArcadeBoard::MidwayWolfUnit => &["midway/midwunit.cpp"],
            ArcadeBoard::MidwayTUnit => &["midway/midtunit.cpp"],
            ArcadeBoard::MidwayVUnit => &["midway/midvunit.cpp"],
            ArcadeBoard::MidwayYUnit => &["midway/midyunit.cpp"],
            ArcadeBoard::MidwaySeattle => &["midway/seattle.cpp"],
            ArcadeBoard::NamcoSystem1 => &["namco/namcos1.cpp", "pre90s/namcos1.cpp", "namcos1.c"],
            ArcadeBoard::NamcoSystem2 => &["namco/namcos2.cpp", "pst90s/namcos2.cpp", "namcos2.c"],
            ArcadeBoard::NamcoSystem11 => &["namco/namcos11.cpp", "namcos11.c"],
            ArcadeBoard::NamcoSystem12 => &["namco/namcos12.cpp", "namcos12.c"],
            ArcadeBoard::NamcoSystem22 => &["namco/namcos22.cpp", "namcos22.c"],
            ArcadeBoard::NamcoSystem23 => &["namco/namcos23.cpp"],
            ArcadeBoard::KonamiGx => &["konami/konamigx.cpp", "konamigx.c"],
            ArcadeBoard::KonamiMysticWarriors => &["konami/mystwarr.cpp", "mystwarr.c"],
            ArcadeBoard::KonamiGq => &["konami/konamigq.cpp"],
            ArcadeBoard::KonamiM2 => &["konami/konamim2.cpp"],
            ArcadeBoard::DataEastDeco32 => &["dataeast/deco32.cpp"],
            ArcadeBoard::DataEastDecoCassette => &["dataeast/dec0.cpp"],
            ArcadeBoard::IremM72 => &["irem/m72.cpp", "m72.c"],
            ArcadeBoard::IremM92 => &["irem/m92.cpp", "m92.c"],
            ArcadeBoard::JalecoMegaSystem1 => &["jaleco/megasys1.cpp", "pre90s/megasys1.cpp"],
        }
    }

    /// Resolve a driver sourcefile to its board by scanning every variant's
    /// [`Self::sourcefiles`] list. Accepts the MAME-current canonical form,
    /// the FBNeo `d_`-stripped form, and the MAME 2003+ legacy `.c` form.
    /// Used only at catalog-build time; the result is stored, the sourcefile
    /// discarded. See [`Self::sourcefiles`] for the normalization callers owe.
    pub fn from_sourcefile(sourcefile: &str) -> Option<Self> {
        if sourcefile.is_empty() {
            return None;
        }
        ArcadeBoard::ALL
            .iter()
            .copied()
            .find(|board| board.sourcefiles().contains(&sourcefile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_round_trip_for_every_variant() {
        for board in ArcadeBoard::ALL {
            assert_eq!(
                ArcadeBoard::from_tag(board.as_tag()),
                Some(*board),
                "from_tag({:?}) failed",
                board.as_tag()
            );
        }
    }

    #[test]
    fn all_variants_have_distinct_tags() {
        let mut seen = std::collections::HashSet::new();
        for board in ArcadeBoard::ALL {
            assert!(
                seen.insert(board.as_tag()),
                "duplicate tag: {:?}",
                board.as_tag()
            );
        }
    }

    #[test]
    fn from_tag_unknown_returns_none() {
        assert!(ArcadeBoard::from_tag("").is_none());
        assert!(ArcadeBoard::from_tag("not_a_board").is_none());
    }

    #[test]
    fn from_sourcefile_covers_all_curated_paths() {
        // Every variant must be reachable from at least one canonical sourcefile.
        // Otherwise build-catalog can never set this variant on an arcade_game row.
        let curated_paths: &[(&str, ArcadeBoard)] = &[
            ("capcom/cps2.cpp", ArcadeBoard::Cps2),
            ("neogeo/neogeo.cpp", ArcadeBoard::NeoGeoMvs),
            ("taito/taitof3.cpp", ArcadeBoard::TaitoF3),
            ("sega/naomi2.cpp", ArcadeBoard::SegaNaomi2),
            ("sega/atomiswave.cpp", ArcadeBoard::SammyAtomiswave),
            ("midway/midwunit.cpp", ArcadeBoard::MidwayWolfUnit),
            ("dataeast/dec0.cpp", ArcadeBoard::DataEastDecoCassette),
            ("jaleco/megasys1.cpp", ArcadeBoard::JalecoMegaSystem1),
        ];
        for (sf, expected) in curated_paths {
            assert_eq!(ArcadeBoard::from_sourcefile(sf), Some(*expected), "{sf}");
        }
    }

    #[test]
    fn from_sourcefile_unknown_returns_none() {
        assert!(ArcadeBoard::from_sourcefile("").is_none());
        assert!(ArcadeBoard::from_sourcefile("misc/galaga.cpp").is_none());
        // Caller is responsible for normalization; raw FBNeo path must not match.
        assert!(ArcadeBoard::from_sourcefile("capcom/d_cps2.cpp").is_none());
    }

    #[test]
    fn display_names_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for board in ArcadeBoard::ALL {
            assert!(
                seen.insert(board.display_name()),
                "duplicate display name: {:?}",
                board.display_name()
            );
        }
    }

    #[test]
    fn manufacturer_set_known_for_every_variant() {
        // Smoke test — just ensure every variant returns something.
        for board in ArcadeBoard::ALL {
            assert!(!board.manufacturer().is_empty());
        }
    }

    #[test]
    fn every_variant_has_at_least_one_sourcefile() {
        for board in ArcadeBoard::ALL {
            assert!(
                !board.sourcefiles().is_empty(),
                "{:?} has no sourcefile spellings",
                board.as_tag()
            );
        }
    }

    #[test]
    fn every_sourcefile_round_trips_to_its_board() {
        for board in ArcadeBoard::ALL {
            for sf in board.sourcefiles() {
                assert_eq!(
                    ArcadeBoard::from_sourcefile(sf),
                    Some(*board),
                    "sourcefile {sf:?} did not resolve back to {:?}",
                    board.as_tag()
                );
            }
        }
    }

    #[test]
    fn no_sourcefile_spelling_is_shared_across_boards() {
        // A spelling mapping to two boards would make from_sourcefile
        // order-dependent and silently mis-attribute one of them.
        let mut seen = std::collections::HashSet::new();
        for board in ArcadeBoard::ALL {
            for sf in board.sourcefiles() {
                assert!(
                    seen.insert(*sf),
                    "sourcefile {sf:?} listed on more than one board"
                );
            }
        }
    }

    #[test]
    fn fbneo_and_legacy_spellings_resolve() {
        // FBNeo `d_`-stripped forms with divergent dirs/names.
        assert_eq!(
            ArcadeBoard::from_sourcefile("pgm/pgm.cpp"),
            Some(ArcadeBoard::IgsPgm)
        );
        assert_eq!(
            ArcadeBoard::from_sourcefile("sega/sys16a.cpp"),
            Some(ArcadeBoard::SegaSystem16a)
        );
        assert_eq!(
            ArcadeBoard::from_sourcefile("taito/taitoz.cpp"),
            Some(ArcadeBoard::TaitoZ)
        );
        assert_eq!(
            ArcadeBoard::from_sourcefile("pre90s/namcos1.cpp"),
            Some(ArcadeBoard::NamcoSystem1)
        );
        // MAME 2003+ legacy `.c`, including the `system16` → 16B alias.
        assert_eq!(
            ArcadeBoard::from_sourcefile("system16.c"),
            Some(ArcadeBoard::SegaSystem16b)
        );
        assert_eq!(
            ArcadeBoard::from_sourcefile("pgm.c"),
            Some(ArcadeBoard::IgsPgm)
        );
    }
}
