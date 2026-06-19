//! Pure enrichment pipeline: scan → match → resolve → produce updates.
//!
//! This module contains the data-oriented enrichment logic that takes a DB
//! connection, filesystem paths, and pre-loaded data, then produces enrichment
//! updates. No web server state, connection pools, or caches — the app layer
//! handles orchestration.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::catalog_pool::CatalogGameResourceRow;
use crate::external_metadata::{LAUNCHBOX_PROVIDER, ProviderGameRow, ProviderResourceRow};
use crate::game_db::CatalogDetailMetadata;
use crate::library_db::{BoxArtGenreRating, LibraryDb, LibraryGameResource, ReleaseDateRow};
use crate::thumbnail_manifest::ManifestMatch;
use replay_control_core::DatePrecision;
use replay_control_core::developer::normalize_developer;
use replay_control_core::title_utils::{filename_stem, normalize_title_for_metadata};

// Re-export image resolution types so existing `use enrichment::*` paths keep working.
pub use crate::image_resolution::{
    ArcadeInfoLookup, BoxArtResult, ImageIndex, build_image_index, format_box_art_url,
    resolve_box_art_with_hash,
};

/// Composite version stamp identifying every bundled input the enrichment
/// pipeline reads from. Persisted per storage as
/// [`crate::library_db::library_meta::keys::ENRICHMENT_INPUTS_VERSION`];
/// when the stored stamp differs from this value on boot, the reconcile
/// phase re-runs enrichment for every active system so libraries pick up
/// new manual catalog rows, new Shmups Wiki index data, and matcher
/// improvements without a manual rescan.
///
/// Inputs composed today:
/// - `catalog.sqlite.db_meta.catalog_enrichment_inputs_version` — SHA over
///   `catalog_game_resource` rows plus catalog-backed detail metadata.
///
/// Returns `None` only if the catalog DB has no version stamp (treated
/// as "skip reconcile this boot").
pub async fn enrichment_inputs_version() -> Option<String> {
    crate::catalog_pool::catalog_enrichment_inputs_version().await
}

/// All enrichment updates produced for a single system.
///
/// The app layer writes these to the DB and handles cache invalidation.
pub struct EnrichmentResult {
    /// Box art, genre, players, rating, rating_count updates.
    pub enrichments: Vec<BoxArtGenreRating>,
    /// Developer updates: (rom_filename, normalized_developer).
    pub developer_updates: Vec<(String, String)>,
    /// Release-date rows destined for `game_release_date`. The app layer
    /// must `upsert_release_dates` BEFORE calling
    /// `resolve_release_date_for_library`, otherwise the resolver will
    /// clear the LaunchBox-set date for systems with no catalog data.
    pub release_date_rows: Vec<ReleaseDateRow>,
    /// Cooperative flag updates: rom_filenames that should be set to cooperative=1.
    pub cooperative_updates: Vec<String>,
    /// `game_detail_metadata` rows for this system: `(rom_filename, description, publisher)`.
    /// Caller calls `LibraryDb::replace_descriptions_for_system` to truncate
    /// + repopulate atomically.
    pub description_rows: Vec<(String, Option<String>, Option<String>)>,
    /// Derived game resources (videos/manuals/etc.) for `library_game_resource`.
    pub resource_rows: Vec<LibraryGameResource>,
    /// On-demand manifest matches that need background downloads.
    /// Each entry is (rom_filename, ManifestMatch).
    pub manifest_downloads: Vec<(String, ManifestMatch)>,
}

/// Pick the first matching LaunchBox provider row for each ROM. Match strength
/// in descending order:
///
/// 1. Stored primary `normalized_title`.
/// 2. Stored arcade-clone parent's `normalized_title` (`normalized_title_alt`).
/// 3. `provider_alternate.normalized_alternate` → primary
///    `normalized_title` (covers regional renames where the ROM filename
///    matches an alternate name rather than the primary).
/// 4. No-Intro `hash_matched_name` canonical filename normalized → primary
///    or alt-name (covers ROMs whose filename diverges from the canonical
///    No-Intro title — fan-translated/redumped sets, abbreviated names).
fn match_launchbox_rows<'a>(
    norm_by_rom: &HashMap<String, (String, String)>,
    hash_matched_names: &HashMap<String, String>,
    launchbox_rows: &'a HashMap<String, ProviderGameRow>,
    alt_to_primary: &HashMap<String, String>,
) -> HashMap<String, &'a ProviderGameRow> {
    let mut out = HashMap::with_capacity(norm_by_rom.len());
    for (rom, (norm, norm_alt)) in norm_by_rom {
        if let Some(row) = match_for_rom(
            norm,
            norm_alt,
            hash_matched_names.get(rom).map(String::as_str),
            launchbox_rows,
            alt_to_primary,
        ) {
            out.insert(rom.clone(), row);
        }
    }
    out
}

fn match_for_rom<'a>(
    norm: &str,
    norm_alt: &str,
    hash_name: Option<&str>,
    launchbox_rows: &'a HashMap<String, ProviderGameRow>,
    alt_to_primary: &HashMap<String, String>,
) -> Option<&'a ProviderGameRow> {
    if let Some(row) = launchbox_rows.get(norm) {
        return Some(row);
    }
    if !norm_alt.is_empty()
        && let Some(row) = launchbox_rows.get(norm_alt)
    {
        return Some(row);
    }
    if let Some(prim) = alt_to_primary.get(norm)
        && let Some(row) = launchbox_rows.get(prim)
    {
        return Some(row);
    }
    if let Some(hn) = hash_name {
        let hn_norm = normalize_title_for_metadata(filename_stem(hn));
        if hn_norm.is_empty() || hn_norm == norm {
            return None;
        }
        if let Some(row) = launchbox_rows.get(&hn_norm) {
            return Some(row);
        }
        if let Some(prim) = alt_to_primary.get(&hn_norm)
            && let Some(row) = launchbox_rows.get(prim)
        {
            return Some(row);
        }
    }
    None
}

fn match_resource_key(
    norm: &str,
    norm_alt: &str,
    hash_name: Option<&str>,
    resource_keys: &HashSet<&str>,
    alt_to_primary: Option<&HashMap<String, String>>,
) -> Option<String> {
    if resource_keys.contains(norm) {
        return Some(norm.to_string());
    }
    if !norm_alt.is_empty() && resource_keys.contains(norm_alt) {
        return Some(norm_alt.to_string());
    }
    if let Some(aliases) = alt_to_primary {
        if let Some(prim) = aliases.get(norm)
            && resource_keys.contains(prim.as_str())
        {
            return Some(prim.clone());
        }
        if !norm_alt.is_empty()
            && let Some(prim) = aliases.get(norm_alt)
            && resource_keys.contains(prim.as_str())
        {
            return Some(prim.clone());
        }
    }
    if let Some(hn) = hash_name {
        let hn_norm = normalize_title_for_metadata(filename_stem(hn));
        if !hn_norm.is_empty() && resource_keys.contains(hn_norm.as_str()) {
            return Some(hn_norm);
        }
    }
    None
}

fn split_resource_title_candidates(base_title: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for sep in [" - ", " / "] {
        if base_title.contains(sep) {
            for piece in base_title.split(sep) {
                let trimmed = piece.trim();
                if !trimmed.is_empty() && !out.contains(&trimmed) {
                    out.push(trimmed);
                }
            }
        }
    }
    out
}

fn push_normalized_resource_candidate(out: &mut Vec<String>, title: &str) {
    let normalized = normalize_title_for_metadata(title);
    if !normalized.is_empty() && !out.contains(&normalized) {
        out.push(normalized);
    }
}

fn catalog_resource_candidates(
    norm: &str,
    norm_alt: &str,
    hash_name: Option<&str>,
    base_title: Option<&str>,
    aliases: &[String],
) -> Vec<String> {
    let mut candidates = Vec::new();
    for title in [norm, norm_alt]
        .into_iter()
        .filter(|title| !title.is_empty())
    {
        if !candidates.iter().any(|candidate| candidate == title) {
            candidates.push(title.to_string());
        }
    }
    if let Some(hash_name) = hash_name {
        push_normalized_resource_candidate(&mut candidates, filename_stem(hash_name));
    }
    if let Some(base_title) = base_title {
        push_normalized_resource_candidate(&mut candidates, base_title);
        for title in split_resource_title_candidates(base_title) {
            push_normalized_resource_candidate(&mut candidates, title);
        }
    }
    for alias in aliases {
        push_normalized_resource_candidate(&mut candidates, alias);
        for title in split_resource_title_candidates(alias) {
            push_normalized_resource_candidate(&mut candidates, title);
        }
    }
    candidates
}

fn append_catalog_resources_for_candidates(
    resource_rows: &mut Vec<LibraryGameResource>,
    rom_filename: &str,
    candidates: &[String],
    catalog_resources: &HashMap<String, Vec<CatalogGameResourceRow>>,
) {
    let mut seen = HashSet::new();
    for candidate in candidates {
        let Some(rows) = catalog_resources.get(candidate) else {
            continue;
        };
        for row in rows {
            // Dedup on the same identity `library_game_resource` is keyed by
            // (source, resource_type, resource_id) — deliberately WITHOUT the
            // url. Candidates are own-title first, then the arcade-clone
            // parent (`normalized_title_alt`), so the game's OWN resource wins
            // and the parent is only a fallback. Including the url here would
            // let a clone keep both its own and the parent's row for the same
            // resource_id (e.g. a shmups Video Index whose resource_id is the
            // shared parent page but whose anchor differs per version), and the
            // `INSERT OR REPLACE` write would then let the parent overwrite the
            // clone's own. Matching the storage key keeps own-first precedence.
            let key = (
                row.source.as_str(),
                row.resource_type.as_str(),
                row.resource_id.as_str(),
            );
            if !seen.insert(key) {
                continue;
            }
            resource_rows.push(LibraryGameResource {
                rom_filename: rom_filename.to_string(),
                source: row.source.clone(),
                resource_type: row.resource_type.clone(),
                resource_id: row.resource_id.clone(),
                url: row.url.clone(),
                title: Some(row.title.clone()),
                languages: Some(row.languages.clone()),
                platform: None,
                mime_type: Some(row.mime_type.clone()),
            });
        }
    }
}

/// Run the full enrichment pipeline for a system.
///
/// Pure data function: reads from DB + filesystem, returns all updates.
/// The app layer is responsible for writing updates and cache invalidation.
///
/// # Arguments
/// * `conn` - Library DB connection (game_library + derived caches).
/// * `system` - System folder name.
/// * `index` - Pre-built image index for this system.
/// * `arcade_lookup` - Per-system arcade-game info (display_name etc.).
/// * `launchbox_rows` - Per-system LaunchBox provider metadata, keyed by
///   normalized title.
///
/// `arcade_lookup` is unused at match time — the normalized title for each
/// ROM is read from `game_library` (populated at scan time). The parameter
/// stays in the signature because the box-art resolver still consumes it.
///
/// `alt_to_primary` maps `normalized_alternate → primary normalized_title`
/// from `provider_alternate`. Caller loads it once per system from the
/// host-global `external_metadata.db`.
pub struct EnrichSystemInput<'a> {
    pub conn: &'a Connection,
    pub system: &'a str,
    pub index: &'a ImageIndex,
    pub arcade_lookup: &'a ArcadeInfoLookup,
    pub launchbox_rows: &'a HashMap<String, ProviderGameRow>,
    pub alt_to_primary: &'a HashMap<String, String>,
    pub provider_resources: &'a HashMap<String, Vec<ProviderResourceRow>>,
    pub catalog_resources: &'a HashMap<String, Vec<CatalogGameResourceRow>>,
    /// `game_library.normalized_title → catalog detail metadata` for the
    /// system. Used as a fallback for ROMs that LaunchBox has no row for
    /// (community-curated entries, future TGDB descriptions/publishers, etc.).
    pub catalog_detail_metadata: &'a HashMap<String, CatalogDetailMetadata>,
}

pub fn enrich_system(input: EnrichSystemInput<'_>) -> EnrichmentResult {
    let EnrichSystemInput {
        conn,
        system,
        index,
        arcade_lookup,
        launchbox_rows,
        alt_to_primary,
        provider_resources,
        catalog_resources,
        catalog_detail_metadata,
    } = input;

    // Load existing game_library values to know which are already set.
    let existing_genres: HashSet<String> = LibraryDb::system_rom_genres(conn, system)
        .map(|map| map.into_keys().collect())
        .unwrap_or_default();
    let existing_players: HashSet<String> =
        LibraryDb::system_rom_players(conn, system).unwrap_or_default();
    let existing_developers: HashSet<String> =
        LibraryDb::system_rom_developers(conn, system).unwrap_or_default();

    // Read visible filenames from game_library.
    let rom_filenames: Vec<String> = LibraryDb::visible_filenames(conn, system).unwrap_or_default();

    if rom_filenames.is_empty() {
        return EnrichmentResult {
            enrichments: Vec::new(),
            developer_updates: Vec::new(),
            release_date_rows: Vec::new(),
            cooperative_updates: Vec::new(),
            description_rows: Vec::new(),
            resource_rows: Vec::new(),
            manifest_downloads: Vec::new(),
        };
    }

    // Resolve each ROM filename to its LaunchBox provider row using the
    // normalized titles stored in `game_library` at scan time, the LB
    // alt-name index, and the No-Intro hash-matched canonical name.
    let norm_by_rom = LibraryDb::visible_normalized_titles(conn, system).unwrap_or_default();
    let hash_matched_names: HashMap<String, String> =
        LibraryDb::visible_hash_matched_names(conn, system).unwrap_or_default();
    // base_title_by_rom is consumed by catalog resource fallback, the
    // release-date resolver, and the description writer; load it once here
    // so the same query isn't issued three times.
    let base_title_by_rom: HashMap<String, String> = LibraryDb::visible_base_titles(conn, system)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let lb_by_rom = match_launchbox_rows(
        &norm_by_rom,
        &hash_matched_names,
        launchbox_rows,
        alt_to_primary,
    );

    let mut resource_rows = Vec::new();
    let provider_resource_keys: HashSet<&str> =
        provider_resources.keys().map(String::as_str).collect();
    let alias_pairs = LibraryDb::alias_pairs_for_system(conn, system).unwrap_or_default();
    let mut alias_equiv: HashMap<String, Vec<String>> = HashMap::new();
    for (base_title, alias_name) in alias_pairs {
        let bt_key = base_title.to_lowercase();
        let an_key = alias_name.to_lowercase();
        alias_equiv.entry(bt_key).or_default().push(alias_name);
        alias_equiv.entry(an_key).or_default().push(base_title);
    }

    for (rom_filename, (norm, norm_alt)) in &norm_by_rom {
        let provider_key = match_resource_key(
            norm,
            norm_alt,
            hash_matched_names.get(rom_filename).map(String::as_str),
            &provider_resource_keys,
            Some(alt_to_primary),
        );
        if let Some(rows) = provider_key
            .as_deref()
            .and_then(|matched_key| provider_resources.get(matched_key))
        {
            for row in rows {
                resource_rows.push(LibraryGameResource {
                    rom_filename: rom_filename.clone(),
                    source: if row.provider.is_empty() {
                        LAUNCHBOX_PROVIDER.to_string()
                    } else {
                        row.provider.clone()
                    },
                    resource_type: row.resource_type.clone(),
                    resource_id: row.resource_id.clone(),
                    url: row.url.clone(),
                    title: row.title.clone(),
                    languages: row.languages.clone(),
                    platform: row.platform.clone(),
                    mime_type: row.mime_type.clone(),
                });
            }
        }
        let base_title = base_title_by_rom.get(rom_filename).map(String::as_str);
        let aliases = base_title
            .and_then(|title| alias_equiv.get(&title.to_lowercase()))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let catalog_candidates = catalog_resource_candidates(
            norm,
            norm_alt,
            hash_matched_names.get(rom_filename).map(String::as_str),
            base_title,
            aliases,
        );
        append_catalog_resources_for_candidates(
            &mut resource_rows,
            rom_filename,
            &catalog_candidates,
            catalog_resources,
        );
    }

    // Build enrichment entries + collect manifest download requests.
    let mut manifest_downloads: Vec<(String, ManifestMatch)> = Vec::new();

    let enrichments: Vec<BoxArtGenreRating> = rom_filenames
        .iter()
        .filter_map(|filename| {
            let hash_name = hash_matched_names.get(filename).map(|s| s.as_str());
            let art = match resolve_box_art_with_hash(
                index,
                arcade_lookup,
                system,
                filename,
                hash_name,
            ) {
                BoxArtResult::Found(path) => Some(format_box_art_url(system, &path)),
                BoxArtResult::ManifestHit(m) => {
                    manifest_downloads.push((filename.clone(), m.clone()));
                    None
                }
                BoxArtResult::NotFound => None,
            };
            let row = lb_by_rom.get(filename).copied();
            let rating = row.and_then(|r| r.rating).map(|v| v as f32);
            let rating_count = row.and_then(|r| r.rating_count);
            let genre = if !existing_genres.contains(filename) {
                row.and_then(|r| r.genre.clone())
            } else {
                None
            };
            let players = if !existing_players.contains(filename) {
                row.and_then(|r| r.players)
            } else {
                None
            };
            if art.is_none()
                && rating.is_none()
                && rating_count.is_none()
                && genre.is_none()
                && players.is_none()
            {
                return None;
            }
            Some(BoxArtGenreRating {
                rom_filename: filename.clone(),
                box_art_url: art,
                genre,
                players,
                rating,
                rating_count,
            })
        })
        .collect();

    // ── Second pass: base_title fallback ──────────────────────────────
    // If a ROM has no art but a sibling (same system + base_title) does, use it.
    // This covers region variants (USA has art, Europe doesn't), revisions, etc.
    let enrichments = apply_base_title_fallback(&base_title_by_rom, enrichments, &rom_filenames);

    // Developer enrichment: fill from LaunchBox for ROMs that don't already have one.
    let developer_updates: Vec<(String, String)> = rom_filenames
        .iter()
        .filter(|f| !existing_developers.contains(*f))
        .filter_map(|f| {
            lb_by_rom
                .get(f)
                .and_then(|row| row.developer.as_ref())
                .map(|dev| (f.clone(), normalize_developer(dev)))
        })
        .filter(|(_, dev)| !dev.is_empty())
        .collect();

    // Release-date enrichment: emit `game_release_date` rows for every LB
    // match. `upsert_release_dates` only overwrites a same-region row when
    // the new precision is strictly higher, so this is safe to call on
    // every enrichment pass — TGDB-year + LB-day yields the LB-day row;
    // LB-year on a system with no catalog rows fills the gap. The app
    // layer must run this BEFORE `resolve_release_date_for_library`,
    // otherwise the resolver will overwrite the LB-set value with NULL
    // for systems where `game_release_date` is empty.
    //
    // LaunchBox doesn't tag releases by region in the imported data; pick
    // `"world"` so the resolver's region preference can fall back to it.
    let release_date_rows: Vec<ReleaseDateRow> = rom_filenames
        .iter()
        .filter_map(|f| {
            let row = lb_by_rom.get(f)?;
            let date = row.release_date.as_deref()?;
            let precision = row.release_precision?;
            let base_title = base_title_by_rom.get(f)?;
            if base_title.is_empty() {
                return None;
            }
            Some(ReleaseDateRow {
                system: system.to_string(),
                base_title: base_title.clone(),
                region: "world".to_string(),
                release_date: date.to_string(),
                precision,
                source: "launchbox".to_string(),
            })
        })
        .collect();

    // Multiple ROMs (region variants, revisions) can share the same
    // `(system, base_title)`; collapse so `upsert_release_dates` doesn't
    // see duplicates within a single batch.
    let release_date_rows = dedup_release_date_rows(release_date_rows);

    // Cooperative enrichment: set cooperative=1 for ROMs flagged by LaunchBox.
    // Only update ROMs that are not already cooperative.
    let existing_cooperative: HashSet<String> =
        LibraryDb::system_rom_cooperative(conn, system).unwrap_or_default();
    let cooperative_updates: Vec<String> = rom_filenames
        .iter()
        .filter(|f| !existing_cooperative.contains(*f))
        .filter(|f| {
            lb_by_rom
                .get(f.as_str())
                .map(|row| row.cooperative)
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    // game_detail_metadata rows: per-ROM denormalized description + publisher.
    // Always rebuild the full set for the system (not just newly-matched
    // ROMs) so removing a ROM also removes its description on the next
    // enrichment pass.
    let description_rows: Vec<(String, Option<String>, Option<String>)> = rom_filenames
        .iter()
        .map(|filename| {
            let row = lb_by_rom.get(filename).copied();
            let description = resolve_description(
                filename,
                row,
                &norm_by_rom,
                &hash_matched_names,
                catalog_detail_metadata,
            );
            let publisher = resolve_publisher(
                filename,
                row,
                &norm_by_rom,
                &hash_matched_names,
                catalog_detail_metadata,
            );
            (filename.clone(), description, publisher)
        })
        .collect();

    EnrichmentResult {
        enrichments,
        developer_updates,
        release_date_rows,
        cooperative_updates,
        description_rows,
        resource_rows,
        manifest_downloads,
    }
}

/// Resolve the per-ROM description. LaunchBox provider rows win when they
/// have a non-empty description; otherwise fall back to the catalog-backed
/// description keyed by:
///
/// 1. the ROM's `normalized_title` (from `game_library`),
/// 2. its `normalized_title_alt` (arcade-clone parent's normalized title), so
///    a clone with no own entry can still inherit the parent's catalog
///    description, and
/// 3. the normalized form of the No-Intro `hash_matched_name` (when CRC
///    matching identified the ROM but its filename differs from canonical),
///    so a hash-renamed file picks up the same description as its canonical
///    sibling.
///
/// Empty strings are treated as "no description" so a catalog row with an
/// empty `description` column doesn't shadow a LaunchBox value or null out
/// the cell. Mirrors the lookup chain in `match_for_rom`/LaunchBox matching
/// so the description fallback fires in the same situations the rest of
/// enrichment does.
fn resolve_description(
    filename: &str,
    lb_row: Option<&ProviderGameRow>,
    norm_by_rom: &HashMap<String, (String, String)>,
    hash_matched_names: &HashMap<String, String>,
    catalog_detail_metadata: &HashMap<String, CatalogDetailMetadata>,
) -> Option<String> {
    if let Some(d) = lb_row.and_then(|r| r.description.clone())
        && !d.is_empty()
    {
        return Some(d);
    }
    resolve_catalog_detail_field(
        filename,
        norm_by_rom,
        hash_matched_names,
        catalog_detail_metadata,
        |metadata| metadata.description.as_deref(),
    )
}

fn resolve_publisher(
    filename: &str,
    lb_row: Option<&ProviderGameRow>,
    norm_by_rom: &HashMap<String, (String, String)>,
    hash_matched_names: &HashMap<String, String>,
    catalog_detail_metadata: &HashMap<String, CatalogDetailMetadata>,
) -> Option<String> {
    if let Some(publisher) = lb_row.and_then(|r| r.publisher.clone())
        && !publisher.is_empty()
    {
        return Some(publisher);
    }
    resolve_catalog_detail_field(
        filename,
        norm_by_rom,
        hash_matched_names,
        catalog_detail_metadata,
        |metadata| metadata.publisher.as_deref(),
    )
}

fn resolve_catalog_detail_field(
    filename: &str,
    norm_by_rom: &HashMap<String, (String, String)>,
    hash_matched_names: &HashMap<String, String>,
    catalog_detail_metadata: &HashMap<String, CatalogDetailMetadata>,
    field: impl for<'a> Fn(&'a CatalogDetailMetadata) -> Option<&'a str>,
) -> Option<String> {
    let (norm, norm_alt) = norm_by_rom.get(filename)?;
    catalog_detail_candidate_keys(filename, norm, norm_alt, hash_matched_names)
        .into_iter()
        .filter_map(|key| catalog_detail_metadata.get(&key))
        .find_map(|metadata| field(metadata).filter(|value| !value.is_empty()))
        .map(str::to_string)
}

fn catalog_detail_candidate_keys(
    filename: &str,
    norm: &str,
    norm_alt: &str,
    hash_matched_names: &HashMap<String, String>,
) -> Vec<String> {
    let mut keys = Vec::with_capacity(3);
    for key in [norm, norm_alt] {
        if !key.is_empty() && !keys.iter().any(|existing| existing == key) {
            keys.push(key.to_string());
        }
    }
    if let Some(hash_name) = hash_matched_names.get(filename) {
        let key = normalize_title_for_metadata(filename_stem(hash_name));
        if !key.is_empty() && !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }
    keys
}

/// Drop duplicate `(system, base_title, region)` keys, keeping the
/// highest-precision row. Region variants of the same game share a
/// `base_title`; without dedup, `upsert_release_dates` would see N rows
/// for the same key and the last write would win.
fn dedup_release_date_rows(rows: Vec<ReleaseDateRow>) -> Vec<ReleaseDateRow> {
    let mut by_key: HashMap<(String, String, String), ReleaseDateRow> =
        HashMap::with_capacity(rows.len());
    for row in rows {
        let key = (
            row.system.clone(),
            row.base_title.clone(),
            row.region.clone(),
        );
        by_key
            .entry(key)
            .and_modify(|existing| {
                if precision_rank(row.precision) > precision_rank(existing.precision) {
                    *existing = row.clone();
                }
            })
            .or_insert(row);
    }
    by_key.into_values().collect()
}

fn precision_rank(p: DatePrecision) -> u8 {
    match p {
        DatePrecision::Day => 3,
        DatePrecision::Month => 2,
        DatePrecision::Year => 1,
    }
}

/// Apply base_title fallback: share box art between ROMs with the same base_title.
///
/// After per-ROM resolution, some ROMs have art and some don't. For those without
/// art, if another ROM in the same system shares the same `base_title` and HAS art,
/// use that art. This handles region variants, revisions, etc.
///
/// Returns a new enrichments vec with fallback art injected.
fn apply_base_title_fallback(
    base_titles: &HashMap<String, String>,
    mut enrichments: Vec<BoxArtGenreRating>,
    rom_filenames: &[String],
) -> Vec<BoxArtGenreRating> {
    // Build map: base_title → box_art_url from enrichments that resolved art.
    let mut art_by_base_title: HashMap<&str, &str> = HashMap::new();
    for e in &enrichments {
        if let Some(ref url) = e.box_art_url
            && let Some(bt) = base_titles.get(&e.rom_filename)
            && !bt.is_empty()
        {
            art_by_base_title.entry(bt.as_str()).or_insert(url.as_str());
        }
    }

    if art_by_base_title.is_empty() {
        return enrichments;
    }

    // Collect owned art URLs by base_title (avoids borrow issues with mutable pass below).
    let art_by_base_title: HashMap<String, String> = art_by_base_title
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Track which ROMs already have an enrichment entry.
    let enriched: HashSet<String> = enrichments.iter().map(|e| e.rom_filename.clone()).collect();

    // Pass 1: fill in existing enrichment entries that have no art.
    for e in &mut enrichments {
        if e.box_art_url.is_none()
            && let Some(bt) = base_titles.get(&e.rom_filename)
            && !bt.is_empty()
            && let Some(url) = art_by_base_title.get(bt.as_str())
        {
            e.box_art_url = Some(url.clone());
        }
    }

    // Pass 2: ROMs with no enrichment entry at all (no art, no rating, no genre, etc.)
    // that can get art via base_title fallback.
    for filename in rom_filenames {
        if enriched.contains(filename) {
            continue;
        }
        if let Some(bt) = base_titles.get(filename)
            && !bt.is_empty()
            && let Some(url) = art_by_base_title.get(bt.as_str())
        {
            enrichments.push(BoxArtGenreRating {
                rom_filename: filename.clone(),
                box_art_url: Some(url.clone()),
                genre: None,
                players: None,
                rating: None,
                rating_count: None,
            });
        }
    }

    enrichments
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_control_core::genre::normalize_genre;
    use replay_control_core::library::resource_kind;

    // ── match_for_rom tests (Phase 3 chain) ──────────────────────────

    fn lb_row_with_developer(dev: &str) -> ProviderGameRow {
        ProviderGameRow {
            description: None,
            genre: None,
            developer: Some(dev.to_string()),
            publisher: None,
            release_date: None,
            release_precision: None,
            rating: None,
            rating_count: None,
            cooperative: false,
            players: None,
        }
    }

    fn catalog_resource_row(
        source: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> CatalogGameResourceRow {
        CatalogGameResourceRow {
            source: source.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            url: format!("https://example.invalid/{resource_id}"),
            title: resource_id.to_string(),
            languages: String::new(),
            mime_type: "text/html".to_string(),
        }
    }

    #[test]
    fn catalog_resources_collect_all_matching_candidate_keys() {
        let mut catalog_resources = HashMap::new();
        catalog_resources.insert(
            "romfilename".to_string(),
            vec![catalog_resource_row(
                resource_kind::RETROKIT_SOURCE,
                resource_kind::MANUAL,
                "manual",
            )],
        );
        catalog_resources.insert(
            "basetitle".to_string(),
            vec![catalog_resource_row(
                resource_kind::SHMUPS_WIKI_SOURCE,
                resource_kind::STRATEGY_GUIDE,
                "guide",
            )],
        );

        let mut rows = Vec::new();
        append_catalog_resources_for_candidates(
            &mut rows,
            "ROM Filename.sfc",
            &["romfilename".to_string(), "basetitle".to_string()],
            &catalog_resources,
        );

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.resource_id == "manual"));
        assert!(rows.iter().any(|row| row.resource_id == "guide"));
    }

    #[test]
    fn catalog_resources_deduplicate_same_resource_across_candidates() {
        let row = catalog_resource_row(
            resource_kind::SHMUPS_WIKI_SOURCE,
            resource_kind::STRATEGY_GUIDE,
            "same-guide",
        );
        let mut catalog_resources = HashMap::new();
        catalog_resources.insert("filename".to_string(), vec![row.clone()]);
        catalog_resources.insert("alias".to_string(), vec![row]);

        let mut rows = Vec::new();
        append_catalog_resources_for_candidates(
            &mut rows,
            "ROM Filename.sfc",
            &["filename".to_string(), "alias".to_string()],
            &catalog_resources,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].resource_id, "same-guide");
    }

    #[test]
    fn catalog_resources_own_video_index_wins_over_clone_parent() {
        // A clone (own normalized title) and its arcade parent
        // (normalized_title_alt) can both carry a Video Index whose resource_id
        // is the shared parent page but whose anchor differs per version. The
        // clone's OWN row must win and the parent must not overwrite it.
        // Regression: ddpdfk10 (Ver 1.0) showed the parent's #Version_1.5.
        let mut own = catalog_resource_row(
            resource_kind::SHMUPS_WIKI_SOURCE,
            resource_kind::VIDEO_INDEX,
            "DoDonPachi DaiFukkatsu",
        );
        own.url = "https://shmups.wiki/library/DoDonPachi_DaiFukkatsu/Video_Index#Version_1.0"
            .to_string();
        let mut parent = catalog_resource_row(
            resource_kind::SHMUPS_WIKI_SOURCE,
            resource_kind::VIDEO_INDEX,
            "DoDonPachi DaiFukkatsu",
        );
        parent.url = "https://shmups.wiki/library/DoDonPachi_DaiFukkatsu/Video_Index#Version_1.5"
            .to_string();

        let mut catalog_resources = HashMap::new();
        catalog_resources.insert("ver10".to_string(), vec![own]);
        catalog_resources.insert("ver15".to_string(), vec![parent]);

        let mut rows = Vec::new();
        append_catalog_resources_for_candidates(
            &mut rows,
            "ddpdfk10.zip",
            // own normalized title first, then the clone-parent alt
            &["ver10".to_string(), "ver15".to_string()],
            &catalog_resources,
        );

        assert_eq!(rows.len(), 1, "clone must keep exactly one Video Index");
        assert!(
            rows[0].url.ends_with("#Version_1.0"),
            "clone must keep its OWN anchor, got {}",
            rows[0].url
        );
    }

    #[test]
    fn match_for_rom_primary_wins_over_alt() {
        let mut rows = HashMap::new();
        rows.insert(
            "supermariobros".to_string(),
            lb_row_with_developer("primary"),
        );
        rows.insert("alternateparent".to_string(), lb_row_with_developer("alt"));

        let alt_to_primary = HashMap::new();
        let row = match_for_rom(
            "supermariobros",
            "alternateparent",
            None,
            &rows,
            &alt_to_primary,
        )
        .expect("primary hit");
        assert_eq!(row.developer.as_deref(), Some("primary"));
    }

    #[test]
    fn match_for_rom_arcade_alt_used_when_primary_missing() {
        let mut rows = HashMap::new();
        rows.insert("sf2ce".to_string(), lb_row_with_developer("parent"));
        let alt_to_primary = HashMap::new();
        let row = match_for_rom("sf2cebootleg", "sf2ce", None, &rows, &alt_to_primary)
            .expect("clone parent hit");
        assert_eq!(row.developer.as_deref(), Some("parent"));
    }

    #[test]
    fn match_for_rom_alt_name_falls_back_to_primary() {
        let mut rows = HashMap::new();
        rows.insert("zelda".to_string(), lb_row_with_developer("zelda-prim"));

        let mut alt_to_primary = HashMap::new();
        alt_to_primary.insert("zeldanodensetsu".to_string(), "zelda".to_string());

        // ROM filename normalises to the alternate name; no arcade alt.
        let row = match_for_rom("zeldanodensetsu", "", None, &rows, &alt_to_primary)
            .expect("alt-name hit");
        assert_eq!(row.developer.as_deref(), Some("zelda-prim"));
    }

    #[test]
    fn match_for_rom_hash_name_resolves_via_primary() {
        // ROM filename "Aero Star (Japan)" normalises to "aerostar"; the
        // No-Intro canonical name is "Aerostar (Japan) (En)" which
        // normalises identically here, so use a divergent filename.
        let mut rows = HashMap::new();
        rows.insert("aerostar".to_string(), lb_row_with_developer("hash-prim"));
        let alt_to_primary = HashMap::new();

        // Library-stored norm derived from the user's filename (e.g. fan-renamed),
        // hash_name is the canonical No-Intro filename.
        let row = match_for_rom(
            "aerostarbluestreak",             // user's filename normalised
            "",                               // no arcade clone
            Some("Aerostar (Japan) (En).gb"), // No-Intro canonical filename
            &rows,
            &alt_to_primary,
        )
        .expect("hash-name primary hit");
        assert_eq!(row.developer.as_deref(), Some("hash-prim"));
    }

    #[test]
    fn match_for_rom_hash_name_resolves_via_alt() {
        let mut rows = HashMap::new();
        rows.insert(
            "officialprimary".to_string(),
            lb_row_with_developer("alt-via-hash"),
        );

        let mut alt_to_primary = HashMap::new();
        alt_to_primary.insert(
            "canonicalalternate".to_string(),
            "officialprimary".to_string(),
        );

        let row = match_for_rom(
            "userscustomname",
            "",
            Some("Canonical Alternate (USA).rom"),
            &rows,
            &alt_to_primary,
        )
        .expect("hash-name alt-resolved hit");
        assert_eq!(row.developer.as_deref(), Some("alt-via-hash"));
    }

    #[test]
    fn match_for_rom_skips_hash_name_when_it_normalises_to_primary() {
        // hash_name normalises identically to `norm`. We've already tried
        // that key — bail out instead of reprobing the same map.
        let rows: HashMap<String, ProviderGameRow> = HashMap::new();
        let alt_to_primary = HashMap::new();
        let result = match_for_rom("samename", "", Some("samename.rom"), &rows, &alt_to_primary);
        assert!(result.is_none());
    }

    #[test]
    fn match_for_rom_returns_none_when_all_keys_miss() {
        let rows: HashMap<String, ProviderGameRow> = HashMap::new();
        let alt_to_primary = HashMap::new();
        let result = match_for_rom("nope", "", Some("nope.rom"), &rows, &alt_to_primary);
        assert!(result.is_none());
    }

    // ── base_title fallback tests ────────────────────────────────────

    /// Open a temp library DB for enrichment tests.
    fn open_temp_db() -> (rusqlite::Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = LibraryDb::open(dir.path()).unwrap();
        (conn, dir)
    }

    /// Helper: load `(rom_filename, base_title)` pairs for tests that call
    /// `apply_base_title_fallback`.
    fn load_base_titles(conn: &rusqlite::Connection, system: &str) -> HashMap<String, String> {
        LibraryDb::visible_base_titles(conn, system)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    /// Helper: create a game entry with a specific base_title.
    fn make_entry_with_base_title(
        system: &str,
        filename: &str,
        base_title: &str,
    ) -> crate::library_db::GameEntry {
        crate::library_db::GameEntry {
            system: system.into(),
            rom_filename: filename.into(),
            rom_path: format!("/roms/{system}/{filename}"),
            display_name: None,
            size_bytes: 1000,
            is_m3u: false,
            box_art_url: None,
            driver_status: None,
            genre: None,
            genre_group: String::new(),
            players: None,
            rating: None,
            rating_count: None,
            is_clone: false,
            base_title: base_title.into(),
            region: String::new(),
            is_translation: false,
            is_hack: false,
            is_special: false,
            crc32: None,
            hash_mtime: None,
            hash_size_bytes: None,
            hash_matched_name: None,
            identity_state: crate::library_db::IdentityState::Unknown,
            series_key: String::new(),
            developer: String::new(),
            release_date: None,
            release_precision: None,
            release_region_used: None,
            cooperative: false,
            normalized_title: String::new(),
            normalized_title_alt: String::new(),
            board: None,
            ra_id: String::new(),
            rc_hash: None,
        }
    }

    #[test]
    fn base_title_fallback_shares_art_between_variants() {
        let (mut conn, _dir) = open_temp_db();

        // Two ROMs with the same base_title "sonic".
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[
                make_entry_with_base_title("sega_smd", "Sonic (USA).md", "sonic"),
                make_entry_with_base_title("sega_smd", "Sonic (Europe).md", "sonic"),
            ],
            None,
        )
        .unwrap();

        // Only USA has art resolved. Europe has no art but has a rating.
        let enrichments = vec![
            BoxArtGenreRating {
                rom_filename: "Sonic (USA).md".into(),
                box_art_url: Some("/media/sega_smd/boxart/Sonic.png".into()),
                genre: None,
                players: None,
                rating: Some(4.5),
                rating_count: None,
            },
            BoxArtGenreRating {
                rom_filename: "Sonic (Europe).md".into(),
                box_art_url: None,
                genre: None,
                players: None,
                rating: Some(4.5),
                rating_count: None,
            },
        ];

        let rom_filenames = vec![
            "Sonic (USA).md".to_string(),
            "Sonic (Europe).md".to_string(),
        ];

        let base_titles = load_base_titles(&conn, "sega_smd");
        let result = apply_base_title_fallback(&base_titles, enrichments, &rom_filenames);

        let europe = result
            .iter()
            .find(|e| e.rom_filename == "Sonic (Europe).md")
            .expect("Europe entry should exist");
        assert_eq!(
            europe.box_art_url.as_deref(),
            Some("/media/sega_smd/boxart/Sonic.png"),
            "Europe should get USA's art via base_title fallback"
        );
    }

    #[test]
    fn base_title_fallback_does_not_cross_systems() {
        let (mut conn, _dir) = open_temp_db();

        // "Sonic" on sega_smd with art.
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_entry_with_base_title(
                "sega_smd",
                "Sonic (USA).md",
                "sonic",
            )],
            None,
        )
        .unwrap();

        // "Sonic" on sega_gg with no art.
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_gg",
            &[make_entry_with_base_title(
                "sega_gg",
                "Sonic (USA).gg",
                "sonic",
            )],
            None,
        )
        .unwrap();

        // sega_smd has art; sega_gg has no enrichments (no art at all in that system).
        let enrichments: Vec<BoxArtGenreRating> = vec![];
        let rom_filenames = vec!["Sonic (USA).gg".to_string()];

        let base_titles = load_base_titles(&conn, "sega_gg");
        let result = apply_base_title_fallback(&base_titles, enrichments, &rom_filenames);

        // GG should NOT get MD's art — fallback is per-system only.
        let gg = result.iter().find(|e| e.rom_filename == "Sonic (USA).gg");
        assert!(
            gg.is_none(),
            "GG should not get art from a different system"
        );
    }

    #[test]
    fn base_title_fallback_skips_empty_base_title() {
        let (mut conn, _dir) = open_temp_db();

        // Two ROMs with empty base_title.
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[
                make_entry_with_base_title("sega_smd", "ROM_A.md", ""),
                make_entry_with_base_title("sega_smd", "ROM_B.md", ""),
            ],
            None,
        )
        .unwrap();

        // ROM_A has art, ROM_B does not.
        let enrichments = vec![BoxArtGenreRating {
            rom_filename: "ROM_A.md".into(),
            box_art_url: Some("/media/sega_smd/boxart/ROM_A.png".into()),
            genre: None,
            players: None,
            rating: None,
            rating_count: None,
        }];

        let rom_filenames = vec!["ROM_A.md".to_string(), "ROM_B.md".to_string()];

        let base_titles = load_base_titles(&conn, "sega_smd");
        let result = apply_base_title_fallback(&base_titles, enrichments, &rom_filenames);

        // ROM_B should NOT get art — empty base_title is excluded.
        let rom_b = result.iter().find(|e| e.rom_filename == "ROM_B.md");
        assert!(
            rom_b.is_none(),
            "Empty base_title should not participate in fallback"
        );
    }

    // Removed: `cooperative_or_merge_from_launchbox` — exercised the legacy
    // game_metadata table (GameMetadata + LibraryDb::bulk_upsert +
    // system_metadata_cooperative). The new external_metadata DB has its own
    // tests in `library/external_metadata_refresh.rs`.

    #[test]
    fn enrichment_fills_genre_gap_but_does_not_overwrite() {
        let (mut conn, _dir) = open_temp_db();

        // Game with existing genre "Action".
        let mut entry_with_genre =
            make_entry_with_base_title("sega_smd", "Sonic (USA).md", "sonic");
        entry_with_genre.genre = Some("Action".into());
        entry_with_genre.genre_group = normalize_genre("Action").to_string();

        // Game with no genre.
        let entry_no_genre = make_entry_with_base_title("sega_smd", "Streets (USA).md", "streets");

        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[entry_with_genre, entry_no_genre],
            None,
        )
        .unwrap();

        // Enrichment tries to set genre for both.
        LibraryDb::update_box_art_genre_rating(
            &mut conn,
            "sega_smd",
            &[
                crate::library_db::BoxArtGenreRating {
                    rom_filename: "Sonic (USA).md".into(),
                    box_art_url: None,
                    genre: Some("Adventure".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
                crate::library_db::BoxArtGenreRating {
                    rom_filename: "Streets (USA).md".into(),
                    box_art_url: None,
                    genre: Some("Adventure".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
            ],
        )
        .unwrap();

        let roms = LibraryDb::load_system_entries(&conn, "sega_smd").unwrap();
        let sonic = roms
            .iter()
            .find(|r| r.rom_filename == "Sonic (USA).md")
            .unwrap();
        let streets = roms
            .iter()
            .find(|r| r.rom_filename == "Streets (USA).md")
            .unwrap();

        assert_eq!(
            sonic.genre.as_deref(),
            Some("Action"),
            "existing genre should NOT be overwritten"
        );
        assert_eq!(
            streets.genre.as_deref(),
            Some("Adventure"),
            "NULL genre should be filled"
        );
    }

    #[test]
    fn base_title_fallback_adds_entry_for_rom_without_any_enrichment() {
        let (mut conn, _dir) = open_temp_db();

        // ROM with art and ROM without any enrichment data (no art, no rating, nothing).
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_entry_with_base_title("snes", "Zelda (USA).sfc", "zelda"),
                make_entry_with_base_title("snes", "Zelda (Japan).sfc", "zelda"),
            ],
            None,
        )
        .unwrap();

        // Only USA is in enrichments. Japan was completely filtered out (no data).
        let enrichments = vec![BoxArtGenreRating {
            rom_filename: "Zelda (USA).sfc".into(),
            box_art_url: Some("/media/snes/boxart/Zelda.png".into()),
            genre: None,
            players: None,
            rating: None,
            rating_count: None,
        }];

        let rom_filenames = vec![
            "Zelda (USA).sfc".to_string(),
            "Zelda (Japan).sfc".to_string(),
        ];

        let base_titles = load_base_titles(&conn, "snes");
        let result = apply_base_title_fallback(&base_titles, enrichments, &rom_filenames);

        let japan = result
            .iter()
            .find(|e| e.rom_filename == "Zelda (Japan).sfc")
            .expect("Japan entry should be created by fallback");
        assert_eq!(
            japan.box_art_url.as_deref(),
            Some("/media/snes/boxart/Zelda.png"),
            "Japan should get USA's art via base_title fallback (new entry)"
        );
    }

    fn lb_row_with_description(desc: &str) -> ProviderGameRow {
        ProviderGameRow {
            description: Some(desc.to_string()),
            genre: None,
            developer: None,
            publisher: None,
            release_date: None,
            release_precision: None,
            rating: None,
            rating_count: None,
            cooperative: false,
            players: None,
        }
    }

    fn catalog_detail(description: Option<&str>, publisher: Option<&str>) -> CatalogDetailMetadata {
        CatalogDetailMetadata {
            description: description.map(str::to_string),
            publisher: publisher.map(str::to_string),
        }
    }

    #[test]
    fn description_uses_launchbox_when_present() {
        let row = lb_row_with_description("from launchbox");
        let mut norm = HashMap::new();
        norm.insert(
            "rom.hdf".to_string(),
            ("normtitle".to_string(), String::new()),
        );
        let mut canonical = HashMap::new();
        canonical.insert(
            "normtitle".to_string(),
            catalog_detail(Some("from catalog"), None),
        );
        let hashes = HashMap::new();

        let got = resolve_description("rom.hdf", Some(&row), &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("from launchbox"));
    }

    #[test]
    fn description_falls_back_to_canonical_when_launchbox_missing() {
        let mut norm = HashMap::new();
        norm.insert(
            "AmigaVision.hdf".to_string(),
            ("amigavision".to_string(), String::new()),
        );
        let mut canonical = HashMap::new();
        canonical.insert(
            "amigavision".to_string(),
            catalog_detail(Some("curated description"), None),
        );
        let hashes = HashMap::new();

        let got = resolve_description("AmigaVision.hdf", None, &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("curated description"));
    }

    #[test]
    fn description_falls_back_to_canonical_when_launchbox_has_no_description() {
        let row = ProviderGameRow {
            description: None,
            genre: None,
            developer: Some("dev".to_string()),
            publisher: None,
            release_date: None,
            release_precision: None,
            rating: None,
            rating_count: None,
            cooperative: false,
            players: None,
        };
        let mut norm = HashMap::new();
        norm.insert("x".to_string(), ("xn".to_string(), String::new()));
        let mut canonical = HashMap::new();
        canonical.insert("xn".to_string(), catalog_detail(Some("fallback"), None));
        let hashes = HashMap::new();

        let got = resolve_description("x", Some(&row), &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("fallback"));
    }

    #[test]
    fn description_empty_when_nothing_matches() {
        let norm = HashMap::new();
        let canonical = HashMap::new();
        let hashes = HashMap::new();
        let got = resolve_description("missing.bin", None, &norm, &hashes, &canonical);
        assert!(got.is_none());
    }

    #[test]
    fn description_falls_back_via_normalized_title_alt() {
        // Arcade clone: primary norm has no catalog description, but norm_alt
        // (the parent's normalized title) does. resolve_description should
        // pick the parent's description rather than returning None.
        let mut norm = HashMap::new();
        norm.insert(
            "clone.zip".to_string(),
            ("clone".to_string(), "parent".to_string()),
        );
        let mut canonical = HashMap::new();
        canonical.insert(
            "parent".to_string(),
            catalog_detail(Some("parent's description"), None),
        );
        let hashes = HashMap::new();

        let got = resolve_description("clone.zip", None, &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("parent's description"));
    }

    #[test]
    fn description_falls_back_via_hash_matched_name() {
        // ROM file "foo.sfc" was CRC-matched to "Super Mario World (USA)" —
        // hash_matched_names carries the canonical filename. The ROM's own
        // normalized title ("foo") doesn't match the catalog description key
        // ("supermarioworld"), but the hash-name fallback should.
        let mut norm = HashMap::new();
        norm.insert("foo.sfc".to_string(), ("foo".to_string(), String::new()));
        let mut hashes = HashMap::new();
        hashes.insert("foo.sfc".to_string(), "Super Mario World (USA)".to_string());
        let mut canonical = HashMap::new();
        canonical.insert(
            "supermarioworld".to_string(),
            catalog_detail(Some("from catalog"), None),
        );

        let got = resolve_description("foo.sfc", None, &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("from catalog"));
    }

    #[test]
    fn publisher_uses_launchbox_when_present() {
        let row = ProviderGameRow {
            description: None,
            genre: None,
            developer: None,
            publisher: Some("LaunchBox Publisher".to_string()),
            release_date: None,
            release_precision: None,
            rating: None,
            rating_count: None,
            cooperative: false,
            players: None,
        };
        let mut norm = HashMap::new();
        norm.insert(
            "rom.hdf".to_string(),
            ("normtitle".to_string(), String::new()),
        );
        let mut canonical = HashMap::new();
        canonical.insert(
            "normtitle".to_string(),
            catalog_detail(None, Some("Catalog Publisher")),
        );
        let hashes = HashMap::new();

        let got = resolve_publisher("rom.hdf", Some(&row), &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("LaunchBox Publisher"));
    }

    #[test]
    fn publisher_falls_back_to_catalog() {
        let mut norm = HashMap::new();
        norm.insert(
            "AmigaVision.hdf".to_string(),
            ("amigavision".to_string(), String::new()),
        );
        let mut canonical = HashMap::new();
        canonical.insert(
            "amigavision".to_string(),
            catalog_detail(None, Some("AmigaVision Project")),
        );
        let hashes = HashMap::new();

        let got = resolve_publisher("AmigaVision.hdf", None, &norm, &hashes, &canonical);
        assert_eq!(got.as_deref(), Some("AmigaVision Project"));
    }
}
