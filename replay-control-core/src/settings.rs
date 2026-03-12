//! App-specific settings stored in `.replay-control/settings.cfg`.
//!
//! Uses the same `key = "value"` format as `replay.cfg` but is kept separate
//! to avoid modifying the RePlayOS system configuration.

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
        assert_eq!(pref, RegionPreference::Usa);
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
}
