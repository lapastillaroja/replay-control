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
}
