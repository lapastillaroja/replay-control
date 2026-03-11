/// Format a byte count as a human-readable string (KB / MB / GB).
pub fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

/// Like [`format_size`], but rounds GB values to whole numbers.
///
/// Returns `(number_string, unit)` — e.g. `("12", "GB")` or `("5.5", "MB")`.
pub fn format_size_short(bytes: u64) -> (String, &'static str) {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;

    if bytes >= GB {
        let gb = (bytes as f64 / GB as f64).round() as u64;
        (gb.to_string(), "GB")
    } else if bytes >= MB {
        (format!("{:.1}", bytes as f64 / MB as f64), "MB")
    } else {
        ((bytes / 1024).to_string(), "KB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_as_kb() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(512 * 1024), "512 KB");
    }

    #[test]
    fn format_bytes_as_mb() {
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(5 * 1_048_576 + 524_288), "5.5 MB");
    }

    #[test]
    fn format_bytes_as_gb() {
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
        assert_eq!(format_size(2_684_354_560), "2.5 GB");
    }

    #[test]
    fn format_short_gb_rounds() {
        assert_eq!(format_size_short(1_073_741_824), ("1".to_string(), "GB"));
        // 2.5 GB rounds to 3
        assert_eq!(format_size_short(2_684_354_560), ("3".to_string(), "GB"));
        // ~12.3 GB rounds to 12
        assert_eq!(
            format_size_short(13_207_024_435),
            ("12".to_string(), "GB")
        );
    }

    #[test]
    fn format_short_mb_keeps_decimals() {
        assert_eq!(
            format_size_short(5 * 1_048_576 + 524_288),
            ("5.5".to_string(), "MB")
        );
    }

    #[test]
    fn format_short_kb() {
        assert_eq!(format_size_short(512 * 1024), ("512".to_string(), "KB"));
    }

    // --- Edge cases for format_size ---

    #[test]
    fn format_zero_bytes() {
        assert_eq!(format_size(0), "0 KB");
    }

    #[test]
    fn format_one_byte() {
        // Integer division: 1 / 1024 = 0
        assert_eq!(format_size(1), "0 KB");
    }

    #[test]
    fn format_1023_bytes() {
        // Just under 1 KB
        assert_eq!(format_size(1023), "0 KB");
    }

    #[test]
    fn format_mb_boundary() {
        // Exactly at the MB boundary
        assert_eq!(format_size(1_048_576), "1.0 MB");
        // One byte below MB
        assert_eq!(format_size(1_048_575), "1023 KB");
    }

    #[test]
    fn format_gb_boundary() {
        // Exactly at the GB boundary
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
        // One byte below GB -- should be MB
        let below_gb = format_size(1_073_741_823);
        assert!(below_gb.ends_with("MB"), "Just below GB should be MB, got {below_gb}");
    }

    #[test]
    fn format_very_large_value() {
        // 1 TB
        assert_eq!(format_size(1_099_511_627_776), "1024.0 GB");
    }

    #[test]
    fn format_u64_max() {
        // Should not panic on max value
        let result = format_size(u64::MAX);
        assert!(result.ends_with("GB"), "u64::MAX should show as GB, got {result}");
    }

    // --- Edge cases for format_size_short ---

    #[test]
    fn format_short_zero() {
        assert_eq!(format_size_short(0), ("0".to_string(), "KB"));
    }

    #[test]
    fn format_short_one_byte() {
        assert_eq!(format_size_short(1), ("0".to_string(), "KB"));
    }

    #[test]
    fn format_short_mb_boundary() {
        assert_eq!(format_size_short(1_048_576), ("1.0".to_string(), "MB"));
    }

    #[test]
    fn format_short_gb_rounding_up() {
        // 1.5 GB should round to 2
        let bytes = 1_073_741_824 + 536_870_912; // 1.5 GB
        assert_eq!(format_size_short(bytes), ("2".to_string(), "GB"));
    }

    #[test]
    fn format_short_gb_rounding_down() {
        // 1.4 GB should round to 1
        let bytes = (1_073_741_824.0 * 1.4) as u64;
        assert_eq!(format_size_short(bytes), ("1".to_string(), "GB"));
    }
}
