//! Pure wire types for the official RePlayOS REST API.
//!
//! RePlayOS ≥ 1.7.4 (minimum supported) serves `http://<device>:55356/api/v1` from the frontend
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
    /// CRT-photo freeze state (`halted` in `get_status`, official since 1.7.4).
    /// Optional only because transient transition payloads omit it.
    #[serde(default)]
    pub halted: Option<bool>,
}

impl StatusResponse {
    /// Transient transition payload with the load-bearing fields missing —
    /// hold the previous state rather than interpreting it.
    pub fn is_degenerate(&self) -> bool {
        self.view_id.is_none() && self.halted.is_none()
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
        self.halted.unwrap_or(false)
    }
}

/// `get_version` payload, e.g. `{"version": "RePlayOS v1.7.4"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionResponse {
    pub version: String,
}

/// `get_playtime` payload: cumulative tracked play time, optionally filtered to
/// one `system` and/or `game_file`.
///
/// Documented in the RePlayOS REST API but **not yet implemented on 1.7.4** (it
/// 404s on-device), so callers must degrade gracefully. We read only the raw
/// `*_seconds` counts and format them ourselves; RePlayOS's preformatted
/// `all`/`time` strings are ignored (serde drops unknown fields).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaytimeResponse {
    /// Whether RePlayOS is tracking play time at all. When `false`, the counts
    /// are meaningless and the UI shows the unavailable placeholder.
    #[serde(default)]
    pub tracking_enabled: bool,
    /// Total tracked seconds across the whole library, or across the filtered
    /// subset when `system`/`game_file` narrow the query.
    #[serde(default)]
    pub all_seconds: u64,
    #[serde(default)]
    pub systems: Vec<PlaytimeSystem>,
    #[serde(default)]
    pub games: Vec<PlaytimeGame>,
}

/// Per-system entry in [`PlaytimeResponse::systems`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaytimeSystem {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub seconds: u64,
}

/// Per-game entry in [`PlaytimeResponse::games`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaytimeGame {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub seconds: u64,
}

/// Minimum RePlayOS version Replay Control supports as `(major, minor, patch)`.
///
/// 1.7.4 renamed the config endpoints (`get_replay_config` →
/// `get_config?type=…`, old names now 404). A 1.7.3 device exposes the API and
/// passes the bare `system_net_control` presence check, so it connects as
/// `Active` — but every config read/write then 404s. Gating on this floor
/// rejects such a device up front with a clear "update RePlayOS" verdict.
pub const MIN_SUPPORTED: (u32, u32, u32) = (1, 7, 4);

/// Parse a RePlayOS version string into a comparable `(major, minor, patch)`.
///
/// Tolerant by design: it scans for the first run of `\d+(\.\d+)*` anywhere in
/// the string, so `"RePlayOS v1.7.4"`, `"v1.7.10"`, `"1.8"`, and
/// `"RePlayOS v2.0.0-beta"` all parse. Missing minor/patch default to 0. A
/// string with no numeric version (`""`, `"RePlayOS"`, `"unknown"`) returns
/// `None`.
pub fn parse_replayos_version(version: &str) -> Option<(u32, u32, u32)> {
    // Find the first character that starts a numeric component.
    let start = version.find(|c: char| c.is_ascii_digit())?;
    let rest = &version[start..];
    // Take the leading dotted-number run (stop at the first non-digit/non-dot).
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(rest.len());
    let mut parts = rest[..end].split('.').filter(|p| !p.is_empty());

    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Whether a reported RePlayOS version meets the [`MIN_SUPPORTED`] floor.
///
/// Fail-OPEN on unparseable input: if the version string carries no recognizable
/// version number we treat the device as supported. RePlayOS could change its
/// version string format in a future release, and locking working users out on
/// a parse miss is worse than letting a config call surface its own error. Only
/// a *successfully parsed* version below the floor is reported as unsupported.
pub fn is_supported_replayos_version(version: &str) -> bool {
    match parse_replayos_version(version) {
        Some(parsed) => parsed >= MIN_SUPPORTED,
        None => true,
    }
}

/// [`MIN_SUPPORTED`] rendered as `"major.minor.patch"` for user-facing messages.
/// The single source of truth for the displayed minimum version — derive it from
/// the constant rather than hardcoding the number in copy.
pub fn min_supported_version_str() -> String {
    let (major, minor, patch) = MIN_SUPPORTED;
    format!("{major}.{minor}.{patch}")
}

/// A `width × height @ refresh_hz` mode from `get_info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub refresh_hz: f64,
}

/// `get_info` payload (RePlayOS ≥ 1.7.4): hardware, resources, the connected
/// display, and the running game's native resolution.
///
/// `game_resolution` is `null` at the menu (no core loaded). Fields are
/// `#[serde(default)]` so partial payloads still deserialize.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InfoResponse {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub eeprom: String,
    #[serde(default)]
    pub cpu_frequency_mhz: u32,
    #[serde(default)]
    pub gpu_frequency_mhz: u32,
    #[serde(default)]
    pub cpu_temperature_c: f64,
    #[serde(default)]
    pub available_ram_mb: u64,
    #[serde(default)]
    pub available_space_bytes: u64,
    #[serde(default)]
    pub display: String,
    #[serde(default)]
    pub display_resolution: Option<Resolution>,
    #[serde(default)]
    pub game_resolution: Option<Resolution>,
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

/// `get_config?type=…` payload: `{"modification_num": N, "config": {...}}`.
///
/// `modification_num` is a session-local change counter (resets on frontend
/// restart). The `wifi_*`/`nfs_*` families, passwords, and tokens are absent
/// from `config` entirely — those stay readable only from replay.cfg.
///
/// NOTE: the official docs (replayos.com/rest_api) describe this body as
/// `{"type","configured","config"}`, but the 1.7.4 device actually returns
/// `modification_num` + `config` — verified on the dev Pi 2026-06-14. The only
/// real 1.7.4 change was the endpoint rename (`get_replay_config` →
/// `get_config?type=…`, old names now 404), not the body. The documented shape
/// still deserializes (unknown fields ignored), so a future firmware that
/// matches the docs is tolerated too.
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

    /// The RePlayOS UI log verbosity (`system_log_level`), if present. Coerces
    /// both the string (`"1"`) and numeric (`1`) JSON encodings RePlayOS may
    /// use for the value.
    pub fn replay_log_level(&self) -> Option<ReplayLogLevel> {
        let raw = match self.config.get("system_log_level")? {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => return None,
        };
        Some(ReplayLogLevel::from_system_value(&raw))
    }
}

/// RePlayOS UI log verbosity, from the `system_log_level` key in the `replay`
/// config. Numeric on the wire and **inverted** — a lower number is more
/// verbose. `Debug` (`"0"`) is not user-selectable in the RePlayOS menu, and
/// the API rejects writes to this key, so it can only be changed on the TV
/// (SYSTEM > LOG LEVEL) and read back here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayLogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Disabled,
    /// Present, but not one of the documented `0`–`4` values.
    Unknown,
}

impl ReplayLogLevel {
    /// Map the raw `system_log_level` value (`"0"`–`"4"`) to a level.
    pub fn from_system_value(value: &str) -> Self {
        match value.trim() {
            "0" => Self::Debug,
            "1" => Self::Info,
            "2" => Self::Warn,
            "3" => Self::Error,
            "4" => Self::Disabled,
            _ => Self::Unknown,
        }
    }
}

/// Config domain selected by the `type=` query parameter of
/// `get_config` / `set_config` (RePlayOS ≥ 1.7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigKind {
    Replay,
    Core,
    Game,
}

impl ConfigKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigKind::Replay => "replay",
            ConfigKind::Core => "core",
            ConfigKind::Game => "game",
        }
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
/// `Halt` is the CRT-photo freeze-frame; its state is reported by
/// `get_status.halted`.
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
    /// RePlayOS can't be remote-controlled. Two causes:
    /// - No Net Control API at all (RePlayOS older than 1.7.x or the feature
    ///   off): detected pre-onboarding by the absence of the
    ///   `system_net_control` key in replay.cfg (`version: None`).
    /// - API present but the firmware is below the [`MIN_SUPPORTED`] floor
    ///   (e.g. 1.7.3, whose config endpoints 404): detected after a successful
    ///   `get_version` whose value parses below 1.7.4 (`version: Some(...)`).
    ///
    /// Either way the user must update RePlayOS on the TV.
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
    fn replay_log_level_reads_string_and_number_encodings() {
        // Device returns it as a JSON string ("1"); tolerate a numeric form too.
        let from_str: ReplayConfigSnapshot =
            serde_json::from_str(r#"{"config":{"system_log_level":"3"}}"#).unwrap();
        assert_eq!(from_str.replay_log_level(), Some(ReplayLogLevel::Error));

        let from_num: ReplayConfigSnapshot =
            serde_json::from_str(r#"{"config":{"system_log_level":1}}"#).unwrap();
        assert_eq!(from_num.replay_log_level(), Some(ReplayLogLevel::Info));

        // Key absent → None (can't show a level).
        let absent: ReplayConfigSnapshot = serde_json::from_str(r#"{"config":{}}"#).unwrap();
        assert_eq!(absent.replay_log_level(), None);
    }

    #[test]
    fn replay_log_level_maps_all_documented_values() {
        use ReplayLogLevel::*;
        assert_eq!(ReplayLogLevel::from_system_value("0"), Debug);
        assert_eq!(ReplayLogLevel::from_system_value("1"), Info);
        assert_eq!(ReplayLogLevel::from_system_value("2"), Warn);
        assert_eq!(ReplayLogLevel::from_system_value("3"), Error);
        assert_eq!(ReplayLogLevel::from_system_value("4"), Disabled);
        assert_eq!(ReplayLogLevel::from_system_value("9"), Unknown);
    }

    #[test]
    fn status_halted_field() {
        let status: StatusResponse =
            serde_json::from_str(r#"{"view_id":2,"halted":true}"#).unwrap();
        assert!(!status.is_degenerate());
        assert!(status.is_halted());

        let status: StatusResponse =
            serde_json::from_str(r#"{"view_id":2,"halted":false}"#).unwrap();
        assert!(!status.is_halted());
    }

    #[test]
    fn status_parses_real_1_7_4_menu_payload() {
        // Captured from the dev Pi on RePlayOS 1.7.4 (menu, no game loaded):
        // `halted` is now a real field and maps onto `halt` via the alias.
        let status: StatusResponse = serde_json::from_str(
            r#"{"system":"replay_menu","game_file":"","game_name":"","paused":false,"halted":false,"view":"system_options","view_id":1,"core_file":"replay_libretro.so","core_info":"RePlay Menu 2.3"}"#,
        )
        .unwrap();
        assert!(!status.game_loaded());
        assert!(!status.is_halted());
        assert_eq!(status.view_kind(), Some(View::SystemOptions));
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
        // Real 1.7.4 `get_config?type=replay` body (verified on the dev Pi):
        // {modification_num, config} — NOT the {type,configured,config} the docs claim.
        let snapshot: ReplayConfigSnapshot = serde_json::from_str(
            r#"{"modification_num":0,"config":{"system_storage":"nfs","rcheevos_enabled":"true"}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.modification_num, 0);
        assert_eq!(snapshot.get_str("system_storage"), Some("nfs"));
        assert_eq!(snapshot.get_str("missing"), None);
    }

    #[test]
    fn config_snapshot_tolerates_documented_shape() {
        // Defensive: if a future firmware matches the docs ({type, configured, config}),
        // it still deserializes (unknown fields ignored, modification_num defaults to 0).
        let snapshot: ReplayConfigSnapshot = serde_json::from_str(
            r#"{"type":"replay","configured":true,"config":{"system_storage":"nfs"}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.get_str("system_storage"), Some("nfs"));
    }

    #[test]
    fn info_parses_real_1_7_4_payload() {
        // Captured from the dev Pi on RePlayOS 1.7.4 (game loaded).
        let info: InfoResponse = serde_json::from_str(
            r#"{"version":"RePlayOS v1.7.4","model":"Raspberry Pi 5","eeprom":"2025-11-05","cpu_frequency_mhz":2600,"gpu_frequency_mhz":1060,"cpu_temperature_c":59.0,"available_ram_mb":1980,"available_space_bytes":2918821920768,"display":"MORTACA DEV00, ATG","display_resolution":{"width":2560,"height":240,"refresh_hz":60.00},"game_resolution":{"width":320,"height":240,"refresh_hz":60.00}}"#,
        )
        .unwrap();
        assert_eq!(info.model, "Raspberry Pi 5");
        assert_eq!(info.cpu_temperature_c, 59.0);
        assert_eq!(info.available_ram_mb, 1980);
        assert_eq!(
            info.display_resolution,
            Some(Resolution {
                width: 2560,
                height: 240,
                refresh_hz: 60.0
            })
        );
        assert_eq!(info.game_resolution.map(|r| r.width), Some(320));
    }

    #[test]
    fn info_game_resolution_null_at_menu() {
        let info: InfoResponse =
            serde_json::from_str(r#"{"version":"RePlayOS v1.7.4","game_resolution":null}"#)
                .unwrap();
        assert_eq!(info.game_resolution, None);
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
    fn version_parses_standard_and_padded_forms() {
        assert_eq!(parse_replayos_version("RePlayOS v1.7.4"), Some((1, 7, 4)));
        assert_eq!(parse_replayos_version("v1.7.10"), Some((1, 7, 10)));
        assert_eq!(parse_replayos_version("1.8.0"), Some((1, 8, 0)));
        assert_eq!(parse_replayos_version("2.0.0"), Some((2, 0, 0)));
        // Missing components default to 0.
        assert_eq!(parse_replayos_version("RePlayOS v1.8"), Some((1, 8, 0)));
        assert_eq!(parse_replayos_version("v2"), Some((2, 0, 0)));
        // Trailing suffixes are ignored.
        assert_eq!(
            parse_replayos_version("RePlayOS v2.0.0-beta"),
            Some((2, 0, 0))
        );
        assert_eq!(parse_replayos_version("1.7.4 (dev)"), Some((1, 7, 4)));
    }

    #[test]
    fn version_parse_rejects_unparseable() {
        assert_eq!(parse_replayos_version(""), None);
        assert_eq!(parse_replayos_version("RePlayOS"), None);
        assert_eq!(parse_replayos_version("unknown"), None);
        assert_eq!(parse_replayos_version("v.x.y"), None);
    }

    #[test]
    fn is_supported_at_exact_minimum() {
        assert!(is_supported_replayos_version("RePlayOS v1.7.4"));
    }

    #[test]
    fn is_supported_above_minimum() {
        assert!(is_supported_replayos_version("RePlayOS v1.7.10"));
        assert!(is_supported_replayos_version("RePlayOS v1.8.0"));
        assert!(is_supported_replayos_version("RePlayOS v2.0.0"));
    }

    #[test]
    fn is_unsupported_below_minimum() {
        assert!(!is_supported_replayos_version("RePlayOS v1.7.3"));
        assert!(!is_supported_replayos_version("RePlayOS v1.6.9"));
        assert!(!is_supported_replayos_version("RePlayOS v1.0.0"));
        assert!(!is_supported_replayos_version("v0.9"));
    }

    #[test]
    fn unparseable_version_fails_open() {
        // A version string we can't parse is treated as supported so a future
        // format change doesn't lock users out.
        assert!(is_supported_replayos_version("RePlayOS"));
        assert!(is_supported_replayos_version(""));
        assert!(is_supported_replayos_version("unknown"));
    }

    #[test]
    fn replay_api_status_serde_round_trip() {
        for status in [
            ReplayApiStatus::NotConfigured,
            ReplayApiStatus::PendingRestart,
            ReplayApiStatus::Active {
                version: "RePlayOS v1.7.4".into(),
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
