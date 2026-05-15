use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use replay_control_core_server::user_data_db::{ManualEntry, ManualOrigin, UserDataDb};

use super::AppState;

const MAX_MANUAL_UPLOAD_BYTES: usize = 64 * 1024 * 1024;

async fn upload_rom(
    State(state): State<AppState>,
    Path(system): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .require_configured_storage_ready_for_mutation("upload ROMs")
        .await
        .map_err(|_| StatusCode::CONFLICT)?;
    let storage = state.storage();
    let mut uploaded = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field
            .file_name()
            .map(String::from)
            .unwrap_or_else(|| "unknown".to_string());

        // TODO(perf): This loads the entire file into memory. For large ROMs,
        // stream to a temp file instead (e.g., via tokio::io::copy from the
        // field stream to a tokio::fs::File). Acceptable for now since uploads
        // are rare and typically single files.
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;

        replay_control_core_server::roms::write_rom(&storage, &system, &filename, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        tracing::info!("Uploaded: {} ({} bytes)", filename, data.len());
        uploaded.push(serde_json::json!({
            "filename": filename,
            "size_bytes": data.len(),
            "path": format!("/roms/{system}/{filename}"),
        }));
    }

    if !uploaded.is_empty() {
        if let Err(e) = state
            .cache
            .invalidate_system(system.clone(), &state.library_writer)
            .await
        {
            tracing::debug!("post-upload invalidate_system skipped: {e}");
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
        .require_configured_storage_ready_for_mutation("upload manuals")
        .await
        .map_err(|_| StatusCode::CONFLICT)?;

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
                let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                upload = Some((filename, data.to_vec()));
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
    if data.len() > MAX_MANUAL_UPLOAD_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
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
    let display_title = title
        .trim()
        .is_empty()
        .then(|| manual_title_from_filename(&original_filename))
        .unwrap_or_else(|| title.trim().to_string());
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
