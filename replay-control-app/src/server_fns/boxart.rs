use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

/// A box art variant returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxArtVariant {
    pub filename: String,
    pub region_label: String,
    pub is_downloaded: bool,
    pub image_url: Option<String>,
    pub is_active: bool,
}

/// Get all available box art variants for a ROM.
#[server(prefix = "/sfn")]
pub async fn get_boxart_variants(
    system: String,
    rom_filename: String,
) -> Result<Vec<BoxArtVariant>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    let arcade_display =
        replay_control_core_server::arcade_db::display_name_if_arcade(&system, &rom_filename).await;

    // Resolve the current active box art URL first.
    let active_url = crate::server_fns::resolve_box_art_url(
        &state,
        &system,
        &rom_filename,
        arcade_display.as_deref(),
    )
    .await;

    // Gracefully return empty when the DB is temporarily unavailable
    // (e.g., during a metadata import or thumbnail update operation).
    let storage_root = storage.root.clone();
    let Some(core_variants) = state
        .library_pool
        .read({
            move |conn| {
                replay_control_core_server::thumbnail_manifest::find_boxart_variants(
                    conn,
                    &system,
                    &rom_filename,
                    arcade_display.as_deref(),
                    &storage_root,
                    active_url.as_deref(),
                )
            }
        })
        .await
    else {
        return Ok(Vec::new());
    };

    let variants = core_variants
        .into_iter()
        .map(|v| BoxArtVariant {
            filename: v.filename,
            region_label: v.region_label,
            is_downloaded: v.is_downloaded,
            image_url: Some(v.image_url),
            is_active: v.is_active,
        })
        .collect();

    Ok(variants)
}

/// Set a box art override: downloads the variant if needed, persists the choice.
/// Returns the new image URL.
#[server(prefix = "/sfn")]
pub async fn set_boxart_override(
    system: String,
    rom_filename: String,
    variant_filename: String,
) -> Result<String, ServerFnError> {
    use replay_control_core_server::thumbnail_manifest;
    use replay_control_core_server::thumbnails::ThumbnailKind;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Look up the variant in the thumbnail index to get repo/branch info.
    let manifest_match = {
        let repo_names = replay_control_core_server::thumbnails::thumbnail_repo_names(&system)
            .ok_or_else(|| ServerFnError::new(format!("No thumbnail repo for {system}")))?;

        let variant_fn = variant_filename.clone();
        state
            .library_pool
            .read(move |conn| {
                for display_name in repo_names {
                    let url_name =
                        replay_control_core_server::thumbnails::repo_url_name(display_name);
                    let source_name =
                        replay_control_core_server::thumbnails::libretro_source_name(display_name);

                    let branch = LibraryDb::get_data_source(conn, &source_name)
                        .ok()
                        .flatten()
                        .and_then(|s| s.branch)
                        .unwrap_or_else(|| "master".to_string());

                    let entries = LibraryDb::query_thumbnail_index(
                        conn,
                        &source_name,
                        replay_control_core_server::thumbnails::ThumbnailKind::Boxart.repo_dir(),
                    )
                    .unwrap_or_default();

                    if entries.iter().any(|e| e.filename == variant_fn) {
                        return Some(thumbnail_manifest::ManifestMatch {
                            filename: variant_fn.clone(),
                            is_symlink: entries
                                .iter()
                                .find(|e| e.filename == variant_fn)
                                .and_then(|e| e.symlink_target.as_ref())
                                .is_some(),
                            repo_url_name: url_name,
                            branch,
                        });
                    }
                }
                None
            })
            .await
            .flatten()
            .ok_or_else(|| {
                ServerFnError::new(format!(
                    "Variant '{}' not found in thumbnail index",
                    variant_filename
                ))
            })?
    };

    // Check if already downloaded; if not, download it.
    let media_dir = storage
        .rc_dir()
        .join("media")
        .join(&system)
        .join(ThumbnailKind::Boxart.media_dir());
    let local_path = media_dir.join(format!("{variant_filename}.png"));
    let is_valid = replay_control_core_server::thumbnails::is_valid_image(local_path).await;

    if !is_valid {
        let bytes = thumbnail_manifest::download_thumbnail(
            &manifest_match,
            ThumbnailKind::Boxart.repo_dir(),
        )
        .await
        .map_err(|e| ServerFnError::new(format!("Download failed: {e}")))?;

        thumbnail_manifest::save_thumbnail(
            &storage.root,
            &system,
            ThumbnailKind::Boxart,
            &manifest_match.filename,
            bytes,
        )
        .await
        .map_err(|e| ServerFnError::new(format!("Save failed: {e}")))?;
    }

    // Persist the override in user_data.db.
    let boxart_dir = ThumbnailKind::Boxart.media_dir();
    let override_path = format!("{boxart_dir}/{variant_filename}.png");
    state
        .user_data_pool
        .write({
            let system = system.clone();
            let rom_filename = rom_filename.clone();
            move |conn| UserDataDb::set_override(conn, &system, &rom_filename, &override_path)
        })
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Update game_library.box_art_url so the override is visible immediately in list views.
    let image_url = format!("/media/{system}/{boxart_dir}/{variant_filename}.png");
    {
        let url = image_url.clone();
        let sys = system.clone();
        let rom = rom_filename.clone();
        let _ = state
            .library_pool
            .write(move |conn| {
                conn.execute(
                    "UPDATE game_library SET box_art_url = ?1 WHERE system = ?2 AND rom_filename = ?3",
                    [&url, &sys, &rom],
                ).ok();
            })
            .await;
    }
    state.invalidate_user_caches().await;
    Ok(image_url)
}

/// Clear a box art override, reverting to the auto-matched default.
#[server(prefix = "/sfn")]
pub async fn reset_boxart_override(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let sys_for_db = system.clone();
    let rom_for_db = rom_filename.clone();
    state
        .user_data_pool
        .write(move |conn| UserDataDb::remove_override(conn, &system, &rom_filename))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Clear box_art_url so it reverts to enrichment-resolved value on next enrichment run.
    {
        let sys = sys_for_db;
        let rom = rom_for_db;
        let _ = state
            .library_pool
            .write(move |conn| {
                conn.execute(
                    "UPDATE game_library SET box_art_url = NULL WHERE system = ?1 AND rom_filename = ?2",
                    [&sys, &rom],
                ).ok();
            })
            .await;
    }
    state.invalidate_user_caches().await;

    Ok(())
}
