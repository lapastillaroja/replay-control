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
/// Strips parenthesized/bracketed tags first, then handles tilde dual-names
/// (`"Name1 ~ Name2"` -> `"Name2"`), lowercases, and normalizes trailing
/// articles (`", The"` / `", A"` / `", An"`) to the front.
///
/// Tags are stripped before tilde splitting because ` ~ ` can appear inside
/// parenthesized content (e.g., Neo Geo `(NGM-055 ~ NGH-055)`).
pub fn base_title(name: &str) -> String {
    let stripped = strip_tags(name);
    let stripped = strip_version(stripped);
    let s = stripped
        .rsplit_once(" ~ ")
        .map(|(_, r)| r)
        .unwrap_or(stripped);
    let lower = s.to_lowercase();
    for article in &[", the", ", an", ", a"] {
        if let Some(title) = lower.strip_suffix(article) {
            let art = &article[2..]; // skip ", "
            return format!("{art} {title}");
        }
    }
    lower
}

/// Aggressively normalize a title by stripping all punctuation and collapsing spaces.
///
/// Strips `' : - _ . , ! ? &` and any other non-alphanumeric, non-space characters,
/// normalizes multiple spaces to single, trims, and lowercases. This is the most
/// aggressive matching tier — used only as a last resort when all other tiers fail.
///
/// Example: `"Shin Megami Tensei: Devil Children - Black Book"` → `"shin megami tensei devil children black book"`
pub fn normalize_aggressive(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            out.push(ch);
        } else {
            // Replace punctuation with space (so "X-Men" becomes "X Men" not "XMen")
            out.push(' ');
        }
    }
    // Collapse multiple spaces, trim, lowercase.
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
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

/// Produce a fuzzy matching key by stripping all non-alphanumeric characters.
///
/// Used to bridge naming differences between data sources:
/// `"Bare Knuckle: Ikari no Tekken"` (TGDB, colon) and
/// `"Bare Knuckle - Ikari no Tekken"` (No-Intro, dash) both produce
/// `"bare knuckle ikari no tekken"`.
///
/// This is NOT used as `base_title` — only for matching external names
/// to library entries when exact `base_title` comparison fails.
pub fn fuzzy_match_key(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(' ');
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Resolve an external name (from TGDB, LaunchBox, etc.) to the library's
/// actual `base_title`, handling colon/dash/punctuation differences.
///
/// Returns the library's `base_title` if found (exact or fuzzy), or the
/// normalized external name if no library match exists.
///
/// `library_exact` should contain all `base_title` values in the library.
/// `library_fuzzy` maps `fuzzy_match_key(base_title)` → `base_title`.
pub fn resolve_to_library_title(
    external_name: &str,
    library_exact: &std::collections::HashSet<&str>,
    library_fuzzy: &std::collections::HashMap<String, &str>,
) -> String {
    let normalized = base_title(external_name);
    if library_exact.contains(normalized.as_str()) {
        return normalized;
    }
    if let Some(&lib_bt) = library_fuzzy.get(&fuzzy_match_key(&normalized)) {
        return lib_bt.to_string();
    }
    normalized
}

/// Normalize a title for matching against Wikidata entries.
///
/// Mirrors the `normalize_title_for_wikidata()` function used at build time:
/// lowercase, strip non-alphanumeric except spaces, collapse whitespace.
///
/// Unlike [`fuzzy_match_key`], this **drops** non-alphanumeric characters
/// rather than replacing them with spaces (so `"Pac-Man"` -> `"pacman"`,
/// not `"pac man"`). This must match the build-time normalization exactly.
pub fn normalize_for_wikidata(title: &str) -> String {
    let trimmed = title.trim();
    let mut result = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

/// Convert a trailing roman numeral to arabic in a normalized title.
///
/// Returns `Some(converted)` if the last word is a roman numeral, `None` otherwise.
/// Input should already be lowercase (e.g., from `normalize_for_wikidata`).
///
/// Example: `"streets of rage ii"` → `Some("streets of rage 2")`
pub fn roman_to_arabic_suffix(normalized: &str) -> Option<String> {
    let last_space = normalized.rfind(' ')?;
    let last_word = &normalized[last_space + 1..];
    for &(numeral, value) in ROMAN_NUMERALS {
        if last_word == numeral {
            return Some(format!("{} {value}", &normalized[..last_space]));
        }
    }
    None
}

/// Strip the `"N64DD - "` prefix from a ROM filename stem.
///
/// N64DD ROMs use this prefix in their filenames, but the thumbnail repos
/// and display name systems do not include it.
pub fn strip_n64dd_prefix(stem: &str) -> &str {
    stem.strip_prefix("N64DD - ").unwrap_or(stem)
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
    fn base_title_tilde_inside_parens() {
        // Neo Geo games have ` ~ ` inside parens: (NGM-055 ~ NGH-055)
        // Tags should be stripped BEFORE tilde splitting.
        assert_eq!(
            base_title("The King of Fighters '94 (NGM-055 ~ NGH-055)"),
            "the king of fighters '94"
        );
    }

    #[test]
    fn base_title_tilde_dual_name() {
        // Tilde between two game names: takes the right half.
        assert_eq!(
            base_title("Bare Knuckle ~ Streets of Rage"),
            "streets of rage"
        );
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
        assert_eq!(series_key("mega man x - maverick hunter"), "mega man");
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

    // --- fuzzy_match_key ---

    #[test]
    fn fuzzy_key_colon_vs_dash() {
        assert_eq!(
            fuzzy_match_key("bare knuckle: ikari no tekken"),
            fuzzy_match_key("bare knuckle - ikari no tekken")
        );
    }

    #[test]
    fn fuzzy_key_preserves_hyphenated_words() {
        // X-Men becomes "x men" — both colon and dash versions match
        assert_eq!(
            fuzzy_match_key("x-men vs street fighter"),
            "x men vs street fighter"
        );
    }

    #[test]
    fn fuzzy_key_strips_punctuation() {
        assert_eq!(
            fuzzy_match_key("Teenage Mutant Ninja Turtles: The Hyperstone Heist"),
            "teenage mutant ninja turtles the hyperstone heist"
        );
    }

    #[test]
    fn fuzzy_key_collapses_whitespace() {
        assert_eq!(fuzzy_match_key("  hello   world  "), "hello world");
    }

    #[test]
    fn fuzzy_key_empty() {
        assert_eq!(fuzzy_match_key(""), "");
    }

    // --- resolve_to_library_title ---

    #[test]
    fn resolve_exact_match() {
        let exact: std::collections::HashSet<&str> = ["streets of rage", "sonic the hedgehog"]
            .into_iter()
            .collect();
        let fuzzy = std::collections::HashMap::new();

        assert_eq!(
            resolve_to_library_title("Streets of Rage (USA)", &exact, &fuzzy),
            "streets of rage"
        );
    }

    #[test]
    fn resolve_fuzzy_colon_to_dash() {
        let exact: std::collections::HashSet<&str> =
            ["teenage mutant ninja turtles - the hyperstone heist"]
                .into_iter()
                .collect();
        let mut fuzzy = std::collections::HashMap::new();
        fuzzy.insert(
            fuzzy_match_key("teenage mutant ninja turtles - the hyperstone heist"),
            "teenage mutant ninja turtles - the hyperstone heist",
        );

        // LaunchBox uses colon, library uses dash
        assert_eq!(
            resolve_to_library_title(
                "Teenage Mutant Ninja Turtles: The Hyperstone Heist",
                &exact,
                &fuzzy
            ),
            "teenage mutant ninja turtles - the hyperstone heist"
        );
    }

    #[test]
    fn resolve_no_match_returns_normalized() {
        let exact = std::collections::HashSet::new();
        let fuzzy = std::collections::HashMap::new();

        assert_eq!(
            resolve_to_library_title("Unknown Game (Japan)", &exact, &fuzzy),
            "unknown game"
        );
    }

    #[test]
    fn resolve_bare_knuckle_colon_dash() {
        let exact: std::collections::HashSet<&str> =
            ["bare knuckle - ikari no tekken"].into_iter().collect();
        let mut fuzzy = std::collections::HashMap::new();
        fuzzy.insert(
            fuzzy_match_key("bare knuckle - ikari no tekken"),
            "bare knuckle - ikari no tekken",
        );

        assert_eq!(
            resolve_to_library_title("Bare Knuckle: Ikari no Tekken", &exact, &fuzzy),
            "bare knuckle - ikari no tekken"
        );
    }

    // --- normalize_aggressive ---

    #[test]
    fn normalize_aggressive_strips_punctuation() {
        assert_eq!(
            normalize_aggressive("Ghouls 'N Ghosts"),
            "ghouls n ghosts"
        );
        assert_eq!(
            normalize_aggressive("E.V.O. Search for Eden"),
            "e v o search for eden"
        );
        assert_eq!(
            normalize_aggressive("Brett Hull Hockey '95"),
            "brett hull hockey 95"
        );
    }

    #[test]
    fn normalize_aggressive_preserves_words() {
        assert_eq!(
            normalize_aggressive("Bio-Hazard Battle"),
            "bio hazard battle"
        );
        assert_eq!(normalize_aggressive("Clever & Smart"), "clever smart");
    }

    #[test]
    fn normalize_aggressive_different_games_dont_collide() {
        // These should produce DIFFERENT keys — the aggressive tier uses
        // exact HashMap equality so they won't collide, but verify the
        // normalized forms are indeed distinct.
        assert_ne!(
            normalize_aggressive("Battletoads"),
            normalize_aggressive("Battletoads & Double Dragon")
        );
        assert_ne!(
            normalize_aggressive("Spider-Man"),
            normalize_aggressive("Spider-Man & Venom - Maximum Carnage")
        );
    }

    // --- normalize_for_wikidata ---

    #[test]
    fn wikidata_normalize_drops_punctuation() {
        // Non-alphanumeric chars are dropped (not replaced with space)
        assert_eq!(normalize_for_wikidata("Pac-Man"), "pacman");
    }

    #[test]
    fn wikidata_normalize_preserves_spaces() {
        assert_eq!(
            normalize_for_wikidata("Super Mario World"),
            "super mario world"
        );
    }

    #[test]
    fn wikidata_normalize_collapses_whitespace() {
        assert_eq!(normalize_for_wikidata("  hello   world  "), "hello world");
    }

    // --- strip_n64dd_prefix ---

    #[test]
    fn strip_n64dd_prefix_removes_prefix() {
        assert_eq!(
            strip_n64dd_prefix("N64DD - Mario Artist Paint Studio"),
            "Mario Artist Paint Studio"
        );
    }

    #[test]
    fn strip_n64dd_prefix_no_prefix_unchanged() {
        assert_eq!(strip_n64dd_prefix("Super Mario 64"), "Super Mario 64");
    }
}
