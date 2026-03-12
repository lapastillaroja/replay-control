use serde::{Deserialize, Serialize};

use crate::arcade_db;
use crate::game_db;
use crate::rom_tags;
use crate::systems::{self, SystemCategory};

/// A reference to a game — the common identity shared across ROM listings,
/// favorites, and recents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRef {
    /// System folder name (e.g., "nintendo_nes")
    pub system: String,
    /// Display name of the system (e.g., "Nintendo Entertainment System")
    pub system_display: String,
    /// ROM filename (e.g., "Super Mario Bros (USA).nes")
    pub rom_filename: String,
    /// Human-readable display name resolved from arcade DB or game DB.
    pub display_name: Option<String>,
    /// Path to the ROM file relative to the storage root
    pub rom_path: String,
}

/// Strip parenthesized/bracketed tags from a filename stem.
/// `"Indiana Jones and the Fate of Atlantis (Spanish)"` → `"Indiana Jones and the Fate of Atlantis"`
fn strip_filename_tags(stem: &str) -> &str {
    stem.find(" (")
        .or_else(|| stem.find(" ["))
        .map(|i| stem[..i].trim())
        .unwrap_or(stem)
}

/// Un-invert a comma-separated trailing article ("Title, The" → "The Title").
///
/// Common in X68000 ROM sets and other collections that sort by title rather than
/// article. Only handles short trailing articles (The, A, An, Les, La, Le, Der,
/// Die, Das, El, Los, Las) to avoid false positives.
fn uninvert_article(name: &str) -> Option<String> {
    let (title, article) = name.rsplit_once(", ")?;
    // Only reorder known short articles to avoid mangling names with commas
    // in other contexts (e.g., "Samurai Shodown II, The" but not "Ace, Jack").
    const ARTICLES: &[&str] = &[
        "The", "A", "An", // English
        "Les", "La", "Le", "L'", // French
        "Der", "Die", "Das", // German
        "El", "Los", "Las", // Spanish
    ];
    let article_trimmed = article.trim();
    if ARTICLES
        .iter()
        .any(|a| a.eq_ignore_ascii_case(article_trimmed))
    {
        Some(format!("{article_trimmed} {title}"))
    } else {
        None
    }
}

impl GameRef {
    /// Create a new GameRef, resolving display_name from the appropriate DB.
    ///
    /// For arcade systems, uses the arcade DB (zip filename lookup).
    /// For non-arcade systems with game DB coverage, uses the game DB
    /// (No-Intro filename lookup).
    pub fn new(system: &str, rom_filename: String, rom_path: String) -> Self {
        let sys_info = systems::find_system(system);

        let system_display = sys_info
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let display_name = if sys_info.is_some_and(|s| s.category == SystemCategory::Arcade) {
            // Arcade: use arcade DB (zip filename without extension)
            let resolved = arcade_db::arcade_display_name(&rom_filename);
            if resolved != rom_filename {
                Some(resolved.to_string())
            } else {
                None
            }
        } else {
            // Non-arcade: try game DB first, then append useful filename tags.
            // Fall back to deriving a clean name from the filename for systems
            // without game DB coverage (e.g., ibm_pc, commodore_ami) or unknown systems.
            game_db::game_display_name(system, &rom_filename)
                .map(|name| rom_tags::display_name_with_tags(name, &rom_filename))
                .or_else(|| {
                    let stem = rom_filename
                        .rfind('.')
                        .map(|i| &rom_filename[..i])
                        .unwrap_or(&rom_filename);
                    // Always return a display name from the stem (without extension).
                    // Use the tag-stripped base as the display name, or the full
                    // stem if there are no tags to strip.
                    let base = strip_filename_tags(stem);
                    let name = if base.is_empty() { stem } else { base };
                    // Un-invert comma-separated trailing articles:
                    // "4th Unit Act 2, The" → "The 4th Unit Act 2"
                    let uninverted = uninvert_article(name);
                    let name = uninverted.as_deref().unwrap_or(name);
                    Some(rom_tags::display_name_with_tags(name, &rom_filename))
                })
        };

        Self {
            system: system.to_string(),
            system_display,
            rom_filename,
            display_name,
            rom_path,
        }
    }

    /// Create a GameRef with a pre-resolved display name (from cache).
    /// Skips the DB lookup — useful when restoring from SQLite rom_cache.
    pub fn new_with_display(
        system: &str,
        rom_filename: String,
        rom_path: String,
        display_name: Option<String>,
    ) -> Self {
        let system_display = systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        Self {
            system: system.to_string(),
            system_display,
            rom_filename,
            display_name,
            rom_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninvert_the() {
        assert_eq!(
            uninvert_article("4th Unit Act 2, The"),
            Some("The 4th Unit Act 2".to_string())
        );
    }

    #[test]
    fn uninvert_a() {
        assert_eq!(
            uninvert_article("Link to the Past, A"),
            Some("A Link to the Past".to_string())
        );
    }

    #[test]
    fn uninvert_case_insensitive() {
        assert_eq!(
            uninvert_article("Legend of Zelda, the"),
            Some("the Legend of Zelda".to_string())
        );
    }

    #[test]
    fn uninvert_french_article() {
        assert_eq!(
            uninvert_article("Aventures de Tintin, Les"),
            Some("Les Aventures de Tintin".to_string())
        );
    }

    #[test]
    fn uninvert_spanish_article() {
        assert_eq!(
            uninvert_article("Caballeros del Zodiaco, Los"),
            Some("Los Caballeros del Zodiaco".to_string())
        );
    }

    #[test]
    fn uninvert_no_article() {
        // "Jack" is not a known article — should not reorder
        assert_eq!(uninvert_article("Ace, Jack"), None);
    }

    #[test]
    fn uninvert_no_comma() {
        assert_eq!(uninvert_article("Simple Title"), None);
    }

    #[test]
    fn uninvert_empty() {
        assert_eq!(uninvert_article(""), None);
    }

    #[test]
    fn display_name_uninverts_m3u() {
        // X68000 M3U file with comma-inverted title
        let game_ref = GameRef::new(
            "sharp_x68k",
            "4th Unit Act 2, The.m3u".to_string(),
            "/roms/sharp_x68k/4th Unit Act 2, The.m3u".to_string(),
        );
        assert_eq!(game_ref.display_name.as_deref(), Some("The 4th Unit Act 2"));
    }

    #[test]
    fn display_name_uninverts_dim() {
        // X68000 .dim file with comma-inverted title and tags
        let game_ref = GameRef::new(
            "sharp_x68k",
            "Emerald Dragon, The (1990)(Glodia)(Disk 1 of 5).dim".to_string(),
            "/roms/sharp_x68k/Emerald Dragon, The (1990)(Glodia)(Disk 1 of 5).dim".to_string(),
        );
        // strip_filename_tags removes "(1990)..." part, leaving "Emerald Dragon, The"
        // uninvert_article turns it into "The Emerald Dragon"
        assert_eq!(game_ref.display_name.as_deref(), Some("The Emerald Dragon"));
    }

    #[test]
    fn display_name_no_comma_no_change() {
        // Regular filename without comma inversion
        let game_ref = GameRef::new(
            "sharp_x68k",
            "Alshark.m3u".to_string(),
            "/roms/sharp_x68k/Alshark.m3u".to_string(),
        );
        assert_eq!(game_ref.display_name.as_deref(), Some("Alshark"));
    }
}
