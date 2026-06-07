//! Server functions for the RePlayOS Net Control integration.
//!
//! Onboarding has exactly two paths (see the integration plan):
//! - **Assisted** ([`enable_replay_api_assisted`]): flips `system_net_control`
//!   in replay.cfg through the stop→write→start dance (the one sanctioned
//!   frontend restart left in the app), then reads the token RePlayOS
//!   generated — the single replay.cfg read in the token lifecycle.
//! - **Manual** ([`verify_replay_api_token`]): the user types the Net Control
//!   code shown on the TV; we verify it against `get_version` before storing.
//!
//! In both cases the token ends up in the app's own settings.cfg and the
//! in-memory client; replay.cfg is never re-read for it afterwards.

use super::*;

#[cfg(feature = "ssr")]
use replay_control_core_server::replay_api::{ApiError, ReplayApiClient};
#[cfg(feature = "ssr")]
use replay_control_core_server::settings::write_replay_api_token;

use replay_control_core::replay_api::ReplayApiStatus;
#[cfg(feature = "ssr")]
use replay_control_core::replay_api::SetCommand;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReplayPlayerCommand {
    Screenshot,
    Halt,
    Mute,
    VolumeDown,
    VolumeUp,
    GameReset,
}

impl ReplayPlayerCommand {
    #[cfg(feature = "ssr")]
    fn set_command(self) -> SetCommand {
        match self {
            ReplayPlayerCommand::Screenshot => SetCommand::Screenshot,
            ReplayPlayerCommand::Halt => SetCommand::Halt,
            ReplayPlayerCommand::Mute => SetCommand::Mute,
            ReplayPlayerCommand::VolumeDown => SetCommand::VolumeDown,
            ReplayPlayerCommand::VolumeUp => SetCommand::VolumeUp,
            ReplayPlayerCommand::GameReset => SetCommand::GameReset,
        }
    }
}

/// Current integration status. Standalone mode has no integration and reports
/// the default (`NotConfigured`); UI surfaces gate on `get_mode` anyway.
#[server(prefix = "/sfn")]
pub async fn get_replay_api_status() -> Result<ReplayApiStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state
        .replay_api
        .as_ref()
        .map(|api| api.status())
        .unwrap_or_default())
}

/// Re-probe the API and return the resulting status. Backs the "Check again" /
/// "Retry" actions on the Net Control settings page.
#[server(prefix = "/sfn")]
pub async fn reprobe_replay_api() -> Result<ReplayApiStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
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
    let state = expect_context::<crate::api::AppState>();
    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(
            "RePlayOS Net Control is only available on the device",
        ));
    };
    if !api.status().is_active() {
        return Err(ServerFnError::new("RePlayOS Net Control is not connected"));
    }

    if let Err(e) = api.client().set_cmd(command.set_command()).await {
        api.report_error(&e);
        return Err(ServerFnError::new(e.to_string()));
    }

    Ok(())
}

/// Manual onboarding: verify a user-entered Net Control code and store it on
/// success. The candidate code is probed with a throwaway client so a typo
/// never clobbers a working stored token.
#[server(prefix = "/sfn")]
pub async fn verify_replay_api_token(code: String) -> Result<ReplayApiStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
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
    match candidate.get_version().await {
        Ok(version) => {
            write_replay_api_token(&state.settings, &code)
                .map_err(|e| ServerFnError::new(format!("Failed to store the code: {e}")))?;
            api.swap_local_token(Some(code));
            api.set_status(ReplayApiStatus::Active {
                version: version.version,
            });
            Ok(api.status())
        }
        Err(ApiError::Unauthorized) => Err(ServerFnError::new(
            "RePlayOS rejected the code — check SYSTEM > INFORMATION and try again",
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
    let state = expect_context::<crate::api::AppState>();
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

    let status = api.probe().await;
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
fn unsupported_replayos_error(api: &crate::api::replay_api::ReplayApi) -> ServerFnError {
    api.set_status(ReplayApiStatus::Unsupported { version: None });
    ServerFnError::new(
        "This RePlayOS version doesn't support remote control — update RePlayOS on the TV",
    )
}

#[cfg(feature = "ssr")]
fn replay_cfg_has_net_control_key(state: &crate::api::AppState) -> bool {
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

#[cfg(feature = "ssr")]
fn adopt_token(
    state: &crate::api::AppState,
    api: &crate::api::replay_api::ReplayApi,
    token: String,
) -> Result<(), ServerFnError> {
    write_replay_api_token(&state.settings, &token)
        .map_err(|e| ServerFnError::new(format!("Failed to store the code: {e}")))?;
    api.swap_local_token(Some(token));
    Ok(())
}

#[cfg(feature = "ssr")]
fn is_valid_net_control_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|c| c.is_ascii_digit())
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
    use super::is_valid_net_control_code;

    #[test]
    fn net_control_code_is_exactly_six_digits() {
        assert!(is_valid_net_control_code("123456"));
        assert!(!is_valid_net_control_code(""));
        assert!(!is_valid_net_control_code("12345"));
        assert!(!is_valid_net_control_code("1234567"));
        assert!(!is_valid_net_control_code("12345a"));
        assert!(!is_valid_net_control_code("１２３４５６"));
    }
}
