use rusqlite::OptionalExtension;
use std::collections::HashMap;

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

/// Metadata for an arcade game ROM.
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

fn row_to_info(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArcadeGameInfo> {
    Ok(ArcadeGameInfo {
        rom_name: row.get(0)?,
        display_name: row.get(1)?,
        year: row.get(2)?,
        manufacturer: row.get(3)?,
        players: row.get::<_, i64>(4)? as u8,
        rotation: rotation_from_str(&row.get::<_, String>(5)?),
        status: status_from_str(&row.get::<_, String>(6)?),
        is_clone: row.get::<_, i64>(7)? != 0,
        is_bios: row.get::<_, i64>(8)? != 0,
        parent: row.get(9)?,
        category: row.get(10)?,
        normalized_genre: row.get(11)?,
    })
}

const ARCADE_COLS: &str = "rom_name, display_name, year, manufacturer, players, rotation, status, \
     is_clone, is_bios, parent, category, normalized_genre";

/// Look up arcade game metadata by ROM name (without `.zip` extension).
pub async fn lookup_arcade_game(rom_name: &str) -> Option<ArcadeGameInfo> {
    {
        let rom = rom_name.to_string();
        return crate::catalog_pool::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ARCADE_COLS} FROM arcade_games WHERE rom_name = ?1"
            ))?;
            stmt.query_row([rom.as_str()], row_to_info).optional()
        })
        .await
        .flatten();
    }
}

/// Batch lookup by ROM names. Returns only entries found in the DB.
pub async fn lookup_arcade_games_batch(rom_names: &[&str]) -> HashMap<String, ArcadeGameInfo> {
    {
        if rom_names.is_empty() {
            return HashMap::new();
        }
        let names_json = serde_json::to_string(rom_names).unwrap_or_else(|_| "[]".into());
        return crate::catalog_pool::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ARCADE_COLS} FROM arcade_games \
                 WHERE rom_name IN (SELECT value FROM json_each(?1))"
            ))?;
            let rows = stmt.query_map([names_json.as_str()], |row| {
                let info = row_to_info(row)?;
                Ok((info.rom_name.clone(), info))
            })?;
            rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        })
        .await
        .unwrap_or_default();
    }
}

/// Get the display name for a ROM filename, falling back to the filename itself.
///
/// Accepts filenames with or without an extension.
pub async fn arcade_display_name(filename: &str) -> String {
    let rom_name = replay_control_core::title_utils::filename_stem(filename);
    match lookup_arcade_game(rom_name).await {
        Some(info) => info.display_name,
        None => filename.to_string(),
    }
}

/// Resolve the arcade display name for a ROM, or `None` if the system isn't
/// arcade (or the ROM isn't in the catalog).
///
pub async fn display_name_if_arcade(system: &str, rom_filename: &str) -> Option<String> {
    if !replay_control_core::systems::is_arcade_system(system) {
        return None;
    }
    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    lookup_arcade_game(stem).await.map(|i| i.display_name)
}

/// All build-time arcade release-date rows as (rom_name, year, source) tuples.
///
/// Each tuple: `(rom_name, year, source)` where `source` is `"mame"`,
/// `"fbneo"`, or `"naomi"`. `year` is always a 4-digit string ("YYYY").
pub async fn arcade_release_dates() -> Vec<(String, String, String)> {
    {
        return crate::catalog_pool::with_catalog(|conn| {
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
        .unwrap_or_default();
    }
}

/// MAME version used as the primary data source (from catalog metadata).
pub const MAME_VERSION: &str = "0.285";

/// Total number of entries in the arcade database.
pub async fn entry_count() -> usize {
    {
        return crate::catalog_pool::with_catalog(|conn| {
            conn.query_row("SELECT COUNT(*) FROM arcade_games", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .await
        .unwrap_or(0) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog_pool::{init_test_catalog, using_stub_data};

    #[tokio::test]
    async fn lookup_known_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("mslug6")
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
        let info = lookup_arcade_game("capsnka")
            .await
            .expect("capsnka should exist in DB");
        assert!(info.is_clone);
        assert_eq!(info.parent, "capsnk");
    }

    #[tokio::test]
    async fn lookup_unknown_returns_none() {
        init_test_catalog().await;
        assert!(lookup_arcade_game("nonexistent_rom_xyz").await.is_none());
    }

    #[tokio::test]
    async fn display_name_with_zip() {
        init_test_catalog().await;
        let name = arcade_display_name("mslug6.zip").await;
        assert_eq!(name, "Metal Slug 6");
    }

    #[tokio::test]
    async fn display_name_without_zip() {
        init_test_catalog().await;
        let name = arcade_display_name("mslug6").await;
        assert_eq!(name, "Metal Slug 6");
    }

    #[tokio::test]
    async fn display_name_fallback() {
        init_test_catalog().await;
        let name = arcade_display_name("unknown_game.zip").await;
        assert_eq!(name, "unknown_game.zip");
    }

    #[tokio::test]
    async fn vertical_rotation_game() {
        init_test_catalog().await;
        // anmlbskt is Animal Basket which has ROT270 (vertical)
        let info = lookup_arcade_game("anmlbskt")
            .await
            .expect("anmlbskt should exist");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[tokio::test]
    async fn horizontal_rotation_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("crzytaxi")
            .await
            .expect("crzytaxi should exist");
        assert_eq!(info.rotation, Rotation::Horizontal);
    }

    #[tokio::test]
    async fn lookup_gdrom_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("ikaruga")
            .await
            .expect("ikaruga should exist (GD-ROM game)");
        assert!(info.display_name.starts_with("Ikaruga"));
        assert_eq!(info.year, "2001");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[tokio::test]
    async fn lookup_atomiswave_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("kofxi")
            .await
            .expect("kofxi should exist (Atomiswave game)");
        assert_eq!(info.display_name, "The King of Fighters XI");
        assert_eq!(info.year, "2005");
    }

    #[tokio::test]
    async fn lookup_sf2_from_mame() {
        init_test_catalog().await;
        let info = lookup_arcade_game("sf2")
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
    async fn lookup_pacman_clone() {
        init_test_catalog().await;
        let info = lookup_arcade_game("pacman")
            .await
            .expect("pacman should exist (MAME 2003+)");
        assert_eq!(info.display_name, "Pac-Man (Midway)");
        assert_eq!(info.year, "1980");
        assert!(info.is_clone);
        assert_eq!(info.parent, "puckman");
        assert_eq!(info.category, "Maze / Collect");
    }

    #[tokio::test]
    async fn lookup_dkong_vertical() {
        init_test_catalog().await;
        let info = lookup_arcade_game("dkong")
            .await
            .expect("dkong should exist (MAME 2003+)");
        assert_eq!(info.display_name, "Donkey Kong (US set 1)");
        assert_eq!(info.year, "1981");
        assert_eq!(info.rotation, Rotation::Vertical);
        assert_eq!(info.category, "Platform / Run Jump");
    }

    #[tokio::test]
    async fn lookup_fbneo_only_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("3countba")
            .await
            .expect("3countba should exist (FBNeo-only)");
        assert_eq!(info.display_name, "3 Count Bout / Fire Suplex (NGM-043)");
        assert_eq!(info.year, "1993");
        assert_eq!(info.manufacturer, "SNK");
        assert!(info.is_clone);
        assert_eq!(info.parent, "3countb");
        assert_eq!(info.rotation, Rotation::Unknown);
        assert_eq!(info.status, DriverStatus::Unknown);
    }

    #[tokio::test]
    async fn lookup_mame_current_only_game() {
        init_test_catalog().await;
        let info = lookup_arcade_game("timecris")
            .await
            .expect("timecris should exist (MAME current only)");
        assert_eq!(info.display_name, "Time Crisis (World, TS2 Ver.B)");
        assert_eq!(info.year, "1996");
        assert_eq!(info.manufacturer, "Namco");
        assert_eq!(info.players, 1);
        assert_eq!(info.rotation, Rotation::Horizontal);
        assert_eq!(info.status, DriverStatus::Imperfect);
        assert!(!info.is_clone);
    }

    #[tokio::test]
    async fn lookup_mame_current_overrides_mame2003() {
        init_test_catalog().await;
        let info = lookup_arcade_game("1941r1")
            .await
            .expect("1941r1 should exist");
        assert_eq!(info.display_name, "1941: Counter Attack (World)");
        assert_eq!(info.year, "1990");
        assert!(info.is_clone);
        assert_eq!(info.parent, "1941");
        assert_eq!(info.rotation, Rotation::Vertical);
        assert_eq!(info.status, DriverStatus::Working);
    }

    #[tokio::test]
    async fn lookup_mame_current_preserves_flycast() {
        init_test_catalog().await;
        let info = lookup_arcade_game("ikaruga")
            .await
            .expect("ikaruga should still be Flycast entry");
        assert!(info.display_name.starts_with("Ikaruga"));
        assert_eq!(info.year, "2001");
        assert_eq!(info.rotation, Rotation::Vertical);
    }

    #[tokio::test]
    async fn mame_current_category_overlay() {
        init_test_catalog().await;
        let info = lookup_arcade_game("timecris")
            .await
            .expect("timecris should exist");
        assert!(
            !info.category.is_empty(),
            "timecris should have a category from catver-mame-current.ini"
        );
    }

    #[tokio::test]
    async fn bios_entry_flagged() {
        init_test_catalog().await;
        let info = lookup_arcade_game("neogeo")
            .await
            .expect("neogeo BIOS should exist in DB");
        assert!(info.is_bios, "neogeo should be flagged as BIOS");
    }

    #[tokio::test]
    async fn regular_game_not_bios() {
        init_test_catalog().await;
        let info = lookup_arcade_game("mslug6")
            .await
            .expect("mslug6 should exist");
        assert!(!info.is_bios, "mslug6 should not be flagged as BIOS");
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
        let map = lookup_arcade_games_batch(&["mslug6", "sf2", "does_not_exist"]).await;
        assert_eq!(map.len(), 2, "should find 2 of 3");
        assert!(map.contains_key("mslug6"));
        assert!(map.contains_key("sf2"));
    }

    #[tokio::test]
    async fn batch_lookup_empty() {
        init_test_catalog().await;
        let map = lookup_arcade_games_batch(&[]).await;
        assert!(map.is_empty());
    }
}
