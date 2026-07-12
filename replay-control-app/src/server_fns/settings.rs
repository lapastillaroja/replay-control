use super::*;

#[cfg(feature = "ssr")]
use crate::api::response_cache::TtlSlot;
#[cfg(feature = "ssr")]
use replay_control_core::replay_api::{ConfigKind, ReplayApiStatus, SetCommand};
use replay_control_core::update::UpdateChangelog;
#[cfg(feature = "ssr")]
use replay_control_core::update::{ChangelogEntry, UpdateChannel};
#[cfg(feature = "ssr")]
use replay_control_core_server::auth::{PasswordSubject, verify_os_password};
#[cfg(feature = "ssr")]
use replay_control_core_server::config::ReplayConfig;
#[cfg(feature = "ssr")]
use replay_control_core_server::security::tls::{
    TlsCertificateStatus, regenerate_self_signed_certificate, tls_certificate_status,
};

/// WiFi configuration (password is never sent to the client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub country: String,
    pub mode: String,
    pub hidden: bool,
}

/// NFS share configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfsConfig {
    pub server: String,
    pub share: String,
    pub version: String,
}

/// RetroAchievements configuration. The password is write-only and is never
/// returned to the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetroAchievementsConfig {
    pub username: String,
    pub password_configured: bool,
}

/// Skin info for the skin page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinInfo {
    pub index: u32,
    pub name: String,
    pub bg: String,
    pub surface: String,
    pub surface_hover: String,
    pub border: String,
    pub text: String,
    pub text_secondary: String,
    pub accent: String,
    pub accent_hover: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsCertificateInfo {
    pub certificate_path: String,
    pub key_path: String,
    pub fingerprint_sha256: Option<String>,
    pub generated_at: Option<String>,
    pub expires_at: Option<String>,
    pub covered_dns_names: Vec<String>,
    pub covered_ip_addresses: Vec<String>,
    pub current_dns_names: Vec<String>,
    pub current_ip_addresses: Vec<String>,
    pub missing_dns_names: Vec<String>,
    pub missing_ip_addresses: Vec<String>,
}

impl TlsCertificateInfo {
    pub fn has_missing_coverage(&self) -> bool {
        !self.missing_dns_names.is_empty() || !self.missing_ip_addresses.is_empty()
    }
}

#[cfg(feature = "ssr")]
impl SkinInfo {
    fn from_palette(index: u32, name: String, p: &replay_control_core::skins::SkinPalette) -> Self {
        Self {
            index,
            name,
            bg: p.bg.to_string(),
            surface: p.surface.to_string(),
            surface_hover: p.surface_hover.to_string(),
            border: p.border.to_string(),
            text: p.text.to_string(),
            text_secondary: p.text_secondary.to_string(),
            accent: p.accent.to_string(),
            accent_hover: p.accent_hover.to_string(),
        }
    }
}

/// Error for config-reading server fns when there is no readable `replay.cfg`
/// — off-device, or on-device in the `ConfigUnavailable` state. We return this
/// rather than fabricating an empty config object (the read-side equivalent of
/// the deleted `ReplayConfig::empty()`). The UI gates these pages off-device,
/// so reaching here means the config genuinely isn't available.
#[cfg(feature = "ssr")]
fn config_unavailable() -> ServerFnError {
    ServerFnError::new("System configuration is unavailable")
}

#[server(prefix = "/sfn")]
pub async fn get_wifi_config() -> Result<WifiConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned");
    let config = guard.as_ref().ok_or_else(config_unavailable)?;
    Ok(WifiConfig {
        ssid: config.wifi_name().unwrap_or("").to_string(),
        country: config.wifi_country().unwrap_or("").to_string(),
        mode: config.wifi_mode().unwrap_or("transition").to_string(),
        hidden: config.wifi_hidden(),
    })
}

#[server(prefix = "/sfn")]
pub async fn save_wifi_config(
    ssid: String,
    password: String,
    country: String,
    auth_mode: String,
    hidden: bool,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (ssid, password, country, auth_mode) =
        validate_wifi_config(&ssid, &password, &country, &auth_mode)?;
    apply_replay_api_config_change(
        &state,
        vec![
            ("wifi_name", ssid),
            ("wifi_pwd", password),
            ("wifi_country", country),
            ("wifi_mode", auth_mode),
            (
                "wifi_hidden",
                if hidden { "true" } else { "false" }.to_string(),
            ),
        ],
    )
    .await
}

#[server(prefix = "/sfn")]
pub async fn get_nfs_config() -> Result<NfsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned");
    let config = guard.as_ref().ok_or_else(config_unavailable)?;
    Ok(NfsConfig {
        server: config.nfs_server().unwrap_or("").to_string(),
        share: config.nfs_share().unwrap_or("").to_string(),
        version: config.nfs_version().unwrap_or("4").to_string(),
    })
}

#[server(prefix = "/sfn")]
pub async fn save_nfs_config(
    server: String,
    share: String,
    version: String,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (server, share, version) = validate_nfs_config(&server, &share, &version)?;
    apply_replay_api_config_change(
        &state,
        vec![
            ("nfs_server", server),
            ("nfs_share", share),
            ("nfs_version", version),
        ],
    )
    .await
}

#[server(prefix = "/sfn")]
pub async fn get_retroachievements_config() -> Result<RetroAchievementsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let config_path = state.config_file_path();

    // Prefer reading fresh from disk so credentials changed out-of-band
    // (e.g. set on the TV via the RePlayOS UI) are reflected immediately.
    // Any read/parse failure — missing file, transient empty file during an
    // atomic-rename window, malformed bytes — falls back to the in-memory
    // last-known-good config rather than surfacing an error: the GET path
    // should never blank or fail just because we caught the file mid-rewrite.
    let fresh = ReplayConfig::from_file(&config_path).ok();
    let (username, password_configured) = match fresh {
        Some(config) => (
            config
                .retroachievements_username()
                .unwrap_or("")
                .to_string(),
            config.retroachievements_password_configured(),
        ),
        None => {
            let guard = state
                .replay_config
                .read()
                .expect("replay_config lock poisoned");
            let config = guard.as_ref().ok_or_else(config_unavailable)?;
            (
                config
                    .retroachievements_username()
                    .unwrap_or("")
                    .to_string(),
                config.retroachievements_password_configured(),
            )
        }
    };

    Ok(RetroAchievementsConfig {
        username,
        password_configured,
    })
}

#[server(prefix = "/sfn")]
pub async fn save_retroachievements_config_and_restart(
    username: String,
    password: String,
) -> Result<String, ServerFnError> {
    // All-or-nothing: both empty clears the credentials; otherwise both must
    // be provided. Validated at the server-fn entry so the rule applies in
    // both Device (which writes) and Standalone (which skips) — a malformed
    // input is a malformed input regardless of mode, not something the
    // Standalone skip-path should silently accept.
    if username.trim().is_empty() != password.trim().is_empty() {
        return Err(ServerFnError::new(
            "RetroAchievements username and password must be provided together",
        ));
    }
    let state = expect_context::<crate::api::AppState>();
    let username = username.trim().to_string();
    let password = password.trim().to_string();
    apply_replay_api_config_change(
        &state,
        vec![
            ("rcheevos_username", username),
            ("rcheevos_password", password),
        ],
    )
    .await
}

#[cfg(feature = "ssr")]
type ConfigWrite = (&'static str, String);

#[cfg(feature = "ssr")]
async fn apply_replay_api_config_change(
    state: &crate::api::AppState,
    writes: Vec<ConfigWrite>,
) -> Result<String, ServerFnError> {
    if !state.mode.is_device() {
        return Ok("Save skipped (standalone mode)".to_string());
    }

    let Some(api) = state.replay_api.clone() else {
        return Err(ServerFnError::new(
            "RePlayOS Net Control is only available on the device",
        ));
    };
    if !api.status().is_active() {
        return Err(ServerFnError::new(
            "RePlayOS Net Control is not connected. Set it up from Settings > RePlayOS Net Control.",
        ));
    }

    // 1.7.4 `set_config` validates the whole request before writing anything,
    // so a multi-key save is atomic — there is no partial-apply state to report.
    let changes: Vec<(&str, &str)> = writes
        .iter()
        .map(|(option, value)| (*option, value.as_str()))
        .collect();
    if let Err(error) = api.client().set_config(ConfigKind::Replay, &changes).await {
        api.report_error(&error);
        return Err(ServerFnError::new(format!(
            "No settings were saved: {error}. Review the settings and save again."
        )));
    }

    // RePlayOS persists API config writes synchronously; refresh our mirror so
    // read-side pages and skin sync see the new values before the reboot.
    let _ = state.reload_replay_config();

    let _restart_window = api.begin_restart_window();
    if let Err(error) = api.client().set_cmd(SetCommand::Reboot).await {
        api.report_error(&error);
        return Err(ServerFnError::new(format!(
            "Settings were saved, but RePlayOS could not be rebooted: {error}. Reboot from the TV to apply them."
        )));
    }
    api.set_status(ReplayApiStatus::PendingRestart);
    Ok("Settings saved; RePlayOS is rebooting...".to_string())
}

#[cfg(feature = "ssr")]
fn validate_wifi_config(
    ssid: &str,
    password: &str,
    country: &str,
    auth_mode: &str,
) -> Result<(String, String, String, String), ServerFnError> {
    let ssid = ssid.trim().to_string();
    if ssid.is_empty() || ssid.len() > 32 {
        return Err(ServerFnError::new(
            "Wi-Fi network name must be 1-32 characters",
        ));
    }

    if !password.is_empty() && !(8..=63).contains(&password.len()) {
        return Err(ServerFnError::new(
            "Wi-Fi password must be 8-63 characters, or blank for an open network",
        ));
    }

    let country = country.trim().to_ascii_uppercase();
    if country.len() != 2 || !country.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(ServerFnError::new(
            "Wi-Fi country code must be two letters, for example US or ES",
        ));
    }

    let auth_mode = auth_mode.trim().to_string();
    if !matches!(auth_mode.as_str(), "transition" | "wpa2" | "wpa3") {
        return Err(ServerFnError::new("Unsupported Wi-Fi security mode"));
    }

    Ok((ssid, password.to_string(), country, auth_mode))
}

#[cfg(feature = "ssr")]
fn validate_nfs_config(
    server: &str,
    share: &str,
    version: &str,
) -> Result<(String, String, String), ServerFnError> {
    let server = server.trim().to_string();
    if server.is_empty() || server.chars().any(char::is_whitespace) {
        return Err(ServerFnError::new(
            "NFS server must be a hostname or IP address without spaces",
        ));
    }

    let share = share.trim().to_string();
    if !share.starts_with('/') {
        return Err(ServerFnError::new("NFS share path must start with /"));
    }

    let version = version.trim().to_string();
    if !matches!(version.as_str(), "3" | "4") {
        return Err(ServerFnError::new("NFS version must be 3 or 4"));
    }

    Ok((server, share, version))
}

/// Stop the frontend, write the config, then start it again. The `systemctl`
/// calls and the config file I/O are all blocking, so the whole sequence runs
/// on a blocking thread to avoid stalling the async runtime.
///
/// `is_device` is a bool, not the full `Mode`, because this function only
/// branches on "does the OS own this config?" — pre-resolving it at the call
/// site (`state.mode.is_device()`) keeps the signature honest about what it
/// actually needs.
#[cfg(feature = "ssr")]
pub(crate) async fn apply_replay_config_change<F>(
    is_device: bool,
    write_config: F,
) -> Result<String, ServerFnError>
where
    F: FnOnce() -> Result<(), Box<dyn std::error::Error>> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        apply_replay_config_change_blocking(is_device, write_config)
    })
    .await
    .map_err(|e| ServerFnError::new(format!("config change task failed: {e}")))?
}

#[cfg(feature = "ssr")]
fn apply_replay_config_change_blocking<F>(
    is_device: bool,
    write_config: F,
) -> Result<String, ServerFnError>
where
    F: FnOnce() -> Result<(), Box<dyn std::error::Error>>,
{
    if !is_device {
        // Standalone is documented as having system-mutation features disabled —
        // no `replay.service` to restart, no `replay.cfg` we're meant to own, no
        // surrounding RePlayOS state to keep in sync. Refuse the write entirely
        // (don't silently mutate the user's storage folder) and return the same
        // skipped result the device path would have for an inert call. The UI
        // hides these pages off-device; a direct /sfn POST hits this guard.
        let _ = write_config;
        return Ok("Save skipped (standalone mode)".to_string());
    }

    replay_control_core_server::replay_service::stop()
        .map_err(|e| ServerFnError::new(format!("Failed to stop: {e}")))?;

    // The frontend is now down. We must bring it back up no matter what happens
    // during the write — including a panic (e.g. a poisoned config lock), which
    // the guard catches on unwind. The guard is disarmed once we run the
    // explicit start below so that start's own error reaches the user.
    let mut restart_guard = StartReplayOnDrop::armed();
    let save_result = write_config();
    restart_guard.disarm();

    let start_result = replay_control_core_server::replay_service::start();

    match (save_result, start_result) {
        (Ok(()), Ok(())) => Ok("ReplayOS restarted".to_string()),
        (Err(save_error), Ok(())) => {
            Err(ServerFnError::new(format!("Failed to save: {save_error}")))
        }
        (Ok(()), Err(start_error)) => Err(ServerFnError::new(format!(
            "Saved, but failed to restart ReplayOS: {start_error}"
        ))),
        (Err(save_error), Err(start_error)) => Err(ServerFnError::new(format!(
            "Failed to save: {save_error}; also failed to start ReplayOS: {start_error}"
        ))),
    }
}

/// Restarts `replay.service` on drop unless disarmed. Used to guarantee the TV
/// frontend is brought back up after `apply_replay_config_change` stops it,
/// even if the config write panics or returns early.
#[cfg(feature = "ssr")]
struct StartReplayOnDrop {
    armed: bool,
}

#[cfg(feature = "ssr")]
impl StartReplayOnDrop {
    fn armed() -> Self {
        Self { armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(feature = "ssr")]
impl Drop for StartReplayOnDrop {
    fn drop(&mut self) {
        if self.armed
            && let Err(e) = replay_control_core_server::replay_service::start()
        {
            tracing::error!("failed to restart replay.service after aborted config change: {e}");
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn reboot_system() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        return Ok("Reboot skipped (standalone mode)".to_string());
    }

    if let Some(api) = state.replay_api.clone()
        && api.status().is_active()
    {
        let _restart_window = api.begin_restart_window();
        match api.client().set_cmd(SetCommand::Reboot).await {
            Ok(()) => {
                api.set_status(ReplayApiStatus::PendingRestart);
                return Ok("Rebooting...".to_string());
            }
            Err(error) => {
                api.report_error(&error);
                tracing::warn!("API reboot failed, falling back to CLI reboot: {error}");
            }
        }
    }

    // Best-effort flush before reboot, but never *wait* on it: the network share
    // is mounted `hard`, so `sync` blocks indefinitely whenever the NFS server is
    // unreachable — e.g. right after a Wi-Fi change drops the network. Waiting on
    // it (the previous `.output()`) hung this handler so the reboot was never
    // issued. systemd's shutdown sequence syncs and unmounts filesystems anyway,
    // so a fire-and-forget flush is enough.
    let _ = std::process::Command::new("sync").spawn();

    let output = std::process::Command::new("reboot")
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to reboot: {e}")))?;

    if output.status.success() {
        Ok("Rebooting...".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!("Reboot failed: {stderr}")))
    }
}

#[server(prefix = "/sfn")]
pub async fn get_hostname() -> Result<String, ServerFnError> {
    let content = std::fs::read_to_string("/etc/hostname")
        .map_err(|e| ServerFnError::new(format!("Failed to read hostname: {e}")))?;
    Ok(content.trim().to_string())
}

#[server(prefix = "/sfn")]
pub async fn save_hostname(hostname: String) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        return Ok("Hostname change skipped (standalone mode)".to_string());
    }

    let hostname = hostname.trim().to_lowercase();

    // Validate: 1-63 chars, lowercase alphanumeric + hyphens, no leading/trailing hyphens.
    if hostname.is_empty() || hostname.len() > 63 {
        return Err(ServerFnError::new("Hostname must be 1-63 characters"));
    }
    if hostname.starts_with('-') || hostname.ends_with('-') {
        return Err(ServerFnError::new(
            "Hostname must not start or end with a hyphen",
        ));
    }
    if !hostname
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ServerFnError::new(
            "Hostname must contain only lowercase letters, digits, and hyphens",
        ));
    }

    // Read old hostname for /etc/hosts update.
    let old_hostname = std::fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();

    // Step 1: Set hostname via hostnamectl (updates /etc/hostname + kernel).
    let output = std::process::Command::new("hostnamectl")
        .args(["set-hostname", &hostname])
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to set hostname: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServerFnError::new(format!("hostnamectl failed: {stderr}")));
    }

    // Step 2: Update /etc/hosts — replace old hostname with new.
    if !old_hostname.is_empty()
        && old_hostname != hostname
        && let Ok(hosts) = std::fs::read_to_string("/etc/hosts")
    {
        let updated = hosts.replace(&old_hostname, &hostname);
        let _ = std::fs::write("/etc/hosts", updated);
    }

    // Step 3: Restart Avahi so mDNS broadcasts the new name.
    let _ = std::process::Command::new("systemctl")
        .args(["restart", "avahi-daemon"])
        .output();

    match regenerate_self_signed_certificate(&state.data_dir) {
        Ok(_) => {
            restart_for_new_certificate(&state);
            Ok(format!(
                "Hostname set to {hostname}. HTTPS certificate regenerated and Replay Control is restarting; reconnect at https://{hostname}.local:8443/ and accept the new certificate."
            ))
        }
        Err(error) => Ok(format!(
            "Hostname set to {hostname}, but HTTPS certificate regeneration failed: {error}"
        )),
    }
}

#[server(prefix = "/sfn")]
pub async fn get_tls_certificate_info() -> Result<TlsCertificateInfo, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(tls_status_to_info(tls_certificate_status(&state.data_dir)))
}

#[server(prefix = "/sfn")]
pub async fn regenerate_tls_certificate_info() -> Result<TlsCertificateInfo, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    regenerate_self_signed_certificate(&state.data_dir)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let info = tls_status_to_info(tls_certificate_status(&state.data_dir));
    restart_for_new_certificate(&state);
    Ok(info)
}

/// Restart replay-control so the running HTTPS server serves a freshly
/// regenerated certificate (it loads the cert once at startup). The restart is
/// deferred briefly so this response — and the client's scheduled page reload —
/// reach the browser before the server goes down; systemd owns the restart once
/// issued. Failures are logged rather than returned, since the client cannot
/// wait on a restart that kills this process. No-op off-device.
#[cfg(feature = "ssr")]
fn restart_for_new_certificate(state: &crate::api::AppState) {
    if !state.mode.is_device() {
        return;
    }
    tokio::task::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        match std::process::Command::new("systemctl")
            .args(["--no-block", "restart", "replay-control"])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => tracing::error!(
                "certificate service restart was rejected: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => {
                tracing::error!("certificate service restart failed to start: {error}")
            }
        }
    });
}

#[cfg(feature = "ssr")]
fn tls_status_to_info(status: TlsCertificateStatus) -> TlsCertificateInfo {
    TlsCertificateInfo {
        certificate_path: status.cert_path.display().to_string(),
        key_path: status.key_path.display().to_string(),
        fingerprint_sha256: status.fingerprint_sha256,
        generated_at: status.generated_at_unix.map(format_unix_date_utc),
        expires_at: status.expires_at,
        covered_dns_names: status.dns_names,
        covered_ip_addresses: status
            .ip_addresses
            .into_iter()
            .map(|ip| ip.to_string())
            .collect(),
        current_dns_names: status.current_dns_names,
        current_ip_addresses: status
            .current_ip_addresses
            .into_iter()
            .map(|ip| ip.to_string())
            .collect(),
        missing_dns_names: status.missing_dns_names,
        missing_ip_addresses: status
            .missing_ip_addresses
            .into_iter()
            .map(|ip| ip.to_string())
            .collect(),
    }
}

#[cfg(feature = "ssr")]
fn format_unix_date_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let (year, month, day) = civil_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(feature = "ssr")]
fn civil_from_unix_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

/// Skin page data: (active_skin_index, sync_enabled, skins_list).
#[server(prefix = "/sfn")]
pub async fn get_skins() -> Result<(u32, bool, Vec<SkinInfo>), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let skin_pref = state.prefs.read().expect("prefs lock poisoned").skin;
    let current = skin_pref.unwrap_or_else(|| {
        state
            .replay_config
            .read()
            .expect("replay_config lock poisoned")
            .as_ref()
            .map(|c| c.system_skin())
            .unwrap_or(0)
    });
    let sync = skin_pref.is_none();

    let mut skins: Vec<SkinInfo> = replay_control_core::skins::SKIN_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let idx = i as u32;
            SkinInfo::from_palette(
                idx,
                name.to_string(),
                replay_control_core::skins::palette(idx).unwrap(),
            )
        })
        .collect();

    // Without a synthetic entry the grid would have nothing to highlight as
    // active when the user is on a custom replayos skin, leaving them with
    // no signal about what's selected.
    if replay_control_core::skins::is_custom(current) {
        skins.push(SkinInfo::from_palette(
            current,
            format!("CUSTOM #{current}"),
            replay_control_core::skins::palette_or_default(current),
        ));
    }

    Ok((current, sync, skins))
}

#[server(prefix = "/sfn")]
pub async fn set_skin(index: u32) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    // Persist to settings.cfg (not replay.cfg).
    replay_control_core_server::settings::write_skin(&state.settings, Some(index))
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").skin = Some(index);

    let skin_css = replay_control_core::skins::theme_css(index);
    let _ = state.events_tx.send(crate::api::ConfigEvent::SkinChanged {
        skin_index: index,
        skin_css,
    });
    Ok(())
}

#[server(prefix = "/sfn")]
pub async fn set_skin_sync(enabled: bool) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if enabled {
        // Clear the skin from settings.cfg so we defer to replay.cfg.
        replay_control_core_server::settings::write_skin(&state.settings, None)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        state.prefs.write().expect("prefs lock poisoned").skin = None;
    } else {
        let current = state.effective_skin();
        replay_control_core_server::settings::write_skin(&state.settings, Some(current))
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        state.prefs.write().expect("prefs lock poisoned").skin = Some(current);
    }

    let effective = state.effective_skin();
    let skin_css = replay_control_core::skins::theme_css(effective);
    let _ = state.events_tx.send(crate::api::ConfigEvent::SkinChanged {
        skin_index: effective,
        skin_css,
    });
    Ok(())
}

/// Get the font size preference from cached preferences.
#[server(prefix = "/sfn")]
pub async fn get_font_size() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .font_size
        .clone())
}

/// Save the font size preference to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_font_size(size: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_font_size(&state.settings, &size)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").font_size = size;
    Ok(())
}

/// Get the GitHub API key from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_github_api_key() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(
        replay_control_core_server::settings::read_github_api_key(&state.settings)
            .unwrap_or_default(),
    )
}

/// Save the GitHub API key to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_github_api_key(key: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_github_api_key(&state.settings, key.trim())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Get the current region preference from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_region_preference() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = state.region_preference();
    Ok(pref.as_str().to_string())
}

/// Set the region preference in `.replay-control/settings.cfg`.
/// Invalidates in-memory library views so lists are re-sorted with the new preference.
#[server(prefix = "/sfn")]
pub async fn save_region_preference(value: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = replay_control_core::rom_tags::RegionPreference::from_str_value(&value);
    replay_control_core_server::settings::write_region_preference(&state.settings, pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").region = pref;
    // Drop only in-memory views; the resolver below rewrites the region-dependent
    // mirror columns. A destructive `clear_all_game_library` here would
    // truncate rows that any concurrent rebuild/import is filling — see
    // `docs/architecture/cross-activity-coordination.md` finding F-3.
    state.library.invalidate_in_memory_views().await;
    state.invalidate_user_caches().await;
    let region_secondary = state.region_preference_secondary();
    match state
        .library_writer
        .try_write_with_timeout(
            crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::resolve_release_date_for_library(
                    conn,
                    pref,
                    region_secondary,
                )
            },
        )
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => tracing::warn!("Region preference release-date resolve failed: {e}"),
        Err(e) => tracing::warn!("Region preference release-date resolve write failed: {e}"),
    }
    Ok(())
}

/// Get the secondary (fallback) region preference from `.replay-control/settings.cfg`.
/// Returns empty string if not set.
#[server(prefix = "/sfn")]
pub async fn get_region_preference_secondary() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = state.region_preference_secondary();
    Ok(pref.map(|p| p.as_str().to_string()).unwrap_or_default())
}

/// Set the secondary (fallback) region preference in `.replay-control/settings.cfg`.
/// Pass empty string to clear. Invalidates in-memory library views.
#[server(prefix = "/sfn")]
pub async fn save_region_preference_secondary(value: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = if value.is_empty() {
        None
    } else {
        Some(replay_control_core::rom_tags::RegionPreference::from_str_value(&value))
    };
    replay_control_core_server::settings::write_region_preference_secondary(&state.settings, pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state
        .prefs
        .write()
        .expect("prefs lock poisoned")
        .region_secondary = pref;
    // Drop only in-memory views; see save_region_preference for why.
    state.library.invalidate_in_memory_views().await;
    state.invalidate_user_caches().await;
    let region_primary = state.region_preference();
    match state
        .library_writer
        .try_write_with_timeout(
            crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::resolve_release_date_for_library(
                    conn,
                    region_primary,
                    pref,
                )
            },
        )
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            tracing::warn!("Secondary region preference release-date resolve failed: {e}")
        }
        Err(e) => {
            tracing::warn!("Secondary region preference release-date resolve write failed: {e}")
        }
    }
    Ok(())
}
/// Get the language preference from `.replay-control/settings.cfg`.
/// Returns (primary, secondary) where each is empty string if not set.
#[server(prefix = "/sfn")]
pub async fn get_language_preference() -> Result<(String, String), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let settings = state.settings.load();
    let primary = settings.language_primary().unwrap_or_default().to_string();
    let secondary = settings
        .language_secondary()
        .unwrap_or_default()
        .to_string();
    Ok((primary, secondary))
}

/// Save the language preference to `.replay-control/settings.cfg`.
/// Empty strings clear the respective fields (revert to auto-detection).
#[server(prefix = "/sfn")]
pub async fn save_language_preference(
    primary: String,
    secondary: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_language_preferences(
        &state.settings,
        primary.trim(),
        secondary.trim(),
    )
    .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Change the root password on the Pi.
/// Verifies the current password before applying the new one.
#[server(prefix = "/sfn")]
pub async fn change_root_password(
    current_password: String,
    new_password: String,
) -> Result<String, ServerFnError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        return Ok("Password change skipped (standalone mode)".to_string());
    }

    if new_password.is_empty() {
        return Err(ServerFnError::new("Password cannot be empty"));
    }
    if !is_safe_chpasswd_password(&new_password) {
        return Err(ServerFnError::new("Password contains invalid characters"));
    }

    if !verify_os_password(PasswordSubject::Root, &current_password)
        .map_err(|e| ServerFnError::new(e.to_string()))?
    {
        return Err(ServerFnError::new("Current password is incorrect"));
    }

    // The admin fingerprint derives from /etc/shadow, so changing the password
    // invalidates the caller's own admin session. Capture its base role now,
    // while the session still resolves, to re-issue it after the change.
    let admin_reissue = super::auth::admin_session_base_role(&state).await?;

    // Apply the new password via chpasswd.
    let mut child = Command::new("chpasswd")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ServerFnError::new(format!("Failed to run chpasswd: {e}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(format!("root:{new_password}\n").as_bytes())
            .map_err(|e| ServerFnError::new(format!("Failed to write to chpasswd: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| ServerFnError::new(format!("chpasswd failed: {e}")))?;

    if output.status.success() {
        if let Some(base_role) = admin_reissue {
            super::auth::reissue_admin_session(&state, base_role)?;
        }
        Ok("Password changed successfully".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!("chpasswd failed: {stderr}")))
    }
}

/// chpasswd reads one `user:password` record per line, so a newline (or NUL) in
/// the password would inject another record. Reject control characters before
/// piping to it.
#[cfg(feature = "ssr")]
fn is_safe_chpasswd_password(password: &str) -> bool {
    !password.contains(['\n', '\r', '\0'])
}

/// Get the UI locale from cached preferences.
/// Returns the stored locale preference code (including "auto").
#[server(prefix = "/sfn")]
pub async fn get_locale() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    use replay_control_core::locale::Locale;
    let locale = state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .locale
        .map(|l| l.code())
        .unwrap_or(Locale::Auto.code());
    Ok(locale.to_string())
}

/// Save the UI locale to `.replay-control/settings.cfg`.
/// Validates against the supported locale list before writing.
#[server(prefix = "/sfn")]
pub async fn save_locale(locale: String) -> Result<(), ServerFnError> {
    use replay_control_core::locale::Locale;
    if !Locale::all_codes().contains(&locale.as_str()) {
        return Err(ServerFnError::new("Unsupported locale"));
    }
    let parsed = Locale::from_code(&locale);
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_locale(&state.settings, parsed)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").locale = parsed.effective();
    Ok(())
}

/// Get the user's preferred languages as a priority-ordered list.
/// Used by manual search to sort results by language relevance.
#[server(prefix = "/sfn")]
pub async fn get_preferred_languages() -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let settings = state.settings.load();
    let primary = settings.language_primary();
    let secondary = settings.language_secondary();
    let region = state.region_preference();
    Ok(replay_control_core_server::settings::preferred_languages(
        primary, secondary, region,
    ))
}

/// Get the analytics preference from `.replay-control/settings.cfg`.
/// Returns `true` if analytics is enabled (default).
#[server(prefix = "/sfn")]
pub async fn get_analytics_preference() -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(replay_control_core_server::settings::read_analytics_enabled(&state.settings))
}

/// Save the analytics preference to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_analytics_preference(enabled: bool) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_analytics(&state.settings, enabled)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Trigger an immediate update check against GitHub API.
/// Nukes the update dir first, checks, writes available.json if found.
#[server(prefix = "/sfn")]
pub async fn check_for_updates()
-> Result<Option<replay_control_core::update::AvailableUpdate>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    crate::api::updates::perform_update_check(&state)
        .await
        .map_err(|e| ServerFnError::new(format!("Update check failed: {e}")))
}

/// Read the current update channel from settings.cfg.
#[server(prefix = "/sfn")]
pub async fn get_update_channel() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(
        replay_control_core_server::settings::read_update_channel(&state.settings)
            .as_str()
            .to_string(),
    )
}

/// Save the update channel. Nukes the update dir and triggers an immediate re-check.
#[server(prefix = "/sfn")]
pub async fn save_update_channel(channel: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let channel_val = replay_control_core::update::UpdateChannel::from_str_value(&channel);
    replay_control_core_server::settings::write_update_channel(&state.settings, channel_val)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Nuke stale update state and trigger re-check with new channel.
    replay_control_core_server::update::nuke_update_dir();
    let state_clone = state.clone();
    tokio::spawn(async move {
        match crate::api::updates::perform_update_check(&state_clone).await {
            Ok(_) => {}
            Err(e) => tracing::debug!("Re-check after channel switch failed: {e}"),
        }
    });
    Ok(())
}

/// Mark a version as skipped in settings.cfg.
#[server(prefix = "/sfn")]
pub async fn skip_version(version: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_skipped_version(&state.settings, &version)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[cfg(feature = "ssr")]
#[derive(Clone)]
struct CachedChangelog {
    entries: Vec<ChangelogEntry>,
    load_failed: bool,
}

/// Changelog cache (reuses the shared `TtlSlot`, whose `RESPONSE_TTL` is 5 min).
/// The GitHub releases fetch is the expensive, rate-limited part and only changes
/// when a release ships, so cache it process-globally (no `AppState` field) to
/// keep user-triggered views from spending the admin's GitHub token / rate
/// budget. `crate::VERSION` and the repo are constant, so the entries need no
/// key; the channel is re-applied per request. Fetch failures are not cached:
/// the next Settings visit should retry instead of pinning a transient outage.
#[cfg(feature = "ssr")]
static CHANGELOG_CACHE: std::sync::LazyLock<TtlSlot<CachedChangelog>> =
    std::sync::LazyLock::new(TtlSlot::default);

/// Fetch the in-app changelog: every release newer than the running version
/// (newest first, notes rendered to HTML) plus the active channel. The banner
/// uses the channel to decide whether prerelease entries show by default.
#[server(prefix = "/sfn")]
pub async fn get_update_changelog() -> Result<UpdateChangelog, ServerFnError> {
    use replay_control_core_server::update as update_io;

    let state = expect_context::<crate::api::AppState>();
    let settings = state.settings.load();
    let channel = UpdateChannel::from_str_value(settings.update_channel());
    let github_key = settings.github_api_key().map(|s| s.to_string());
    drop(settings);

    let cached = match CHANGELOG_CACHE.get() {
        Some(cached) => cached,
        None => {
            let cached = match update_io::fetch_changelog(
                crate::VERSION,
                &update_io::github_api_base_url(),
                crate::api::updates::REPO,
                github_key.as_deref(),
            )
            .await
            {
                Ok(entries) => CachedChangelog {
                    entries,
                    load_failed: false,
                },
                Err(error) => {
                    tracing::warn!("Changelog fetch failed: {error}");
                    CachedChangelog {
                        entries: Vec::new(),
                        load_failed: true,
                    }
                }
            };
            if !cached.load_failed {
                CHANGELOG_CACHE.set(cached.clone());
            }
            cached
        }
    };

    Ok(UpdateChangelog {
        channel,
        entries: cached.entries,
        load_failed: cached.load_failed,
    })
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn change_root_password_rejects_chpasswd_injection() {
        // A newline (or NUL) would inject a second `user:password` record into
        // chpasswd stdin (`root:evilpass\ndaemon:hacked`); the guard must reject
        // it before the password is piped.
        assert!(!is_safe_chpasswd_password("evilpass\ndaemon:hacked"));
        assert!(!is_safe_chpasswd_password("x\0y"));
        assert!(!is_safe_chpasswd_password("trailing\r"));
        assert!(is_safe_chpasswd_password("goodpass123"));
    }

    #[test]
    fn wifi_validation_trims_and_normalizes_country() {
        let (ssid, password, country, mode) =
            validate_wifi_config(" ReplayNet ", "", "es", "wpa2").unwrap();

        assert_eq!(ssid, "ReplayNet");
        assert_eq!(password, "");
        assert_eq!(country, "ES");
        assert_eq!(mode, "wpa2");
    }

    #[test]
    fn wifi_validation_rejects_short_password() {
        assert!(validate_wifi_config("ReplayNet", "short", "US", "wpa2").is_err());
    }

    #[test]
    fn wifi_validation_rejects_bad_country() {
        assert!(validate_wifi_config("ReplayNet", "", "USA", "wpa2").is_err());
    }

    #[test]
    fn nfs_validation_accepts_trimmed_values() {
        let (server, share, version) =
            validate_nfs_config(" 192.168.1.10 ", " /exports/roms ", "4").unwrap();

        assert_eq!(server, "192.168.1.10");
        assert_eq!(share, "/exports/roms");
        assert_eq!(version, "4");
    }

    #[test]
    fn nfs_validation_rejects_invalid_share_and_version() {
        assert!(validate_nfs_config("192.168.1.10", "exports/roms", "4").is_err());
        assert!(validate_nfs_config("192.168.1.10", "/exports/roms", "2").is_err());
    }
}

/// Start the download + install process for a specific release tag.
/// Returns Ok after spawning the helper script (server will restart shortly).
#[server(prefix = "/sfn")]
pub async fn start_update(tag: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    crate::api::updates::start_update(&state, &tag)
        .await
        .map_err(|e| ServerFnError::new(format!("Update failed: {e}")))
}
