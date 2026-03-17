#[cfg(feature = "ssr")]
use super::recommendations::{resolve_box_art_for_picks, to_recommended};
use super::*;

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
    /// Arcade clone siblings sharing the same parent ROM. Empty for non-arcade systems.
    pub arcade_versions: Vec<ArcadeVersion>,
    /// Cross-name variants of the same game (e.g., "Bare Knuckle" / "Streets of Rage").
    pub alias_variants: Vec<RecommendedGame>,
    /// Other games in the same series/franchise (cross-system).
    pub series_siblings: Vec<RecommendedGame>,
    /// Series name from Wikidata (e.g., "Streets of Rage"). Empty if using algorithmic fallback.
    pub series_name: String,
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

/// An arcade clone/version chip linking to another version of the same arcade game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArcadeVersion {
    pub rom_filename: String,
    /// Concise label: just the parenthesized tag if the base name matches the parent,
    /// or the full display name if it differs.
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

        // Load all entries once — used for current entry lookup and arcade clone siblings.
        let all_entries = db.load_system_entries(&system).unwrap_or_default();

        // Get the current game's base_title, series_key for relationship queries.
        let current_entry = all_entries.iter().find(|e| e.rom_filename == filename);

        let base_title = current_entry
            .map(|e| e.base_title.clone())
            .unwrap_or_default();
        let series_key = current_entry
            .map(|e| e.series_key.clone())
            .unwrap_or_default();
        let detail_genre = current_entry
            .and_then(|e| e.genre.clone().filter(|g| !g.is_empty()))
            .or_else(|| {
                let g = super::search::lookup_genre(&system, &filename);
                if g.is_empty() { None } else { Some(g) }
            })
            .unwrap_or_default();

        // Series siblings: prefer Wikidata data (has ordering), fall back to algorithmic series_key.
        let (series_raw, series_name_raw) = {
            let wikidata = db
                .wikidata_series_siblings(&system, &base_title, &region_pref_str, 20)
                .unwrap_or_default();
            if !wikidata.is_empty() {
                // Wikidata series found: use it (entries come with optional order).
                let sname = db
                    .lookup_series_name(&system, &base_title)
                    .unwrap_or_default();
                let entries: Vec<_> = wikidata.into_iter().map(|(entry, _order)| entry).collect();
                (entries, sname)
            } else {
                // Fall back to algorithmic series_key matching.
                let fallback = db
                    .series_siblings(&series_key, &base_title, &region_pref_str, 20)
                    .unwrap_or_default();
                (fallback, String::new())
            }
        };

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

        // For arcade systems, collect all ROM filenames for clone sibling lookup.
        let all_system_roms: Vec<String> = if is_arcade {
            all_entries.iter().map(|e| e.rom_filename.clone()).collect()
        } else {
            Vec::new()
        };

        (
            variants,
            translations_raw,
            hacks_raw,
            specials_raw,
            series_raw,
            series_name_raw,
            alias_raw,
            similar,
            base_title,
            all_system_roms,
        )
    });

    let Some((
        variants_raw,
        translations_raw,
        hacks_raw,
        specials_raw,
        series_raw,
        series_name,
        alias_raw,
        similar_pool,
        base_title,
        all_system_roms,
    )) = db_data
    else {
        return Ok(RelatedGamesData {
            regional_variants: Vec::new(),
            translations: Vec::new(),
            hacks: Vec::new(),
            specials: Vec::new(),
            arcade_versions: Vec::new(),
            alias_variants: Vec::new(),
            series_siblings: Vec::new(),
            series_name: String::new(),
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
                let href = format!("/games/{}/{}", system, urlencoding::encode(&rom_fn));
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
            let href = format!("/games/{}/{}", system, urlencoding::encode(&rom_fn));
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
            let href = format!("/games/{}/{}", system, urlencoding::encode(&rom_fn));
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
            let href = format!("/games/{}/{}", system, urlencoding::encode(&rom_fn));
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

    // Build arcade versions: clone siblings sharing the same parent ROM.
    let arcade_versions = if is_arcade {
        build_arcade_versions(&system, &filename, &all_system_roms)
    } else {
        Vec::new()
    };

    // Build alias variants (cross-name versions like "Bare Knuckle" / "Streets of Rage").
    // Label logic: if the title is the same as the current game (just a regional variant),
    // show only the region (e.g., "Europe"). If the title is different (cross-name variant),
    // show the different name + region (e.g., "Vampire Killer (Japan)").
    let current_bt = &base_title;
    let mut alias_variants: Vec<RecommendedGame> = alias_raw
        .iter()
        .filter_map(|rom| {
            let mut game = to_recommended(&rom.system, rom, &systems)?;
            let tags = replay_control_core::rom_tags::extract_tags(&rom.rom_filename);
            let name = rom.display_name.as_deref().unwrap_or(&rom.rom_filename);
            let title = name.find(" (").map(|i| &name[..i]).unwrap_or(name);
            let same_name = rom.base_title == *current_bt;
            let label = if same_name && !tags.is_empty() {
                // Same game, different region — just show the region
                tags
            } else if !tags.is_empty() {
                // Different name — show name + region
                format!("{title} ({tags})")
            } else {
                title.to_string()
            };
            game.label = Some(label);
            Some(game)
        })
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
        arcade_versions,
        alias_variants,
        series_siblings,
        series_name,
        similar_games,
    })
}

/// Build the arcade versions list: clone siblings sharing the same parent ROM.
///
/// Uses `arcade_db` to resolve parent/clone relationships, then cross-references
/// with the ROMs that actually exist in this system's `game_library`.
#[cfg(feature = "ssr")]
fn build_arcade_versions(
    system: &str,
    current_filename: &str,
    all_system_roms: &[String],
) -> Vec<ArcadeVersion> {
    use replay_control_core::arcade_db;

    let current_stem = current_filename
        .strip_suffix(".zip")
        .unwrap_or(current_filename);

    let current_info = match arcade_db::lookup_arcade_game(current_stem) {
        Some(info) => info,
        None => return Vec::new(),
    };

    // Determine the parent ROM name. If this ROM is a clone, use its parent.
    // If this ROM is a parent, use its own name.
    let parent_name = if current_info.is_clone && !current_info.parent.is_empty() {
        current_info.parent
    } else {
        current_info.rom_name
    };

    // Get the parent's display name for label extraction.
    let parent_display = arcade_db::lookup_arcade_game(parent_name)
        .map(|info| info.display_name)
        .unwrap_or(parent_name);

    // Find all ROMs in this system that share the same parent via arcade_db.
    let mut versions: Vec<ArcadeVersion> = all_system_roms
        .iter()
        .filter_map(|rom_fn| {
            let stem = rom_fn.strip_suffix(".zip").unwrap_or(rom_fn);
            let info = arcade_db::lookup_arcade_game(stem)?;

            // Must share the same parent (or BE the parent).
            let rom_parent = if info.is_clone && !info.parent.is_empty() {
                info.parent
            } else {
                info.rom_name
            };
            if rom_parent != parent_name {
                return None;
            }

            // Filter out hacks.
            if info.display_name.to_lowercase().contains("hack") {
                return None;
            }

            // Filter out bootlegs.
            if info.display_name.to_lowercase().contains("bootleg") {
                return None;
            }

            // Filter out BIOS entries.
            if info.is_bios {
                return None;
            }

            let is_current = rom_fn == current_filename;
            let label = arcade_clone_label(parent_display, info.display_name);
            let href = format!("/games/{}/{}", system, urlencoding::encode(rom_fn));

            Some(ArcadeVersion {
                rom_filename: rom_fn.clone(),
                label,
                href,
                is_current,
            })
        })
        .collect();

    // Sort by label for consistent ordering.
    versions.sort_by(|a, b| a.label.cmp(&b.label));

    // Cap at 10 results.
    versions.truncate(10);

    // Only return if there are siblings (more than just the current ROM).
    if versions.len() > 1 {
        versions
    } else {
        Vec::new()
    }
}

/// Extract a concise label for an arcade clone relative to its parent.
///
/// If the clone has the same base name as the parent (before any parenthesized tag),
/// show just the parenthesized tag (e.g., "Japan 910214" from
/// "Street Fighter II: The World Warrior (Japan 910214)").
///
/// If the clone has a different base name, show the full display name
/// (e.g., "Super Street Fighter II X: Grand Master Challenge (Japan 940311)").
#[cfg(any(feature = "ssr", test))]
pub(crate) fn arcade_clone_label(parent_display: &str, clone_display: &str) -> String {
    // Strip all parenthesized content to get base names.
    let parent_base = parent_display
        .find(" (")
        .map(|i| &parent_display[..i])
        .unwrap_or(parent_display);
    let clone_base = clone_display
        .find(" (")
        .map(|i| &clone_display[..i])
        .unwrap_or(clone_display);

    // Extract parenthesized tag from clone.
    let tag = clone_display
        .rfind('(')
        .and_then(|start| {
            clone_display
                .rfind(')')
                .map(|end| &clone_display[start + 1..end])
        })
        .unwrap_or("");

    if clone_base == parent_base {
        // Same name, show just the tag (region + date).
        if tag.is_empty() {
            clone_display.to_string()
        } else {
            tag.to_string()
        }
    } else {
        // Different name, show full clone display name.
        clone_display.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::arcade_clone_label;

    #[test]
    fn same_base_name_extracts_tag() {
        let label = arcade_clone_label(
            "Street Fighter II: The World Warrior (World 910522)",
            "Street Fighter II: The World Warrior (Japan 910214)",
        );
        assert_eq!(label, "Japan 910214");
    }

    #[test]
    fn different_base_name_shows_full() {
        let label = arcade_clone_label(
            "Street Fighter II: The World Warrior (World 910522)",
            "Street Fighter II: Champion Edition (World 920513)",
        );
        assert_eq!(label, "Street Fighter II: Champion Edition (World 920513)");
    }

    #[test]
    fn parent_without_tag() {
        let label = arcade_clone_label("Metal Slug 6", "Metal Slug 6 (Japan)");
        assert_eq!(label, "Japan");
    }

    #[test]
    fn clone_without_tag_same_base() {
        // Unusual case: clone has no parenthesized tag but same base name.
        let label = arcade_clone_label("Metal Slug 6 (World)", "Metal Slug 6");
        assert_eq!(label, "Metal Slug 6");
    }

    #[test]
    fn completely_different_name() {
        let label = arcade_clone_label("Pac-Man (Midway)", "Puck Man (Japan set 1)");
        assert_eq!(label, "Puck Man (Japan set 1)");
    }

    #[test]
    fn nested_parentheses_uses_last() {
        // Some arcade names have nested parens; rfind should get the outermost last pair.
        let label = arcade_clone_label("Donkey Kong (US set 1)", "Donkey Kong (US set 2)");
        assert_eq!(label, "US set 2");
    }
}
