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
}
