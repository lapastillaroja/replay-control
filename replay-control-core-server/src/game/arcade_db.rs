use std::collections::HashMap;

use replay_control_core::arcade_board::ArcadeBoard;
use replay_control_core::systems::{ArcadeSource, arcade_source_priority};

/// Fixed-size table of one optional row per source, indexed by `ArcadeSource::idx()`.
/// Avoids heap allocation per merge and lets the merger walk priorities in O(1).
type SourceRows = [Option<SourceRow>; 4];

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

/// Per-system merged metadata for an arcade game ROM.
///
/// Built by [`merge_for_system`] from one row per upstream source — see
/// [`arcade_source_priority`] for the priority order.
#[derive(Debug, Clone)]
pub struct ArcadeGameInfo {
    pub rom_name: String,
    pub display_name: String,
    pub year: String,
    pub manufacturer: String,
    pub players: u8,
    pub rotation: Rotation,
    pub status: DriverStatus,
    pub is_clone: bool,
    pub is_bios: bool,
    pub parent: String,
    pub category: String,
    pub normalized_genre: String,
    /// Curated arcade board (CPS-2, Neo Geo MVS, Taito F3, …) resolved at
    /// catalog-build time from the upstream MAME driver sourcefile. `None`
    /// for unmapped or non-arcade rows.
    pub board: Option<ArcadeBoard>,
    /// True when the catver category carries the `* Mature *` marker (adult /
    /// strip mahjong, etc.); shown as metadata/audit info.
    pub is_mature: bool,
    /// RetroAchievements game id, resolved at catalog-build time by matching
    /// `md5(lowercase rom_name)` against RA's Arcade hash set. Empty when the
    /// romset has no RA set. (The matched hash itself stays in the catalog as
    /// `arcade_game.ra_hash` and is not read at runtime.)
    pub ra_id: String,
    /// Distinct display names from the *non-winning* sources, for thumbnail
    /// matching only. The per-system priority merge picks one `display_name`,
    /// but libretro-thumbnails may file a game under a different emulator's
    /// name (e.g. MAME 0.285 "Atomic Runner Chelnov (World)" vs the repo's
    /// MAME-current "Chelnov - Atomic Runner (World)"). Trying these recovers
    /// covers that exist upstream under an alternate curated name. Excludes
    /// `display_name` and empties; order follows [`ArcadeSource::ALL`].
    pub alt_display_names: Vec<String>,
}

/// Single-source row as stored in the `arcade_game` table.
#[derive(Debug, Clone)]
struct SourceRow {
    rom_name: String,
    source: ArcadeSource,
    display_name: String,
    year: String,
    manufacturer: String,
    players: u8,
    rotation: Rotation,
    status: DriverStatus,
    is_clone: bool,
    is_bios: bool,
    parent: String,
    category: String,
    normalized_genre: String,
    board: Option<ArcadeBoard>,
    ra_id: String,
    is_mature: bool,
}

fn rotation_from_str(s: &str) -> Rotation {
    match s {
        "horizontal" => Rotation::Horizontal,
        "vertical" => Rotation::Vertical,
        _ => Rotation::Unknown,
    }
}

fn status_from_str(s: &str) -> DriverStatus {
    match s {
        "working" => DriverStatus::Working,
        "imperfect" => DriverStatus::Imperfect,
        "preliminary" => DriverStatus::Preliminary,
        _ => DriverStatus::Unknown,
    }
}

fn row_to_source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
    let source_tag: String = row.get(1)?;
    let source = ArcadeSource::from_tag(&source_tag).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            format!("unknown arcade source tag: {source_tag}").into(),
        )
    })?;
    let board_tag: String = row.get(13)?;
    Ok(SourceRow {
        rom_name: row.get(0)?,
        source,
        display_name: row.get(2)?,
        year: row.get(3)?,
        manufacturer: row.get(4)?,
        players: row.get::<_, i64>(5)? as u8,
        rotation: rotation_from_str(&row.get::<_, String>(6)?),
        status: status_from_str(&row.get::<_, String>(7)?),
        is_clone: row.get::<_, i64>(8)? != 0,
        is_bios: row.get::<_, i64>(9)? != 0,
        parent: row.get(10)?,
        category: row.get(11)?,
        normalized_genre: row.get(12)?,
        board: ArcadeBoard::from_tag(&board_tag),
        ra_id: row.get(14)?,
        is_mature: row.get::<_, i64>(15)? != 0,
    })
}

/// Full `arcade_game` column set, driving the runtime schema check at
/// `catalog_pool::init_catalog` (an exact column-set match). This must list
/// **every** column the table has — including `ra_hash`, which the runtime
/// never reads but the catalog stores as RA reference data.
///
/// `ARCADE_COLS` below is the SELECT *projection* and is intentionally a subset
/// (it omits `ra_hash`); `row_to_source_row` reads by that projection's order.
pub(crate) const ARCADE_COL_NAMES: &[&str] = &[
    "rom_name",
    "source",
    "display_name",
    "year",
    "manufacturer",
    "players",
    "rotation",
    "status",
    "is_clone",
    "is_bios",
    "parent",
    "category",
    "normalized_genre",
    "board",
    "ra_id",
    "ra_hash",
    "is_mature",
];

const ARCADE_COLS: &str = "rom_name, source, display_name, year, manufacturer, players, rotation, status, \
     is_clone, is_bios, parent, category, normalized_genre, board, ra_id, is_mature";

/// Merge a `rom_name`'s per-source rows into a single `ArcadeGameInfo`,
/// walking the system's priority list first and falling back to any
/// remaining source. For each field, the first source with a non-default
/// value wins; booleans take the first source that has a row at all
/// (since `false` is a valid value, not "missing").
fn merge_for_system(rom_name: &str, rows: &SourceRows, system: &str) -> ArcadeGameInfo {
    let mut info = ArcadeGameInfo {
        rom_name: rom_name.to_string(),
        display_name: String::new(),
        year: String::new(),
        manufacturer: String::new(),
        players: 0,
        rotation: Rotation::Unknown,
        status: DriverStatus::Unknown,
        is_clone: false,
        is_bios: false,
        parent: String::new(),
        category: String::new(),
        normalized_genre: String::new(),
        board: None,
        is_mature: false,
        ra_id: String::new(),
        alt_display_names: Vec::new(),
    };
    let mut got_bool_decision = false;

    let mut apply = |src: ArcadeSource| {
        let Some(row) = rows[src.idx()].as_ref() else {
            return;
        };
        if info.display_name.is_empty() {
            info.display_name = row.display_name.clone();
        }
        if info.year.is_empty() {
            info.year = row.year.clone();
        }
        if info.manufacturer.is_empty() {
            info.manufacturer = row.manufacturer.clone();
        }
        if info.players == 0 {
            info.players = row.players;
        }
        if info.rotation == Rotation::Unknown {
            info.rotation = row.rotation;
        }
        if info.status == DriverStatus::Unknown {
            info.status = row.status;
        }
        if info.parent.is_empty() {
            info.parent = row.parent.clone();
        }
        if info.category.is_empty() {
            info.category = row.category.clone();
        }
        if info.normalized_genre.is_empty() {
            info.normalized_genre = row.normalized_genre.clone();
        }
        if info.ra_id.is_empty() {
            info.ra_id = row.ra_id.clone();
        }
        // Mature if *any* source flags it — a content property, not per-source
        // metadata the priority merge would pick a single winner for.
        info.is_mature |= row.is_mature;
        if !got_bool_decision {
            info.is_clone = row.is_clone;
            info.is_bios = row.is_bios;
            got_bool_decision = true;
        }
    };

    let priority = arcade_source_priority(system);
    for &src in priority {
        apply(src);
    }
    // Fallback: any source not yet visited via the priority list.
    for src in ArcadeSource::ALL {
        if !priority.contains(&src) {
            apply(src);
        }
    }

    // Collect the other sources' distinct display names as thumbnail-matching
    // alternates (see `alt_display_names`). Order follows `ArcadeSource::ALL`.
    for src in ArcadeSource::ALL {
        if let Some(row) = rows[src.idx()].as_ref() {
            let name = &row.display_name;
            if !name.is_empty()
                && *name != info.display_name
                && !info.alt_display_names.contains(name)
            {
                info.alt_display_names.push(name.clone());
            }
        }
    }

    // Board gets its own fixed source order, independent of the per-system
    // metadata priority above. A board is a physical property of the PCB —
    // the same no matter which emulator's metadata names it — so it should
    // not follow, say, `arcade_mame`'s "MAME first" preference. FBNeo is
    // Replay's primary arcade core and carries the richest board coverage,
    // so it outranks MAME 2003+ (legacy, enabled only on older Pis). MAME
    // 0.285's compact XML has no `sourcefile`, so it never contributes a
    // board. Naomi leads because GD-ROM board hints (Naomi / Naomi 2 /
    // Atomiswave) live only in that source.
    const BOARD_PRIORITY: &[ArcadeSource] = &[
        ArcadeSource::Naomi,
        ArcadeSource::Fbneo,
        ArcadeSource::Mame2k3p,
        ArcadeSource::Mame,
    ];
    // Explicit order first, then a sweep of any source not named above — the
    // same belt-and-suspenders the metadata loop uses, so a newly added
    // `ArcadeSource` still contributes a board rather than silently dropping.
    for &src in BOARD_PRIORITY.iter().chain(
        ArcadeSource::ALL
            .iter()
            .filter(|s| !BOARD_PRIORITY.contains(s)),
    ) {
        if info.board.is_some() {
            break;
        }
        if let Some(row) = rows[src.idx()].as_ref() {
            info.board = row.board;
        }
    }

    info
}

/// Group raw source rows by `rom_name` into the fixed-size table the merger
/// expects. The `[Option<SourceRow>; 4]` slot is indexed by `ArcadeSource::idx()`.
fn group_by_rom(rows: Vec<SourceRow>) -> HashMap<String, SourceRows> {
    let mut grouped: HashMap<String, SourceRows> = HashMap::new();
    for row in rows {
        let slot = row.source.idx();
        let entry = grouped.entry(row.rom_name.clone()).or_default();
        entry[slot] = Some(row);
    }
    grouped
}

/// Look up merged arcade game metadata for a ROM, picking field values from
/// the upstream sources in `system`'s priority order.
///
/// Returns `None` only if the ROM isn't present in any source.
pub async fn lookup_arcade_game(system: &str, rom_name: &str) -> Option<ArcadeGameInfo> {
    let rom = rom_name.to_string();
    let rom_for_merge = rom_name.to_string();
    let system_for_merge = system.to_string();
    crate::catalog_pool::with_catalog(move |conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ARCADE_COLS} FROM arcade_game WHERE rom_name = ?1"
        ))?;
        let rows = stmt
            .query_map([rom.as_str()], row_to_source_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() {
            return Ok(None);
        }
        let mut slots: SourceRows = Default::default();
        for row in rows {
            let i = row.source.idx();
            slots[i] = Some(row);
        }
        Ok(Some(merge_for_system(
            &rom_for_merge,
            &slots,
            &system_for_merge,
        )))
    })
    .await
    .flatten()
}

/// Batch lookup by ROM names. Returns only entries found in the DB.
///
/// Each entry's fields are merged using `system`'s priority order.
pub async fn lookup_arcade_games_batch(
    system: &str,
    rom_names: &[&str],
) -> HashMap<String, ArcadeGameInfo> {
    if rom_names.is_empty() {
        return HashMap::new();
    }
    let names_json = serde_json::to_string(rom_names).unwrap_or_else(|_| "[]".into());
    let system = system.to_string();
    crate::catalog_pool::with_catalog(move |conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ARCADE_COLS} FROM arcade_game \
             WHERE rom_name IN (SELECT value FROM json_each(?1))"
        ))?;
        let rows = stmt
            .query_map([names_json.as_str()], row_to_source_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(group_by_rom(rows)
            .into_iter()
            .map(|(rom, slots)| {
                let merged = merge_for_system(&rom, &slots, &system);
                (rom, merged)
            })
            .collect::<HashMap<String, ArcadeGameInfo>>())
    })
    .await
    .unwrap_or_default()
}

/// Get the display name for an arcade ROM filename on a specific system.
///
/// Picks the curated upstream's name matching the system, falling back to
/// other sources field-by-field, and finally to the filename if the ROM
/// isn't in the catalog at all.
///
/// Accepts filenames with or without an extension.
pub async fn arcade_display_name_for_system(system: &str, filename: &str) -> String {
    let rom_name = replay_control_core::title_utils::filename_stem(filename);
    match lookup_arcade_game(system, rom_name).await {
        Some(info) if !info.display_name.is_empty() => info.display_name,
        _ => filename.to_string(),
    }
}

/// Resolve the arcade display name for a ROM, or `None` if the system isn't
/// arcade (or the ROM isn't in the catalog).
pub async fn display_name_if_arcade(system: &str, rom_filename: &str) -> Option<String> {
    if !replay_control_core::systems::is_arcade_system(system) {
        return None;
    }
    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    lookup_arcade_game(system, stem)
        .await
        .map(|i| i.display_name)
        .filter(|s| !s.is_empty())
}

/// All build-time arcade release-date rows as (rom_name, year, source) tuples.
///
/// Each tuple: `(rom_name, year, source)` where `source` is `"mame"`,
/// `"fbneo"`, or `"naomi"`. `year` is always a 4-digit string ("YYYY").
pub async fn arcade_release_dates() -> Vec<(String, String, String)> {
    crate::catalog_pool::with_catalog(|conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT rom_name, year, source FROM arcade_release_date ORDER BY rom_name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
    .await
    .unwrap_or_default()
}

/// MAME version used as the primary data source (from catalog metadata).
pub const MAME_VERSION: &str = "0.285";

/// Total number of distinct ROM names in the arcade database.
///
/// (Multiple source rows per ROM are de-duplicated.)
pub async fn entry_count() -> usize {
    crate::catalog_pool::with_catalog(|conn| {
        conn.query_row(
            "SELECT COUNT(DISTINCT rom_name) FROM arcade_game",
            [],
            |row| row.get::<_, i64>(0),
        )
    })
    .await
    .unwrap_or(0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog_pool::{init_test_catalog, using_stub_data};

    #[tokio::test]
    async fn lookup_known_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "mslug6")
            .await
            .expect("mslug6 should exist in DB");
        assert_eq!(info.display_name, "Metal Slug 6");
        assert_eq!(info.year, "2006");
        assert!(!info.is_clone);
        assert!(info.parent.is_empty());
    }

    #[tokio::test]
    async fn lookup_clone() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "capsnka")
            .await
            .expect("capsnka should exist in DB");
        assert!(info.is_clone);
        assert_eq!(info.parent, "capsnk");
    }

    #[tokio::test]
    async fn lookup_unknown_returns_none() {
        init_test_catalog().await;
        assert!(
            lookup_arcade_game("arcade_mame", "nonexistent_rom_xyz")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn board_resolved_from_mame_sourcefile() {
        // sf2's MAME 0.285 fixture row carries `sourcefile="capcom/cps2.cpp"`,
        // which build-catalog resolves to `ArcadeBoard::Cps2` at insert time.
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "sf2")
            .await
            .expect("sf2 should exist");
        let board = info.board.expect("CPS-2 board should resolve");
        assert_eq!(board, ArcadeBoard::Cps2);
        assert_eq!(board.display_name(), "CPS-2");
        assert_eq!(board.manufacturer(), "Capcom");
    }

    #[tokio::test]
    async fn board_resolved_from_fbneo_sourcefile_with_d_prefix_stripped() {
        // 3countb's FBNeo fixture row carries `sourcefile="neogeo/d_neogeo.cpp"`.
        // build-catalog normalizes the `d_` prefix away at insert time before
        // resolving to `ArcadeBoard::NeoGeoMvs`.
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_fbneo", "3countb")
            .await
            .expect("3countb should exist (FBNeo)");
        let board = info.board.expect("Neo Geo MVS board should resolve");
        assert_eq!(board, ArcadeBoard::NeoGeoMvs);
        assert_eq!(board.display_name(), "Neo Geo MVS");
        assert_eq!(board.manufacturer(), "SNK");
    }

    #[tokio::test]
    async fn unmapped_or_missing_sourcefile_leaves_board_none() {
        // 1941 has no sourcefile in any fixture; board stays None.
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "1941")
            .await
            .expect("1941 should exist");
        assert!(info.board.is_none());
    }

    #[tokio::test]
    async fn display_name_with_zip() {
        init_test_catalog().await;
        let name = arcade_display_name_for_system("arcade_mame", "mslug6.zip").await;
        assert_eq!(name, "Metal Slug 6");
    }

    #[tokio::test]
    async fn display_name_without_zip() {
        init_test_catalog().await;
        let name = arcade_display_name_for_system("arcade_mame", "mslug6").await;
        assert_eq!(name, "Metal Slug 6");
    }

    /// Minimal `SourceRow` carrying only the fields the merge cares about here.
    fn src_row(source: ArcadeSource, display_name: &str) -> SourceRow {
        SourceRow {
            rom_name: "chelnov".to_string(),
            source,
            display_name: display_name.to_string(),
            year: String::new(),
            manufacturer: String::new(),
            players: 0,
            rotation: Rotation::Unknown,
            status: DriverStatus::Unknown,
            is_clone: false,
            is_bios: false,
            parent: String::new(),
            category: String::new(),
            normalized_genre: String::new(),
            board: None,
            ra_id: String::new(),
            is_mature: false,
        }
    }

    #[test]
    fn merge_collects_distinct_non_winner_alt_names() {
        // chelnov: MAME 0.285 and FBNeo agree on one name; MAME 2003+ differs
        // (the name libretro-thumbnails actually files the cover under).
        let mut rows: SourceRows = Default::default();
        rows[ArcadeSource::Mame.idx()] =
            Some(src_row(ArcadeSource::Mame, "Atomic Runner Chelnov (World)"));
        rows[ArcadeSource::Fbneo.idx()] = Some(src_row(
            ArcadeSource::Fbneo,
            "Atomic Runner Chelnov (World)",
        ));
        rows[ArcadeSource::Mame2k3p.idx()] = Some(src_row(
            ArcadeSource::Mame2k3p,
            "Chelnov - Atomic Runner (World)",
        ));

        // arcade_mame priority is [Mame, Mame2k3p, Fbneo] -> MAME name wins.
        let info = merge_for_system("chelnov", &rows, "arcade_mame");
        assert_eq!(info.display_name, "Atomic Runner Chelnov (World)");
        // The winner is excluded and the FBNeo duplicate is deduped, leaving
        // exactly the one distinct alternate (the matching libretro name).
        assert_eq!(
            info.alt_display_names,
            vec!["Chelnov - Atomic Runner (World)".to_string()]
        );
    }

    #[test]
    fn merge_has_no_alts_when_all_sources_agree() {
        let mut rows: SourceRows = Default::default();
        rows[ArcadeSource::Mame.idx()] = Some(src_row(ArcadeSource::Mame, "Metal Slug 6"));
        rows[ArcadeSource::Fbneo.idx()] = Some(src_row(ArcadeSource::Fbneo, "Metal Slug 6"));
        let info = merge_for_system("mslug6", &rows, "arcade_mame");
        assert_eq!(info.display_name, "Metal Slug 6");
        assert!(info.alt_display_names.is_empty());
    }

    #[tokio::test]
    async fn display_name_fallback() {
        init_test_catalog().await;
        let name = arcade_display_name_for_system("arcade_mame", "unknown_game.zip").await;
        assert_eq!(name, "unknown_game.zip");
    }

    #[tokio::test]
    async fn fbneo_only_game_falls_back_to_fbneo_on_mame_system() {
        init_test_catalog().await;
        // 3countba is in FBNeo only. arcade_mame's priority is [mame,
        // mame_2k3p, fbneo] — first two are absent, fbneo wins.
        let info = lookup_arcade_game("arcade_mame", "3countba")
            .await
            .expect("3countba should exist (FBNeo-only)");
        assert_eq!(info.display_name, "3 Count Bout / Fire Suplex (NGM-043)");
        assert_eq!(info.manufacturer, "SNK");
    }

    #[tokio::test]
    async fn merge_pulls_fields_from_different_sources() {
        init_test_catalog().await;
        // pacman exists in MAME 2003+ (with year/players/category) and
        // potentially MAME current. For arcade_fbneo system the priority is
        // [fbneo, mame, mame_2k3p]; fbneo's pacman row is unlikely so the
        // merge should still produce all-fields-set result via fallback.
        let info = lookup_arcade_game("arcade_fbneo", "pacman")
            .await
            .expect("pacman should exist");
        assert!(!info.display_name.is_empty(), "display_name should be set");
        assert!(!info.year.is_empty(), "year should be set");
        assert!(info.players > 0, "players should be set");
    }

    #[tokio::test]
    async fn vertical_rotation_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "anmlbskt")
            .await
            .expect("anmlbskt should exist");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[tokio::test]
    async fn horizontal_rotation_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "crzytaxi")
            .await
            .expect("crzytaxi should exist");
        assert_eq!(info.rotation, Rotation::Horizontal);
    }

    #[tokio::test]
    async fn lookup_atomiswave_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_dc", "kofxi")
            .await
            .expect("kofxi should exist (Atomiswave)");
        assert_eq!(info.display_name, "The King of Fighters XI");
        assert_eq!(info.year, "2005");
    }

    #[tokio::test]
    async fn lookup_sf2_from_mame() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "sf2")
            .await
            .expect("sf2 should exist (MAME current)");
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

    #[tokio::test]
    async fn lookup_dkong_vertical() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame_2k3p", "dkong")
            .await
            .expect("dkong should exist");
        assert_eq!(info.display_name, "Donkey Kong (US set 1)");
        assert_eq!(info.year, "1981");
        assert_eq!(info.rotation, Rotation::Vertical);
        assert_eq!(info.category, "Platform / Run Jump");
    }

    #[tokio::test]
    async fn bios_entry_flagged() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "neogeo")
            .await
            .expect("neogeo BIOS should exist");
        assert!(info.is_bios);
    }

    #[tokio::test]
    async fn regular_game_not_bios() {
        init_test_catalog().await;
        let info = lookup_arcade_game("arcade_mame", "mslug6")
            .await
            .expect("mslug6 should exist");
        assert!(!info.is_bios);
    }

    #[tokio::test]
    async fn total_entry_count() {
        init_test_catalog().await;
        let min_expected = if using_stub_data() { 14 } else { 15000 };
        let count = entry_count().await;
        assert!(
            count >= min_expected,
            "Expected {min_expected}+ entries, got {count}"
        );
    }

    #[tokio::test]
    async fn batch_lookup() {
        init_test_catalog().await;
        let map =
            lookup_arcade_games_batch("arcade_mame", &["mslug6", "sf2", "does_not_exist"]).await;
        assert_eq!(map.len(), 2, "should find 2 of 3");
        assert!(map.contains_key("mslug6"));
        assert!(map.contains_key("sf2"));
    }

    #[tokio::test]
    async fn batch_lookup_empty() {
        init_test_catalog().await;
        let map = lookup_arcade_games_batch("arcade_mame", &[]).await;
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn unknown_system_uses_default_priority() {
        init_test_catalog().await;
        // Non-arcade system → empty priority → falls back to deterministic
        // mame > mame_2k3p > fbneo > naomi order. Should still resolve mslug6.
        let info = lookup_arcade_game("nintendo_snes", "mslug6")
            .await
            .expect("mslug6 should exist regardless of system");
        assert_eq!(info.display_name, "Metal Slug 6");
    }
}
