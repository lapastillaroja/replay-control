//! Pure wire types for the official RePlayOS REST API.
//!
//! RePlayOS ≥ 1.7.3 serves `http://<device>:55356/api/v1` from the frontend
//! process, gated on the `system_net_control` config option and authenticated
//! with the `X-RePlay-Token` header (the "Net Control code"). The native
//! client lives in `replay_control_core_server::replay_api`; this module holds
//! the request/response shapes and the integration status enum so both the
//! SSR server and the hydrate client can name them.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// API base URL when Replay Control runs on the RePlayOS device itself.
pub const LOCAL_BASE_URL: &str = "http://127.0.0.1:55356/api/v1";

/// RePlayOS UI views as reported by `get_status.view_id`.
///
/// Menu views are *not* reliable for "is a game running": with a game loaded,
/// menu screens report 0, 1, or 3 depending on navigation history. Map play
/// state off `game_file` instead (see `replay-control-app` now-playing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    SystemList,
    SystemOptions,
    GamePlay,
    GameOptions,
    Achievements,
    Leaderboards,
    Unknown(u8),
}

impl View {
    pub fn from_id(id: u8) -> View {
        match id {
            0 => View::SystemList,
            1 => View::SystemOptions,
            2 => View::GamePlay,
            3 => View::GameOptions,
            4 => View::Achievements,
            5 => View::Leaderboards,
            other => View::Unknown(other),
        }
    }

    pub fn is_game_play(self) -> bool {
        self == View::GamePlay
    }
}

/// `get_status` payload.
///
/// Every field is optional: during UI transitions RePlayOS can return a `200`
/// with fields missing entirely (measured 2026-06-07). Callers hold their
/// previous state on such degenerate payloads instead of flapping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatusResponse {
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub game_file: Option<String>,
    #[serde(default)]
    pub game_name: Option<String>,
    #[serde(default)]
    pub paused: Option<bool>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub view_id: Option<u8>,
    #[serde(default)]
    pub core_file: Option<String>,
    #[serde(default)]
    pub core_info: Option<String>,
    /// Newer RePlayOS builds may report `halt` here. Optional so public
    /// RePlayOS 1.7.3 payloads continue to deserialize unchanged.
    #[serde(default, alias = "state", alias = "play_status")]
    pub status: Option<String>,
    /// Tolerate boolean spellings if the API shape changes before the next
    /// public RePlayOS release.
    #[serde(default, alias = "halted")]
    pub halt: Option<bool>,
}

impl StatusResponse {
    /// Transient transition payload with the load-bearing fields missing —
    /// hold the previous state rather than interpreting it.
    pub fn is_degenerate(&self) -> bool {
        self.view_id.is_none() && self.status.is_none() && self.halt.is_none()
    }

    pub fn view_kind(&self) -> Option<View> {
        self.view_id.map(View::from_id)
    }

    /// A game is loaded. RePlayOS has no "exit a game" concept: once loaded,
    /// `game_file` stays set until shutdown/restart or another launch.
    pub fn game_loaded(&self) -> bool {
        self.game_file.as_deref().is_some_and(|f| !f.is_empty())
    }

    pub fn is_halted(&self) -> bool {
        self.halt.unwrap_or(false)
            || self
                .status
                .as_deref()
                .is_some_and(|status| status.eq_ignore_ascii_case("halt"))
            || self
                .view
                .as_deref()
                .is_some_and(|view| view.eq_ignore_ascii_case("halt"))
    }
}

/// `get_version` payload, e.g. `{"version": "RePlayOS v1.7.3"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionResponse {
    pub version: String,
}

/// One disc entry in `get_media_status.images`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaImage {
    pub index: u32,
    pub number: u32,
    #[serde(default)]
    pub label: String,
}

/// `get_media_status` payload (disk control for multi-disc games).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaStatus {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub tray_open: bool,
    #[serde(default)]
    pub current_index: u32,
    #[serde(default)]
    pub current_number: u32,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub images: Vec<MediaImage>,
}

impl MediaStatus {
    /// Current-disc summary for now-playing surfaces, `None` when the loaded
    /// game has no disc control (cartridges, ScummVM, menu).
    pub fn disc_info(&self) -> Option<DiscInfo> {
        (self.available && self.count > 0).then_some(DiscInfo {
            number: self.current_number,
            count: self.count,
        })
    }
}

/// Current disc of a multi-disc game ("Disc 2/4") carried inside the
/// now-playing state and refreshed on every detection poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscInfo {
    pub number: u32,
    pub count: u32,
}

/// `get_replay_config` payload: `{"modification_num": N, "config": {...}}`.
///
/// `modification_num` is a session-local change counter (resets on frontend
/// restart). The `wifi_*`/`nfs_*` families, passwords, and tokens are absent
/// from `config` entirely — those stay readable only from replay.cfg.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplayConfigSnapshot {
    #[serde(default)]
    pub modification_num: u64,
    #[serde(default)]
    pub config: BTreeMap<String, serde_json::Value>,
}

impl ReplayConfigSnapshot {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.config.get(key).and_then(|value| value.as_str())
    }
}

/// Error body RePlayOS returns on failures: `{"error": "...", "detail": "..."}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ApiErrorBody {
    /// "Invalid Game: Game file not found" style message, or `None` when the
    /// body carried neither field.
    pub fn message(&self) -> Option<String> {
        match (self.error.as_deref(), self.detail.as_deref()) {
            (Some(error), Some(detail)) => Some(format!("{error}: {detail}")),
            (Some(error), None) => Some(error.to_string()),
            (None, Some(detail)) => Some(detail.to_string()),
            (None, None) => None,
        }
    }
}

/// Commands accepted by `set_cmd`.
///
/// `Halt` is the CRT-photo freeze-frame. Newer RePlayOS builds report it in
/// `get_status`; older 1.7.3 builds accept the command but do not expose the
/// state, so callers must tolerate missing halt status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetCommand {
    Reboot,
    PowerOff,
    GameReset,
    GameRestart,
    Screenshot,
    Halt,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl SetCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            SetCommand::Reboot => "reboot",
            SetCommand::PowerOff => "power_off",
            SetCommand::GameReset => "game_reset",
            SetCommand::GameRestart => "game_restart",
            SetCommand::Screenshot => "screenshot",
            SetCommand::Halt => "halt",
            SetCommand::VolumeUp => "volume_up",
            SetCommand::VolumeDown => "volume_down",
            SetCommand::Mute => "mute",
        }
    }
}

/// Commands accepted by `set_media`. `next`/`previous` return `409 Media
/// Boundary` at the ends (no wraparound); `next` auto-closes the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaCommand {
    OpenTray,
    CloseTray,
    Next,
    Previous,
    SetIndex(u32),
}

impl MediaCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaCommand::OpenTray => "open_tray",
            MediaCommand::CloseTray => "close_tray",
            MediaCommand::Next => "next",
            MediaCommand::Previous => "previous",
            MediaCommand::SetIndex(_) => "set_index",
        }
    }
}

/// Classified client-side error for RePlayOS API calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ApiError {
    /// The client has no token configured — onboarding hasn't happened.
    MissingToken,
    /// Token rejected (401). Manual-only rotation: the user reset the code on
    /// the TV. Never auto-recovered; surfaces as `ReplayApiStatus::Unauthorized`.
    Unauthorized,
    /// Connection refused / timed out. With the measured 0–3 ms localhost
    /// latencies, "slow" means the frontend is down or Net Control is off.
    Unreachable { reason: String },
    /// Non-2xx other than 401 (e.g. `404 Invalid Game`, `409 Media Boundary`).
    BadStatus { status: u16, message: String },
    /// `200` body that didn't parse as the expected shape.
    Decode { reason: String },
}

impl ApiError {
    pub fn is_media_boundary(&self) -> bool {
        matches!(self, ApiError::BadStatus { status: 409, .. })
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::MissingToken => write!(f, "RePlayOS API token is not configured"),
            ApiError::Unauthorized => write!(f, "RePlayOS rejected the Net Control code"),
            ApiError::Unreachable { reason } => write!(f, "RePlayOS API unreachable: {reason}"),
            ApiError::BadStatus { status, message } => {
                write!(f, "RePlayOS API error (HTTP {status}): {message}")
            }
            ApiError::Decode { reason } => {
                write!(f, "RePlayOS API returned an unexpected response: {reason}")
            }
        }
    }
}

impl std::error::Error for ApiError {}

/// Replay Control's connection state to the RePlayOS API. Broadcast to the
/// client over the events SSE stream and rendered by the onboarding surfaces
/// and the `ReplayApiStatusBanner`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReplayApiStatus {
    /// No token stored — onboarding hasn't completed. Setup checklist and the
    /// Net Control settings page own this state; no banner.
    #[default]
    NotConfigured,
    /// Assisted enable is mid-flight; the API being down is expected.
    PendingRestart,
    Active {
        version: String,
    },
    /// Stored token rejected (the code was reset on the TV) — user re-onboards.
    Unauthorized,
    /// RePlayOS too old for the API (< 1.7.3). Detected pre-onboarding by the
    /// absence of the `system_net_control` key in replay.cfg.
    Unsupported {
        version: Option<String>,
    },
    /// Transient: API unreachable while it should be up. Polling keeps
    /// retrying with backoff and self-recovers to `Active` — no user action.
    Error {
        reason: String,
    },
}

impl ReplayApiStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, ReplayApiStatus::Active { .. })
    }
}

/// Sub-state of a loaded game for now-playing surfaces. Derived from
/// `get_status` per the measured mapping: `game_file` empty ⇒ no game (menu);
/// otherwise `halt` ⇒ `Halted`, `view == game_play` ⇒ `Playing`, `paused` ⇒
/// `Paused`, any other view ⇒ `InMenu` (the game stays loaded behind RePlayOS
/// menus — running or paused per the `system_ui_pauses_core` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayState {
    Playing,
    Paused,
    Halted,
    InMenu,
}

impl PlayState {
    pub fn from_status(view: Option<View>, paused: bool, halted: bool) -> PlayState {
        match (view, halted) {
            (_, true) => PlayState::Halted,
            (Some(View::GamePlay), false) if !paused => PlayState::Playing,
            _ if paused => PlayState::Paused,
            _ => PlayState::InMenu,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured from the dev Pi, 2026-06-06.
    const REAL_STATUS: &str = r#"{"system":"sega_smd","game_file":"/media/nfs/roms/sega_smd/00 Clean Romset/3 Ninjas Kick Back (USA).md","game_name":"3 Ninjas Kick Back (USA).md","paused":false,"view":"game_play","view_id":2,"core_file":"genesis_plus_gx_libretro.so","core_info":"Genesis Plus  v1.7.4"}"#;

    #[test]
    fn status_parses_real_payload() {
        let status: StatusResponse = serde_json::from_str(REAL_STATUS).unwrap();
        assert!(!status.is_degenerate());
        assert!(status.game_loaded());
        assert_eq!(status.view_kind(), Some(View::GamePlay));
        assert_eq!(
            status.game_name.as_deref(),
            Some("3 Ninjas Kick Back (USA).md")
        );
        assert_eq!(status.paused, Some(false));
    }

    #[test]
    fn status_tolerates_degenerate_payload() {
        // Observed during UI transitions: 200 with fields missing entirely.
        let status: StatusResponse = serde_json::from_str(r#"{"game_file":""}"#).unwrap();
        assert!(status.is_degenerate());
        assert!(!status.game_loaded());
        assert_eq!(status.view_kind(), None);
    }

    #[test]
    fn status_tolerates_optional_halt_shape() {
        let status: StatusResponse = serde_json::from_str(
            r#"{"game_file":"/media/sd/roms/sega_smd/Sonic.md","view_id":2,"paused":true,"status":"halt"}"#,
        )
        .unwrap();
        assert!(!status.is_degenerate());
        assert!(status.is_halted());

        let status: StatusResponse =
            serde_json::from_str(r#"{"view_id":2,"halted":true}"#).unwrap();
        assert!(status.is_halted());
    }

    #[test]
    fn empty_game_file_means_no_game() {
        let status: StatusResponse =
            serde_json::from_str(r#"{"game_file":"","view_id":0,"paused":false}"#).unwrap();
        assert!(!status.is_degenerate());
        assert!(!status.game_loaded());
    }

    #[test]
    fn config_snapshot_models_nesting() {
        let snapshot: ReplayConfigSnapshot = serde_json::from_str(
            r#"{"modification_num":3,"config":{"system_storage":"nfs","system_net_control":"true"}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.modification_num, 3);
        assert_eq!(snapshot.get_str("system_storage"), Some("nfs"));
        assert_eq!(snapshot.get_str("missing"), None);
    }

    #[test]
    fn media_status_disc_info() {
        // Captured shape from the 4-disc Slam City session, 2026-06-07.
        let media: MediaStatus = serde_json::from_str(
            r#"{"available":true,"tray_open":false,"current_index":1,"current_number":2,"count":4,"images":[{"index":0,"number":1,"label":"Disc 1"},{"index":1,"number":2,"label":"Disc 2"},{"index":2,"number":3,"label":"Disc 3"},{"index":3,"number":4,"label":"Disc 4"}]}"#,
        )
        .unwrap();
        assert_eq!(
            media.disc_info(),
            Some(DiscInfo {
                number: 2,
                count: 4
            })
        );

        let unavailable: MediaStatus =
            serde_json::from_str(r#"{"available":false,"count":0}"#).unwrap();
        assert_eq!(unavailable.disc_info(), None);
    }

    #[test]
    fn play_state_mapping() {
        use PlayState::*;
        assert_eq!(
            PlayState::from_status(Some(View::GamePlay), false, false),
            Playing
        );
        assert_eq!(
            PlayState::from_status(Some(View::GamePlay), true, false),
            Paused
        );
        assert_eq!(
            PlayState::from_status(Some(View::GamePlay), true, true),
            Halted
        );
        assert_eq!(
            PlayState::from_status(Some(View::GameOptions), true, false),
            Paused
        );
        assert_eq!(
            PlayState::from_status(Some(View::SystemList), false, false),
            InMenu
        );
        assert_eq!(PlayState::from_status(None, false, false), InMenu);
    }

    #[test]
    fn api_error_body_message() {
        let body: ApiErrorBody =
            serde_json::from_str(r#"{"error":"Invalid Game","detail":"Game file not found"}"#)
                .unwrap();
        assert_eq!(
            body.message().as_deref(),
            Some("Invalid Game: Game file not found")
        );
        assert_eq!(ApiErrorBody::default().message(), None);
    }

    #[test]
    fn replay_api_status_serde_round_trip() {
        for status in [
            ReplayApiStatus::NotConfigured,
            ReplayApiStatus::PendingRestart,
            ReplayApiStatus::Active {
                version: "RePlayOS v1.7.3".into(),
            },
            ReplayApiStatus::Unauthorized,
            ReplayApiStatus::Unsupported { version: None },
            ReplayApiStatus::Error {
                reason: "down".into(),
            },
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: ReplayApiStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }
}
