use std::collections::HashMap;

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
}

/// Single-source row as stored in the `arcade_games` table.
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
    })
}

/// Column name list driving both the SELECT projection (`ARCADE_COLS`) and
/// the runtime schema check at `catalog_pool::init_catalog`. Single source
/// of truth — adding/removing/renaming a column here is the one edit that
/// flows to both sites. Keep `ARCADE_COLS` below in lockstep.
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
];

const ARCADE_COLS: &str = "rom_name, source, display_name, year, manufacturer, players, rotation, status, \
     is_clone, is_bios, parent, category, normalized_genre";

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
    let rows: Vec<SourceRow> = crate::catalog_pool::with_catalog(move |conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ARCADE_COLS} FROM arcade_games WHERE rom_name = ?1"
        ))?;
        let rows = stmt.query_map([rom.as_str()], row_to_source_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
    .await
    .unwrap_or_default();

    if rows.is_empty() {
        return None;
    }
    let mut slots: SourceRows = Default::default();
    for row in rows {
        let i = row.source.idx();
        slots[i] = Some(row);
    }
    Some(merge_for_system(rom_name, &slots, system))
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
    let rows: Vec<SourceRow> = crate::catalog_pool::with_catalog(move |conn| {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {ARCADE_COLS} FROM arcade_games \
             WHERE rom_name IN (SELECT value FROM json_each(?1))"
        ))?;
        let rows = stmt.query_map([names_json.as_str()], row_to_source_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
    .await
    .unwrap_or_default();

    group_by_rom(rows)
        .into_iter()
        .map(|(rom, slots)| {
            let merged = merge_for_system(&rom, &slots, system);
            (rom, merged)
        })
        .collect()
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
            "SELECT rom_name, year, source FROM arcade_release_dates ORDER BY rom_name",
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
            "SELECT COUNT(DISTINCT rom_name) FROM arcade_games",
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
