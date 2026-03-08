use gloo_net::http::Request;

use crate::types::*;

const BASE: &str = "/api";

pub async fn fetch_info() -> Result<SystemInfo, String> {
    Request::get(&format!("{BASE}/info"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn fetch_systems() -> Result<Vec<SystemSummary>, String> {
    Request::get(&format!("{BASE}/systems"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn fetch_roms(system: &str) -> Result<Vec<RomEntry>, String> {
    Request::get(&format!("{BASE}/systems/{system}/roms"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn fetch_favorites() -> Result<Vec<Favorite>, String> {
    Request::get(&format!("{BASE}/favorites"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn fetch_recents() -> Result<Vec<RecentEntry>, String> {
    Request::get(&format!("{BASE}/recents"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn fetch_last_played() -> Result<Option<RecentEntry>, String> {
    Request::get(&format!("{BASE}/recents/last"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn add_favorite(system: &str, rom_path: &str, grouped: bool) -> Result<Favorite, String> {
    Request::post(&format!("{BASE}/favorites"))
        .json(&serde_json::json!({
            "system": system,
            "rom_path": rom_path,
            "grouped": grouped,
        }))
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

pub async fn remove_favorite(filename: &str, subfolder: Option<&str>) -> Result<(), String> {
    let mut body = serde_json::json!({ "filename": filename });
    if let Some(sub) = subfolder {
        body["subfolder"] = serde_json::json!(sub);
    }

    let resp = Request::delete(&format!("{BASE}/favorites"))
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.ok() {
        Ok(())
    } else {
        Err(format!("Failed: {}", resp.status()))
    }
}

pub async fn group_favorites() -> Result<usize, String> {
    let resp: serde_json::Value = Request::put(&format!("{BASE}/favorites/group"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    Ok(resp["moved"].as_u64().unwrap_or(0) as usize)
}

pub async fn flatten_favorites() -> Result<usize, String> {
    let resp: serde_json::Value = Request::put(&format!("{BASE}/favorites/flatten"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    Ok(resp["moved"].as_u64().unwrap_or(0) as usize)
}
