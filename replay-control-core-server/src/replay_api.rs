//! Native client for the official RePlayOS REST API.
//!
//! Pure wire types live in `replay_control_core::replay_api` (re-exported
//! here); this module adds the HTTP client on top of the shared `reqwest`
//! client. All endpoints are GET; auth is the `X-RePlay-Token` header.
//!
//! Error classification keeps full fidelity (see `ApiError`): 401 means the
//! token was rejected, refused/timeout means the frontend is down or Net
//! Control is off, and other non-2xx statuses carry RePlayOS's
//! `{"error","detail"}` body as the message.
//!
//! The full endpoint surface is implemented (and tested) ahead of its UI on
//! purpose: `set_cmd`/`save_state`/`load_state`/`set_msg`/`set_media` are
//! consumed by the player-bar and config phases of the integration plan.

pub use replay_control_core::replay_api::*;

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::http::shared_client;

const TOKEN_HEADER: &str = "X-RePlay-Token";

/// Measured localhost latencies are 0–3 ms; if the API doesn't answer within
/// this, the frontend is effectively down — that *is* the signal.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

/// Client for the RePlayOS API. Cheap to clone; the token slot is shared so
/// re-onboarding can swap it in place (never auto-swapped on 401).
#[derive(Clone)]
pub struct ReplayApiClient {
    base_url: Arc<str>,
    token: Arc<RwLock<Option<String>>>,
    timeout: Duration,
}

impl ReplayApiClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').into(),
            token: Arc::new(RwLock::new(token.filter(|t| !t.is_empty()))),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Client for the device-local API (`http://127.0.0.1:55356/api/v1`).
    pub fn local(token: Option<String>) -> Self {
        Self::new(LOCAL_BASE_URL, token)
    }

    /// Swap the token this client sends as the `X-RePlay-Token` header —
    /// purely local, nothing is written to RePlayOS. Used only by the
    /// onboarding paths (manual verify, assisted post-restart read) — never
    /// as 401 recovery.
    pub fn swap_local_token(&self, token: Option<String>) {
        *self.token.write().expect("replay api token lock poisoned") =
            token.filter(|t| !t.is_empty());
    }

    pub fn has_token(&self) -> bool {
        self.token
            .read()
            .expect("replay api token lock poisoned")
            .is_some()
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        query: &[(&str, String)],
    ) -> Result<T, ApiError> {
        let token = self
            .token
            .read()
            .expect("replay api token lock poisoned")
            .clone()
            .ok_or(ApiError::MissingToken)?;

        let url = format!("{}/{endpoint}", self.base_url);
        let response = shared_client()
            .get(&url)
            .header(TOKEN_HEADER, token)
            .query(query)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| ApiError::Unreachable {
                reason: e.to_string(),
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| ApiError::Unreachable {
            reason: e.to_string(),
        })?;

        if status.as_u16() == 401 {
            return Err(ApiError::Unauthorized);
        }
        if !status.is_success() {
            let message = serde_json::from_str::<ApiErrorBody>(&body)
                .ok()
                .and_then(|parsed| parsed.message())
                .unwrap_or_else(|| truncate(&body, 200));
            return Err(ApiError::BadStatus {
                status: status.as_u16(),
                message,
            });
        }

        serde_json::from_str(&body).map_err(|e| ApiError::Decode {
            reason: format!("{e}; body: {}", truncate(&body, 200)),
        })
    }

    pub async fn get_version(&self) -> Result<VersionResponse, ApiError> {
        self.get_json("get_version", &[]).await
    }

    pub async fn get_status(&self) -> Result<StatusResponse, ApiError> {
        self.get_json("get_status", &[]).await
    }

    pub async fn get_replay_config(&self) -> Result<ReplayConfigSnapshot, ApiError> {
        self.get_json("get_replay_config", &[]).await
    }

    pub async fn get_media_status(&self) -> Result<MediaStatus, ApiError> {
        self.get_json("get_media_status", &[]).await
    }

    /// Launch a game. `game_file` is relative to the system folder. Works from
    /// the menu and mid-game (including core swaps) without a frontend
    /// restart. Bad paths come back as `BadStatus` with RePlayOS's
    /// "Invalid Game: Game file not found" message.
    pub async fn load_game(&self, system: &str, game_file: &str) -> Result<(), ApiError> {
        self.get_json::<serde_json::Value>(
            "load_game",
            &[
                ("system", system.to_string()),
                ("game_file", game_file.to_string()),
            ],
        )
        .await
        .map(|_| ())
    }

    pub async fn set_cmd(&self, cmd: SetCommand) -> Result<(), ApiError> {
        self.get_json::<serde_json::Value>("set_cmd", &[("cmd", cmd.as_str().to_string())])
            .await
            .map(|_| ())
    }

    /// Save the running game's state. Slots are 1–18 and blind (no occupancy
    /// listing). With no game loaded RePlayOS answers `200` and silently does
    /// nothing — callers gate on play state.
    pub async fn save_state(&self, slot: u8) -> Result<(), ApiError> {
        self.get_json::<serde_json::Value>("save_state", &[("slot", slot.to_string())])
            .await
            .map(|_| ())
    }

    /// Load a saved state. Same slot semantics and silent no-op caveat as
    /// [`Self::save_state`].
    pub async fn load_state(&self, slot: u8) -> Result<(), ApiError> {
        self.get_json::<serde_json::Value>("load_state", &[("slot", slot.to_string())])
            .await
            .map(|_| ())
    }

    /// Show a popup message on the TV. Duration is clamped by RePlayOS to
    /// 1–10 seconds.
    pub async fn set_msg(&self, text: &str, duration_secs: u8) -> Result<(), ApiError> {
        self.get_json::<serde_json::Value>(
            "set_msg",
            &[
                ("text", text.to_string()),
                ("duration", duration_secs.to_string()),
            ],
        )
        .await
        .map(|_| ())
    }

    /// Disc control for multi-disc games. `Next`/`Previous` at the ends return
    /// `409 Media Boundary` — check [`ApiError::is_media_boundary`].
    pub async fn set_media(&self, cmd: MediaCommand) -> Result<(), ApiError> {
        let mut query = vec![("cmd", cmd.as_str().to_string())];
        if let MediaCommand::SetIndex(index) = cmd {
            query.push(("index", index.to_string()));
        }
        self.get_json::<serde_json::Value>("set_media", &query)
            .await
            .map(|_| ())
    }
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= max {
        trimmed.to_string()
    } else {
        let mut end = max;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &trimmed[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{mock_replay_api as serve, refused_replay_api};

    fn client_for(base_url: String) -> ReplayApiClient {
        ReplayApiClient::new(base_url, Some("123456".to_string()))
    }

    #[tokio::test]
    async fn get_status_parses_payload() {
        let base = serve(
            "200 OK",
            r#"{"system":"sega_smd","game_file":"/media/nfs/roms/sega_smd/g.md","game_name":"g.md","paused":false,"view":"game_play","view_id":2,"core_file":"c.so","core_info":"c"}"#,
        );
        let status = client_for(base).get_status().await.unwrap();
        assert!(status.game_loaded());
        assert_eq!(status.view_kind(), Some(View::GamePlay));
    }

    #[tokio::test]
    async fn unauthorized_is_classified() {
        let base = serve("401 Unauthorized", r#"{"error":"Unauthorized"}"#);
        let err = client_for(base).get_version().await.unwrap_err();
        assert_eq!(err, ApiError::Unauthorized);
    }

    #[tokio::test]
    async fn bad_status_carries_replayos_message() {
        let base = serve(
            "404 Not Found",
            r#"{"error":"Invalid Game","detail":"Game file not found"}"#,
        );
        let err = client_for(base)
            .load_game("sega_smd", "missing.md")
            .await
            .unwrap_err();
        match err {
            ApiError::BadStatus { status, message } => {
                assert_eq!(status, 404);
                assert_eq!(message, "Invalid Game: Game file not found");
            }
            other => panic!("expected BadStatus, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn media_boundary_is_detectable() {
        let base = serve(
            "409 Conflict",
            r#"{"error":"Media Boundary","detail":"Already at the last media image"}"#,
        );
        let err = client_for(base)
            .set_media(MediaCommand::Next)
            .await
            .unwrap_err();
        assert!(err.is_media_boundary());
    }

    #[tokio::test]
    async fn refused_connection_is_unreachable() {
        let client = client_for(refused_replay_api());
        let err = client.get_version().await.unwrap_err();
        assert!(matches!(err, ApiError::Unreachable { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn invalid_json_is_decode_error() {
        let base = serve("200 OK", "not json");
        let err = client_for(base).get_status().await.unwrap_err();
        assert!(matches!(err, ApiError::Decode { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_token_short_circuits_without_network() {
        let client = ReplayApiClient::new("http://127.0.0.1:1/api/v1", None);
        let err = client.get_version().await.unwrap_err();
        assert_eq!(err, ApiError::MissingToken);
        client.swap_local_token(Some("123456".to_string()));
        assert!(client.has_token());
        client.swap_local_token(Some(String::new()));
        assert!(!client.has_token(), "empty tokens are treated as absent");
    }
}
