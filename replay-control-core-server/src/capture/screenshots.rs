use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};

/// A user-taken screenshot found on disk.
#[derive(Debug, Clone)]
pub struct UserScreenshot {
    pub filename: String,
    /// Unix timestamp in seconds, parsed from the `_YYYYMMDD_HHMMSS` suffix.
    pub timestamp: Option<i64>,
}

/// Scan the captures directory for screenshots matching a specific ROM.
///
/// Screenshots are matched by filename prefix: the file must start with
/// `rom_filename` followed by `_` (timestamped) or `.` (legacy `.png`).
/// Returns results sorted by timestamp descending (newest first).
pub fn find_screenshots_for_rom(
    storage: &StorageLocation,
    system: &str,
    rom_filename: &str,
) -> Vec<UserScreenshot> {
    let dir = storage.captures_dir().join(system);
    if !dir.exists() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut screenshots: Vec<UserScreenshot> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".png") {
                return None;
            }
            if !screenshot_belongs_to_rom(&name, rom_filename) {
                return None;
            }

            let timestamp = parse_timestamp_suffix(&name);
            Some(UserScreenshot {
                filename: name,
                timestamp,
            })
        })
        .collect();

    // Sort by timestamp descending (newest first). Screenshots without a
    // timestamp (legacy format) sort last.
    screenshots.sort_by_key(|s| std::cmp::Reverse(s.timestamp));
    screenshots
}

/// Delete one user capture screenshot owned by `rom_filename`.
///
/// The filename must be a single PNG basename under `<storage>/captures/<system>`
/// and must start with the exact ROM filename followed by `_` or `.`.
pub fn delete_screenshot_for_rom(
    storage: &StorageLocation,
    system: &str,
    rom_filename: &str,
    screenshot_filename: &str,
) -> Result<()> {
    validate_path_component(system, "system")?;
    validate_path_component(rom_filename, "ROM filename")?;
    validate_path_component(screenshot_filename, "screenshot filename")?;

    if !screenshot_belongs_to_rom(screenshot_filename, rom_filename) {
        return Err(Error::Other(
            "Screenshot does not belong to this ROM".to_string(),
        ));
    }

    let path = storage
        .captures_dir()
        .join(system)
        .join(screenshot_filename);
    if !path.exists() {
        return Err(Error::RomNotFound(path));
    }

    std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))
}

fn validate_path_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(Error::Other(format!("Invalid {label}")));
    }
    Ok(())
}

pub fn screenshot_belongs_to_rom(screenshot_filename: &str, rom_filename: &str) -> bool {
    if !screenshot_filename.ends_with(".png") || !screenshot_filename.starts_with(rom_filename) {
        return false;
    }
    matches!(
        screenshot_filename.as_bytes().get(rom_filename.len()),
        Some(b'_' | b'.')
    )
}

/// Parse the `_YYYYMMDD_HHMMSS.png` suffix from a screenshot filename.
/// Returns the corresponding Unix timestamp in seconds, or `None` if the
/// suffix doesn't match the expected pattern.
fn parse_timestamp_suffix(filename: &str) -> Option<i64> {
    // Expected: ..._{YYYYMMDD}_{HHMMSS}.png
    let stem = filename.strip_suffix(".png")?;
    // Split from the right to find _HHMMSS and _YYYYMMDD
    let (rest, time_str) = rsplit_at_char(stem, '_')?;
    let (_, date_str) = rsplit_at_char(rest, '_')?;

    if date_str.len() != 8 || time_str.len() != 6 {
        return None;
    }

    let year: i32 = date_str[0..4].parse().ok()?;
    let month: u32 = date_str[4..6].parse().ok()?;
    let day: u32 = date_str[6..8].parse().ok()?;
    let hour: u32 = time_str[0..2].parse().ok()?;
    let minute: u32 = time_str[2..4].parse().ok()?;
    let second: u32 = time_str[4..6].parse().ok()?;

    // Validate ranges
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }

    // Convert to Unix timestamp manually (UTC assumed).
    // days_from_civil algorithm (Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u32;
    let m = month as i32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u32;
    let days = era * 146097 + doe as i32 - 719468;

    let timestamp = days as i64 * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    Some(timestamp)
}

/// Split a string at the last occurrence of `ch`, returning (before, after).
fn rsplit_at_char(s: &str, ch: char) -> Option<(&str, &str)> {
    let idx = s.rfind(ch)?;
    Some((&s[..idx], &s[idx + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_suffix() {
        let ts = parse_timestamp_suffix("Sonic.md_20260310_015805.png");
        assert!(ts.is_some());
        // 2026-03-10 01:58:05 UTC
        let t = ts.unwrap();
        assert!(t > 0);

        // Legacy format: no timestamp
        assert!(parse_timestamp_suffix("Sonic.md.png").is_none());

        // Invalid timestamp
        assert!(parse_timestamp_suffix("Sonic.md_20261310_015805.png").is_none());
    }

    #[test]
    fn test_parse_known_timestamp() {
        // 2026-01-01 00:00:00 UTC
        let ts = parse_timestamp_suffix("game.zip_20260101_000000.png").unwrap();
        // 2026-01-01 is day 20454 from epoch (verified independently)
        assert_eq!(ts, 1767225600);
    }

    #[test]
    fn test_rsplit_at_char() {
        assert_eq!(rsplit_at_char("a_b_c", '_'), Some(("a_b", "c")));
        assert_eq!(rsplit_at_char("abc", '_'), None);
    }

    #[test]
    fn screenshot_ownership_requires_rom_prefix_boundary_and_png() {
        assert!(screenshot_belongs_to_rom(
            "Sonic.md_20260310_015805.png",
            "Sonic.md"
        ));
        assert!(screenshot_belongs_to_rom("Sonic.md.png", "Sonic.md"));
        assert!(!screenshot_belongs_to_rom(
            "Sonic.md-20260310_015805.png",
            "Sonic.md"
        ));
        assert!(!screenshot_belongs_to_rom("Sonic.md_1.jpg", "Sonic.md"));
        assert!(!screenshot_belongs_to_rom(
            "Sonic.md2_20260310_015805.png",
            "Sonic.md"
        ));
    }

    #[test]
    fn delete_screenshot_only_removes_owned_png_under_system_captures() {
        let root = unique_test_root("capture-delete");
        let captures = root.join("captures/nintendo_nes");
        std::fs::create_dir_all(&captures).unwrap();
        let owned = captures.join("TestGame.nes_20260310_015805.png");
        let neighbor = captures.join("TestGame.nes2_20260310_015805.png");
        let non_png = captures.join("TestGame.nes_20260310_015805.jpg");
        std::fs::write(&owned, b"png").unwrap();
        std::fs::write(&neighbor, b"png").unwrap();
        std::fs::write(&non_png, b"jpg").unwrap();

        let storage = StorageLocation::from_path(root.clone(), crate::storage::StorageKind::Folder);

        delete_screenshot_for_rom(
            &storage,
            "nintendo_nes",
            "TestGame.nes",
            "TestGame.nes_20260310_015805.png",
        )
        .unwrap();

        assert!(!owned.exists());
        assert!(neighbor.exists());
        assert!(non_png.exists());

        let result = delete_screenshot_for_rom(
            &storage,
            "nintendo_nes",
            "TestGame.nes",
            "TestGame.nes2_20260310_015805.png",
        );
        assert!(result.is_err());
        assert!(neighbor.exists());

        let result = delete_screenshot_for_rom(
            &storage,
            "nintendo_nes",
            "TestGame.nes",
            "../TestGame.nes_20260310_015805.png",
        );
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_test_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "replay-control-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }
}
