//! Materialized per-system library stats.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use replay_control_core::error::{Error, Result};
use replay_control_core::resource_kind;
use replay_control_core::systems::system_display_name;

use super::{
    CountBucket, DownloadedThumbnailStats, DriverStatusCounts, LibraryDb, SystemCoverage,
    SystemStatsRefreshState,
};
use crate::thumbnails::ThumbnailMediaStats;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatsRefreshState {
    #[default]
    Unknown = 0,
    Fresh = 1,
    Stale = 2,
    Refreshing = 3,
    Failed = 4,
}

impl StatsRefreshState {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Fresh,
            2 => Self::Stale,
            3 => Self::Refreshing,
            4 => Self::Failed,
            _ => Self::Unknown,
        }
    }

    fn as_wire(self) -> SystemStatsRefreshState {
        match self {
            Self::Unknown => SystemStatsRefreshState::Unknown,
            Self::Fresh => SystemStatsRefreshState::Fresh,
            Self::Stale => SystemStatsRefreshState::Stale,
            Self::Refreshing => SystemStatsRefreshState::Refreshing,
            Self::Failed => SystemStatsRefreshState::Failed,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct GameLibrarySystemStats {
    system: String,
    rom_count: usize,
    total_size_bytes: u64,
    clone_count: usize,
    hack_count: usize,
    translation_count: usize,
    homebrew_count: usize,
    unlicensed_count: usize,
    special_count: usize,
    mature_count: usize,
    region_counts_json: Option<String>,
    release_year_min: Option<u16>,
    release_year_max: Option<u16>,
    release_date_known_count: usize,
    genre_counts_json: Option<String>,
    genre_group_counts_json: Option<String>,
    developer_known_count: usize,
    publisher_known_count: usize,
    player_count_distribution_json: Option<String>,
    rating_known_count: usize,
    description_count: usize,
    boxart_count: usize,
    snap_count: usize,
    title_screen_count: usize,
    manual_count: usize,
    video_count: usize,
    resource_count: usize,
    coop_count: usize,
    verified_count: usize,
    ra_id_count: usize,
    driver_status_json: Option<String>,
    refresh_state: StatsRefreshState,
    updated_at: Option<i64>,
}

struct OverviewStatsRow {
    system: String,
    rom_count: usize,
    total_size_bytes: u64,
    clone_count: usize,
    hack_count: usize,
    translation_count: usize,
    homebrew_count: usize,
    unlicensed_count: usize,
    special_count: usize,
    mature_count: usize,
    region_counts_json: Option<String>,
    release_year_min: Option<u16>,
    release_year_max: Option<u16>,
    release_date_known_count: usize,
    genre_counts_json: Option<String>,
    genre_group_counts_json: Option<String>,
    developer_known_count: usize,
    publisher_known_count: usize,
    player_count_distribution_json: Option<String>,
    rating_known_count: usize,
    description_count: usize,
    boxart_count: usize,
    snap_count: usize,
    title_screen_count: usize,
    thumbnail_total_size_bytes: u64,
    thumbnail_file_count: usize,
    thumbnail_boxart_file_count: usize,
    thumbnail_snap_file_count: usize,
    thumbnail_title_file_count: usize,
    manual_count: usize,
    video_count: usize,
    resource_count: usize,
    coop_count: usize,
    verified_count: usize,
    ra_id_count: usize,
    driver_status_json: Option<String>,
    refresh_state: StatsRefreshState,
    updated_at: Option<i64>,
}

impl LibraryDb {
    pub fn backfill_missing_game_library_system_stats(conn: &Connection) -> Result<usize> {
        let mut stmt = conn
            .prepare(
                "SELECT m.system
                 FROM game_library_meta m
                 LEFT JOIN game_library_system_stats s ON s.system = m.system
                 WHERE m.rom_count > 0 AND s.system IS NULL
                 ORDER BY m.system",
            )
            .map_err(|e| {
                Error::Other(format!("prepare backfill game_library_system_stats: {e}"))
            })?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("query backfill game_library_system_stats: {e}")))?;

        let mut systems = Vec::new();
        for row in rows {
            systems.push(row.map_err(|e| {
                Error::Other(format!(
                    "read backfill game_library_system_stats system: {e}"
                ))
            })?);
        }

        for system in &systems {
            Self::refresh_game_library_system_stats(conn, system)?;
        }
        Ok(systems.len())
    }

    pub fn refresh_game_library_system_stats(conn: &Connection, system: &str) -> Result<()> {
        let stats = Self::compute_game_library_system_stats(conn, system)?;
        Self::upsert_game_library_system_stats(conn, &stats)
    }

    pub fn refresh_game_library_system_stats_state(
        conn: &Connection,
        system: &str,
        state: StatsRefreshState,
    ) -> Result<()> {
        let mut stats = Self::compute_game_library_system_stats(conn, system)?;
        stats.refresh_state = state;
        Self::upsert_game_library_system_stats(conn, &stats)
    }

    pub fn set_game_library_system_stats_state(
        conn: &Connection,
        system: &str,
        state: StatsRefreshState,
    ) -> Result<usize> {
        conn.execute(
            "UPDATE game_library_system_stats
             SET refresh_state = ?2
             WHERE system = ?1",
            params![system, state.as_i64()],
        )
        .map_err(|e| Error::Other(format!("set game_library_system_stats state: {e}")))
    }

    pub fn refresh_game_library_system_boxart_count(
        conn: &Connection,
        system: &str,
    ) -> Result<usize> {
        conn.execute(
            "UPDATE game_library_system_stats
             SET boxart_count = (
                    SELECT COUNT(*)
                    FROM game_library
                    WHERE system = ?1 AND box_art_url IS NOT NULL
                 ),
                 updated_at = CAST(strftime('%s', 'now') AS INTEGER)
             WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("refresh game_library_system_stats boxart: {e}")))
    }

    pub fn clear_game_library_system_boxart_counts(conn: &Connection) -> Result<usize> {
        conn.execute(
            "UPDATE game_library_system_stats
             SET boxart_count = 0,
                 updated_at = CAST(strftime('%s', 'now') AS INTEGER)",
            [],
        )
        .map_err(|e| Error::Other(format!("clear game_library_system_stats boxart: {e}")))
    }

    pub fn replace_thumbnail_media_stats(
        conn: &mut Connection,
        stats: &[ThumbnailMediaStats],
    ) -> Result<()> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin thumbnail media stats refresh: {e}")))?;
        let now = unix_now();
        tx.execute(
            "UPDATE game_library_system_stats
             SET thumbnail_total_size_bytes = 0,
                 thumbnail_file_count = 0,
                 thumbnail_boxart_file_count = 0,
                 thumbnail_snap_file_count = 0,
                 thumbnail_title_file_count = 0,
                 updated_at = ?1",
            params![now],
        )
        .map_err(|e| Error::Other(format!("clear thumbnail media stats: {e}")))?;
        for stat in stats {
            tx.execute(
                "INSERT INTO game_library_system_stats (
                    system,
                    thumbnail_total_size_bytes,
                    thumbnail_file_count,
                    thumbnail_boxart_file_count,
                    thumbnail_snap_file_count,
                    thumbnail_title_file_count,
                    updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(system) DO UPDATE SET
                    thumbnail_total_size_bytes = excluded.thumbnail_total_size_bytes,
                    thumbnail_file_count = excluded.thumbnail_file_count,
                    thumbnail_boxart_file_count = excluded.thumbnail_boxart_file_count,
                    thumbnail_snap_file_count = excluded.thumbnail_snap_file_count,
                    thumbnail_title_file_count = excluded.thumbnail_title_file_count,
                    updated_at = excluded.updated_at",
                params![
                    stat.system,
                    stat.total_size_bytes as i64,
                    stat.file_count as i64,
                    stat.boxart_file_count as i64,
                    stat.snap_file_count as i64,
                    stat.title_file_count as i64,
                    now,
                ],
            )
            .map_err(|e| Error::Other(format!("upsert thumbnail media stats: {e}")))?;
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("commit thumbnail media stats refresh: {e}")))
    }

    pub fn clear_thumbnail_media_stats(conn: &Connection) -> Result<()> {
        conn.execute(
            "UPDATE game_library_system_stats
             SET thumbnail_total_size_bytes = 0,
                 thumbnail_file_count = 0,
                 thumbnail_boxart_file_count = 0,
                 thumbnail_snap_file_count = 0,
                 thumbnail_title_file_count = 0,
                 updated_at = CAST(strftime('%s', 'now') AS INTEGER)",
            [],
        )
        .map(|_| ())
        .map_err(|e| Error::Other(format!("clear thumbnail media stats: {e}")))
    }

    pub fn thumbnail_media_totals_from_system_stats(
        conn: &Connection,
    ) -> Result<DownloadedThumbnailStats> {
        conn.query_row(
            "SELECT
                COALESCE(SUM(thumbnail_file_count), 0),
                COALESCE(SUM(thumbnail_boxart_file_count), 0),
                COALESCE(SUM(thumbnail_snap_file_count), 0),
                COALESCE(SUM(thumbnail_title_file_count), 0),
                COALESCE(SUM(thumbnail_total_size_bytes), 0)
             FROM game_library_system_stats",
            [],
            |row| {
                Ok(DownloadedThumbnailStats {
                    total_files: row.get::<_, i64>(0).unwrap_or(0).max(0) as usize,
                    boxart_files: row.get::<_, i64>(1).unwrap_or(0).max(0) as usize,
                    snap_files: row.get::<_, i64>(2).unwrap_or(0).max(0) as usize,
                    title_files: row.get::<_, i64>(3).unwrap_or(0).max(0) as usize,
                    total_size_bytes: row.get::<_, i64>(4).unwrap_or(0).max(0) as u64,
                })
            },
        )
        .map_err(|e| Error::Other(format!("read thumbnail media stats: {e}")))
    }

    #[cfg(test)]
    fn load_game_library_system_stats(conn: &Connection) -> Result<Vec<GameLibrarySystemStats>> {
        let mut stmt = conn
            .prepare(
                "SELECT system, rom_count, total_size_bytes, clone_count, hack_count,
                        translation_count, homebrew_count, unlicensed_count, special_count,
                        mature_count, region_counts_json, release_year_min, release_year_max,
                        release_date_known_count, genre_counts_json, genre_group_counts_json,
                        developer_known_count, publisher_known_count,
                        player_count_distribution_json, rating_known_count, description_count,
                        boxart_count, snap_count, title_screen_count, manual_count, video_count,
                        resource_count, coop_count, verified_count, driver_status_json,
                        refresh_state, updated_at, ra_id_count
                 FROM game_library_system_stats
                 ORDER BY system",
            )
            .map_err(|e| Error::Other(format!("prepare load_game_library_system_stats: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GameLibrarySystemStats {
                    system: row.get(0)?,
                    rom_count: row.get::<_, i64>(1).unwrap_or(0) as usize,
                    total_size_bytes: row.get::<_, i64>(2).unwrap_or(0) as u64,
                    clone_count: row.get::<_, i64>(3).unwrap_or(0) as usize,
                    hack_count: row.get::<_, i64>(4).unwrap_or(0) as usize,
                    translation_count: row.get::<_, i64>(5).unwrap_or(0) as usize,
                    homebrew_count: row.get::<_, i64>(6).unwrap_or(0) as usize,
                    unlicensed_count: row.get::<_, i64>(7).unwrap_or(0) as usize,
                    special_count: row.get::<_, i64>(8).unwrap_or(0) as usize,
                    mature_count: row.get::<_, i64>(9).unwrap_or(0) as usize,
                    region_counts_json: row.get(10)?,
                    release_year_min: row.get::<_, Option<i64>>(11)?.map(|v| v as u16),
                    release_year_max: row.get::<_, Option<i64>>(12)?.map(|v| v as u16),
                    release_date_known_count: row.get::<_, i64>(13).unwrap_or(0) as usize,
                    genre_counts_json: row.get(14)?,
                    genre_group_counts_json: row.get(15)?,
                    developer_known_count: row.get::<_, i64>(16).unwrap_or(0) as usize,
                    publisher_known_count: row.get::<_, i64>(17).unwrap_or(0) as usize,
                    player_count_distribution_json: row.get(18)?,
                    rating_known_count: row.get::<_, i64>(19).unwrap_or(0) as usize,
                    description_count: row.get::<_, i64>(20).unwrap_or(0) as usize,
                    boxart_count: row.get::<_, i64>(21).unwrap_or(0) as usize,
                    snap_count: row.get::<_, i64>(22).unwrap_or(0) as usize,
                    title_screen_count: row.get::<_, i64>(23).unwrap_or(0) as usize,
                    manual_count: row.get::<_, i64>(24).unwrap_or(0) as usize,
                    video_count: row.get::<_, i64>(25).unwrap_or(0) as usize,
                    resource_count: row.get::<_, i64>(26).unwrap_or(0) as usize,
                    coop_count: row.get::<_, i64>(27).unwrap_or(0) as usize,
                    verified_count: row.get::<_, i64>(28).unwrap_or(0) as usize,
                    driver_status_json: row.get(29)?,
                    refresh_state: StatsRefreshState::from_i64(row.get::<_, i64>(30)?),
                    updated_at: row.get(31)?,
                    ra_id_count: row.get::<_, i64>(32).unwrap_or(0) as usize,
                })
            })
            .map_err(|e| Error::Other(format!("query load_game_library_system_stats: {e}")))?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row.map_err(|e| Error::Other(format!("read library system stats: {e}")))?);
        }
        Ok(stats)
    }

    pub fn library_overview_from_system_stats(
        conn: &Connection,
    ) -> Result<(super::LibrarySummary, Vec<SystemCoverage>)> {
        let mut stmt = conn
            .prepare(
                "SELECT system, rom_count, total_size_bytes, clone_count, hack_count,
                        translation_count, homebrew_count, unlicensed_count, special_count,
                        mature_count, region_counts_json, release_year_min, release_year_max,
                        release_date_known_count, genre_counts_json, genre_group_counts_json,
                        developer_known_count, publisher_known_count,
                        player_count_distribution_json, rating_known_count, description_count,
                        boxart_count, snap_count, title_screen_count,
                        thumbnail_total_size_bytes, thumbnail_file_count,
                        thumbnail_boxart_file_count, thumbnail_snap_file_count,
                        thumbnail_title_file_count, manual_count, video_count, resource_count,
                        coop_count, verified_count, driver_status_json, refresh_state, updated_at,
                        ra_id_count
                 FROM game_library_system_stats
                 WHERE rom_count > 0",
            )
            .map_err(|e| Error::Other(format!("prepare library overview stats: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OverviewStatsRow {
                    system: row.get(0)?,
                    rom_count: row.get::<_, i64>(1).unwrap_or(0) as usize,
                    total_size_bytes: row.get::<_, i64>(2).unwrap_or(0) as u64,
                    clone_count: row.get::<_, i64>(3).unwrap_or(0) as usize,
                    hack_count: row.get::<_, i64>(4).unwrap_or(0) as usize,
                    translation_count: row.get::<_, i64>(5).unwrap_or(0) as usize,
                    homebrew_count: row.get::<_, i64>(6).unwrap_or(0) as usize,
                    unlicensed_count: row.get::<_, i64>(7).unwrap_or(0) as usize,
                    special_count: row.get::<_, i64>(8).unwrap_or(0) as usize,
                    mature_count: row.get::<_, i64>(9).unwrap_or(0) as usize,
                    region_counts_json: row.get(10)?,
                    release_year_min: row.get::<_, Option<i64>>(11)?.map(|v| v as u16),
                    release_year_max: row.get::<_, Option<i64>>(12)?.map(|v| v as u16),
                    release_date_known_count: row.get::<_, i64>(13).unwrap_or(0) as usize,
                    genre_counts_json: row.get(14)?,
                    genre_group_counts_json: row.get(15)?,
                    developer_known_count: row.get::<_, i64>(16).unwrap_or(0) as usize,
                    publisher_known_count: row.get::<_, i64>(17).unwrap_or(0) as usize,
                    player_count_distribution_json: row.get(18)?,
                    rating_known_count: row.get::<_, i64>(19).unwrap_or(0) as usize,
                    description_count: row.get::<_, i64>(20).unwrap_or(0) as usize,
                    boxart_count: row.get::<_, i64>(21).unwrap_or(0) as usize,
                    snap_count: row.get::<_, i64>(22).unwrap_or(0) as usize,
                    title_screen_count: row.get::<_, i64>(23).unwrap_or(0) as usize,
                    thumbnail_total_size_bytes: row.get::<_, i64>(24).unwrap_or(0) as u64,
                    thumbnail_file_count: row.get::<_, i64>(25).unwrap_or(0) as usize,
                    thumbnail_boxart_file_count: row.get::<_, i64>(26).unwrap_or(0) as usize,
                    thumbnail_snap_file_count: row.get::<_, i64>(27).unwrap_or(0) as usize,
                    thumbnail_title_file_count: row.get::<_, i64>(28).unwrap_or(0) as usize,
                    manual_count: row.get::<_, i64>(29).unwrap_or(0) as usize,
                    video_count: row.get::<_, i64>(30).unwrap_or(0) as usize,
                    resource_count: row.get::<_, i64>(31).unwrap_or(0) as usize,
                    coop_count: row.get::<_, i64>(32).unwrap_or(0) as usize,
                    verified_count: row.get::<_, i64>(33).unwrap_or(0) as usize,
                    driver_status_json: row.get(34)?,
                    refresh_state: StatsRefreshState::from_i64(row.get::<_, i64>(35)?),
                    updated_at: row.get(36)?,
                    ra_id_count: row.get::<_, i64>(37).unwrap_or(0) as usize,
                })
            })
            .map_err(|e| Error::Other(format!("query library overview stats: {e}")))?;

        let mut summary = super::LibrarySummary::default();
        let mut coverage = Vec::new();
        for row in rows {
            let row = row.map_err(|e| Error::Other(format!("read library overview stats: {e}")))?;
            let with_genre = count_json_total(row.genre_counts_json.as_deref());
            let driver_status = row
                .driver_status_json
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok());

            summary.total_games += row.rom_count;
            summary.system_count += 1;
            summary.with_genre += with_genre;
            summary.with_developer += row.developer_known_count;
            summary.with_publisher += row.publisher_known_count;
            summary.with_rating += row.rating_known_count;
            summary.with_release_date += row.release_date_known_count;
            summary.with_box_art += row.boxart_count.min(row.rom_count);
            summary.with_snap += row.snap_count.min(row.rom_count);
            summary.with_title_screen += row.title_screen_count.min(row.rom_count);
            summary.with_manual += row.manual_count.min(row.rom_count);
            summary.with_video += row.video_count.min(row.rom_count);
            summary.with_resource += row.resource_count;
            summary.coop_games += row.coop_count;
            summary.total_size_bytes += row.total_size_bytes;
            summary.downloaded_thumbnail_files += row.thumbnail_file_count;
            summary.downloaded_boxart_files += row.thumbnail_boxart_file_count;
            summary.downloaded_snap_files += row.thumbnail_snap_file_count;
            summary.downloaded_title_files += row.thumbnail_title_file_count;
            summary.downloaded_thumbnail_bytes += row.thumbnail_total_size_bytes;
            summary.min_year = min_optional(summary.min_year, row.release_year_min);
            summary.max_year = max_optional(summary.max_year, row.release_year_max);

            coverage.push(SystemCoverage {
                display_name: system_display_name(&row.system),
                total_games: row.rom_count,
                with_thumbnail: row.boxart_count.min(row.rom_count),
                with_snap: row.snap_count.min(row.rom_count),
                with_title_screen: row.title_screen_count.min(row.rom_count),
                with_manual: row.manual_count.min(row.rom_count),
                with_video: row.video_count.min(row.rom_count),
                with_resource: row.resource_count,
                with_genre,
                with_developer: row.developer_known_count,
                with_publisher: row.publisher_known_count,
                with_rating: row.rating_known_count,
                with_release_date: row.release_date_known_count,
                size_bytes: row.total_size_bytes,
                with_description: row.description_count.min(row.rom_count),
                clone_count: row.clone_count,
                hack_count: row.hack_count,
                translation_count: row.translation_count,
                homebrew_count: row.homebrew_count,
                unlicensed_count: row.unlicensed_count,
                special_count: row.special_count,
                mature_count: row.mature_count,
                coop_count: row.coop_count,
                verified_count: row.verified_count,
                with_ra_id: row.ra_id_count,
                min_year: row.release_year_min,
                max_year: row.release_year_max,
                driver_status,
                downloaded_thumbnail_files: row.thumbnail_file_count,
                downloaded_boxart_files: row.thumbnail_boxart_file_count,
                downloaded_snap_files: row.thumbnail_snap_file_count,
                downloaded_title_files: row.thumbnail_title_file_count,
                downloaded_thumbnail_bytes: row.thumbnail_total_size_bytes,
                stats_refresh_state: row.refresh_state.as_wire(),
                stats_updated_at: row.updated_at,
                region_counts: parse_count_buckets(row.region_counts_json.as_deref()),
                genre_group_counts: parse_count_buckets(row.genre_group_counts_json.as_deref()),
                player_count_distribution: parse_count_buckets(
                    row.player_count_distribution_json.as_deref(),
                ),
                system: row.system,
            });
        }
        coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok((summary, coverage))
    }

    fn compute_game_library_system_stats(
        conn: &Connection,
        system: &str,
    ) -> Result<GameLibrarySystemStats> {
        let Some((rom_count, total_size_bytes)) = conn
            .query_row(
                "SELECT rom_count, total_size_bytes
                 FROM game_library_meta
                 WHERE system = ?1",
                params![system],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|e| Error::Other(format!("query game_library_system_stats meta: {e}")))?
        else {
            conn.execute(
                "DELETE FROM game_library_system_stats WHERE system = ?1",
                params![system],
            )
            .map_err(|e| Error::Other(format!("delete stale game_library_system_stats: {e}")))?;
            return Ok(GameLibrarySystemStats::default());
        };

        // All single-column flag/identity counts + the release-year range in one
        // scan of the system's rows, via conditional aggregation. This replaces
        // ~13 separate `COUNT(*) WHERE system=?1 AND <predicate>` queries that
        // each re-scanned the same rows. COALESCE guards the all-rows-NULL case
        // (a system whose meta row exists but has no game_library rows yet).
        let (
            clone_count,
            hack_count,
            translation_count,
            special_count,
            mature_count,
            release_date_known_count,
            developer_known_count,
            rating_known_count,
            boxart_count,
            coop_count,
            ra_id_count,
            verified_count,
            release_year_min,
            release_year_max,
        ) = conn
            .query_row(
                "SELECT
                    COALESCE(SUM(is_clone = 1), 0),
                    COALESCE(SUM(is_hack = 1), 0),
                    COALESCE(SUM(is_translation = 1), 0),
                    COALESCE(SUM(is_special = 1), 0),
                    COALESCE(SUM(is_mature = 1), 0),
                    COALESCE(SUM(release_date IS NOT NULL AND release_date != ''), 0),
                    COALESCE(SUM(developer != ''), 0),
                    COALESCE(SUM(rating IS NOT NULL), 0),
                    COALESCE(SUM(box_art_url IS NOT NULL), 0),
                    COALESCE(SUM(cooperative = 1), 0),
                    COALESCE(SUM(ra_id != ''), 0),
                    COALESCE(SUM(
                        identity_state = ?2 OR hash_matched_name IS NOT NULL OR ra_id != ''
                    ), 0),
                    MIN(CASE WHEN release_date GLOB '[0-9][0-9][0-9][0-9]*'
                             THEN CAST(substr(release_date, 1, 4) AS INTEGER) END),
                    MAX(CASE WHEN release_date GLOB '[0-9][0-9][0-9][0-9]*'
                             THEN CAST(substr(release_date, 1, 4) AS INTEGER) END)
                 FROM game_library
                 WHERE system = ?1",
                params![system, super::IdentityState::CompleteMatched.as_i64()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                        row.get::<_, i64>(2)? as usize,
                        row.get::<_, i64>(3)? as usize,
                        row.get::<_, i64>(4)? as usize,
                        row.get::<_, i64>(5)? as usize,
                        row.get::<_, i64>(6)? as usize,
                        row.get::<_, i64>(7)? as usize,
                        row.get::<_, i64>(8)? as usize,
                        row.get::<_, i64>(9)? as usize,
                        row.get::<_, i64>(10)? as usize,
                        row.get::<_, i64>(11)? as usize,
                        row.get::<_, Option<i64>>(12)?,
                        row.get::<_, Option<i64>>(13)?,
                    ))
                },
            )
            .map_err(|e| Error::Other(format!("query system flag/identity stats: {e}")))?;

        // Description + publisher coverage from one scan of game_detail_metadata.
        let (description_count, publisher_known_count) = conn
            .query_row(
                "SELECT
                    COALESCE(SUM(description IS NOT NULL AND description != ''), 0),
                    COALESCE(SUM(publisher IS NOT NULL AND publisher != ''), 0)
                 FROM game_detail_metadata
                 WHERE system = ?1",
                params![system],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                    ))
                },
            )
            .map_err(|e| Error::Other(format!("query detail-metadata stats: {e}")))?;

        let (manual_count, video_count, resource_count) = resource_counts(conn, system)?;

        Ok(GameLibrarySystemStats {
            system: system.to_string(),
            rom_count: rom_count.max(0) as usize,
            total_size_bytes: total_size_bytes.max(0) as u64,
            clone_count,
            hack_count,
            translation_count,
            homebrew_count: 0,
            unlicensed_count: 0,
            special_count,
            mature_count,
            region_counts_json: counts_json(conn, system, "region")?,
            release_year_min: release_year_min.map(|v| v as u16),
            release_year_max: release_year_max.map(|v| v as u16),
            release_date_known_count,
            genre_counts_json: counts_json(conn, system, "genre")?,
            genre_group_counts_json: counts_json(conn, system, "genre_group")?,
            developer_known_count,
            publisher_known_count,
            player_count_distribution_json: counts_json(conn, system, "players")?,
            rating_known_count,
            description_count,
            boxart_count,
            snap_count: 0,
            title_screen_count: 0,
            manual_count,
            video_count,
            resource_count,
            coop_count,
            verified_count,
            ra_id_count,
            driver_status_json: driver_status_json(conn, system)?,
            refresh_state: StatsRefreshState::Fresh,
            updated_at: Some(unix_now()),
        })
    }

    fn upsert_game_library_system_stats(
        conn: &Connection,
        stats: &GameLibrarySystemStats,
    ) -> Result<()> {
        if stats.system.is_empty() {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO game_library_system_stats (
                system, rom_count, total_size_bytes, clone_count, hack_count,
                translation_count, homebrew_count, unlicensed_count, special_count,
                mature_count, region_counts_json, release_year_min, release_year_max,
                release_date_known_count, genre_counts_json, genre_group_counts_json,
                developer_known_count, publisher_known_count,
                player_count_distribution_json, rating_known_count, description_count,
                boxart_count, snap_count, title_screen_count, manual_count, video_count,
                resource_count, coop_count, verified_count, driver_status_json,
                refresh_state, updated_at, ra_id_count
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
                     ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30,
                     ?31, ?32, ?33)
             ON CONFLICT(system) DO UPDATE SET
                rom_count = excluded.rom_count,
                total_size_bytes = excluded.total_size_bytes,
                clone_count = excluded.clone_count,
                hack_count = excluded.hack_count,
                translation_count = excluded.translation_count,
                homebrew_count = excluded.homebrew_count,
                unlicensed_count = excluded.unlicensed_count,
                special_count = excluded.special_count,
                mature_count = excluded.mature_count,
                region_counts_json = excluded.region_counts_json,
                release_year_min = excluded.release_year_min,
                release_year_max = excluded.release_year_max,
                release_date_known_count = excluded.release_date_known_count,
                genre_counts_json = excluded.genre_counts_json,
                genre_group_counts_json = excluded.genre_group_counts_json,
                developer_known_count = excluded.developer_known_count,
                publisher_known_count = excluded.publisher_known_count,
                player_count_distribution_json = excluded.player_count_distribution_json,
                rating_known_count = excluded.rating_known_count,
                description_count = excluded.description_count,
                boxart_count = excluded.boxart_count,
                snap_count = excluded.snap_count,
                title_screen_count = excluded.title_screen_count,
                manual_count = excluded.manual_count,
                video_count = excluded.video_count,
                resource_count = excluded.resource_count,
                coop_count = excluded.coop_count,
                verified_count = excluded.verified_count,
                driver_status_json = excluded.driver_status_json,
                refresh_state = excluded.refresh_state,
                updated_at = excluded.updated_at,
                ra_id_count = excluded.ra_id_count",
            params![
                stats.system,
                stats.rom_count as i64,
                stats.total_size_bytes as i64,
                stats.clone_count as i64,
                stats.hack_count as i64,
                stats.translation_count as i64,
                stats.homebrew_count as i64,
                stats.unlicensed_count as i64,
                stats.special_count as i64,
                stats.mature_count as i64,
                stats.region_counts_json,
                stats.release_year_min.map(i64::from),
                stats.release_year_max.map(i64::from),
                stats.release_date_known_count as i64,
                stats.genre_counts_json,
                stats.genre_group_counts_json,
                stats.developer_known_count as i64,
                stats.publisher_known_count as i64,
                stats.player_count_distribution_json,
                stats.rating_known_count as i64,
                stats.description_count as i64,
                stats.boxart_count as i64,
                stats.snap_count as i64,
                stats.title_screen_count as i64,
                stats.manual_count as i64,
                stats.video_count as i64,
                stats.resource_count as i64,
                stats.coop_count as i64,
                stats.verified_count as i64,
                stats.driver_status_json,
                stats.refresh_state.as_i64(),
                stats.updated_at,
                stats.ra_id_count as i64,
            ],
        )
        .map_err(|e| Error::Other(format!("upsert game_library_system_stats: {e}")))?;
        Ok(())
    }
}

/// Manual coverage, video coverage, and total resource count for a system in a
/// single scan of `library_game_resource`. Manual/video are distinct-ROM counts
/// (a ROM with several manuals counts once); the total counts every resource row.
fn resource_counts(conn: &Connection, system: &str) -> Result<(usize, usize, usize)> {
    let mut stmt = conn
        .prepare(
            "SELECT resource_type, COUNT(*), COUNT(DISTINCT rom_filename)
             FROM library_game_resource
             WHERE system = ?1
             GROUP BY resource_type",
        )
        .map_err(|e| Error::Other(format!("prepare resource coverage stats: {e}")))?;
    let rows = stmt
        .query_map(params![system], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, i64>(2)? as usize,
            ))
        })
        .map_err(|e| Error::Other(format!("query resource coverage stats: {e}")))?;
    let (mut manual, mut video, mut total) = (0usize, 0usize, 0usize);
    for row in rows {
        let (resource_type, count, distinct_roms) =
            row.map_err(|e| Error::Other(format!("read resource coverage stats: {e}")))?;
        total += count;
        match resource_type.as_str() {
            resource_kind::MANUAL => manual = distinct_roms,
            resource_kind::VIDEO => video = distinct_roms,
            _ => {}
        }
    }
    Ok((manual, video, total))
}

fn counts_json(conn: &Connection, system: &str, column: &str) -> Result<Option<String>> {
    let sql = format!(
        "SELECT CAST({column} AS TEXT), COUNT(*)
         FROM game_library
         WHERE system = ?1 AND {column} IS NOT NULL AND CAST({column} AS TEXT) != ''
         GROUP BY {column}
         ORDER BY COUNT(*) DESC, CAST({column} AS TEXT)"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| Error::Other(format!("prepare distribution stats: {e}")))?;
    let rows = stmt
        .query_map(params![system], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(|e| Error::Other(format!("query distribution stats: {e}")))?;
    let mut map = BTreeMap::new();
    for row in rows {
        let (key, count) =
            row.map_err(|e| Error::Other(format!("read distribution stats: {e}")))?;
        map.insert(key, count);
    }
    if map.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&map)
            .map(Some)
            .map_err(|e| Error::Other(format!("encode distribution stats: {e}")))
    }
}

fn driver_status_json(conn: &Connection, system: &str) -> Result<Option<String>> {
    let mut stmt = conn
        .prepare(
            "SELECT driver_status, COUNT(*)
             FROM game_library
             WHERE system = ?1
               AND driver_status IS NOT NULL
               AND driver_status != ''
             GROUP BY driver_status",
        )
        .map_err(|e| Error::Other(format!("prepare driver stats: {e}")))?;
    let rows = stmt
        .query_map(params![system], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })
        .map_err(|e| Error::Other(format!("query driver stats: {e}")))?;
    let mut counts = DriverStatusCounts::default();
    let mut seen = false;
    for row in rows {
        let (status, count) = row.map_err(|e| Error::Other(format!("read driver stats: {e}")))?;
        seen = true;
        match status.as_str() {
            "working" => counts.working = count,
            "imperfect" => counts.imperfect = count,
            "preliminary" => counts.preliminary = count,
            _ => counts.unknown += count,
        }
    }
    if seen {
        serde_json::to_string(&counts)
            .map(Some)
            .map_err(|e| Error::Other(format!("encode driver stats: {e}")))
    } else {
        Ok(None)
    }
}

fn count_json_total(json: Option<&str>) -> usize {
    json.and_then(|json| serde_json::from_str::<BTreeMap<String, usize>>(json).ok())
        .map(|counts| counts.values().sum())
        .unwrap_or(0)
}

fn parse_count_buckets(json: Option<&str>) -> Vec<CountBucket> {
    json.and_then(|json| serde_json::from_str::<BTreeMap<String, usize>>(json).ok())
        .map(|counts| {
            counts
                .into_iter()
                .map(|(label, count)| CountBucket { label, count })
                .collect()
        })
        .unwrap_or_default()
}

fn min_optional(current: Option<u16>, next: Option<u16>) -> Option<u16> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (None, value) | (value, None) => value,
    }
}

fn max_optional(current: Option<u16>, next: Option<u16>) -> Option<u16> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (None, value) | (value, None) => value,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use replay_control_core::DatePrecision;

    use super::*;
    use crate::library_db::{IdentityState, LibraryGameResource};

    #[test]
    fn save_system_entries_refreshes_base_stats() {
        let (mut conn, _tmp) = super::super::tests::open_temp_db();
        let mut clone = super::super::tests::make_game_entry("snes", "Clone.sfc", false);
        clone.is_clone = true;
        clone.size_bytes = 10;
        clone.region = "usa".into();
        let mut hack = super::super::tests::make_game_entry("snes", "Hack.sfc", false);
        hack.is_hack = true;
        hack.is_translation = true;
        hack.size_bytes = 20;
        hack.region = "japan".into();

        LibraryDb::save_system_entries(&mut conn, "snes", &[clone, hack], None).unwrap();

        let stats = LibraryDb::load_game_library_system_stats(&conn).unwrap();
        assert_eq!(stats.len(), 1);
        let stats = &stats[0];
        assert_eq!(stats.system, "snes");
        assert_eq!(stats.rom_count, 2);
        assert_eq!(stats.total_size_bytes, 30);
        assert_eq!(stats.clone_count, 1);
        assert_eq!(stats.hack_count, 1);
        assert_eq!(stats.translation_count, 1);
        assert!(stats.region_counts_json.as_deref().unwrap().contains("usa"));
    }

    #[test]
    fn coverage_reads_materialized_stats() {
        let (mut conn, _tmp) = super::super::tests::open_temp_db();
        let mut mario = super::super::tests::make_game_entry("snes", "Mario.sfc", false);
        mario.genre = Some("Platform".into());
        mario.genre_group = "Platform".into();
        mario.developer = "Nintendo".into();
        mario.rating = Some(4.5);
        mario.box_art_url = Some("/media/flyers/Mario.png".into());
        mario.release_date = Some("1991-08-23".into());
        mario.release_precision = Some(DatePrecision::Day);
        mario.cooperative = true;
        mario.identity_state = IdentityState::CompleteMatched;
        mario.hash_matched_name = Some("Super Mario World".into());
        mario.ra_id = "228".into();
        let zelda = super::super::tests::make_game_entry("snes", "Zelda.sfc", false);

        LibraryDb::save_system_entries(&mut conn, "snes", &[mario, zelda], None).unwrap();
        LibraryDb::replace_detail_metadata_and_resources_for_system(
            &mut conn,
            "snes",
            &[(
                "Mario.sfc".to_string(),
                Some("A platform game".to_string()),
                Some("Nintendo".to_string()),
            )],
            &[LibraryGameResource {
                rom_filename: "Mario.sfc".to_string(),
                source: "catalog".to_string(),
                resource_type: resource_kind::MANUAL.to_string(),
                resource_id: "mario-manual".to_string(),
                url: "https://example.invalid/manual.pdf".to_string(),
                title: Some("Manual".to_string()),
                languages: Some("en".to_string()),
                platform: None,
                mime_type: Some("application/pdf".to_string()),
            }],
        )
        .unwrap();
        LibraryDb::refresh_game_library_system_stats(&conn, "snes").unwrap();

        let (summary, coverage) = LibraryDb::library_overview_from_system_stats(&conn).unwrap();
        assert_eq!(coverage.len(), 1);
        let coverage = &coverage[0];
        assert_eq!(coverage.total_games, 2);
        assert_eq!(coverage.with_genre, 1);
        assert_eq!(coverage.with_developer, 1);
        assert_eq!(coverage.with_rating, 1);
        assert_eq!(coverage.with_thumbnail, 1);
        assert_eq!(coverage.with_description, 1);
        assert_eq!(coverage.coop_count, 1);
        assert_eq!(coverage.verified_count, 1);
        assert_eq!(coverage.with_ra_id, 1);
        assert_eq!(coverage.min_year, Some(1991));
        assert_eq!(coverage.max_year, Some(1991));

        let stats = LibraryDb::load_game_library_system_stats(&conn).unwrap();
        assert_eq!(stats[0].manual_count, 1);
        assert_eq!(stats[0].resource_count, 1);

        assert_eq!(summary.total_games, 2);
        assert_eq!(summary.system_count, 1);
        assert_eq!(summary.with_genre, 1);
        assert_eq!(summary.with_developer, 1);
        assert_eq!(summary.with_rating, 1);
        assert_eq!(summary.with_box_art, 1);
        assert_eq!(summary.coop_games, 1);
        assert_eq!(summary.min_year, Some(1991));
        assert_eq!(summary.max_year, Some(1991));
    }

    #[test]
    fn verified_coverage_counts_ra_only_identity_matches() {
        let (mut conn, _tmp) = super::super::tests::open_temp_db();
        let mut disc = super::super::tests::make_game_entry("sony_psx", "Game.m3u", false);
        disc.is_m3u = true;
        disc.identity_state = IdentityState::CompleteMatched;
        disc.hash_matched_name = None;
        disc.ra_id = "9876".into();
        disc.rc_hash = Some("disc-ra-hash".into());

        LibraryDb::save_system_entries(&mut conn, "sony_psx", &[disc], None).unwrap();
        LibraryDb::refresh_game_library_system_stats(&conn, "sony_psx").unwrap();

        let (_, coverage) = LibraryDb::library_overview_from_system_stats(&conn).unwrap();
        assert_eq!(coverage.len(), 1);
        assert_eq!(coverage[0].verified_count, 1);
        assert_eq!(coverage[0].with_ra_id, 1);
    }

    #[test]
    fn thumbnail_media_stats_are_materialized_on_system_stats() {
        let (mut conn, _tmp) = super::super::tests::open_temp_db();
        let mario = super::super::tests::make_game_entry("snes", "Mario.sfc", false);
        LibraryDb::save_system_entries(&mut conn, "snes", &[mario], None).unwrap();

        LibraryDb::replace_thumbnail_media_stats(
            &mut conn,
            &[
                ThumbnailMediaStats {
                    system: "snes".to_string(),
                    total_size_bytes: 300,
                    file_count: 3,
                    boxart_file_count: 1,
                    snap_file_count: 2,
                    title_file_count: 0,
                },
                ThumbnailMediaStats {
                    system: "genesis".to_string(),
                    total_size_bytes: 200,
                    file_count: 2,
                    boxart_file_count: 1,
                    snap_file_count: 0,
                    title_file_count: 1,
                },
            ],
        )
        .unwrap();

        let media = LibraryDb::thumbnail_media_totals_from_system_stats(&conn).unwrap();
        assert_eq!(media.total_files, 5);
        assert_eq!(media.boxart_files, 2);
        assert_eq!(media.snap_files, 2);
        assert_eq!(media.title_files, 1);
        assert_eq!(media.total_size_bytes, 500);

        LibraryDb::refresh_game_library_system_stats(&conn, "snes").unwrap();
        let media = LibraryDb::thumbnail_media_totals_from_system_stats(&conn).unwrap();
        assert_eq!(media.total_files, 5);
        assert_eq!(media.boxart_files, 2);
        assert_eq!(media.snap_files, 2);
        assert_eq!(media.title_files, 1);
        assert_eq!(media.total_size_bytes, 500);

        LibraryDb::clear_thumbnail_media_stats(&conn).unwrap();
        let media = LibraryDb::thumbnail_media_totals_from_system_stats(&conn).unwrap();
        assert_eq!(media.total_files, 0);
        assert_eq!(media.boxart_files, 0);
        assert_eq!(media.snap_files, 0);
        assert_eq!(media.title_files, 0);
        assert_eq!(media.total_size_bytes, 0);
    }

    #[test]
    fn open_backfills_missing_system_stats_for_existing_library() {
        let (mut conn, dir) = super::super::tests::open_temp_db();
        let mut mario = super::super::tests::make_game_entry("snes", "Mario.sfc", false);
        mario.genre = Some("Platform".into());
        mario.genre_group = "Platform".into();
        mario.box_art_url = Some("/media/flyers/Mario.png".into());
        LibraryDb::save_system_entries(&mut conn, "snes", &[mario], None).unwrap();
        conn.execute("DELETE FROM game_library_system_stats", [])
            .unwrap();
        drop(conn);

        let conn = LibraryDb::open(dir.path()).unwrap();
        let (_summary, coverage) = LibraryDb::library_overview_from_system_stats(&conn).unwrap();

        assert_eq!(coverage.len(), 1);
        assert_eq!(coverage[0].system, "snes");
        assert_eq!(coverage[0].total_games, 1);
        assert_eq!(coverage[0].with_thumbnail, 1);
        assert_eq!(coverage[0].with_genre, 1);
    }
}
