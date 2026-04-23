//! Retrokit manual index: TSV-based deterministic manual resolution.
//!
//! The retrokit-manuals Archive.org collection provides per-system TSV files
//! mapping game titles to manual download URLs. This module loads and caches
//! these indexes for instant, zero-search-latency manual lookups.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A single manual source from a retrokit TSV entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualSource {
    /// Parent title as it appears in the TSV (e.g., "Super Mario World")
    pub title: String,
    /// Language code(s): "en", "ja", "en-gb,de,es,fr,it"
    pub language: String,
    /// Direct download URL for the manual PDF
    pub url: String,
}

/// A manual recommendation returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualRecommendation {
    /// Source: "retrokit" or "archive.org"
    pub source: String,
    /// Display title
    pub title: String,
    /// Direct URL to the PDF file
    pub url: String,
    /// File size in bytes (not available from retrokit TSV)
    pub size_bytes: Option<u64>,
    /// Language code(s): "en", "ja", "en-gb,de,es,fr,it"
    pub language: Option<String>,
    /// Archive.org item identifier (for attribution)
    pub source_id: String,
}

/// Parsed retrokit TSV index for one system.
/// Keys are normalized parent titles (lowercase, article-reordered).
pub type RetrokitIndex = HashMap<String, Vec<ManualSource>>;

/// Parse a retrokit TSV string into an index.
///
/// TSV format (no header, tab-separated):
/// ```text
/// parent_title\tlanguage\tsource_url
/// ```
pub fn parse_retrokit_tsv(tsv_data: &str) -> RetrokitIndex {
    let mut index = RetrokitIndex::new();

    for line in tsv_data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let title = parts[0].trim();
        let language = parts[1].trim();
        let url = parts[2].trim();

        if title.is_empty() || url.is_empty() {
            continue;
        }

        let normalized = normalize_retrokit_title(title);
        let source = ManualSource {
            title: title.to_string(),
            language: language.to_string(),
            url: url.to_string(),
        };

        index.entry(normalized).or_default().push(source);
    }

    index
}

/// Normalize a title for retrokit index lookup.
///
/// Lowercases, handles trailing article reordering ("Addams Family, The" ->
/// "the addams family"), and collapses whitespace.
pub fn normalize_retrokit_title(title: &str) -> String {
    let trimmed = title.trim();
    let lower = trimmed.to_lowercase();

    // Handle trailing articles: "Legend of Zelda, The" -> "the legend of zelda"
    for article in &[", the", ", an", ", a"] {
        if let Some(base) = lower.strip_suffix(article) {
            let art = &article[2..]; // skip ", "
            return format!("{art} {base}");
        }
    }

    lower
}

/// Map our system IDs to retrokit folder names.
pub fn retrokit_folder_name(system: &str) -> Option<&'static str> {
    Some(match system {
        "nintendo_snes" => "snes",
        "nintendo_nes" => "nes",
        "nintendo_gb" => "gb",
        "nintendo_gba" => "gba",
        "nintendo_gbc" => "gbc",
        "nintendo_n64" => "n64",
        "nintendo_ds" => "nds",
        "sega_smd" => "megadrive",
        "sega_sms" => "mastersystem",
        "sega_gg" => "gamegear",
        "sega_32x" => "sega32x",
        "sega_cd" => "segacd",
        "sega_dc" => "dreamcast",
        "sega_st" => "saturn",
        "sega_sg" => "sg-1000",
        "sony_psx" => "psx",
        "nec_pce" => "pcengine",
        "nec_pcecd" => "pce-cd",
        "atari_2600" => "atari2600",
        "atari_5200" => "atari5200",
        "atari_7800" => "atari7800",
        "atari_jaguar" => "atarijaguar",
        "atari_lynx" => "atarilynx",
        "commodore_c64" => "c64",
        "commodore_ami" => "amiga",
        "snk_ng" => "neogeo",
        "snk_ngcd" => "neogeocd",
        "snk_ngp" => "ngp",
        "panasonic_3do" => "3do",
        "ibm_pc" | "scummvm" => "pc",
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc" => "arcade",
        _ => return None,
    })
}

/// Map our system IDs to platform search terms for Archive.org fallback.
pub fn platform_search_terms(system: &str) -> &'static str {
    match system {
        "nintendo_snes" => "SNES OR Super Nintendo OR Super Famicom",
        "nintendo_nes" => "NES OR Nintendo Entertainment System OR Famicom",
        "nintendo_gb" => "Game Boy",
        "nintendo_gba" => "Game Boy Advance OR GBA",
        "nintendo_gbc" => "Game Boy Color",
        "nintendo_n64" => "N64 OR Nintendo 64",
        "nintendo_ds" => "Nintendo DS OR NDS",
        "sega_smd" => "Genesis OR Mega Drive",
        "sega_sms" => "Master System",
        "sega_gg" => "Game Gear",
        "sega_dc" => "Dreamcast",
        "sega_st" => "Saturn",
        "sony_psx" => "PlayStation OR PSX OR PS1",
        "nec_pce" => "PC Engine OR TurboGrafx",
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc" => "Arcade",
        _ => "",
    }
}

/// Map our system IDs to manual folder names (for `<storage>/manuals/<folder>/`).
/// Same as retrokit_folder_name but used for local storage.
pub fn manual_folder_name(system: &str) -> &str {
    retrokit_folder_name(system).unwrap_or(system)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsv_basic() {
        let tsv = "Super Mario World\ten\thttps://example.com/smw.pdf\n\
                    ActRaiser\ten\thttps://example.com/actraiser.pdf\n\
                    ActRaiser\tja\thttps://example.com/actraiser_ja.pdf\n";
        let index = parse_retrokit_tsv(tsv);
        assert_eq!(index.len(), 2);
        assert_eq!(index["super mario world"].len(), 1);
        assert_eq!(index["actraiser"].len(), 2);
    }

    #[test]
    fn parse_tsv_trailing_article() {
        let tsv = "Addams Family, The\ten\thttps://example.com/af.pdf\n";
        let index = parse_retrokit_tsv(tsv);
        assert!(index.contains_key("the addams family"));
    }

    #[test]
    fn normalize_title_basic() {
        assert_eq!(
            normalize_retrokit_title("Super Mario World"),
            "super mario world"
        );
    }

    #[test]
    fn normalize_title_trailing_article() {
        assert_eq!(
            normalize_retrokit_title("Legend of Zelda, The"),
            "the legend of zelda"
        );
    }

    #[test]
    fn retrokit_folder_snes() {
        assert_eq!(retrokit_folder_name("nintendo_snes"), Some("snes"));
    }

    #[test]
    fn retrokit_folder_unknown() {
        assert_eq!(retrokit_folder_name("unknown_system"), None);
    }

    #[test]
    fn retrokit_folder_scummvm_maps_to_pc() {
        assert_eq!(retrokit_folder_name("scummvm"), Some("pc"));
    }

    #[test]
    fn platform_terms_snes() {
        assert!(platform_search_terms("nintendo_snes").contains("SNES"));
    }
}
