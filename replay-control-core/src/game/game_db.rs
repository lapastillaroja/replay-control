#[cfg(not(target_arch = "wasm32"))]
use rusqlite::OptionalExtension;
use std::collections::HashMap;

/// Shared metadata for a canonical game (one per unique game title per system).
#[derive(Debug, Clone)]
pub struct CanonicalGame {
    pub display_name: String,
    pub year: u16,
    pub genre: String,
    pub developer: String,
    pub publisher: String,
    pub players: u8,
    pub coop: Option<bool>,
    pub rating: String,
    pub normalized_genre: String,
}

/// Metadata for a specific ROM file variant.
#[derive(Debug, Clone)]
pub struct GameEntry {
    pub canonical_name: String,
    pub region: String,
    pub crc32: u32,
    pub game: CanonicalGame,
}

#[cfg(not(target_arch = "wasm32"))]
fn row_to_canonical_game(row: &rusqlite::Row<'_>) -> rusqlite::Result<CanonicalGame> {
    let coop_val: Option<i64> = row.get(6)?;
    Ok(CanonicalGame {
        display_name: row.get(0)?,
        year: row.get::<_, i64>(1)? as u16,
        genre: row.get(2)?,
        developer: row.get(3)?,
        publisher: row.get(4)?,
        players: row.get::<_, i64>(5)? as u8,
        coop: coop_val.map(|v| v != 0),
        rating: row.get(7)?,
        normalized_genre: row.get(8)?,
    })
}

// Columns: re.filename_stem(0), re.region(1), re.crc32(2),
//          cg.display_name(3)…cg.normalized_genre(11)
#[cfg(not(target_arch = "wasm32"))]
fn row_to_game_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameEntry> {
    let coop_val: Option<i64> = row.get(9)?;
    Ok(GameEntry {
        canonical_name: row.get(0)?,
        region: row.get(1)?,
        crc32: row.get::<_, i64>(2)? as u32,
        game: CanonicalGame {
            display_name: row.get(3)?,
            year: row.get::<_, i64>(4)? as u16,
            genre: row.get(5)?,
            developer: row.get(6)?,
            publisher: row.get(7)?,
            players: row.get::<_, i64>(8)? as u8,
            coop: coop_val.map(|v| v != 0),
            rating: row.get(10)?,
            normalized_genre: row.get(11)?,
        },
    })
}

#[cfg(not(target_arch = "wasm32"))]
const ENTRY_COLS: &str = "re.filename_stem, re.region, re.crc32, \
     cg.display_name, cg.year, cg.genre, cg.developer, cg.publisher, \
     cg.players, cg.coop, cg.rating, cg.normalized_genre";

#[cfg(not(target_arch = "wasm32"))]
const CANONICAL_COLS: &str = "cg.display_name, cg.year, cg.genre, cg.developer, cg.publisher, \
     cg.players, cg.coop, cg.rating, cg.normalized_genre";

/// Look up game metadata by filename stem (without extension) for a system.
pub async fn lookup_game(system: &str, filename_stem: &str) -> Option<GameEntry> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        let stem = filename_stem.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 AND re.filename_stem = ?2"
            ))?;
            stmt.query_row(rusqlite::params![system, stem], row_to_game_entry)
                .optional()
        })
        .await
        .flatten();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, filename_stem);
        None
    }
}

/// Batch lookup by filename stems for a system. Returns entries keyed by the
/// `canonical_name` (filename stem) that was found.
pub async fn lookup_games_batch(
    system: &str,
    stems: &[&str],
) -> HashMap<String, GameEntry> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if stems.is_empty() {
            return HashMap::new();
        }
        let system = system.to_string();
        let stems_json = serde_json::to_string(stems).unwrap_or_else(|_| "[]".into());
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 \
                   AND re.filename_stem IN (SELECT value FROM json_each(?2))"
            ))?;
            let rows = stmt.query_map(rusqlite::params![system, stems_json], |row| {
                let entry = row_to_game_entry(row)?;
                Ok((entry.canonical_name.clone(), entry))
            })?;
            rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, stems);
        HashMap::new()
    }
}

/// Look up game metadata by CRC32 hash for a system.
pub async fn lookup_by_crc(system: &str, crc32: u32) -> Option<GameEntry> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 AND re.crc32 = ?2 \
                 LIMIT 1"
            ))?;
            stmt.query_row(rusqlite::params![system, crc32 as i64], row_to_game_entry)
                .optional()
        })
        .await
        .flatten();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, crc32);
        None
    }
}

/// Batch CRC32 lookup. Returns entries keyed by their crc32.
pub async fn lookup_by_crcs_batch(
    system: &str,
    crcs: &[u32],
) -> HashMap<u32, GameEntry> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if crcs.is_empty() {
            return HashMap::new();
        }
        let system = system.to_string();
        let crcs_i64: Vec<i64> = crcs.iter().map(|c| *c as i64).collect();
        let crcs_json = serde_json::to_string(&crcs_i64).unwrap_or_else(|_| "[]".into());
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 \
                   AND re.crc32 IN (SELECT value FROM json_each(?2))"
            ))?;
            let rows = stmt.query_map(rusqlite::params![system, crcs_json], |row| {
                let entry = row_to_game_entry(row)?;
                Ok((entry.crc32, entry))
            })?;
            // Deduplicate: first match per crc32 wins.
            let mut map: HashMap<u32, GameEntry> = HashMap::new();
            for row in rows {
                let (k, v) = row?;
                map.entry(k).or_insert(v);
            }
            Ok(map)
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, crcs);
        HashMap::new()
    }
}

/// Look up a canonical game by normalized title for a system.
pub async fn lookup_by_normalized_title(
    system: &str,
    normalized: &str,
) -> Option<CanonicalGame> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        let norm = normalized.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {CANONICAL_COLS} \
                 FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 AND re.normalized_title = ?2 \
                 LIMIT 1"
            ))?;
            stmt.query_row(rusqlite::params![system, norm], row_to_canonical_game)
                .optional()
        })
        .await
        .flatten();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, normalized);
        None
    }
}

/// Batch lookup of canonical games by normalized title. Results are keyed by
/// the normalized title that was found in the DB.
pub async fn lookup_by_normalized_titles_batch(
    system: &str,
    normalized: &[&str],
) -> HashMap<String, CanonicalGame> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if normalized.is_empty() {
            return HashMap::new();
        }
        let system = system.to_string();
        let norms_json = serde_json::to_string(normalized).unwrap_or_else(|_| "[]".into());
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT re.normalized_title, {CANONICAL_COLS} \
                 FROM rom_entries re \
                 JOIN canonical_games cg ON cg.id = re.canonical_game_id \
                 WHERE re.system = ?1 \
                   AND re.normalized_title IN (SELECT value FROM json_each(?2))"
            ))?;
            let rows = stmt.query_map(rusqlite::params![system, norms_json], |row| {
                let norm: String = row.get(0)?;
                let coop_val: Option<i64> = row.get(7)?;
                let cg = CanonicalGame {
                    display_name: row.get(1)?,
                    year: row.get::<_, i64>(2)? as u16,
                    genre: row.get(3)?,
                    developer: row.get(4)?,
                    publisher: row.get(5)?,
                    players: row.get::<_, i64>(6)? as u8,
                    coop: coop_val.map(|v| v != 0),
                    rating: row.get(8)?,
                    normalized_genre: row.get(9)?,
                };
                Ok((norm, cg))
            })?;
            // Deduplicate: first match per normalized_title wins.
            let mut map: HashMap<String, CanonicalGame> = HashMap::new();
            for row in rows {
                let (k, v) = row?;
                map.entry(k).or_insert(v);
            }
            Ok(map)
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, normalized);
        HashMap::new()
    }
}

/// Normalize a ROM filename stem for fuzzy title matching.
pub fn normalize_filename(stem: &str) -> String {
    let base = stem
        .find(['(', '['])
        .map(|i| &stem[..i])
        .unwrap_or(stem)
        .trim();

    let mut result = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }

    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

/// Get the display name for a ROM file on a given system.
pub async fn game_display_name(system: &str, filename: &str) -> Option<String> {
    let stem = crate::title_utils::filename_stem(filename);

    if let Some(entry) = lookup_game(system, stem).await {
        return Some(entry.game.display_name);
    }

    let normalized = normalize_filename(stem);
    if !normalized.is_empty()
        && let Some(game) = lookup_by_normalized_title(system, &normalized).await
    {
        return Some(game.display_name);
    }

    if stem.contains('~') {
        for part in stem.split('~') {
            let part_normalized = normalize_filename(part.trim());
            if !part_normalized.is_empty()
                && let Some(game) = lookup_by_normalized_title(system, &part_normalized).await
            {
                return Some(game.display_name);
            }
        }
    }

    None
}

/// Batch version of `game_display_name`. Returns a map keyed by the input
/// filename (as provided) to its resolved display name.
///
/// Tries exact filename-stem match first, then normalized title for misses.
pub async fn display_names_batch(
    system: &str,
    filenames: &[&str],
) -> HashMap<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if filenames.is_empty() {
            return HashMap::new();
        }

        let stems: Vec<(&str, &str)> = filenames
            .iter()
            .map(|f| (*f, crate::title_utils::filename_stem(f)))
            .collect();

        let stems_for_exact: Vec<&str> = stems.iter().map(|(_, s)| *s).collect();
        let mut exact = lookup_games_batch(system, &stems_for_exact).await;

        let mut out: HashMap<String, String> = HashMap::new();
        let mut missing: Vec<(String, String)> = Vec::new(); // (filename, normalized)

        for (filename, stem) in &stems {
            if let Some(entry) = exact.remove(*stem) {
                out.insert((*filename).to_string(), entry.game.display_name);
            } else {
                let normalized = normalize_filename(stem);
                if !normalized.is_empty() {
                    missing.push(((*filename).to_string(), normalized));
                }
            }
        }

        if !missing.is_empty() {
            let norms: Vec<&str> = missing.iter().map(|(_, n)| n.as_str()).collect();
            let fuzzy = lookup_by_normalized_titles_batch(system, &norms).await;
            for (filename, normalized) in missing {
                if let Some(cg) = fuzzy.get(&normalized) {
                    out.insert(filename, cg.display_name.clone());
                }
            }
        }

        out
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (system, filenames);
        HashMap::new()
    }
}

/// Systems that have game DB coverage.
pub async fn supported_systems() -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return crate::game::with_catalog(|conn| {
            let mut stmt = conn
                .prepare_cached("SELECT DISTINCT system FROM canonical_games ORDER BY system")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        vec![]
    }
}

/// Total number of ROM entries across all systems.
pub async fn total_rom_entries() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return crate::game::with_catalog(|conn| {
            conn.query_row("SELECT COUNT(*) FROM rom_entries", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .await
        .unwrap_or(0) as usize;
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Number of distinct systems in the canonical games table.
pub async fn system_count() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return crate::game::with_catalog(|conn| {
            conn.query_row(
                "SELECT COUNT(DISTINCT system) FROM canonical_games",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .unwrap_or(0) as usize;
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

pub async fn has_system(system: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT COUNT(*) FROM canonical_games WHERE system = ?1 LIMIT 1",
            )?;
            stmt.query_row(rusqlite::params![system], |row| row.get::<_, i64>(0))
        })
        .await
        .unwrap_or(0)
            > 0;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = system;
        false
    }
}

/// Get TGDB alternate names for a system.
///
/// Returns `(canonical_game_id, alternate_name)` pairs.
pub async fn system_alternates(system: &str) -> Vec<(u32, Vec<String>)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT canonical_game_id, alternate_name FROM rom_alternates \
                 WHERE system = ?1 ORDER BY canonical_game_id",
            )?;
            let rows = stmt.query_map(rusqlite::params![system], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, String>(1)?))
            })?;
            let pairs: Vec<(u32, String)> = rows.collect::<rusqlite::Result<Vec<_>>>()?;

            let mut grouped: Vec<(u32, Vec<String>)> = Vec::new();
            for (id, alt) in pairs {
                if grouped.last().is_none_or(|(last_id, _)| *last_id != id) {
                    grouped.push((id, vec![alt]));
                } else if let Some(last) = grouped.last_mut() {
                    last.1.push(alt);
                }
            }
            Ok(grouped)
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = system;
        vec![]
    }
}

/// Get the canonical games for a system.
pub async fn system_games(system: &str) -> Vec<CanonicalGame> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system = system.to_string();
        return crate::game::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT display_name, year, genre, developer, publisher, players, coop, rating, \
                 normalized_genre FROM canonical_games WHERE system = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map(rusqlite::params![system], row_to_canonical_game)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = system;
        vec![]
    }
}

/// All per-region release-date rows from catalog.
///
/// Each tuple: `(system_folder, base_title_lowercased, region, release_date, precision, source)`.
pub async fn console_release_dates() -> Vec<(String, String, String, String, String, String)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        return crate::game::with_catalog(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT system, base_title, region, release_date, precision, source \
                 FROM console_release_dates ORDER BY system, base_title",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
    #[cfg(target_arch = "wasm32")]
    {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{init_test_catalog, using_stub_data};

    #[tokio::test]
    async fn supported_systems_not_empty() {
        init_test_catalog().await;
        let systems = supported_systems().await;
        assert!(
            !systems.is_empty(),
            "Game DB should have at least one supported system"
        );
        assert!(systems.iter().any(|s| s == "nintendo_nes"));
        assert!(systems.iter().any(|s| s == "nintendo_snes"));
        assert!(systems.iter().any(|s| s == "sega_smd"));
        assert!(systems.iter().any(|s| s == "sega_sg"));
        assert!(systems.iter().any(|s| s == "sega_32x"));
    }

    #[tokio::test]
    async fn has_system_checks() {
        init_test_catalog().await;
        assert!(has_system("nintendo_nes").await);
        assert!(has_system("sega_smd").await);
        assert!(!has_system("arcade_fbneo").await);
        assert!(!has_system("nonexistent_system").await);
    }

    #[tokio::test]
    async fn lookup_nonexistent_system() {
        init_test_catalog().await;
        assert!(lookup_game("nonexistent_system", "anything").await.is_none());
    }

    #[tokio::test]
    async fn lookup_nonexistent_rom() {
        init_test_catalog().await;
        assert!(
            lookup_game("nintendo_nes", "nonexistent_rom_xyz_123")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn lookup_nes_super_mario_bros() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_nes", "Super Mario Bros. (World)")
            .await
            .expect("Super Mario Bros. should exist in NES DB");
        assert_eq!(entry.game.display_name, "Super Mario Bros.");
        assert_eq!(entry.region, "World");
        assert!(entry.crc32 != 0, "CRC32 should be set");
    }

    #[tokio::test]
    async fn lookup_nes_zelda() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_nes", "Legend of Zelda, The (USA)")
            .await
            .expect("Zelda should exist in NES DB");
        assert_eq!(entry.game.display_name, "The Legend of Zelda");
        assert_eq!(entry.region, "USA");
    }

    #[tokio::test]
    async fn nes_display_name_with_extension() {
        init_test_catalog().await;
        let name = game_display_name("nintendo_nes", "Super Mario Bros. (World).nes").await;
        assert_eq!(name.as_deref(), Some("Super Mario Bros."));
    }

    #[tokio::test]
    async fn lookup_snes_super_mario_world() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_snes", "Super Mario World (USA)")
            .await
            .expect("Super Mario World should exist in SNES DB");
        assert_eq!(entry.game.display_name, "Super Mario World");
        assert_eq!(entry.region, "USA");
    }

    #[tokio::test]
    async fn snes_zelda_link_to_past() {
        init_test_catalog().await;
        let entry = lookup_game(
            "nintendo_snes",
            "Legend of Zelda, The - A Link to the Past (USA)",
        )
        .await
        .expect("Zelda ALTTP should exist in SNES DB");
        assert_eq!(
            entry.game.display_name,
            "The Legend of Zelda - A Link to the Past"
        );
    }

    #[tokio::test]
    async fn canonical_game_shared_across_regions() {
        init_test_catalog().await;
        let usa = lookup_game("nintendo_snes", "Super Mario World (USA)").await;
        let eur = lookup_game("nintendo_snes", "Super Mario World (Europe)").await;
        if let (Some(usa), Some(eur)) = (usa, eur) {
            assert_eq!(
                usa.game.display_name, eur.game.display_name,
                "USA and Europe variants should share the same display name"
            );
        }
    }

    #[tokio::test]
    async fn lookup_smd_sonic() {
        init_test_catalog().await;
        let entry = lookup_game("sega_smd", "Sonic The Hedgehog (USA, Europe)")
            .await
            .expect("Sonic should exist in SMD DB");
        assert_eq!(entry.game.display_name, "Sonic The Hedgehog");
    }

    #[tokio::test]
    async fn sg1000_in_supported_systems() {
        init_test_catalog().await;
        assert!(
            has_system("sega_sg").await,
            "sega_sg should be in supported systems"
        );
    }

    #[tokio::test]
    async fn lookup_sg1000_flicky() {
        init_test_catalog().await;
        let entry = lookup_game("sega_sg", "Flicky (Japan) (Rev 1)")
            .await
            .expect("Flicky should exist in SG-1000 DB");
        assert_eq!(entry.game.display_name, "Flicky");
        assert_eq!(entry.region, "Japan");
    }

    #[tokio::test]
    async fn sg1000_has_players_data() {
        init_test_catalog().await;
        let entry = lookup_game("sega_sg", "Congo Bongo (Japan)")
            .await
            .expect("Congo Bongo should exist in SG-1000 DB");
        assert!(
            entry.game.players > 0,
            "Congo Bongo should have players data"
        );
    }

    #[tokio::test]
    async fn s32x_in_supported_systems() {
        init_test_catalog().await;
        assert!(
            has_system("sega_32x").await,
            "sega_32x should be in supported systems"
        );
    }

    #[tokio::test]
    async fn lookup_32x_doom() {
        init_test_catalog().await;
        let entry = lookup_game("sega_32x", "Doom (Europe)")
            .await
            .expect("Doom should exist in 32X DB");
        assert_eq!(entry.game.display_name, "Doom");
        assert_eq!(entry.region, "Europe");
    }

    #[tokio::test]
    async fn s32x_has_players_data() {
        init_test_catalog().await;
        let entry = lookup_game("sega_32x", "Doom (Europe)")
            .await
            .expect("Doom should exist in 32X DB");
        assert!(entry.game.players > 0, "Doom should have players data");
    }

    #[tokio::test]
    async fn lookup_gb_tetris() {
        init_test_catalog().await;
        let entry = match lookup_game("nintendo_gb", "Tetris (World) (Rev 1)").await {
            Some(e) => e,
            None => lookup_game("nintendo_gb", "Tetris (Japan) (En)")
                .await
                .expect("Tetris should exist in GB DB"),
        };
        assert_eq!(entry.game.display_name, "Tetris");
    }

    #[tokio::test]
    async fn lookup_by_crc_nes() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_nes", "Super Mario Bros. (World)")
            .await
            .expect("SMB should exist");
        let crc = entry.crc32;
        assert!(crc != 0);
        let by_crc = lookup_by_crc("nintendo_nes", crc)
            .await
            .expect("CRC32 lookup should find Super Mario Bros.");
        assert_eq!(by_crc.game.display_name, "Super Mario Bros.");
    }

    #[tokio::test]
    async fn nes_has_genre_data() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_nes", "Super Mario Bros. (World)")
            .await
            .expect("SMB should exist");
        assert!(
            !entry.game.genre.is_empty(),
            "Super Mario Bros. should have genre data, got empty"
        );
    }

    #[tokio::test]
    async fn snes_has_players_data() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_snes", "Super Mario World (USA)")
            .await
            .expect("SMW should exist");
        assert!(
            entry.game.players > 0,
            "Super Mario World should have players data"
        );
    }

    #[tokio::test]
    async fn snes_has_year_data() {
        init_test_catalog().await;
        let entry = lookup_game("nintendo_snes", "Super Mario World (USA)")
            .await
            .expect("SMW should exist");
        assert!(
            entry.game.year > 0,
            "Super Mario World should have a release year"
        );
    }

    #[test]
    fn normalize_filename_strips_tags() {
        assert_eq!(
            normalize_filename("Super Mario World (USA)"),
            "super mario world"
        );
        assert_eq!(
            normalize_filename("Super Mario World (USA) (Rev 1)"),
            "super mario world"
        );
        assert_eq!(
            normalize_filename("Sonic The Hedgehog (USA, Europe)"),
            "sonic the hedgehog"
        );
    }

    #[test]
    fn normalize_filename_strips_punctuation() {
        assert_eq!(
            normalize_filename("Legend of Zelda, The (USA)"),
            "legend of zelda the"
        );
        assert_eq!(
            normalize_filename("Super Mario Bros. (World)"),
            "super mario bros"
        );
    }

    #[test]
    fn normalize_filename_handles_brackets() {
        assert_eq!(
            normalize_filename("Game Name [T-Spa1.0v] (USA)"),
            "game name"
        );
        assert_eq!(normalize_filename("Game Name [!] (USA)"), "game name");
    }

    #[test]
    fn normalize_filename_handles_bare_names() {
        assert_eq!(
            normalize_filename("Battletoads & Double Dragon"),
            "battletoads double dragon"
        );
        assert_eq!(normalize_filename("Doom Troopers"), "doom troopers");
    }

    #[test]
    fn normalize_filename_collapses_whitespace() {
        assert_eq!(normalize_filename("  Game   Name  (USA)  "), "game name");
    }

    #[tokio::test]
    async fn normalized_fallback_finds_game_with_extra_tags() {
        init_test_catalog().await;
        let name = game_display_name(
            "nintendo_snes",
            "Super Mario World (USA) (Traducido Es).smc",
        )
        .await;
        assert_eq!(name.as_deref(), Some("Super Mario World"));
    }

    #[tokio::test]
    async fn normalized_fallback_finds_game_with_60hz_tag() {
        init_test_catalog().await;
        let name =
            game_display_name("nintendo_snes", "Super Mario World (Europe) (60hz).sfc").await;
        assert_eq!(name.as_deref(), Some("Super Mario World"));
    }

    #[tokio::test]
    async fn normalized_fallback_finds_game_with_fastrom_tag() {
        init_test_catalog().await;
        let name =
            game_display_name("nintendo_snes", "Super Mario World (USA) (FastRom).sfc").await;
        assert_eq!(name.as_deref(), Some("Super Mario World"));
    }

    #[tokio::test]
    async fn normalized_fallback_finds_bare_filename() {
        init_test_catalog().await;
        let name = game_display_name("nintendo_snes", "Super Mario World.sfc").await;
        assert_eq!(name.as_deref(), Some("Super Mario World"));
    }

    #[tokio::test]
    async fn exact_match_still_preferred_over_normalized() {
        init_test_catalog().await;
        let name = game_display_name("nintendo_snes", "Super Mario World (USA).sfc").await;
        assert_eq!(name.as_deref(), Some("Super Mario World"));
    }

    #[tokio::test]
    async fn normalized_lookup_nonexistent_game() {
        init_test_catalog().await;
        let name = game_display_name(
            "nintendo_snes",
            "Totally Fake Game That Does Not Exist (USA).sfc",
        )
        .await;
        assert!(name.is_none());
    }

    #[tokio::test]
    async fn lookup_by_normalized_title_smd() {
        init_test_catalog().await;
        let game = lookup_by_normalized_title("sega_smd", "sonic the hedgehog").await;
        assert!(
            game.is_some(),
            "Sonic should be findable by normalized title"
        );
        assert_eq!(game.unwrap().display_name, "Sonic The Hedgehog");
    }

    #[tokio::test]
    async fn total_entry_count() {
        init_test_catalog().await;
        let min_expected = if using_stub_data() { 8 } else { 20000 };
        let total = total_rom_entries().await;
        assert!(
            total >= min_expected,
            "Expected {min_expected}+ total ROM entries across all systems, got {total}"
        );
    }

    #[tokio::test]
    async fn batch_lookup_games() {
        init_test_catalog().await;
        let map = lookup_games_batch(
            "nintendo_snes",
            &["Super Mario World (USA)", "does_not_exist"],
        )
        .await;
        assert!(map.contains_key("Super Mario World (USA)"));
        assert!(!map.contains_key("does_not_exist"));
    }

    #[tokio::test]
    async fn batch_display_names() {
        init_test_catalog().await;
        let map = display_names_batch(
            "nintendo_snes",
            &[
                "Super Mario World (USA).sfc",
                "Super Mario World (Europe) (60hz).sfc",
                "Totally Fake Game.sfc",
            ],
        )
        .await;
        assert_eq!(
            map.get("Super Mario World (USA).sfc").map(String::as_str),
            Some("Super Mario World")
        );
        assert_eq!(
            map.get("Super Mario World (Europe) (60hz).sfc")
                .map(String::as_str),
            Some("Super Mario World")
        );
        assert!(!map.contains_key("Totally Fake Game.sfc"));
    }
}
