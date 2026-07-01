use replay_control_core::error::{Error, Result};
use replay_control_core::stats::{
    StatsDashboard, LibrarySummary, SystemStat, GenreStat, DecadeStat, DeveloperStat,
    PlayerModeStat, VariantStat, MetadataCoverage,
};
use replay_control_core::systems;
use rusqlite::Connection;

/// Games shown in the library UI — excludes clones, hacks, translations, and
/// romset-special entries. Dashboard totals and coverage percentages count only
/// these so the numbers match what the user actually sees; otherwise a
/// clone-heavy system (e.g. 54k ZX Spectrum ROMs) drags every percentage down.
/// Mirrors the visibility filter used across recommendations/relationships.
const VISIBLE: &str = "is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0";

fn count(conn: &Connection, sql: &str) -> Result<usize> {
    conn.query_row(sql, [], |r| r.get::<_, i64>(0))
        .map(|v| v as usize)
        .map_err(|e| Error::Other(format!("query: {sql}: {e}")))
}

/// Compute the complete stats dashboard from the library database.
pub fn compute_dashboard(conn: &Connection) -> Result<StatsDashboard> {
    let summary = compute_library_summary(conn)?;
    let systems = compute_system_stats(conn)?;
    let genres = compute_genre_stats(conn, summary.total_games)?;
    let decades = compute_decade_stats(conn)?;
    let developers = compute_developer_stats(conn)?;
    let player_modes = compute_player_mode_stats(conn)?;
    let variants = compute_variant_stats(conn)?;
    let metadata_coverage = compute_metadata_coverage(conn, summary.total_games)?;

    Ok(StatsDashboard {
        summary,
        systems,
        genres,
        decades,
        developers,
        player_modes,
        variants,
        metadata_coverage,
    })
}

fn compute_library_summary(conn: &Connection) -> Result<LibrarySummary> {
    let total_games = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE {VISIBLE}"))?;
    let total_systems = count(conn, &format!("SELECT COUNT(DISTINCT system) FROM game_library WHERE {VISIBLE}"))?;
    let total_size_bytes: u64 = conn
        .query_row(&format!("SELECT COALESCE(SUM(size_bytes), 0) FROM game_library WHERE {VISIBLE}"), [], |r| {
            r.get::<_, i64>(0).map(|v| v as u64)
        })
        .map_err(|e| Error::Other(format!("sum size: {e}")))?;

    let arcade_count = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE driver_status IS NOT NULL AND {VISIBLE}"))?;

    Ok(LibrarySummary {
        total_games,
        total_systems,
        total_size_bytes,
        total_favorites: 0,
        arcade_count,
    })
}

fn compute_system_stats(conn: &Connection) -> Result<Vec<SystemStat>> {
    let mut stmt = conn
        .prepare(
            &format!("SELECT system, COUNT(*) as game_count, COALESCE(SUM(size_bytes), 0) as total_size
             FROM game_library
             WHERE {VISIBLE}
             GROUP BY system
             ORDER BY game_count DESC"),
        )
        .map_err(|e| Error::Other(format!("prepare system stats: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|e| Error::Other(format!("query system stats: {e}")))?;

    let mut stats = Vec::new();
    for row in rows {
        let (system, game_count, size_bytes) = row.map_err(|e| Error::Other(format!("row: {e}")))?;
        let display_name = systems::find_system(&system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.clone());

        stats.push(SystemStat {
            system,
            display_name,
            game_count,
            size_bytes,
            favorite_count: 0,
        });
    }

    Ok(stats)
}

fn compute_genre_stats(conn: &Connection, total_games: usize) -> Result<Vec<GenreStat>> {
    let mut stmt = conn
        .prepare(
            &format!("SELECT genre_group, COUNT(*) as cnt
             FROM game_library
             WHERE genre_group != '' AND {VISIBLE}
             GROUP BY genre_group
             ORDER BY cnt DESC
             LIMIT 15"),
        )
        .map_err(|e| Error::Other(format!("prepare genre stats: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(|e| Error::Other(format!("query genre stats: {e}")))?;

    let mut stats = Vec::new();
    for row in rows {
        let (genre, cnt) = row.map_err(|e| Error::Other(format!("row: {e}")))?;
        let percentage = if total_games > 0 {
            (cnt as f64 / total_games as f64) * 100.0
        } else {
            0.0
        };
        stats.push(GenreStat {
            genre,
            count: cnt,
            percentage,
        });
    }

    Ok(stats)
}

fn compute_decade_stats(conn: &Connection) -> Result<Vec<DecadeStat>> {
    let mut stmt = conn
        .prepare(
            &format!("SELECT CAST(SUBSTR(release_date, 1, 3) AS INTEGER) * 10 as decade, COUNT(*) as cnt
             FROM game_library
             WHERE release_date IS NOT NULL AND LENGTH(release_date) >= 4 AND {VISIBLE}
             GROUP BY decade
             ORDER BY decade"),
        )
        .map_err(|e| Error::Other(format!("prepare decade stats: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, u16>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(|e| Error::Other(format!("query decade stats: {e}")))?;

    let mut stats = Vec::new();
    for row in rows {
        let (decade, count) = row.map_err(|e| Error::Other(format!("row: {e}")))?;
        stats.push(DecadeStat { decade, count });
    }

    Ok(stats)
}

fn compute_developer_stats(conn: &Connection) -> Result<Vec<DeveloperStat>> {
    let mut stmt = conn
        .prepare(
            &format!("SELECT developer, COUNT(*) as cnt
             FROM game_library
             WHERE developer != '' AND {VISIBLE}
             GROUP BY developer
             ORDER BY cnt DESC
             LIMIT 15"),
        )
        .map_err(|e| Error::Other(format!("prepare developer stats: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(|e| Error::Other(format!("query developer stats: {e}")))?;

    let mut stats = Vec::new();
    for row in rows {
        let (developer, cnt) = row.map_err(|e| Error::Other(format!("row: {e}")))?;
        stats.push(DeveloperStat {
            developer,
            count: cnt,
            game_count: cnt,
        });
    }

    Ok(stats)
}

fn compute_player_mode_stats(conn: &Connection) -> Result<PlayerModeStat> {
    let single_player = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE players = 1 AND {VISIBLE}"))?;
    let multiplayer = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE players > 1 AND cooperative = 0 AND {VISIBLE}"))?;
    let cooperative = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE cooperative = 1 AND {VISIBLE}"))?;
    let unknown = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE (players IS NULL OR players = 0) AND {VISIBLE}"))?;

    Ok(PlayerModeStat {
        single_player,
        multiplayer,
        cooperative,
        unknown,
    })
}

fn compute_variant_stats(conn: &Connection) -> Result<VariantStat> {
    // Intentionally NOT filtered by VISIBLE: this section reports how many of
    // those otherwise-hidden entries exist (clones/hacks/translations/special).
    let clones = count(conn, "SELECT COUNT(*) FROM game_library WHERE is_clone = 1")?;
    let hacks = count(conn, "SELECT COUNT(*) FROM game_library WHERE is_hack = 1")?;
    let translations = count(conn, "SELECT COUNT(*) FROM game_library WHERE is_translation = 1")?;
    let special = count(conn, "SELECT COUNT(*) FROM game_library WHERE is_special = 1")?;
    let verified = count(conn, "SELECT COUNT(*) FROM game_library WHERE hash_matched_name IS NOT NULL")?;

    Ok(VariantStat {
        clones,
        hacks,
        translations,
        special,
        verified,
    })
}

fn compute_metadata_coverage(conn: &Connection, total_games: usize) -> Result<MetadataCoverage> {
    let pct = |count: usize| -> f64 {
        if total_games > 0 {
            (count as f64 / total_games as f64) * 100.0
        } else {
            0.0
        }
    };

    let with_genre = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE genre_group != '' AND {VISIBLE}"))?;
    let with_developer = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE developer != '' AND {VISIBLE}"))?;
    let with_rating = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE rating IS NOT NULL AND {VISIBLE}"))?;
    let with_boxart = count(conn, &format!("SELECT COUNT(*) FROM game_library WHERE box_art_url IS NOT NULL AND {VISIBLE}"))?;
    // game_metadata is a legacy table with no visibility columns; scope its
    // screenshot count to visible games via the game_library join.
    let with_screenshot = count(conn,
        "SELECT COUNT(*) FROM game_metadata gm
         WHERE gm.screenshot_path IS NOT NULL
           AND EXISTS (SELECT 1 FROM game_library gl
                       WHERE gl.system = gm.system AND gl.rom_filename = gm.rom_filename
                         AND gl.is_clone = 0 AND gl.is_translation = 0
                         AND gl.is_hack = 0 AND gl.is_special = 0)"
    )
    .unwrap_or(0);

    Ok(MetadataCoverage {
        with_genre,
        genre_pct: pct(with_genre),
        with_developer,
        developer_pct: pct(with_developer),
        with_rating,
        rating_pct: pct(with_rating),
        with_boxart,
        boxart_pct: pct(with_boxart),
        with_screenshot,
        screenshot_pct: pct(with_screenshot),
    })
}
