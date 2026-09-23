//! Server functions for the RePlayOS Net Control integration.
//!
//! Onboarding has exactly two paths (see the integration plan):
//! - **Assisted** ([`enable_replay_api_assisted`]): flips `system_net_control`
//!   in replay.cfg through the stop→write→start dance (the one sanctioned
//!   frontend restart left in the app), then reads the token RePlayOS
//!   generated — the single replay.cfg read in the token lifecycle.
//! - **Manual** ([`verify_replay_api_token`]): the user types the Net Control
//!   code shown on the TV; we verify it against authenticated status before
//!   storing.
//!
//! In both cases the token ends up in the app's own settings.cfg and the
//! in-memory client; replay.cfg is never re-read for it afterwards.

use super::*;

#[cfg(feature = "ssr")]
use crate::api::{AppState, replay_api::ReplayApi};
#[cfg(feature = "ssr")]
use crate::util::is_valid_net_control_code;
#[cfg(feature = "ssr")]
use replay_control_core_server::replay_api::{ApiError, ReplayApiClient};
#[cfg(feature = "ssr")]
use replay_control_core_server::settings::{
    read_replayos_message_duration_secs, write_replay_api_token,
    write_replayos_message_duration_secs,
};
#[cfg(feature = "ssr")]
use std::sync::Arc;

#[cfg(feature = "ssr")]
use crate::i18n::{Key, Locale, feature_restriction_message, t};
use replay_control_core::replay_api::DeviceCapabilities;
#[cfg(feature = "ssr")]
use replay_control_core::replay_api::ReplayFeature;
#[cfg(feature = "ssr")]
use replay_control_core::replay_api::{
    ConfigKind, SetCommand, is_supported_replayos_version, min_supported_version_str,
};
use replay_control_core::replay_api::{ReplayApiStatus, ReplayLogLevel};

#[server(prefix = "/sfn")]
pub async fn get_replay_capabilities() -> Result<DeviceCapabilities, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.as_ref() else {
        return Ok(DeviceCapabilities::default());
    };
    read_replay_capabilities(api).await
}

#[cfg(feature = "ssr")]
async fn read_replay_capabilities(api: &ReplayApi) -> Result<DeviceCapabilities, ServerFnError> {
    api.client()
        .get_info()
        .await
        .map(DeviceCapabilities::from)
        .map_err(|error| {
            api.report_error(&error);
            ServerFnError::new(error.to_string())
        })
}

#[cfg(feature = "ssr")]
pub(crate) async fn ensure_game_launch_allowed(
    api: &ReplayApi,
    system: &str,
    locale: Locale,
) -> Result<(), ServerFnError> {
    let Some(feature) = ReplayFeature::for_system(system) else {
        return Ok(());
    };
    ensure_feature_available(api, feature, locale).await
}

#[cfg(feature = "ssr")]
async fn ensure_feature_available(
    api: &ReplayApi,
    feature: ReplayFeature,
    locale: Locale,
) -> Result<(), ServerFnError> {
    let capabilities = read_replay_capabilities(api)
        .await
        .map_err(|_| ServerFnError::new(t(locale, Key::ReplayCapabilitiesUnavailable)))?;
    if let Some(restriction) = capabilities.restriction(feature) {
        return Err(ServerFnError::new(feature_restriction_message(
            locale,
            restriction,
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReplayPlayerCommand {
    Screenshot,
    Halt,
    Mute,
    VolumeDown,
    VolumeUp,
    GameReset,
    SaveState { slot: u8 },
    LoadState { slot: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayOsSettings {
    pub kiosk_mode: bool,
    pub message_duration_secs: u8,
}

#[cfg(feature = "ssr")]
use replay_control_core::replay_settings::ActiveGameIdentity;
pub use replay_control_core::replay_settings::{
    ActiveGameConfig, GameSettingsOutcome, LinkPlayConfig, SettingsOutcome,
};

#[server(prefix = "/sfn")]
pub async fn get_active_game_config() -> Result<ActiveGameConfig, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "read game settings")?;
    let client = api.client();
    let result = async {
        let before = ActiveGameIdentity::from_status(&client.get_status().await?);
        let game = client.get_config(ConfigKind::Game).await?;
        let core = client.get_config(ConfigKind::Core).await?;
        let global = client.get_config(ConfigKind::Replay).await?;
        let after = ActiveGameIdentity::from_status(&client.get_status().await?);
        Ok::<_, ApiError>((before, after, game, core, global))
    }
    .await
    .map_err(|error| {
        api.report_error(&error);
        ServerFnError::new(error.to_string())
    })?;
    let (before, after, game, core, global) = result;
    let identity = before
        .filter(|identity| Some(identity) == after.as_ref())
        .ok_or_else(|| ServerFnError::new("Active game changed while reading settings"))?;
    Ok(ActiveGameConfig::from_snapshots(
        identity, &game, &core, &global,
    ))
}

#[server(prefix = "/sfn")]
pub async fn save_active_game_config(
    config: ActiveGameConfig,
) -> Result<GameSettingsOutcome, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "change game settings")?;
    apply_active_game_config(&api, config).await
}

#[cfg(feature = "ssr")]
async fn apply_active_game_config(
    api: &ReplayApi,
    config: ActiveGameConfig,
) -> Result<GameSettingsOutcome, ServerFnError> {
    if !config.is_valid() {
        return Ok(GameSettingsOutcome::InvalidValue);
    }
    let encoded: Vec<_> = config
        .overrides
        .iter()
        .map(|entry| {
            (
                entry.setting.key(),
                entry.value.map_or_else(
                    || "default".to_string(),
                    |value| entry.setting.encode(value),
                ),
            )
        })
        .collect();
    let changes: Vec<_> = encoded
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let status = api.client().get_status().await.map_err(|error| {
        api.report_error(&error);
        ServerFnError::new(error.to_string())
    })?;
    if ActiveGameIdentity::from_status(&status).as_ref() != Some(&config.identity) {
        return Ok(GameSettingsOutcome::GameChanged);
    }
    // Best-effort identity guard only: RePlayOS has no atomic conditional write.
    // A TV-side game switch between this check and set_config remains possible.
    api.client()
        .set_config(ConfigKind::Game, &changes)
        .await
        .map_err(|error| {
            api.report_error(&error);
            ServerFnError::new(error.to_string())
        })?;
    Ok(GameSettingsOutcome::Applied)
}

#[server(prefix = "/sfn")]
pub async fn get_link_play_config() -> Result<LinkPlayConfig, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "read Link Play settings")?;
    let locale = state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .locale
        .unwrap_or_default();
    ensure_feature_available(&api, ReplayFeature::LinkPlay, locale).await?;
    let guard = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned");
    let config = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("RePlayOS configuration unavailable"))?;
    Ok(LinkPlayConfig {
        mode: config.link_play_mode().to_string(),
        address: config.link_play_address().to_string(),
        port: config.link_play_port(),
        lan_discovery: config.link_play_lan_discovery_enabled(),
    })
}

#[server(prefix = "/sfn")]
pub async fn save_link_play_config(
    config: LinkPlayConfig,
) -> Result<SettingsOutcome, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "change Link Play settings")?;
    let locale = state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .locale
        .unwrap_or_default();
    ensure_feature_available(&api, ReplayFeature::LinkPlay, locale).await?;
    if let Some(error) = config.validation_error() {
        return Ok(error);
    }
    let mode = config.mode.trim();
    let address = config.address.trim();
    let port = config.port.to_string();
    let discovery = if config.lan_discovery {
        "true"
    } else {
        "false"
    };
    if let Err(error) = api
        .client()
        .set_config(
            ConfigKind::Replay,
            &[
                ("link_play_mode", mode),
                ("link_play_address", address),
                ("link_play_port", &port),
                ("link_play_lan_discovery", discovery),
            ],
        )
        .await
    {
        api.report_error(&error);
        return Err(ServerFnError::new(error.to_string()));
    }
    let _ = state.reload_replay_config();
    Ok(SettingsOutcome::Applied)
}

/// Whether RePlayOS could report play time. Disabled and Unavailable both
/// render the same i18n placeholder in the UI; they are distinct so a future
/// surface can tell "tracking is off on the TV" from "this firmware/build has
/// no play-time data".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaytimeAvailability {
    /// Not on a device, Net Control not connected, the endpoint 404s
    /// (unimplemented on current firmware), or the call errored.
    #[default]
    Unavailable,
    /// RePlayOS answered but `tracking_enabled` is false.
    Disabled,
    /// Real tracked totals are present.
    Tracked,
}

/// Play time for one system, in seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPlaytime {
    pub system: String,
    pub seconds: u64,
}

/// Library-wide play time for the settings library page: a grand total plus
/// per-system seconds. Empty/`Unavailable` on standalone builds and on firmware
/// that doesn't implement `get_playtime`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlaytimeSummary {
    pub availability: PlaytimeAvailability,
    pub all_seconds: u64,
    pub session_seconds: Option<u64>,
    pub week_seconds: Option<u64>,
    pub games_played: Option<u64>,
    pub total_sessions: Option<u64>,
    pub most_played_game: Option<String>,
    pub most_played_system: Option<String>,
    pub average_session_seconds: Option<u64>,
    pub longest_session_seconds: Option<u64>,
    pub systems: Vec<SystemPlaytime>,
}

/// Play time for a single game, for the game detail page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GamePlaytime {
    pub availability: PlaytimeAvailability,
    pub seconds: u64,
}

impl ReplayPlayerCommand {
    #[cfg(feature = "ssr")]
    fn set_command(self) -> Option<SetCommand> {
        match self {
            ReplayPlayerCommand::Screenshot => Some(SetCommand::Screenshot),
            ReplayPlayerCommand::Halt => Some(SetCommand::Halt),
            ReplayPlayerCommand::Mute => Some(SetCommand::Mute),
            ReplayPlayerCommand::VolumeDown => Some(SetCommand::VolumeDown),
            ReplayPlayerCommand::VolumeUp => Some(SetCommand::VolumeUp),
            ReplayPlayerCommand::GameReset => Some(SetCommand::GameReset),
            ReplayPlayerCommand::SaveState { .. } | ReplayPlayerCommand::LoadState { .. } => None,
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn get_replayos_settings() -> Result<ReplayOsSettings, ServerFnError> {
    let state = super::app_state()?;
    let kiosk_mode = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned")
        .as_ref()
        .is_some_and(|config| config.system_kiosk_mode_enabled());

    Ok(ReplayOsSettings {
        kiosk_mode,
        message_duration_secs: read_replayos_message_duration_secs(&state.settings),
    })
}

/// The RePlayOS UI log level (`system_log_level`), read live via the API.
/// Returns `None` whenever it can't be read — standalone build, Net Control not
/// connected, the key is absent, or the call errored — so the page shows
/// "Unavailable" rather than a wrong value. Read-only: the API rejects writes
/// to this key (it's not in `allowed_config_variables`), so it can only be
/// changed on the TV via SYSTEM > LOG LEVEL.
#[server(prefix = "/sfn")]
pub async fn get_replayos_log_level() -> Result<Option<ReplayLogLevel>, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Ok(None);
    };
    if !api.status().is_active() {
        return Ok(None);
    }
    match api.client().get_config(ConfigKind::Replay).await {
        Ok(snapshot) => Ok(snapshot.replay_log_level()),
        Err(error) => {
            api.report_error(&error);
            Ok(None)
        }
    }
}

/// Current integration status. Standalone mode has no integration and reports
/// the default (`NotConfigured`); UI surfaces gate on `get_mode` anyway.
#[server(prefix = "/sfn")]
pub async fn get_replay_api_status() -> Result<ReplayApiStatus, ServerFnError> {
    let state = super::app_state()?;
    Ok(state
        .replay_api
        .as_ref()
        .map(|api| api.status())
        .unwrap_or_default())
}

/// Library-wide play time totals for the settings library page. Fetched
/// client-side (never during SSR) so a slow or missing endpoint never blocks
/// the page. Any "not available" condition — standalone build, Net Control not
/// connected, the documented-but-unimplemented endpoint 404ing, or a call
/// error — collapses to `Unavailable`; `tracking_enabled=false` is `Disabled`.
/// Both render the placeholder.
#[server(prefix = "/sfn")]
pub async fn get_library_playtime() -> Result<PlaytimeSummary, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Ok(PlaytimeSummary::default());
    };
    if !api.status().is_active() {
        return Ok(PlaytimeSummary::default());
    }

    match api.client().get_playtime(None, None).await {
        Ok(response) if response.tracking_enabled => Ok(PlaytimeSummary {
            availability: PlaytimeAvailability::Tracked,
            all_seconds: response.all_seconds,
            session_seconds: response.session_seconds,
            week_seconds: response.week_seconds,
            games_played: response.games_played,
            total_sessions: response.total_sessions,
            most_played_game: response.most_played_game,
            most_played_system: response.most_played_system,
            average_session_seconds: response.average_session_seconds,
            longest_session_seconds: response.longest_session_seconds,
            systems: response
                .systems
                .into_iter()
                .map(|s| SystemPlaytime {
                    system: s.system,
                    seconds: s.seconds,
                })
                .collect(),
        }),
        Ok(_) => Ok(PlaytimeSummary {
            availability: PlaytimeAvailability::Disabled,
            ..Default::default()
        }),
        Err(error) => {
            // 404 (unimplemented) is a no-op for the status machine; only a
            // genuinely unreachable frontend flips it to Error.
            api.report_error(&error);
            Ok(PlaytimeSummary::default())
        }
    }
}

/// Total play time for one game, for the game detail page. Same graceful-
/// degradation contract as [`get_library_playtime`]. The `game_file` identity
/// mirrors the launch path (system folder + ROM filename); since the endpoint
/// is unimplemented on current firmware, the exact match key is a best-effort
/// assumption that falls back to the filtered total.
#[server(prefix = "/sfn")]
pub async fn get_game_playtime(
    system: String,
    game_file: String,
) -> Result<GamePlaytime, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Ok(GamePlaytime::default());
    };
    if !api.status().is_active() {
        return Ok(GamePlaytime::default());
    }

    match api
        .client()
        .get_playtime(Some(&system), Some(&game_file))
        .await
    {
        Ok(response) if response.tracking_enabled => {
            // Only report a time when THIS game has its own entry. The previous
            // fallback to `response.all_seconds` showed the system/library-wide
            // total as if it were this one game's playtime — and the match key is
            // a best-effort assumption (see fn docs), so a miss must read as
            // "unavailable", not as a misleading aggregate.
            match response
                .games
                .iter()
                .find(|g| g.game == game_file && g.system == system)
            {
                Some(g) => Ok(GamePlaytime {
                    availability: PlaytimeAvailability::Tracked,
                    seconds: g.seconds,
                }),
                None => Ok(GamePlaytime::default()),
            }
        }
        Ok(_) => Ok(GamePlaytime {
            availability: PlaytimeAvailability::Disabled,
            seconds: 0,
        }),
        Err(error) => {
            api.report_error(&error);
            Ok(GamePlaytime::default())
        }
    }
}

/// Re-probe the API and return the resulting status. Backs the "Check again" /
/// "Retry" actions on the Net Control settings page.
#[server(prefix = "/sfn")]
pub async fn reprobe_replay_api() -> Result<ReplayApiStatus, ServerFnError> {
    let state = super::app_state()?;
    match state.replay_api.as_ref() {
        Some(api) => Ok(api.probe().await),
        None => Ok(ReplayApiStatus::default()),
    }
}

/// Safe commands exposed by the now-playing player bar. The browser cannot
/// request arbitrary `set_cmd` values from here: no stop, reboot, or power-off
/// surface is wired through this endpoint.
#[server(prefix = "/sfn")]
pub async fn send_replay_player_command(command: ReplayPlayerCommand) -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(
            "RePlayOS Net Control is only available on the device",
        ));
    };
    if !api.status().is_active() {
        return Err(ServerFnError::new("RePlayOS Net Control is not connected"));
    }

    let result = match command {
        ReplayPlayerCommand::SaveState { slot } => {
            validate_save_state_slot(slot)?;
            api.client().save_state(slot).await
        }
        ReplayPlayerCommand::LoadState { slot } => {
            validate_save_state_slot(slot)?;
            api.client().load_state(slot).await
        }
        _ => {
            // Every non-save/load command maps to a `set_cmd` payload. Return
            // an error rather than panicking the process if a future variant
            // is added without a mapping.
            let Some(cmd) = command.set_command() else {
                return Err(ServerFnError::new("Unsupported player command"));
            };
            api.client().set_cmd(cmd).await
        }
    };

    if let Err(e) = result {
        api.report_error(&e);
        return Err(ServerFnError::new(e.to_string()));
    }

    Ok(())
}

#[cfg(feature = "ssr")]
fn validate_save_state_slot(slot: u8) -> Result<(), ServerFnError> {
    if (1..=18).contains(&slot) {
        Ok(())
    } else {
        Err(ServerFnError::new(
            "Save state slot must be between 1 and 18",
        ))
    }
}

#[server(prefix = "/sfn")]
pub async fn send_replayos_message(
    text: String,
    duration_secs: u8,
) -> Result<String, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "send an on-screen message")?;
    let text = text.trim();
    if text.is_empty() {
        return Err(ServerFnError::new("Message cannot be empty"));
    }
    if text.chars().count() > 120 {
        return Err(ServerFnError::new(
            "Message must be 120 characters or fewer",
        ));
    }
    let duration_secs = duration_secs.clamp(1, 10);

    if let Err(error) = write_replayos_message_duration_secs(&state.settings, duration_secs) {
        tracing::warn!("Failed to save RePlayOS message duration: {error}");
    }

    if let Err(error) = api.client().set_msg(text, duration_secs).await {
        api.report_error(&error);
        return Err(ServerFnError::new(error.to_string()));
    }

    Ok("Message sent".to_string())
}

#[server(prefix = "/sfn")]
pub async fn restart_replayos_game() -> Result<String, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "restart the current game")?;

    if let Err(error) = api.client().set_cmd(SetCommand::GameRestart).await {
        api.report_error(&error);
        return Err(ServerFnError::new(error.to_string()));
    }

    Ok("Restarting game...".to_string())
}

#[server(prefix = "/sfn")]
pub async fn power_off_replayos_device() -> Result<String, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "power off the device")?;

    if let Err(error) = api.client().set_cmd(SetCommand::PowerOff).await {
        api.report_error(&error);
        return Err(ServerFnError::new(error.to_string()));
    }

    Ok("Powering off...".to_string())
}

#[server(prefix = "/sfn")]
pub async fn save_replayos_kiosk_mode(enabled: bool) -> Result<String, ServerFnError> {
    let state = super::app_state()?;
    let api = require_active_replay_api(&state, "change kiosk mode")?;
    let value = if enabled { "true" } else { "false" };

    if let Err(error) = api
        .client()
        .set_config(ConfigKind::Replay, &[("system_kiosk_mode", value)])
        .await
    {
        api.report_error(&error);
        return Err(ServerFnError::new(error.to_string()));
    }
    let _ = state.reload_replay_config();

    Ok("Kiosk mode saved".to_string())
}

/// Manual onboarding: verify a user-entered Net Control code and store it on
/// success. The candidate code is probed with a throwaway client so a typo
/// never clobbers a working stored token.
#[server(prefix = "/sfn")]
pub async fn verify_replay_api_token(code: String) -> Result<ReplayApiStatus, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(
            "RePlayOS Net Control is only available on the device",
        ));
    };

    let code = code.trim().to_string();
    if !is_valid_net_control_code(&code) {
        return Err(ServerFnError::new(
            "Enter the 6-digit Net Control code shown on the TV",
        ));
    }

    let candidate = ReplayApiClient::local(Some(code.clone()));
    match candidate.get_status().await {
        Ok(_) => {}
        Err(ApiError::Unauthorized) => {
            return Err(ServerFnError::new(
                "RePlayOS rejected the code — check SYSTEM > INFORMATION and try again",
            ));
        }
        Err(ApiError::RateLimited { .. }) => {
            return Err(ServerFnError::new(
                "Too many invalid codes were entered. Wait a moment, then try again.",
            ));
        }
        Err(ApiError::Unreachable { .. }) => {
            return Err(ServerFnError::new(
                "RePlayOS Net Control is not reachable — enable it on the TV and try again",
            ));
        }
        Err(error) => return Err(ServerFnError::new(error.to_string())),
    }
    match candidate.get_version().await {
        // Reject an old device (parsed version below 1.8.0) before storing the
        // token, consistent with the probe. Unparseable versions fail open. The
        // status flips to `Unsupported` so the UI matches the probe's verdict.
        Ok(version) if !is_supported_replayos_version(&version.version) => {
            api.set_status(ReplayApiStatus::Unsupported {
                version: Some(version.version),
            });
            Err(ServerFnError::new(format!(
                "This RePlayOS version is too old for remote control — update RePlayOS on the TV ({} or newer is required)",
                min_supported_version_str(),
            )))
        }
        Ok(version) => {
            write_replay_api_token(&state.settings, &code)
                .map_err(|e| ServerFnError::new(format!("Failed to store the code: {e}")))?;
            api.swap_local_token(Some(code));
            api.set_status(ReplayApiStatus::Active {
                version: version.version,
            });
            Ok(api.status())
        }
        Err(ApiError::Unauthorized) => Err(ServerFnError::new("RePlayOS rejected the code")),
        Err(ApiError::RateLimited { .. }) => Err(ServerFnError::new(
            "Too many invalid codes were entered. Wait a moment, then try again.",
        )),
        Err(ApiError::Unreachable { .. }) => {
            if replay_cfg_has_net_control_key(&state) {
                Err(ServerFnError::new(
                    "Could not reach RePlayOS — is Net Control enabled on the TV (SYSTEM > OPTIONS)?",
                ))
            } else {
                Err(unsupported_replayos_error(&api))
            }
        }
        Err(other) => Err(ServerFnError::new(other.to_string())),
    }
}

/// Assisted onboarding. Flips `system_net_control` in replay.cfg (restarting
/// the RePlayOS frontend — the action copy in the UI warns about this), waits
/// for RePlayOS to write its `replay_http_token`, then stores that token
/// app-side and probes to `Active`.
///
/// If Net Control is already enabled, no restart happens: the token is read
/// and stored directly.
#[server(prefix = "/sfn")]
pub async fn enable_replay_api_assisted() -> Result<ReplayApiStatus, ServerFnError> {
    let state = super::app_state()?;
    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(
            "RePlayOS Net Control setup is only available on the device",
        ));
    };

    // Already onboarded and healthy? Done.
    if api.status().is_active() {
        return Ok(api.status());
    }

    // Version pre-check: the `system_net_control` key is absent on RePlayOS
    // < 1.7.3 — fail before restarting the user's TV for nothing.
    if !replay_cfg_has_net_control_key(&state) {
        return Err(unsupported_replayos_error(&api));
    }

    let already_enabled = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned")
        .as_ref()
        .is_some_and(|config| config.system_net_control_enabled());

    if !already_enabled {
        // The one sanctioned frontend restart: enable the flag through the
        // stop→write→start dance. The restart window keeps probes from
        // flapping the status to `Error` while the API is expectedly down.
        api.set_status(ReplayApiStatus::PendingRestart);
        let restart_window = api.begin_restart_window();
        let result = super::settings::apply_replay_config_change(state.mode.is_device(), {
            let state = state.clone();
            move || state.enable_replayos_net_control()
        })
        .await;
        if let Err(e) = result {
            drop(restart_window);
            api.probe().await;
            return Err(e);
        }

        let token = wait_for_replay_cfg_token(&state).await;
        drop(restart_window);

        let Some(token) = token else {
            api.probe().await;
            return Err(ServerFnError::new(
                "Net Control was enabled, but RePlayOS did not write a token yet",
            ));
        };
        adopt_token(&state, &api, token)?;
    } else {
        // Net Control already on (e.g. enabled from the TV): no restart —
        // just adopt the token RePlayOS already generated.
        let token = current_replay_cfg_token(&state).ok_or_else(|| {
            ServerFnError::new("Net Control is enabled, but replay.cfg has no token")
        })?;
        adopt_token(&state, &api, token)?;
    }

    // The beta frontend can expose `replay_http_token` before its HTTP server
    // is ready to answer authenticated requests. A single one-second probe
    // made successful setup look like a failure during that restart window.
    let status = wait_for_replay_api(&api).await;
    if status.is_active() {
        Ok(status)
    } else {
        Err(ServerFnError::new(format!(
            "Net Control setup did not finish connecting: {}",
            replay_api_status_detail(&status)
        )))
    }
}

/// Detection-ladder verdict for a RePlayOS that predates the API (the
/// `system_net_control` key is the version marker): flip the status to
/// `Unsupported` and tell the user to update — the one place this rule and
/// its message live.
#[cfg(feature = "ssr")]
fn unsupported_replayos_error(api: &ReplayApi) -> ServerFnError {
    api.set_status(ReplayApiStatus::Unsupported { version: None });
    ServerFnError::new(
        "This RePlayOS version doesn't support remote control — update RePlayOS on the TV",
    )
}

#[cfg(feature = "ssr")]
fn replay_cfg_has_net_control_key(state: &AppState) -> bool {
    state
        .replay_config
        .read()
        .expect("replay_config lock poisoned")
        .as_ref()
        .is_some_and(|config| config.has_net_control_key())
}

#[cfg(feature = "ssr")]
fn current_replay_cfg_token(state: &crate::api::AppState) -> Option<String> {
    state
        .replay_config
        .read()
        .expect("replay_config lock poisoned")
        .as_ref()
        .and_then(|config| config.replay_http_token())
        .map(|token| token.to_string())
}

/// Poll the freshly restarted frontend's replay.cfg for the generated token.
/// This is the single replay.cfg read in the token lifecycle.
#[cfg(feature = "ssr")]
async fn wait_for_replay_cfg_token(state: &crate::api::AppState) -> Option<String> {
    const TOKEN_RETRIES: usize = 20;
    const TOKEN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

    for _ in 0..TOKEN_RETRIES {
        if state.reload_replay_config()
            && let Some(token) = current_replay_cfg_token(state)
        {
            return Some(token);
        }
        tokio::time::sleep(TOKEN_RETRY_DELAY).await;
    }
    None
}

/// Wait for the restarted frontend to accept the adopted token. Keep this
/// separate from token generation: replay.cfg can be ready several seconds
/// before the Net Control listener and active core have settled.
#[cfg(feature = "ssr")]
async fn wait_for_replay_api(api: &ReplayApi) -> ReplayApiStatus {
    const API_RETRIES: usize = 30;
    const API_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

    for attempt in 0..API_RETRIES {
        let result = api.probe_checked().await;
        if matches!(result, Err(ApiError::RateLimited { .. })) {
            return api.status();
        }
        let status = api.status();
        if status.is_active()
            || matches!(
                status,
                ReplayApiStatus::Unauthorized | ReplayApiStatus::Unsupported { .. }
            )
            || attempt + 1 == API_RETRIES
        {
            return status;
        }
        tokio::time::sleep(API_RETRY_DELAY).await;
    }

    unreachable!("the final retry always returns")
}

#[cfg(feature = "ssr")]
fn adopt_token(
    state: &crate::api::AppState,
    api: &crate::api::replay_api::ReplayApi,
    token: String,
) -> Result<(), ServerFnError> {
    let token = normalize_adopted_replay_api_token(token)?;
    write_replay_api_token(&state.settings, &token)
        .map_err(|e| ServerFnError::new(format!("Failed to store the code: {e}")))?;
    api.swap_local_token(Some(token));
    Ok(())
}

#[cfg(feature = "ssr")]
fn normalize_adopted_replay_api_token(token: String) -> Result<String, ServerFnError> {
    let token = token.trim().to_string();
    if is_valid_net_control_code(&token) {
        Ok(token)
    } else {
        Err(ServerFnError::new(
            "RePlayOS wrote an invalid Net Control code; regenerate the code from RePlayOS and try again",
        ))
    }
}

#[cfg(feature = "ssr")]
fn require_active_replay_api(
    state: &AppState,
    action: &str,
) -> Result<Arc<ReplayApi>, ServerFnError> {
    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(format!(
            "RePlayOS Net Control is required to {action}"
        )));
    };
    if !api.status().is_active() {
        return Err(ServerFnError::new(format!(
            "RePlayOS Net Control is not connected; connect it before you {action}"
        )));
    }
    Ok(api)
}

#[cfg(feature = "ssr")]
fn replay_api_status_detail(status: &ReplayApiStatus) -> String {
    match status {
        ReplayApiStatus::NotConfigured => "no Net Control code is stored".to_string(),
        ReplayApiStatus::PendingRestart => "RePlayOS is still restarting".to_string(),
        ReplayApiStatus::Active { version } => format!("connected to {version}"),
        ReplayApiStatus::Unauthorized => "RePlayOS rejected the Net Control code".to_string(),
        ReplayApiStatus::Unsupported { .. } => {
            "this RePlayOS version does not support remote control".to_string()
        }
        ReplayApiStatus::Error { reason } => reason.clone(),
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use replay_control_core::replay_settings::{GameOverride, GameSetting};
    use replay_control_core_server::test_utils::{mock_replay_api, refused_replay_api};
    use std::time::Duration;
    use tokio::sync::broadcast;

    fn api_for(url: String) -> ReplayApi {
        let (events, _) = broadcast::channel(8);
        ReplayApi::from_client(
            ReplayApiClient::new(url, Some("123456".to_string())),
            events,
        )
    }
    fn config() -> ActiveGameConfig {
        ActiveGameConfig {
            identity: ActiveGameIdentity {
                system: "nintendo_gc".to_string(),
                game_file: "/media/nfs/roms/nintendo_gc/game.iso".to_string(),
            },
            overrides: GameSetting::ALL
                .into_iter()
                .map(|setting| GameOverride {
                    setting,
                    value: None,
                    inherited: None,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn insider_launch_guard_uses_reported_gates() {
        for system in ["nintendo_gc", "sony_ps2"] {
            let locked = api_for(mock_replay_api(
                "200 OK",
                r#"{"version":"2.0.47","insider_enabled":false,"insider_features":["gamecube","playstation2"]}"#,
            ));
            let error = ensure_game_launch_allowed(&locked, system, Locale::Es)
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(t(Locale::Es, Key::ReplayInsiderRequired))
            );
            for payload in [
                r#"{"version":"2.0.0"}"#,
                r#"{"version":"2.0.0","insider_enabled":true,"insider_features":["gamecube","playstation2"]}"#,
                r#"{"version":"2.0.0","insider_enabled":false,"insider_features":["vulkan"]}"#,
            ] {
                let allowed = api_for(mock_replay_api("200 OK", payload));
                ensure_game_launch_allowed(&allowed, system, Locale::En)
                    .await
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    async fn feature_guard_rejects_old_and_unknown_versions() {
        for body in [
            r#"{"version":"1.8.0"}"#,
            r#"{"version":"1.8.0","insider_enabled":true}"#,
            "{}",
        ] {
            let api = api_for(mock_replay_api("200 OK", body));
            for feature in [
                ReplayFeature::GameCube,
                ReplayFeature::PlayStation2,
                ReplayFeature::LinkPlay,
            ] {
                assert!(
                    ensure_feature_available(&api, feature, Locale::En)
                        .await
                        .is_err()
                );
            }
        }
        let api = api_for(mock_replay_api(
            "200 OK",
            r#"{"version":"2.0.0","insider_enabled":false}"#,
        ));
        ensure_feature_available(&api, ReplayFeature::LinkPlay, Locale::En)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn insider_launch_guard_rejects_failed_checks_without_affecting_other_systems() {
        for (status, body) in [
            ("500 Internal Server Error", "{}"),
            ("401 Unauthorized", "{}"),
            ("429 Too Many Requests", "{}"),
            ("200 OK", "not json"),
        ] {
            let api = api_for(mock_replay_api(status, body));
            assert!(
                ensure_game_launch_allowed(&api, "nintendo_gc", Locale::En)
                    .await
                    .is_err()
            );
        }
        let offline = api_for(refused_replay_api());
        assert!(
            ensure_game_launch_allowed(&offline, "sony_ps2", Locale::En)
                .await
                .is_err()
        );
        ensure_game_launch_allowed(&offline, "sega_dc", Locale::En)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn game_override_rejects_changed_or_missing_game() {
        for body in [
            r#"{"system":"nintendo_gc","game_file":"/media/nfs/roms/nintendo_gc/other.iso"}"#,
            r#"{"system":"sony_ps2","game_file":"/media/nfs/roms/nintendo_gc/game.iso"}"#,
            r#"{"system":"nintendo_gc","game_file":""}"#,
        ] {
            let api = api_for(mock_replay_api("200 OK", body));
            assert_eq!(
                apply_active_game_config(&api, config()).await.unwrap(),
                GameSettingsOutcome::GameChanged
            );
        }
    }
    #[tokio::test]
    async fn game_override_accepts_matching_game() {
        let api = api_for(mock_replay_api(
            "200 OK",
            r#"{"system":"nintendo_gc","game_file":"/media/nfs/roms/nintendo_gc/game.iso"}"#,
        ));
        assert_eq!(
            apply_active_game_config(&api, config()).await.unwrap(),
            GameSettingsOutcome::Applied
        );
    }
    #[tokio::test]
    async fn game_override_rejects_invalid_values_before_network() {
        let api = api_for(refused_replay_api());
        let mut invalid = config();
        invalid.overrides[0].value = Some(100);
        assert_eq!(
            apply_active_game_config(&api, invalid).await.unwrap(),
            GameSettingsOutcome::InvalidValue
        );
    }
    #[tokio::test]
    async fn setup_stops_on_rate_limit_without_retry_delay() {
        let api = api_for(mock_replay_api(
            "429 Too Many Requests",
            r#"{"error":"Too Many Requests"}"#,
        ));
        let status = tokio::time::timeout(Duration::from_millis(400), wait_for_replay_api(&api))
            .await
            .expect("must not enter the 500ms retry delay");
        assert!(matches!(status, ReplayApiStatus::Error { .. }));
    }

    #[test]
    fn adopted_replay_api_token_must_be_six_ascii_digits() {
        assert_eq!(
            normalize_adopted_replay_api_token(" 123456 ".to_string()).unwrap(),
            "123456"
        );
        assert!(normalize_adopted_replay_api_token("12345".to_string()).is_err());
        assert!(normalize_adopted_replay_api_token("1234567".to_string()).is_err());
        assert!(normalize_adopted_replay_api_token("12345a".to_string()).is_err());
        assert!(normalize_adopted_replay_api_token("１２３４５６".to_string()).is_err());
    }
}
