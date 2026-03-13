use super::*;
#[cfg(feature = "ssr")]
use super::recommendations::{resolve_box_art_for_picks, to_recommended};

/// Related games data: regional variants + translations + similar games by genre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedGamesData {
    /// Other regions of the same game. Empty if only one region exists.
    pub regional_variants: Vec<RegionalVariant>,
    /// Translations of the same game. Empty if no translations exist.
    pub translations: Vec<TranslationVariant>,
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

/// Fetch related games for the detail page: regional variants and similar-genre games.
#[server(prefix = "/sfn")]
pub async fn get_related_games(
    system: String,
    filename: String,
) -> Result<RelatedGamesData, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    // Look up genre from the game DB/arcade DB (same source as game detail).
    let genre = super::search::lookup_genre(&system, &filename);

    // For arcade: look up the category for preferential sorting.
    use replay_control_core::systems::{self, SystemCategory};
    let sys_info = systems::find_system(&system);
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);
    let arcade_category = if is_arcade {
        let stem = filename.strip_suffix(".zip").unwrap_or(&filename);
        replay_control_core::arcade_db::lookup_arcade_game(stem)
            .map(|info| info.category.to_string())
            .filter(|c| !c.is_empty())
    } else {
        None
    };

    // Single DB access for all queries.
    let db_data = state.cache.with_db_read(&storage, |db| {
        let variants = db.regional_variants(&system, &filename).unwrap_or_default();
        let translations_raw = db.translations(&system, &filename).unwrap_or_default();

        let similar = if genre.is_empty() {
            Vec::new()
        } else {
            // For arcade: query a larger pool so we can prefer same-category games.
            let limit = if is_arcade { 24 } else { 8 };
            db.similar_by_genre(&system, &genre, &filename, limit)
                .unwrap_or_default()
        };

        (variants, translations_raw, similar)
    });

    let Some((variants_raw, translations_raw, similar_pool)) = db_data else {
        return Ok(RelatedGamesData {
            regional_variants: Vec::new(),
            translations: Vec::new(),
            similar_games: Vec::new(),
        });
    };

    // Build regional variants (only if more than 1).
    let regional_variants = if variants_raw.len() > 1 {
        variants_raw
            .into_iter()
            .map(|(rom_fn, region)| {
                let is_current = rom_fn == filename;
                let href = format!(
                    "/games/{}/{}",
                    system,
                    urlencoding::encode(&rom_fn)
                );
                RegionalVariant {
                    rom_filename: rom_fn,
                    region,
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

    // Build similar games, applying arcade category preference.
    let mut similar_games: Vec<RecommendedGame> = if is_arcade && arcade_category.is_some() {
        let cat = arcade_category.as_deref().unwrap();
        // Partition into same-category and other.
        let (mut same_cat, mut other): (Vec<_>, Vec<_>) =
            similar_pool.into_iter().partition(|rom| {
                let stem = rom
                    .rom_filename
                    .strip_suffix(".zip")
                    .unwrap_or(&rom.rom_filename);
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| info.category == cat)
                    .unwrap_or(false)
            });

        // Take up to 8, preferring same-category.
        same_cat.truncate(8);
        let remaining = 8 - same_cat.len();
        other.truncate(remaining);
        same_cat.extend(other);

        same_cat
            .iter()
            .filter_map(|rom| to_recommended(&system, rom, &systems))
            .collect()
    } else {
        similar_pool
            .iter()
            .take(8)
            .filter_map(|rom| to_recommended(&system, rom, &systems))
            .collect()
    };

    // Resolve box art from filesystem.
    resolve_box_art_for_picks(&state, &mut similar_games);

    Ok(RelatedGamesData {
        regional_variants,
        translations,
        similar_games,
    })
}
