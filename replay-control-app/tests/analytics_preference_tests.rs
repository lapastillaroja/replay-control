#![cfg(feature = "ssr")]

use replay_control_app::api::analytics::{AnalyticsClient, build_analytics_ping};
use replay_control_core_server::settings::{SettingsStore, write_analytics};

#[tokio::test]
async fn saved_analytics_preference_controls_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(temp.path());
    write_analytics(&store, false).unwrap();
    assert!(build_analytics_ping(&store).is_none());

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/ping")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let client = AnalyticsClient::new(reqwest::Client::new(), format!("{}/ping", server.url()));

    write_analytics(&store, true).unwrap();
    let (ping, _) = build_analytics_ping(&store).unwrap();
    assert!(client.send(&ping).await);
    mock.assert_async().await;

    write_analytics(&store, false).unwrap();
    assert!(build_analytics_ping(&store).is_none());
}
