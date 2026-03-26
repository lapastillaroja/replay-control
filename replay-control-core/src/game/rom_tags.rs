/// Parse useful identifying tags from ROM filenames to append to display names.
///
/// ROM filenames from No-Intro and other sources contain parenthesized and
/// bracketed tags like `(USA)`, `(Rev 1)`, `(Traducido Es)`, `[T-Spa1.0v_Wave]`,
/// `(60hz)`, `(FastRom)`, `(Hack)`, etc.
///
/// This module extracts the *useful* tags — the ones that help users distinguish
/// between multiple versions of the same game — and formats them as a concise
/// suffix string for display.
///
/// Tags considered useful:
/// - Region: USA, Europe, Japan, World, Spain, France, etc.
/// - Revision: Rev 1, Rev A, Rev 2, etc.
/// - Translation language: Traducido Es, Translated En, T-Spa, T+Fre, PT-BR, etc.
/// - Patches: 60hz, FastROM
/// - Hack/Aftermarket indicators
/// - Beta/Proto/Demo indicators
/// - Unlicensed indicator
/// - TOSEC bracket dump flags: [a] Alternate, [h] Hack, [cr] Cracked, [t] Trained,
///   [f] Fixed, [o] Overdump, [b] Bad Dump, [p] Pirate
///
/// Tags NOT shown (noise for end users):
/// - Verified dump markers: [!]
/// - Version dates: [2017-03-28]
/// - Hacker credits in brackets: [T-Spa1.0v_Wave] -> shown as "ES" not the full tag
/// - Language codes already in the region: (En,Fr,De) merged with region
///
/// Classification tier for sorting ROMs — lower value = shown first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RomTier {
    /// Clean original ROM.
    Original = 0,
    /// Revision of an original (Rev 1, Rev A).
    Revision = 1,
    /// Non-primary region variant.
    RegionVariant = 2,
    /// Translation patch applied.
    Translation = 3,
    /// Unlicensed but commercial.
    Unlicensed = 4,
    /// Homebrew / aftermarket.
    Homebrew = 5,
    /// ROM hack.
    Hack = 6,
    /// Beta, prototype, or demo.
    PreRelease = 7,
    /// Pirate / bootleg.
    Pirate = 8,
}

/// Region priority for sorting — lower = shown first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionPriority {
    World = 0,
    Usa = 1,
    Europe = 2,
    Japan = 3,
    Other = 4,
    Unknown = 5,
}

/// User's preferred region for sorting and search prioritization.
/// Controls which region's ROMs appear first in game lists and search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RegionPreference {
    Usa,
    Europe,
    Japan,
    #[default]
    World,
}

impl RegionPreference {
    /// Parse a region preference from a string value (as stored in settings.cfg).
    /// Returns `World` for unrecognized values.
    pub fn from_str_value(s: &str) -> Self {
        match s {
            "usa" => Self::Usa,
            "europe" => Self::Europe,
            "japan" => Self::Japan,
            _ => Self::World,
        }
    }

    /// Convert to a string value suitable for storing in settings.cfg.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Usa => "usa",
            Self::Europe => "europe",
            Self::Japan => "japan",
            Self::World => "world",
        }
    }
}

impl RegionPriority {
    /// Return a sort key for this region given the user's preferences.
    ///
    /// Strategy C (Primary > Secondary > World):
    /// - sort_key 0 = Primary preference
    /// - sort_key 1 = Secondary preference (or World when no secondary)
    /// - sort_key 2 = World (when secondary is set)
    /// - sort_key 3 = Remaining major region(s)
    /// - sort_key 4 = Other
    /// - sort_key 5 = Unknown
    ///
    /// When primary is `World` and no secondary: World > Usa > Europe > Japan > Other > Unknown.
    /// When secondary equals primary or is `None`, behaves as if no secondary is set.
    pub fn sort_key(self, pref: RegionPreference, secondary: Option<RegionPreference>) -> u8 {
        // Normalize: if secondary equals primary, treat as None.
        let secondary = secondary.filter(|s| *s != pref);

        // Other and Unknown always occupy the bottom two slots.
        if self == RegionPriority::Other {
            return 4;
        }
        if self == RegionPriority::Unknown {
            return 5;
        }

        // Map RegionPreference to the corresponding RegionPriority.
        let pref_priority = pref_to_priority(pref);

        // Check primary.
        if self == pref_priority {
            return 0;
        }

        if let Some(sec) = secondary {
            let sec_priority = pref_to_priority(sec);

            // Check secondary.
            if self == sec_priority {
                return 1;
            }

            // Check World (when secondary is set, World goes to slot 2).
            if self == RegionPriority::World {
                return 2;
            }

            // Remaining major region(s) go to slot 3.
            return 3;
        }

        // No secondary: World goes to slot 1.
        if self == RegionPriority::World {
            return 1;
        }

        // Remaining major regions fill slots 2-3 in a fixed order.
        // Use a hardcoded fallback order for the remaining majors.
        let remaining = remaining_order(pref);
        for (i, r) in remaining.iter().enumerate() {
            if self == *r {
                return (2 + i) as u8;
            }
        }

        // Should not reach here for valid RegionPriority values,
        // but return 3 as a safe fallback.
        3
    }
}

/// Map a `RegionPreference` to the corresponding `RegionPriority` variant.
fn pref_to_priority(pref: RegionPreference) -> RegionPriority {
    match pref {
        RegionPreference::Usa => RegionPriority::Usa,
        RegionPreference::Europe => RegionPriority::Europe,
        RegionPreference::Japan => RegionPriority::Japan,
        RegionPreference::World => RegionPriority::World,
    }
}

/// Return the remaining major regions (excluding World and the preferred region)
/// in a fixed fallback order: Usa, Europe, Japan (skipping the one that matches pref).
fn remaining_order(pref: RegionPreference) -> Vec<RegionPriority> {
    let all = [
        RegionPriority::Usa,
        RegionPriority::Europe,
        RegionPriority::Japan,
    ];
    let pref_priority = pref_to_priority(pref);
    all.iter()
        .copied()
        .filter(|r| *r != pref_priority && *r != RegionPriority::World)
        .collect()
}

/// Classify a ROM filename into a tier, region priority, and special flag.
///
/// The `is_special` bool is `true` for ROMs that should be excluded from
/// recommendations and the regional variants chip row: FastROM patches,
/// 60Hz patches, unlicensed, homebrew, pre-release, and pirate ROMs.
pub fn classify(filename: &str) -> (RomTier, RegionPriority, bool) {
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    let mut has_region = false;
    let mut region_priority = RegionPriority::Unknown;
    let mut has_revision = false;
    let mut has_translation = false;
    let mut is_hack = false;
    let mut is_beta = false;
    let mut is_proto = false;
    let mut is_demo = false;
    let mut is_sample = false;
    let mut is_unlicensed = false;
    let mut is_aftermarket = false;
    let mut is_pirate = false;
    let mut has_fastrom = false;
    let mut has_60hz = false;

    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();

        if (lower.starts_with("rev ") || (lower.starts_with("rev") && trimmed.len() <= 6))
            && parse_revision(trimmed).is_some()
        {
            has_revision = true;
            continue;
        }
        if parse_translation_paren(trimmed).is_some() {
            has_translation = true;
            continue;
        }
        if lower == "hack"
            || lower == "smw hack"
            || lower == "sa-1 smw hack"
            || lower == "smw2 hack"
            || lower == "smrpg hack"
            || lower == "smk hack"
            || lower.ends_with(" hack")
        {
            is_hack = true;
            continue;
        }
        if lower == "beta"
            || lower.starts_with("beta ")
            || lower == "alpha"
            || lower == "preview"
            || lower == "pre-release"
        {
            is_beta = true;
            continue;
        }
        if lower == "proto" || lower == "prototype" || lower.starts_with("proto ") {
            is_proto = true;
            continue;
        }
        if lower == "demo" || lower.starts_with("demo ") {
            is_demo = true;
            continue;
        }
        if lower == "sample" {
            is_sample = true;
            continue;
        }
        if lower == "unl" || lower == "unlicensed" {
            is_unlicensed = true;
            continue;
        }
        if lower == "aftermarket" || lower == "homebrew" {
            is_aftermarket = true;
            continue;
        }
        if lower == "pirate" {
            is_pirate = true;
            continue;
        }
        if lower == "fastrom" {
            has_fastrom = true;
            continue;
        }
        if lower == "60hz" {
            has_60hz = true;
            continue;
        }
        if !is_noise_tag(&lower) && looks_like_region(trimmed) {
            has_region = true;
            region_priority = region_to_priority(trimmed);
        }
    }

    // Check bracketed tags: translations and TOSEC dump flags
    for tag in BracketTags::new(stem) {
        let trimmed = tag.trim();
        if parse_translation_bracket(trimmed).is_some() {
            has_translation = true;
            continue;
        }
        // TOSEC standard bracket flags (case-insensitive, with optional suffix)
        if let Some(flag) = classify_tosec_bracket(trimmed) {
            match flag {
                TosecBracketFlag::Hack => is_hack = true,
                TosecBracketFlag::Pirate => is_pirate = true,
                TosecBracketFlag::Alternate | TosecBracketFlag::Fixed | TosecBracketFlag::Overdump => {
                    has_revision = true;
                }
                TosecBracketFlag::BadDump => is_pirate = true,
                TosecBracketFlag::Trained | TosecBracketFlag::Cracked => is_hack = true,
            }
        }
    }

    let tier = if is_pirate {
        RomTier::Pirate
    } else if is_beta || is_proto || is_demo || is_sample {
        RomTier::PreRelease
    } else if is_hack {
        RomTier::Hack
    } else if is_aftermarket {
        RomTier::Homebrew
    } else if is_unlicensed {
        RomTier::Unlicensed
    } else if has_translation {
        RomTier::Translation
    } else if has_revision {
        RomTier::Revision
    } else if has_region && matches!(region_priority, RegionPriority::Other) {
        RomTier::RegionVariant
    } else {
        RomTier::Original
    };

    // is_special: non-standard ROMs that should be excluded from recommendations
    // and the regional variants chip row.
    let is_special = matches!(
        tier,
        RomTier::Unlicensed | RomTier::Homebrew | RomTier::PreRelease | RomTier::Pirate
    ) || has_fastrom
        || has_60hz;

    (tier, region_priority, is_special)
}

/// Map a region tag to a sort priority.
fn region_to_priority(tag: &str) -> RegionPriority {
    // Check TOSEC two-letter country codes first
    match tag {
        "US" => return RegionPriority::Usa,
        "EU" | "GB" => return RegionPriority::Europe,
        "JP" => return RegionPriority::Japan,
        _ => {}
    }
    // TOSEC codes for minor regions
    if is_tosec_country_code(tag) {
        return RegionPriority::Other;
    }
    let lower = tag.to_lowercase();
    let parts: Vec<&str> = lower.split(',').map(|s| s.trim()).collect();
    let first = parts.first().copied().unwrap_or("");
    match first {
        "world" | "w" => RegionPriority::World,
        "usa" | "u" => RegionPriority::Usa,
        "usa, europe" | "ue" => RegionPriority::Usa,
        "europe" | "e" => RegionPriority::Europe,
        "japan" | "j" => RegionPriority::Japan,
        _ if first.contains("usa") => RegionPriority::Usa,
        _ if first.contains("europe") => RegionPriority::Europe,
        _ if first.contains("japan") => RegionPriority::Japan,
        _ => RegionPriority::Other,
    }
}

/// Extract a concise display suffix from a ROM filename.
///
/// Returns an empty string if no useful tags are found, or a string like
/// `"USA, Rev 1"` (without outer parentheses — the caller wraps them).
///
/// # Examples
/// ```
/// use replay_control_core::rom_tags::extract_tags;
///
/// assert_eq!(extract_tags("Super Mario World (USA).sfc"), "USA");
/// assert_eq!(extract_tags("Super Mario World (Europe) (60hz).sfc"), "Europe, 60Hz");
/// assert_eq!(extract_tags("Super Mario World (Japan) (Rev 1).sfc"), "Japan, Rev 1");
/// assert_eq!(extract_tags("Zelda (USA) (Traducido Es).smc"), "USA, ES Translation");
/// assert_eq!(extract_tags("Sonic (USA, Europe).md"), "USA, Europe");
/// assert_eq!(extract_tags("Game (USA) (FastRom).sfc"), "USA, FastROM");
/// assert_eq!(extract_tags("Game (USA) (Hack).sfc"), "USA, Hack");
/// assert_eq!(extract_tags("Game (USA) (Beta).sfc"), "USA, Beta");
/// assert_eq!(extract_tags("Game.sfc"), "");
/// ```
pub fn extract_tags(filename: &str) -> String {
    // Strip extension
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    let mut region: Option<String> = None;
    let mut revision: Option<String> = None;
    let mut translation: Option<String> = None;
    let mut patch_60hz = false;
    let mut patch_fastrom = false;
    let mut is_hack = false;
    let mut is_beta = false;
    let mut is_proto = false;
    let mut is_demo = false;
    let mut is_sample = false;
    let mut is_unlicensed = false;
    let mut is_aftermarket = false;
    let mut is_pirate = false;
    let mut is_alternate = false;
    let mut is_cracked = false;
    let mut is_trained = false;
    let mut is_fixed = false;
    let mut is_overdump = false;
    let mut is_bad_dump = false;

    // Extract all parenthesized tags: (...)
    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();

        // Revision tags
        if (lower.starts_with("rev ") || (lower.starts_with("rev") && trimmed.len() <= 6))
            && let Some(rev) = parse_revision(trimmed)
        {
            revision = Some(rev);
            continue;
        }

        // Translation tags (parenthesized style)
        if let Some(lang) = parse_translation_paren(trimmed) {
            translation = Some(lang);
            continue;
        }

        // Patch tags
        if lower == "60hz" {
            patch_60hz = true;
            continue;
        }
        if lower == "fastrom" {
            patch_fastrom = true;
            continue;
        }

        // Status tags
        if lower == "hack"
            || lower == "smw hack"
            || lower == "sa-1 smw hack"
            || lower == "smw2 hack"
            || lower == "smrpg hack"
            || lower == "smk hack"
            || lower == "sd gundam g next hack"
            || lower == "uncensored hack"
            || lower.ends_with(" hack")
        {
            is_hack = true;
            continue;
        }
        if lower == "beta"
            || lower.starts_with("beta ")
            || lower == "alpha"
            || lower == "preview"
            || lower == "pre-release"
        {
            is_beta = true;
            continue;
        }
        if lower == "proto" || lower == "prototype" || lower.starts_with("proto ") {
            is_proto = true;
            continue;
        }
        if lower == "demo" || lower.starts_with("demo ") {
            is_demo = true;
            continue;
        }
        if lower == "sample" {
            is_sample = true;
            continue;
        }
        if lower == "unl" || lower == "unlicensed" {
            is_unlicensed = true;
            continue;
        }
        if lower == "aftermarket" || lower == "homebrew" {
            is_aftermarket = true;
            continue;
        }
        if lower == "pirate" {
            is_pirate = true;
            continue;
        }

        // Skip non-region noise: Virtual Console, Switch Online, language-only
        // codes like (En), (Ja), (En,Fr,De), NP, BS, etc.
        if is_noise_tag(&lower) {
            continue;
        }

        // If we haven't matched anything else, treat as region if it looks like one
        if region.is_none() && looks_like_region(trimmed) {
            region = Some(normalize_region(trimmed));
            continue;
        }
    }

    // Extract bracketed tags: [...]
    for tag in BracketTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Translation tags in brackets: [T-Spa1.0v_Wave], [T+Fre], [T+Rus Pirate], etc.
        if let Some(lang) = parse_translation_bracket(trimmed) {
            if translation.is_none() {
                translation = Some(lang);
            }
            continue;
        }

        // TOSEC standard bracket flags — classify and set display label flags.
        if let Some(flag) = classify_tosec_bracket(trimmed) {
            match flag {
                TosecBracketFlag::Hack => is_hack = true,
                TosecBracketFlag::Trained => is_trained = true,
                TosecBracketFlag::Cracked => is_cracked = true,
                TosecBracketFlag::Pirate => is_pirate = true,
                TosecBracketFlag::Alternate => is_alternate = true,
                TosecBracketFlag::Fixed => is_fixed = true,
                TosecBracketFlag::Overdump => is_overdump = true,
                TosecBracketFlag::BadDump => is_bad_dump = true,
            }
            continue;
        }

        // Skip dates: [2017-03-28], verified dumps: [!]
    }

    // Build the suffix parts
    let mut parts: Vec<String> = Vec::new();

    if let Some(r) = region {
        parts.push(r);
    }

    if let Some(rev) = revision {
        parts.push(rev);
    }

    if let Some(lang) = translation {
        parts.push(format!("{lang} Translation"));
    }

    if patch_60hz {
        parts.push("60Hz".to_string());
    }
    if patch_fastrom {
        parts.push("FastROM".to_string());
    }

    if is_hack {
        parts.push("Hack".to_string());
    }
    if is_cracked {
        parts.push("Cracked".to_string());
    }
    if is_trained {
        parts.push("Trained".to_string());
    }
    if is_alternate {
        parts.push("Alternate".to_string());
    }
    if is_fixed {
        parts.push("Fixed".to_string());
    }
    if is_overdump {
        parts.push("Overdump".to_string());
    }
    if is_bad_dump {
        parts.push("Bad Dump".to_string());
    }
    if is_beta {
        parts.push("Beta".to_string());
    }
    if is_proto {
        parts.push("Proto".to_string());
    }
    if is_demo {
        parts.push("Demo".to_string());
    }
    if is_sample {
        parts.push("Sample".to_string());
    }
    if is_unlicensed {
        parts.push("Unlicensed".to_string());
    }
    if is_aftermarket {
        parts.push("Homebrew".to_string());
    }
    if is_pirate {
        parts.push("Pirate".to_string());
    }

    parts.join(", ")
}

/// TOSEC standard square-bracket dump flag classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TosecBracketFlag {
    /// `[a]`, `[a2]`, etc. — alternate dump
    Alternate,
    /// `[h]`, `[h Hack Name]` — hack
    Hack,
    /// `[cr]`, `[cr Cracker]` — cracked (copy protection removed)
    Cracked,
    /// `[t]`, `[t1]`, `[t +2]` — trained/trainer
    Trained,
    /// `[f]`, `[f1]` — fixed for emulator compatibility
    Fixed,
    /// `[o]`, `[o1]` — overdump
    Overdump,
    /// `[b]`, `[b1]` — bad dump
    BadDump,
    /// `[p]`, `[p1]` — pirate
    Pirate,
}

/// Classify a TOSEC square-bracket flag into a known dump flag.
///
/// Recognizes patterns like `[a]`, `[a2]`, `[h Hack Name]`, `[cr Cracker]`,
/// `[t +2]`, `[f1]`, `[o1]`, `[b]`, `[p1]`.
///
/// Returns `None` for unknown bracket tags (game-specific flags like
/// `[joystick]`, `[experimental]`, etc.).
fn classify_tosec_bracket(tag: &str) -> Option<TosecBracketFlag> {
    let lower = tag.to_lowercase();

    // [!] = verified good dump — not a flag, skip
    if lower == "!" {
        return None;
    }

    // Check single-letter flags with optional numeric suffix or space-separated content
    // Pattern: flag letter(s) followed by optional digit(s) or space + description
    if lower == "b" || lower.starts_with("b ") || starts_with_flag(&lower, "b") {
        return Some(TosecBracketFlag::BadDump);
    }
    if lower == "a" || starts_with_flag(&lower, "a") {
        return Some(TosecBracketFlag::Alternate);
    }
    if lower == "h" || lower.starts_with("h ") {
        return Some(TosecBracketFlag::Hack);
    }
    if lower == "cr" || lower.starts_with("cr ") || starts_with_flag(&lower, "cr") {
        return Some(TosecBracketFlag::Cracked);
    }
    if lower == "t" || lower.starts_with("t ") || lower.starts_with("t+") || starts_with_flag(&lower, "t") {
        // Avoid matching translation tags [T-xxx] and [T+xxx] — those are handled by
        // parse_translation_bracket() which runs first.
        // But [t] (lowercase, no + or -) is a trainer flag.
        // [t +2] is "trainer with 2 cheats".
        // Since parse_translation_bracket checks for T+ and T- (case-insensitive),
        // if we got here, it wasn't matched as a translation.
        return Some(TosecBracketFlag::Trained);
    }
    if lower == "f" || starts_with_flag(&lower, "f") {
        return Some(TosecBracketFlag::Fixed);
    }
    if lower == "o" || starts_with_flag(&lower, "o") {
        return Some(TosecBracketFlag::Overdump);
    }
    if lower == "p" || lower.starts_with("p ") || starts_with_flag(&lower, "p") {
        return Some(TosecBracketFlag::Pirate);
    }

    None
}

/// Check if a lowercase string starts with a flag prefix followed by a digit.
/// E.g., `starts_with_flag("a2", "a")` → true, `starts_with_flag("apple", "a")` → false.
fn starts_with_flag(lower: &str, prefix: &str) -> bool {
    lower.starts_with(prefix)
        && lower.len() > prefix.len()
        && lower.as_bytes()[prefix.len()].is_ascii_digit()
}

/// Check if a ROM filename has any known TOSEC bracket dump flag.
///
/// Returns `true` if the filename contains `[a]`, `[a2]`, `[h]`, `[cr]`,
/// `[t]`, `[f]`, `[o]`, `[b]`, or `[p]` bracket tags.
///
/// Used by the clone inference pipeline to determine if a ROM is a
/// variant that should be marked as a clone when a clean sibling exists.
pub fn has_tosec_bracket_flag(filename: &str) -> bool {
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    for tag in BracketTags::new(stem) {
        let trimmed = tag.trim();
        // Skip translation tags — those are not dump quality flags
        if parse_translation_bracket(trimmed).is_some() {
            continue;
        }
        if classify_tosec_bracket(trimmed).is_some() {
            return true;
        }
    }
    false
}

/// Extract non-standard bracket tag content from a TOSEC filename for disambiguation.
///
/// Returns the content of bracket tags that are NOT standard TOSEC dump flags
/// and NOT translation tags. These are game-specific descriptors like
/// `[joystick]`, `[experimental]`, `[full]`, etc.
///
/// Used to disambiguate display names when multiple non-clone entries share
/// the same base display name.
pub fn extract_bracket_descriptors(filename: &str) -> Vec<String> {
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    let mut descriptors = Vec::new();
    for tag in BracketTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() || trimmed == "!" {
            continue;
        }
        // Skip translation tags
        if parse_translation_bracket(trimmed).is_some() {
            continue;
        }
        // Skip standard dump flags
        if classify_tosec_bracket(trimmed).is_some() {
            continue;
        }
        // Skip dates like [2017-03-28]
        if trimmed.len() == 10
            && trimmed.as_bytes().get(4) == Some(&b'-')
            && trimmed.as_bytes().get(7) == Some(&b'-')
        {
            continue;
        }
        descriptors.push(trimmed.to_string());
    }
    descriptors
}

/// Format the final display name with optional tag suffix.
///
/// If `extract_tags` returns a non-empty string, wraps it in parentheses
/// and appends to the display name.
pub fn display_name_with_tags(display_name: &str, filename: &str) -> String {
    let tags = extract_tags(filename);
    if tags.is_empty() {
        display_name.to_string()
    } else {
        format!("{display_name} ({tags})")
    }
}

// --- Tag parsing helpers ---

/// Parse revision from tags like "Rev 1", "Rev A", "Rev 2", "REV01", "REV02"
fn parse_revision(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    if lower.starts_with("rev ") {
        let rest = tag[4..].trim();
        if !rest.is_empty() {
            return Some(format!("Rev {rest}"));
        }
    }
    // REV01, REV02 pattern
    if lower.starts_with("rev") && lower.len() >= 5 {
        let rest = &tag[3..];
        if rest.chars().all(|c| c.is_ascii_digit()) {
            let n: u32 = rest.parse().unwrap_or(0);
            return Some(format!("Rev {n}"));
        }
    }
    None
}

/// Parse translation language from parenthesized tags.
///
/// Patterns:
/// - `(Traducido Es)` -> "ES"
/// - `(Traduzido Por)` -> "PT-BR"
/// - `(Translated En)` -> "EN"
/// - `(Translated Fre)` -> "FR"
/// - `(Translated Ger)` -> "DE"
/// - `(Translated Ita)` -> "IT"
/// - `(Translated Swe)` -> "SV"
/// - `(Translated Pol)` -> "PL"
/// - `(Translated Kor)` -> "KO"
/// - `(Translated Rus)` -> "RU"
/// - `(Translated Gre)` -> "EL"
/// - `(Translated Chinese)` -> "ZH"
/// - `(PT-BR)` -> "PT-BR" (standalone)
fn parse_translation_paren(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();

    if lower.starts_with("traducido ") || lower.starts_with("traduccion ") {
        return Some("ES".to_string());
    }
    if lower.starts_with("traduzido ") || lower == "traduzido" {
        return Some("PT-BR".to_string());
    }
    if lower == "pt-br" {
        return Some("PT-BR".to_string());
    }
    if lower.starts_with("translated ") {
        let lang = &tag[11..].trim().to_lowercase();
        return Some(normalize_language(lang));
    }

    None
}

/// Parse translation language from bracketed tags.
///
/// Patterns:
/// - `[T-Spa1.0v_Wave]` -> "ES"
/// - `[T+Fre]` -> "FR"
/// - `[T+Rus Pirate]` -> "RU"
/// - `[T+Bra]` -> "PT-BR"
/// - `[T+Bra_TMT]` -> "PT-BR"
/// - `[T+Ger1.00_Star-trans]` -> "DE"
/// - `[T-Eng v1.2 Zoinkity]` -> "EN"
fn parse_translation_bracket(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();

    // Must start with T+ or T- (translation indicator)
    if !lower.starts_with("t+") && !lower.starts_with("t-") {
        return None;
    }

    let rest = &lower[2..]; // after "T+" or "T-"

    // Extract language code: everything until a digit, space, underscore, or bracket
    let lang_end = rest
        .find(|c: char| c.is_ascii_digit() || c == ' ' || c == '_' || c == ']')
        .unwrap_or(rest.len());
    let lang = &rest[..lang_end];

    if lang.is_empty() {
        return None;
    }

    Some(normalize_language(lang))
}

/// Normalize a language name/code to a short display code.
fn normalize_language(lang: &str) -> String {
    match lang.to_lowercase().as_str() {
        "en" | "eng" | "english" => "EN".to_string(),
        "es" | "spa" | "spanish" | "espanol" => "ES".to_string(),
        "fr" | "fre" | "french" | "fra" => "FR".to_string(),
        "de" | "ger" | "german" | "deu" => "DE".to_string(),
        "it" | "ita" | "italian" => "IT".to_string(),
        "pt" | "por" | "portuguese" => "PT".to_string(),
        "bra" | "pt-br" => "PT-BR".to_string(),
        "ru" | "rus" | "russian" => "RU".to_string(),
        "ja" | "jpn" | "japanese" => "JA".to_string(),
        "ko" | "kor" | "korean" => "KO".to_string(),
        "zh" | "chi" | "chinese" => "ZH".to_string(),
        "sv" | "swe" | "swedish" => "SV".to_string(),
        "pl" | "pol" | "polish" => "PL".to_string(),
        "nl" | "dut" | "dutch" => "NL".to_string(),
        "el" | "gre" | "greek" => "EL".to_string(),
        "no" | "nor" | "norwegian" => "NO".to_string(),
        "da" | "dan" | "danish" => "DA".to_string(),
        "fi" | "fin" | "finnish" => "FI".to_string(),
        "hu" | "hun" | "hungarian" => "HU".to_string(),
        "cs" | "cze" | "czech" => "CS".to_string(),
        "ro" | "rom" | "romanian" => "RO".to_string(),
        "tr" | "tur" | "turkish" => "TR".to_string(),
        "ar" | "ara" | "arabic" => "AR".to_string(),
        "ca" | "cat" | "catalan" => "CA".to_string(),
        other => other.to_uppercase(),
    }
}

/// Normalize a region string for display.
///
/// Expands single-letter region codes used in GoodTools naming:
/// - `(U)` -> "USA"
/// - `(E)` -> "Europe"
/// - `(J)` -> "Japan"
/// - `(W)` -> "World"
/// - `(B)` -> "Brazil"
/// - `(UE)` -> "USA, Europe"
/// - `(JU)` -> "Japan, USA"
/// - `(EB)` -> "Europe, Brazil"
/// - `(UEB)` -> "USA, Europe, Brazil"
/// - `(EBK)` -> "Europe, Brazil, Korea"
/// - `(UEBK)` -> "USA, Europe, Brazil, Korea"
/// - `(JUEBK)` -> "Japan, USA, Europe, Brazil, Korea"
fn normalize_region(region: &str) -> String {
    // Check TOSEC two-letter country codes first (before per-character expansion)
    if let Some(expanded) = expand_tosec_country_code(region) {
        return expanded.to_string();
    }
    // Check if it's a compact GoodTools-style region code (all uppercase letters, <= 5 chars)
    if region.len() <= 5 && region.chars().all(|c| c.is_ascii_uppercase()) {
        let expanded = expand_region_code(region);
        if !expanded.is_empty() {
            return expanded;
        }
    }
    // Otherwise, return as-is (already readable like "USA", "Europe", "USA, Europe", etc.)
    region.to_string()
}

/// Expand compact region codes like "UE" to "USA, Europe".
fn expand_region_code(code: &str) -> String {
    let mut parts = Vec::new();
    for c in code.chars() {
        match c {
            'J' => parts.push("Japan"),
            'U' => parts.push("USA"),
            'E' => parts.push("Europe"),
            'B' => parts.push("Brazil"),
            'K' => parts.push("Korea"),
            'W' => parts.push("World"),
            _ => return String::new(), // Unknown code, don't expand
        }
    }
    parts.join(", ")
}

/// Check if a tag looks like a region (not a translation, revision, etc.)
fn looks_like_region(tag: &str) -> bool {
    let lower = tag.to_lowercase();

    // Known region names (full and abbreviated)
    const REGIONS: &[&str] = &[
        "usa",
        "europe",
        "japan",
        "world",
        "spain",
        "france",
        "germany",
        "italy",
        "brazil",
        "korea",
        "taiwan",
        "china",
        "australia",
        "asia",
        "russia",
        "argentina",
        "netherlands",
        "sweden",
        "scandinavia",
        "uk",
        "canada",
    ];

    // TOSEC two-letter country codes (ALL UPPERCASE only to avoid language code conflicts)
    if is_tosec_country_code(tag) {
        return true;
    }

    // Single-letter/compact codes: U, E, J, W, B, UE, JU, EB, etc.
    if tag.len() <= 5 && tag.chars().all(|c| "JUEBKW".contains(c)) {
        return true;
    }

    // Multi-region like "USA, Europe" or "USA, Europe, Brazil"
    let parts: Vec<&str> = lower.split(',').map(|s| s.trim()).collect();
    if parts
        .iter()
        .all(|p| REGIONS.contains(p) || is_language_code(p))
    {
        // If ALL parts are language codes (like "En,Fr,De"), it's not a region
        if parts.iter().all(|p| is_language_code(p)) {
            return false;
        }
        return true;
    }

    false
}

/// Check if a string is a language code like "en", "fr", "de", etc.
fn is_language_code(s: &str) -> bool {
    matches!(
        s,
        "en" | "fr"
            | "de"
            | "es"
            | "it"
            | "ja"
            | "pt"
            | "nl"
            | "sv"
            | "no"
            | "da"
            | "fi"
            | "ko"
            | "zh"
            | "ru"
            | "pl"
            | "hu"
            | "cs"
            | "ca"
            | "ro"
            | "tr"
            | "ar"
            | "pt-br"
    )
}

/// Check if a tag is a TOSEC two-letter country code (ALL UPPERCASE only).
///
/// Only matches exact two-letter uppercase codes to avoid conflicts with
/// mixed-case language codes like "en", "fr", "de".
fn is_tosec_country_code(tag: &str) -> bool {
    matches!(
        tag,
        "US" | "GB"
            | "JP"
            | "DE"
            | "FR"
            | "ES"
            | "IT"
            | "NL"
            | "SE"
            | "PT"
            | "AU"
            | "BR"
            | "KR"
            | "TW"
            | "CN"
            | "RU"
            | "EU"
    )
}

/// Expand a TOSEC two-letter country code to a full region name.
/// Returns `None` if the code is not a recognized TOSEC country code.
fn expand_tosec_country_code(code: &str) -> Option<&'static str> {
    match code {
        "US" => Some("USA"),
        "GB" => Some("UK"),
        "JP" => Some("Japan"),
        "DE" => Some("Germany"),
        "FR" => Some("France"),
        "ES" => Some("Spain"),
        "IT" => Some("Italy"),
        "NL" => Some("Netherlands"),
        "SE" => Some("Sweden"),
        "PT" => Some("Portugal"),
        "AU" => Some("Australia"),
        "BR" => Some("Brazil"),
        "KR" => Some("Korea"),
        "TW" => Some("Taiwan"),
        "CN" => Some("China"),
        "RU" => Some("Russia"),
        "EU" => Some("Europe"),
        _ => None,
    }
}

/// Try to parse a TOSEC year from a parenthesized tag.
///
/// Recognizes:
/// - Exact 4-digit year: `1987`, `2001` (range 1970-2030)
/// - TOSEC date: `1987-03-15`, `1987-03` — extracts just the year
/// - Unknown/wildcard years: `19xx`, `199x`, `198x` — returns `None`
fn parse_year_tag(tag: &str) -> Option<u16> {
    let lower = tag.to_lowercase();

    // TOSEC full date: YYYY-MM-DD
    if tag.len() == 10
        && tag.as_bytes().get(4) == Some(&b'-')
        && tag.as_bytes().get(7) == Some(&b'-')
    {
        let year_str = &tag[..4];
        if year_str.chars().all(|c| c.is_ascii_digit()) {
            let y: u16 = year_str.parse().ok()?;
            if (1970..=2030).contains(&y) {
                return Some(y);
            }
        }
        return None;
    }

    // TOSEC partial date: YYYY-MM
    if tag.len() == 7 && tag.as_bytes().get(4) == Some(&b'-') {
        let year_str = &tag[..4];
        if year_str.chars().all(|c| c.is_ascii_digit()) {
            let y: u16 = year_str.parse().ok()?;
            if (1970..=2030).contains(&y) {
                return Some(y);
            }
        }
        return None;
    }

    // Must be exactly 4 characters for remaining patterns
    if tag.len() != 4 {
        return None;
    }

    // Skip wildcard years: 19xx, 199x, 198x, etc.
    if lower.ends_with("xx") || lower.ends_with('x') {
        return None;
    }

    // Exact 4-digit year
    if tag.chars().all(|c| c.is_ascii_digit()) {
        let y: u16 = tag.parse().ok()?;
        if (1970..=2030).contains(&y) {
            return Some(y);
        }
    }

    None
}

/// Structured metadata extracted from TOSEC-style filename tags.
///
/// Used by the scan pipeline to populate `GameEntry` fields that aren't
/// available from baked-in game databases.
#[derive(Debug, Clone, Default)]
pub struct TosecMetadata {
    /// Release year extracted from the first parenthesized tag.
    pub year: Option<u16>,
    /// Full date string from the first parenthesized tag (e.g., "2017-08-15", "2017-08", "2017").
    /// More precise than `year` — used for disambiguating display names.
    pub date: Option<String>,
    /// Publisher extracted from the second parenthesized tag (after a year).
    pub publisher: Option<String>,
    /// Disc/Side label for multi-part games (e.g., "Side A", "Disk 1 of 3").
    pub disc_label: Option<String>,
}

/// Extract structured TOSEC metadata from a ROM filename.
///
/// TOSEC convention: `Title (Year)(Publisher)(Country)(Media)...`
///
/// This uses positional logic: the first paren tag is checked for a year,
/// the second (after a year) for a publisher. Publisher extraction only
/// triggers when a year was found, preventing false positives on No-Intro
/// filenames where the first tag is a region.
pub fn extract_tosec_metadata(filename: &str) -> TosecMetadata {
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    let mut meta = TosecMetadata::default();
    let mut tag_index = 0u32;
    let mut found_year = false;

    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        tag_index += 1;
        let lower = trimmed.to_lowercase();

        // First tag: check for year/date (TOSEC naming convention)
        if tag_index == 1
            && let Some(year) = parse_year_tag(trimmed)
        {
            meta.year = Some(year);
            // Store the full date string for disambiguation (e.g., "2017-08-15", "2017-08", "2017")
            meta.date = Some(trimmed.to_string());
            found_year = true;
            continue;
        }

        // Second tag after a year: publisher (TOSEC naming convention)
        if tag_index == 2
            && found_year
            && trimmed != "-"
            && !trimmed.is_empty()
            && !is_noise_tag(&lower)
            && !looks_like_region(trimmed)
        {
            meta.publisher = Some(trimmed.to_string());
            continue;
        }

        // Disc/Side labels
        if lower.starts_with("side ") || lower.starts_with("disc ") || lower.starts_with("disk ") {
            meta.disc_label = Some(trimmed.to_string());
        }
    }

    meta
}

/// Extract a disc/side label from a ROM filename.
///
/// Returns the label text if the filename contains a disc/side pattern:
/// - `(Side A)` -> "Side A"
/// - `(Disk 1 of 3)` -> "Disk 1 of 3"
/// - `(Disc 2)` -> "Disc 2"
pub fn extract_disc_label(filename: &str) -> Option<String> {
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("side ") || lower.starts_with("disc ") || lower.starts_with("disk ") {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Check if a parenthesized tag is noise that should be skipped.
fn is_noise_tag(lower: &str) -> bool {
    // Language-only tags: (En), (Ja), (En,Fr,De), etc.
    let parts: Vec<&str> = lower.split(',').map(|s| s.trim()).collect();
    if parts.iter().all(|p| is_language_code(p)) {
        return true;
    }

    // Distribution/compilation tags
    matches!(
        lower,
        "virtual console"
            | "switch online"
            | "virtual console, switch online"
            | "virtual console, classic mini, switch online"
            | "virtual console, classic mini"
            | "classic mini"
            | "np" // Nintendo Power
            | "bs" // BS-X Satellaview
            | "program"
            | "ntsc"
            | "pal"
            | "sufami turbo"
            | "seganet"
            | "sega channel"
            | "fixed"
            | "alt"
            | "final"
            | "update"
            | "steam"
            | "collection of mana"
            | "mega man x legacy collection"
            | "game no kanzume otokuyou"
            | "pd" // Public Domain
            | "vc" // Virtual Console
            | "j-cart"
            | "nintendo super system"
            | "mame snes bootleg"
            | "unknown"
            // TOSEC copyright status tags
            | "cw" // Copyrighted Freeware
            | "fw" // Freeware
            | "gw" // GNU/GPL
            | "sw" // Shareware
            // TOSEC development status tags (not already handled as status indicators)
            | "alpha"
            | "preview"
            | "pre-release"
    )
}

// --- Iterators for extracting tags ---

/// Iterator over parenthesized tags in a string.
struct ParenTags<'a> {
    remaining: &'a str,
}

impl<'a> ParenTags<'a> {
    fn new(s: &'a str) -> Self {
        Self { remaining: s }
    }
}

impl<'a> Iterator for ParenTags<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let open = self.remaining.find('(')?;
        let after_open = &self.remaining[open + 1..];
        let close = after_open.find(')')?;
        let tag = &after_open[..close];
        self.remaining = &after_open[close + 1..];
        Some(tag)
    }
}

/// Iterator over bracketed tags in a string.
struct BracketTags<'a> {
    remaining: &'a str,
}

impl<'a> BracketTags<'a> {
    fn new(s: &'a str) -> Self {
        Self { remaining: s }
    }
}

impl<'a> Iterator for BracketTags<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let open = self.remaining.find('[')?;
        let after_open = &self.remaining[open + 1..];
        let close = after_open.find(']')?;
        let tag = &after_open[..close];
        self.remaining = &after_open[close + 1..];
        Some(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Region extraction ---

    #[test]
    fn region_usa() {
        assert_eq!(extract_tags("Super Mario World (USA).sfc"), "USA");
    }

    #[test]
    fn region_europe() {
        assert_eq!(extract_tags("Asterix (Europe) (En,Fr,De,Es).sfc"), "Europe");
    }

    #[test]
    fn region_japan() {
        assert_eq!(extract_tags("Bahamut Lagoon (Japan).sfc"), "Japan");
    }

    #[test]
    fn region_world() {
        assert_eq!(extract_tags("Super Mario Bros. (World).nes"), "World");
    }

    #[test]
    fn region_multi() {
        assert_eq!(
            extract_tags("Sonic The Hedgehog (USA, Europe).md"),
            "USA, Europe"
        );
    }

    #[test]
    fn region_goodtools_u() {
        assert_eq!(extract_tags("Game (U).sfc"), "USA");
    }

    #[test]
    fn region_goodtools_ue() {
        assert_eq!(extract_tags("Game (UE).md"), "USA, Europe");
    }

    #[test]
    fn region_goodtools_jue() {
        assert_eq!(extract_tags("Game (JU).md"), "Japan, USA");
    }

    #[test]
    fn region_goodtools_uebk() {
        assert_eq!(
            extract_tags("Game (UEBK).sms"),
            "USA, Europe, Brazil, Korea"
        );
    }

    #[test]
    fn region_spain() {
        assert_eq!(extract_tags("Game (Spain).sfc"), "Spain");
    }

    // --- Revision ---

    #[test]
    fn revision_1() {
        assert_eq!(
            extract_tags("Albert Odyssey (Japan) (Rev 1).sfc"),
            "Japan, Rev 1"
        );
    }

    #[test]
    fn revision_a() {
        assert_eq!(extract_tags("Game (USA) (Rev A).nes"), "USA, Rev A");
    }

    #[test]
    fn revision_rev01() {
        assert_eq!(extract_tags("Game (USA) (REV01).nes"), "USA, Rev 1");
    }

    #[test]
    fn revision_rev00() {
        assert_eq!(extract_tags("Game (USA) (REV00).nes"), "USA, Rev 0");
    }

    // --- Translations (parenthesized) ---

    #[test]
    fn translation_traducido_es() {
        assert_eq!(
            extract_tags("Zelda (USA) (Traducido Es).smc"),
            "USA, ES Translation"
        );
    }

    #[test]
    fn translation_traduzido_por() {
        assert_eq!(
            extract_tags("Game (USA) (Traduzido Por).smc"),
            "USA, PT-BR Translation"
        );
    }

    #[test]
    fn translation_translated_en() {
        assert_eq!(
            extract_tags("Game (Japan) (Translated En).sfc"),
            "Japan, EN Translation"
        );
    }

    #[test]
    fn translation_translated_fre() {
        assert_eq!(
            extract_tags("Game (USA) (Translated Fre).sfc"),
            "USA, FR Translation"
        );
    }

    #[test]
    fn translation_translated_ger() {
        assert_eq!(
            extract_tags("Game (USA) (Translated Ger).sfc"),
            "USA, DE Translation"
        );
    }

    #[test]
    fn translation_pt_br_standalone() {
        assert_eq!(extract_tags("Game (PT-BR).md"), "PT-BR Translation");
    }

    // --- Translations (bracketed) ---

    #[test]
    fn translation_bracket_t_spa() {
        assert_eq!(
            extract_tags("Game (UE) [T-Spa1.0v_Wave].md"),
            "USA, Europe, ES Translation"
        );
    }

    #[test]
    fn translation_bracket_t_fre() {
        assert_eq!(
            extract_tags("Game (E) [T+Fre].sms"),
            "Europe, FR Translation"
        );
    }

    #[test]
    fn translation_bracket_t_rus() {
        assert_eq!(
            extract_tags("Game (UE) [T+Rus Pirate].gen"),
            "USA, Europe, RU Translation"
        );
    }

    #[test]
    fn translation_bracket_t_bra() {
        assert_eq!(
            extract_tags("Game (E) [T+Bra_TMT].sms"),
            "Europe, PT-BR Translation"
        );
    }

    #[test]
    fn translation_bracket_t_eng() {
        assert_eq!(extract_tags("Game (J) T+Eng v1.2 Zoinkity.z64"), "Japan");
        // N64 style doesn't use brackets, so T+Eng isn't picked up from bare text
        // This is fine — the region "Japan" is still useful.
    }

    // --- Patches ---

    #[test]
    fn patch_60hz() {
        assert_eq!(extract_tags("Game (Europe) (60hz).sfc"), "Europe, 60Hz");
    }

    #[test]
    fn patch_fastrom() {
        assert_eq!(extract_tags("Game (USA) (FastRom).sfc"), "USA, FastROM");
    }

    #[test]
    fn combined_fastrom_translation() {
        assert_eq!(
            extract_tags("Game (Japan) (FastRom) (Translated En).sfc"),
            "Japan, EN Translation, FastROM"
        );
    }

    // --- Status indicators ---

    #[test]
    fn hack() {
        assert_eq!(extract_tags("Game (USA) (Hack).sfc"), "USA, Hack");
    }

    #[test]
    fn smw_hack() {
        assert_eq!(extract_tags("Game (SMW Hack).sfc"), "Hack");
    }

    #[test]
    fn beta() {
        assert_eq!(extract_tags("Game (USA) (Beta).sfc"), "USA, Beta");
    }

    #[test]
    fn proto() {
        assert_eq!(extract_tags("Game (USA) (Proto).sfc"), "USA, Proto");
    }

    #[test]
    fn demo() {
        assert_eq!(extract_tags("Game (USA) (Demo).sfc"), "USA, Demo");
    }

    #[test]
    fn unlicensed() {
        assert_eq!(extract_tags("Game (Unl).sfc"), "Unlicensed");
    }

    #[test]
    fn aftermarket() {
        assert_eq!(extract_tags("Game (Aftermarket).sfc"), "Homebrew");
    }

    #[test]
    fn homebrew() {
        assert_eq!(extract_tags("Game (Homebrew).sfc"), "Homebrew");
    }

    #[test]
    fn pirate() {
        assert_eq!(extract_tags("Game (USA) (Pirate).sfc"), "USA, Pirate");
    }

    // --- No tags / noise filtering ---

    #[test]
    fn no_tags() {
        assert_eq!(extract_tags("Super Mario World.sfc"), "");
    }

    #[test]
    fn only_language_codes() {
        assert_eq!(extract_tags("Game (En).sfc"), "");
    }

    #[test]
    fn only_multi_language_codes() {
        assert_eq!(extract_tags("Game (En,Fr,De).sfc"), "");
    }

    #[test]
    fn virtual_console_ignored() {
        assert_eq!(extract_tags("Game (USA) (Virtual Console).sfc"), "USA");
    }

    #[test]
    fn dump_info_ignored() {
        assert_eq!(extract_tags("Game (USA) [!].sfc"), "USA");
    }

    #[test]
    fn dump_bad_shown() {
        assert_eq!(extract_tags("Game (USA) [b1].sfc"), "USA, Bad Dump");
    }

    #[test]
    fn date_ignored() {
        assert_eq!(extract_tags("Game (USA) [2017-03-28].sfc"), "USA");
    }

    // --- display_name_with_tags ---

    #[test]
    fn display_with_tags() {
        assert_eq!(
            display_name_with_tags("Super Mario World", "Super Mario World (USA).sfc"),
            "Super Mario World (USA)"
        );
    }

    #[test]
    fn display_without_tags() {
        assert_eq!(
            display_name_with_tags("Super Mario World", "Super Mario World.sfc"),
            "Super Mario World"
        );
    }

    #[test]
    fn display_with_translation() {
        assert_eq!(
            display_name_with_tags(
                "Super Mario World",
                "Super Mario World (USA) (Traducido Es).smc"
            ),
            "Super Mario World (USA, ES Translation)"
        );
    }

    #[test]
    fn display_with_60hz() {
        assert_eq!(
            display_name_with_tags("Super Mario World", "Super Mario World (Europe) (60hz).sfc"),
            "Super Mario World (Europe, 60Hz)"
        );
    }

    #[test]
    fn display_with_revision() {
        assert_eq!(
            display_name_with_tags("Super Mario World", "Super Mario World (Japan) (Rev 1).sfc"),
            "Super Mario World (Japan, Rev 1)"
        );
    }

    // --- Real-world filenames from the ROM collection ---

    #[test]
    fn real_snes_clean() {
        assert_eq!(extract_tags("ActRaiser (USA).sfc"), "USA");
    }

    #[test]
    fn real_snes_japan_en() {
        assert_eq!(extract_tags("Acrobat Mission (Japan) (En).sfc"), "Japan");
    }

    #[test]
    fn real_snes_europe_multi_lang() {
        assert_eq!(
            extract_tags("Asterix & Obelix (Europe) (En,Fr,De,Es).sfc"),
            "Europe"
        );
    }

    #[test]
    fn real_smd_translation_wave() {
        assert_eq!(
            extract_tags("Addams Family, The (UE) [T-Spa1.0v_Wave].md"),
            "USA, Europe, ES Translation"
        );
    }

    #[test]
    fn real_sms_translation_bracket() {
        assert_eq!(
            extract_tags("Air Rescue (E) [T+Bra_Emutrans].sms"),
            "Europe, PT-BR Translation"
        );
    }

    #[test]
    fn real_snes_fastrom_translation() {
        assert_eq!(
            extract_tags("Actraiser (Japan) (FastRom) (Translated En).sfc"),
            "Japan, EN Translation, FastROM"
        );
    }

    #[test]
    fn real_n64_translation() {
        // N64 translations don't use brackets, they embed in the filename differently
        // "Bomberman 64 - Arcade Edition (J) T+Eng v1.2 Zoinkity.z64"
        // We can at least extract the region
        assert_eq!(
            extract_tags("Bomberman 64 - Arcade Edition (J) T+Eng v1.2 Zoinkity.z64"),
            "Japan"
        );
    }

    #[test]
    fn real_smd_pt_br() {
        assert_eq!(
            extract_tags("Aero the Acro-Bat (PT-BR).md"),
            "PT-BR Translation"
        );
    }

    #[test]
    fn real_snes_europe_brazil() {
        assert_eq!(extract_tags("Game (Europe, Brazil).sfc"), "Europe, Brazil");
    }

    #[test]
    fn real_snes_usa_europe_brazil() {
        assert_eq!(
            extract_tags("Game (USA, Europe, Brazil).sfc"),
            "USA, Europe, Brazil"
        );
    }

    // --- Version tags ---

    #[test]
    fn version_v1_1() {
        // Version tags like (v1.1) are currently treated as noise
        // since they're not common enough to warrant special handling
        assert_eq!(extract_tags("Game (USA) (v1.1).sfc"), "USA");
    }

    // --- TOSEC bracket flag classification ---

    #[test]
    fn tosec_bracket_alternate() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [a].dsk");
        assert_eq!(tier, RomTier::Revision);
    }

    #[test]
    fn tosec_bracket_alternate2() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [a2].dsk");
        assert_eq!(tier, RomTier::Revision);
    }

    #[test]
    fn tosec_bracket_hack() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [h Hack Name].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_hack_bare() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [h].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_cracked() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [cr Cracker].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_cracked_bare() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [cr].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_trained() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [t].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_trained_plus() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [t +2].dsk");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn tosec_bracket_fixed() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [f].dsk");
        assert_eq!(tier, RomTier::Revision);
    }

    #[test]
    fn tosec_bracket_overdump() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [o].dsk");
        assert_eq!(tier, RomTier::Revision);
    }

    #[test]
    fn tosec_bracket_baddump() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [b].dsk");
        assert_eq!(tier, RomTier::Pirate);
    }

    #[test]
    fn tosec_bracket_pirate() {
        let (tier, _, _) = classify("Game (1987)(Publisher) [p].dsk");
        assert_eq!(tier, RomTier::Pirate);
    }

    #[test]
    fn tosec_bracket_verified_unchanged() {
        // [!] is not a flag — it's a verified good dump
        let (tier, _, _) = classify("Game (USA) [!].sfc");
        assert_eq!(tier, RomTier::Original);
    }

    #[test]
    fn tosec_bracket_unknown_tag_ignored() {
        // Custom bracket tags like [joystick] are not TOSEC dump flags
        let (tier, _, _) = classify("Game (1987)(Publisher) [joystick].dsk");
        assert_eq!(tier, RomTier::Original);
    }

    // --- extract_tags for TOSEC bracket flags ---

    #[test]
    fn extract_tags_tosec_trained() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [t].dsk"), "Trained");
    }

    #[test]
    fn extract_tags_tosec_cracked() {
        assert_eq!(
            extract_tags("Game (1987)(Publisher) [cr Cracker].dsk"),
            "Cracked"
        );
    }

    #[test]
    fn extract_tags_tosec_alternate() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [a].dsk"), "Alternate");
    }

    #[test]
    fn extract_tags_tosec_baddump() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [b].dsk"), "Bad Dump");
    }

    // --- extract_tags: TOSEC bracket flag display labels (comprehensive) ---

    #[test]
    fn extract_tags_tosec_alternate_a2() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [a2].dsk"), "Alternate");
    }

    #[test]
    fn extract_tags_tosec_alternate_a3() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [a3].dsk"), "Alternate");
    }

    #[test]
    fn extract_tags_tosec_hack_bare() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [h].dsk"), "Hack");
    }

    #[test]
    fn extract_tags_tosec_hack_named() {
        assert_eq!(
            extract_tags("Game (1987)(Publisher) [h Cool Hack].dsk"),
            "Hack"
        );
    }

    #[test]
    fn extract_tags_tosec_cracked_bare() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [cr].dsk"), "Cracked");
    }

    #[test]
    fn extract_tags_tosec_cracked_cr1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [cr1].dsk"), "Cracked");
    }

    #[test]
    fn extract_tags_tosec_trained_t1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [t1].dsk"), "Trained");
    }

    #[test]
    fn extract_tags_tosec_trained_plus2() {
        assert_eq!(
            extract_tags("Game (1987)(Publisher) [t +2].dsk"),
            "Trained"
        );
    }

    #[test]
    fn extract_tags_tosec_fixed() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [f].dsk"), "Fixed");
    }

    #[test]
    fn extract_tags_tosec_fixed_f1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [f1].dsk"), "Fixed");
    }

    #[test]
    fn extract_tags_tosec_overdump() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [o].dsk"), "Overdump");
    }

    #[test]
    fn extract_tags_tosec_overdump_o1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [o1].dsk"), "Overdump");
    }

    #[test]
    fn extract_tags_tosec_baddump_b1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [b1].dsk"), "Bad Dump");
    }

    #[test]
    fn extract_tags_tosec_pirate() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [p].dsk"), "Pirate");
    }

    #[test]
    fn extract_tags_tosec_pirate_p1() {
        assert_eq!(extract_tags("Game (1987)(Publisher) [p1].dsk"), "Pirate");
    }

    #[test]
    fn extract_tags_tosec_alternate_with_region() {
        // Real TOSEC filename pattern: region + bracket flag
        assert_eq!(
            extract_tags("007 - A View to a Kill (1985)(Domark)(GB) [a].dsk"),
            "UK, Alternate"
        );
    }

    #[test]
    fn extract_tags_tosec_no_intro_hack_bracket_ignored() {
        // No-Intro style [Hack by hosiza] should NOT match TOSEC bracket flags
        // (starts with "ha" not "h ")
        assert_eq!(
            extract_tags("Game (USA) [Hack by hosiza].sfc"),
            "USA"
        );
    }

    // --- has_tosec_bracket_flag ---

    #[test]
    fn has_bracket_flag_alternate() {
        assert!(has_tosec_bracket_flag("Game (1987)(Publisher) [a].dsk"));
    }

    #[test]
    fn has_bracket_flag_trained() {
        assert!(has_tosec_bracket_flag("Game (1987)(Publisher) [t].dsk"));
    }

    #[test]
    fn has_bracket_flag_no_flags() {
        assert!(!has_tosec_bracket_flag("Game (1987)(Publisher).dsk"));
    }

    #[test]
    fn has_bracket_flag_translation_not_flag() {
        assert!(!has_tosec_bracket_flag("Game (E) [T-Spa1.0v_Wave].md"));
    }

    #[test]
    fn has_bracket_flag_custom_not_flag() {
        assert!(!has_tosec_bracket_flag("Game (1987)(Publisher) [joystick].dsk"));
    }

    #[test]
    fn has_bracket_flag_verified_not_flag() {
        assert!(!has_tosec_bracket_flag("Game (USA) [!].sfc"));
    }

    // --- extract_bracket_descriptors ---

    #[test]
    fn bracket_descriptors_custom() {
        let descs = extract_bracket_descriptors("Game (2017-08)(Publisher) [joystick].dsk");
        assert_eq!(descs, vec!["joystick"]);
    }

    #[test]
    fn bracket_descriptors_multiple() {
        let descs =
            extract_bracket_descriptors("Game (2017-08)(Publisher) [experimental] [slow].dsk");
        assert_eq!(descs, vec!["experimental", "slow"]);
    }

    #[test]
    fn bracket_descriptors_skip_standard_flags() {
        let descs =
            extract_bracket_descriptors("Game (2017-08)(Publisher) [a] [joystick].dsk");
        assert_eq!(descs, vec!["joystick"]);
    }

    #[test]
    fn bracket_descriptors_skip_dates() {
        let descs = extract_bracket_descriptors("Game (USA) [2017-03-28].sfc");
        assert!(descs.is_empty());
    }

    #[test]
    fn bracket_descriptors_empty_for_clean() {
        let descs = extract_bracket_descriptors("Game (1987)(Publisher).dsk");
        assert!(descs.is_empty());
    }

    // ==========================================
    // classify() tests
    // ==========================================

    // --- Tier classification ---

    #[test]
    fn classify_original_usa() {
        let (tier, region, _) = classify("Super Mario World (USA).sfc");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::Usa);
    }

    #[test]
    fn classify_original_world() {
        let (tier, region, _) = classify("Tetris (World).nes");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::World);
    }

    #[test]
    fn classify_original_europe() {
        let (tier, region, _) = classify("Game (Europe).sfc");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::Europe);
    }

    #[test]
    fn classify_original_japan() {
        let (tier, region, _) = classify("Game (Japan).sfc");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::Japan);
    }

    #[test]
    fn classify_revision() {
        let (tier, _, _) = classify("Game (USA) (Rev 1).sfc");
        assert_eq!(tier, RomTier::Revision);
    }

    #[test]
    fn classify_region_variant() {
        let (tier, region, _) = classify("Game (Spain).sfc");
        assert_eq!(tier, RomTier::RegionVariant);
        assert_eq!(region, RegionPriority::Other);
    }

    #[test]
    fn classify_translation_paren() {
        let (tier, _, _) = classify("Game (USA) (Traducido Es).sfc");
        assert_eq!(tier, RomTier::Translation);
    }

    #[test]
    fn classify_translation_bracket() {
        let (tier, _, _) = classify("Game (UE) [T-Spa1.0v_Wave].md");
        assert_eq!(tier, RomTier::Translation);
    }

    #[test]
    fn classify_hack() {
        let (tier, _, _) = classify("Game (Hack).sfc");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn classify_smw_hack() {
        let (tier, _, _) = classify("Custom Level (SMW Hack).sfc");
        assert_eq!(tier, RomTier::Hack);
    }

    #[test]
    fn classify_beta() {
        let (tier, _, _) = classify("Game (Beta).sfc");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_proto() {
        let (tier, _, _) = classify("Game (Proto).sfc");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_prototype() {
        let (tier, _, _) = classify("Game (Prototype).sfc");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_demo() {
        let (tier, _, _) = classify("Game (Demo).sfc");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_unlicensed() {
        let (tier, _, _) = classify("Game (Unl).sfc");
        assert_eq!(tier, RomTier::Unlicensed);
    }

    #[test]
    fn classify_homebrew() {
        let (tier, _, _) = classify("Game (Homebrew).sfc");
        assert_eq!(tier, RomTier::Homebrew);
    }

    #[test]
    fn classify_aftermarket() {
        let (tier, _, _) = classify("Game (Aftermarket).sfc");
        assert_eq!(tier, RomTier::Homebrew);
    }

    #[test]
    fn classify_pirate() {
        let (tier, _, _) = classify("Game (Pirate).sfc");
        assert_eq!(tier, RomTier::Pirate);
    }

    // --- Tier priority (pirate > prerelease > hack > homebrew > unlicensed > translation) ---

    #[test]
    fn classify_pirate_beats_hack() {
        // If both (Hack) and (Pirate) are present, Pirate tier should win
        let (tier, _, _) = classify("Game (Hack) (Pirate).sfc");
        assert_eq!(tier, RomTier::Pirate);
    }

    #[test]
    fn classify_prerelease_beats_hack() {
        let (tier, _, _) = classify("Game (Hack) (Beta).sfc");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_hack_beats_translation() {
        let (tier, _, _) = classify("Game (Traducido Es) (Hack).sfc");
        assert_eq!(tier, RomTier::Hack);
    }

    // --- No tags ---

    #[test]
    fn classify_no_tags() {
        let (tier, region, _) = classify("Game.sfc");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::Unknown);
    }

    #[test]
    fn classify_no_extension() {
        let (tier, region, _) = classify("Game (USA)");
        assert_eq!(tier, RomTier::Original);
        assert_eq!(region, RegionPriority::Usa);
    }

    // --- Region priority with GoodTools codes ---

    #[test]
    fn classify_goodtools_u() {
        let (_, region, _) = classify("Game (U).sfc");
        assert_eq!(region, RegionPriority::Usa);
    }

    #[test]
    fn classify_goodtools_e() {
        let (_, region, _) = classify("Game (E).sfc");
        assert_eq!(region, RegionPriority::Europe);
    }

    #[test]
    fn classify_goodtools_j() {
        let (_, region, _) = classify("Game (J).sfc");
        assert_eq!(region, RegionPriority::Japan);
    }

    #[test]
    fn classify_goodtools_w() {
        let (_, region, _) = classify("Game (W).sfc");
        assert_eq!(region, RegionPriority::World);
    }

    // --- Tier ordering is consistent ---

    #[test]
    fn tier_ordering() {
        assert!(RomTier::Original < RomTier::Revision);
        assert!(RomTier::Revision < RomTier::RegionVariant);
        assert!(RomTier::RegionVariant < RomTier::Translation);
        assert!(RomTier::Translation < RomTier::Unlicensed);
        assert!(RomTier::Unlicensed < RomTier::Homebrew);
        assert!(RomTier::Homebrew < RomTier::Hack);
        assert!(RomTier::Hack < RomTier::PreRelease);
        assert!(RomTier::PreRelease < RomTier::Pirate);
    }

    #[test]
    fn region_priority_ordering() {
        assert!(RegionPriority::World < RegionPriority::Usa);
        assert!(RegionPriority::Usa < RegionPriority::Europe);
        assert!(RegionPriority::Europe < RegionPriority::Japan);
        assert!(RegionPriority::Japan < RegionPriority::Other);
        assert!(RegionPriority::Other < RegionPriority::Unknown);
    }

    // --- RegionPreference + sort_key tests (Strategy C: Primary > Secondary > World) ---

    #[test]
    fn sort_key_usa_preference_no_secondary() {
        let pref = RegionPreference::Usa;
        // Primary(Usa)=0, World=1, then remaining majors, Other=4, Unknown=5
        assert_eq!(RegionPriority::Usa.sort_key(pref, None), 0);
        assert_eq!(RegionPriority::World.sort_key(pref, None), 1);
        assert_eq!(RegionPriority::Europe.sort_key(pref, None), 2);
        assert_eq!(RegionPriority::Japan.sort_key(pref, None), 3);
        assert_eq!(RegionPriority::Other.sort_key(pref, None), 4);
        assert_eq!(RegionPriority::Unknown.sort_key(pref, None), 5);
    }

    #[test]
    fn sort_key_europe_preference_no_secondary() {
        let pref = RegionPreference::Europe;
        assert_eq!(RegionPriority::Europe.sort_key(pref, None), 0);
        assert_eq!(RegionPriority::World.sort_key(pref, None), 1);
        assert_eq!(RegionPriority::Usa.sort_key(pref, None), 2);
        assert_eq!(RegionPriority::Japan.sort_key(pref, None), 3);
        assert_eq!(RegionPriority::Other.sort_key(pref, None), 4);
        assert_eq!(RegionPriority::Unknown.sort_key(pref, None), 5);
    }

    #[test]
    fn sort_key_japan_preference_no_secondary() {
        let pref = RegionPreference::Japan;
        assert_eq!(RegionPriority::Japan.sort_key(pref, None), 0);
        assert_eq!(RegionPriority::World.sort_key(pref, None), 1);
        assert_eq!(RegionPriority::Usa.sort_key(pref, None), 2);
        assert_eq!(RegionPriority::Europe.sort_key(pref, None), 3);
        assert_eq!(RegionPriority::Other.sort_key(pref, None), 4);
        assert_eq!(RegionPriority::Unknown.sort_key(pref, None), 5);
    }

    #[test]
    fn sort_key_world_preference_no_secondary() {
        // World preference: World=0, then Usa > Europe > Japan (fallback order).
        let pref = RegionPreference::World;
        assert_eq!(RegionPriority::World.sort_key(pref, None), 0);
        // World is primary, so "remaining" = World=1(but World is primary=0), Usa=2, Europe=3
        // Actually when pref=World: primary=World(0), no secondary, World slot=1 skipped
        // because World IS the primary. Remaining = Usa, Europe, Japan.
        assert_eq!(RegionPriority::Usa.sort_key(pref, None), 2);
        assert_eq!(RegionPriority::Europe.sort_key(pref, None), 3);
    }

    #[test]
    fn sort_key_primary_always_first() {
        // Primary should always get sort_key 0 regardless of preference.
        assert_eq!(RegionPriority::Usa.sort_key(RegionPreference::Usa, None), 0);
        assert_eq!(
            RegionPriority::Europe.sort_key(RegionPreference::Europe, None),
            0
        );
        assert_eq!(
            RegionPriority::Japan.sort_key(RegionPreference::Japan, None),
            0
        );
        assert_eq!(
            RegionPriority::World.sort_key(RegionPreference::World, None),
            0
        );
    }

    #[test]
    fn sort_key_other_and_unknown_always_last() {
        for pref in [
            RegionPreference::Usa,
            RegionPreference::Europe,
            RegionPreference::Japan,
            RegionPreference::World,
        ] {
            assert_eq!(RegionPriority::Other.sort_key(pref, None), 4);
            assert_eq!(RegionPriority::Unknown.sort_key(pref, None), 5);
            assert_eq!(
                RegionPriority::Other.sort_key(pref, Some(RegionPreference::Usa)),
                4
            );
            assert_eq!(
                RegionPriority::Unknown.sort_key(pref, Some(RegionPreference::Usa)),
                5
            );
        }
    }

    // --- Strategy C: Secondary region tests ---

    #[test]
    fn sort_key_japan_primary_usa_secondary() {
        let pref = RegionPreference::Japan;
        let sec = Some(RegionPreference::Usa);
        assert_eq!(RegionPriority::Japan.sort_key(pref, sec), 0); // Primary
        assert_eq!(RegionPriority::Usa.sort_key(pref, sec), 1); // Secondary
        assert_eq!(RegionPriority::World.sort_key(pref, sec), 2); // World
        assert_eq!(RegionPriority::Europe.sort_key(pref, sec), 3); // Remaining
        assert_eq!(RegionPriority::Other.sort_key(pref, sec), 4);
        assert_eq!(RegionPriority::Unknown.sort_key(pref, sec), 5);
    }

    #[test]
    fn sort_key_europe_primary_japan_secondary() {
        let pref = RegionPreference::Europe;
        let sec = Some(RegionPreference::Japan);
        assert_eq!(RegionPriority::Europe.sort_key(pref, sec), 0); // Primary
        assert_eq!(RegionPriority::Japan.sort_key(pref, sec), 1); // Secondary
        assert_eq!(RegionPriority::World.sort_key(pref, sec), 2); // World
        assert_eq!(RegionPriority::Usa.sort_key(pref, sec), 3); // Remaining
        assert_eq!(RegionPriority::Other.sort_key(pref, sec), 4);
        assert_eq!(RegionPriority::Unknown.sort_key(pref, sec), 5);
    }

    #[test]
    fn sort_key_usa_primary_europe_secondary() {
        let pref = RegionPreference::Usa;
        let sec = Some(RegionPreference::Europe);
        assert_eq!(RegionPriority::Usa.sort_key(pref, sec), 0); // Primary
        assert_eq!(RegionPriority::Europe.sort_key(pref, sec), 1); // Secondary
        assert_eq!(RegionPriority::World.sort_key(pref, sec), 2); // World
        assert_eq!(RegionPriority::Japan.sort_key(pref, sec), 3); // Remaining
    }

    #[test]
    fn sort_key_world_primary_japan_secondary() {
        let pref = RegionPreference::World;
        let sec = Some(RegionPreference::Japan);
        assert_eq!(RegionPriority::World.sort_key(pref, sec), 0); // Primary
        assert_eq!(RegionPriority::Japan.sort_key(pref, sec), 1); // Secondary
        // World is already primary, so remaining slots go to Usa, Europe
        assert_eq!(RegionPriority::Usa.sort_key(pref, sec), 3); // Remaining
        assert_eq!(RegionPriority::Europe.sort_key(pref, sec), 3); // Remaining (same slot)
    }

    #[test]
    fn sort_key_secondary_same_as_primary_ignored() {
        // When secondary equals primary, it should behave as if no secondary.
        let pref = RegionPreference::Japan;
        let sec = Some(RegionPreference::Japan);
        assert_eq!(RegionPriority::Japan.sort_key(pref, sec), 0);
        assert_eq!(RegionPriority::World.sort_key(pref, sec), 1);
        assert_eq!(RegionPriority::Usa.sort_key(pref, sec), 2);
        assert_eq!(RegionPriority::Europe.sort_key(pref, sec), 3);
    }

    // --- RegionPreference parsing ---

    #[test]
    fn region_preference_from_str() {
        assert_eq!(
            RegionPreference::from_str_value("usa"),
            RegionPreference::Usa
        );
        assert_eq!(
            RegionPreference::from_str_value("europe"),
            RegionPreference::Europe
        );
        assert_eq!(
            RegionPreference::from_str_value("japan"),
            RegionPreference::Japan
        );
        assert_eq!(
            RegionPreference::from_str_value("world"),
            RegionPreference::World
        );
        // Unknown values default to World.
        assert_eq!(
            RegionPreference::from_str_value("brazil"),
            RegionPreference::World
        );
        assert_eq!(
            RegionPreference::from_str_value(""),
            RegionPreference::World
        );
    }

    #[test]
    fn region_preference_as_str() {
        assert_eq!(RegionPreference::Usa.as_str(), "usa");
        assert_eq!(RegionPreference::Europe.as_str(), "europe");
        assert_eq!(RegionPreference::Japan.as_str(), "japan");
        assert_eq!(RegionPreference::World.as_str(), "world");
    }

    #[test]
    fn region_preference_roundtrip() {
        for pref in [
            RegionPreference::Usa,
            RegionPreference::Europe,
            RegionPreference::Japan,
            RegionPreference::World,
        ] {
            assert_eq!(RegionPreference::from_str_value(pref.as_str()), pref);
        }
    }

    // ==========================================
    // is_special tests
    // ==========================================

    #[test]
    fn classify_fastrom_is_special() {
        let (tier, _, is_special) = classify("Game (USA) (FastRom).sfc");
        assert_eq!(tier, RomTier::Original);
        assert!(is_special, "FastROM patch should be is_special");
    }

    #[test]
    fn classify_60hz_is_special() {
        let (tier, _, is_special) = classify("Game (Europe) (60hz).sfc");
        assert_eq!(tier, RomTier::Original);
        assert!(is_special, "60Hz patch should be is_special");
    }

    #[test]
    fn classify_sample_is_prerelease() {
        let (tier, _, is_special) = classify("Game (Sample).sfc");
        assert_eq!(tier, RomTier::PreRelease);
        assert!(is_special, "Sample should be PreRelease and is_special");
    }

    #[test]
    fn classify_unlicensed_is_special() {
        let (_, _, is_special) = classify("Game (Unl).sfc");
        assert!(is_special, "Unlicensed should be is_special");
    }

    #[test]
    fn classify_homebrew_is_special() {
        let (_, _, is_special) = classify("Game (Homebrew).sfc");
        assert!(is_special, "Homebrew should be is_special");
    }

    #[test]
    fn classify_prerelease_is_special() {
        let (_, _, is_special) = classify("Game (Beta).sfc");
        assert!(is_special, "PreRelease should be is_special");
    }

    #[test]
    fn classify_pirate_is_special() {
        let (_, _, is_special) = classify("Game (Pirate).sfc");
        assert!(is_special, "Pirate should be is_special");
    }

    #[test]
    fn classify_original_not_special() {
        let (_, _, is_special) = classify("Super Mario World (USA).sfc");
        assert!(!is_special, "Original should NOT be is_special");
    }

    #[test]
    fn classify_revision_not_special() {
        let (_, _, is_special) = classify("Game (USA) (Rev 1).sfc");
        assert!(!is_special, "Revision should NOT be is_special");
    }

    #[test]
    fn classify_translation_not_special() {
        let (_, _, is_special) = classify("Game (USA) (Traducido Es).sfc");
        assert!(!is_special, "Translation should NOT be is_special");
    }

    #[test]
    fn classify_hack_not_special() {
        let (_, _, is_special) = classify("Game (Hack).sfc");
        assert!(!is_special, "Hack should NOT be is_special");
    }

    // ==========================================
    // parse_year_tag() tests
    // ==========================================

    #[test]
    fn year_exact_4digit() {
        assert_eq!(parse_year_tag("1987"), Some(1987));
        assert_eq!(parse_year_tag("2001"), Some(2001));
        assert_eq!(parse_year_tag("1970"), Some(1970));
        assert_eq!(parse_year_tag("2030"), Some(2030));
    }

    #[test]
    fn year_out_of_range() {
        assert_eq!(parse_year_tag("1969"), None);
        assert_eq!(parse_year_tag("2031"), None);
        assert_eq!(parse_year_tag("1800"), None);
    }

    #[test]
    fn year_tosec_date_full() {
        assert_eq!(parse_year_tag("1987-03-15"), Some(1987));
        assert_eq!(parse_year_tag("2001-12-25"), Some(2001));
    }

    #[test]
    fn year_tosec_date_partial() {
        assert_eq!(parse_year_tag("1987-03"), Some(1987));
        assert_eq!(parse_year_tag("2001-12"), Some(2001));
    }

    #[test]
    fn year_wildcard_skip() {
        assert_eq!(parse_year_tag("19xx"), None);
        assert_eq!(parse_year_tag("199x"), None);
        assert_eq!(parse_year_tag("198x"), None);
    }

    #[test]
    fn year_non_year_strings() {
        assert_eq!(parse_year_tag("USA"), None);
        assert_eq!(parse_year_tag("Rev 1"), None);
        assert_eq!(parse_year_tag("v1.02"), None);
        assert_eq!(parse_year_tag("Imagine"), None);
    }

    // ==========================================
    // extract_tosec_metadata() tests
    // ==========================================

    #[test]
    fn tosec_year_publisher_region() {
        let meta = extract_tosec_metadata("Arkanoid (1987)(Imagine)(GB)(Side A).dsk");
        assert_eq!(meta.year, Some(1987));
        assert_eq!(meta.publisher.as_deref(), Some("Imagine"));
        assert_eq!(meta.disc_label.as_deref(), Some("Side A"));
    }

    #[test]
    fn tosec_year_publisher_no_side() {
        let meta = extract_tosec_metadata("Commando (1985)(Elite)(GB).dsk");
        assert_eq!(meta.year, Some(1985));
        assert_eq!(meta.publisher.as_deref(), Some("Elite"));
        assert!(meta.disc_label.is_none());
    }

    #[test]
    fn tosec_no_year_no_publisher() {
        // No-Intro style: first tag is region, not year — no publisher extraction
        let meta = extract_tosec_metadata("Super Mario World (USA).sfc");
        assert!(meta.year.is_none());
        assert!(meta.publisher.is_none());
    }

    #[test]
    fn tosec_year_with_date() {
        let meta = extract_tosec_metadata("Game (1987-03-15)(Publisher)(JP).rom");
        assert_eq!(meta.year, Some(1987));
        assert_eq!(meta.publisher.as_deref(), Some("Publisher"));
    }

    #[test]
    fn tosec_year_partial_date() {
        let meta = extract_tosec_metadata("Game (1987-03)(Publisher)(JP).rom");
        assert_eq!(meta.year, Some(1987));
        assert_eq!(meta.publisher.as_deref(), Some("Publisher"));
    }

    #[test]
    fn tosec_unknown_publisher_dash() {
        // TOSEC uses (-) for unknown publisher
        let meta = extract_tosec_metadata("Game (1987)(-)(JP).rom");
        assert_eq!(meta.year, Some(1987));
        assert!(meta.publisher.is_none());
    }

    #[test]
    fn tosec_publisher_is_region_skip() {
        // When second tag is a region, don't extract as publisher
        let meta = extract_tosec_metadata("Game (1987)(JP).rom");
        assert_eq!(meta.year, Some(1987));
        assert!(meta.publisher.is_none());
    }

    #[test]
    fn tosec_disk_label() {
        let meta = extract_tosec_metadata("Game (1989)(System Sacom)(Disk 1 of 5).dim");
        assert_eq!(meta.year, Some(1989));
        assert_eq!(meta.publisher.as_deref(), Some("System Sacom"));
        assert_eq!(meta.disc_label.as_deref(), Some("Disk 1 of 5"));
    }

    #[test]
    fn tosec_disc_label() {
        let meta = extract_tosec_metadata("Panzer Dragoon Saga (USA) (Disc 2).chd");
        // No year in first tag (USA is first), so no publisher extraction
        assert!(meta.year.is_none());
        assert_eq!(meta.disc_label.as_deref(), Some("Disc 2"));
    }

    #[test]
    fn tosec_wildcard_year_no_extract() {
        let meta = extract_tosec_metadata("Game (19xx)(Publisher)(JP).rom");
        assert!(meta.year.is_none());
        // No year found, so publisher extraction doesn't trigger
        assert!(meta.publisher.is_none());
    }

    // ==========================================
    // TOSEC noise tag tests
    // ==========================================

    #[test]
    fn noise_tosec_copyright_tags() {
        assert!(is_noise_tag("cw"));
        assert!(is_noise_tag("fw"));
        assert!(is_noise_tag("gw"));
        assert!(is_noise_tag("sw"));
    }

    #[test]
    fn noise_tosec_devstatus_tags() {
        assert!(is_noise_tag("alpha"));
        assert!(is_noise_tag("preview"));
        assert!(is_noise_tag("pre-release"));
    }

    // ==========================================
    // classify() with TOSEC pre-release status
    // ==========================================

    // ==========================================
    // extract_disc_label() tests
    // ==========================================

    #[test]
    fn disc_label_side_a() {
        assert_eq!(
            extract_disc_label("Arkanoid (1987)(Imagine)(GB)(Side A).dsk"),
            Some("Side A".to_string())
        );
    }

    #[test]
    fn disc_label_disk_1_of_5() {
        assert_eq!(
            extract_disc_label("Game (1989)(System Sacom)(Disk 1 of 5).dim"),
            Some("Disk 1 of 5".to_string())
        );
    }

    #[test]
    fn disc_label_disc_2() {
        assert_eq!(
            extract_disc_label("Panzer Dragoon Saga (USA) (Disc 2).chd"),
            Some("Disc 2".to_string())
        );
    }

    #[test]
    fn disc_label_none() {
        assert!(extract_disc_label("Super Mario World (USA).sfc").is_none());
    }

    #[test]
    fn classify_alpha_prerelease() {
        let (tier, _, _) = classify("Game (alpha).dsk");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_preview_prerelease() {
        let (tier, _, _) = classify("Game (preview).dsk");
        assert_eq!(tier, RomTier::PreRelease);
    }

    #[test]
    fn classify_pre_release_prerelease() {
        let (tier, _, _) = classify("Game (pre-release).dsk");
        assert_eq!(tier, RomTier::PreRelease);
    }
}
