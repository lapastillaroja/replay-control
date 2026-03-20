/// Format a number with thousands separators (e.g., 15440 -> "15,440").
pub fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

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

/// Systems whose ROM sizes should be displayed in Megabit (Mbit/Kbit).
///
/// Mirrors `MEGABIT_SYSTEMS` in `replay-control-core/src/systems.rs`.
/// Duplicated here so the function works in WASM (hydrate) builds where
/// the core crate is not available.
const MEGABIT_SYSTEMS: &[&str] = &[
    "atari_2600",
    "atari_5200",
    "atari_7800",
    "atari_jaguar",
    "atari_lynx",
    "nintendo_nes",
    "nintendo_snes",
    "nintendo_n64",
    "nintendo_gb",
    "nintendo_gbc",
    "nintendo_gba",
    "sega_sg",
    "sega_sms",
    "sega_smd",
    "sega_32x",
    "sega_gg",
    "nec_pce",
    "snk_ng",
    "snk_ngp",
    "arcade_fbneo",
    "arcade_mame",
    "arcade_mame_2k3p",
];

/// Format a byte count using historically appropriate units for the given system.
///
/// Cartridge-based and arcade ROM-chip systems display in Megabit (Mbit) or
/// Kilobit (Kbit). Disc-based and computer systems display in KB/MB/GB.
pub fn format_size_for_system(bytes: u64, system: &str) -> String {
    if MEGABIT_SYSTEMS.contains(&system) {
        format_size_megabit(bytes)
    } else {
        format_size(bytes)
    }
}

/// Format bytes as Megabit (Mbit) or Kilobit (Kbit) for cartridge-based systems.
///
/// - Under 1 Mbit (131,072 bytes): displays as Kbit (e.g., "256 Kbit")
/// - 1 Mbit and above: displays as Mbit, with one decimal place if not whole
///   (e.g., "16 Mbit", "4.5 Mbit")
/// - Under 128 bytes (1 Kbit): falls back to showing bytes
pub fn format_size_megabit(bytes: u64) -> String {
    let bits = bytes * 8;
    const MEGABIT: u64 = 1_048_576; // 1,048,576 bits = 1 Mbit

    if bits >= MEGABIT {
        let mbit = bits as f64 / MEGABIT as f64;
        if (mbit - mbit.round()).abs() < 0.01 {
            format!("{} Mbit", mbit.round() as u64)
        } else {
            format!("{:.1} Mbit", mbit)
        }
    } else {
        let kbit = bits / 1024;
        if kbit > 0 {
            format!("{} Kbit", kbit)
        } else {
            format!("{} bytes", bytes)
        }
    }
}

/// URL-safe base64 encoding (no padding, uses `-` and `_` instead of `+` and `/`).
///
/// Used for encoding ROM paths in URLs where the path may contain special characters.
pub fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        }
    }

    result
}

/// URL-safe base64 decoding (no padding, uses `-` and `_`).
pub fn base64_decode(s: &str) -> Result<Vec<u8>, &'static str> {
    fn decode_char(c: u8) -> Result<u8, &'static str> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'-' => Ok(62),
            b'_' => Ok(63),
            _ => Err("invalid base64 character"),
        }
    }

    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        let a = decode_char(chunk[0])? as u32;
        let b = if chunk.len() > 1 { decode_char(chunk[1])? as u32 } else { 0 };
        let c = if chunk.len() > 2 { decode_char(chunk[2])? as u32 } else { 0 };
        let d = if chunk.len() > 3 { decode_char(chunk[3])? as u32 } else { 0 };
        let n = (a << 18) | (b << 12) | (c << 6) | d;

        result.push((n >> 16) as u8);
        if chunk.len() > 2 {
            result.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            result.push(n as u8);
        }
    }

    Ok(result)
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
        assert_eq!(format_size_short(13_207_024_435), ("12".to_string(), "GB"));
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
        assert!(
            below_gb.ends_with("MB"),
            "Just below GB should be MB, got {below_gb}"
        );
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
        assert!(
            result.ends_with("GB"),
            "u64::MAX should show as GB, got {result}"
        );
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

    // --- format_size_megabit tests ---

    #[test]
    fn megabit_zero() {
        assert_eq!(format_size_megabit(0), "0 bytes");
    }

    #[test]
    fn megabit_tiny_bytes() {
        // Under 128 bytes (1 Kbit) -> falls back to bytes
        assert_eq!(format_size_megabit(64), "64 bytes");
        assert_eq!(format_size_megabit(1), "1 bytes");
    }

    #[test]
    fn megabit_kbit_values() {
        // 2 KB = 16 Kbit (Atari 2600 ROM)
        assert_eq!(format_size_megabit(2048), "16 Kbit");
        // 4 KB = 32 Kbit
        assert_eq!(format_size_megabit(4096), "32 Kbit");
        // 32 KB = 256 Kbit
        assert_eq!(format_size_megabit(32_768), "256 Kbit");
    }

    #[test]
    fn megabit_exact_mbit_values() {
        // 128 KB = 1 Mbit
        assert_eq!(format_size_megabit(131_072), "1 Mbit");
        // 256 KB = 2 Mbit
        assert_eq!(format_size_megabit(262_144), "2 Mbit");
        // 512 KB = 4 Mbit (classic SMS/GG)
        assert_eq!(format_size_megabit(524_288), "4 Mbit");
        // 1 MB = 8 Mbit (Super Mario World on SNES)
        assert_eq!(format_size_megabit(1_048_576), "8 Mbit");
        // 2 MB = 16 Mbit (Sonic 3)
        assert_eq!(format_size_megabit(2_097_152), "16 Mbit");
        // 3 MB = 24 Mbit (Phantasy Star IV)
        assert_eq!(format_size_megabit(3_145_728), "24 Mbit");
        // 4 MB = 32 Mbit (DKC on SNES)
        assert_eq!(format_size_megabit(4_194_304), "32 Mbit");
        // 8 MB = 64 Mbit (Super Mario 64)
        assert_eq!(format_size_megabit(8_388_608), "64 Mbit");
        // 32 MB = 256 Mbit (RE2 on N64)
        assert_eq!(format_size_megabit(33_554_432), "256 Mbit");
        // 64 MB = 512 Mbit (Conker's Bad Fur Day)
        assert_eq!(format_size_megabit(67_108_864), "512 Mbit");
    }

    #[test]
    fn megabit_whole_mbit_no_decimal() {
        // 768 KB = 6 Mbit (whole number)
        assert_eq!(format_size_megabit(786_432), "6 Mbit");
        // 640 KB = 5 Mbit (whole number)
        assert_eq!(format_size_megabit(655_360), "5 Mbit");
    }

    #[test]
    fn megabit_fractional_mbit() {
        // 192 KB = 1.5 Mbit
        assert_eq!(format_size_megabit(196_608), "1.5 Mbit");
        // 576 KB = 4.5 Mbit
        assert_eq!(format_size_megabit(589_824), "4.5 Mbit");
    }

    #[test]
    fn megabit_large_values() {
        // 86 MB = 688 Mbit (largest Neo Geo carts)
        assert_eq!(format_size_megabit(90_177_536), "688 Mbit");
    }

    // --- format_size_for_system tests ---

    #[test]
    fn format_for_system_megabit() {
        // SNES ROM: 1 MB should show as 8 Mbit
        assert_eq!(format_size_for_system(1_048_576, "nintendo_snes"), "8 Mbit");
    }

    #[test]
    fn format_for_system_regular() {
        // PlayStation: 500 MB should show in MB
        assert_eq!(format_size_for_system(524_288_000, "sony_psx"), "500.0 MB");
    }

    #[test]
    fn format_for_system_ds_uses_mb() {
        // DS uses MB, not Mbit
        assert_eq!(format_size_for_system(33_554_432, "nintendo_ds"), "32.0 MB");
    }

    #[test]
    fn format_for_system_arcade_dc_uses_mb() {
        // arcade_dc uses MB
        assert_eq!(format_size_for_system(524_288_000, "arcade_dc"), "500.0 MB");
    }

    #[test]
    fn format_for_system_unknown_uses_mb() {
        // Unknown system defaults to MB/GB
        assert_eq!(
            format_size_for_system(1_048_576, "unknown_system"),
            "1.0 MB"
        );
    }
}
