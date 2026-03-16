//! Title normalization and series key extraction utilities.
//!
//! Provides functions for computing normalized base titles (for dedup)
//! and series keys (for franchise grouping).

/// Strip parenthesized tags and trailing whitespace from a name for fuzzy matching.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` -> `"Indiana Jones and the Fate of Atlantis"`
/// `"Dark Seed"` -> `"Dark Seed"` (unchanged)
pub fn strip_tags(name: &str) -> &str {
    name.find(" (")
        .or_else(|| name.find(" ["))
        .map(|i| &name[..i])
        .unwrap_or(name)
        .trim()
}

/// Strip GDI/TOSEC version strings from a name for fuzzy matching.
/// `"Sonic Adventure 2 v1.008"` -> `"Sonic Adventure 2"`
/// `"Sega Rally 2 v1 001"` -> `"Sega Rally 2"`
/// Returns the original string if no version pattern is found.
pub fn strip_version(name: &str) -> &str {
    // Look for " v" followed by a digit, then optional digits/dots/spaces/underscores
    let bytes = name.as_bytes();
    let mut i = 0;
    let mut last_version_start = None;
    while i + 2 < bytes.len() {
        if bytes[i] == b' '
            && bytes[i + 1] == b'v'
            && bytes.get(i + 2).is_some_and(|b| b.is_ascii_digit())
        {
            // Check that everything after " v\d" is digits, dots, spaces, or underscores
            let rest = &bytes[i + 2..];
            if rest
                .iter()
                .all(|b| b.is_ascii_digit() || *b == b'.' || *b == b' ' || *b == b'_')
            {
                last_version_start = Some(i);
            }
        }
        i += 1;
    }
    match last_version_start {
        Some(pos) => name[..pos].trim(),
        None => name,
    }
}

/// Compute a lowercased base title for fuzzy image matching and dedup.
///
/// Handles tilde dual-names (`"Name1 ~ Name2"` -> `"Name2"`), strips
/// parenthesized/bracketed tags, lowercases the result, and normalizes
/// trailing articles (`", The"` / `", A"` / `", An"`) to the front.
pub fn base_title(name: &str) -> String {
    let s = name.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(name);
    let lower = strip_tags(s).to_lowercase();
    for article in &[", the", ", an", ", a"] {
        if let Some(title) = lower.strip_suffix(article) {
            let art = &article[2..]; // skip ", "
            return format!("{art} {title}");
        }
    }
    lower
}

/// Roman numeral values for series key extraction.
const ROMAN_NUMERALS: &[(&str, u32)] = &[
    ("xviii", 18),
    ("xvii", 17),
    ("xvi", 16),
    ("xiii", 13),
    ("xiv", 14),
    ("xv", 15),
    ("xii", 12),
    ("xi", 11),
    ("viii", 8),
    ("vii", 7),
    ("vi", 6),
    ("iv", 4),
    ("ix", 9),
    ("iii", 3),
    ("ii", 2),
    ("x", 10),
    ("v", 5),
];

/// Extract a series key by stripping trailing numbers, roman numerals,
/// and colon+subtitle from a base title.
///
/// Used for algorithmic franchise grouping: games with the same `series_key`
/// but different `base_title` are likely in the same series.
///
/// Returns an empty string if the result equals the original base_title
/// or is too short (< 4 chars), indicating no series could be extracted.
///
/// # Examples
/// ```
/// use replay_control_core::title_utils::series_key;
/// assert_eq!(series_key("streets of rage 2"), "streets of rage");
/// assert_eq!(series_key("final fantasy vi"), "final fantasy");
/// assert_eq!(series_key("mega man 5"), "mega man");
/// assert_eq!(series_key("sonic the hedgehog"), ""); // no number to strip
/// assert_eq!(series_key("the legend of zelda: a link to the past"), "the legend of zelda");
/// ```
pub fn series_key(base_title: &str) -> String {
    let trimmed = base_title.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut result = trimmed.to_string();

    // 1. Strip trailing colon+subtitle: "title: subtitle" -> "title"
    //    Also handle " - subtitle" pattern
    if let Some(colon_pos) = result.find(": ") {
        result = result[..colon_pos].trim().to_string();
    } else if let Some(dash_pos) = result.find(" - ") {
        // Only strip if the part before the dash is long enough
        let before = result[..dash_pos].trim();
        if before.len() >= 4 {
            result = before.to_string();
        }
    }

    // 2. Strip trailing roman numerals
    let lower = result.to_lowercase();
    for &(numeral, _) in ROMAN_NUMERALS {
        let suffix = format!(" {numeral}");
        if lower.ends_with(&suffix) {
            let end = result.len() - suffix.len();
            result = result[..end].trim().to_string();
            break;
        }
    }

    // 3. Strip trailing arabic numerals: " 2", " 10", " 64", etc.
    let lower = result.to_lowercase();
    if let Some(last_space) = lower.rfind(' ') {
        let after = &lower[last_space + 1..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
            result = result[..last_space].trim().to_string();
        }
    }

    // 4. Normalize: lowercase, strip non-alphanumeric except spaces
    let normalized: String = result
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // 5. Return empty if result equals original normalized or is too short
    let original_normalized: String = trimmed
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized == original_normalized || normalized.len() < 4 {
        return String::new();
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- base_title ---

    #[test]
    fn base_title_reorders_trailing_the() {
        assert_eq!(base_title("Legend of Zelda, The"), "the legend of zelda");
    }

    #[test]
    fn base_title_no_article_unchanged() {
        assert_eq!(base_title("Super Mario World"), "super mario world");
    }

    #[test]
    fn base_title_strips_tags() {
        assert_eq!(base_title("Game Name (USA)"), "game name");
    }

    // --- strip_tags ---

    #[test]
    fn strip_tags_removes_parenthesized() {
        assert_eq!(strip_tags("Game Name (USA)"), "Game Name");
    }

    #[test]
    fn strip_tags_no_tags() {
        assert_eq!(strip_tags("Dark Seed"), "Dark Seed");
    }

    // --- strip_version ---

    #[test]
    fn strip_version_standard() {
        assert_eq!(
            strip_version("Sonic Adventure 2 v1.008"),
            "Sonic Adventure 2"
        );
    }

    #[test]
    fn strip_version_no_version() {
        assert_eq!(strip_version("Super Mario World"), "Super Mario World");
    }

    // --- series_key ---

    #[test]
    fn series_key_arabic_numeral() {
        assert_eq!(series_key("streets of rage 2"), "streets of rage");
        assert_eq!(series_key("mega man 5"), "mega man");
        assert_eq!(series_key("sonic the hedgehog 3"), "sonic the hedgehog");
    }

    #[test]
    fn series_key_roman_numeral() {
        assert_eq!(series_key("final fantasy vi"), "final fantasy");
        assert_eq!(series_key("mega man ii"), "mega man");
        assert_eq!(series_key("castlevania iii"), "castlevania");
    }

    #[test]
    fn series_key_colon_subtitle() {
        assert_eq!(
            series_key("the legend of zelda: a link to the past"),
            "the legend of zelda"
        );
    }

    #[test]
    fn series_key_dash_subtitle() {
        // "mega man x - maverick hunter" -> strip dash -> "mega man x"
        // -> strip roman numeral "x" -> "mega man"
        assert_eq!(
            series_key("mega man x - maverick hunter"),
            "mega man"
        );
    }

    #[test]
    fn series_key_no_series() {
        // No number to strip => equals original => empty
        assert_eq!(series_key("sonic the hedgehog"), "");
        assert_eq!(series_key("contra"), "");
        assert_eq!(series_key("tetris"), "");
    }

    #[test]
    fn series_key_too_short() {
        // Result would be too short (< 4 chars)
        assert_eq!(series_key("r 2"), "");
    }

    #[test]
    fn series_key_empty_input() {
        assert_eq!(series_key(""), "");
    }

    #[test]
    fn series_key_numeral_and_subtitle() {
        // "Street Fighter II: The World Warrior" should get series "street fighter"
        assert_eq!(
            series_key("street fighter ii: the world warrior"),
            "street fighter"
        );
    }

    #[test]
    fn series_key_trailing_64() {
        // "super mario 64" -> "super mario"
        assert_eq!(series_key("super mario 64"), "super mario");
    }
}
