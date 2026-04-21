//! Relationship queries: regional variants, translations, hacks, specials,
//! alias variants, search aliases.

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb};

impl MetadataDb {
    /// Find regional variants of a game: other ROMs sharing the same base_title
    /// that are not translations, hacks, specials, or arcade clones.
    /// Returns `(rom_filename, region, display_name)` tuples.
    pub fn regional_variants(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, region, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                   AND is_clone = 0
                   AND base_title = (
                       SELECT base_title FROM game_library
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY
                   CASE region
                       WHEN 'USA' THEN 1
                       WHEN 'Europe' THEN 2
                       WHEN 'Japan' THEN 3
                       ELSE 4
                   END,
                   region",
            )
            .map_err(|e| Error::Other(format!("Prepare regional_variants: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query regional_variants: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find translations of a game: other ROMs sharing the same base_title that are translations.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    pub fn translations(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare translations: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query translations: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find hacks of a game: other ROMs sharing the same base_title that are hacks.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    pub fn hacks(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_hack = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare hacks: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query hacks: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find special versions of a game: other ROMs sharing the same base_title that are special.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    pub fn specials(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_special = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare specials: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query specials: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find cross-name variants of a game using game_alias.
    ///
    /// Given a ROM's system and base_title, find other games in the same system
    /// that are linked via aliases (same game, different name).
    pub fn alias_variants(
        conn: &Connection,
        system: &str,
        base_title: &str,
        current_filename: &str,
        region_pref: &str,
    ) -> Result<Vec<GameEntry>> {
        let mut stmt = conn
            .prepare(
                "WITH related_titles AS (
                    -- Games whose base_title is an alias of the current game
                    SELECT DISTINCT gl.base_title AS bt
                    FROM game_alias ga
                    JOIN game_library gl ON gl.system = ga.system AND gl.base_title = ga.alias_name
                    WHERE ga.system = ?1 AND ga.base_title = ?2
                    UNION
                    -- The canonical base_title that the current game is an alias of
                    SELECT DISTINCT ga.base_title AS bt
                    FROM game_alias ga
                    WHERE ga.system = ?1 AND ga.alias_name = ?2
                    UNION
                    -- Games that are aliases of the same canonical title
                    SELECT DISTINCT ga2.alias_name AS bt
                    FROM game_alias ga
                    JOIN game_alias ga2 ON ga2.system = ga.system AND ga2.base_title = ga.base_title
                    WHERE ga.system = ?1 AND ga.alias_name = ?2
                ),
                deduped AS (
                    SELECT gl.*, ROW_NUMBER() OVER (
                        PARTITION BY gl.system, gl.base_title
                        ORDER BY CASE
                            WHEN gl.region = ?4 THEN 0
                            WHEN gl.region = 'world' THEN 1
                            ELSE 2
                        END
                    ) AS rn
                    FROM game_library gl
                    WHERE gl.system = ?1
                      AND gl.base_title IN (SELECT bt FROM related_titles)
                      AND gl.base_title != ?2
                      AND gl.rom_filename != ?3
                      AND gl.is_clone = 0
                      AND gl.is_translation = 0
                      AND gl.is_hack = 0
                      AND gl.is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                        release_date, release_precision, release_region_used, cooperative
                FROM deduped WHERE rn = 1
                ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare alias_variants: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, base_title, current_filename, region_pref],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query alias_variants: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Search game aliases: find base_titles whose aliases match the query.
    ///
    /// Returns `(system, base_title)` pairs where an alias matches the query.
    /// Used by search to expand results (e.g., searching "Bare Knuckle" finds "Streets of Rage").
    pub fn search_aliases(conn: &Connection, query: &str) -> Result<Vec<(String, String)>> {
        let like_pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT system, base_title FROM game_alias
                 WHERE alias_name LIKE ?1 COLLATE NOCASE",
            )
            .map_err(|e| Error::Other(format!("Prepare search_aliases: {e}")))?;

        let rows = stmt
            .query_map(params![like_pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query search_aliases: {e}")))?;

        Ok(rows.flatten().collect())
    }
    /// Find alternate versions (clones that are not hacks/translations/specials)
    /// of a game: other ROMs sharing the same base_title with `is_clone = 1`.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    ///
    /// Excludes hacks (`is_hack = 0`) since those already appear in the Hacks section.
    /// Intended for non-arcade systems — arcade systems use the "Arcade Versions" section instead.
    pub fn alternate_versions(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_clone = 1
                   AND is_hack = 0
                   AND is_translation = 0
                   AND is_special = 0
                   AND base_title = (
                       SELECT base_title FROM game_library
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare alternate_versions: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query alternate_versions: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find cross-system availability: the same game (by base_title) on other systems.
    ///
    /// Returns one entry per system (best region match), excluding clones/hacks/translations/specials.
    /// Results are ordered with arcade systems first, then by display_name.
    pub fn cross_system_availability(
        conn: &Connection,
        system: &str,
        base_title: &str,
        region_pref: &str,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        if base_title.is_empty() || base_title.len() <= 1 {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "WITH deduped AS (
                    SELECT gl.*, ROW_NUMBER() OVER (
                        PARTITION BY gl.system
                        ORDER BY CASE
                            WHEN gl.region = ?3 THEN 0
                            WHEN gl.region = 'world' THEN 1
                            ELSE 2
                        END
                    ) AS rn
                    FROM game_library gl
                    WHERE gl.base_title = ?2
                      AND gl.system != ?1
                      AND gl.is_clone = 0
                      AND gl.is_translation = 0
                      AND gl.is_hack = 0
                      AND gl.is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                        release_date, release_precision, release_region_used, cooperative
                FROM deduped WHERE rn = 1
                ORDER BY CASE
                    WHEN system LIKE 'arcade_%' THEN 0
                    ELSE 1
                END, display_name
                LIMIT ?4",
            )
            .map_err(|e| Error::Other(format!("Prepare cross_system_availability: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, base_title, region_pref, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query cross_system_availability: {e}")))?;

        Ok(rows.flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::MetadataDb;
    use super::super::tests::*;

    #[test]
    fn specials_returns_special_roms_sharing_base_title() {
        let (mut conn, _dir) = open_temp_db();

        let mut original = make_game_entry("snes", "Game (USA).sfc", false);
        original.base_title = "Game".into();
        original.region = "usa".into();

        let mut fastrom = make_game_entry("snes", "Game (USA) (FastRom).sfc", false);
        fastrom.base_title = "Game".into();
        fastrom.region = "usa".into();
        fastrom.is_special = true;

        let mut hz60 = make_game_entry("snes", "Game (Europe) (60hz).sfc", false);
        hz60.base_title = "Game".into();
        hz60.region = "europe".into();
        hz60.is_special = true;

        MetadataDb::save_system_entries(&mut conn, "snes", &[original, fastrom, hz60], None)
            .unwrap();

        let specials = MetadataDb::specials(&conn, "snes", "Game (USA).sfc").unwrap();
        assert_eq!(specials.len(), 2);
        let filenames: Vec<&str> = specials.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA) (FastRom).sfc"));
        assert!(filenames.contains(&"Game (Europe) (60hz).sfc"));
    }

    #[test]
    fn regional_variants_excludes_clones_and_specials() {
        let (mut conn, _dir) = open_temp_db();

        let mut original = make_game_entry("snes", "Game (USA).sfc", false);
        original.base_title = "game".into();
        original.region = "usa".into();

        let mut europe = make_game_entry("snes", "Game (Europe).sfc", false);
        europe.base_title = "game".into();
        europe.region = "europe".into();

        let mut clone = make_game_entry("snes", "Game (Japan).sfc", false);
        clone.base_title = "game".into();
        clone.region = "japan".into();
        clone.is_clone = true;

        let mut special = make_game_entry("snes", "Game (USA) (FastRom).sfc", false);
        special.base_title = "game".into();
        special.region = "usa".into();
        special.is_special = true;

        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[original, europe, clone, special],
            None,
        )
        .unwrap();

        let variants = MetadataDb::regional_variants(&conn, "snes", "Game (USA).sfc").unwrap();
        assert_eq!(variants.len(), 2);
        let filenames: Vec<&str> = variants.iter().map(|(f, _, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA).sfc"));
        assert!(filenames.contains(&"Game (Europe).sfc"));
        assert!(!filenames.contains(&"Game (Japan).sfc"));
        assert!(!filenames.contains(&"Game (USA) (FastRom).sfc"));
    }

    #[test]
    fn alternate_versions_returns_clones_excluding_hacks() {
        let (mut conn, _dir) = open_temp_db();

        let mut parent = make_game_entry("cpc", "Game (1990)(Pub).dsk", false);
        parent.base_title = "game".into();
        parent.region = "".into();

        let mut alt1 = make_game_entry("cpc", "Game (1990)(Pub)[a].dsk", false);
        alt1.base_title = "game".into();
        alt1.is_clone = true;
        alt1.display_name = Some("Game (Alternate)".into());

        let mut alt2 = make_game_entry("cpc", "Game (1990)(Pub)[a2].dsk", false);
        alt2.base_title = "game".into();
        alt2.is_clone = true;
        alt2.display_name = Some("Game (Alternate 2)".into());

        // Clone that is also a hack — should be excluded
        let mut hack_clone = make_game_entry("cpc", "Game (1990)(Pub)[a][h].dsk", false);
        hack_clone.base_title = "game".into();
        hack_clone.is_clone = true;
        hack_clone.is_hack = true;

        MetadataDb::save_system_entries(&mut conn, "cpc", &[parent, alt1, alt2, hack_clone], None)
            .unwrap();

        let alts = MetadataDb::alternate_versions(&conn, "cpc", "Game (1990)(Pub).dsk").unwrap();
        assert_eq!(alts.len(), 2);
        let filenames: Vec<&str> = alts.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (1990)(Pub)[a].dsk"));
        assert!(filenames.contains(&"Game (1990)(Pub)[a2].dsk"));
        assert!(!filenames.contains(&"Game (1990)(Pub)[a][h].dsk"));
    }

    #[test]
    fn alternate_versions_works_when_viewing_clone() {
        let (mut conn, _dir) = open_temp_db();

        let mut parent = make_game_entry("cpc", "Game (1990)(Pub).dsk", false);
        parent.base_title = "game".into();

        let mut alt = make_game_entry("cpc", "Game (1990)(Pub)[a].dsk", false);
        alt.base_title = "game".into();
        alt.is_clone = true;
        alt.display_name = Some("Game (Alternate)".into());

        MetadataDb::save_system_entries(&mut conn, "cpc", &[parent, alt], None).unwrap();

        // Query from the clone's perspective — should still find the other clone
        let alts = MetadataDb::alternate_versions(&conn, "cpc", "Game (1990)(Pub)[a].dsk").unwrap();
        assert_eq!(alts.len(), 1);
        assert_eq!(alts[0].0, "Game (1990)(Pub)[a].dsk");
    }

    #[test]
    fn cross_system_returns_other_systems() {
        let (mut conn, _dir) = open_temp_db();

        let mut cpc = make_game_entry("cpc", "Pac-Man (1990)(Pub).dsk", false);
        cpc.base_title = "pac-man".into();
        cpc.region = "europe".into();

        let mut smd = make_game_entry("smd", "Pac-Man (USA).md", false);
        smd.base_title = "pac-man".into();
        smd.region = "usa".into();
        smd.display_name = Some("Pac-Man (USA)".into());

        let mut snes = make_game_entry("snes", "Pac-Man (Europe).sfc", false);
        snes.base_title = "pac-man".into();
        snes.region = "europe".into();
        snes.display_name = Some("Pac-Man (Europe)".into());

        // Clone on another system — should be excluded
        let mut clone = make_game_entry("snes", "Pac-Man (Japan).sfc", false);
        clone.base_title = "pac-man".into();
        clone.region = "japan".into();
        clone.is_clone = true;

        MetadataDb::save_system_entries(&mut conn, "cpc", &[cpc], None).unwrap();
        MetadataDb::save_system_entries(&mut conn, "smd", &[smd], None).unwrap();
        MetadataDb::save_system_entries(&mut conn, "snes", &[snes, clone], None).unwrap();

        let results =
            MetadataDb::cross_system_availability(&conn, "cpc", "pac-man", "usa", 10).unwrap();
        assert_eq!(results.len(), 2);
        let systems: Vec<&str> = results.iter().map(|e| e.system.as_str()).collect();
        assert!(systems.contains(&"smd"));
        assert!(systems.contains(&"snes"));
        // Should NOT contain the CPC entry itself
        assert!(!systems.contains(&"cpc"));
    }

    #[test]
    fn cross_system_deduplicates_per_system() {
        let (mut conn, _dir) = open_temp_db();

        let mut cpc = make_game_entry("cpc", "Game (1990)(Pub).dsk", false);
        cpc.base_title = "game".into();

        let mut smd_usa = make_game_entry("smd", "Game (USA).md", false);
        smd_usa.base_title = "game".into();
        smd_usa.region = "usa".into();
        smd_usa.display_name = Some("Game (USA)".into());

        let mut smd_eu = make_game_entry("smd", "Game (Europe).md", false);
        smd_eu.base_title = "game".into();
        smd_eu.region = "europe".into();
        smd_eu.display_name = Some("Game (Europe)".into());

        MetadataDb::save_system_entries(&mut conn, "cpc", &[cpc], None).unwrap();
        MetadataDb::save_system_entries(&mut conn, "smd", &[smd_usa, smd_eu], None).unwrap();

        // Prefer USA region
        let results =
            MetadataDb::cross_system_availability(&conn, "cpc", "game", "usa", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rom_filename, "Game (USA).md");
    }

    #[test]
    fn cross_system_empty_for_short_title() {
        let (mut conn, _dir) = open_temp_db();

        let mut g = make_game_entry("cpc", "X.dsk", false);
        g.base_title = "x".into();

        MetadataDb::save_system_entries(&mut conn, "cpc", &[g], None).unwrap();

        let results = MetadataDb::cross_system_availability(&conn, "cpc", "x", "usa", 10).unwrap();
        assert!(results.is_empty());
    }
}
