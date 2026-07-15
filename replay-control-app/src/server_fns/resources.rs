use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

pub use replay_control_core::user_data_db::ResourceEntry;

/// Get saved external links for a game, shared across regional variants via base_title.
#[server(prefix = "/sfn")]
pub async fn get_game_resource_links(
    system: String,
    base_title: String,
) -> Result<Vec<ResourceEntry>, ServerFnError> {
    let state = super::app_state()?;
    let all_titles = super::resolve_shared_titles(&state, &system, &base_title).await;
    resource_links_for_titles(&state, &system, all_titles).await
}

/// User-saved external links for a pre-resolved title set — one user_data read.
#[cfg(feature = "ssr")]
pub(crate) async fn resource_links_for_titles(
    state: &crate::api::AppState,
    system: &str,
    all_titles: Vec<String>,
) -> Result<Vec<ResourceEntry>, ServerFnError> {
    state
        .user_data_reader
        .read({
            let system = system.to_string();
            move |conn| {
                let title_refs: Vec<&str> = all_titles.iter().map(String::as_str).collect();
                UserDataDb::get_game_resource_links(conn, &system, &title_refs)
            }
        })
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Save an external link for a game.
#[server(prefix = "/sfn")]
pub async fn add_game_resource_link(
    system: String,
    rom_filename: String,
    base_title: String,
    url: String,
    title: String,
    source: Option<String>,
    resource_type: String,
) -> Result<ResourceEntry, ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "add resources").await?;

    if base_title.contains("..") || base_title.contains('/') || base_title.contains('\\') {
        return Err(ServerFnError::new("Invalid title"));
    }
    if rom_filename.contains("..") || rom_filename.contains('/') || rom_filename.contains('\\') {
        return Err(ServerFnError::new("Invalid ROM filename"));
    }

    let url = canonical_resource_url(&url)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ServerFnError::new("Resource title is required"));
    }

    let resource_type = resource_type.trim().to_string();
    if resource_type.is_empty() {
        return Err(ServerFnError::new("Resource type is required"));
    }
    let source = source
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let resource_id = stable_url_id(&normalized_resource_url_key(&url));
    let entry = ResourceEntry {
        id: resource_id,
        url,
        title,
        source,
        resource_type,
        added_at: unix_now_secs(),
        rom_filename: rom_filename.clone(),
    };

    state
        .user_data_writer
        .try_write({
            let entry = entry.clone();
            move |conn| {
                UserDataDb::add_game_resource_link(
                    conn,
                    &system,
                    &rom_filename,
                    &base_title,
                    &entry,
                )
            }
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(entry)
}

/// Remove a saved external link from a game.
#[server(prefix = "/sfn")]
pub async fn remove_game_resource_link(
    system: String,
    rom_filename: String,
    resource_id: String,
) -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "remove resources").await?;

    state
        .user_data_writer
        .try_write(move |conn| {
            UserDataDb::remove_game_resource_link(conn, &system, &rom_filename, &resource_id)
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[cfg(feature = "ssr")]
fn canonical_resource_url(url: &str) -> Result<String, ServerFnError> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(ServerFnError::new("Resource URL must be HTTP or HTTPS"));
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or_default();
    if without_scheme.trim_matches('/').is_empty() || without_scheme.contains(char::is_whitespace) {
        return Err(ServerFnError::new("Resource URL must be a valid URL"));
    }
    Ok(trimmed.to_string())
}

#[cfg(feature = "ssr")]
fn normalized_resource_url_key(url: &str) -> String {
    url.trim_end_matches('/').to_ascii_lowercase()
}
