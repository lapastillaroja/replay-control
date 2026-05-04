//! App-specific settings stored in `settings.cfg`.
//!
//! Uses the same `key = "value"` format as `replay.cfg` but is kept separate
//! to avoid modifying the RePlayOS system configuration.
//!
//! Settings are accessed through [`SettingsStore`], which owns the resolved
//! directory path. On Pi production this is `/etc/replay-control/`; in local
//! dev it is `<storage>/.replay-control/`; in tests it is a tempdir.

use std::path::{Path, PathBuf};

use crate::config::AppSettings;
use crate::storage::{RC_DIR, SETTINGS_FILE};
use replay_control_core::error::Result;
use replay_control_core::locale::Locale;
use replay_control_core::rom_tags::RegionPreference;

/// Resolved settings directory. Contains `settings.cfg`.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    /// Create a store pointing at the given directory.
    /// Does NOT create the directory -- that happens on first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Path to the settings directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }

    /// Load settings from disk, returning empty settings if absent/corrupt.
    pub fn load(&self) -> AppSettings {
        AppSettings::from_file(&self.settings_path()).unwrap_or_else(|_| AppSettings::empty())
    }

    /// Save settings to disk, creating the directory if needed.
    ///
    /// Note: no synchronization — concurrent writes from different threads may
    /// race (last writer wins). Acceptable for this single-user app; the
    /// in-memory `UserPreferences` cache in `AppState` avoids repeated reads.
    pub fn save(&self, settings: &AppSettings) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| replay_control_core::error::Error::io(&self.dir, e))?;
        settings.save(&self.settings_path())
    }

    /// One-time migration from old storage-level settings to this store.
    /// Moves settings.cfg from `<storage_root>/.replay-control/` to this
    /// store's directory. Uses atomic rename when possible, falls back to
    /// copy + delete for cross-filesystem moves.
    pub fn migrate_from_storage(&self, storage_root: &Path) -> Result<()> {
        let old_path = storage_root.join(RC_DIR).join(SETTINGS_FILE);

        if self.settings_path().exists() {
            tracing::debug!(
                "Settings already at {}, skipping migration",
                self.settings_path().display()
            );
            return Ok(());
        }

        if !old_path.exists() {
            tracing::debug!(
                "No old settings at {}, nothing to migrate",
                old_path.display()
            );
            return Ok(());
        }

        std::fs::create_dir_all(&self.dir)
            .map_err(|e| replay_control_core::error::Error::io(&self.dir, e))?;

        // Try atomic rename first; falls back to copy+delete across filesystems.
        let dest = self.settings_path();
        if std::fs::rename(&old_path, &dest).is_err() {
            std::fs::copy(&old_path, &dest)
                .map_err(|e| replay_control_core::error::Error::io(&dest, e))?;
            if let Err(e) = std::fs::remove_file(&old_path) {
                tracing::warn!(
                    "Failed to delete old settings at {}: {e}",
                    old_path.display()
                );
            }
        }

        tracing::info!(
            "Settings migrated: {} -> {}",
            old_path.display(),
            dest.display()
        );

        Ok(())
    }
}

/// Cached snapshot of frequently-read user preferences.
///
/// Loaded once at startup and updated in-memory whenever a preference changes,
/// avoiding repeated file I/O on every SSR render or server function call.
#[derive(Debug, Clone)]
pub struct UserPreferences {
    pub skin: Option<u32>,
    pub locale: Option<Locale>,
    pub region: RegionPreference,
    pub region_secondary: Option<RegionPreference>,
    pub font_size: String,
    pub setup_dismissed: bool,
    pub ra_api_key: Option<String>,
    pub ra_username: Option<String>,
    pub ra_web_token: Option<String>,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            skin: None,
            locale: None,
            region: RegionPreference::default(),
            region_secondary: None,
            font_size: "normal".to_string(),
            setup_dismissed: false,
            ra_api_key: None,
            ra_username: None,
            ra_web_token: None,
        }
    }
}

impl UserPreferences {
    /// Load all preferences from `settings.cfg` in a single file read.
    pub fn load(store: &SettingsStore) -> Self {
        let settings = store.load();
        Self {
            skin: settings.skin(),
            locale: settings.locale(),
            region: RegionPreference::from_str_value(settings.region_preference()),
            region_secondary: settings
                .region_preference_secondary()
                .map(RegionPreference::from_str_value),
            font_size: settings.font_size().to_string(),
            setup_dismissed: settings.setup_dismissed(),
            ra_api_key: settings.ra_api_key().map(|s| s.to_string()),
            ra_username: settings.ra_username().map(|s| s.to_string()),
            ra_web_token: settings.ra_web_token().map(|s| s.to_string()),
        }
    }
}

/// Read the region preference from settings.
/// Returns the default (`World`) if the file doesn't exist or the key is missing.
pub fn read_region_preference(store: &SettingsStore) -> RegionPreference {
    RegionPreference::from_str_value(store.load().region_preference())
}

/// Write the region preference to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference(store: &SettingsStore, pref: RegionPreference) -> Result<()> {
    let mut settings = store.load();
    settings.set_region_preference(pref.as_str());
    store.save(&settings)
}

/// Read the secondary region preference from settings.
/// Returns `None` if the file doesn't exist, the key is missing, or the value is empty.
pub fn read_region_preference_secondary(store: &SettingsStore) -> Option<RegionPreference> {
    let settings = store.load();
    let value = settings.region_preference_secondary()?;
    Some(RegionPreference::from_str_value(value))
}

/// Write the secondary region preference to settings.
/// Pass `None` to clear the secondary preference (removes the key value).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference_secondary(
    store: &SettingsStore,
    pref: Option<RegionPreference>,
) -> Result<()> {
    let mut settings = store.load();
    let value = pref.map(|p| p.as_str()).unwrap_or("");
    settings.set_region_preference_secondary(value);
    store.save(&settings)
}

/// Read the font size preference from settings.
/// Returns `"normal"` or `"large"`, defaults to `"normal"`.
pub fn read_font_size(store: &SettingsStore) -> String {
    store.load().font_size().to_string()
}

/// Write the font size preference to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_font_size(store: &SettingsStore, size: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_font_size(size);
    store.save(&settings)
}

/// Read the skin preference from settings.
/// Returns `Some(index)` if the user has explicitly chosen a skin (sync off),
/// or `None` if the key is absent (sync on — read from `replay.cfg` instead).
pub fn read_skin(store: &SettingsStore) -> Option<u32> {
    store.load().skin()
}

/// Write the skin preference to settings.
/// Pass `Some(index)` to store a specific skin (sync off).
/// Pass `None` to clear the key (sync on — defer to `replay.cfg`).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_skin(store: &SettingsStore, skin: Option<u32>) -> Result<()> {
    let mut settings = store.load();
    settings.set_skin(skin);
    store.save(&settings)
}

/// Read the primary language preference from settings.
/// Returns `None` if not explicitly set (caller should derive from region).
pub fn read_language_primary(store: &SettingsStore) -> Option<String> {
    store.load().language_primary().map(|s| s.to_string())
}

/// Write the primary language preference to settings.
/// Pass empty string to clear (revert to auto-detection from region).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_language_primary(store: &SettingsStore, lang: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_language_primary(lang);
    store.save(&settings)
}

/// Read the secondary (fallback) language preference from settings.
/// Returns `None` if not set. Defaults to `"en"` in the UI when not explicitly configured.
pub fn read_language_secondary(store: &SettingsStore) -> Option<String> {
    store.load().language_secondary().map(|s| s.to_string())
}

/// Write the secondary (fallback) language preference to settings.
/// Pass empty string to clear.
pub fn write_language_secondary(store: &SettingsStore, lang: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_language_secondary(lang);
    store.save(&settings)
}

/// Write both language preferences in a single load/save cycle.
pub fn write_language_preferences(
    store: &SettingsStore,
    primary: &str,
    secondary: &str,
) -> Result<()> {
    let mut settings = store.load();
    settings.set_language_primary(primary);
    settings.set_language_secondary(secondary);
    store.save(&settings)
}

/// Derive the default primary language from a region preference.
pub fn default_language_for_region(region: RegionPreference) -> &'static str {
    match region {
        RegionPreference::Japan => "ja",
        _ => "en",
    }
}

/// Build the preferred language list, resolving defaults from region if needed.
///
/// Returns a list of language codes in priority order. Used for sorting
/// manual search results, in-folder documents, etc.
pub fn preferred_languages(
    primary: Option<&str>,
    secondary: Option<&str>,
    region: RegionPreference,
) -> Vec<String> {
    let mut langs = Vec::new();

    if let Some(p) = primary.filter(|s| !s.is_empty()) {
        langs.push(p.to_string());
    } else {
        // Derive from region
        langs.push(default_language_for_region(region).to_string());
    }

    if let Some(s) = secondary.filter(|s| !s.is_empty())
        && !langs.contains(&s.to_string())
    {
        langs.push(s.to_string());
    }

    // Always include English as a fallback
    let en = "en".to_string();
    if !langs.contains(&en) {
        langs.push(en);
    }

    langs
}

/// Compute a language match score for sorting.
///
/// Lower score = better match. Used to sort manual search results.
/// - 0: exact match on primary language
/// - 1: exact match on secondary language
/// - 2: English fallback (if not already primary/secondary)
/// - 3: other language
pub fn language_match_score(manual_languages: &str, preferred: &[String]) -> u8 {
    // Parse comma-separated language codes (e.g., "en-gb,de,es,fr,it")
    let manual_langs: Vec<&str> = manual_languages
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if manual_langs.is_empty() {
        return 3;
    }

    for (priority, pref_lang) in preferred.iter().enumerate() {
        for manual_lang in &manual_langs {
            // Exact match or base-code match (e.g., "es" matches "es-mx")
            if lang_matches(manual_lang, pref_lang) {
                return priority as u8;
            }
        }
    }

    3
}

/// Check if a manual language code matches a preferred language code.
///
/// Supports base-code matching: "en-gb" matches "en", "es-mx" matches "es".
fn lang_matches(manual_lang: &str, pref_lang: &str) -> bool {
    let m = manual_lang.to_lowercase();
    let p = pref_lang.to_lowercase();

    if m == p {
        return true;
    }

    // Base code match: "en-gb" starts with "en"
    if let Some(base) = m.split('-').next()
        && base == p
    {
        return true;
    }

    // Reverse: preference "en-gb" should match manual "en"
    if let Some(base) = p.split('-').next()
        && base == m
    {
        return true;
    }

    false
}

/// Read the UI locale from settings.
/// Returns `None` if not set or "auto" (caller should fall back to Accept-Language).
pub fn read_locale(store: &SettingsStore) -> Option<replay_control_core::locale::Locale> {
    store.load().locale()
}

/// Read the stored locale preference including `Auto`.
pub fn read_locale_preference(store: &SettingsStore) -> replay_control_core::locale::Locale {
    store.load().locale_preference()
}

/// Write the UI locale to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_locale(
    store: &SettingsStore,
    locale: replay_control_core::locale::Locale,
) -> Result<()> {
    let mut settings = store.load();
    settings.set_locale(locale.code());
    store.save(&settings)
}

/// Read the setup dismissed flag from settings.
/// Returns `false` if the file doesn't exist or the key is missing.
pub fn read_setup_dismissed(store: &SettingsStore) -> bool {
    store.load().setup_dismissed()
}

/// Write the setup dismissed flag to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_setup_dismissed(store: &SettingsStore, dismissed: bool) -> Result<()> {
    let mut settings = store.load();
    settings.set_setup_dismissed(dismissed);
    store.save(&settings)
}

/// Read the GitHub API key from settings.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_github_api_key(store: &SettingsStore) -> Option<String> {
    store.load().github_api_key().map(|s| s.to_string())
}

/// Write the GitHub API key to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_github_api_key(store: &SettingsStore, key: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_github_api_key(key);
    store.save(&settings)
}

/// Read the RetroAchievements API key from settings.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_ra_api_key(store: &SettingsStore) -> Option<String> {
    store.load().ra_api_key().map(|s| s.to_string())
}

/// Write the RetroAchievements API key to settings.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_ra_api_key(store: &SettingsStore, key: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_ra_api_key(key);
    store.save(&settings)
}

/// Read the RetroAchievements username from settings.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_ra_username(store: &SettingsStore) -> Option<String> {
    store.load().ra_username().map(|s| s.to_string())
}

/// Write the RetroAchievements username to settings.
pub fn write_ra_username(store: &SettingsStore, username: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_ra_username(username);
    store.save(&settings)
}

/// Read the RetroAchievements web token from settings.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_ra_web_token(store: &SettingsStore) -> Option<String> {
    store.load().ra_web_token().map(|s| s.to_string())
}

/// Write the RetroAchievements web token to settings.
pub fn write_ra_web_token(store: &SettingsStore, token: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_ra_web_token(token);
    store.save(&settings)
}

/// Read the update channel from settings.
pub fn read_update_channel(store: &SettingsStore) -> replay_control_core::update::UpdateChannel {
    replay_control_core::update::UpdateChannel::from_str_value(store.load().update_channel())
}

/// Write the update channel to settings.
pub fn write_update_channel(
    store: &SettingsStore,
    channel: replay_control_core::update::UpdateChannel,
) -> Result<()> {
    let mut settings = store.load();
    settings.set_update_channel(channel.as_str());
    store.save(&settings)
}

/// Read the analytics preference from settings.
/// Returns `true` if analytics is enabled (default).
pub fn read_analytics_enabled(store: &SettingsStore) -> bool {
    store.load().analytics_enabled()
}

/// Write the analytics preference to settings.
pub fn write_analytics(store: &SettingsStore, enabled: bool) -> Result<()> {
    let mut settings = store.load();
    settings.set_analytics(enabled);
    store.save(&settings)
}

/// Write the install ID to settings.
pub fn write_install_id(store: &SettingsStore, id: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_install_id(id);
    store.save(&settings)
}

/// Write the last-reported version to settings.
pub fn write_version_last_reported(store: &SettingsStore, version: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_version_last_reported(version);
    store.save(&settings)
}

/// Read the skipped version from settings.
pub fn read_skipped_version(store: &SettingsStore) -> Option<String> {
    store.load().skipped_version().map(|s| s.to_string())
}

/// Write the skipped version to settings.
pub fn write_skipped_version(store: &SettingsStore, version: &str) -> Result<()> {
    let mut settings = store.load();
    settings.set_skipped_version(version);
    store.save(&settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn test_store() -> SettingsStore {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("replay-settings-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        SettingsStore::new(dir)
    }

    #[test]
    fn default_when_no_file() {
        let store = test_store();
        let pref = read_region_preference(&store);
        assert_eq!(pref, RegionPreference::World);
    }

    #[test]
    fn write_and_read_europe() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Europe).unwrap();
        let pref = read_region_preference(&store);
        assert_eq!(pref, RegionPreference::Europe);
    }

    #[test]
    fn write_and_read_japan() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Japan).unwrap();
        let pref = read_region_preference(&store);
        assert_eq!(pref, RegionPreference::Japan);
    }

    #[test]
    fn write_and_read_world() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::World).unwrap();
        let pref = read_region_preference(&store);
        assert_eq!(pref, RegionPreference::World);
    }

    #[test]
    fn overwrite_preserves_other_keys() {
        let store = test_store();
        let dir = store.dir();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(SETTINGS_FILE),
            "other_key = \"value\"\nregion_preference = \"usa\"\n",
        )
        .unwrap();

        write_region_preference(&store, RegionPreference::Japan).unwrap();

        let content = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(content.contains("other_key = \"value\""));
        assert!(content.contains("region_preference = \"japan\""));
    }

    // --- Secondary region preference tests ---

    #[test]
    fn secondary_default_when_no_file() {
        let store = test_store();
        let pref = read_region_preference_secondary(&store);
        assert_eq!(pref, None);
    }

    #[test]
    fn write_and_read_secondary_usa() {
        let store = test_store();
        write_region_preference_secondary(&store, Some(RegionPreference::Usa)).unwrap();
        let pref = read_region_preference_secondary(&store);
        assert_eq!(pref, Some(RegionPreference::Usa));
    }

    #[test]
    fn write_and_read_secondary_japan() {
        let store = test_store();
        write_region_preference_secondary(&store, Some(RegionPreference::Japan)).unwrap();
        let pref = read_region_preference_secondary(&store);
        assert_eq!(pref, Some(RegionPreference::Japan));
    }

    #[test]
    fn write_secondary_none_clears() {
        let store = test_store();
        write_region_preference_secondary(&store, Some(RegionPreference::Europe)).unwrap();
        assert_eq!(
            read_region_preference_secondary(&store),
            Some(RegionPreference::Europe)
        );
        write_region_preference_secondary(&store, None).unwrap();
        assert_eq!(read_region_preference_secondary(&store), None);
    }

    #[test]
    fn secondary_preserves_primary() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Japan).unwrap();
        write_region_preference_secondary(&store, Some(RegionPreference::Usa)).unwrap();

        assert_eq!(read_region_preference(&store), RegionPreference::Japan);
        assert_eq!(
            read_region_preference_secondary(&store),
            Some(RegionPreference::Usa)
        );
    }

    // --- Skin preference tests ---

    #[test]
    fn skin_none_when_no_file() {
        let store = test_store();
        assert_eq!(read_skin(&store), None);
    }

    #[test]
    fn write_and_read_skin() {
        let store = test_store();
        write_skin(&store, Some(5)).unwrap();
        assert_eq!(read_skin(&store), Some(5));
    }

    #[test]
    fn write_skin_none_clears() {
        let store = test_store();
        write_skin(&store, Some(3)).unwrap();
        assert_eq!(read_skin(&store), Some(3));
        write_skin(&store, None).unwrap();
        assert_eq!(read_skin(&store), None);
    }

    #[test]
    fn skin_preserves_other_keys() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Japan).unwrap();
        write_skin(&store, Some(7)).unwrap();

        assert_eq!(read_region_preference(&store), RegionPreference::Japan);
        assert_eq!(read_skin(&store), Some(7));
    }

    // --- Language preference tests ---

    #[test]
    fn language_default_when_no_file() {
        let store = test_store();
        assert_eq!(read_language_primary(&store), None);
        assert_eq!(read_language_secondary(&store), None);
    }

    #[test]
    fn write_and_read_language_primary() {
        let store = test_store();
        write_language_primary(&store, "es").unwrap();
        assert_eq!(read_language_primary(&store), Some("es".to_string()));
    }

    #[test]
    fn write_and_read_language_secondary() {
        let store = test_store();
        write_language_secondary(&store, "fr").unwrap();
        assert_eq!(read_language_secondary(&store), Some("fr".to_string()));
    }

    #[test]
    fn language_clear_by_empty_string() {
        let store = test_store();
        write_language_primary(&store, "ja").unwrap();
        assert_eq!(read_language_primary(&store), Some("ja".to_string()));
        write_language_primary(&store, "").unwrap();
        assert_eq!(read_language_primary(&store), None);
    }

    #[test]
    fn language_preserves_other_keys() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Europe).unwrap();
        write_language_primary(&store, "es").unwrap();
        write_language_secondary(&store, "en").unwrap();

        assert_eq!(read_region_preference(&store), RegionPreference::Europe);
        assert_eq!(read_language_primary(&store), Some("es".to_string()));
        assert_eq!(read_language_secondary(&store), Some("en".to_string()));
    }

    #[test]
    fn preferred_languages_explicit() {
        let langs = preferred_languages(Some("es"), Some("fr"), RegionPreference::Europe);
        assert_eq!(langs, vec!["es", "fr", "en"]);
    }

    #[test]
    fn preferred_languages_derived_from_japan() {
        let langs = preferred_languages(None, None, RegionPreference::Japan);
        assert_eq!(langs, vec!["ja", "en"]);
    }

    #[test]
    fn preferred_languages_derived_from_usa() {
        let langs = preferred_languages(None, None, RegionPreference::Usa);
        assert_eq!(langs, vec!["en"]);
    }

    #[test]
    fn preferred_languages_no_duplicate_en() {
        let langs = preferred_languages(Some("en"), Some("en"), RegionPreference::Usa);
        assert_eq!(langs, vec!["en"]);
    }

    #[test]
    fn language_match_score_primary() {
        let prefs = vec!["es".to_string(), "en".to_string()];
        assert_eq!(language_match_score("es", &prefs), 0);
    }

    #[test]
    fn language_match_score_secondary() {
        let prefs = vec!["es".to_string(), "en".to_string()];
        assert_eq!(language_match_score("en", &prefs), 1);
    }

    #[test]
    fn language_match_score_base_code() {
        let prefs = vec!["en".to_string()];
        assert_eq!(language_match_score("en-gb,de,es,fr,it", &prefs), 0);
    }

    #[test]
    fn language_match_score_no_match() {
        let prefs = vec!["es".to_string(), "en".to_string()];
        assert_eq!(language_match_score("ja", &prefs), 3);
    }

    // --- Locale tests ---

    #[test]
    fn locale_default_when_no_file() {
        let store = test_store();
        assert_eq!(read_locale(&store), None);
    }

    #[test]
    fn write_and_read_locale_en() {
        use replay_control_core::locale::Locale;
        let store = test_store();
        write_locale(&store, Locale::En).unwrap();
        assert_eq!(read_locale(&store), Some(Locale::En));
    }

    #[test]
    fn write_and_read_locale_ja() {
        use replay_control_core::locale::Locale;
        let store = test_store();
        write_locale(&store, Locale::Ja).unwrap();
        assert_eq!(read_locale(&store), Some(Locale::Ja));
    }

    #[test]
    fn write_and_read_locale_es() {
        use replay_control_core::locale::Locale;
        let store = test_store();
        write_locale(&store, Locale::Es).unwrap();
        assert_eq!(read_locale(&store), Some(Locale::Es));
    }

    #[test]
    fn write_auto_returns_none_for_read_locale() {
        use replay_control_core::locale::Locale;
        let store = test_store();
        write_locale(&store, Locale::Auto).unwrap();
        // read_locale filters out Auto
        assert_eq!(read_locale(&store), None);
        // but read_locale_preference returns Auto
        assert_eq!(read_locale_preference(&store), Locale::Auto);
    }

    #[test]
    fn locale_preserves_other_keys() {
        use replay_control_core::locale::Locale;
        let store = test_store();
        write_region_preference(&store, RegionPreference::Japan).unwrap();
        write_locale(&store, Locale::Ja).unwrap();
        assert_eq!(read_region_preference(&store), RegionPreference::Japan);
        assert_eq!(read_locale(&store), Some(Locale::Ja));
    }

    #[test]
    fn language_match_score_multi_lang() {
        let prefs = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(
            language_match_score("en-gb,de,es,fi,fr,it,nl,sv", &prefs),
            0
        );
    }

    // --- Update channel tests ---

    #[test]
    fn update_channel_default_stable() {
        let store = test_store();
        assert_eq!(
            read_update_channel(&store),
            replay_control_core::update::UpdateChannel::Stable
        );
    }

    #[test]
    fn write_and_read_update_channel_beta() {
        let store = test_store();
        write_update_channel(&store, replay_control_core::update::UpdateChannel::Beta).unwrap();
        assert_eq!(
            read_update_channel(&store),
            replay_control_core::update::UpdateChannel::Beta
        );
    }

    #[test]
    fn update_channel_preserves_other_keys() {
        let store = test_store();
        write_region_preference(&store, RegionPreference::Japan).unwrap();
        write_update_channel(&store, replay_control_core::update::UpdateChannel::Beta).unwrap();
        assert_eq!(read_region_preference(&store), RegionPreference::Japan);
        assert_eq!(
            read_update_channel(&store),
            replay_control_core::update::UpdateChannel::Beta
        );
    }

    // --- Skipped version tests ---

    #[test]
    fn skipped_version_default_none() {
        let store = test_store();
        assert!(read_skipped_version(&store).is_none());
    }

    #[test]
    fn write_and_read_skipped_version() {
        let store = test_store();
        write_skipped_version(&store, "0.3.0").unwrap();
        assert_eq!(read_skipped_version(&store), Some("0.3.0".to_string()));
    }

    // --- Migration tests ---

    #[test]
    fn migrate_copies_and_deletes_old() {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("replay-migrate-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        // Set up old-style storage layout
        let storage_root = base.join("storage");
        let old_dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(
            old_dir.join(SETTINGS_FILE),
            "locale = \"ja\"\ninstall_id = \"abc\"\n",
        )
        .unwrap();

        // Create destination store
        let dest_dir = base.join("etc");
        let store = SettingsStore::new(&dest_dir);

        store.migrate_from_storage(&storage_root).unwrap();

        // Destination has the file
        let content = std::fs::read_to_string(dest_dir.join(SETTINGS_FILE)).unwrap();
        assert!(content.contains("locale = \"ja\""));
        assert!(content.contains("install_id = \"abc\""));

        // Old file is deleted
        assert!(!old_dir.join(SETTINGS_FILE).exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_skips_if_dest_exists() {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("replay-migrate-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        let storage_root = base.join("storage");
        let old_dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join(SETTINGS_FILE), "locale = \"ja\"\n").unwrap();

        let dest_dir = base.join("etc");
        std::fs::create_dir_all(&dest_dir).unwrap();
        std::fs::write(dest_dir.join(SETTINGS_FILE), "locale = \"en\"\n").unwrap();

        let store = SettingsStore::new(&dest_dir);
        store.migrate_from_storage(&storage_root).unwrap();

        // Destination unchanged
        let content = std::fs::read_to_string(dest_dir.join(SETTINGS_FILE)).unwrap();
        assert!(content.contains("locale = \"en\""));

        // Old file still exists (not touched)
        assert!(old_dir.join(SETTINGS_FILE).exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_noop_when_no_old_file() {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("replay-migrate-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        let storage_root = base.join("storage");
        std::fs::create_dir_all(&storage_root).unwrap();

        let dest_dir = base.join("etc");
        let store = SettingsStore::new(&dest_dir);
        store.migrate_from_storage(&storage_root).unwrap();

        // No file created at destination
        assert!(!dest_dir.join(SETTINGS_FILE).exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
