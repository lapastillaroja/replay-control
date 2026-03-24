//! Developer name normalizer.
//!
//! Normalizes raw developer/manufacturer strings from arcade_db (MAME/FBNeo
//! manufacturer) and LaunchBox (developer field) to canonical company names
//! for search grouping and favorites organization.
//!
//! The raw value is preserved in `game_metadata.developer` and
//! `arcade_db.manufacturer` for display on game detail pages.
//!
//! Uses a hybrid approach: an algorithmic pipeline handles the mechanical
//! patterns (licensing suffixes, corporate suffixes, regional qualifiers,
//! joint ventures) that account for ~95% of variations, while a small
//! curated override table corrects the genuinely ambiguous cases that
//! require human judgment.

// ── Override table ──────────────────────────────────────────────────────
//
// Applied *before* the algorithm. Maps raw input strings (after trimming)
// to canonical names for cases where the algorithm would produce wrong
// results.

fn developer_override(raw: &str) -> Option<&'static str> {
    match raw {
        // Single company masquerading as joint venture (slash is part of the name).
        "Strata/Incredible Technologies" => Some("Incredible Technologies"),
        // Cave is the developer; Victor/Capcom are publishers.
        "Victor / Cave / Capcom" => Some("Cave"),
        "Capcom / Cave / Victor Interactive Software" => Some("Cave"),
        // Capcom developed, Sony published.
        "Sony/Capcom" => Some("Capcom"),
        // Corporate rebrand of the same company.
        "SNK Playmore" => Some("SNK"),
        // Subsidiary collapsed to parent.
        "Sega Toys" => Some("Sega"),
        // Capcom developed (e.g., Zelda Oracle games); Nintendo published.
        "Nintendo / Capcom" => Some("Capcom"),
        // Taito published Midway's game in Japan.
        "Taito Corporation (licensed from Midway)" => Some("Midway"),
        // Cave is the developer; IGS is the publisher.
        "IGS / Cave (Tong Li Animation license)" => Some("Cave"),
        "IGS / Cave" => Some("Cave"),
        _ => None,
    }
}

// ── Corporate suffixes ──────────────────────────────────────────────────
//
// Removed from the end of the string (after trimming). Order matters:
// longer/more specific patterns must come before shorter ones so that
// "Co., Ltd." is matched before "Co." and "Ltd.".

const CORPORATE_SUFFIXES: &[&str] = &[
    " Computer Entertainment Osaka",
    " Computer Entertainment Kobe",
    " Computer Entertainment Tokyo",
    " Digital Entertainment",
    " Technical Institute",
    " Interactive Software",
    " Entertainment",
    " Enterprises",
    " Corporation",
    " Industry",
    " of America",
    " of Japan",
    " Co., Ltd.",
    " Co., Ltd",
    " Corp.",
    " Corp",
    " LTD.",
    " Ltd.",
    " Ltd",
    " Inc.",
    " Inc",
    " Co.",
    " Co",
    " USA",
];

// ── Regional qualifiers ─────────────────────────────────────────────────
//
// Removed from the end of the string after corporate suffixes are stripped.

const REGIONAL_QUALIFIERS: &[&str] = &[" America", " Japan", " Europe", " do Brasil"];

// ── Division patterns ───────────────────────────────────────────────────
//
// Internal division names that should be collapsed to the parent company.
// Matched as suffixes after previous normalization steps.
// E.g., "Sega AM2" -> "Sega", "Nintendo R&D1" -> "Nintendo".

const DIVISION_SUFFIXES: &[&str] = &[
    " AM1", " AM2", " AM3", " AM4", " AM5", " CS1", " CS2", " CS3", " R&D 1", " R&D 2", " R&D 3",
    " R&D 4", " R&D1", " R&D2", " R&D3", " R&D4", " EAD", " SPD",
];

// ── Noise strings ───────────────────────────────────────────────────────

fn is_noise(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower == "bootleg"
        || lower == "<unknown>"
        || lower == "unknown"
        || lower.starts_with("bootleg ")
        || lower.starts_with("bootleg(")
        || lower.starts_with("hack ")
        || lower.starts_with("hack(")
        || lower == "hack"
}

/// Normalize a raw developer/manufacturer string to a canonical company name.
///
/// Used for search grouping and favorites organization. The raw value is
/// preserved in `game_metadata.developer` and `arcade_db.manufacturer`
/// for display on game detail pages.
///
/// Returns an empty string for empty/whitespace input and noise values
/// like `"bootleg"`, `"hack"`, `"<unknown>"`.
pub fn normalize_developer(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Step 0: Check override table (exact match on trimmed input).
    if let Some(canonical) = developer_override(trimmed) {
        return canonical.to_string();
    }

    // Step 9 (early): Filter noise values.
    if is_noise(trimmed) {
        return String::new();
    }

    let mut s = trimmed.to_string();

    // Step 1: Strip licensing annotations — "(... license)" or "(... license?)".
    // Also handles "(licensed from ...)" form.
    if let Some(paren_idx) = s.find('(') {
        let after_paren = &s[paren_idx..];
        let lower_paren = after_paren.to_ascii_lowercase();
        if lower_paren.contains("license") {
            s = s[..paren_idx].trim().to_string();
        }
    }

    // Step 2: Extract bracket-prefixed developer — "[Developer] Publisher".
    // The bracketed name is the actual developer (MAME convention).
    if s.starts_with('[')
        && let Some(close) = s.find(']')
    {
        let bracket_name = s[1..close].trim().to_string();
        if !bracket_name.is_empty() {
            s = bracket_name;
        }
    }

    // Step 3: Strip corporate suffixes (case-insensitive).
    strip_suffixes_ci(&mut s, CORPORATE_SUFFIXES);

    // Step 4: Strip regional qualifiers.
    strip_suffixes_ci(&mut s, REGIONAL_QUALIFIERS);

    // Step 5: Handle spaced-slash joint ventures — "A / B" -> "A".
    if let Some(idx) = s.find(" / ") {
        s = s[..idx].trim().to_string();
    }

    // Step 6: Handle no-space-slash collaborations — "A/B" -> "A".
    // Also handle "A + B" format.
    if let Some(idx) = s.find('/') {
        s = s[..idx].trim().to_string();
    } else if let Some(idx) = s.find(" + ") {
        s = s[..idx].trim().to_string();
    }

    // Step 3b: Re-strip corporate suffixes that may have been exposed after
    // splitting joint ventures (e.g., "Taito Corporation/Warashi" -> step 6
    // produces "Taito Corporation" -> needs suffix strip).
    strip_suffixes_ci(&mut s, CORPORATE_SUFFIXES);

    // Step 4b: Re-strip regional qualifiers.
    strip_suffixes_ci(&mut s, REGIONAL_QUALIFIERS);

    // Division collapse: "Sega AM2" -> "Sega", "Nintendo R&D1" -> "Nintendo".
    strip_suffixes_ci(&mut s, DIVISION_SUFFIXES);

    // Step 7: Trim trailing punctuation and whitespace.
    let s = s.trim_end_matches(|c: char| c == '/' || c == '?' || c.is_whitespace());
    let s = s.trim();

    if s.is_empty() {
        return String::new();
    }

    // Step 8: Case normalize — if the string is all uppercase (or all lowercase
    // with length > 1), title-case it. If it's already mixed case, preserve it.
    let result = normalize_case(s);

    // Final noise check after all transformations.
    if is_noise(&result) {
        return String::new();
    }

    result
}

/// Strip suffixes from `s` in-place (case-insensitive).
fn strip_suffixes_ci(s: &mut String, suffixes: &[&str]) {
    loop {
        let before = s.len();
        for suffix in suffixes {
            let s_lower = s.to_ascii_lowercase();
            let suffix_lower = suffix.to_ascii_lowercase();
            if s_lower.ends_with(&suffix_lower) {
                let new_len = s.len() - suffix.len();
                s.truncate(new_len);
                *s = s.trim().to_string();
            }
        }
        if s.len() == before {
            break;
        }
    }
}

/// Normalize case: if all-uppercase and longer than a typical acronym,
/// convert to title case. Short all-uppercase strings (like "SNK", "ADK",
/// "IGS") are preserved as-is since they're likely acronyms.
fn normalize_case(s: &str) -> String {
    if s.len() <= 1 {
        return s.to_string();
    }

    // Check if the string is all ASCII uppercase (ignoring non-alpha characters).
    let alpha_chars: Vec<char> = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if alpha_chars.is_empty() {
        return s.to_string();
    }

    let all_upper = alpha_chars.iter().all(|c| c.is_ascii_uppercase());
    // Only title-case if all-uppercase AND has more than 3 alpha characters.
    // Short all-uppercase strings are likely acronyms (SNK, ADK, IGS, NMK).
    if all_upper && alpha_chars.len() > 3 {
        // Title-case: uppercase first letter, lowercase the rest.
        let mut chars = s.chars();
        let first = chars.next().unwrap();
        let mut result = first.to_uppercase().to_string();
        for c in chars {
            result.extend(c.to_lowercase());
        }
        result
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Clean input (pass-through) ──

    #[test]
    fn clean_name_unchanged() {
        assert_eq!(normalize_developer("Capcom"), "Capcom");
        assert_eq!(normalize_developer("Konami"), "Konami");
        assert_eq!(normalize_developer("Sega"), "Sega");
        assert_eq!(normalize_developer("Namco"), "Namco");
        assert_eq!(normalize_developer("Cave"), "Cave");
        assert_eq!(normalize_developer("Irem"), "Irem");
    }

    // ── Empty and whitespace ──

    #[test]
    fn empty_input() {
        assert_eq!(normalize_developer(""), "");
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(normalize_developer("  "), "");
        assert_eq!(normalize_developer("\t"), "");
    }

    // ── Step 1: License stripping ──

    #[test]
    fn strip_license_annotation() {
        assert_eq!(normalize_developer("Capcom (Romstar license)"), "Capcom");
        assert_eq!(normalize_developer("Konami (Centuri license)"), "Konami");
        assert_eq!(normalize_developer("Cave (Capcom license)"), "Cave");
        assert_eq!(normalize_developer("Cave (Jaleco license)"), "Cave");
        assert_eq!(
            normalize_developer("Sega (Stern Electronics license)"),
            "Sega"
        );
        assert_eq!(normalize_developer("Irem (Taito license)"), "Irem");
        assert_eq!(normalize_developer("SNK (Centuri license)"), "SNK");
        assert_eq!(normalize_developer("Namco (Atari license)"), "Namco");
        assert_eq!(normalize_developer("Namco (Bally Midway license)"), "Namco");
    }

    // ── Step 2: Bracket extraction ──

    #[test]
    fn bracket_prefixed_developer() {
        assert_eq!(
            normalize_developer("[Toaplan] Taito Corporation"),
            "Toaplan"
        );
        assert_eq!(
            normalize_developer("[Toaplan] Taito America Corporation"),
            "Toaplan"
        );
        assert_eq!(
            normalize_developer("[Toaplan] Taito Corporation Japan"),
            "Toaplan"
        );
        assert_eq!(normalize_developer("[Konami] (Sega license)"), "Konami");
        assert_eq!(normalize_developer("[Namco] (Midway license)"), "Namco");
        assert_eq!(normalize_developer("[Namco] (Gremlin license)"), "Namco");
        assert_eq!(normalize_developer("[Sanritsu] Sega"), "Sanritsu");
        assert_eq!(normalize_developer("[Technos] (Taito license)"), "Technos");
        assert_eq!(normalize_developer("[SNK] (Rock-ola license)"), "SNK");
    }

    #[test]
    fn bracket_with_corporate_suffix() {
        // "[Alpha Denshi Co.] (SNK license)" -> "Alpha Denshi" (strips Co.)
        assert_eq!(
            normalize_developer("[Alpha Denshi Co.] (SNK license)"),
            "Alpha Denshi"
        );
    }

    // ── Step 3: Corporate suffix stripping ──

    #[test]
    fn corporate_suffixes() {
        assert_eq!(normalize_developer("Data East Corporation"), "Data East");
        assert_eq!(normalize_developer("Taito Corporation"), "Taito");
        assert_eq!(normalize_developer("Konami Co., Ltd."), "Konami");
        assert_eq!(normalize_developer("Namco LTD."), "Namco");
        assert_eq!(normalize_developer("Video System Co."), "Video System");
        assert_eq!(normalize_developer("Sega Enterprises"), "Sega");
        assert_eq!(normalize_developer("Konami Industry"), "Konami");
        assert_eq!(
            normalize_developer("Konami Computer Entertainment Osaka"),
            "Konami"
        );
        assert_eq!(
            normalize_developer("Konami Computer Entertainment Kobe"),
            "Konami"
        );
        assert_eq!(
            normalize_developer("Konami Digital Entertainment"),
            "Konami"
        );
    }

    // ── Step 4: Regional qualifier stripping ──

    #[test]
    fn regional_qualifiers() {
        assert_eq!(normalize_developer("Sega of America"), "Sega");
        assert_eq!(normalize_developer("Taito America Corporation"), "Taito");
        assert_eq!(normalize_developer("Taito America Corp"), "Taito");
        assert_eq!(normalize_developer("Taito America Corp."), "Taito");
        assert_eq!(normalize_developer("Taito Corporation Japan"), "Taito");
        assert_eq!(normalize_developer("Taito Europe Corporation"), "Taito");
        assert_eq!(normalize_developer("Taito do Brasil"), "Taito");
        assert_eq!(normalize_developer("Capcom USA"), "Capcom");
        assert_eq!(normalize_developer("SNK of America"), "SNK");
    }

    // ── Step 5: Spaced-slash joint ventures ──

    #[test]
    fn spaced_slash_joint_venture() {
        assert_eq!(normalize_developer("Capcom / SNK"), "Capcom");
        assert_eq!(normalize_developer("Coreland / Sega"), "Coreland");
        assert_eq!(normalize_developer("ADK / SNK"), "ADK");
        assert_eq!(normalize_developer("Eolith / SNK"), "Eolith");
        assert_eq!(normalize_developer("Sega / Banpresto"), "Sega");
        assert_eq!(normalize_developer("Sega / Westone"), "Sega");
        assert_eq!(
            normalize_developer("Amusement Vision / Sega"),
            "Amusement Vision"
        );
        assert_eq!(normalize_developer("Toaplan / Taito"), "Toaplan");
    }

    // ── Step 6: No-space-slash collaborations ──

    #[test]
    fn no_space_slash_collaboration() {
        assert_eq!(normalize_developer("Capcom/Arika"), "Capcom");
        assert_eq!(normalize_developer("Atlus/Cave"), "Atlus");
        assert_eq!(normalize_developer("Sega/Gremlin"), "Sega");
    }

    #[test]
    fn plus_collaboration() {
        assert_eq!(normalize_developer("Mitchell + Capcom"), "Mitchell");
    }

    // ── Suffix after slash split ──

    #[test]
    fn suffix_after_slash_split() {
        // "Taito Corporation/Warashi" -> split -> "Taito Corporation" -> strip -> "Taito"
        assert_eq!(normalize_developer("Taito Corporation/Warashi"), "Taito");
        // "Kaneko / Taito Corporation" -> split -> "Kaneko"
        assert_eq!(normalize_developer("Kaneko / Taito Corporation"), "Kaneko");
    }

    // ── Division collapse ──

    #[test]
    fn division_collapse() {
        assert_eq!(normalize_developer("Sega AM2"), "Sega");
        assert_eq!(normalize_developer("Sega AM1"), "Sega");
        assert_eq!(normalize_developer("Sega AM3"), "Sega");
        assert_eq!(normalize_developer("Sega CS1"), "Sega");
        assert_eq!(normalize_developer("Sega R&D 2"), "Sega");
        assert_eq!(normalize_developer("Nintendo R&D1"), "Nintendo");
        assert_eq!(normalize_developer("Nintendo R&D2"), "Nintendo");
        assert_eq!(normalize_developer("Nintendo R&D3"), "Nintendo");
        assert_eq!(normalize_developer("Nintendo EAD"), "Nintendo");
        assert_eq!(normalize_developer("Sega Technical Institute"), "Sega");
    }

    // ── Override table ──

    #[test]
    fn override_snk_playmore() {
        assert_eq!(normalize_developer("SNK Playmore"), "SNK");
    }

    #[test]
    fn override_sega_toys() {
        assert_eq!(normalize_developer("Sega Toys"), "Sega");
    }

    #[test]
    fn override_strata_incredible_technologies() {
        assert_eq!(
            normalize_developer("Strata/Incredible Technologies"),
            "Incredible Technologies"
        );
    }

    #[test]
    fn override_cave_publisher_variants() {
        assert_eq!(normalize_developer("Victor / Cave / Capcom"), "Cave");
        assert_eq!(
            normalize_developer("Capcom / Cave / Victor Interactive Software"),
            "Cave"
        );
        assert_eq!(
            normalize_developer("IGS / Cave (Tong Li Animation license)"),
            "Cave"
        );
        assert_eq!(normalize_developer("IGS / Cave"), "Cave");
    }

    #[test]
    fn override_sony_capcom() {
        assert_eq!(normalize_developer("Sony/Capcom"), "Capcom");
    }

    #[test]
    fn override_nintendo_capcom() {
        assert_eq!(normalize_developer("Nintendo / Capcom"), "Capcom");
    }

    #[test]
    fn override_taito_licensed_from_midway() {
        assert_eq!(
            normalize_developer("Taito Corporation (licensed from Midway)"),
            "Midway"
        );
    }

    // ── Noise filtering ──

    #[test]
    fn noise_bootleg() {
        assert_eq!(normalize_developer("bootleg"), "");
        assert_eq!(normalize_developer("bootleg (Itisa)"), "");
        assert_eq!(normalize_developer("bootleg (Capcom)"), "");
    }

    #[test]
    fn noise_hack() {
        assert_eq!(normalize_developer("hack"), "");
        assert_eq!(normalize_developer("hack (Two Bit Score)"), "");
    }

    #[test]
    fn noise_unknown() {
        assert_eq!(normalize_developer("<unknown>"), "");
        assert_eq!(normalize_developer("unknown"), "");
    }

    // ── Case normalization ──

    #[test]
    fn all_uppercase_normalized() {
        assert_eq!(normalize_developer("CAPCOM"), "Capcom");
        assert_eq!(normalize_developer("SEGA"), "Sega");
    }

    #[test]
    fn short_acronyms_preserved() {
        // Short all-uppercase strings (<=3 alpha chars) are likely acronyms.
        assert_eq!(normalize_developer("SNK"), "SNK");
        assert_eq!(normalize_developer("ADK"), "ADK");
        assert_eq!(normalize_developer("IGS"), "IGS");
        assert_eq!(normalize_developer("NMK"), "NMK");
    }

    #[test]
    fn mixed_case_preserved() {
        assert_eq!(normalize_developer("Data East"), "Data East");
        assert_eq!(normalize_developer("dB-Soft"), "dB-Soft");
    }

    // ── License stripping with joint venture ──

    #[test]
    fn license_then_joint_venture() {
        // "Eighting / Raizing (Capcom license)" -> strip license -> "Eighting / Raizing" -> split -> "Eighting"
        assert_eq!(
            normalize_developer("Eighting / Raizing (Capcom license)"),
            "Eighting"
        );
    }

    // ── Trailing punctuation ──

    #[test]
    fn trailing_slash() {
        assert_eq!(normalize_developer("Capcom/"), "Capcom");
    }

    #[test]
    fn trailing_question_mark() {
        assert_eq!(normalize_developer("SomeCompany?"), "SomeCompany");
    }

    // ── Combined patterns from real data ──

    #[test]
    fn combined_real_world() {
        // Atari Games -> strip "Games" is NOT in corporate suffixes, stays as-is.
        // But "Atari Games" is a legitimate company name, not a suffix issue.
        assert_eq!(normalize_developer("Atari Games"), "Atari Games");

        // "Capcom (Williams Electronics license)" -> "Capcom"
        assert_eq!(
            normalize_developer("Capcom (Williams Electronics license)"),
            "Capcom"
        );
    }
}
