//! Relationship queries: regional variants, translations, hacks, specials,
//! alias variants, search aliases.

use rusqlite::params;

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb};

impl MetadataDb {
    /// Find regional variants of a game: other ROMs sharing the same base_title
    /// that are not translations, hacks, specials, or arcade clones.
    /// Returns `(rom_filename, region, display_name)` tuples.
    pub fn regional_variants(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = self
            .conn
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
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
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
    pub fn hacks(&self, system: &str, rom_filename: &str) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
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
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
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
        &self,
        system: &str,
        base_title: &str,
        current_filename: &str,
        region_pref: &str,
    ) -> Result<Vec<GameEntry>> {
        let mut stmt = self
            .conn
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
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
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
    pub fn search_aliases(&self, query: &str) -> Result<Vec<(String, String)>> {
        let like_pattern = format!("%{query}%");
        let mut stmt = self
            .conn
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
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;

    #[test]
    fn specials_returns_special_roms_sharing_base_title() {
        let (mut db, _dir) = open_temp_db();

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

        db.save_system_entries("snes", &[original, fastrom, hz60], None)
            .unwrap();

        let specials = db.specials("snes", "Game (USA).sfc").unwrap();
        assert_eq!(specials.len(), 2);
        let filenames: Vec<&str> = specials.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA) (FastRom).sfc"));
        assert!(filenames.contains(&"Game (Europe) (60hz).sfc"));
    }

    #[test]
    fn regional_variants_excludes_clones_and_specials() {
        let (mut db, _dir) = open_temp_db();

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

        db.save_system_entries("snes", &[original, europe, clone, special], None)
            .unwrap();

        let variants = db.regional_variants("snes", "Game (USA).sfc").unwrap();
        assert_eq!(variants.len(), 2);
        let filenames: Vec<&str> = variants.iter().map(|(f, _, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA).sfc"));
        assert!(filenames.contains(&"Game (Europe).sfc"));
        assert!(!filenames.contains(&"Game (Japan).sfc"));
        assert!(!filenames.contains(&"Game (USA) (FastRom).sfc"));
    }
}
