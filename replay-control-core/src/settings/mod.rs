//! App-specific settings stored in `.replay-control/settings.cfg`.
//!
//! Uses the same `key = "value"` format as `replay.cfg` but is kept separate
//! to avoid modifying the RePlayOS system configuration.

pub mod skins;

use std::path::Path;

use crate::config::AppSettings;
use crate::error::Result;
use crate::rom_tags::RegionPreference;
use crate::storage::{RC_DIR, SETTINGS_FILE};

/// Load settings from disk, returning empty settings if the file doesn't exist.
/// Use this directly when you need to read multiple settings to avoid repeated file I/O.
pub fn load_settings(storage_root: &Path) -> AppSettings {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    AppSettings::from_file(&path).unwrap_or_else(|_| AppSettings::empty())
}

/// Save settings to disk, creating the directory if needed.
pub fn save_settings(storage_root: &Path, settings: &AppSettings) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    let path = rc_dir.join(SETTINGS_FILE);
    settings.save(&path)
}

/// Read the region preference from `.replay-control/settings.cfg`.
/// Returns the default (`World`) if the file doesn't exist or the key is missing.
pub fn read_region_preference(storage_root: &Path) -> RegionPreference {
    let settings = load_settings(storage_root);
    RegionPreference::from_str_value(settings.region_preference())
}

/// Write the region preference to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference(storage_root: &Path, pref: RegionPreference) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_region_preference(pref.as_str());
    save_settings(storage_root, &settings)
}

/// Read the secondary region preference from `.replay-control/settings.cfg`.
/// Returns `None` if the file doesn't exist, the key is missing, or the value is empty.
pub fn read_region_preference_secondary(storage_root: &Path) -> Option<RegionPreference> {
    let settings = load_settings(storage_root);
    let value = settings.region_preference_secondary()?;
    Some(RegionPreference::from_str_value(value))
}

/// Write the secondary region preference to `.replay-control/settings.cfg`.
/// Pass `None` to clear the secondary preference (removes the key value).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference_secondary(
    storage_root: &Path,
    pref: Option<RegionPreference>,
) -> Result<()> {
    let mut settings = load_settings(storage_root);
    let value = pref.map(|p| p.as_str()).unwrap_or("");
    settings.set_region_preference_secondary(value);
    save_settings(storage_root, &settings)
}

/// Read the font size preference from `.replay-control/settings.cfg`.
/// Returns `"normal"` or `"large"`, defaults to `"normal"`.
pub fn read_font_size(storage_root: &Path) -> String {
    let settings = load_settings(storage_root);
    settings.font_size().to_string()
}

/// Write the font size preference to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_font_size(storage_root: &Path, size: &str) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_font_size(size);
    save_settings(storage_root, &settings)
}

/// Read the skin preference from `.replay-control/settings.cfg`.
/// Returns `Some(index)` if the user has explicitly chosen a skin (sync off),
/// or `None` if the key is absent (sync on — read from `replay.cfg` instead).
pub fn read_skin(storage_root: &Path) -> Option<u32> {
    let settings = load_settings(storage_root);
    settings.skin()
}

/// Write the skin preference to `.replay-control/settings.cfg`.
/// Pass `Some(index)` to store a specific skin (sync off).
/// Pass `None` to clear the key (sync on — defer to `replay.cfg`).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_skin(storage_root: &Path, skin: Option<u32>) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_skin(skin);
    save_settings(storage_root, &settings)
}

/// Read the primary language preference from `.replay-control/settings.cfg`.
/// Returns `None` if not explicitly set (caller should derive from region).
pub fn read_language_primary(storage_root: &Path) -> Option<String> {
    let settings = load_settings(storage_root);
    settings.language_primary().map(|s| s.to_string())
}

/// Write the primary language preference to `.replay-control/settings.cfg`.
/// Pass empty string to clear (revert to auto-detection from region).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_language_primary(storage_root: &Path, lang: &str) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_language_primary(lang);
    save_settings(storage_root, &settings)
}

/// Read the secondary (fallback) language preference from `.replay-control/settings.cfg`.
/// Returns `None` if not set. Defaults to `"en"` in the UI when not explicitly configured.
pub fn read_language_secondary(storage_root: &Path) -> Option<String> {
    let settings = load_settings(storage_root);
    settings.language_secondary().map(|s| s.to_string())
}

/// Write the secondary (fallback) language preference to `.replay-control/settings.cfg`.
/// Pass empty string to clear.
pub fn write_language_secondary(storage_root: &Path, lang: &str) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_language_secondary(lang);
    save_settings(storage_root, &settings)
}

/// Write both language preferences in a single load/save cycle.
pub fn write_language_preferences(
    storage_root: &Path,
    primary: &str,
    secondary: &str,
) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_language_primary(primary);
    settings.set_language_secondary(secondary);
    save_settings(storage_root, &settings)
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

/// Read the UI locale from `.replay-control/settings.cfg`.
/// Returns `None` if not set or "auto" (caller should fall back to Accept-Language).
pub fn read_locale(storage_root: &Path) -> Option<crate::locale::Locale> {
    let settings = load_settings(storage_root);
    settings.locale()
}

/// Read the stored locale preference including `Auto`.
pub fn read_locale_preference(storage_root: &Path) -> crate::locale::Locale {
    let settings = load_settings(storage_root);
    settings.locale_preference()
}

/// Write the UI locale to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_locale(storage_root: &Path, locale: crate::locale::Locale) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_locale(locale.code());
    save_settings(storage_root, &settings)
}

/// Read the GitHub API key from `.replay-control/settings.cfg`.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_github_api_key(storage_root: &Path) -> Option<String> {
    let settings = load_settings(storage_root);
    settings.github_api_key().map(|s| s.to_string())
}

/// Write the GitHub API key to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_github_api_key(storage_root: &Path, key: &str) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_github_api_key(key);
    save_settings(storage_root, &settings)
}

/// Read the update channel from `.replay-control/settings.cfg`.
pub fn read_update_channel(storage_root: &Path) -> crate::update::UpdateChannel {
    let settings = load_settings(storage_root);
    crate::update::UpdateChannel::from_str_value(settings.update_channel())
}

/// Write the update channel to `.replay-control/settings.cfg`.
pub fn write_update_channel(
    storage_root: &Path,
    channel: crate::update::UpdateChannel,
) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_update_channel(channel.as_str());
    save_settings(storage_root, &settings)
}

/// Read the skipped version from `.replay-control/settings.cfg`.
pub fn read_skipped_version(storage_root: &Path) -> Option<String> {
    let settings = load_settings(storage_root);
    settings.skipped_version().map(|s| s.to_string())
}

/// Write the skipped version to `.replay-control/settings.cfg`.
pub fn write_skipped_version(storage_root: &Path, version: &str) -> Result<()> {
    let mut settings = load_settings(storage_root);
    settings.set_skipped_version(version);
    save_settings(storage_root, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("replay-settings-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_when_no_file() {
        let tmp = tempdir();
        let pref = read_region_preference(&tmp);
        assert_eq!(pref, RegionPreference::World);
    }

    #[test]
    fn write_and_read_europe() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Europe).unwrap();
        let pref = read_region_preference(&tmp);
        assert_eq!(pref, RegionPreference::Europe);
    }

    #[test]
    fn write_and_read_japan() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Japan).unwrap();
        let pref = read_region_preference(&tmp);
        assert_eq!(pref, RegionPreference::Japan);
    }

    #[test]
    fn write_and_read_world() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::World).unwrap();
        let pref = read_region_preference(&tmp);
        assert_eq!(pref, RegionPreference::World);
    }

    #[test]
    fn overwrite_preserves_other_keys() {
        let tmp = tempdir();
        let rc = tmp.join(RC_DIR);
        std::fs::create_dir_all(&rc).unwrap();
        std::fs::write(
            rc.join(SETTINGS_FILE),
            "other_key = \"value\"\nregion_preference = \"usa\"\n",
        )
        .unwrap();

        write_region_preference(&tmp, RegionPreference::Japan).unwrap();

        let content = std::fs::read_to_string(rc.join(SETTINGS_FILE)).unwrap();
        assert!(content.contains("other_key = \"value\""));
        assert!(content.contains("region_preference = \"japan\""));
    }

    // --- Secondary region preference tests ---

    #[test]
    fn secondary_default_when_no_file() {
        let tmp = tempdir();
        let pref = read_region_preference_secondary(&tmp);
        assert_eq!(pref, None);
    }

    #[test]
    fn write_and_read_secondary_usa() {
        let tmp = tempdir();
        write_region_preference_secondary(&tmp, Some(RegionPreference::Usa)).unwrap();
        let pref = read_region_preference_secondary(&tmp);
        assert_eq!(pref, Some(RegionPreference::Usa));
    }

    #[test]
    fn write_and_read_secondary_japan() {
        let tmp = tempdir();
        write_region_preference_secondary(&tmp, Some(RegionPreference::Japan)).unwrap();
        let pref = read_region_preference_secondary(&tmp);
        assert_eq!(pref, Some(RegionPreference::Japan));
    }

    #[test]
    fn write_secondary_none_clears() {
        let tmp = tempdir();
        // Write a value, then clear it.
        write_region_preference_secondary(&tmp, Some(RegionPreference::Europe)).unwrap();
        assert_eq!(
            read_region_preference_secondary(&tmp),
            Some(RegionPreference::Europe)
        );
        write_region_preference_secondary(&tmp, None).unwrap();
        assert_eq!(read_region_preference_secondary(&tmp), None);
    }

    #[test]
    fn secondary_preserves_primary() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Japan).unwrap();
        write_region_preference_secondary(&tmp, Some(RegionPreference::Usa)).unwrap();

        assert_eq!(read_region_preference(&tmp), RegionPreference::Japan);
        assert_eq!(
            read_region_preference_secondary(&tmp),
            Some(RegionPreference::Usa)
        );
    }

    // --- Skin preference tests ---

    #[test]
    fn skin_none_when_no_file() {
        let tmp = tempdir();
        assert_eq!(read_skin(&tmp), None);
    }

    #[test]
    fn write_and_read_skin() {
        let tmp = tempdir();
        write_skin(&tmp, Some(5)).unwrap();
        assert_eq!(read_skin(&tmp), Some(5));
    }

    #[test]
    fn write_skin_none_clears() {
        let tmp = tempdir();
        write_skin(&tmp, Some(3)).unwrap();
        assert_eq!(read_skin(&tmp), Some(3));
        write_skin(&tmp, None).unwrap();
        assert_eq!(read_skin(&tmp), None);
    }

    #[test]
    fn skin_preserves_other_keys() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Japan).unwrap();
        write_skin(&tmp, Some(7)).unwrap();

        assert_eq!(read_region_preference(&tmp), RegionPreference::Japan);
        assert_eq!(read_skin(&tmp), Some(7));
    }

    // --- Language preference tests ---

    #[test]
    fn language_default_when_no_file() {
        let tmp = tempdir();
        assert_eq!(read_language_primary(&tmp), None);
        assert_eq!(read_language_secondary(&tmp), None);
    }

    #[test]
    fn write_and_read_language_primary() {
        let tmp = tempdir();
        write_language_primary(&tmp, "es").unwrap();
        assert_eq!(read_language_primary(&tmp), Some("es".to_string()));
    }

    #[test]
    fn write_and_read_language_secondary() {
        let tmp = tempdir();
        write_language_secondary(&tmp, "fr").unwrap();
        assert_eq!(read_language_secondary(&tmp), Some("fr".to_string()));
    }

    #[test]
    fn language_clear_by_empty_string() {
        let tmp = tempdir();
        write_language_primary(&tmp, "ja").unwrap();
        assert_eq!(read_language_primary(&tmp), Some("ja".to_string()));
        write_language_primary(&tmp, "").unwrap();
        assert_eq!(read_language_primary(&tmp), None);
    }

    #[test]
    fn language_preserves_other_keys() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Europe).unwrap();
        write_language_primary(&tmp, "es").unwrap();
        write_language_secondary(&tmp, "en").unwrap();

        assert_eq!(read_region_preference(&tmp), RegionPreference::Europe);
        assert_eq!(read_language_primary(&tmp), Some("es".to_string()));
        assert_eq!(read_language_secondary(&tmp), Some("en".to_string()));
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
        let tmp = tempdir();
        assert_eq!(read_locale(&tmp), None);
    }

    #[test]
    fn write_and_read_locale_en() {
        use crate::locale::Locale;
        let tmp = tempdir();
        write_locale(&tmp, Locale::En).unwrap();
        assert_eq!(read_locale(&tmp), Some(Locale::En));
    }

    #[test]
    fn write_and_read_locale_ja() {
        use crate::locale::Locale;
        let tmp = tempdir();
        write_locale(&tmp, Locale::Ja).unwrap();
        assert_eq!(read_locale(&tmp), Some(Locale::Ja));
    }

    #[test]
    fn write_and_read_locale_es() {
        use crate::locale::Locale;
        let tmp = tempdir();
        write_locale(&tmp, Locale::Es).unwrap();
        assert_eq!(read_locale(&tmp), Some(Locale::Es));
    }

    #[test]
    fn write_auto_returns_none_for_read_locale() {
        use crate::locale::Locale;
        let tmp = tempdir();
        write_locale(&tmp, Locale::Auto).unwrap();
        // read_locale filters out Auto
        assert_eq!(read_locale(&tmp), None);
        // but read_locale_preference returns Auto
        assert_eq!(read_locale_preference(&tmp), Locale::Auto);
    }

    #[test]
    fn locale_preserves_other_keys() {
        use crate::locale::Locale;
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Japan).unwrap();
        write_locale(&tmp, Locale::Ja).unwrap();
        assert_eq!(read_region_preference(&tmp), RegionPreference::Japan);
        assert_eq!(read_locale(&tmp), Some(Locale::Ja));
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
        let tmp = tempdir();
        assert_eq!(
            read_update_channel(&tmp),
            crate::update::UpdateChannel::Stable
        );
    }

    #[test]
    fn write_and_read_update_channel_beta() {
        let tmp = tempdir();
        write_update_channel(&tmp, crate::update::UpdateChannel::Beta).unwrap();
        assert_eq!(
            read_update_channel(&tmp),
            crate::update::UpdateChannel::Beta
        );
    }

    #[test]
    fn update_channel_preserves_other_keys() {
        let tmp = tempdir();
        write_region_preference(&tmp, RegionPreference::Japan).unwrap();
        write_update_channel(&tmp, crate::update::UpdateChannel::Beta).unwrap();
        assert_eq!(read_region_preference(&tmp), RegionPreference::Japan);
        assert_eq!(
            read_update_channel(&tmp),
            crate::update::UpdateChannel::Beta
        );
    }

    // --- Skipped version tests ---

    #[test]
    fn skipped_version_default_none() {
        let tmp = tempdir();
        assert!(read_skipped_version(&tmp).is_none());
    }

    #[test]
    fn write_and_read_skipped_version() {
        let tmp = tempdir();
        write_skipped_version(&tmp, "0.3.0").unwrap();
        assert_eq!(read_skipped_version(&tmp), Some("0.3.0".to_string()));
    }
}
