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
///
/// Tags NOT shown (noise for end users):
/// - Dump info: [!], [b1], [h1], [o1], [f1], [c], etc.
/// - Version dates: [2017-03-28]
/// - Hacker credits in brackets: [T-Spa1.0v_Wave] -> shown as "ES" not the full tag
/// - Language codes already in the region: (En,Fr,De) merged with region

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

/// Classify a ROM filename into a tier and region priority for sorting.
pub fn classify(filename: &str) -> (RomTier, RegionPriority) {
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
    let mut is_unlicensed = false;
    let mut is_aftermarket = false;
    let mut is_pirate = false;

    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();

        if lower.starts_with("rev ") || (lower.starts_with("rev") && trimmed.len() <= 6) {
            if parse_revision(trimmed).is_some() {
                has_revision = true;
                continue;
            }
        }
        if parse_translation_paren(trimmed).is_some() {
            has_translation = true;
            continue;
        }
        if lower == "hack" || lower == "smw hack" || lower == "sa-1 smw hack"
            || lower == "smw2 hack" || lower == "smrpg hack" || lower == "smk hack"
            || lower.ends_with(" hack")
        {
            is_hack = true;
            continue;
        }
        if lower == "beta" || lower.starts_with("beta ") {
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
        if !is_noise_tag(&lower) && looks_like_region(trimmed) {
            has_region = true;
            region_priority = region_to_priority(trimmed);
        }
    }

    // Check bracketed translation tags too
    for tag in BracketTags::new(stem) {
        if parse_translation_bracket(tag.trim()).is_some() {
            has_translation = true;
        }
    }

    let tier = if is_pirate {
        RomTier::Pirate
    } else if is_beta || is_proto || is_demo {
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

    (tier, region_priority)
}

/// Map a region tag to a sort priority.
fn region_to_priority(tag: &str) -> RegionPriority {
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
    let mut is_unlicensed = false;
    let mut is_aftermarket = false;
    let mut is_pirate = false;

    // Extract all parenthesized tags: (...)
    for tag in ParenTags::new(stem) {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();

        // Revision tags
        if lower.starts_with("rev ") || lower.starts_with("rev") && trimmed.len() <= 6 {
            if let Some(rev) = parse_revision(trimmed) {
                revision = Some(rev);
                continue;
            }
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
        if lower == "beta" || lower.starts_with("beta ") {
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

        // Skip dump info: [!], [b1], [b], [h1], [o1], [f1], [c], [p1], etc.
        // Skip dates: [2017-03-28]
        // These are all noise for the user.
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
    if is_beta {
        parts.push("Beta".to_string());
    }
    if is_proto {
        parts.push("Proto".to_string());
    }
    if is_demo {
        parts.push("Demo".to_string());
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

    // Single-letter/compact codes: U, E, J, W, B, UE, JU, EB, etc.
    if tag.len() <= 5 && tag.chars().all(|c| "JUEBKW".contains(c)) {
        return true;
    }

    // Multi-region like "USA, Europe" or "USA, Europe, Brazil"
    let parts: Vec<&str> = lower.split(',').map(|s| s.trim()).collect();
    if parts.iter().all(|p| {
        REGIONS.contains(p)
            || is_language_code(p)
    }) {
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
        "en" | "fr" | "de" | "es" | "it" | "ja" | "pt" | "nl" | "sv" | "no" | "da" | "fi"
            | "ko" | "zh" | "ru" | "pl" | "hu" | "cs" | "ca" | "ro" | "tr" | "ar" | "pt-br"
    )
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
            | "sample"
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
        assert_eq!(
            extract_tags("Game (J) T+Eng v1.2 Zoinkity.z64"),
            "Japan"
        );
        // N64 style doesn't use brackets, so T+Eng isn't picked up from bare text
        // This is fine — the region "Japan" is still useful.
    }

    // --- Patches ---

    #[test]
    fn patch_60hz() {
        assert_eq!(
            extract_tags("Game (Europe) (60hz).sfc"),
            "Europe, 60Hz"
        );
    }

    #[test]
    fn patch_fastrom() {
        assert_eq!(
            extract_tags("Game (USA) (FastRom).sfc"),
            "USA, FastROM"
        );
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
        assert_eq!(
            extract_tags("Game (USA) (Virtual Console).sfc"),
            "USA"
        );
    }

    #[test]
    fn dump_info_ignored() {
        assert_eq!(extract_tags("Game (USA) [!].sfc"), "USA");
    }

    #[test]
    fn dump_bad_ignored() {
        assert_eq!(extract_tags("Game (USA) [b1].sfc"), "USA");
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
            display_name_with_tags("Super Mario World", "Super Mario World (USA) (Traducido Es).smc"),
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
        assert_eq!(
            extract_tags("ActRaiser (USA).sfc"),
            "USA"
        );
    }

    #[test]
    fn real_snes_japan_en() {
        assert_eq!(
            extract_tags("Acrobat Mission (Japan) (En).sfc"),
            "Japan"
        );
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
        assert_eq!(
            extract_tags("Game (Europe, Brazil).sfc"),
            "Europe, Brazil"
        );
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
}
