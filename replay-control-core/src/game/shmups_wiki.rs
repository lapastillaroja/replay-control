//! Shmups Wiki cross-reference index.
//!
//! Maps a normalized game title to the exact wiki page title and exposes a
//! deep-link helper into `https://shmups.wiki/library/`. The index is bundled
//! from `data/shmups-wiki/games.json`, refreshed by
//! `scripts/shmups-wiki-extract.py`. Source licensing (CC BY-SA 4.0 → GPL
//! one-way compatibility) is documented in `NOTICES.md`.

use std::collections::HashMap;
use std::sync::OnceLock;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;

use super::title_utils::normalize_title_for_metadata;

const INDEX_JSON: &str = include_str!("../../../data/shmups-wiki/games.json");

/// Bytes that must be percent-encoded in a MediaWiki path segment.
///
/// Starts from `CONTROLS` (ASCII control bytes) and adds every byte that
/// is not part of the RFC 3986 unreserved set or one of the few extras
/// (`!()*',`) MediaWiki accepts unescaped. The complement defines our
/// path-safe set.
const MEDIAWIKI_PATH_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Debug, Deserialize)]
struct IndexEntry {
    normalized_title: String,
    page_title: String,
}

/// Canonical page metadata for a Shmups Wiki match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShmupsWikiPage {
    /// Wiki article title (e.g. `"RayForce"`). Stable enough to use as a
    /// resource-row identifier; survives across refreshes unless the wiki
    /// itself renames the page.
    pub page_title: String,
    /// Full deep link including the `https://shmups.wiki/library/` prefix.
    pub url: String,
}

fn index() -> &'static HashMap<String, String> {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let entries: Vec<IndexEntry> = serde_json::from_str(INDEX_JSON).unwrap_or_default();
        entries
            .into_iter()
            .map(|entry| (entry.normalized_title, entry.page_title))
            .collect()
    })
}

/// Look up the canonical wiki page for a game by `base_title`. Returns
/// `None` if the title doesn't appear in the bundled index.
pub fn shmups_wiki_page(base_title: &str) -> Option<ShmupsWikiPage> {
    let key = normalize_title_for_metadata(base_title);
    let page_title = index().get(&key)?;
    Some(ShmupsWikiPage {
        url: build_page_url(page_title),
        page_title: page_title.clone(),
    })
}

fn build_page_url(page_title: &str) -> String {
    // MediaWiki convention: spaces become underscores in the path segment
    // before percent-encoding kicks in for everything else.
    let with_underscores = page_title.replace(' ', "_");
    let encoded = utf8_percent_encode(&with_underscores, MEDIAWIKI_PATH_ENCODE);
    format!("https://shmups.wiki/library/{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unindexed_title_returns_none() {
        assert!(shmups_wiki_page("super mario kart").is_none());
        assert!(shmups_wiki_page("the legend of zelda ocarina of time").is_none());
    }

    #[test]
    fn indexed_title_returns_page_and_url() {
        // Asserts against the committed bundled snapshot. If this fires after
        // a CI refresh the wiki renamed the page — update the expected values.
        let hit = shmups_wiki_page("battle garegga").expect("indexed");
        assert_eq!(hit.page_title, "Battle Garegga");
        assert_eq!(hit.url, "https://shmups.wiki/library/Battle_Garegga");
    }

    #[test]
    fn lookup_normalizes_region_tags_and_case() {
        for input in ["Battle Garegga (Japan)", "BATTLE GAREGGA"] {
            let hit = shmups_wiki_page(input).expect("indexed");
            assert_eq!(hit.page_title, "Battle Garegga");
            assert_eq!(hit.url, "https://shmups.wiki/library/Battle_Garegga");
        }
    }

    #[test]
    fn build_page_url_replaces_spaces_with_underscores() {
        assert_eq!(
            build_page_url("Battle Garegga"),
            "https://shmups.wiki/library/Battle_Garegga"
        );
    }

    #[test]
    fn build_page_url_keeps_safe_punctuation_literal() {
        assert_eq!(
            build_page_url("Don't Look Back!"),
            "https://shmups.wiki/library/Don't_Look_Back!"
        );
        assert_eq!(
            build_page_url("R-Type Final 2 (Trial)"),
            "https://shmups.wiki/library/R-Type_Final_2_(Trial)"
        );
    }

    #[test]
    fn build_page_url_percent_encodes_unsafe_chars() {
        // `&` and `#` would otherwise be parsed as URL query/fragment markers.
        assert_eq!(
            build_page_url("Foo & Bar"),
            "https://shmups.wiki/library/Foo_%26_Bar"
        );
        assert_eq!(
            build_page_url("Mario #1"),
            "https://shmups.wiki/library/Mario_%231"
        );
    }

    #[test]
    fn build_page_url_handles_non_ascii_utf8() {
        // U+30AC (Japanese KA katakana) encodes as 3 bytes E3 82 AC.
        assert_eq!(
            build_page_url("ガレッガ"),
            "https://shmups.wiki/library/%E3%82%AC%E3%83%AC%E3%83%83%E3%82%AC"
        );
    }
}
