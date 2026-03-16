use super::*;
#[cfg(feature = "ssr")]
use super::recommendations::{resolve_box_art_for_picks, to_recommended};

/// Related games data: regional variants + translations + hacks + specials + series + similar games.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedGamesData {
    /// Other regions of the same game. Empty if only one region exists.
    pub regional_variants: Vec<RegionalVariant>,
    /// Translations of the same game. Empty if no translations exist.
    pub translations: Vec<TranslationVariant>,
    /// Hacks of the same game. Empty if no hacks exist.
    pub hacks: Vec<HackVariant>,
    /// Special versions of the same game (FastROM, 60Hz, unlicensed, etc.).
    pub specials: Vec<SpecialVariant>,
    /// Cross-name variants of the same game (e.g., "Bare Knuckle" / "Streets of Rage").
    pub alias_variants: Vec<RecommendedGame>,
    /// Other games in the same series/franchise (cross-system).
    pub series_siblings: Vec<RecommendedGame>,
    /// Games from the same system + genre. Empty if no genre or no matches.
    pub similar_games: Vec<RecommendedGame>,
}

/// A regional variant chip linking to another version of the same game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionalVariant {
    pub rom_filename: String,
    pub region: String,
    pub href: String,
    /// True if this is the current game (for active chip styling).
    pub is_current: bool,
}

/// A translation variant chip linking to a translated version of the same game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationVariant {
    pub rom_filename: String,
    /// Short label extracted from the filename tags, e.g., "ES Translation".
    pub label: String,
    pub href: String,
    /// True if this is the current game (for active chip styling).
    pub is_current: bool,
}

/// A hack variant chip linking to a hacked version of the same game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackVariant {
    pub rom_filename: String,
    /// Short label extracted from the filename tags, e.g., "Hack".
    pub label: String,
    pub href: String,
    /// True if this is the current game (for active chip styling).
    pub is_current: bool,
}

/// A special variant chip (FastROM, 60Hz, unlicensed, homebrew, pre-release, pirate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialVariant {
    pub rom_filename: String,
    /// Short label extracted from the filename tags, e.g., "FastROM", "60Hz".
    pub label: String,
    pub href: String,
    /// True if this is the current game (for active chip styling).
    pub is_current: bool,
}

/// Fetch related games for the detail page: regional variants and similar-genre games.
#[server(prefix = "/sfn")]
pub async fn get_related_games(
    system: String,
    filename: String,
) -> Result<RelatedGamesData, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    // Look up the detail genre from game_library for two-tier similar matching.
    // The detail genre (e.g., "Maze / Shooter") is passed to similar_by_genre(),
    // which internally normalizes it to genre_group for broader matching.
    use replay_control_core::systems::{self, SystemCategory};
    let sys_info = systems::find_system(&system);
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);

    let region_pref = state.region_preference();
    let region_pref_str = format!("{:?}", region_pref).to_lowercase();

    // Single DB access for all queries.
    let db_data = state.cache.with_db_read(&storage, |db| {
        let variants = db.regional_variants(&system, &filename).unwrap_or_default();
        let translations_raw = db.translations(&system, &filename).unwrap_or_default();
        let hacks_raw = db.hacks(&system, &filename).unwrap_or_default();
        let specials_raw = db.specials(&system, &filename).unwrap_or_default();

        // Get the current game's base_title, series_key for relationship queries.
        let current_entry = db
            .load_system_entries(&system)
            .ok()
            .and_then(|entries| entries.into_iter().find(|e| e.rom_filename == filename));

        let base_title = current_entry.as_ref().map(|e| e.base_title.clone()).unwrap_or_default();
        let series_key = current_entry.as_ref().map(|e| e.series_key.clone()).unwrap_or_default();
        let detail_genre = current_entry
            .as_ref()
            .and_then(|e| e.genre.clone().filter(|g| !g.is_empty()))
            .or_else(|| {
                let g = super::search::lookup_genre(&system, &filename);
                if g.is_empty() { None } else { Some(g) }
            })
            .unwrap_or_default();

        // Series siblings: games with same series_key, different base_title, cross-system.
        let series_raw = db
            .series_siblings(&series_key, &base_title, &region_pref_str, 20)
            .unwrap_or_default();

        // Alias variants: cross-name variants via game_alias table.
        let alias_raw = db
            .alias_variants(&system, &base_title, &filename, &region_pref_str)
            .unwrap_or_default();

        let similar = if detail_genre.is_empty() {
            Vec::new()
        } else {
            let limit = if is_arcade { 24 } else { 8 };
            db.similar_by_genre(&system, &detail_genre, &filename, limit)
                .unwrap_or_default()
        };

        (variants, translations_raw, hacks_raw, specials_raw, series_raw, alias_raw, similar)
    });

    let Some((variants_raw, translations_raw, hacks_raw, specials_raw, series_raw, alias_raw, similar_pool)) = db_data
    else {
        return Ok(RelatedGamesData {
            regional_variants: Vec::new(),
            translations: Vec::new(),
            hacks: Vec::new(),
            specials: Vec::new(),
            alias_variants: Vec::new(),
            series_siblings: Vec::new(),
            similar_games: Vec::new(),
        });
    };

    // Build regional variants (only if more than 1).
    // Use extract_tags() for richer labels (e.g., "Japan, Rev 1" instead of "japan").
    // For arcade ROMs (no parenthesized tags), fall back to the display name.
    let regional_variants = if variants_raw.len() > 1 {
        variants_raw
            .into_iter()
            .map(|(rom_fn, region, display_name)| {
                let is_current = rom_fn == filename;
                let href = format!(
                    "/games/{}/{}",
                    system,
                    urlencoding::encode(&rom_fn)
                );
                let tags = replay_control_core::rom_tags::extract_tags(&rom_fn);
                let label = if !tags.is_empty() {
                    tags
                } else if let Some(dn) = display_name {
                    dn
                } else {
                    region
                };
                RegionalVariant {
                    rom_filename: rom_fn,
                    region: label,
                    href,
                    is_current,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    // Build translations list.
    let translations: Vec<TranslationVariant> = translations_raw
        .into_iter()
        .map(|(rom_fn, display_name)| {
            let is_current = rom_fn == filename;
            let href = format!(
                "/games/{}/{}",
                system,
                urlencoding::encode(&rom_fn)
            );
            // Extract the translation label from the filename tags (e.g., "ES Translation").
            let tags = replay_control_core::rom_tags::extract_tags(&rom_fn);
            let label = tags
                .split(", ")
                .find(|part| part.ends_with("Translation"))
                .unwrap_or(&tags)
                .to_string();
            let label = if label.is_empty() {
                display_name.unwrap_or_else(|| rom_fn.clone())
            } else {
                label
            };
            TranslationVariant {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build hacks list.
    let hacks: Vec<HackVariant> = hacks_raw
        .into_iter()
        .map(|(rom_fn, display_name)| {
            let is_current = rom_fn == filename;
            let href = format!(
                "/games/{}/{}",
                system,
                urlencoding::encode(&rom_fn)
            );
            // Extract hack-related labels from the filename tags.
            let tags = replay_control_core::rom_tags::extract_tags(&rom_fn);
            let label = tags
                .split(", ")
                .find(|part| part.contains("Hack"))
                .unwrap_or(&tags)
                .to_string();
            let label = if label.is_empty() {
                display_name.unwrap_or_else(|| rom_fn.clone())
            } else {
                label
            };
            HackVariant {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build specials list.
    let specials: Vec<SpecialVariant> = specials_raw
        .into_iter()
        .map(|(rom_fn, display_name)| {
            let is_current = rom_fn == filename;
            let href = format!(
                "/games/{}/{}",
                system,
                urlencoding::encode(&rom_fn)
            );
            let tags = replay_control_core::rom_tags::extract_tags(&rom_fn);
            let label = if tags.is_empty() {
                display_name.unwrap_or_else(|| rom_fn.clone())
            } else {
                tags
            };
            SpecialVariant {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build alias variants (cross-name versions like "Bare Knuckle" / "Streets of Rage").
    let mut alias_variants: Vec<RecommendedGame> = alias_raw
        .iter()
        .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
        .collect();
    resolve_box_art_for_picks(&state, &mut alias_variants);

    // Build series siblings (other games in the same franchise, cross-system).
    let mut series_siblings: Vec<RecommendedGame> = series_raw
        .iter()
        .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
        .collect();
    resolve_box_art_for_picks(&state, &mut series_siblings);

    // Build similar games. The two-tier similar_by_genre query already orders
    // by exact genre match first (relevance=2) then genre_group (relevance=1),
    // so we just take the top results.
    let mut similar_games: Vec<RecommendedGame> = similar_pool
        .iter()
        .take(8)
        .filter_map(|rom| to_recommended(&system, rom, &systems))
        .collect();

    // Resolve box art from filesystem.
    resolve_box_art_for_picks(&state, &mut similar_games);

    Ok(RelatedGamesData {
        regional_variants,
        translations,
        hacks,
        specials,
        alias_variants,
        series_siblings,
        similar_games,
    })
}
