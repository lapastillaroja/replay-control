#[cfg(feature = "ssr")]
use super::recommendations::to_recommended;
use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::metadata_db::MetadataDb;

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
    /// Alternate dumps/versions of the same game (same system, is_clone=1, not hacks).
    /// Empty for arcade systems (they use arcade_versions instead).
    pub alternate_versions: Vec<AlternateVersion>,
    /// Arcade clone siblings sharing the same parent ROM. Empty for non-arcade systems.
    pub arcade_versions: Vec<ArcadeVersion>,
    /// Cross-name variants of the same game (e.g., "Bare Knuckle" / "Streets of Rage").
    pub alias_variants: Vec<RecommendedGame>,
    /// Same game on other systems (cross-system base_title match).
    /// Empty when series_siblings already covers cross-system entries.
    pub cross_system: Vec<RecommendedGame>,
    /// Other games in the same series/franchise (cross-system).
    pub series_siblings: Vec<RecommendedGame>,
    /// Series name from Wikidata (e.g., "Streets of Rage"). Empty if using algorithmic fallback.
    pub series_name: String,
    /// Games from the same system + genre. Empty if no genre or no matches.
    pub similar_games: Vec<RecommendedGame>,
    /// Predecessor game in the sequel chain. `None` if no predecessor data.
    pub sequel_prev: Option<SequelLink>,
    /// Successor game in the sequel chain. `None` if no successor data.
    pub sequel_next: Option<SequelLink>,
    /// Position in the series for "N of M" display, e.g., `(2, 3)` for "2 of 3".
    pub series_position: Option<(i32, i32)>,
}

/// A link to a predecessor or successor game in a sequel chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequelLink {
    /// Display name for this game. Always present.
    pub title: String,
    /// Link to the game's detail page, if the game is in the user's library.
    pub href: Option<String>,
    /// Whether the game exists in the user's library.
    pub in_library: bool,
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

/// An alternate version chip (alternate dump, trained, cracked — is_clone=1, not hacks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternateVersion {
    pub rom_filename: String,
    /// Short label extracted from the filename tags, e.g., "Alternate", "Alternate 2".
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
    let systems = state
        .cache
        .cached_systems(&storage, &state.metadata_pool)
        .await;

    let is_arcade = replay_control_core::systems::is_arcade_system(&system);

    let region_pref = state.region_preference();
    let region_pref_str = region_pref.as_str().to_string();

    let genre_fallback = {
        let g = super::search::lookup_genre(&system, &filename).await;
        if g.is_empty() { None } else { Some(g) }
    };

    let system_cl = system.clone();
    let filename_cl = filename.clone();
    let region_pref_str_cl = region_pref_str.clone();

    let db_data = state
        .metadata_pool
        .read(move |conn| {
            let variants =
                MetadataDb::regional_variants(conn, &system_cl, &filename_cl).unwrap_or_default();
            let translations_raw =
                MetadataDb::translations(conn, &system_cl, &filename_cl).unwrap_or_default();
            let hacks_raw = MetadataDb::hacks(conn, &system_cl, &filename_cl).unwrap_or_default();
            let alternates_raw = if !is_arcade {
                MetadataDb::alternate_versions(conn, &system_cl, &filename_cl).unwrap_or_default()
            } else {
                Vec::new()
            };
            let specials_raw =
                MetadataDb::specials(conn, &system_cl, &filename_cl).unwrap_or_default();

            let all_entries = MetadataDb::load_system_entries(conn, &system_cl).unwrap_or_default();
            let current_entry = all_entries.iter().find(|e| e.rom_filename == filename_cl);

            let base_title = current_entry
                .map(|e| e.base_title.clone())
                .unwrap_or_default();
            let series_key = current_entry
                .map(|e| e.series_key.clone())
                .unwrap_or_default();
            let detail_genre = current_entry
                .and_then(|e| e.genre.clone().filter(|g| !g.is_empty()))
                .or(genre_fallback)
                .unwrap_or_default();

            // Series siblings: prefer Wikidata (has ordering), fall back to algorithmic series_key.
            let (series_raw, series_name_raw) = {
                let wikidata = MetadataDb::wikidata_series_siblings(
                    conn,
                    &system_cl,
                    &base_title,
                    &region_pref_str_cl,
                    20,
                )
                .unwrap_or_default();
                if !wikidata.is_empty() {
                    let sname = MetadataDb::lookup_series_name(conn, &system_cl, &base_title)
                        .unwrap_or_default();
                    let entries: Vec<_> =
                        wikidata.into_iter().map(|(entry, _order)| entry).collect();
                    (entries, sname)
                } else {
                    let fallback = MetadataDb::series_siblings(
                        conn,
                        &series_key,
                        &base_title,
                        &region_pref_str_cl,
                        20,
                    )
                    .unwrap_or_default();
                    (fallback, String::new())
                }
            };

            // Skip when Wikidata series data covers cross-system entries, or for clones/hacks.
            let is_primary = current_entry.is_none_or(|e| !e.is_clone && !e.is_hack);
            let cross_system_raw = if series_raw.is_empty() && is_primary {
                MetadataDb::cross_system_availability(
                    conn,
                    &system_cl,
                    &base_title,
                    &region_pref_str_cl,
                    10,
                )
                .unwrap_or_default()
            } else {
                Vec::new()
            };

            // Alias variants: cross-name variants via game_alias table.
            let alias_raw = MetadataDb::alias_variants(
                conn,
                &system_cl,
                &base_title,
                &filename_cl,
                &region_pref_str_cl,
            )
            .unwrap_or_default();

            let similar = if detail_genre.is_empty() {
                Vec::new()
            } else {
                let limit = if is_arcade { 24 } else { 8 };
                MetadataDb::similar_by_genre(conn, &system_cl, &detail_genre, &filename_cl, limit)
                    .unwrap_or_default()
            };

            let all_system_roms: Vec<String> = if is_arcade {
                all_entries.iter().map(|e| e.rom_filename.clone()).collect()
            } else {
                Vec::new()
            };

            // Sequel/prequel chain info (Wikidata P155/P156).
            let sequel_chain =
                MetadataDb::sequel_info(conn, &system_cl, &base_title, &region_pref_str_cl)
                    .unwrap_or_default();

            (
                variants,
                translations_raw,
                hacks_raw,
                alternates_raw,
                specials_raw,
                series_raw,
                series_name_raw,
                alias_raw,
                cross_system_raw,
                similar,
                base_title,
                all_system_roms,
                sequel_chain,
            )
        })
        .await;

    let Some((
        variants_raw,
        translations_raw,
        hacks_raw,
        alternates_raw,
        specials_raw,
        series_raw,
        series_name,
        alias_raw,
        cross_system_raw,
        similar_pool,
        base_title,
        all_system_roms,
        sequel_chain,
    )) = db_data
    else {
        return Ok(RelatedGamesData {
            regional_variants: Vec::new(),
            translations: Vec::new(),
            hacks: Vec::new(),
            alternate_versions: Vec::new(),
            specials: Vec::new(),
            arcade_versions: Vec::new(),
            alias_variants: Vec::new(),
            cross_system: Vec::new(),
            series_siblings: Vec::new(),
            series_name: String::new(),
            similar_games: Vec::new(),
            sequel_prev: None,
            sequel_next: None,
            series_position: None,
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

    // Build alternate versions list (clones that are not hacks).
    let alternate_versions: Vec<AlternateVersion> = alternates_raw
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
            AlternateVersion {
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
        build_arcade_versions(&system, &filename, &all_system_roms).await
    } else {
        Vec::new()
    };

    // Build alias variants (cross-name versions like "Bare Knuckle" / "Streets of Rage").
    // Label logic: if the title is the same as the current game (just a regional variant),
    // show only the region (e.g., "Europe"). If the title is different (cross-name variant),
    // show the different name + region (e.g., "Vampire Killer (Japan)").
    let current_bt = &base_title;
    let alias_variants: Vec<RecommendedGame> = alias_raw
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

    // Build cross-system availability (same game on other systems).
    let cross_system: Vec<RecommendedGame> = cross_system_raw
        .iter()
        .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
        .collect();

    // Build series siblings (other games in the same franchise, cross-system).
    let series_siblings: Vec<RecommendedGame> = series_raw
        .iter()
        .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
        .collect();

    // Build similar games. The two-tier similar_by_genre query already orders
    // by exact genre match first (relevance=2) then genre_group (relevance=1),
    // so we just take the top results.
    let similar_games: Vec<RecommendedGame> = similar_pool
        .iter()
        .take(8)
        .filter_map(|rom| to_recommended(&system, rom, &systems))
        .collect();

    // Build sequel/prequel links.
    let sequel_prev = sequel_chain.follows_title.map(|title| {
        let (href, in_library) = match &sequel_chain.follows_entry {
            Some(entry) => {
                let href = format!(
                    "/games/{}/{}",
                    entry.system,
                    urlencoding::encode(&entry.rom_filename)
                );
                (Some(href), true)
            }
            None => (None, false),
        };
        let display = sequel_chain
            .follows_entry
            .as_ref()
            .and_then(|e| e.display_name.clone())
            .unwrap_or(title);
        SequelLink {
            title: display,
            href,
            in_library,
        }
    });

    let sequel_next = sequel_chain.followed_by_title.map(|title| {
        let (href, in_library) = match &sequel_chain.followed_by_entry {
            Some(entry) => {
                let href = format!(
                    "/games/{}/{}",
                    entry.system,
                    urlencoding::encode(&entry.rom_filename)
                );
                (Some(href), true)
            }
            None => (None, false),
        };
        let display = sequel_chain
            .followed_by_entry
            .as_ref()
            .and_then(|e| e.display_name.clone())
            .unwrap_or(title);
        SequelLink {
            title: display,
            href,
            in_library,
        }
    });

    let series_position = sequel_chain.series_order.zip(sequel_chain.series_max_order);

    Ok(RelatedGamesData {
        regional_variants,
        translations,
        hacks,
        alternate_versions,
        specials,
        arcade_versions,
        alias_variants,
        cross_system,
        series_siblings,
        series_name,
        similar_games,
        sequel_prev,
        sequel_next,
        series_position,
    })
}

/// Build the arcade versions list: clone siblings sharing the same parent ROM.
///
/// Uses `arcade_db` to resolve parent/clone relationships, then cross-references
/// with the ROMs that actually exist in this system's `game_library`.
#[cfg(feature = "ssr")]
async fn build_arcade_versions(
    system: &str,
    current_filename: &str,
    all_system_roms: &[String],
) -> Vec<ArcadeVersion> {
    use replay_control_core_server::arcade_db;

    let current_stem = replay_control_core::title_utils::filename_stem(current_filename);

    let stems: Vec<&str> = all_system_roms
        .iter()
        .map(|f| replay_control_core::title_utils::filename_stem(f))
        .collect();
    let mut batch = arcade_db::lookup_arcade_games_batch(&stems).await;

    let current_info = match batch.get(current_stem).cloned() {
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

    // Parent may not be in the system's ROM list, so fall back to a singular lookup.
    let parent_display = if let Some(info) = batch.get(parent_name.as_str()) {
        info.display_name.clone()
    } else if let Some(info) = arcade_db::lookup_arcade_game(&parent_name).await {
        let name = info.display_name.clone();
        batch.insert(parent_name.clone(), info);
        name
    } else {
        parent_name.clone()
    };

    // Find all ROMs in this system that share the same parent via arcade_db.
    let mut versions: Vec<ArcadeVersion> = all_system_roms
        .iter()
        .filter_map(|rom_fn| {
            let stem = replay_control_core::title_utils::filename_stem(rom_fn);
            let info = batch.get(stem)?.clone();

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
            let label = arcade_clone_label(&parent_display, &info.display_name);
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
