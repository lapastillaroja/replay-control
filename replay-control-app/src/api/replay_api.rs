//! RePlayOS API integration state.
//!
//! Owns the [`ReplayApiClient`] and the [`ReplayApiStatus`] state machine.
//! Constructed device-only (`AppState.replay_api` is `None` in standalone, so
//! off-device code structurally cannot reach the API). Status changes are
//! broadcast as [`ConfigEvent::ReplayApiStatusChanged`] on the app event bus,
//! which feeds the `/sse/events` stream.
//!
//! Token lifecycle: the Net Control code lives in the app's own settings.cfg
//! (`replay_api_token`). It is acquired exactly two ways — manual entry on the
//! Net Control settings page, or the assisted flow's one-time replay.cfg read
//! after enabling `system_net_control`. A TV-side code reset surfaces as 401 →
//! [`ReplayApiStatus::Unauthorized`] → the user re-onboards. Never silently
//! re-read or hot-swapped.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use replay_control_core::replay_api::ReplayApiStatus;
use replay_control_core_server::replay_api::{ApiError, ReplayApiClient};
use tokio::sync::broadcast;

use super::ConfigEvent;

pub struct ReplayApi {
    client: ReplayApiClient,
    status: RwLock<ReplayApiStatus>,
    events_tx: broadcast::Sender<ConfigEvent>,
    /// A self-initiated frontend restart (assisted enable, API reboot) is in
    /// flight: the API being unreachable is expected — hold state instead of
    /// flapping to `Error`.
    restart_in_flight: AtomicBool,
}

impl ReplayApi {
    pub fn new(token: Option<String>, events_tx: broadcast::Sender<ConfigEvent>) -> Self {
        Self::from_client(ReplayApiClient::local(token), events_tx)
    }

    /// Build on an explicit client (custom base URL) — tests, and the seam a
    /// future remote-control mode would use.
    pub fn from_client(client: ReplayApiClient, events_tx: broadcast::Sender<ConfigEvent>) -> Self {
        Self {
            client,
            status: RwLock::new(ReplayApiStatus::NotConfigured),
            events_tx,
            restart_in_flight: AtomicBool::new(false),
        }
    }

    pub fn client(&self) -> &ReplayApiClient {
        &self.client
    }

    pub fn status(&self) -> ReplayApiStatus {
        self.status
            .read()
            .expect("replay_api status lock poisoned")
            .clone()
    }

    /// The only status mutation path — keeps the stored status and the SSE
    /// broadcast from drifting apart. No-op (no broadcast) when unchanged.
    pub fn set_status(&self, status: ReplayApiStatus) {
        let changed = {
            let mut guard = self
                .status
                .write()
                .expect("replay_api status lock poisoned");
            if *guard == status {
                false
            } else {
                *guard = status.clone();
                true
            }
        };
        if changed {
            tracing::info!(?status, "replay_api status changed");
            let _ = self
                .events_tx
                .send(ConfigEvent::ReplayApiStatusChanged { status });
        }
    }

    /// Swap the token the client sends — local only, nothing is written to
    /// RePlayOS. Onboarding paths only; persistence to settings.cfg is the
    /// caller's job (server fns own the `SettingsStore`).
    pub fn swap_local_token(&self, token: Option<String>) {
        self.client.swap_local_token(token);
    }

    pub fn restart_in_flight(&self) -> bool {
        self.restart_in_flight.load(Ordering::Relaxed)
    }

    /// Mark a self-initiated frontend restart window. Probes hold the current
    /// status (instead of flapping to `Error`) until the guard drops.
    pub fn begin_restart_window(&self) -> RestartWindowGuard<'_> {
        self.restart_in_flight.store(true, Ordering::Relaxed);
        RestartWindowGuard { api: self }
    }

    /// Probe `get_version` and update the status machine. Returns the new
    /// status. Used at startup, by the maintenance loop, and by "Check again".
    pub async fn probe(&self) -> ReplayApiStatus {
        let status = if !self.client.has_token() {
            ReplayApiStatus::NotConfigured
        } else {
            match self.client.get_version().await {
                Ok(version) => ReplayApiStatus::Active {
                    version: version.version,
                },
                Err(ApiError::MissingToken) => ReplayApiStatus::NotConfigured,
                Err(ApiError::Unauthorized) => ReplayApiStatus::Unauthorized,
                Err(ApiError::Unreachable { reason }) => {
                    if self.restart_in_flight() {
                        // Expected outage mid-restart: keep the current status
                        // (typically `PendingRestart`).
                        return self.status();
                    }
                    ReplayApiStatus::Error { reason }
                }
                Err(other) => ReplayApiStatus::Error {
                    reason: other.to_string(),
                },
            }
        };
        self.set_status(status.clone());
        status
    }

    /// Classify an error observed by a live call site (now-playing poll,
    /// launch, player controls) into the status machine. 401 stops everything
    /// until the user re-onboards; unreachable outside a restart window is a
    /// transient `Error` the maintenance loop recovers from.
    pub fn report_error(&self, error: &ApiError) {
        match error {
            ApiError::Unauthorized => self.set_status(ReplayApiStatus::Unauthorized),
            ApiError::Unreachable { reason } => {
                if !self.restart_in_flight() {
                    self.set_status(ReplayApiStatus::Error {
                        reason: reason.clone(),
                    });
                }
            }
            // MissingToken means we raced an onboarding reset; Bad/Decode are
            // call-specific failures, not connection-state changes.
            ApiError::MissingToken | ApiError::BadStatus { .. } | ApiError::Decode { .. } => {}
        }
    }
}

pub struct RestartWindowGuard<'a> {
    api: &'a ReplayApi,
}

impl Drop for RestartWindowGuard<'_> {
    fn drop(&mut self) {
        self.api.restart_in_flight.store(false, Ordering::Relaxed);
    }
}

/// Maintenance loop: keeps `Error`/`PendingRestart` self-recovering with a
/// gentle backoff, and re-verifies `Active` occasionally so a dead frontend
/// is noticed even before any live call site reports it.
pub async fn run_replay_api_maintenance(state: super::AppState) {
    use std::time::Duration;

    let Some(api) = state.replay_api.clone() else {
        return;
    };

    // Startup probe.
    api.probe().await;

    let mut backoff = Duration::from_secs(5);
    loop {
        let delay = match api.status() {
            // Self-recovering states: retry with backoff up to 60 s.
            ReplayApiStatus::Error { .. } | ReplayApiStatus::PendingRestart => {
                backoff = (backoff * 2).min(Duration::from_secs(60));
                backoff
            }
            // Healthy or user-action states: low-frequency re-verify. `Active`
            // self-heals within a minute if the frontend died quietly;
            // `NotConfigured`/`Unauthorized`/`Unsupported` only change through
            // onboarding actions, which probe directly.
            _ => {
                backoff = Duration::from_secs(5);
                Duration::from_secs(60)
            }
        };
        tokio::time::sleep(delay).await;

        match api.status() {
            ReplayApiStatus::NotConfigured
            | ReplayApiStatus::Unauthorized
            | ReplayApiStatus::Unsupported { .. } => continue,
            _ => {
                api.probe().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use replay_control_core_server::test_utils::{mock_replay_api, refused_replay_api};

    use super::*;

    const VERSION_BODY: &str = r#"{"version":"RePlayOS v1.7.3"}"#;

    fn api_with(
        base_url: String,
        token: Option<&str>,
    ) -> (ReplayApi, broadcast::Receiver<ConfigEvent>) {
        let (events_tx, events_rx) = broadcast::channel(8);
        let client = ReplayApiClient::new(base_url, token.map(str::to_string));
        (ReplayApi::from_client(client, events_tx), events_rx)
    }

    #[tokio::test]
    async fn probe_reaches_active() {
        let (api, _rx) = api_with(mock_replay_api("200 OK", VERSION_BODY), Some("123456"));
        let status = api.probe().await;
        assert_eq!(
            status,
            ReplayApiStatus::Active {
                version: "RePlayOS v1.7.3".to_string()
            }
        );
        assert!(api.status().is_active());
    }

    #[tokio::test]
    async fn probe_without_token_is_not_configured() {
        // No network involved: short-circuits before any request.
        let (api, _rx) = api_with(refused_replay_api(), None);
        assert_eq!(api.probe().await, ReplayApiStatus::NotConfigured);
    }

    #[tokio::test]
    async fn probe_classifies_unauthorized() {
        let (api, _rx) = api_with(
            mock_replay_api("401 Unauthorized", r#"{"error":"Unauthorized"}"#),
            Some("000000"),
        );
        assert_eq!(api.probe().await, ReplayApiStatus::Unauthorized);
    }

    #[tokio::test]
    async fn probe_unreachable_is_error_outside_restart_window() {
        let (api, _rx) = api_with(refused_replay_api(), Some("123456"));
        assert!(matches!(api.probe().await, ReplayApiStatus::Error { .. }));
    }

    #[tokio::test]
    async fn restart_window_holds_status_through_expected_outage() {
        let (api, _rx) = api_with(refused_replay_api(), Some("123456"));
        api.set_status(ReplayApiStatus::PendingRestart);

        {
            let _window = api.begin_restart_window();
            // Unreachable mid-restart is expected: status holds.
            assert_eq!(api.probe().await, ReplayApiStatus::PendingRestart);
        }

        // Window over: the same outage is now a real error.
        assert!(matches!(api.probe().await, ReplayApiStatus::Error { .. }));
    }

    #[tokio::test]
    async fn set_status_broadcasts_only_real_changes() {
        let (api, mut rx) = api_with(refused_replay_api(), None);
        api.set_status(ReplayApiStatus::Unauthorized);
        api.set_status(ReplayApiStatus::Unauthorized);

        let first = rx.try_recv().expect("one event for the change");
        assert!(matches!(
            first,
            ConfigEvent::ReplayApiStatusChanged {
                status: ReplayApiStatus::Unauthorized
            }
        ));
        assert!(rx.try_recv().is_err(), "no duplicate event for a no-op set");
    }

    #[tokio::test]
    async fn report_error_maps_into_the_state_machine() {
        let (api, _rx) = api_with(refused_replay_api(), Some("123456"));
        api.set_status(ReplayApiStatus::Active {
            version: "RePlayOS v1.7.3".into(),
        });

        // Call-specific failures don't change connection state.
        api.report_error(&ApiError::BadStatus {
            status: 404,
            message: "Invalid Game".into(),
        });
        assert!(api.status().is_active());

        // Unreachable inside a restart window is expected.
        {
            let _window = api.begin_restart_window();
            api.report_error(&ApiError::Unreachable {
                reason: "refused".into(),
            });
            assert!(api.status().is_active());
        }

        // Outside it, it's a transient Error.
        api.report_error(&ApiError::Unreachable {
            reason: "refused".into(),
        });
        assert!(matches!(api.status(), ReplayApiStatus::Error { .. }));

        // 401 always wins: re-onboard required.
        api.report_error(&ApiError::Unauthorized);
        assert_eq!(api.status(), ReplayApiStatus::Unauthorized);
    }
}
