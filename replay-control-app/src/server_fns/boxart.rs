use super::*;

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

    // Resolve the current active box art URL BEFORE locking metadata_db,
    // because resolve_box_art_url also needs that lock.
    let active_url = crate::server_fns::resolve_box_art_url(&state, &system, &rom_filename);

    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;

    let core_variants = replay_control_core::thumbnail_manifest::find_boxart_variants(
        db,
        &system,
        &rom_filename,
        &storage.root,
        active_url.as_deref(),
    );

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
    use replay_control_core::thumbnail_manifest;
    use replay_control_core::thumbnails::ThumbnailKind;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Look up the variant in the thumbnail index to get repo/branch info.
    let manifest_match = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;

        let repo_names = replay_control_core::thumbnails::thumbnail_repo_names(&system)
            .ok_or_else(|| ServerFnError::new(format!("No thumbnail repo for {system}")))?;

        let mut found = None;
        for display_name in repo_names {
            let url_name = display_name.replace(' ', "_");
            let source_name = format!("libretro:{url_name}");

            let branch = db
                .get_data_source(&source_name)
                .ok()
                .flatten()
                .and_then(|s| s.branch)
                .unwrap_or_else(|| "master".to_string());

            let entries = db
                .query_thumbnail_index(&source_name, "Named_Boxarts")
                .unwrap_or_default();

            if entries.iter().any(|e| e.filename == variant_filename) {
                found = Some(thumbnail_manifest::ManifestMatch {
                    filename: variant_filename.clone(),
                    is_symlink: entries
                        .iter()
                        .find(|e| e.filename == variant_filename)
                        .and_then(|e| e.symlink_target.as_ref())
                        .is_some(),
                    repo_url_name: url_name,
                    branch,
                });
                break;
            }
        }
        found.ok_or_else(|| {
            ServerFnError::new(format!(
                "Variant '{}' not found in thumbnail index",
                variant_filename
            ))
        })?
    };

    // Check if already downloaded; if not, download it.
    let media_dir = storage.rc_dir().join("media").join(&system).join("boxart");
    let local_path = media_dir.join(format!("{variant_filename}.png"));
    let is_valid = local_path
        .metadata()
        .map(|m| m.len() >= 200)
        .unwrap_or(false);

    if !is_valid {
        let m = manifest_match.clone();
        let storage_root = storage.root.clone();
        let sys = system.clone();

        // Run the blocking download in a spawn_blocking context.
        let result = tokio::task::spawn_blocking(move || {
            let bytes = thumbnail_manifest::download_thumbnail(&m, "Named_Boxarts")?;
            thumbnail_manifest::save_thumbnail(
                &storage_root,
                &sys,
                ThumbnailKind::Boxart,
                &m.filename,
                &bytes,
            )?;
            Ok::<_, replay_control_core::error::Error>(())
        })
        .await
        .map_err(|e| ServerFnError::new(format!("Download task failed: {e}")))?;

        result.map_err(|e| ServerFnError::new(format!("Download failed: {e}")))?;
    }

    // Persist the override in user_data.db.
    let override_path = format!("boxart/{variant_filename}.png");
    {
        let ud_guard = state
            .user_data_db()
            .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?;
        let ud_db = ud_guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("User data DB not available"))?;
        ud_db
            .set_override(&system, &rom_filename, &override_path)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    }

    // Invalidate the image cache for this system.
    state.cache.invalidate_system_images(&system);

    let image_url = format!("/media/{system}/boxart/{variant_filename}.png");
    Ok(image_url)
}

/// Clear a box art override, reverting to the auto-matched default.
#[server(prefix = "/sfn")]
pub async fn reset_boxart_override(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    {
        let ud_guard = state
            .user_data_db()
            .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?;
        let ud_db = ud_guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("User data DB not available"))?;
        ud_db
            .remove_override(&system, &rom_filename)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    }

    // Invalidate the image cache for this system.
    state.cache.invalidate_system_images(&system);

    Ok(())
}
