use axum::extract::multipart::Field;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use replay_control_core_server::roms::{MAX_ROM_UPLOAD_BYTES, RomUploadStaging};
use replay_control_core_server::user_data_db::{ManualEntry, ManualOrigin, UserDataDb};

use super::AppState;

const MAX_MANUAL_UPLOAD_BYTES: u64 = 64 * 1024 * 1024;

/// Read a multipart field into memory, aborting with 413 once it exceeds
/// `max_bytes`. Unlike `Field::bytes()`, which buffers the whole field before
/// any size check, this bounds peak memory to `max_bytes` + one chunk — the
/// difference between a rejected request and an OOM on a 512 MB appliance.
async fn read_field_capped(mut field: Field<'_>, max_bytes: u64) -> Result<Vec<u8>, StatusCode> {
    let mut buf = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        if buf.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

async fn upload_rom(
    State(state): State<AppState>,
    Path(system): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .require_storage_ready_or_conflict("upload ROMs")
        .await?;
    let storage = state.storage();
    let mut uploaded = Vec::new();

    while let Ok(Some(mut field)) = multipart.next_field().await {
        let filename = field
            .file_name()
            .map(String::from)
            .unwrap_or_else(|| "unknown".to_string());

        // Stage the upload beside its destination and copy chunk-by-chunk with
        // a running byte cap, so a multi-GB body never lands in memory. The
        // destination is replaced only by the atomic commit after a full,
        // in-bounds transfer — a rejected or interrupted upload leaves any
        // existing ROM of the same name untouched (see `RomUploadStaging`).
        let mut staging = RomUploadStaging::create(&storage, &system, &filename)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        let mut written: u64 = 0;
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    written += chunk.len() as u64;
                    if written > MAX_ROM_UPLOAD_BYTES {
                        return Err(StatusCode::PAYLOAD_TOO_LARGE); // staging dropped → temp removed
                    }
                    if staging.write_all(&chunk).await.is_err() {
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
                Ok(None) => break,
                Err(_) => return Err(StatusCode::BAD_REQUEST),
            }
        }
        staging
            .commit()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        tracing::info!("Uploaded: {} ({} bytes)", filename, written);
        uploaded.push(serde_json::json!({
            "filename": filename,
            "size_bytes": written,
            "path": format!("/roms/{system}/{filename}"),
        }));
    }

    if !uploaded.is_empty() {
        if let Err(e) = state
            .library
            .clear_system_and_invalidate_caches(system.clone(), &state.library_writer)
            .await
        {
            tracing::debug!("post-upload system library clear skipped: {e}");
        }
        state.invalidate_user_caches().await;
    }

    Ok(Json(serde_json::json!({ "uploaded": uploaded })))
}

async fn list_upload_targets(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let summaries = super::library_systems::system_summaries(&state.library_reader).await;
    Json(
        summaries
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "folder_name": s.folder_name,
                    "display_name": s.display_name,
                    "game_count": s.game_count,
                })
            })
            .collect(),
    )
}

async fn upload_manual(
    State(state): State<AppState>,
    Path(system): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .require_storage_ready_or_conflict("upload manuals")
        .await?;

    let mut rom_filename = String::new();
    let mut base_title = String::new();
    let mut title = String::new();
    let mut language = String::new();
    let mut upload: Option<(String, Vec<u8>)> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                let filename = field
                    .file_name()
                    .map(String::from)
                    .unwrap_or_else(|| "manual".to_string());
                let data = read_field_capped(field, MAX_MANUAL_UPLOAD_BYTES).await?;
                upload = Some((filename, data));
            }
            "rom_filename" => {
                rom_filename = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            "base_title" => {
                base_title = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            "title" => {
                title = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            "language" => {
                language = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            _ => {}
        }
    }

    if system.trim().is_empty()
        || rom_filename.trim().is_empty()
        || base_title.trim().is_empty()
        || system.contains("..")
        || system.contains('/')
        || system.contains('\\')
        || rom_filename.contains("..")
        || rom_filename.contains('/')
        || rom_filename.contains('\\')
        || base_title.contains("..")
        || base_title.contains('/')
        || base_title.contains('\\')
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let Some((original_filename, data)) = upload else {
        return Err(StatusCode::BAD_REQUEST);
    };
    if data.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Size is already bounded by read_field_capped above.
    if !is_allowed_manual_filename(&original_filename) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let (extension, mime_type) = validate_manual_bytes(&data).ok_or(StatusCode::BAD_REQUEST)?;
    let manual_id = stable_upload_manual_id(&system, &rom_filename, &data);
    let safe_id = manual_id.replace(':', "_");
    let filename = format!("{safe_id}.{extension}");
    let manuals_dir = state.storage().rc_dir().join("manuals").join(&system);
    let target_path = manuals_dir.join(&filename);
    tokio::fs::create_dir_all(&manuals_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&target_path, &data)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let storage_path = format!("{system}/{filename}");
    let title = title.trim();
    let display_title = if title.is_empty() {
        manual_title_from_filename(&original_filename)
    } else {
        title.to_string()
    };
    let entry = ManualEntry {
        manual_id: manual_id.clone(),
        resource_key: format!("upload:{manual_id}"),
        title: Some(display_title),
        origin: ManualOrigin::Upload,
        provider: Some("user_upload".to_string()),
        url: None,
        storage_path: Some(storage_path.clone()),
        original_filename: Some(original_filename),
        languages: language.trim().to_string(),
        mime_type: mime_type.to_string(),
        size_bytes: Some(data.len() as u64),
        added_at: unix_now_secs(),
    };

    let db_result = state
        .user_data_writer
        .try_write({
            let system = system.clone();
            let rom_filename = rom_filename.clone();
            let base_title = base_title.clone();
            move |conn| {
                UserDataDb::add_game_manual(conn, &system, &rom_filename, &base_title, &entry)
            }
        })
        .await;
    match db_result {
        Ok(Ok(())) => {}
        Ok(Err(_)) | Err(_) => {
            let _ = tokio::fs::remove_file(&target_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    state.invalidate_user_caches().await;

    Ok(Json(serde_json::json!({
        "manual_id": manual_id,
        "url": format!("/owned-manuals/{}", urlencoding::encode(&storage_path)),
        "size_bytes": data.len(),
        "mime_type": mime_type,
    })))
}

fn validate_manual_bytes(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"%PDF-") {
        return Some(("pdf", "application/pdf"));
    }
    std::str::from_utf8(bytes).ok()?;
    Some(("txt", "text/plain"))
}

fn is_allowed_manual_filename(filename: &str) -> bool {
    let lower = filename.trim().to_lowercase();
    lower.ends_with(".pdf") || lower.ends_with(".txt")
}

fn stable_upload_manual_id(system: &str, rom_filename: &str, data: &[u8]) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(system.as_bytes());
    ctx.update(b"\0");
    ctx.update(rom_filename.as_bytes());
    ctx.update(b"\0");
    ctx.update(data);
    let digest = ctx.finish();
    let mut out = String::from("uploadhash:");
    for byte in digest.as_ref() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn manual_title_from_filename(filename: &str) -> String {
    std::path::Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("Manual")
        .to_string()
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/upload/:system", post(upload_rom))
        .route("/manuals/upload/:system", post(upload_manual))
        .route("/upload/targets", get(list_upload_targets))
}
