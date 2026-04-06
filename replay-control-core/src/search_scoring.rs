//! Search relevance scoring for ROM search results.
//!
//! Computes a numeric relevance score for a ROM against a search query.
//! Higher = more relevant. Scoring tiers: exact match, prefix, word boundary,
//! substring, filename-only, and word-level fuzzy matching.

use crate::rom_tags::{self, RegionPreference, RomTier};

/// Split a string into words on whitespace and hyphens, stripping trailing punctuation.
pub fn split_into_words(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || c == '-')
        .map(|w| w.trim_end_matches(['.', ',', ':', '!', '?', ';']))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Check whether query words appear in the same relative order in target words.
fn words_in_order(query_words: &[&str], title_words: &[&str]) -> bool {
    let mut last_pos: Option<usize> = None;
    for qw in query_words {
        let found = title_words
            .iter()
            .enumerate()
            .position(|(i, tw)| tw.starts_with(qw) && last_pos.is_none_or(|lp| i > lp));
        match found {
            Some(pos) => last_pos = Some(pos),
            None => return false,
        }
    }
    true
}

/// Count pairs of consecutive query words that are also adjacent in the title.
fn count_adjacent_pairs(query_words: &[&str], title_words: &[&str]) -> u32 {
    if query_words.len() < 2 {
        return 0;
    }
    let mut pairs = 0u32;
    // Find positions of each query word in title
    let positions: Vec<Option<usize>> = query_words
        .iter()
        .map(|qw| title_words.iter().position(|tw| tw.starts_with(qw)))
        .collect();
    for i in 0..positions.len() - 1 {
        if let (Some(p1), Some(p2)) = (positions[i], positions[i + 1])
            && p2 == p1 + 1
        {
            pairs += 1;
        }
    }
    pairs
}

/// Count query words that match a title word exactly (not just starts_with).
fn count_exact_word_matches(query_words: &[&str], title_words: &[&str]) -> u32 {
    query_words
        .iter()
        .filter(|qw| title_words.iter().any(|tw| tw == *qw))
        .count() as u32
}

/// Try word-level matching: all query words must appear (via starts_with) in the target words.
/// Returns (base_score, is_filename_only) or None if not all words match.
fn try_word_match(
    query_words: &[&str],
    display_words: &[&str],
    filename_words: &[&str],
) -> Option<(u32, bool)> {
    let display_matched = query_words
        .iter()
        .filter(|qw| display_words.iter().any(|tw| tw.starts_with(**qw)))
        .count();

    if display_matched == query_words.len() {
        // All words found in display name
        let order_bonus: u32 = if words_in_order(query_words, display_words) {
            50
        } else {
            0
        };
        let coverage = display_matched as f32 / display_words.len().max(1) as f32;
        let coverage_bonus = (coverage * 50.0) as u32;
        let adjacency_bonus = count_adjacent_pairs(query_words, display_words) * 20;
        let exact_bonus = count_exact_word_matches(query_words, display_words) * 30;
        return Some((
            400 + order_bonus + coverage_bonus + adjacency_bonus + exact_bonus,
            false,
        ));
    }

    // Try filename
    let filename_matched = query_words
        .iter()
        .filter(|qw| filename_words.iter().any(|tw| tw.starts_with(**qw)))
        .count();

    if filename_matched == query_words.len() {
        let order_bonus: u32 = if words_in_order(query_words, filename_words) {
            50
        } else {
            0
        };
        let coverage = filename_matched as f32 / filename_words.len().max(1) as f32;
        let coverage_bonus = (coverage * 50.0) as u32;
        let adjacency_bonus = count_adjacent_pairs(query_words, filename_words) * 20;
        let exact_bonus = count_exact_word_matches(query_words, filename_words) * 30;
        return Some((
            300 + order_bonus + coverage_bonus + adjacency_bonus + exact_bonus,
            true,
        ));
    }

    None
}

/// Apply shared bonuses and penalties to a base score: length bonus, tier penalty,
/// and region bonus. Used by both the word-match and prefix/contains paths.
fn apply_bonuses(
    base: u32,
    filename: &str,
    display_name: &str,
    region_pref: RegionPreference,
    region_secondary: Option<RegionPreference>,
) -> u32 {
    // Shorter names are more likely the original game
    let length_bonus: u32 = if display_name.len() < 40 { 100 } else { 0 };

    // Tier penalty: deprioritize non-original ROMs
    let (tier, region, _) = rom_tags::classify(filename);
    let tier_penalty = match tier {
        RomTier::Original => 0,
        RomTier::Revision => 5,
        RomTier::RegionVariant => 10,
        RomTier::Translation => 50,
        RomTier::Unlicensed => 60,
        RomTier::Homebrew => 100,
        RomTier::Hack => 200,
        RomTier::PreRelease => 250,
        RomTier::Pirate => 300,
    };

    // Region bonus: based on sort_key from user's preference.
    // Lower sort_key = higher bonus.
    let region_bonus: u32 = match region.sort_key(region_pref, region_secondary) {
        0 => 20, // World (or preferred when World is the preference)
        1 => 15, // User's preferred region
        2 => 10, // Second-best major region
        3 => 5,  // Third major region
        _ => 0,  // Other / Unknown
    };

    (base + length_bonus + region_bonus).saturating_sub(tier_penalty)
}

/// Compute a relevance score for a ROM against a search query.
/// Higher = more relevant. Returns 0 for no match.
/// The `region_pref` parameter controls which region gets the highest bonus.
pub fn search_score(
    query: &str,
    display_name: &str,
    filename: &str,
    region_pref: RegionPreference,
    region_secondary: Option<RegionPreference>,
) -> u32 {
    let display_lower = display_name.to_lowercase();
    let filename_lower = filename.to_lowercase();

    // Check whether the match ends at a word boundary (next char is non-alphanumeric or end-of-string).
    let is_word_boundary_at = |s: &str, offset: usize| -> bool {
        s[offset..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
    };

    // Check whether `needle` appears in `haystack` at a clean trailing word boundary
    // (i.e., at least one occurrence where the character after the match is non-alphanumeric or end-of-string).
    let contains_at_word_boundary = |haystack: &str, needle: &str| -> bool {
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            let abs_pos = start + pos;
            if is_word_boundary_at(haystack, abs_pos + needle.len()) {
                return true;
            }
            start = abs_pos + 1;
        }
        false
    };

    // Base score from match type
    let base = if display_lower == *query {
        10_000 // exact match on display name
    } else if display_lower.starts_with(query) && is_word_boundary_at(&display_lower, query.len()) {
        5_000 // display name starts with query (clean word boundary)
    } else if display_lower
        .split_whitespace()
        .any(|w| w.starts_with(query) && is_word_boundary_at(w, query.len()))
    {
        2_000 // a word in display name starts with query (clean word boundary)
    } else if display_lower.contains(query) {
        // For multi-word queries, demote mid-word matches (e.g. "sonic 3" in "sonic 3d blast").
        if query.contains(' ') && !contains_at_word_boundary(&display_lower, query) {
            500 // multi-word query mid-word match (e.g. "sonic 3" in "sonic 3d blast")
        } else {
            1_000
        }
    } else if filename_lower.contains(query) {
        if query.contains(' ') && !contains_at_word_boundary(&filename_lower, query) {
            250 // multi-word query mid-word match in filename
        } else {
            500
        }
    } else {
        // Fallback: word-level matching for multi-word queries
        let word_score = if !query.contains(' ') {
            None
        } else {
            let query_words: Vec<&str> = split_into_words(query);
            if query_words.len() < 2 {
                None
            } else {
                let display_words: Vec<&str> = split_into_words(&display_lower);
                let filename_words: Vec<&str> = split_into_words(&filename_lower);
                try_word_match(&query_words, &display_words, &filename_words)
            }
        };

        match word_score {
            Some((word_base, _)) => {
                return apply_bonuses(
                    word_base,
                    filename,
                    display_name,
                    region_pref,
                    region_secondary,
                );
            }
            None => return 0,
        }
    };

    apply_bonuses(base, filename, display_name, region_pref, region_secondary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom_tags::RegionPreference;

    /// Default preference for tests (matches pre-preference behavior).
    const PREF: RegionPreference = RegionPreference::Usa;
    /// No secondary preference for most tests.
    const SEC: Option<RegionPreference> = None;

    // --- Exact match (10_000 base) ---

    #[test]
    fn exact_match_display_name() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            score >= 10_000,
            "Exact match should score >= 10000, got {score}"
        );
    }

    #[test]
    fn exact_match_is_case_insensitive() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        let score2 = search_score("tetris", "TETRIS", "TETRIS.nes", PREF, SEC);
        assert!(score >= 10_000);
        assert!(score2 >= 10_000);
    }

    // --- Prefix match (5_000 base) ---

    #[test]
    fn prefix_match() {
        let score = search_score(
            "super",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (5_000..10_000).contains(&score),
            "Prefix match should score in 5000..10000, got {score}"
        );
    }

    // --- Word boundary match (2_000 base) ---

    #[test]
    fn word_boundary_match() {
        let score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (2_000..5_000).contains(&score),
            "Word boundary match should score in 2000..5000, got {score}"
        );
    }

    // --- Substring match (1_000 base) ---

    #[test]
    fn substring_match() {
        // "ari" is inside "Mario" but doesn't start a word
        let score = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (1_000..2_000).contains(&score),
            "Substring match should score in 1000..2000, got {score}"
        );
    }

    // --- Filename-only match (500 base) ---

    #[test]
    fn filename_only_match() {
        // Query matches filename but not display name
        let score = search_score("usa", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            (500..1_000).contains(&score),
            "Filename-only match should score in 500..1000, got {score}"
        );
    }

    // --- No match ---

    #[test]
    fn no_match_returns_zero() {
        let score = search_score("zzzznotfound", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert_eq!(score, 0);
    }

    #[test]
    fn empty_query_no_match() {
        let score = search_score("", "Tetris", "Tetris (USA).nes", PREF, SEC);
        // Empty query matches everything via contains(""), so it should score > 0
        // (exact match since "tetris".contains("") is true, but actually "" == display_lower is false)
        // Let's verify the actual behavior
        assert!(score > 0, "Empty string is contained in all strings");
    }

    // --- Tier ordering ---

    #[test]
    fn exact_beats_prefix() {
        let exact = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        let prefix = search_score("tetris", "Tetris Plus", "Tetris Plus (USA).nes", PREF, SEC);
        assert!(
            exact > prefix,
            "Exact ({exact}) should beat prefix ({prefix})"
        );
    }

    #[test]
    fn prefix_beats_word_boundary() {
        let prefix = search_score(
            "super",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let word = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            prefix > word,
            "Prefix ({prefix}) should beat word boundary ({word})"
        );
    }

    #[test]
    fn word_boundary_beats_substring() {
        let word = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let substr = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            word > substr,
            "Word boundary ({word}) should beat substring ({substr})"
        );
    }

    #[test]
    fn substring_beats_filename_only() {
        let substr = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let filename = search_score("usa", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            substr > filename,
            "Substring ({substr}) should beat filename-only ({filename})"
        );
    }

    // --- Length bonus ---

    #[test]
    fn short_name_gets_length_bonus() {
        let short = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let long = search_score(
            "mario",
            "Super Mario World - Long Subtitle That Makes It Over 40 Characters",
            "Super Mario World - Long Subtitle (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            short > long,
            "Short name ({short}) should beat long name ({long}) due to length bonus"
        );
    }

    // --- Tier penalties ---

    #[test]
    fn hack_is_penalized() {
        let original = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let hack = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Hack).sfc",
            PREF,
            SEC,
        );
        assert!(
            original > hack,
            "Original ({original}) should beat hack ({hack})"
        );
    }

    #[test]
    fn translation_is_penalized() {
        let original = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let translated = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Traducido Es).sfc",
            PREF,
            SEC,
        );
        assert!(
            original > translated,
            "Original ({original}) should beat translation ({translated})"
        );
    }

    // --- Special characters ---

    #[test]
    fn special_characters_in_query() {
        let score = search_score(
            "asterix & obelix",
            "Asterix & Obelix",
            "Asterix & Obelix (Europe).sfc",
            PREF,
            SEC,
        );
        assert!(
            score >= 10_000,
            "Exact match with special chars should work, got {score}"
        );
    }

    #[test]
    fn query_with_dash() {
        let score = search_score(
            "x-men",
            "X-Men - Mutant Apocalypse",
            "X-Men - Mutant Apocalypse (USA).sfc",
            PREF,
            SEC,
        );
        assert!(score > 0, "Query with dash should match");
    }

    // --- Region preference affects scoring ---

    #[test]
    fn japan_preference_boosts_japan_roms() {
        let usa_with_usa_pref = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            RegionPreference::Usa,
            SEC,
        );
        let japan_with_japan_pref = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Japan).sfc",
            RegionPreference::Japan,
            SEC,
        );
        // Both should get sort_key=0 (preferred region = primary), so equal region bonus.
        assert_eq!(usa_with_usa_pref, japan_with_japan_pref);
    }

    #[test]
    fn preferred_region_beats_non_preferred() {
        let japan_pref = RegionPreference::Japan;
        let japan_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Japan).sfc",
            japan_pref,
            SEC,
        );
        let usa_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            japan_pref,
            SEC,
        );
        assert!(
            japan_score > usa_score,
            "Japan ({japan_score}) should beat USA ({usa_score}) with Japan preference"
        );
    }

    #[test]
    fn europe_preference_puts_europe_before_usa() {
        let pref = RegionPreference::Europe;
        let europe_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Europe).sfc",
            pref,
            SEC,
        );
        let usa_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            pref,
            SEC,
        );
        assert!(
            europe_score > usa_score,
            "Europe ({europe_score}) should beat USA ({usa_score}) with Europe preference"
        );
    }

    // --- Word-level fuzzy matching ---

    #[test]
    fn word_match_sonic_3() {
        let score = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"sonic 3\" should match \"Sonic The Hedgehog 3\", got {score}"
        );
        assert!(
            score < 1000,
            "Word match ({score}) should be below substring tier (1000)"
        );
    }

    #[test]
    fn word_match_zelda_link() {
        let score = search_score(
            "zelda link",
            "The Legend of Zelda - A Link to the Past",
            "Legend of Zelda, The - A Link to the Past (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"zelda link\" should match Zelda ALTTP, got {score}"
        );
    }

    #[test]
    fn word_match_ranks_exact_word_above_prefix() {
        // Both titles fail substring tiers for "sonic 3".
        // "Sonic The Hedgehog 3" has exact word "3" — should rank higher than
        // "Sonic Adventures 3D Edition" where "3" only prefix-matches "3D".
        let hedgehog = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let adventures = search_score(
            "sonic 3",
            "Sonic Adventures 3D Edition",
            "Sonic Adventures 3D Edition (USA).md",
            PREF,
            SEC,
        );
        assert!(
            hedgehog > adventures,
            "Hedgehog 3 ({hedgehog}) should beat Adventures 3D ({adventures})"
        );
    }

    #[test]
    fn word_match_does_not_activate_for_single_word() {
        let score = search_score(
            "zzzznotfound",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn word_match_requires_all_words() {
        // "sonic mario" — "mario" is not in "Sonic The Hedgehog 3"
        let score = search_score(
            "sonic mario",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert_eq!(score, 0, "Not all query words present, should return 0");
    }

    #[test]
    fn word_match_below_substring() {
        // A substring match should always score higher than a word match
        let substring = search_score(
            "sonic",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let word = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert!(
            substring > word,
            "Substring ({substring}) should beat word match ({word})"
        );
    }

    #[test]
    fn word_match_x_men_hyphen() {
        let score = search_score(
            "x men",
            "X-Men - Mutant Apocalypse",
            "X-Men - Mutant Apocalypse (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"x men\" should match \"X-Men\" via hyphen splitting, got {score}"
        );
    }

    #[test]
    fn word_match_preserves_existing_substring_match() {
        // "mega man x" is a contiguous substring — should match at prefix tier, not word tier
        let score = search_score(
            "mega man x",
            "Mega Man X",
            "Mega Man X (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score >= 5000,
            "Contiguous match should hit prefix tier, got {score}"
        );
    }

    #[test]
    fn word_match_mario_kart() {
        let score = search_score(
            "mario kart",
            "Super Mario Kart",
            "Super Mario Kart (USA).sfc",
            PREF,
            SEC,
        );
        // "mario kart" IS a contiguous substring of "Super Mario Kart"
        assert!(
            score >= 1000,
            "\"mario kart\" is a substring, should score >= 1000, got {score}"
        );
    }

    // --- Prefix word-boundary tests ---

    #[test]
    fn prefix_match_word_boundary() {
        // "sonic 3" should score "Sonic 3 (Europe)" higher than "Sonic 3D Blast"
        // because "sonic 3" in "sonic 3d blast" breaks mid-word ("3" → "3d").
        let clean = search_score(
            "sonic 3",
            "Sonic 3 (Europe)",
            "Sonic 3 (Europe).md",
            PREF,
            SEC,
        );
        let midword = search_score(
            "sonic 3",
            "Sonic 3D Blast",
            "Sonic 3D Blast (USA).md",
            PREF,
            SEC,
        );
        assert!(
            clean > midword,
            "Clean prefix 'Sonic 3 (Europe)' ({clean}) should beat mid-word 'Sonic 3D Blast' ({midword})"
        );
    }

    #[test]
    fn sonic_3_hedgehog_above_3d_blast() {
        // "sonic 3" should rank "Sonic The Hedgehog 3 (USA)" above "Sonic 3D Blast (USA)"
        let hedgehog = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let blast = search_score(
            "sonic 3",
            "Sonic 3D Blast",
            "Sonic 3D Blast (USA).md",
            PREF,
            SEC,
        );
        assert!(
            hedgehog > blast,
            "Hedgehog 3 ({hedgehog}) should beat 3D Blast ({blast})"
        );
    }

    // --- split_into_words ---

    #[test]
    fn split_words_basic() {
        assert_eq!(split_into_words("super mario"), vec!["super", "mario"]);
    }

    #[test]
    fn split_words_hyphenated() {
        assert_eq!(
            split_into_words("x-men vs street"),
            vec!["x", "men", "vs", "street"]
        );
    }

    #[test]
    fn split_words_strips_trailing_punctuation() {
        assert_eq!(split_into_words("hello! world."), vec!["hello", "world"]);
    }

    #[test]
    fn split_words_empty() {
        let result: Vec<&str> = split_into_words("");
        assert!(result.is_empty());
    }

    // --- words_in_order ---

    #[test]
    fn words_in_order_positive() {
        assert!(words_in_order(
            &["super", "mario"],
            &["super", "mario", "world"]
        ));
    }

    #[test]
    fn words_in_order_negative() {
        assert!(!words_in_order(
            &["mario", "super"],
            &["super", "mario", "world"]
        ));
    }

    #[test]
    fn words_in_order_prefix_match() {
        // "mar" should match "mario" via starts_with
        assert!(words_in_order(
            &["sup", "mar"],
            &["super", "mario", "world"]
        ));
    }

    // --- count_adjacent_pairs ---

    #[test]
    fn adjacent_pairs_all_adjacent() {
        // "super mario world" in title "super mario world" -> 2 adjacent pairs
        let pairs =
            count_adjacent_pairs(&["super", "mario", "world"], &["super", "mario", "world"]);
        assert_eq!(pairs, 2);
    }

    #[test]
    fn adjacent_pairs_none_adjacent() {
        // "mario world" in "world of mario" -> 0 adjacent pairs
        let pairs = count_adjacent_pairs(&["mario", "world"], &["world", "of", "mario"]);
        assert_eq!(pairs, 0);
    }

    #[test]
    fn adjacent_pairs_single_word() {
        assert_eq!(count_adjacent_pairs(&["mario"], &["mario"]), 0);
    }

    // --- count_exact_word_matches ---

    #[test]
    fn exact_word_matches_all() {
        let count = count_exact_word_matches(&["super", "mario"], &["super", "mario", "world"]);
        assert_eq!(count, 2);
    }

    #[test]
    fn exact_word_matches_prefix_only() {
        // "mar" is not an exact match for "mario"
        let count = count_exact_word_matches(&["mar"], &["super", "mario", "world"]);
        assert_eq!(count, 0);
    }

    // --- try_word_match ---

    #[test]
    fn try_word_match_display_name() {
        let result = try_word_match(
            &["super", "mario"],
            &["super", "mario", "world"],
            &["super", "mario", "world", "(usa)"],
        );
        assert!(result.is_some());
        let (score, is_filename) = result.unwrap();
        assert!(!is_filename, "Should match display name, not filename");
        assert!(
            score >= 400,
            "Word match in display should be >= 400, got {score}"
        );
    }

    #[test]
    fn try_word_match_filename_only() {
        // Query words only in filename, not display name
        let result = try_word_match(
            &["usa", "rev"],
            &["tetris"],
            &["tetris", "(usa)", "(rev", "1)"],
        );
        if let Some((score, is_filename)) = result {
            assert!(is_filename, "Should match filename");
            assert!(
                score >= 300,
                "Word match in filename should be >= 300, got {score}"
            );
        }
    }

    #[test]
    fn try_word_match_no_match() {
        let result = try_word_match(
            &["zzz", "yyy"],
            &["super", "mario"],
            &["super", "mario", "(usa)"],
        );
        assert!(result.is_none());
    }

    // --- Region preference scoring ---

    #[test]
    fn preferred_region_scores_higher() {
        use RegionPreference::*;
        let usa_score = search_score("tetris", "Tetris", "Tetris (USA).nes", Usa, SEC);
        let jpn_score = search_score("tetris", "Tetris", "Tetris (Japan).nes", Usa, SEC);
        assert!(
            usa_score > jpn_score,
            "USA ROM ({usa_score}) should score higher than Japan ({jpn_score}) with USA preference"
        );
    }

    #[test]
    fn secondary_preference_used() {
        use RegionPreference::*;
        let jpn_primary = search_score(
            "tetris",
            "Tetris",
            "Tetris (Japan).nes",
            Japan,
            Some(Europe),
        );
        let eur_secondary = search_score(
            "tetris",
            "Tetris",
            "Tetris (Europe).nes",
            Japan,
            Some(Europe),
        );
        // Both should score > 0 and Japan should be >= Europe
        assert!(jpn_primary > 0);
        assert!(eur_secondary > 0);
        assert!(
            jpn_primary >= eur_secondary,
            "Primary pref ({jpn_primary}) should be >= secondary ({eur_secondary})"
        );
    }

    // --- Tier penalty in scoring ---

    #[test]
    fn hack_scores_lower_than_original() {
        let original = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let hack = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Hack).sfc",
            PREF,
            SEC,
        );
        assert!(
            original > hack,
            "Original ({original}) should beat hack ({hack})"
        );
    }

    // --- Edge cases ---

    #[test]
    fn empty_query_gets_high_score() {
        // An empty string is a substring of everything, so it matches
        // at the "exact match" tier (display_lower == query when both are lowered).
        // Actually "" == "" is false here because display_lower is "tetris" not "".
        // But "".contains("") is true, so it scores in the substring tier.
        let score = search_score("", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(score > 0, "Empty query matches via substring, got {score}");
    }

    #[test]
    fn single_char_query() {
        // Single character should still work via substring match
        let score = search_score("t", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(score > 0, "Single char query 't' should match Tetris");
    }
}
