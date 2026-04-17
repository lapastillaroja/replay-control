// Embedded Wikidata series database.
//
// Provides game series/franchise data extracted from Wikidata P179 (part of the series)
// with P1545 ordinals and P155/P156 sequel/prequel chains.
//
// The data is embedded at build time from `data/wikidata/series.json`.
// At scan time, entries are matched to library games by normalized title + system
// and used to populate the `game_series` table in the metadata database.

// Include the build-generated series database code.
include!(concat!(env!("OUT_DIR"), "/series_db.rs"));

/// Look up all Wikidata series entries for a given system and normalized title.
///
/// The `normalized_title` should be produced by the same normalization used at build time:
/// lowercase, strip non-alphanumeric except spaces, collapse whitespace.
pub fn lookup_series(system: &str, normalized_title: &str) -> Vec<&'static WikidataSeriesEntry> {
    wikidata_series()
        .iter()
        .filter(|e| e.system == system && e.normalized_title == normalized_title)
        .collect()
}

/// Look up all Wikidata series entries for a given system.
///
/// Returns all entries for the system, useful for batch matching during scan.
pub fn system_series_entries(system: &str) -> Vec<&'static WikidataSeriesEntry> {
    wikidata_series()
        .iter()
        .filter(|e| e.system == system)
        .collect()
}

/// Get all unique series names from the embedded data.
pub fn all_series_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = wikidata_series()
        .iter()
        .filter(|e| !e.series_name.is_empty())
        .map(|e| e.series_name)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Return all entries from the embedded series database.
///
/// Useful when matching should not be restricted to a single system (e.g.,
/// a game's Wikidata entry may list a different platform than the ROM's system).
pub fn all_entries() -> &'static [WikidataSeriesEntry] {
    wikidata_series()
}

/// Total number of entries in the embedded series database.
pub fn entry_count() -> usize {
    wikidata_series().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether this test binary was built against the committed `fixtures/` stubs
    /// rather than the real `data/` sources. The value of `REPLAY_BUILD_STUB` is
    /// captured at compile time via `option_env!`.
    fn using_stub_data() -> bool {
        matches!(option_env!("REPLAY_BUILD_STUB"), Some("1") | Some("true"))
    }

    #[test]
    fn series_db_not_empty() {
        assert!(
            entry_count() > 0,
            "Series DB should have entries (check data/wikidata/series.json)"
        );
    }

    #[test]
    fn series_db_has_known_series() {
        let names = all_series_names();
        // These are well-known series that should be in Wikidata with English labels
        let expected = ["Mega Man", "Streets of Rage", "Sonic the Hedgehog"];
        for name in &expected {
            assert!(
                names.iter().any(|n| n == name),
                "Series DB should contain '{name}'"
            );
        }
    }

    #[test]
    fn lookup_streets_of_rage() {
        let entries = lookup_series("sega_smd", "streets of rage 2");
        assert!(
            !entries.is_empty(),
            "Should find Streets of Rage 2 on sega_smd"
        );
        let entry = &entries[0];
        assert_eq!(entry.series_name, "Streets of Rage");
        assert_eq!(entry.series_order, Some(2));
    }

    #[test]
    fn system_entries_nes() {
        let entries = system_series_entries("nintendo_nes");
        let min_expected = if using_stub_data() { 2 } else { 50 };
        assert!(
            entries.len() > min_expected,
            "NES should have {}+ series entries, got {}",
            min_expected,
            entries.len()
        );
    }

    #[test]
    fn normalized_title_matching() {
        // Verify that our normalize matches Wikidata titles
        let entries = lookup_series("nintendo_nes", "mega man 2");
        assert!(
            !entries.is_empty(),
            "Should find Mega Man 2 by normalized title"
        );
    }

    #[test]
    fn system_entries_arcade_fbneo() {
        let entries = system_series_entries("arcade_fbneo");
        let min_expected = if using_stub_data() { 1 } else { 400 };
        assert!(
            entries.len() > min_expected,
            "arcade_fbneo should have {}+ series entries, got {}",
            min_expected,
            entries.len()
        );
    }

    #[test]
    fn donpachi_entries_exist() {
        let entries = lookup_series("arcade_fbneo", "donpachi");
        assert!(!entries.is_empty(), "Should find DonPachi on arcade_fbneo");
        assert_eq!(entries[0].series_name, "DonPachi");
    }
}
