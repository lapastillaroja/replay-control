/// A Wikidata-sourced game series entry.
#[derive(Debug, Clone)]
pub struct WikidataSeriesEntry {
    pub game_title: String,
    pub series_name: String,
    pub system: String,
    pub series_order: Option<i32>,
    pub follows: String,
    pub followed_by: String,
    pub normalized_title: String,
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<WikidataSeriesEntry> {
    Ok(WikidataSeriesEntry {
        game_title: row.get(0)?,
        series_name: row.get(1)?,
        system: row.get(2)?,
        series_order: row.get(3)?,
        follows: row.get(4)?,
        followed_by: row.get(5)?,
        normalized_title: row.get(6)?,
    })
}

const ENTRY_COLS: &str = "game_title, series_name, system, series_order, follows, followed_by, \
     normalized_title";

/// Look up all Wikidata series entries for a given system and normalized title.
pub async fn lookup_series(system: &str, normalized_title: &str) -> Vec<WikidataSeriesEntry> {
    {
        let system = system.to_string();
        let normalized_title = normalized_title.to_string();
        return crate::catalog_pool::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM series_entries \
                 WHERE system = ?1 AND normalized_title = ?2"
            ))?;
            let rows = stmt.query_map(rusqlite::params![system, normalized_title], row_to_entry)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
}

/// Look up all Wikidata series entries for a given system.
pub async fn system_series_entries(system: &str) -> Vec<WikidataSeriesEntry> {
    {
        let system = system.to_string();
        return crate::catalog_pool::with_catalog(move |conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {ENTRY_COLS} FROM series_entries WHERE system = ?1"
            ))?;
            let rows = stmt.query_map(rusqlite::params![system], row_to_entry)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
}

/// Get all unique series names.
pub async fn all_series_names() -> Vec<String> {
    {
        return crate::catalog_pool::with_catalog(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT DISTINCT series_name FROM series_entries \
                 WHERE series_name != '' ORDER BY series_name",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
}

/// Return all entries from the series database.
pub async fn all_entries() -> Vec<WikidataSeriesEntry> {
    {
        return crate::catalog_pool::with_catalog(|conn| {
            let mut stmt =
                conn.prepare_cached(&format!("SELECT {ENTRY_COLS} FROM series_entries"))?;
            let rows = stmt.query_map([], row_to_entry)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .unwrap_or_default();
    }
}

/// Total number of entries in the series database.
pub async fn entry_count() -> usize {
    {
        return crate::catalog_pool::with_catalog(|conn| {
            conn.query_row("SELECT COUNT(*) FROM series_entries", [], |row| {
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
    async fn series_db_not_empty() {
        init_test_catalog().await;
        assert!(
            entry_count().await > 0,
            "Series DB should have entries (check data/wikidata/series.json)"
        );
    }

    #[tokio::test]
    async fn series_db_has_known_series() {
        init_test_catalog().await;
        let names = all_series_names().await;
        let expected = ["Mega Man", "Streets of Rage", "Sonic the Hedgehog"];
        for name in &expected {
            assert!(
                names.iter().any(|n| n == name),
                "Series DB should contain '{name}'"
            );
        }
    }

    #[tokio::test]
    async fn lookup_streets_of_rage() {
        init_test_catalog().await;
        let entries = lookup_series("sega_smd", "streets of rage 2").await;
        assert!(
            !entries.is_empty(),
            "Should find Streets of Rage 2 on sega_smd"
        );
        let entry = &entries[0];
        assert_eq!(entry.series_name, "Streets of Rage");
        assert_eq!(entry.series_order, Some(2));
    }

    #[tokio::test]
    async fn system_entries_nes() {
        init_test_catalog().await;
        let entries = system_series_entries("nintendo_nes").await;
        let min_expected = if using_stub_data() { 2 } else { 50 };
        assert!(
            entries.len() > min_expected,
            "NES should have {}+ series entries, got {}",
            min_expected,
            entries.len()
        );
    }

    #[tokio::test]
    async fn normalized_title_matching() {
        init_test_catalog().await;
        let entries = lookup_series("nintendo_nes", "mega man 2").await;
        assert!(
            !entries.is_empty(),
            "Should find Mega Man 2 by normalized title"
        );
    }

    #[tokio::test]
    async fn system_entries_arcade_fbneo() {
        init_test_catalog().await;
        let entries = system_series_entries("arcade_fbneo").await;
        let min_expected = if using_stub_data() { 1 } else { 400 };
        assert!(
            entries.len() > min_expected,
            "arcade_fbneo should have {}+ series entries, got {}",
            min_expected,
            entries.len()
        );
    }

    #[tokio::test]
    async fn donpachi_entries_exist() {
        init_test_catalog().await;
        let entries = lookup_series("arcade_fbneo", "donpachi").await;
        assert!(!entries.is_empty(), "Should find DonPachi on arcade_fbneo");
        assert_eq!(entries[0].series_name, "DonPachi");
    }
}
