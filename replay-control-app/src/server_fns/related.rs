#[cfg(feature = "ssr")]
use super::recommendations::to_recommended;
use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// Related games data: regional variants + translations + hacks + specials + series + similar games.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedGamesData {
    /// Other regions of the same game. Empty if only one region exists.
    pub regional_variants: Vec<RegionalVariant>,
    /// Translations of the same game. Empty if no translations exist.
    pub translations: Vec<VariantChip>,
    /// Hacks of the same game. Empty if no hacks exist.
    pub hacks: Vec<VariantChip>,
    /// Special versions of the same game (FastROM, 60Hz, unlicensed, etc.).
    pub specials: Vec<VariantChip>,
    /// Alternate dumps/versions of the same game (same system, is_clone=1, not hacks).
    /// Empty for arcade systems (they use arcade_versions instead).
    pub alternate_versions: Vec<VariantChip>,
    /// Arcade clone siblings sharing the same parent ROM. Empty for non-arcade systems.
    pub arcade_versions: Vec<VariantChip>,
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
    /// Other games on the same arcade board (excludes the current title).
    /// Empty for non-arcade games or boards with no siblings.
    pub same_board: Vec<RecommendedGame>,
    /// `/board/<tag>` link for the "more on this board" row's see-all.
    /// Empty when `same_board` is empty.
    pub same_board_href: String,
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

/// A variant chip linking to another version of the same game — a translation,
/// hack, special (FastROM/60Hz/unlicensed/…), alternate dump, or arcade clone.
/// Which list a chip belongs to is carried by its field on `RelatedGamesData`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantChip {
    pub rom_filename: String,
    /// Short label shown on the chip (e.g. "ES Translation", "Hack", "60Hz").
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

    let is_arcade = replay_control_core::systems::is_arcade_system(&system);

    let (region_pref_str, region_secondary_str) = super::region_strings(&state);

    let genre_fallback = {
        let g = super::search::lookup_genre(&system, &filename).await;
        if g.is_empty() { None } else { Some(g) }
    };

    let system_cl = system.clone();
    let filename_cl = filename.clone();
    let region_pref_str_cl = region_pref_str.clone();
    let region_secondary_cl = region_secondary_str.clone();

    let db_data = state
        .library_reader
        .read(move |conn| {
            let variants =
                LibraryDb::regional_variants(conn, &system_cl, &filename_cl).unwrap_or_default();
            let translations_raw =
                LibraryDb::translations(conn, &system_cl, &filename_cl).unwrap_or_default();
            let hacks_raw = LibraryDb::hacks(conn, &system_cl, &filename_cl).unwrap_or_default();
            let alternates_raw = if !is_arcade {
                LibraryDb::alternate_versions(conn, &system_cl, &filename_cl).unwrap_or_default()
            } else {
                Vec::new()
            };
            let specials_raw =
                LibraryDb::specials(conn, &system_cl, &filename_cl).unwrap_or_default();

            let all_entries = LibraryDb::load_system_entries(conn, &system_cl).unwrap_or_default();
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
            //
            // Capped at 20 below, which truncates the largest franchises on purpose.
            // For reference (measured 2026-06-21): curated Wikidata series reach
            // ~38 distinct titles (Saga Bomberman; DDR 31, Mega Man 27, Zelda 23),
            // and the algorithmic series_key fallback groups up to ~80 (Pokémon).
            // Raise the limit or add a "see all" if showing full franchises matters.
            let (mut series_raw, series_name_raw) = {
                let wikidata = LibraryDb::wikidata_series_siblings(
                    conn,
                    &system_cl,
                    &base_title,
                    &region_pref_str_cl,
                    &region_secondary_cl,
                    20,
                )
                .unwrap_or_default();
                if !wikidata.is_empty() {
                    let sname = LibraryDb::lookup_series_name(conn, &system_cl, &base_title)
                        .unwrap_or_default();
                    let entries: Vec<_> =
                        wikidata.into_iter().map(|(entry, _order)| entry).collect();
                    (entries, sname)
                } else {
                    let fallback = LibraryDb::series_siblings(
                        conn,
                        &series_key,
                        &base_title,
                        &region_pref_str_cl,
                        &region_secondary_cl,
                        20,
                    )
                    .unwrap_or_default();
                    (fallback, String::new())
                }
            };

            // Same-title cross-system entries and series entries are distinct sections.
            let is_primary = current_entry.is_none_or(|e| !e.is_clone && !e.is_hack);
            let cross_system_raw = if is_primary {
                LibraryDb::cross_system_availability(
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
            if !cross_system_raw.is_empty() {
                let cross_keys: std::collections::HashSet<(&str, &str)> = cross_system_raw
                    .iter()
                    .map(|entry| (entry.system.as_str(), entry.rom_filename.as_str()))
                    .collect();
                series_raw.retain(|entry| {
                    !cross_keys.contains(&(entry.system.as_str(), entry.rom_filename.as_str()))
                });
            }

            // Alias variants: cross-name variants via game_alias table.
            let alias_raw = LibraryDb::alias_variants(
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
                LibraryDb::similar_by_genre(
                    conn,
                    &system_cl,
                    &detail_genre,
                    &filename_cl,
                    crate::MAX_PICKS,
                )
                .unwrap_or_default()
            };

            let all_system_roms: Vec<String> = if is_arcade {
                all_entries.iter().map(|e| e.rom_filename.clone()).collect()
            } else {
                Vec::new()
            };

            // Sequel/prequel chain info (Wikidata P155/P156).
            let sequel_chain = LibraryDb::sequel_info(
                conn,
                &system_cl,
                &base_title,
                &region_pref_str_cl,
                &region_secondary_cl,
            )
            .unwrap_or_default();

            // Other games on the same arcade board (excluding this title).
            let same_board_raw = current_entry
                .and_then(|e| e.board)
                .map(|board| {
                    let tag = board.as_tag();
                    let games = LibraryDb::games_by_board(
                        conn,
                        tag,
                        12,
                        &region_pref_str_cl,
                        &region_secondary_cl,
                    )
                    .unwrap_or_default();
                    (tag.to_string(), games)
                })
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
                same_board_raw,
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
        (same_board_tag, same_board_pool),
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
            same_board: Vec::new(),
            same_board_href: String::new(),
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
    let translations: Vec<VariantChip> = translations_raw
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
            VariantChip {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build hacks list.
    let hacks: Vec<VariantChip> = hacks_raw
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
            VariantChip {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build alternate versions list (clones that are not hacks).
    let alternate_versions: Vec<VariantChip> = alternates_raw
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
            VariantChip {
                rom_filename: rom_fn,
                label,
                href,
                is_current,
            }
        })
        .collect();

    // Build specials list.
    let specials: Vec<VariantChip> = specials_raw
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
            VariantChip {
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
        .map(|rom| {
            let mut game = to_recommended(rom);
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
            game
        })
        .collect();

    // Build cross-system availability (same game on other systems).
    let cross_system: Vec<RecommendedGame> = cross_system_raw.iter().map(to_recommended).collect();

    // Build series siblings (other games in the same franchise, cross-system).
    let series_siblings: Vec<RecommendedGame> = series_raw.iter().map(to_recommended).collect();

    // Build similar games. The two-tier similar_by_genre query already orders
    // by exact genre match first (relevance=2) then genre_group (relevance=1),
    // so we just take the top results.
    let similar_games: Vec<RecommendedGame> = similar_pool
        .iter()
        .take(crate::MAX_PICKS)
        .map(to_recommended)
        .collect();

    // Build "more on this board" row: other games on the same board, excluding
    // the current title. Links to the dedicated /board/<tag> page.
    let same_board: Vec<RecommendedGame> = same_board_pool
        .iter()
        .filter(|e| e.base_title != base_title)
        .take(crate::MAX_PICKS)
        .map(to_recommended)
        .collect();
    let same_board_href = if same_board.is_empty() {
        String::new()
    } else {
        format!("/board/{}", urlencoding::encode(&same_board_tag))
    };

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
        same_board,
        same_board_href,
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
) -> Vec<VariantChip> {
    use replay_control_core_server::arcade_db;

    let current_stem = replay_control_core::title_utils::filename_stem(current_filename);

    let stems: Vec<&str> = all_system_roms
        .iter()
        .map(|f| replay_control_core::title_utils::filename_stem(f))
        .collect();
    let mut batch = arcade_db::lookup_arcade_games_batch(system, &stems).await;

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
    } else if let Some(info) = arcade_db::lookup_arcade_game(system, &parent_name).await {
        let name = info.display_name.clone();
        batch.insert(parent_name.clone(), info);
        name
    } else {
        parent_name.clone()
    };

    // Find all ROMs in this system that share the same parent via arcade_db.
    let mut versions: Vec<VariantChip> = all_system_roms
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
            let label = replay_control_core::title_utils::arcade_clone_label(
                &parent_display,
                &info.display_name,
            );
            let href = format!("/games/{}/{}", system, urlencoding::encode(rom_fn));

            Some(VariantChip {
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
