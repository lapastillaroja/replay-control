use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::error::Error as CoreError;
use replay_control_core::library_db::LibraryResourceLink;
#[cfg(feature = "ssr")]
use replay_control_core::resource_kind;
#[cfg(feature = "ssr")]
use replay_control_core::{systems, title_utils};
#[cfg(feature = "ssr")]
use replay_control_core_server::favorites;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::{GameEntry, LibraryDb, LibraryGameResource};
#[cfg(feature = "ssr")]
use replay_control_core_server::recents::add_recent;
#[cfg(feature = "ssr")]
use replay_control_core_server::roms::{
    FileKind, GroupedFile, check_rename_allowed, delete_rom_group, detect_disc_set,
    list_data_dir_contents, list_rom_group, rename_rom as rename_rom_file,
};
#[cfg(feature = "ssr")]
use replay_control_core_server::storage::StorageLocation;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::{screenshots, thumbnails};

/// A page of ROM results with total count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomPage {
    pub roms: Vec<RomListEntry>,
    pub total: usize,
    pub has_more: bool,
    /// Human-readable system name (e.g., "Arcade (Atomiswave/Naomi)")
    #[serde(default)]
    pub system_display: String,
    /// Whether this system is an arcade system (for clone filter visibility).
    #[serde(default)]
    pub is_arcade: bool,
    /// Set when the search recognizer extracted a structured filter from
    /// the user's free-text query. Drives the pill on the system ROM list.
    #[serde(
        default,
        skip_serializing_if = "super::search::RecognizedFilter::is_empty"
    )]
    pub recognized: super::search::RecognizedFilter,
}

/// A user-taken screenshot URL for the game detail page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotUrl {
    pub filename: String,
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
    /// Whether this ROM is a hack (suppresses "Change cover" affordance).
    #[serde(default)]
    pub is_hack: bool,
    /// Whether this ROM is a special version (FastROM, 60Hz, unlicensed, etc.).
    #[serde(default)]
    pub is_special: bool,
    /// Normalized base_title for cross-variant video sharing.
    #[serde(default)]
    pub base_title: String,
    /// Whether this ROM can be safely renamed.
    #[serde(default = "default_true")]
    pub rename_allowed: bool,
    /// Explanation when rename is not allowed.
    #[serde(default)]
    pub rename_reason: Option<String>,
    /// Multi-disc set info (if part of a disc set without M3U wrapper).
    #[serde(default)]
    pub disc_info: Option<DiscInfoDto>,
    /// Every `library_game_resource` row for this ROM, loaded once at SSR
    /// in `get_rom_detail` and partitioned client-side by `resource_type` /
    /// `source`. Today this carries the Shmups Wiki strategy-guide link
    /// (`resource_type="strategy_guide"`, `source="shmups_wiki"`); future
    /// external-link sources (HG101, etc.) plug into the same Vec without
    /// expanding the wire shape.
    ///
    /// Manuals and videos still load lazily through their own server fns —
    /// they do alias-expanded user_data joins and filesystem walks beyond
    /// the bare resource read, and live behind UI sections that aren't
    /// always shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_resources: Vec<LibraryResourceLink>,
    /// Resources-section data, loaded with the page in `get_rom_detail` (the six
    /// reads run concurrently) so a client-side navigation makes ONE request
    /// instead of six separate per-section fetches. Over imperfect networks a
    /// dropped per-section fetch used to silently empty that section on
    /// transition (SSR was unaffected, since it runs in-process); bundling them
    /// into the page request ties their availability to the page itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<GameDocument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_manuals: Vec<LocalManual>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_videos: Vec<VideoEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_resource_links: Vec<ResourceEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual_suggestions: Vec<ManualRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_suggestions: Vec<VideoRecommendation>,
}

impl RomDetail {
    /// Return the URL of the first `library_resources` entry whose
    /// `resource_type` and `source` match. Single helper so UI code
    /// doesn't repeat the `iter().find(…).map(url.clone())` pattern for
    /// every external link surfaced on the detail page.
    pub fn find_resource_url(&self, resource_type: &str, source: &str) -> Option<String> {
        self.library_resources
            .iter()
            .find(|r| r.resource_type == resource_type && r.source == source)
            .map(|r| r.url.clone())
    }
}

fn default_true() -> bool {
    true
}

/// Serializable multi-disc info for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscInfoDto {
    pub disc_number: u32,
    pub total_discs: u32,
    pub siblings: Vec<String>,
}

/// Summary of files in a ROM group (for delete confirmation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomFileGroup {
    pub files: Vec<RomFileEntry>,
    pub total_size: u64,
}

/// A single file entry in a ROM group summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomFileEntry {
    pub filename: String,
    pub size_bytes: u64,
    /// Set when this entry summarizes a whole companion directory with more
    /// files than the display cap: the number of files inside. The client
    /// renders the localized "N files" suffix from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir_file_count: Option<usize>,
}

// clippy::too_many_arguments — Leptos server functions require flat parameter lists
// for serialization; wrapping in a struct is not supported by the #[server] macro.
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
    #[server(default)] coop_only: bool,
    #[server(default)] has_achievements: bool,
    #[server(default)] min_rating: Option<f32>,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
    #[server(default)] only_mature: bool,
) -> Result<RomPage, ServerFnError> {
    use replay_control_core::systems as sys_db;

    let state = expect_context::<crate::api::AppState>();
    let system_display = sys_db::system_display_name(&system);
    let is_arcade = sys_db::is_arcade_system(&system);
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    // Unified path: all filtering (content, text search) at the SQL level via
    // search_game_library(). GameEntry rows from the DB already carry genre,
    // rating, players, and driver_status, so enrichment is minimal (just box art
    // and favorites overlay).
    use replay_control_core_server::library_db::SearchFilter;

    let min_rating_f64 = min_rating.map(|r| r as f64);
    let genre_owned = genre.clone();
    let sys_owned = system.clone();

    // Route structured terms (board name) out of the free-text query and
    // into exact-filter dimensions before the ranked scorer runs.
    let recognized_query =
        replay_control_core_server::library::search_recognizer::recognize(search.trim());
    let recognized_board = recognized_query.filters.board;
    let remaining_query_display = recognized_query.remaining_text.clone();
    let search_owned = recognized_query.remaining_text.to_lowercase();

    let db_result = state
        .library_reader
        .try_read(move |conn| {
            let filter = SearchFilter {
                hide_hacks,
                hide_translations,
                hide_betas,
                hide_clones,
                genre: &genre_owned,
                multiplayer_only,
                coop_only,
                min_rating: min_rating_f64,
                min_year,
                max_year,
                board: recognized_board,
                has_achievements,
                only_mature,
            };
            LibraryDb::search_game_library_ranked(
                conn,
                Some(&sys_owned),
                &search_owned,
                &filter,
                offset,
                limit,
                region_pref,
                region_secondary,
            )
        })
        .await;

    let (page_entries, total) = match db_result {
        Ok(Ok(page)) => page,
        Ok(Err(e)) => {
            return Err(ServerFnError::new(e.to_string()));
        }
        Err(e) => {
            return Err(ServerFnError::new(e.to_string()));
        }
    };
    let has_more = offset + page_entries.len() < total;

    // Enrich page entries: box art, favorites, genre (shared with developer page).
    let list_entries = super::enrich_game_entries(&state, page_entries).await;

    Ok(RomPage {
        roms: list_entries,
        total,
        has_more,
        system_display,
        is_arcade,
        recognized: super::search::RecognizedFilter {
            board: recognized_board.map(|b| b.display_label()),
            remaining_query: remaining_query_display,
        },
    })
}

#[server(prefix = "/sfn", endpoint = "/get_rom_detail")]
pub async fn get_rom_detail(system: String, filename: String) -> Result<RomDetail, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Fetch the full GameEntry from game_library (source of truth for all metadata).
    let sys_owned = system.clone();
    let fname_owned = filename.clone();
    let entry = state
        .library_reader
        .read(move |conn| LibraryDb::load_single_entry(conn, &sys_owned, &fname_owned))
        .await
        .and_then(|r| r.ok())
        .flatten();

    let entry = match entry {
        Some(entry) => entry,
        // Alpha Player movies (and any other multimedia/utility system) are never
        // indexed in the library, yet the device writes the same Recents/
        // Favorites markers for them. Don't error — render a minimal detail page
        // from the marker so the entry opens. Real games keep the usual errors.
        None if systems::is_multimedia_system(&system) => {
            return Ok(multimedia_rom_detail(&storage, &system, &filename).await);
        }
        None => {
            return Err(if !state.is_idle() {
                ServerFnError::new(
                    "Game data is temporarily unavailable while the library is being rebuilt. Please try again in a moment.",
                )
            } else {
                ServerFnError::new(format!("ROM not found: {filename}"))
            });
        }
    };

    // These four reads are independent (they only need `entry` + the ids) and
    // hit different pools / the filesystem, so run them concurrently instead of
    // sequentially — this hides their latencies behind the slowest one rather
    // than summing them on the detail-page critical path.
    let (is_favorite, (game, variant_count), library_resources) = tokio::join!(
        replay_control_core_server::favorites::is_favorite(&storage, &system, &filename),
        // Box art variant count (manifest index only — no filesystem scan)
        // chains after build_game_detail so it can reuse the arcade display
        // name that lookup already resolved, instead of a second catalog
        // lookup for the same ROM.
        async {
            let (game, arcade_display) = build_game_detail(&state, &entry).await;
            // Match the semantics of the display_name_if_arcade lookup this
            // replaced: an empty catalog display name counts as no name.
            let arcade_display = arcade_display.filter(|name| !name.is_empty());
            let variant_count = state
                .external_metadata_reader
                .read({
                    let system = system.clone();
                    let filename = filename.clone();
                    move |em_conn| {
                        replay_control_core_server::thumbnail_manifest::count_boxart_variants(
                            em_conn,
                            &system,
                            &filename,
                            arcade_display.as_deref(),
                        )
                    }
                })
                .await
                .unwrap_or(0);
            (game, variant_count)
        },
        load_library_resources(&state, &system, &filename),
    );

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_rom_detail game_info resolved"
    );

    let user_screenshots = screenshot_urls_for_rom(&storage, &system, &filename);

    let (tier, _, is_special) = replay_control_core::rom_tags::classify(&filename);
    let is_hack = tier == replay_control_core::rom_tags::RomTier::Hack;

    // Deliberately recomputed from the resolved display name rather than read
    // from `entry.base_title`: the stored column falls back to the filename
    // *stem* while this convention falls back to the full filename, and —
    // decisive — user_data rows (saved manuals/videos/links) are keyed by the
    // value clients echo back from this field. Switching to the stored column
    // would orphan saved data wherever the two diverge.
    let base_title = replay_control_core::title_utils::base_title(&game.display_name);

    // Determine rename restrictions.
    let (rename_allowed, rename_reason) =
        check_rename_allowed(&storage, &system, entry.rom_path.trim_start_matches('/'));

    // Detect multi-disc set.
    let disc_info = detect_disc_set(&storage, &system, &filename).map(|di| DiscInfoDto {
        disc_number: di.disc_number,
        total_discs: di.total_discs,
        siblings: di.siblings,
    });

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_rom_detail complete"
    );
    let size_bytes = list_rom_group(&storage, &system, &entry.rom_path)
        .map(|group| group.iter().map(|file| file.size_bytes).sum())
        .unwrap_or(entry.size_bytes);

    // Load the resources-section data concurrently so it ships with this single
    // request (see RomDetail::documents). Shared inputs are resolved once and
    // threaded in: the alias-title set (the standalone section endpoints each
    // re-resolve it), the saved-manuals rows (feeding both the local-manuals
    // list and the suggestion exclusion keys), and the per-type partition of
    // the library resource rows already loaded above. Reads stay best-effort:
    // a failed read degrades that section to empty rather than failing the
    // whole detail page. Deliberate tradeoff of the sharing: a transient
    // failure of one shared read now degrades its dependent sections
    // together (they used to fail independently) — accepted, since each
    // failure mode is unchanged and the dedup removes more failure
    // opportunities than it correlates.
    let all_titles = super::resolve_shared_titles(&state, &system, &base_title).await;
    let saved_manuals =
        super::manuals::fetch_saved_manuals(&state, &system, all_titles.clone()).await;
    let saved_manual_keys: std::collections::HashSet<String> = saved_manuals
        .iter()
        .map(|m| m.resource_key.clone())
        .collect();
    let partition = |kind: &str| -> Vec<LibraryResourceLink> {
        library_resources
            .iter()
            .filter(|row| row.resource_type == kind)
            .cloned()
            .collect()
    };
    let manual_rows = partition(resource_kind::MANUAL);
    let video_rows = partition(resource_kind::VIDEO);

    let (documents, local_manuals, saved_videos, saved_resource_links) = tokio::join!(
        get_game_documents(system.clone(), filename.clone()),
        super::manuals::local_manuals_inner(&state, &system, all_titles.clone(), saved_manuals),
        super::videos::videos_for_titles(&state, &system, all_titles.clone()),
        super::resources::resource_links_for_titles(&state, &system, all_titles),
    );
    let manual_suggestions = super::manuals::manual_recommendations_from_rows(
        &state,
        &base_title,
        &saved_manual_keys,
        manual_rows,
    );
    let video_suggestions = super::videos::provider_videos_from_links(video_rows);

    Ok(RomDetail {
        game,
        size_bytes,
        is_m3u: entry.is_m3u,
        is_favorite,
        user_screenshots,
        variant_count,
        is_hack,
        is_special,
        base_title,
        rename_allowed,
        rename_reason,
        disc_info,
        library_resources,
        documents: documents.unwrap_or_default(),
        local_manuals: local_manuals.unwrap_or_default(),
        saved_videos: saved_videos.unwrap_or_default(),
        saved_resource_links: saved_resource_links.unwrap_or_default(),
        manual_suggestions,
        video_suggestions,
    })
}

/// Build a minimal [`RomDetail`] for a multimedia/utility entry (an Alpha Player
/// movie or audio file) that has no `game_library` row. Carries only the
/// marker-derived title and path so the detail page opens cleanly; all the
/// game-only sections (metadata, box art, RetroAchievements, resources) stay
/// empty and rename is disabled. See [`systems::is_multimedia_system`].
#[cfg(feature = "ssr")]
async fn multimedia_rom_detail(
    storage: &StorageLocation,
    system: &str,
    filename: &str,
) -> RomDetail {
    let rom_path = format!("/roms/{system}/{filename}");
    let game_ref = GameRef::from_parts(system, filename.to_string(), rom_path.clone(), None);
    let display_name = game_ref
        .display_name
        .clone()
        .unwrap_or_else(|| filename.to_string());
    let base_title = title_utils::base_title(&display_name);

    let is_favorite = favorites::is_favorite(storage, system, filename).await;
    let size_bytes = list_rom_group(storage, system, &rom_path)
        .map(|group| group.iter().map(|file| file.size_bytes).sum())
        .unwrap_or(0);

    let game = GameInfo {
        system: game_ref.system,
        system_display: game_ref.system_display,
        rom_filename: game_ref.rom_filename,
        rom_path: game_ref.rom_path,
        display_name,
        year: String::new(),
        release_date: None,
        release_precision: None,
        release_region_used: None,
        genre: String::new(),
        developer: String::new(),
        players: 0,
        cooperative: false,
        rotation: None,
        driver_status: None,
        is_clone: None,
        parent_rom: None,
        arcade_category: None,
        is_mature: false,
        arcade_board: None,
        arcade_board_tag: None,
        region: None,
        ra_id: String::new(),
        ra_count: 0,
        description: None,
        rating: None,
        publisher: None,
        box_art_url: None,
        screenshot_url: None,
        title_url: None,
    };

    RomDetail {
        game,
        size_bytes,
        is_m3u: false,
        is_favorite,
        user_screenshots: Vec::new(),
        variant_count: 0,
        is_hack: false,
        is_special: false,
        base_title,
        rename_allowed: false,
        rename_reason: None,
        disc_info: None,
        library_resources: Vec::new(),
        documents: Vec::new(),
        local_manuals: Vec::new(),
        saved_videos: Vec::new(),
        saved_resource_links: Vec::new(),
        manual_suggestions: Vec::new(),
        video_suggestions: Vec::new(),
    }
}

/// One trip through the reader pool to fetch every `library_game_resource`
/// row for this ROM. Consumers partition by `resource_type` / `source`
/// (e.g. the Shmups Wiki strategy-guide link picks the row with
/// `resource_type == STRATEGY_GUIDE && source == SHMUPS_WIKI_SOURCE`).
/// Returns an empty Vec on pool-acquire failure or SQL error — the link
/// surfaces are best-effort and never block detail-page render.
#[cfg(feature = "ssr")]
async fn load_library_resources(
    state: &crate::api::AppState,
    system: &str,
    filename: &str,
) -> Vec<LibraryResourceLink> {
    let sys_owned = system.to_string();
    let fname_owned = filename.to_string();
    let result = state
        .library_reader
        .read(move |conn| LibraryDb::game_resources_for_rom(conn, &sys_owned, &fname_owned))
        .await;
    let rows: Vec<LibraryGameResource> = match result {
        Some(Ok(rows)) => rows,
        Some(Err(e)) => {
            tracing::warn!(
                system = %system,
                filename = %filename,
                error = %e,
                "load_library_resources: SQL failed; surfacing no external links"
            );
            Vec::new()
        }
        None => Vec::new(),
    };
    rows.into_iter().map(LibraryResourceLink::from).collect()
}

/// Reject paths that attempt directory traversal.
#[cfg(feature = "ssr")]
fn validate_path_safe(path: &str) -> Result<(), ServerFnError> {
    if path.contains("..") || path.contains('\\') {
        return Err(ServerFnError::new("Invalid path"));
    }
    Ok(())
}

/// Get the file group for a ROM (for delete confirmation UI).
#[server(prefix = "/sfn")]
pub async fn get_rom_file_group(
    system: String,
    relative_path: String,
) -> Result<RomFileGroup, ServerFnError> {
    validate_path_safe(&relative_path)?;
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // The group is built entirely from synchronous storage reads (read_dir,
    // metadata) that can stall on a slow USB/NFS mount, so run them off the
    // async runtime to avoid tying up a tokio worker.
    tokio::task::spawn_blocking(move || build_rom_file_group(&storage, &system, &relative_path))
        .await
        .map_err(|_| ServerFnError::new("Failed to read ROM files"))?
        .map_err(|e| super::to_user_error("Failed to read ROM files", e))
}

/// Assemble a [`RomFileGroup`] via blocking filesystem reads. Runs on the
/// blocking pool (see [`get_rom_file_group`]).
#[cfg(feature = "ssr")]
fn build_rom_file_group(
    storage: &StorageLocation,
    system: &str,
    relative_path: &str,
) -> Result<RomFileGroup, CoreError> {
    let mut group = list_rom_group(storage, system, relative_path)?;

    // If this ROM is part of a multi-disc set (no M3U), include sibling discs.
    let rom_filename = std::path::Path::new(relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(disc_info) = detect_disc_set(storage, system, &rom_filename) {
        let system_dir = storage.system_roms_dir(system);
        for sibling in &disc_info.siblings {
            if *sibling == rom_filename {
                continue; // Already in the group as Primary.
            }
            let sibling_path = system_dir.join(sibling);
            if sibling_path.exists() {
                let size = std::fs::metadata(&sibling_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                group.push(GroupedFile {
                    path: sibling_path,
                    size_bytes: size,
                    kind: FileKind::Disc,
                });
            }
        }
    }

    let total_size: u64 = group.iter().map(|g| g.size_bytes).sum();

    // How many individual files a directory entry expands into before the
    // dialog shows a count summary instead — a ScummVM game folder can hold
    // hundreds of files.
    const DATA_DIR_DISPLAY_CAP: usize = 8;

    let mut files = Vec::with_capacity(group.len());
    for g in group {
        let name = g
            .path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| g.path.display().to_string());

        // A DataDir (e.g. a MAME CHD companion folder, a ScummVM game
        // folder) is one GroupedFile representing a whole directory. Expand
        // it into its individual files so the delete confirmation shows what
        // will actually be removed, unless there are too many to usefully
        // list — then one summary row carrying the true file count.
        if g.kind == FileKind::DataDir {
            let (contents, count) = list_data_dir_contents(&g.path, DATA_DIR_DISPLAY_CAP);
            if count > contents.len() {
                files.push(RomFileEntry {
                    filename: format!("{name}/"),
                    size_bytes: g.size_bytes,
                    dir_file_count: Some(count),
                });
            } else {
                for (relative, size_bytes) in contents {
                    files.push(RomFileEntry {
                        filename: format!("{name}/{}", relative.display()),
                        size_bytes,
                        dir_file_count: None,
                    });
                }
            }
        } else {
            files.push(RomFileEntry {
                filename: name,
                size_bytes: g.size_bytes,
                dir_file_count: None,
            });
        }
    }

    Ok(RomFileGroup { files, total_size })
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(system: String, relative_path: String) -> Result<(), ServerFnError> {
    validate_path_safe(&relative_path)?;
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "delete ROMs").await?;
    let storage = state.storage();

    // Extract ROM filename for cleanup.
    let rom_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    // Check for multi-disc set — include siblings in the deletion.
    let disc_siblings: Vec<String> = detect_disc_set(&storage, &system, &rom_filename)
        .map(|di| di.siblings)
        .unwrap_or_default();

    // Delete the primary ROM group.
    let report = delete_rom_group(&storage, &system, &relative_path)
        .map_err(|e| super::to_user_error("Failed to delete ROM", e))?;

    if !report.errors.is_empty() {
        tracing::warn!("Errors during ROM group delete: {:?}", report.errors);
    }

    // Delete multi-disc siblings (if any).
    for sibling in &disc_siblings {
        if *sibling == rom_filename {
            continue; // Already deleted as part of the primary group.
        }
        let sibling_rel = format!("roms/{system}/{sibling}");
        if let Err(e) = delete_rom_group(&storage, &system, &sibling_rel) {
            tracing::warn!("Failed to delete disc sibling {sibling}: {e}");
        }
    }

    // Phase 3: Orphan data cascade — clean up associated data.
    let filenames_to_clean: Vec<String> = if disc_siblings.is_empty() {
        vec![rom_filename]
    } else {
        disc_siblings
    };

    for fname in &filenames_to_clean {
        delete_rom_cleanup(&state, &storage, &system, fname).await;
    }

    if let Err(e) = state
        .library
        .clear_system_and_invalidate_caches(system, &state.library_writer)
        .await
    {
        tracing::debug!("post-mutation system library clear skipped: {e}");
    }
    state.library.invalidate_favorites().await;
    state.invalidate_user_caches().await;

    Ok(())
}

#[server(prefix = "/sfn")]
pub async fn get_user_captures(
    system: String,
    rom_filename: String,
) -> Result<Vec<ScreenshotUrl>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    // Reads the captures directory (read_dir), which can stall on slow storage;
    // keep it off the async runtime.
    tokio::task::spawn_blocking(move || screenshot_urls_for_rom(&storage, &system, &rom_filename))
        .await
        .map_err(|_| ServerFnError::new("Failed to read captures"))
}

#[server(prefix = "/sfn")]
pub async fn delete_user_capture(
    system: String,
    rom_filename: String,
    screenshot_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "delete captures").await?;
    let storage = state.storage();

    screenshots::delete_screenshot_for_rom(&storage, &system, &rom_filename, &screenshot_filename)
        .map_err(|e| super::to_user_error("Failed to delete capture", e))?;

    Ok(())
}

#[cfg(feature = "ssr")]
fn screenshot_urls_for_rom(
    storage: &StorageLocation,
    system: &str,
    rom_filename: &str,
) -> Vec<ScreenshotUrl> {
    screenshots::find_screenshots_for_rom(storage, system, rom_filename)
        .into_iter()
        .map(|s| ScreenshotUrl {
            filename: s.filename.clone(),
            url: format!("/captures/{system}/{}", s.filename),
            timestamp: s.timestamp,
        })
        .collect()
}

/// Find screenshot files matching a ROM filename prefix.
///
/// Returns `(path, suffix)` pairs where suffix starts with `_` or `.`
/// (e.g., `_001.png`, `.png`).
#[cfg(feature = "ssr")]
fn find_matching_screenshots(
    captures_dir: &std::path::Path,
    rom_filename: &str,
) -> Vec<(std::path::PathBuf, String)> {
    let mut matches = Vec::new();
    if captures_dir.exists()
        && let Ok(entries) = std::fs::read_dir(captures_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.strip_prefix(rom_filename)
                && screenshots::screenshot_belongs_to_rom(&name, rom_filename)
            {
                matches.push((entry.path(), rest.to_string()));
            }
        }
    }
    matches
}

/// Clean up orphaned data after a ROM deletion.
#[cfg(feature = "ssr")]
async fn delete_rom_cleanup(
    state: &crate::api::AppState,
    storage: &StorageLocation,
    system: &str,
    rom_filename: &str,
) {
    // 1 & 2. Delete matching favorites (all subfolders) and screenshots. Both
    // walk storage directories, so run them off the async runtime to avoid
    // stalling a tokio worker on slow USB/NFS.
    let _ = tokio::task::spawn_blocking({
        let storage = storage.clone();
        let system = system.to_string();
        let rom_filename = rom_filename.to_string();
        move || {
            let fav_filename = format!("{system}@{rom_filename}.fav");
            delete_fav_recursive(&storage.favorites_dir(), &fav_filename);

            let captures_dir = storage.captures_dir().join(&system);
            for (path, _) in find_matching_screenshots(&captures_dir, &rom_filename) {
                let _ = std::fs::remove_file(path);
            }
        }
    })
    .await;

    // 3. Delete user_data.db entries (videos, box art overrides).
    if let Err(e) = state
        .user_data_writer
        .try_write({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                UserDataDb::delete_for_rom(conn, &system, &rom_filename);
            }
        })
        .await
    {
        tracing::warn!("ROM delete user-data cascade failed: {e}");
    }

    // 4. Delete library.db game_library entry.
    let deleted_entry = match state
        .library_writer
        .try_write({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                let mut matches = LibraryDb::lookup_game_entries(
                    conn,
                    &[(system.as_str(), rom_filename.as_str())],
                )?;
                let deleted_entry = matches.remove(&(system.clone(), rom_filename.clone()));
                LibraryDb::delete_for_rom(conn, &system, &rom_filename);
                Ok::<Option<GameEntry>, CoreError>(deleted_entry)
            }
        })
        .await
    {
        Ok(Ok(entry)) => entry,
        Ok(Err(e)) => {
            tracing::warn!("ROM delete library cascade failed: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("ROM delete library cascade failed: {e}");
            return;
        }
    };

    let active_entries = match state
        .library_reader
        .try_read({
            let system = system.to_string();
            move |conn| LibraryDb::load_system_entries(conn, &system)
        })
        .await
    {
        Ok(Ok(entries)) => entries,
        Ok(Err(e)) => {
            tracing::warn!("ROM delete thumbnail cleanup skipped: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("ROM delete thumbnail cleanup skipped: {e}");
            return;
        }
    };

    let (display_name, box_art_url) = deleted_entry
        .as_ref()
        .map(|entry| (entry.display_name.as_deref(), entry.box_art_url.as_deref()))
        .unwrap_or((None, None));
    let orphans = thumbnails::orphaned_thumbnail_files_for_deleted_rom(
        &storage.root,
        system,
        rom_filename,
        display_name,
        box_art_url,
        &active_entries,
    );
    if orphans.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || thumbnails::delete_thumbnail_files(&orphans)).await;
}

/// Recursively search for and delete a .fav file in the favorites directory tree.
#[cfg(feature = "ssr")]
fn delete_fav_recursive(dir: &std::path::Path, fav_filename: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                // Check if the file exists directly in this subdir.
                let candidate = path.join(fav_filename);
                if candidate.exists() {
                    let _ = std::fs::remove_file(&candidate);
                }
                delete_fav_recursive(&path, fav_filename);
            }
        } else if entry.file_name().to_string_lossy() == fav_filename {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(
    system: String,
    relative_path: String,
    new_filename: String,
) -> Result<String, ServerFnError> {
    validate_path_safe(&relative_path)?;
    validate_path_safe(&new_filename)?;
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "rename ROMs").await?;
    let storage = state.storage();

    // Extract old filename for cascade.
    let old_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let new_path =
        rename_rom_file(&storage, &system, &relative_path, &new_filename).map_err(|e| {
            // A name collision is actionable — the user can pick a different name —
            // so surface it distinctly (without the underlying path) instead of the
            // generic failure message.
            if matches!(e, CoreError::RenameTargetExists(_)) {
                ServerFnError::new("That name is already in use")
            } else {
                super::to_user_error("Failed to rename ROM", e)
            }
        })?;

    // Phase 3: Rename cascade — update all associated data.
    rename_rom_cascade(&state, &storage, &system, &old_filename, &new_filename).await;

    if let Err(e) = state
        .library
        .clear_system_and_invalidate_caches(system, &state.library_writer)
        .await
    {
        tracing::debug!("post-mutation system library clear skipped: {e}");
    }
    state.library.invalidate_favorites().await;
    state.invalidate_user_caches().await;

    Ok(new_path.display().to_string())
}

/// Cascade rename updates to all data sources.
///
/// Errors are logged but do not block the rename — the file rename
/// has already succeeded by the time this is called.
#[cfg(feature = "ssr")]
async fn rename_rom_cascade(
    state: &crate::api::AppState,
    storage: &StorageLocation,
    system: &str,
    old_filename: &str,
    new_filename: &str,
) {
    // 1 & 2. Rename favorites (.fav rename + content update) and screenshots.
    // Both walk storage directories, so run them off the async runtime to
    // avoid stalling a tokio worker on slow USB/NFS.
    let _ = tokio::task::spawn_blocking({
        let storage = storage.clone();
        let system = system.to_string();
        let old_filename = old_filename.to_string();
        let new_filename = new_filename.to_string();
        move || {
            let old_fav = format!("{system}@{old_filename}.fav");
            let new_fav = format!("{system}@{new_filename}.fav");
            rename_fav_recursive(
                &storage.favorites_dir(),
                &old_fav,
                &new_fav,
                &system,
                &new_filename,
            );

            let captures_dir = storage.captures_dir().join(&system);
            for (path, rest) in find_matching_screenshots(&captures_dir, &old_filename) {
                let new_name = format!("{new_filename}{rest}");
                let new_path = captures_dir.join(&new_name);
                if let Err(e) = std::fs::rename(&path, &new_path) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    tracing::warn!("Failed to rename screenshot {name} -> {new_name}: {e}");
                }
            }
        }
    })
    .await;

    // 3. Update user_data.db (box art overrides, game videos).
    if let Err(e) = state
        .user_data_writer
        .try_write({
            let system = system.to_string();
            let old_filename = old_filename.to_string();
            let new_filename = new_filename.to_string();
            move |conn| {
                UserDataDb::rename_for_rom(conn, &system, &old_filename, &new_filename);
            }
        })
        .await
    {
        tracing::warn!("ROM rename user-data cascade failed: {e}");
    }

    // 4. Update library.db game_library entry.
    if let Err(e) = state
        .library_writer
        .try_write({
            let system = system.to_string();
            let old_filename = old_filename.to_string();
            let new_filename = new_filename.to_string();
            move |conn| {
                LibraryDb::rename_for_rom(conn, &system, &old_filename, &new_filename);
            }
        })
        .await
    {
        tracing::warn!("ROM rename library cascade failed: {e}");
    }
}

/// Recursively find and rename a .fav file, updating its content too.
#[cfg(feature = "ssr")]
fn rename_fav_recursive(
    dir: &std::path::Path,
    old_fav: &str,
    new_fav: &str,
    system: &str,
    new_filename: &str,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                rename_fav_recursive(&path, old_fav, new_fav, system, new_filename);
            }
        } else if entry.file_name().to_string_lossy() == old_fav {
            let new_path = path.parent().unwrap_or(dir).join(new_fav);
            // Update the content (rom_path inside the .fav file).
            let new_content = format!("/roms/{system}/{new_filename}");
            if let Err(e) = std::fs::write(&path, &new_content) {
                tracing::warn!("Failed to update .fav content: {e}");
            }
            if let Err(e) = std::fs::rename(&path, &new_path) {
                tracing::warn!("Failed to rename .fav file: {e}");
            }
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String, return_to: String) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        #[cfg(feature = "ssr")]
        redirect_after_progressive_form(&return_to);
        return Ok("Launch simulated (standalone mode)".into());
    }

    super::require_storage_mutation_allowed(&state, "launch games").await?;
    let storage = state.storage();

    // Launching goes through the RePlayOS API: no integration ⇒ point the
    // user at onboarding instead of failing cryptically.
    let api = state
        .replay_api
        .clone()
        .filter(|api| api.client().has_token())
        .ok_or_else(|| {
            ServerFnError::new(
                "Launching games needs the RePlayOS Net Control connection — set it up in Settings",
            )
        })?;

    replay_control_core_server::launch::validate_rom_exists(&storage, &rom_path)
        .await
        .map_err(|e| super::to_user_error("Game file not found", e))?;
    let (system, game_file) = replay_control_core_server::launch::launch_parts(&rom_path)
        .map_err(|e| super::to_user_error("Invalid ROM path", e))?;

    tracing::info!(rom = %rom_path, system, game_file, "launching game via RePlayOS API");
    if let Err(e) = api.client().load_game(system, game_file).await {
        // Feed connection-state failures (401 after a TV-side code reset,
        // frontend down) into the status machine so the UI surfaces them.
        api.report_error(&e);
        return Err(super::to_user_error("Failed to launch game", e));
    }

    // Write our own recents marker even though RePlayOS writes one on
    // `load_game` too. Measured on the dev Pi (2026-06-14, NFS storage):
    // RePlayOS's marker lands ~120 ms after launch when warm and ~670 ms cold,
    // whereas this write is ~3 ms. Writing it here makes the just-launched game
    // show up in recents / on the home page immediately instead of lagging that
    // window (the launch redirect + invalidate below would otherwise rescan
    // before RePlayOS's marker exists). `list_recents` dedupes the two markers
    // by (system, rom_filename), so the overlap is harmless.
    let rom_filename = game_file
        .rsplit_once('/')
        .map(|(_, filename)| filename)
        .unwrap_or(game_file);
    if let Err(e) = add_recent(&storage, system, rom_filename, &rom_path) {
        tracing::warn!("Failed to create recents entry: {e}");
    }
    state.library.invalidate_after_launch().await;

    #[cfg(feature = "ssr")]
    redirect_after_progressive_form(&return_to);
    Ok("Game launching".into())
}

#[cfg(feature = "ssr")]
fn redirect_after_progressive_form(return_to: &str) {
    if !return_to.is_empty() && return_to.starts_with('/') && !return_to.starts_with("//") {
        leptos_axum::redirect(return_to);
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    // --- validate_path_safe ---

    #[test]
    fn safe_path_accepted() {
        assert!(validate_path_safe("sega_smd/Sonic.md").is_ok());
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(validate_path_safe("../etc/passwd").is_err());
        assert!(validate_path_safe("foo/../../bar").is_err());
    }

    #[test]
    fn backslash_rejected() {
        assert!(validate_path_safe("foo\\bar.rom").is_err());
    }

    #[test]
    fn empty_path_accepted() {
        assert!(validate_path_safe("").is_ok());
    }
}
