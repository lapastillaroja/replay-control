//! App-specific settings stored in `.replay-control/settings.cfg`.
//!
//! Uses the same `key = "value"` format as `replay.cfg` but is kept separate
//! to avoid modifying the RePlayOS system configuration.

pub mod skins;

use std::path::Path;

use crate::config::ReplayConfig;
use crate::error::Result;
use crate::rom_tags::RegionPreference;
use crate::storage::{RC_DIR, SETTINGS_FILE};

/// Read the region preference from `.replay-control/settings.cfg`.
/// Returns the default (`Usa`) if the file doesn't exist or the key is missing.
pub fn read_region_preference(storage_root: &Path) -> RegionPreference {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = match ReplayConfig::from_file(&path) {
        Ok(c) => c,
        Err(_) => return RegionPreference::default(),
    };
    let value = config.get("region_preference").unwrap_or("world");
    RegionPreference::from_str_value(value)
}

/// Write the region preference to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference(storage_root: &Path, pref: RegionPreference) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    config.set("region_preference", pref.as_str());

    // Write the config. If the file exists, preserve comments and order.
    // If it doesn't exist, write from scratch.
    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        // Write a fresh file with just this key.
        let content = format!("region_preference = \"{}\"\n", pref.as_str());
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
}

/// Read the secondary region preference from `.replay-control/settings.cfg`.
/// Returns `None` if the file doesn't exist, the key is missing, or the value is empty.
pub fn read_region_preference_secondary(storage_root: &Path) -> Option<RegionPreference> {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = ReplayConfig::from_file(&path).ok()?;
    let value = config.get("region_preference_secondary").unwrap_or("");
    if value.is_empty() {
        return None;
    }
    Some(RegionPreference::from_str_value(value))
}

/// Write the secondary region preference to `.replay-control/settings.cfg`.
/// Pass `None` to clear the secondary preference (removes the key value).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_region_preference_secondary(
    storage_root: &Path,
    pref: Option<RegionPreference>,
) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    let value = pref.map(|p| p.as_str()).unwrap_or("");
    config.set("region_preference_secondary", value);

    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        let content = format!("region_preference_secondary = \"{value}\"\n");
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
}

/// Read the font size preference from `.replay-control/settings.cfg`.
/// Returns `"normal"` or `"large"`, defaults to `"normal"`.
pub fn read_font_size(storage_root: &Path) -> String {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = match ReplayConfig::from_file(&path) {
        Ok(c) => c,
        Err(_) => return "normal".to_string(),
    };
    let value = config.get("font_size").unwrap_or("normal");
    match value {
        "large" => "large".to_string(),
        _ => "normal".to_string(),
    }
}

/// Write the font size preference to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_font_size(storage_root: &Path, size: &str) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    let value = if size == "large" { "large" } else { "normal" };
    config.set("font_size", value);

    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        let content = format!("font_size = \"{value}\"\n");
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
}

/// Read the skin preference from `.replay-control/settings.cfg`.
/// Returns `Some(index)` if the user has explicitly chosen a skin (sync off),
/// or `None` if the key is absent (sync on — read from `replay.cfg` instead).
pub fn read_skin(storage_root: &Path) -> Option<u32> {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = ReplayConfig::from_file(&path).ok()?;
    let value = config.get("skin")?;
    if value.is_empty() {
        return None;
    }
    value.parse().ok()
}

/// Write the skin preference to `.replay-control/settings.cfg`.
/// Pass `Some(index)` to store a specific skin (sync off).
/// Pass `None` to clear the key (sync on — defer to `replay.cfg`).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_skin(storage_root: &Path, skin: Option<u32>) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    match skin {
        Some(index) => config.set("skin", &index.to_string()),
        None => config.set("skin", ""),
    }

    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        let value = skin.map(|i| i.to_string()).unwrap_or_default();
        let content = format!("skin = \"{value}\"\n");
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
}

/// Read the primary language preference from `.replay-control/settings.cfg`.
/// Returns `None` if not explicitly set (caller should derive from region).
pub fn read_language_primary(storage_root: &Path) -> Option<String> {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = ReplayConfig::from_file(&path).ok()?;
    let value = config.get("language_primary").unwrap_or("").to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Write the primary language preference to `.replay-control/settings.cfg`.
/// Pass empty string to clear (revert to auto-detection from region).
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_language_primary(storage_root: &Path, lang: &str) -> Result<()> {
    write_setting(storage_root, "language_primary", lang)
}

/// Read the secondary (fallback) language preference from `.replay-control/settings.cfg`.
/// Returns `None` if not set. Defaults to `"en"` in the UI when not explicitly configured.
pub fn read_language_secondary(storage_root: &Path) -> Option<String> {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = ReplayConfig::from_file(&path).ok()?;
    let value = config.get("language_secondary").unwrap_or("").to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Write the secondary (fallback) language preference to `.replay-control/settings.cfg`.
/// Pass empty string to clear.
pub fn write_language_secondary(storage_root: &Path, lang: &str) -> Result<()> {
    write_setting(storage_root, "language_secondary", lang)
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

/// Generic helper: write a single key to settings.cfg, creating directory if needed.
fn write_setting(storage_root: &Path, key: &str, value: &str) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    config.set(key, value);

    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        let content = format!("{key} = \"{value}\"\n");
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
}

/// Read the GitHub API key from `.replay-control/settings.cfg`.
/// Returns `None` if the file doesn't exist or the key is empty.
pub fn read_github_api_key(storage_root: &Path) -> Option<String> {
    let path = storage_root.join(RC_DIR).join(SETTINGS_FILE);
    let config = ReplayConfig::from_file(&path).ok()?;
    let value = config.get("github_api_key").unwrap_or("").to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Write the GitHub API key to `.replay-control/settings.cfg`.
/// Creates the directory and file if they don't exist. Preserves other keys.
pub fn write_github_api_key(storage_root: &Path, key: &str) -> Result<()> {
    let rc_dir = storage_root.join(RC_DIR);
    if !rc_dir.exists() {
        std::fs::create_dir_all(&rc_dir).map_err(|e| crate::error::Error::io(&rc_dir, e))?;
    }

    let path = rc_dir.join(SETTINGS_FILE);

    let mut config = if path.exists() {
        ReplayConfig::from_file(&path)?
    } else {
        ReplayConfig::parse("")?
    };

    config.set("github_api_key", key);

    if path.exists() {
        config.write_to_file(&path, &path)?;
    } else {
        let content = format!("github_api_key = \"{key}\"\n");
        std::fs::write(&path, content).map_err(|e| crate::error::Error::io(&path, e))?;
    }

    Ok(())
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

    #[test]
    fn language_match_score_multi_lang() {
        let prefs = vec!["fr".to_string(), "en".to_string()];
        assert_eq!(
            language_match_score("en-gb,de,es,fi,fr,it,nl,sv", &prefs),
            0
        );
    }
}
