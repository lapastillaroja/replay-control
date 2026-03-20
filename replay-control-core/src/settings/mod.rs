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
    let value = config.get("region_preference").unwrap_or("usa");
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
}
