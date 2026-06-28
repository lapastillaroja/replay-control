//! Recognize structured filters in a free-text search query before the
//! ranked-search pipeline scores anything.
//!
//! The existing ranked search uses `search_text LIKE` as a prefilter and then
//! scores each candidate row against `display_name` + `rom_filename`. Terms
//! that only live in metadata (board, future: developer, year ranges) would
//! pass the prefilter and then score 0 — the row gets dropped. The recognizer
//! converts those terms into exact-filter dimensions (`SearchFilter::board`,
//! …) so they short-circuit straight into `WHERE` clauses, and only the
//! tokens it didn't consume reach the ranked scorer.
//!
//! Current arms:
//! - **Board** — `ArcadeBoard` enum tags, display names, and per-variant
//!   synonyms. Leading or trailing match consumption; internal matches are
//!   left for the ranked scorer to handle.
//!
//! Future arms (see plan §8): developer, genre, multiplayer phrasing, year
//! ranges, board family. Each is one `try_recognize_*` branch with its own
//! dictionary; no API change.

use replay_control_core::arcade_board::ArcadeBoard;

use super::db::SearchFilter;

/// Outcome of recognizing structured filters in a query string.
///
/// `filters` carries any exact-filter dimensions the recognizer extracted.
/// `remaining_text` is what's left over after consuming the recognized
/// tokens — passed unchanged to the ranked scorer over the filtered candidate
/// set. Empty `remaining_text` means the recognizer consumed everything; the
/// caller should skip ranked scoring and just enumerate the filter result.
#[derive(Debug, Default)]
pub struct RecognizedQuery {
    pub filters: SearchFilter<'static>,
    pub remaining_text: String,
}

/// Pre-process a free-text search query.
///
/// Recognizes structured filter dimensions (currently: arcade board) at the
/// **start** or **end** of the input. Internal matches are left alone so a
/// query like "shoot the cps2 robots" stays as free text rather than getting
/// mangled.
///
/// Examples:
/// - `"CPS-2"` → board = Cps2, remaining = `""`.
/// - `"cps2 fighter"` → board = Cps2, remaining = `"fighter"`.
/// - `"Neo Geo MVS"` → board = NeoGeoMvs, remaining = `""`.
/// - `"Capcom CPS-2 fighter"` → board = Cps2, remaining = `"Capcom fighter"`.
/// - `"sonic"` → no match, remaining = `"sonic"`.
pub fn recognize(input: &str) -> RecognizedQuery {
    let mut working = input.trim().to_string();
    let mut filters = SearchFilter::default();

    // Try board recognition from the start and end. A single board can only
    // be assigned once — additional matches are ignored.
    if filters.board.is_none()
        && let Some((board, rest)) = try_strip_board_leading(&working)
    {
        filters.board = Some(board);
        working = rest;
    }
    if filters.board.is_none()
        && let Some((board, rest)) = try_strip_board_trailing(&working)
    {
        filters.board = Some(board);
        working = rest;
    }

    RecognizedQuery {
        filters,
        remaining_text: working.trim().to_string(),
    }
}

/// Lowercased search tokens for a board: display_name, tag, and a small
/// per-variant synonym set covering common dash / spacing variants. Listed
/// **longest-first** so multi-word matches consume before single-word ones
/// (e.g. "Neo Geo MVS" before "MVS").
fn board_tokens(board: ArcadeBoard) -> Vec<String> {
    let mut toks = Vec::with_capacity(4);
    toks.push(board.display_name().to_lowercase());
    toks.push(board.as_tag().to_lowercase());

    // Hand-curated synonyms — common ways users type these names.
    let extras: &[&str] = match board {
        ArcadeBoard::Cps1 => &["cps-1", "cps 1", "capcom play system 1"],
        ArcadeBoard::Cps2 => &["cps-2", "cps 2", "capcom play system 2"],
        ArcadeBoard::Cps3 => &["cps-3", "cps 3", "capcom play system 3"],
        ArcadeBoard::NeoGeoMvs => &[
            "neo geo",
            "neogeo",
            "neo-geo",
            "mvs",
            "neo geo mvs",
            "neogeo mvs",
        ],
        ArcadeBoard::SegaNaomi => &["naomi"],
        ArcadeBoard::SegaNaomi2 => &["naomi 2", "naomi2"],
        ArcadeBoard::SammyAtomiswave => &["atomiswave", "aw"],
        ArcadeBoard::TaitoF2 => &["taito f2", "f2"],
        ArcadeBoard::TaitoF3 => &["taito f3", "f3"],
        ArcadeBoard::TaitoGNet => &["g-net", "gnet", "g net", "taito g-net", "taito gnet"],
        ArcadeBoard::SonyZn => &[
            "zn",
            "zn-1",
            "zn-2",
            "zn1",
            "zn2",
            "sony zn",
            "sony zn-1",
            "sony zn-2",
        ],
        ArcadeBoard::SegaSystem16a => &["system 16a", "sys16a", "sega system 16a"],
        ArcadeBoard::SegaSystem16b => &["system 16b", "sys16b", "sega system 16b", "system 16"],
        ArcadeBoard::SegaSystem18 => &["sega system 18"],
        ArcadeBoard::SegaSystem24 => &["sega system 24"],
        ArcadeBoard::SegaSystem32 => &["sega system 32"],
        ArcadeBoard::IgsPgm => &["pgm"],
        ArcadeBoard::CaveCv1000 => &["cv1000", "cv-1000", "cave cv1000"],
        ArcadeBoard::IremM72 => &["irem m72", "m72"],
        ArcadeBoard::IremM92 => &["irem m92", "m92"],
        ArcadeBoard::SegaModel1 => &["model 1", "sega model 1"],
        ArcadeBoard::SegaModel2 => &["model 2", "sega model 2"],
        ArcadeBoard::SegaModel3 => &["model 3", "sega model 3"],
        ArcadeBoard::MidwaySeattle => &["midway seattle"],
        ArcadeBoard::MidwayVegas => &["midway vegas", "vegas"],
        ArcadeBoard::NamcoSystem10 => &["system 10", "namco system 10", "namcos10"],
        ArcadeBoard::NamcoSystem22 => &["system 22", "namco system 22", "namcos22"],
        ArcadeBoard::Gaelco3d => &["gaelco 3d", "gaelco3d", "gaelco"],
        ArcadeBoard::Cojag => &["cojag", "coin-op jaguar", "coinop jaguar"],
        _ => &[],
    };
    for syn in extras {
        toks.push((*syn).to_string());
    }

    toks.sort_by_key(|s| std::cmp::Reverse(s.len()));
    toks.dedup();
    toks
}

/// Rank boards by how well their tokens match the user's free-text query —
/// used by the `/search` discovery card to surface multiple matches (e.g.
/// "cps" → CPS-1, CPS-2, CPS-3; "naomi" → Naomi, Naomi 2).
///
/// A board contributes the best score across its tokens:
/// - Exact token match → 4
/// - Token equals query case-folded with non-alphanumerics stripped → 3
///   (catches "cps-1" matching "cps1")
/// - Token starts with query → 2
/// - Token contains query as substring → 1
///
/// Boards with score 0 are dropped. Returned in score-desc order; ties
/// break on display-name asc so the list is deterministic.
pub fn find_board_matches(query: &str) -> Vec<ArcadeBoard> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let q_squashed: String = q.chars().filter(|c| c.is_alphanumeric()).collect();

    let mut scored: Vec<(ArcadeBoard, u8)> = ArcadeBoard::ALL
        .iter()
        .filter_map(|&board| {
            let best = board_tokens(board)
                .iter()
                .map(|tok| score_token(tok, &q, &q_squashed))
                .max()
                .unwrap_or(0);
            (best > 0).then_some((board, best))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.display_name().cmp(b.0.display_name()))
    });

    scored.into_iter().map(|(b, _)| b).collect()
}

fn score_token(token: &str, query: &str, query_squashed: &str) -> u8 {
    if token == query {
        return 4;
    }
    let token_squashed: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
    if !query_squashed.is_empty() && token_squashed == query_squashed {
        return 3;
    }
    if token.starts_with(query) {
        return 2;
    }
    if token.contains(query) {
        return 1;
    }
    0
}

/// All `(board, token)` pairs across `ArcadeBoard::ALL`, sorted by token
/// length **descending** so multi-word matches consume before short ones
/// (e.g. "Naomi 2" wins over "Naomi"; "Neo Geo MVS" wins over "MVS").
fn all_tokens() -> Vec<(ArcadeBoard, String)> {
    let mut all: Vec<(ArcadeBoard, String)> = ArcadeBoard::ALL
        .iter()
        .flat_map(|b| board_tokens(*b).into_iter().map(move |t| (*b, t)))
        .collect();
    all.sort_by_key(|(_, t)| std::cmp::Reverse(t.len()));
    all
}

/// Try to strip a board phrase from the start of `input`. Returns the matched
/// board and the remaining tail (still in its original casing) when a token
/// matches at the start AND ends at a word boundary.
fn try_strip_board_leading(input: &str) -> Option<(ArcadeBoard, String)> {
    let lower = input.to_lowercase();
    for (board, token) in all_tokens() {
        if lower.starts_with(&token) && ends_at_word_boundary(&lower, token.len()) {
            let rest = input[token.len()..].trim_start();
            return Some((board, rest.to_string()));
        }
    }
    None
}

/// Try to strip a board phrase from the end of `input`. Mirror of leading.
fn try_strip_board_trailing(input: &str) -> Option<(ArcadeBoard, String)> {
    let lower = input.to_lowercase();
    for (board, token) in all_tokens() {
        if lower.ends_with(&token) {
            let start = lower.len() - token.len();
            if starts_at_word_boundary(&lower, start) {
                let rest = input[..start].trim_end();
                return Some((board, rest.to_string()));
            }
        }
    }
    None
}

fn ends_at_word_boundary(haystack: &str, offset: usize) -> bool {
    haystack[offset..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric())
}

fn starts_at_word_boundary(haystack: &str, offset: usize) -> bool {
    if offset == 0 {
        return true;
    }
    haystack[..offset]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> RecognizedQuery {
        recognize(s)
    }

    #[test]
    fn empty_input_no_match() {
        let out = r("");
        assert!(out.filters.board.is_none());
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn plain_title_passthrough() {
        let out = r("sonic");
        assert!(out.filters.board.is_none());
        assert_eq!(out.remaining_text, "sonic");
    }

    #[test]
    fn cps2_exact_match() {
        let out = r("CPS-2");
        assert_eq!(out.filters.board, Some(ArcadeBoard::Cps2));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn cps2_lowercase_no_dash() {
        let out = r("cps2");
        assert_eq!(out.filters.board, Some(ArcadeBoard::Cps2));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn cps2_leading_then_free_text() {
        let out = r("cps2 fighter");
        assert_eq!(out.filters.board, Some(ArcadeBoard::Cps2));
        assert_eq!(out.remaining_text, "fighter");
    }

    #[test]
    fn cps2_trailing_after_free_text() {
        let out = r("fighter cps2");
        assert_eq!(out.filters.board, Some(ArcadeBoard::Cps2));
        assert_eq!(out.remaining_text, "fighter");
    }

    #[test]
    fn neo_geo_mvs_multiword_phrase() {
        let out = r("Neo Geo MVS");
        assert_eq!(out.filters.board, Some(ArcadeBoard::NeoGeoMvs));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn neo_geo_shorthand() {
        let out = r("Neo Geo");
        assert_eq!(out.filters.board, Some(ArcadeBoard::NeoGeoMvs));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn naomi_2_recognized_before_naomi_due_to_length() {
        let out = r("Naomi 2 puzzle");
        assert_eq!(out.filters.board, Some(ArcadeBoard::SegaNaomi2));
        assert_eq!(out.remaining_text, "puzzle");
    }

    #[test]
    fn internal_match_left_as_free_text() {
        // "the cps2 robots" — neither leading nor trailing. We do NOT split
        // the query; the user can wrap with the filter pill UI when A1 lands.
        let out = r("shoot the cps2 robots");
        assert!(out.filters.board.is_none());
        assert_eq!(out.remaining_text, "shoot the cps2 robots");
    }

    #[test]
    fn word_boundary_required_no_partial_match() {
        // "cps20" is not a board — must end at a word boundary.
        let out = r("cps20 alpha");
        assert!(out.filters.board.is_none());
        assert_eq!(out.remaining_text, "cps20 alpha");
    }

    #[test]
    fn trailing_word_boundary_required() {
        // "boardcps2" — `cps2` does end the string but doesn't start at a word
        // boundary, so we don't strip it.
        let out = r("xcps2");
        assert!(out.filters.board.is_none());
        assert_eq!(out.remaining_text, "xcps2");
    }

    #[test]
    fn tag_form_recognized_via_synonym() {
        // Tag form "neogeo_mvs" — the underscore separator is a word boundary
        // in our recognizer (only alphanumeric counts as a word char).
        let out = r("neogeo_mvs");
        assert_eq!(out.filters.board, Some(ArcadeBoard::NeoGeoMvs));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn taito_f3_phrase() {
        let out = r("Taito F3");
        assert_eq!(out.filters.board, Some(ArcadeBoard::TaitoF3));
        assert_eq!(out.remaining_text, "");
    }

    #[test]
    fn issue_71_boards_recognized() {
        assert_eq!(r("Model 2").filters.board, Some(ArcadeBoard::SegaModel2));
        assert_eq!(
            r("Namco System 10").filters.board,
            Some(ArcadeBoard::NamcoSystem10)
        );
        assert_eq!(
            r("Midway Vegas").filters.board,
            Some(ArcadeBoard::MidwayVegas)
        );
        assert_eq!(r("gaelco 3d").filters.board, Some(ArcadeBoard::Gaelco3d));
    }

    #[test]
    fn find_board_matches_cps_prefix_returns_all_three_cps() {
        let matches = find_board_matches("cps");
        assert!(matches.contains(&ArcadeBoard::Cps1), "should include CPS-1");
        assert!(matches.contains(&ArcadeBoard::Cps2), "should include CPS-2");
        assert!(matches.contains(&ArcadeBoard::Cps3), "should include CPS-3");
    }

    #[test]
    fn find_board_matches_f3_returns_taito_f3() {
        let matches = find_board_matches("f3");
        assert_eq!(matches.first(), Some(&ArcadeBoard::TaitoF3));
    }

    #[test]
    fn find_board_matches_naomi_returns_both_naomi_boards() {
        let matches = find_board_matches("naomi");
        assert!(matches.contains(&ArcadeBoard::SegaNaomi));
        assert!(matches.contains(&ArcadeBoard::SegaNaomi2));
    }

    #[test]
    fn find_board_matches_empty_query_returns_empty() {
        assert!(find_board_matches("").is_empty());
        assert!(find_board_matches("   ").is_empty());
    }

    #[test]
    fn find_board_matches_no_hit_returns_empty() {
        assert!(find_board_matches("totally-unrelated-xyz").is_empty());
    }

    #[test]
    fn find_board_matches_exact_tag_wins_over_substring() {
        // "cps2" exact-matches Cps2's tag; should outrank Cps1/Cps3 substring hits.
        let matches = find_board_matches("cps2");
        assert_eq!(matches.first(), Some(&ArcadeBoard::Cps2));
    }
}
