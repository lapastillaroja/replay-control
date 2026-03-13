use super::*;

/// A page of ROM results with total count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomPage {
    pub roms: Vec<RomEntry>,
    pub total: usize,
    pub has_more: bool,
    /// Human-readable system name (e.g., "Arcade (Atomiswave/Naomi)")
    #[serde(default)]
    pub system_display: String,
    /// Whether this system is an arcade system (for clone filter visibility).
    #[serde(default)]
    pub is_arcade: bool,
}

/// A user-taken screenshot URL for the game detail page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotUrl {
    pub url: String,
    pub timestamp: Option<i64>,
}

/// Detailed ROM info including unified game metadata and favorite status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomDetail {
    pub game: GameInfo,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub is_favorite: bool,
    pub user_screenshots: Vec<ScreenshotUrl>,
    /// Number of distinct box art variants available (for "Change cover" affordance).
    #[serde(default)]
    pub variant_count: usize,
}

#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn get_roms_page(
    system: String,
    offset: usize,
    limit: usize,
    search: String,
    #[server(default)] hide_hacks: bool,
    #[server(default)] hide_translations: bool,
    #[server(default)] hide_betas: bool,
    #[server(default)] hide_clones: bool,
    #[server(default)] genre: String,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] min_rating: Option<f32>,
) -> Result<RomPage, ServerFnError> {
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self as sys_db, SystemCategory};

    let state = expect_context::<crate::api::AppState>();
    let sys_info = sys_db::find_system(&system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);
    let storage = state.storage();
    let region_pref = state.region_preference();
    let all_roms = state
        .cache
        .get_roms(&storage, &system, region_pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Apply tier-based, clone, and genre filters before search scoring.
    let pre_filtered: Vec<RomEntry> = all_roms
        .into_iter()
        .filter(|r| {
            if hide_hacks || hide_translations || hide_betas {
                let (tier, _) = rom_tags::classify(&r.game.rom_filename);
                if hide_hacks && tier == rom_tags::RomTier::Hack {
                    return false;
                }
                if hide_translations && tier == rom_tags::RomTier::Translation {
                    return false;
                }
                if hide_betas && tier == rom_tags::RomTier::PreRelease {
                    return false;
                }
            }
            if hide_clones && is_arcade {
                use replay_control_core::arcade_db;
                let stem = r
                    .game
                    .rom_filename
                    .strip_suffix(".zip")
                    .unwrap_or(&r.game.rom_filename);
                if let Some(info) = arcade_db::lookup_arcade_game(stem)
                    && info.is_clone
                {
                    return false;
                }
            }
            true
        })
        .filter(|r| {
            if genre.is_empty() {
                return true;
            }
            let rom_genre = lookup_genre(&system, &r.game.rom_filename);
            rom_genre.eq_ignore_ascii_case(&genre)
        })
        .filter(|r| {
            if !multiplayer_only {
                return true;
            }
            lookup_players(&system, &r.game.rom_filename) >= 2
        })
        .collect();

    // Apply minimum rating filter: batch-load all ratings for the system,
    // then exclude ROMs below the threshold (unrated games are excluded).
    let pre_filtered: Vec<RomEntry> = if let Some(threshold) = min_rating {
        let ratings = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                db.system_ratings(&system).unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };
        pre_filtered
            .into_iter()
            .filter(|r| {
                ratings
                    .get(&r.game.rom_filename)
                    .is_some_and(|&rating| rating >= threshold as f64)
            })
            .collect()
    } else {
        pre_filtered
    };

    let filtered: Vec<RomEntry> = if search.is_empty() {
        pre_filtered
    } else {
        let q = search.to_lowercase();
        let mut scored: Vec<(u32, RomEntry)> = pre_filtered
            .into_iter()
            .filter_map(|r| {
                let display = r
                    .game
                    .display_name
                    .as_deref()
                    .unwrap_or(&r.game.rom_filename);
                let score = search_score(&q, display, &r.game.rom_filename, region_pref);
                if score > 0 { Some((score, r)) } else { None }
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, r)| r).collect()
    };

    let total = filtered.len();
    let mut roms: Vec<RomEntry> = filtered.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + roms.len() < total;

    // Use cached favorites set instead of per-request filesystem scan.
    let fav_set = state.cache.get_favorites_set(&storage, &system);
    for rom in &mut roms {
        rom.is_favorite = fav_set.contains(&rom.game.rom_filename);
    }

    // Populate box art URLs using cached per-system image index (single dir read).
    let image_index = state.cache.get_image_index(&state, &system);
    for rom in &mut roms {
        rom.box_art_url =
            state
                .cache
                .resolve_box_art(&state, &image_index, &system, &rom.game.rom_filename);
    }

    // Populate driver status for arcade systems.
    if is_arcade {
        use replay_control_core::arcade_db;
        for rom in &mut roms {
            let stem = rom
                .game
                .rom_filename
                .strip_suffix(".zip")
                .unwrap_or(&rom.game.rom_filename);
            if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                let status = match info.status {
                    arcade_db::DriverStatus::Working => "Working",
                    arcade_db::DriverStatus::Imperfect => "Imperfect",
                    arcade_db::DriverStatus::Preliminary => "Preliminary",
                    arcade_db::DriverStatus::Unknown => "Unknown",
                };
                rom.driver_status = Some(status.to_string());
            }
        }
    }

    // Populate players from game_db / arcade_db.
    for rom in &mut roms {
        let p = lookup_players(&system, &rom.game.rom_filename);
        if p > 0 {
            rom.players = Some(p);
        }
    }

    // Populate ratings from metadata DB (batch lookup for efficiency).
    if let Some(guard) = state.metadata_db()
        && let Some(db) = guard.as_ref()
    {
        let filenames: Vec<&str> = roms.iter().map(|r| r.game.rom_filename.as_str()).collect();
        if let Ok(ratings) = db.lookup_ratings(&system, &filenames) {
            for rom in &mut roms {
                if let Some(&rating) = ratings.get(&rom.game.rom_filename)
                    && rating > 0.0
                {
                    rom.rating = Some(rating as f32);
                }
            }
        }
    }

    Ok(RomPage {
        roms,
        total,
        has_more,
        system_display,
        is_arcade,
    })
}

#[server(prefix = "/sfn")]
pub async fn get_rom_detail(system: String, filename: String) -> Result<RomDetail, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let all_roms = state
        .cache
        .get_roms(&storage, &system, state.region_preference())
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let rom = all_roms
        .into_iter()
        .find(|r| r.game.rom_filename == filename)
        .ok_or_else(|| ServerFnError::new(format!("ROM not found: {filename}")))?;

    let is_favorite = replay_control_core::favorites::is_favorite(&storage, &system, &filename);

    let game = resolve_game_info(&system, &filename, &rom.game.rom_path);

    let user_screenshots =
        replay_control_core::screenshots::find_screenshots_for_rom(&storage, &system, &filename)
            .into_iter()
            .map(|s| ScreenshotUrl {
                url: format!("/captures/{}/{}", system, s.filename),
                timestamp: s.timestamp,
            })
            .collect();

    // Count box art variants (lightweight — only needs the thumbnail index).
    let variant_count = state
        .metadata_db()
        .and_then(|guard| {
            guard.as_ref().map(|db| {
                replay_control_core::thumbnail_manifest::count_boxart_variants(
                    db, &system, &filename,
                )
            })
        })
        .unwrap_or(0);

    Ok(RomDetail {
        game,
        size_bytes: rom.size_bytes,
        is_m3u: rom.is_m3u,
        is_favorite,
        user_screenshots,
        variant_count,
    })
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(relative_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::roms::delete_rom(&state.storage(), &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(
    relative_path: String,
    new_filename: String,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let new_path =
        replay_control_core::roms::rename_rom(&state.storage(), &relative_path, &new_filename)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(new_path.display().to_string())
}

#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Launch simulated (not on RePlayOS)".into());
    }

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    replay_control_core::launch::launch_game(&storage, &rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Create a recents entry so the home page reflects the launch immediately.
    // Extract system and rom_filename from the rom_path.
    // rom_path format: "/roms/<system>/<optional_subdirs>/<rom_filename>"
    if let Some((system, rom_filename)) = parse_rom_path(&rom_path) {
        if let Err(e) =
            replay_control_core::recents::add_recent(&storage, &system, &rom_filename, &rom_path)
        {
            tracing::warn!("Failed to create recents entry: {e}");
        }
        state.cache.invalidate_recents();
    }

    Ok("Game launching".into())
}

/// Extract system folder and ROM filename from a rom_path.
///
/// Handles paths like `/roms/sega_smd/Sonic.md` (simple) and
/// `/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip` (nested).
#[cfg(feature = "ssr")]
fn parse_rom_path(rom_path: &str) -> Option<(String, String)> {
    let path = rom_path.strip_prefix("/roms/")?;
    let (system, rest) = path.split_once('/')?;
    let rom_filename = rest.rsplit_once('/').map(|(_, f)| f).unwrap_or(rest);
    Some((system.to_string(), rom_filename.to_string()))
}
