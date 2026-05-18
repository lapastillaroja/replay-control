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

/// Bump whenever the bundled index (`data/shmups-wiki/games.json`) is
/// regenerated or [`shmups_wiki_page`]'s matching logic changes its
/// output for any input. Composed into `enrichment_inputs_version()`
/// (in `replay_control_core_server::library::enrichment`) so deployed
/// appliances re-run per-system enrichment on next boot — picking up
/// newly indexed games, added `video_index: true` flags, and matches
/// that were previously missed by the matcher.
///
/// Version 2 added `Category:Video Index` flags + ` - ` / ` / ` fallback
/// splits that resolve arcade dual-name titles like Darius Gaiden and
/// Soukyugurentai.
pub const SHMUPS_WIKI_VERSION: u32 = 2;

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
    /// Set when the wiki has a `<page_title>/Video Index` sub-page (i.e.
    /// the parent is a member of `Category:Video Index`). Defaulted to
    /// `false` so older snapshots without the field still deserialize.
    #[serde(default)]
    video_index: bool,
}

#[derive(Debug, Clone)]
struct IndexValue {
    page_title: String,
    has_video_index: bool,
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
    /// `Some(url)` when the wiki has a `<page>/Video Index` sub-page
    /// curated under `Category:Video Index`.
    pub video_index_url: Option<String>,
}

fn index() -> &'static HashMap<String, IndexValue> {
    static INDEX: OnceLock<HashMap<String, IndexValue>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let entries: Vec<IndexEntry> = serde_json::from_str(INDEX_JSON).unwrap_or_default();
        entries
            .into_iter()
            .map(|entry| {
                (
                    entry.normalized_title,
                    IndexValue {
                        page_title: entry.page_title,
                        has_video_index: entry.video_index,
                    },
                )
            })
            .collect()
    })
}

fn lookup(base_title: &str) -> Option<&'static IndexValue> {
    let key = normalize_title_for_metadata(base_title);
    if key.is_empty() {
        return None;
    }
    index().get(&key)
}

/// Look up the canonical wiki page for a game by `base_title`. Returns
/// `None` if neither the title nor any of its split variants appear in
/// the bundled index.
///
/// Falls back to splitting on ` - ` (subtitle, e.g. `"Darius Gaiden -
/// Silver Hawk"` → tries `"Darius Gaiden"` and `"Silver Hawk"`) and on
/// ` / ` (dual-region name, e.g. `"Soukyugurentai / Terra Diver"` →
/// tries each side). Arcade MAME/FBNeo titles routinely carry these
/// joined forms in the DAT files; the wiki indexes them under a single
/// canonical name.
pub fn shmups_wiki_page(base_title: &str) -> Option<ShmupsWikiPage> {
    let value = lookup(base_title).or_else(|| {
        split_candidates(base_title)
            .into_iter()
            .find_map(|candidate| lookup(&candidate))
    })?;
    let url = build_page_url(&value.page_title);
    let video_index_url = value.has_video_index.then(|| format!("{url}/Video_Index"));
    Some(ShmupsWikiPage {
        page_title: value.page_title.clone(),
        url,
        video_index_url,
    })
}

/// Yield title segments to retry after a direct miss: the parts on each
/// side of ` - ` (subtitle separator) and ` / ` (dual-region separator),
/// trimmed. Only splits when the separator is surrounded by spaces so
/// real-title hyphens (`"R-Type"`) and slashes (rare) survive.
fn split_candidates(base_title: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sep in [" - ", " / "] {
        if base_title.contains(sep) {
            for piece in base_title.split(sep) {
                let trimmed = piece.trim();
                if !trimmed.is_empty() && !out.iter().any(|s| s == trimmed) {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
    out
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
    fn dual_region_slash_separator_falls_back_to_first_segment() {
        // arcade MAME `sokyugrt.zip` carries this exact dual-region base_title.
        let hit = shmups_wiki_page("soukyugurentai / terra diver").expect("indexed");
        assert_eq!(hit.page_title, "Soukyugurentai");
    }

    #[test]
    fn subtitle_dash_separator_falls_back_to_first_segment() {
        // arcade FBNeo `dariusg.zip` base_title is "darius gaiden - silver hawk".
        let hit = shmups_wiki_page("darius gaiden - silver hawk").expect("indexed");
        assert_eq!(hit.page_title, "Darius Gaiden");
    }

    #[test]
    fn r_type_hyphen_is_not_split() {
        // Real titles with hyphens (no surrounding spaces) must not be split.
        // "R-Type" should normalize whole and hit its index entry.
        let hit = shmups_wiki_page("R-Type").expect("indexed");
        assert_eq!(hit.page_title, "R-Type");
    }

    #[test]
    fn split_candidates_yields_both_sides_dedup() {
        assert_eq!(
            split_candidates("foo - bar / foo"),
            vec![
                "foo".to_string(),
                "bar / foo".to_string(),
                "foo - bar".to_string()
            ],
        );
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
