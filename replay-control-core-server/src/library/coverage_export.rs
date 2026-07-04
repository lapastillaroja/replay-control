//! Per-ROM metadata coverage export (CSV).
//!
//! Produces one row per ROM describing, for every metadata field, what the two
//! upstream tiers carry:
//!
//! - **catalog** — the compiled reference data (`game_db` for console,
//!   `arcade_db` for arcade). This is the built-in catalog shipped with the app.
//! - **launchbox** — the imported external LaunchBox metadata
//!   (`external_metadata.db`, `provider_game` rows).
//!
//! Splitting the two tiers lets a pack/data maintainer see *which* upstream
//! source is missing a field, not just that the merged value is absent.
//!
//! Media columns (`box_art` / `screenshot` / `title_image`) report **on-disk
//! presence** under the device's media directory — i.e. what is actually
//! present in this device's pack, not whether an image exists upstream.
//!
//! The builder owns its catalog and RA-count lookups (globally reachable from
//! core-server) but takes the already-fetched, pool-bound library and LaunchBox
//! rows as inputs, so the caller owns pool access and the coverage rules live in
//! one place.

use std::collections::HashMap;
use std::path::Path;

use time::OffsetDateTime;

use replay_control_core::rom_tags::{self, RomTier};
use replay_control_core::systems;
use replay_control_core::title_utils::filename_stem;

use crate::arcade_db;
use crate::external_metadata::ProviderGameRow;
use crate::game_db::{self, CanonicalGame};
use crate::image_matching::{build_dir_index, find_best_match};
use crate::library_db::{GameEntry, year_from_release_date};
use crate::thumbnails::ThumbnailKind;

/// Column order for the exported CSV — must stay positionally in sync with
/// [`RomCoverage::fields`] (a test asserts the two have equal length; order is
/// kept aligned by hand).
pub const CSV_COLUMNS: &[&str] = &[
    "system",
    "rom_filename",
    "rom_path",
    "display_name",
    "base_title",
    "normalized_title",
    "series_key",
    "region",
    "crc32",
    "verified_name",
    "classification",
    "is_clone",
    "is_hack",
    "is_translation",
    "is_special",
    "is_m3u",
    "year_catalog",
    "year_launchbox",
    "genre_catalog",
    "genre_launchbox",
    "genre_group",
    "arcade_board",
    "developer_catalog",
    "developer_launchbox",
    "publisher_catalog",
    "publisher_launchbox",
    "players_catalog",
    "players_launchbox",
    "rating_catalog",
    "rating_launchbox",
    "source_catalog",
    "has_description_catalog",
    "has_description_launchbox",
    "box_art",
    "screenshot",
    "title_image",
    "ra_id",
    "ra_count",
    "missing_fields",
];

/// One exported row: per-ROM, per-field coverage across the catalog and
/// LaunchBox tiers, plus on-disk media presence and RA support.
#[derive(Debug, Clone, Default)]
pub struct RomCoverage {
    pub system: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: String,
    pub base_title: String,
    pub normalized_title: String,
    pub series_key: String,
    pub region: String,
    pub crc32: String,
    pub verified_name: String,
    /// ROM classification from filename tags (original / revision / hack /
    /// translation / unlicensed / homebrew / prerelease / pirate / …). Finer
    /// than the `is_*` booleans — distinguishes unlicensed/homebrew/pirate.
    pub classification: String,
    pub is_clone: bool,
    pub is_hack: bool,
    pub is_translation: bool,
    pub is_special: bool,
    pub is_m3u: bool,
    pub year_catalog: String,
    pub year_launchbox: String,
    pub genre_catalog: String,
    pub genre_launchbox: String,
    pub genre_group: String,
    /// User-facing arcade board label (`CPS-2 (Capcom)`, etc.). Empty for
    /// non-arcade systems and arcade rows with no curated board match.
    pub arcade_board: String,
    pub developer_catalog: String,
    pub developer_launchbox: String,
    pub publisher_catalog: String,
    pub publisher_launchbox: String,
    pub players_catalog: String,
    pub players_launchbox: String,
    pub rating_catalog: String,
    pub rating_launchbox: String,
    /// Provenance of the catalog (embedded) row: no-intro / community / wikidata
    /// for console; empty for arcade.
    pub source_catalog: String,
    pub has_description_catalog: bool,
    pub has_description_launchbox: bool,
    pub box_art: bool,
    pub screenshot: bool,
    pub title_image: bool,
    pub ra_id: String,
    /// Achievement count for `ra_id`; `None` when the ROM has no RA set.
    pub ra_count: Option<u32>,
    pub missing_fields: String,
}

impl RomCoverage {
    /// Column values in [`CSV_COLUMNS`] order.
    pub fn fields(&self) -> Vec<String> {
        vec![
            self.system.clone(),
            self.rom_filename.clone(),
            self.rom_path.clone(),
            self.display_name.clone(),
            self.base_title.clone(),
            self.normalized_title.clone(),
            self.series_key.clone(),
            self.region.clone(),
            self.crc32.clone(),
            self.verified_name.clone(),
            self.classification.clone(),
            yn(self.is_clone),
            yn(self.is_hack),
            yn(self.is_translation),
            yn(self.is_special),
            yn(self.is_m3u),
            self.year_catalog.clone(),
            self.year_launchbox.clone(),
            self.genre_catalog.clone(),
            self.genre_launchbox.clone(),
            self.genre_group.clone(),
            self.arcade_board.clone(),
            self.developer_catalog.clone(),
            self.developer_launchbox.clone(),
            self.publisher_catalog.clone(),
            self.publisher_launchbox.clone(),
            self.players_catalog.clone(),
            self.players_launchbox.clone(),
            self.rating_catalog.clone(),
            self.rating_launchbox.clone(),
            self.source_catalog.clone(),
            yn(self.has_description_catalog),
            yn(self.has_description_launchbox),
            yn(self.box_art),
            yn(self.screenshot),
            yn(self.title_image),
            self.ra_id.clone(),
            self.ra_count.map(|c| c.to_string()).unwrap_or_default(),
            self.missing_fields.clone(),
        ]
    }

    /// One RFC-4180 record (data line) for this row, terminated by CRLF.
    pub fn to_csv_line(&self) -> String {
        csv_record(self.fields().iter().map(String::as_str))
    }
}

/// The CSV header line (column names), terminated by CRLF.
pub fn csv_header_line() -> String {
    csv_record(CSV_COLUMNS.iter().copied())
}

/// Filesystem-safe ISO-8601 basic UTC timestamp (e.g. `20260628T143005Z`) for
/// the export filename. Basic format avoids the `:` separators that the
/// extended form would put in a filename.
pub fn export_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}

/// Build the per-ROM coverage rows for a single system.
///
/// - `entries` are the library rows for `system` (the file list + merged
///   values), typically the full unfiltered set so the export covers every ROM.
/// - `launchbox` is the system's `provider_game` rows keyed by normalized title
///   (as returned by `external_metadata::system_launchbox_rows`).
/// - `media_base` is `<rc_dir>/media/<system>`; its `Named_*` subdirs are each
///   scanned once for on-disk media presence.
pub async fn build_system_coverage(
    system: &str,
    entries: &[GameEntry],
    launchbox: &HashMap<String, ProviderGameRow>,
    media_base: &Path,
) -> Vec<RomCoverage> {
    let is_arcade = systems::is_arcade_system(system);

    let stems: Vec<&str> = entries
        .iter()
        .map(|e| filename_stem(e.rom_filename.as_str()))
        .collect();

    // Catalog (embedded) tier, keyed by filename stem.
    let arcade_batch = if is_arcade {
        arcade_db::lookup_arcade_games_batch(system, &stems).await
    } else {
        HashMap::new()
    };
    let (game_by_stem, game_by_norm) = if is_arcade {
        (HashMap::new(), HashMap::new())
    } else {
        let by_stem = game_db::lookup_games_batch(system, &stems).await;
        // Normalized fallback for stems that had no exact catalog match.
        let missing_norms: Vec<String> = entries
            .iter()
            .filter_map(|e| {
                let stem = filename_stem(e.rom_filename.as_str());
                if by_stem.contains_key(stem) {
                    return None;
                }
                let n = game_db::normalize_filename(stem);
                (!n.is_empty()).then_some(n)
            })
            .collect();
        let norm_refs: Vec<&str> = missing_norms.iter().map(String::as_str).collect();
        let by_norm = game_db::lookup_by_normalized_titles_batch(system, &norm_refs).await;
        (by_stem, by_norm)
    };

    // RA achievement counts for every referenced ra_id, one query.
    let ra_ids: Vec<&str> = entries.iter().map(|e| e.ra_id.as_str()).collect();
    let ra_counts = game_db::lookup_ra_counts_batch(&ra_ids).await;

    // Media presence: one directory scan per kind for the whole system.
    let boxart_idx = build_dir_index(
        &media_base.join(ThumbnailKind::Boxart.media_dir()),
        ThumbnailKind::Boxart.media_dir(),
    );
    let snap_idx = build_dir_index(
        &media_base.join(ThumbnailKind::Snap.media_dir()),
        ThumbnailKind::Snap.media_dir(),
    );
    let title_idx = build_dir_index(
        &media_base.join(ThumbnailKind::Title.media_dir()),
        ThumbnailKind::Title.media_dir(),
    );

    entries
        .iter()
        .map(|entry| {
            let stem = filename_stem(entry.rom_filename.as_str());

            // --- Catalog (embedded) tier ---
            let mut cat = CatalogFields::default();
            if is_arcade {
                if let Some(info) = arcade_batch.get(stem) {
                    cat.year = info.year.clone();
                    cat.developer = info.manufacturer.clone();
                    cat.genre = info.category.clone();
                    cat.players = (info.players > 0).then_some(info.players);
                    // Arcade catalog carries no publisher/description/rating.
                }
            } else {
                let canonical: Option<&CanonicalGame> =
                    game_by_stem.get(stem).map(|e| &e.game).or_else(|| {
                        let n = game_db::normalize_filename(stem);
                        (!n.is_empty()).then(|| game_by_norm.get(&n)).flatten()
                    });
                if let Some(g) = canonical {
                    if g.year > 0 {
                        cat.year = g.year.to_string();
                    }
                    cat.genre = g.genre.clone();
                    cat.developer = g.developer.clone();
                    cat.publisher = g.publisher.clone();
                    cat.players = (g.players > 0).then_some(g.players);
                    cat.rating = g.rating.clone();
                    cat.has_description = !g.description.is_empty();
                    cat.source = g.source.clone();
                }
            }

            // --- LaunchBox (external) tier, via the stored normalized key
            //     (arcade clones fall back to the parent's normalized title). ---
            let lb = launchbox.get(&entry.normalized_title).or_else(|| {
                (!entry.normalized_title_alt.is_empty())
                    .then(|| launchbox.get(&entry.normalized_title_alt))
                    .flatten()
            });

            let year_launchbox = lb
                .and_then(|r| r.release_date.as_deref())
                .and_then(year_from_release_date)
                .map(|y| y.to_string())
                .unwrap_or_default();
            let genre_launchbox = lb
                .and_then(|r| non_empty(r.genre.as_deref()))
                .unwrap_or_default();
            let developer_launchbox = lb
                .and_then(|r| non_empty(r.developer.as_deref()))
                .unwrap_or_default();
            let publisher_launchbox = lb
                .and_then(|r| non_empty(r.publisher.as_deref()))
                .unwrap_or_default();
            let players_launchbox = lb
                .and_then(|r| r.players)
                .filter(|p| *p > 0)
                .map(|p| p.to_string())
                .unwrap_or_default();
            let rating_launchbox = lb
                .and_then(|r| r.rating)
                .map(|v| format!("{v:.1}"))
                .unwrap_or_default();
            let has_description_launchbox =
                lb.is_some_and(|r| r.description.as_deref().is_some_and(|d| !d.is_empty()));

            // --- Media presence (on disk) ---
            let arcade_display = is_arcade
                .then(|| arcade_batch.get(stem).map(|i| i.display_name.as_str()))
                .flatten();
            let box_art =
                find_best_match(&boxart_idx, &entry.rom_filename, arcade_display, None).is_some();
            let screenshot =
                find_best_match(&snap_idx, &entry.rom_filename, arcade_display, None).is_some();
            let title_image =
                find_best_match(&title_idx, &entry.rom_filename, arcade_display, None).is_some();

            let players_catalog = cat.players.map(|p| p.to_string()).unwrap_or_default();
            let ra_count = (!entry.ra_id.is_empty())
                .then(|| ra_counts.get(&entry.ra_id).copied().unwrap_or(0));

            let missing_fields = missing_fields(
                &cat,
                &year_launchbox,
                &genre_launchbox,
                &developer_launchbox,
                &publisher_launchbox,
                &players_launchbox,
                has_description_launchbox,
                box_art,
                screenshot,
                title_image,
            );

            RomCoverage {
                system: entry.system.clone(),
                rom_filename: entry.rom_filename.clone(),
                rom_path: entry.rom_path.clone(),
                display_name: entry
                    .display_name
                    .clone()
                    .unwrap_or_else(|| entry.base_title.clone()),
                base_title: entry.base_title.clone(),
                normalized_title: entry.normalized_title.clone(),
                series_key: entry.series_key.clone(),
                region: entry.region.clone(),
                crc32: entry.crc32.map(|c| format!("{c:08X}")).unwrap_or_default(),
                verified_name: entry.hash_matched_name.clone().unwrap_or_default(),
                classification: classification_str(rom_tags::classify(&entry.rom_filename).0)
                    .to_string(),
                is_clone: entry.is_clone,
                is_hack: entry.is_hack,
                is_translation: entry.is_translation,
                is_special: entry.is_special,
                is_m3u: entry.is_m3u,
                year_catalog: cat.year.clone(),
                year_launchbox,
                genre_catalog: cat.genre.clone(),
                genre_launchbox,
                genre_group: entry.genre_group.clone(),
                arcade_board: entry
                    .board
                    .map(|board| board.display_label())
                    .unwrap_or_default(),
                developer_catalog: cat.developer.clone(),
                developer_launchbox,
                publisher_catalog: cat.publisher.clone(),
                publisher_launchbox,
                players_catalog,
                players_launchbox,
                rating_catalog: cat.rating.clone(),
                rating_launchbox,
                source_catalog: cat.source.clone(),
                has_description_catalog: cat.has_description,
                has_description_launchbox,
                box_art,
                screenshot,
                title_image,
                ra_id: entry.ra_id.clone(),
                ra_count,
                missing_fields,
            }
        })
        .collect()
}

/// Catalog-tier extraction for one ROM (console `CanonicalGame` or arcade info).
#[derive(Default)]
struct CatalogFields {
    year: String,
    genre: String,
    developer: String,
    publisher: String,
    players: Option<u8>,
    rating: String,
    has_description: bool,
    source: String,
}

/// Build the `missing_fields` summary: the key fields absent in *both* tiers
/// (or, for media, absent on disk). Semicolon-separated, worst-first sortable.
#[allow(clippy::too_many_arguments)]
fn missing_fields(
    cat: &CatalogFields,
    year_lb: &str,
    genre_lb: &str,
    developer_lb: &str,
    publisher_lb: &str,
    players_lb: &str,
    has_description_lb: bool,
    box_art: bool,
    screenshot: bool,
    title_image: bool,
) -> String {
    // (column, present-in-either-source?) — a field is "missing" when absent
    // everywhere it could come from.
    let present = [
        ("year", !cat.year.is_empty() || !year_lb.is_empty()),
        ("genre", !cat.genre.is_empty() || !genre_lb.is_empty()),
        (
            "developer",
            !cat.developer.is_empty() || !developer_lb.is_empty(),
        ),
        (
            "publisher",
            !cat.publisher.is_empty() || !publisher_lb.is_empty(),
        ),
        ("players", cat.players.is_some() || !players_lb.is_empty()),
        ("description", cat.has_description || has_description_lb),
        ("box_art", box_art),
        ("screenshot", screenshot),
        ("title_image", title_image),
    ];
    present
        .iter()
        .filter(|(_, present)| !present)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";")
}

/// Lowercase, snake_case name for a [`RomTier`] — the `classification` column.
fn classification_str(tier: RomTier) -> &'static str {
    match tier {
        RomTier::Original => "original",
        RomTier::Revision => "revision",
        RomTier::RegionVariant => "region_variant",
        RomTier::Translation => "translation",
        RomTier::Unlicensed => "unlicensed",
        RomTier::Homebrew => "homebrew",
        RomTier::Hack => "hack",
        RomTier::PreRelease => "prerelease",
        RomTier::Pirate => "pirate",
    }
}

fn yn(b: bool) -> String {
    if b { "yes" } else { "no" }.to_string()
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.filter(|v| !v.is_empty()).map(str::to_string)
}

/// Encode one CSV record (RFC 4180): comma-separated, fields quoted when they
/// contain a comma, quote, CR, or LF; embedded quotes doubled. CRLF-terminated.
/// Cells whose first character could be read as a spreadsheet formula are
/// neutralized — see [`write_cell`].
fn csv_record<'a>(fields: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for (i, field) in fields.enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_cell(&mut out, field);
    }
    out.push_str("\r\n");
    out
}

/// Write one CSV cell into `out`, applying RFC-4180 quoting and CSV-injection
/// hardening. A cell beginning with `= + - @` or a control character is prefixed
/// with an apostrophe so Excel/Sheets treat it as text rather than a formula —
/// otherwise a crafted ROM filename/title could execute on open. Numeric and
/// enum columns never start with those characters, so real data is untouched.
fn write_cell(out: &mut String, field: &str) {
    let formula_lead = field
        .as_bytes()
        .first()
        .is_some_and(|b| matches!(b, b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r'));
    let needs_quote = field.contains([',', '"', '\r', '\n']);

    if needs_quote {
        out.push('"');
        if formula_lead {
            out.push('\'');
        }
        out.push_str(&field.replace('"', "\"\""));
        out.push('"');
    } else if formula_lead {
        out.push('\'');
        out.push_str(field);
    } else {
        out.push_str(field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_matches_field_count() {
        let row = RomCoverage::default();
        assert_eq!(row.fields().len(), CSV_COLUMNS.len());
    }

    #[test]
    fn rom_path_is_third_column() {
        let row = RomCoverage {
            system: "nintendo_nes".to_string(),
            rom_filename: "TestGame.nes".to_string(),
            rom_path: "roms/nintendo_nes/TestGame.nes".to_string(),
            ..Default::default()
        };

        assert_eq!(&CSV_COLUMNS[..3], ["system", "rom_filename", "rom_path"]);
        assert_eq!(
            &row.fields()[..3],
            [
                "nintendo_nes".to_string(),
                "TestGame.nes".to_string(),
                "roms/nintendo_nes/TestGame.nes".to_string(),
            ]
        );
    }

    #[test]
    fn csv_quotes_special_chars() {
        // Plain field: no quoting.
        assert_eq!(csv_record(["snes", "Mario"].into_iter()), "snes,Mario\r\n");
        // Comma, quote, and newline force quoting; embedded quote is doubled.
        assert_eq!(
            csv_record(["a,b", "say \"hi\"", "line1\nline2"].into_iter()),
            "\"a,b\",\"say \"\"hi\"\"\",\"line1\nline2\"\r\n"
        );
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        // Leading =,+,-,@ get an apostrophe so spreadsheets treat them as text.
        assert_eq!(
            csv_record(["=SUM(A1)", "+1", "@x", "-2"].into_iter()),
            "'=SUM(A1),'+1,'@x,'-2\r\n"
        );
        // Combined with RFC quoting (field also contains a comma).
        assert_eq!(csv_record(["=a,b"].into_iter()), "\"'=a,b\"\r\n");
        // Ordinary text and numbers are untouched.
        assert_eq!(csv_record(["Mario", "1995"].into_iter()), "Mario,1995\r\n");
    }

    #[test]
    fn classification_reflects_filename_tags() {
        let c = |f: &str| classification_str(rom_tags::classify(f).0);
        assert_eq!(c("Sonic (USA).md"), "original");
        assert_eq!(c("Game (Unl).nes"), "unlicensed");
        assert_eq!(c("Game (USA) [T+Eng].sfc"), "translation");
        assert_eq!(c("Game (Proto).gba"), "prerelease");
    }

    #[test]
    fn missing_fields_unions_both_tiers() {
        // Year present only in LaunchBox -> not missing; genre absent both -> missing.
        let cat = CatalogFields {
            year: String::new(),
            genre: String::new(),
            developer: "Capcom".into(),
            ..Default::default()
        };
        let m = missing_fields(&cat, "1998", "", "", "", "", false, false, true, false);
        // year covered by LB; genre/publisher/players/description missing; box_art & title missing.
        assert!(!m.split(';').any(|f| f == "year"));
        assert!(m.split(';').any(|f| f == "genre"));
        assert!(m.split(';').any(|f| f == "box_art"));
        assert!(!m.split(';').any(|f| f == "screenshot"));
        assert!(m.split(';').any(|f| f == "title_image"));
        assert!(!m.split(';').any(|f| f == "developer"));
    }
}
