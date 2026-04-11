use replay_control_core::settings;

pub const ENDPOINT: &str = "https://replay-control-analytics.bubbleb.workers.dev/ping";

/// Anonymous analytics ping payload.
#[derive(Debug, serde::Serialize)]
pub struct PingRequest {
    pub event: String,
    pub install_id: String,
    pub version: String,
    pub arch: String,
    pub channel: String,
}

/// Pure HTTP analytics client — no settings, no storage, no UUID generation.
pub struct AnalyticsClient {
    client: reqwest::Client,
    endpoint: String,
}

impl AnalyticsClient {
    /// Production constructor — accepts a shared HTTP client and endpoint URL.
    pub fn new(client: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self {
            client,
            endpoint: endpoint.into(),
        }
    }

    #[cfg(test)]
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.into(),
        }
    }

    /// Send a POST request with the given ping payload.
    ///
    /// Returns `true` on 2xx, `false` otherwise. Errors are logged as warnings.
    pub async fn send(&self, ping: &PingRequest) -> bool {
        tracing::debug!("Analytics ping: {ping:?}");

        let result = self.client.post(&self.endpoint).json(ping).send().await;

        match result {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::warn!("Analytics ping failed: {e}");
                false
            }
        }
    }
}

/// Build an analytics ping from current settings, or `None` if analytics is disabled.
///
/// Returns `Some((ping, is_install))` where `is_install` is `true` when this is a new version.
/// Generates and persists a new install ID if one doesn't exist or is invalid.
///
/// NOTE: `storage_root` is a temporary parameter — after the Pi-level settings migration
/// this function will take no arguments (settings live at `/etc/replay-control/settings.cfg`).
pub fn build_analytics_ping(storage_root: &std::path::Path) -> Option<(PingRequest, bool)> {
    let mut app_settings = settings::load_settings(storage_root);

    if !app_settings.analytics_enabled() {
        return None;
    }

    let install_id = match app_settings
        .install_id()
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
    {
        Some(id) => id.to_string(),
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            app_settings.set_install_id(&id);
            let _ = settings::save_settings(storage_root, &app_settings);
            id
        }
    };

    let current_version = crate::VERSION;
    let is_install = app_settings.version_last_reported() != Some(current_version);

    let channel =
        replay_control_core::update::UpdateChannel::from_str_value(app_settings.update_channel());

    let ping = PingRequest {
        event: if is_install { "install" } else { "heartbeat" }.to_string(),
        install_id,
        version: current_version.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        channel: channel.as_str().to_string(),
    };

    Some((ping, is_install))
}

/// Persist the current version as `version_last_reported`.
///
/// NOTE: `storage_root` is a temporary parameter — after the Pi-level settings migration
/// this function will take no arguments.
pub fn mark_version_reported(storage_root: &std::path::Path) {
    let _ = settings::write_version_last_reported(storage_root, crate::VERSION);
}

#[cfg(test)]
mod tests {
    use super::{AnalyticsClient, PingRequest, build_analytics_ping, mark_version_reported};

    fn settings_path(tmp: &std::path::Path) -> std::path::PathBuf {
        tmp.join(".replay-control/settings.cfg")
    }

    fn write_settings(tmp: &std::path::Path, content: &str) {
        let path = settings_path(tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }

    fn read_settings(tmp: &std::path::Path) -> String {
        std::fs::read_to_string(settings_path(tmp)).unwrap_or_default()
    }

    fn make_tmp() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("replay-analytics-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_ping_returns_install_on_new_version() {
        let tmp = make_tmp();
        let (ping, is_install) = build_analytics_ping(&tmp).unwrap();
        assert!(is_install);
        assert_eq!(ping.event, "install");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn build_ping_returns_heartbeat_after_version_reported() {
        let tmp = make_tmp();
        write_settings(
            &tmp,
            &format!("version_last_reported = \"{}\"\n", crate::VERSION),
        );
        let (ping, is_install) = build_analytics_ping(&tmp).unwrap();
        assert!(!is_install);
        assert_eq!(ping.event, "heartbeat");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn build_ping_returns_none_when_opted_out() {
        let tmp = make_tmp();
        write_settings(&tmp, "analytics = \"false\"\n");
        assert!(build_analytics_ping(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn build_ping_replaces_invalid_uuid() {
        let tmp = make_tmp();
        write_settings(&tmp, "install_id = \"not-a-uuid\"\n");
        let (ping, _) = build_analytics_ping(&tmp).unwrap();
        uuid::Uuid::parse_str(&ping.install_id).expect("install_id should be a valid UUID");
        assert!(
            !read_settings(&tmp).contains("not-a-uuid"),
            "invalid UUID should have been replaced"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn mark_version_reported_persists_version() {
        let tmp = make_tmp();
        mark_version_reported(&tmp);
        let content = read_settings(&tmp);
        assert!(content.contains("version_last_reported"));
        assert!(content.contains(crate::VERSION));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn send_returns_true_on_2xx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/ping")
            .with_status(200)
            .create_async()
            .await;
        let client = AnalyticsClient::with_endpoint(format!("{}/ping", server.url()));
        let ping = PingRequest {
            event: "install".to_string(),
            install_id: "00000000-0000-0000-0000-000000000000".to_string(),
            version: crate::VERSION.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            channel: "stable".to_string(),
        };
        assert!(client.send(&ping).await);
    }

    #[tokio::test]
    async fn send_returns_false_on_5xx() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/ping")
            .with_status(500)
            .create_async()
            .await;
        let client = AnalyticsClient::with_endpoint(format!("{}/ping", server.url()));
        let ping = PingRequest {
            event: "heartbeat".to_string(),
            install_id: "00000000-0000-0000-0000-000000000000".to_string(),
            version: crate::VERSION.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            channel: "stable".to_string(),
        };
        assert!(!client.send(&ping).await);
    }
}
