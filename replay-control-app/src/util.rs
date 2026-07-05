use replay_control_core::DatePrecision;
use replay_control_core::systems::find_system_uses_megabit;

use crate::i18n::Locale;

/// Reload the page after `delay_ms`. Used after regenerating the TLS certificate:
/// the service restarts with a new cert whose fingerprint the browser hasn't
/// accepted, so every subsequent fetch fails. A full reload routes the user
/// through the browser's certificate-accept flow instead of a broken SPA.
#[cfg(target_arch = "wasm32")]
pub fn reload_after_ms(delay_ms: i32) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let cb = Closure::<dyn Fn()>::new(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.location().reload();
        }
    });
    let func: web_sys::js_sys::Function = cb
        .as_ref()
        .unchecked_ref::<web_sys::js_sys::Function>()
        .clone();
    cb.forget();
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&func, delay_ms);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn reload_after_ms(_delay_ms: i32) {}

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

/// Compute integer percentage, returning 0 if `total` is zero.
pub fn pct(count: usize, total: usize) -> u32 {
    if total == 0 {
        0
    } else {
        (count as f64 / total as f64 * 100.0) as u32
    }
}

pub fn numeric_code(value: &str, max_len: usize) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(max_len)
        .collect()
}

pub fn is_valid_net_control_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|ch| ch.is_ascii_digit())
}

pub fn sanitize_next_path(next: Option<String>) -> String {
    let Some(next) = next else {
        return "/".to_string();
    };
    if next.starts_with('/')
        && !next.starts_with("//")
        && !next.starts_with("/\\")
        && !next.starts_with("/login")
    {
        next
    } else {
        "/".to_string()
    }
}

/// Format a min/max year pair as "1985–1999" (or single year, or empty).
pub fn format_year_range(min: Option<u16>, max: Option<u16>) -> Option<String> {
    match (min, max) {
        (Some(a), Some(b)) if a != b => Some(format!("{a}\u{2013}{b}")),
        (Some(y), _) | (_, Some(y)) => Some(y.to_string()),
        _ => None,
    }
}

/// Format an ISO 8601 partial/full release date according to precision + locale.
///
/// - `Year` → `"1991"`
/// - `Month` → localized "Aug 1991" / "ago. 1991" / "1991年8月"
/// - `Day` → localized "Aug 23, 1991" / "23 ago. 1991" / "1991年8月23日"
///
/// When `precision` is `None`, it's inferred from the string length.
/// Returns `None` if the date is unparseable.
pub fn format_release_date(
    date: &str,
    precision: Option<DatePrecision>,
    locale: Locale,
) -> Option<String> {
    // Parse YYYY[-MM[-DD]] loosely.
    let year = date.get(..4).and_then(|y| y.parse::<u16>().ok())?;
    let month = date
        .get(5..7)
        .and_then(|m| m.parse::<u8>().ok())
        .filter(|m| (1..=12).contains(m));
    let day = date
        .get(8..10)
        .and_then(|d| d.parse::<u8>().ok())
        .filter(|d| (1..=31).contains(d));

    let effective = precision.unwrap_or(match (month, day) {
        (Some(_), Some(_)) => DatePrecision::Day,
        (Some(_), None) => DatePrecision::Month,
        _ => DatePrecision::Year,
    });

    match effective {
        DatePrecision::Day => match (month, day) {
            (Some(m), Some(d)) => Some(format_day(year, m, d, locale)),
            _ => Some(year.to_string()),
        },
        DatePrecision::Month => match month {
            Some(m) => Some(format_month(year, m, locale)),
            None => Some(year.to_string()),
        },
        DatePrecision::Year => Some(year.to_string()),
    }
}

fn month_short(month: u8, locale: Locale) -> &'static str {
    use crate::i18n::{Key, t};
    let key = match month {
        1 => Key::MonthJanShort,
        2 => Key::MonthFebShort,
        3 => Key::MonthMarShort,
        4 => Key::MonthAprShort,
        5 => Key::MonthMayShort,
        6 => Key::MonthJunShort,
        7 => Key::MonthJulShort,
        8 => Key::MonthAugShort,
        9 => Key::MonthSepShort,
        10 => Key::MonthOctShort,
        11 => Key::MonthNovShort,
        12 => Key::MonthDecShort,
        _ => return "",
    };
    t(locale, key)
}

fn format_day(year: u16, month: u8, day: u8, locale: Locale) -> String {
    let m = month_short(month, locale);
    match locale {
        Locale::Es => format!("{day} {m} {year}"),
        Locale::Ja => format!("{year}年{month}月{day}日"),
        _ => format!("{m} {day}, {year}"),
    }
}

fn format_month(year: u16, month: u8, locale: Locale) -> String {
    let m = month_short(month, locale);
    match locale {
        Locale::Ja => format!("{year}年{month}月"),
        _ => format!("{m} {year}"),
    }
}

/// Display-ready size information for a game detail page.
///
/// `storage` is the actual filesystem size. `rom_capacity` is the historical
/// cartridge/ROM-chip capacity, present only for systems where Mbit/Kbit is a
/// meaningful user-facing convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSizeDisplay {
    pub storage: String,
    pub rom_capacity: Option<String>,
}

/// Format a filesystem storage byte count as a human-readable decimal size
/// (KB / MB / GB). This is for storage accounting surfaces: system totals,
/// delete confirmations, downloaded files, and disk usage.
pub fn format_storage_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} KB", bytes / 1000)
    }
}

/// Format an elapsed-time duration for live "now playing" displays.
///
/// Shows "0m" during the first minute so a freshly launched game has a
/// visible timer immediately. Minute granularity is deliberate: 1 Hz text
/// updates reflow the page and iOS Safari cancels in-flight horizontal
/// momentum scrolls on any reflow, resetting `scrollLeft` to 0 on
/// neighboring `.scroll-card-row` rows.
pub fn format_elapsed_short(secs: u64) -> String {
    let minutes = secs / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if days > 0 {
        let hours_part = hours % 24;
        if hours_part == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {hours_part}h")
        }
    } else if hours > 0 {
        let mins_part = minutes % 60;
        if mins_part == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {mins_part}m")
        }
    } else {
        format!("{minutes}m")
    }
}

/// Format bytes as Megabit (Mbit) or Kilobit (Kbit) for cartridge/ROM-chip
/// capacity display.
///
/// - Under 1 Mbit (131,072 bytes): displays as Kbit (e.g., "256 Kbit")
/// - 1 Mbit and above: displays as Mbit, with one decimal place if not whole
///   (e.g., "16 Mbit", "4.5 Mbit")
/// - Under 128 bytes (1 Kbit): falls back to showing bytes
pub fn format_rom_capacity(bytes: u64) -> String {
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

/// Return historical ROM capacity for systems that conventionally expose ROM
/// chip sizes in Mbit/Kbit. Disc, disk, and computer systems return `None`.
pub fn rom_capacity_for_system(bytes: u64, system: &str) -> Option<String> {
    find_system_uses_megabit(system).then(|| format_rom_capacity(bytes))
}

/// Build the game-detail size display: storage is always shown, and ROM
/// capacity is shown in addition for cartridge/ROM-chip systems.
pub fn format_game_size(bytes: u64, system: &str) -> GameSizeDisplay {
    GameSizeDisplay {
        storage: format_storage_size(bytes),
        rom_capacity: rom_capacity_for_system(bytes, system),
    }
}

/// URL-safe base64 encoding (no padding, uses `-` and `_` instead of `+` and `/`).
///
/// Used for encoding ROM paths in URLs where the path may contain special characters.
pub fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

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
        let b = if chunk.len() > 1 {
            decode_char(chunk[1])? as u32
        } else {
            0
        };
        let c = if chunk.len() > 2 {
            decode_char(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 {
            decode_char(chunk[3])? as u32
        } else {
            0
        };
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

/// Like [`format_storage_size`], but returns number/unit parts and rounds GB
/// values to whole numbers.
///
/// Returns `(number_string, unit)` — e.g. `("12", "GB")` or `("5.5", "MB")`.
pub fn format_storage_size_short(bytes: u64) -> (String, &'static str) {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;

    if bytes >= GB {
        let gb = (bytes as f64 / GB as f64).round() as u64;
        (gb.to_string(), "GB")
    } else if bytes >= MB {
        (format!("{:.1}", bytes as f64 / MB as f64), "MB")
    } else {
        ((bytes / 1000).to_string(), "KB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_path_accepts_only_local_non_login_paths() {
        assert_eq!(
            sanitize_next_path(Some("/settings".to_string())),
            "/settings"
        );
        assert_eq!(
            sanitize_next_path(Some("/settings/wifi?from=login".to_string())),
            "/settings/wifi?from=login"
        );
        assert_eq!(sanitize_next_path(None), "/");
        assert_eq!(
            sanitize_next_path(Some("https://example.com".to_string())),
            "/"
        );
        assert_eq!(sanitize_next_path(Some("//example.com".to_string())), "/");
        assert_eq!(sanitize_next_path(Some("/login".to_string())), "/");
    }

    #[test]
    fn net_control_code_is_exactly_six_digits() {
        assert!(is_valid_net_control_code("123456"));
        assert!(!is_valid_net_control_code(""));
        assert!(!is_valid_net_control_code("12345"));
        assert!(!is_valid_net_control_code("1234567"));
        assert!(!is_valid_net_control_code("12345a"));
        assert!(!is_valid_net_control_code("１２３４５６"));
    }

    #[test]
    fn elapsed_shows_zero_minutes_immediately() {
        assert_eq!(format_elapsed_short(0), "0m");
        assert_eq!(format_elapsed_short(59), "0m");
    }

    #[test]
    fn elapsed_minutes_and_hours() {
        assert_eq!(format_elapsed_short(60), "1m");
        assert_eq!(format_elapsed_short(59 * 60), "59m");
        assert_eq!(format_elapsed_short(3600), "1h");
        assert_eq!(format_elapsed_short(3600 + 90), "1h 1m");
    }

    #[test]
    fn elapsed_days() {
        assert_eq!(format_elapsed_short(86400), "1d");
        assert_eq!(format_elapsed_short(86400 + 3600), "1d 1h");
        assert_eq!(format_elapsed_short(2 * 86400 + 3 * 3600), "2d 3h");
    }

    #[test]
    fn format_bytes_as_kb() {
        assert_eq!(format_storage_size(0), "0 KB");
        assert_eq!(format_storage_size(1000), "1 KB");
        assert_eq!(format_storage_size(512_000), "512 KB");
    }

    #[test]
    fn format_bytes_as_mb() {
        assert_eq!(format_storage_size(1_000_000), "1.0 MB");
        assert_eq!(format_storage_size(5_500_000), "5.5 MB");
    }

    #[test]
    fn format_bytes_as_gb() {
        assert_eq!(format_storage_size(1_000_000_000), "1.0 GB");
        assert_eq!(format_storage_size(2_500_000_000), "2.5 GB");
    }

    #[test]
    fn format_short_gb_rounds() {
        assert_eq!(
            format_storage_size_short(1_000_000_000),
            ("1".to_string(), "GB")
        );
        // 2.5 GB rounds to 3
        assert_eq!(
            format_storage_size_short(2_500_000_000),
            ("3".to_string(), "GB")
        );
        // ~12.3 GB rounds to 12
        assert_eq!(
            format_storage_size_short(12_300_000_000),
            ("12".to_string(), "GB")
        );
    }

    #[test]
    fn format_short_mb_keeps_decimals() {
        assert_eq!(
            format_storage_size_short(5_500_000),
            ("5.5".to_string(), "MB")
        );
    }

    #[test]
    fn format_short_kb() {
        assert_eq!(
            format_storage_size_short(512_000),
            ("512".to_string(), "KB")
        );
    }

    // --- Edge cases for format_storage_size ---

    #[test]
    fn format_zero_bytes() {
        assert_eq!(format_storage_size(0), "0 KB");
    }

    #[test]
    fn format_one_byte() {
        // Integer division: 1 / 1000 = 0
        assert_eq!(format_storage_size(1), "0 KB");
    }

    #[test]
    fn format_999_bytes() {
        // Just under 1 KB
        assert_eq!(format_storage_size(999), "0 KB");
    }

    #[test]
    fn format_mb_boundary() {
        // Exactly at the MB boundary
        assert_eq!(format_storage_size(1_000_000), "1.0 MB");
        // One byte below MB
        assert_eq!(format_storage_size(999_999), "999 KB");
    }

    #[test]
    fn format_gb_boundary() {
        // Exactly at the GB boundary
        assert_eq!(format_storage_size(1_000_000_000), "1.0 GB");
        // One byte below GB -- should be MB
        let below_gb = format_storage_size(999_999_999);
        assert!(
            below_gb.ends_with("MB"),
            "Just below GB should be MB, got {below_gb}"
        );
    }

    #[test]
    fn format_very_large_value() {
        // 1 TB
        assert_eq!(format_storage_size(1_000_000_000_000), "1000.0 GB");
    }

    #[test]
    fn format_u64_max() {
        // Should not panic on max value
        let result = format_storage_size(u64::MAX);
        assert!(
            result.ends_with("GB"),
            "u64::MAX should show as GB, got {result}"
        );
    }

    // --- Edge cases for format_size_short ---

    #[test]
    fn format_short_zero() {
        assert_eq!(format_storage_size_short(0), ("0".to_string(), "KB"));
    }

    #[test]
    fn format_short_one_byte() {
        assert_eq!(format_storage_size_short(1), ("0".to_string(), "KB"));
    }

    #[test]
    fn format_short_mb_boundary() {
        assert_eq!(
            format_storage_size_short(1_000_000),
            ("1.0".to_string(), "MB")
        );
    }

    #[test]
    fn format_short_gb_rounding_up() {
        // 1.5 GB should round to 2
        let bytes = 1_500_000_000;
        assert_eq!(format_storage_size_short(bytes), ("2".to_string(), "GB"));
    }

    #[test]
    fn format_short_gb_rounding_down() {
        // 1.4 GB should round to 1
        let bytes = 1_400_000_000;
        assert_eq!(format_storage_size_short(bytes), ("1".to_string(), "GB"));
    }

    // --- format_rom_capacity tests ---

    #[test]
    fn megabit_zero() {
        assert_eq!(format_rom_capacity(0), "0 bytes");
    }

    #[test]
    fn megabit_tiny_bytes() {
        // Under 128 bytes (1 Kbit) -> falls back to bytes
        assert_eq!(format_rom_capacity(64), "64 bytes");
        assert_eq!(format_rom_capacity(1), "1 bytes");
    }

    #[test]
    fn megabit_kbit_values() {
        // 2 KB = 16 Kbit (Atari 2600 ROM)
        assert_eq!(format_rom_capacity(2048), "16 Kbit");
        // 4 KB = 32 Kbit
        assert_eq!(format_rom_capacity(4096), "32 Kbit");
        // 32 KB = 256 Kbit
        assert_eq!(format_rom_capacity(32_768), "256 Kbit");
    }

    #[test]
    fn megabit_exact_mbit_values() {
        // 128 KB = 1 Mbit
        assert_eq!(format_rom_capacity(131_072), "1 Mbit");
        // 256 KB = 2 Mbit
        assert_eq!(format_rom_capacity(262_144), "2 Mbit");
        // 512 KB = 4 Mbit (classic SMS/GG)
        assert_eq!(format_rom_capacity(524_288), "4 Mbit");
        // 1 MB = 8 Mbit (Super Mario World on SNES)
        assert_eq!(format_rom_capacity(1_048_576), "8 Mbit");
        // 2 MB = 16 Mbit (Sonic 3)
        assert_eq!(format_rom_capacity(2_097_152), "16 Mbit");
        // 3 MB = 24 Mbit (Phantasy Star IV)
        assert_eq!(format_rom_capacity(3_145_728), "24 Mbit");
        // 4 MB = 32 Mbit (DKC on SNES)
        assert_eq!(format_rom_capacity(4_194_304), "32 Mbit");
        // 8 MB = 64 Mbit (Super Mario 64)
        assert_eq!(format_rom_capacity(8_388_608), "64 Mbit");
        // 32 MB = 256 Mbit (RE2 on N64)
        assert_eq!(format_rom_capacity(33_554_432), "256 Mbit");
        // 64 MB = 512 Mbit (Conker's Bad Fur Day)
        assert_eq!(format_rom_capacity(67_108_864), "512 Mbit");
    }

    #[test]
    fn megabit_whole_mbit_no_decimal() {
        // 768 KB = 6 Mbit (whole number)
        assert_eq!(format_rom_capacity(786_432), "6 Mbit");
        // 640 KB = 5 Mbit (whole number)
        assert_eq!(format_rom_capacity(655_360), "5 Mbit");
    }

    #[test]
    fn megabit_fractional_mbit() {
        // 192 KB = 1.5 Mbit
        assert_eq!(format_rom_capacity(196_608), "1.5 Mbit");
        // 576 KB = 4.5 Mbit
        assert_eq!(format_rom_capacity(589_824), "4.5 Mbit");
    }

    #[test]
    fn megabit_large_values() {
        // 86 MB = 688 Mbit (largest Neo Geo carts)
        assert_eq!(format_rom_capacity(90_177_536), "688 Mbit");
    }

    // --- game detail size tests ---

    #[test]
    fn game_size_for_megabit_system_shows_storage_and_capacity() {
        let size = format_game_size(1_048_576, "nintendo_snes");
        assert_eq!(size.storage, "1.0 MB");
        assert_eq!(size.rom_capacity.as_deref(), Some("8 Mbit"));
    }

    #[test]
    fn game_size_for_disc_system_shows_storage_only() {
        let size = format_game_size(524_288_000, "sony_psx");
        assert_eq!(size.storage, "524.3 MB");
        assert_eq!(size.rom_capacity, None);
    }

    #[test]
    fn game_size_for_ds_uses_storage_only() {
        let size = format_game_size(33_554_432, "nintendo_ds");
        assert_eq!(size.storage, "33.6 MB");
        assert_eq!(size.rom_capacity, None);
    }

    #[test]
    fn game_size_for_arcade_dc_uses_storage_only() {
        let size = format_game_size(524_288_000, "arcade_dc");
        assert_eq!(size.storage, "524.3 MB");
        assert_eq!(size.rom_capacity, None);
    }

    #[test]
    fn game_size_for_unknown_uses_storage_only() {
        let size = format_game_size(1_048_576, "unknown_system");
        assert_eq!(size.storage, "1.0 MB");
        assert_eq!(size.rom_capacity, None);
    }

    // --- format_release_date tests ---

    #[test]
    fn release_date_year_only_en() {
        assert_eq!(
            format_release_date("1991", Some(DatePrecision::Year), Locale::En),
            Some("1991".to_string())
        );
    }

    #[test]
    fn release_date_month_en() {
        assert_eq!(
            format_release_date("1991-08", Some(DatePrecision::Month), Locale::En),
            Some("Aug 1991".to_string())
        );
    }

    #[test]
    fn release_date_day_en() {
        assert_eq!(
            format_release_date("1991-08-23", Some(DatePrecision::Day), Locale::En),
            Some("Aug 23, 1991".to_string())
        );
    }

    #[test]
    fn release_date_year_only_es() {
        assert_eq!(
            format_release_date("1991", Some(DatePrecision::Year), Locale::Es),
            Some("1991".to_string())
        );
    }

    #[test]
    fn release_date_month_es() {
        assert_eq!(
            format_release_date("1991-08", Some(DatePrecision::Month), Locale::Es),
            Some("ago 1991".to_string())
        );
    }

    #[test]
    fn release_date_day_es() {
        assert_eq!(
            format_release_date("1991-08-23", Some(DatePrecision::Day), Locale::Es),
            Some("23 ago 1991".to_string())
        );
    }

    #[test]
    fn release_date_year_only_ja() {
        assert_eq!(
            format_release_date("1991", Some(DatePrecision::Year), Locale::Ja),
            Some("1991".to_string())
        );
    }

    #[test]
    fn release_date_month_ja() {
        assert_eq!(
            format_release_date("1991-08", Some(DatePrecision::Month), Locale::Ja),
            Some("1991年8月".to_string())
        );
    }

    #[test]
    fn release_date_day_ja() {
        assert_eq!(
            format_release_date("1991-08-23", Some(DatePrecision::Day), Locale::Ja),
            Some("1991年8月23日".to_string())
        );
    }

    #[test]
    fn release_date_invalid_returns_none() {
        assert_eq!(
            format_release_date("not-a-date", Some(DatePrecision::Day), Locale::En),
            None
        );
    }

    #[test]
    fn release_date_infers_precision_from_length() {
        // Missing precision, infer from date string.
        assert_eq!(
            format_release_date("1991", None, Locale::En),
            Some("1991".to_string())
        );
        assert_eq!(
            format_release_date("1991-08", None, Locale::En),
            Some("Aug 1991".to_string())
        );
        assert_eq!(
            format_release_date("1991-08-23", None, Locale::En),
            Some("Aug 23, 1991".to_string())
        );
    }

    #[test]
    fn release_date_day_precision_with_only_year_falls_back() {
        // When caller claims "day" but the date is year-only, emit the year.
        assert_eq!(
            format_release_date("1991", Some(DatePrecision::Day), Locale::En),
            Some("1991".to_string())
        );
    }
}
