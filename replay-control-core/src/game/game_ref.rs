//! `GameRef` — the common identity for a ROM across listings, favorites,
//! and recents. Pure type + pure display-name processing. The native DB
//! lookup variant (`replay_control_core_server::game_ref::new`) lives in
//! core-server alongside the arcade/game DB queries it depends on.

use serde::{Deserialize, Serialize};

use crate::rom_tags;
use crate::systems;
use crate::title_utils;

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
    /// Build a `GameRef` from a pre-resolved catalog name (or `None` if the
    /// catalog had no match). Applies tag/disc-label/article processing.
    pub fn from_parts(
        system: &str,
        rom_filename: String,
        rom_path: String,
        resolved_name: Option<String>,
    ) -> Self {
        let display_name = if systems::is_arcade_system(system) {
            resolved_name
        } else {
            Some(compute_console_display_name(
                resolved_name.as_deref(),
                &rom_filename,
            ))
        };

        Self {
            system: system.to_string(),
            system_display: system_display_name(system),
            rom_filename,
            display_name,
            rom_path,
        }
    }

    /// Create a GameRef with a pre-resolved display name (from cache).
    /// Skips the DB lookup — useful when restoring from the game library.
    pub fn new_with_display(
        system: &str,
        rom_filename: String,
        rom_path: String,
        display_name: Option<String>,
    ) -> Self {
        Self {
            system: system.to_string(),
            system_display: system_display_name(system),
            rom_filename,
            display_name,
            rom_path,
        }
    }
}

fn system_display_name(system: &str) -> String {
    systems::find_system(system)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.to_string())
}

/// Build the final display string for a non-arcade ROM given the catalog's
/// resolved canonical name (or `None` if no catalog match). Handles tag
/// passthrough (region, revision, disc labels) and falls back to filename
/// stem processing (article inversion, version stripping) when the catalog
/// has no match.
fn compute_console_display_name(resolved: Option<&str>, rom_filename: &str) -> String {
    let base_name: String = match resolved {
        Some(name) => name.to_string(),
        None => {
            let stem = title_utils::filename_stem(rom_filename);
            let base = strip_filename_tags(stem);
            let name = if base.is_empty() { stem } else { base };
            let uninverted = uninvert_article(name);
            let name = uninverted.as_deref().unwrap_or(name);
            title_utils::strip_version(name).to_string()
        }
    };
    let mut display = rom_tags::display_name_with_tags(&base_name, rom_filename);
    if let Some(label) = rom_tags::extract_disc_label(rom_filename) {
        display.push_str(" [");
        display.push_str(&label);
        display.push(']');
    }
    display
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
}
