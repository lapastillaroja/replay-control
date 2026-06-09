use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use replay_control_core::error::{Error, Result};

/// Default location of `replay.cfg` on the RePlayOS device.
pub const DEFAULT_REPLAY_CFG: &str = "/media/sd/config/replay.cfg";

/// Path to `replay.cfg` relative to a storage root (used off-device with
/// `--storage-path`).
const CONFIG_SUBPATH: &str = "config/replay.cfg";

/// Resolve where `replay.cfg` lives. On the RePlayOS device it has exactly one
/// home — the SD card (`DEFAULT_REPLAY_CFG`). Off-device (a `--storage-path`
/// override, i.e. local/dev) the exact location is immaterial, so we just keep
/// it under that storage root. The RePlayOS file layout is owned here in
/// core-server, not decided by the app. Pure path manipulation; no disk access.
pub fn replay_config_path(storage_override: Option<&Path>) -> PathBuf {
    match storage_override {
        Some(storage) => storage.join(CONFIG_SUBPATH),
        None => PathBuf::from(DEFAULT_REPLAY_CFG),
    }
}

/// Internal key=value file parser with comment-preserving write.
/// Not exposed publicly — only used by `ReplayConfig` and `AppSettings`.
/// `Default` is an empty file (no entries) — the honest way to build an empty
/// config without round-tripping through the string parser.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyValueFile {
    entries: HashMap<String, String>,
}

impl KeyValueFile {
    pub(crate) fn parse(content: &str) -> Result<Self> {
        let mut entries = HashMap::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(Error::ConfigParse {
                    line: line_num + 1,
                    message: format!("expected 'key = \"value\"', got: {line}"),
                });
            };

            let key = key.trim().to_string();
            let value = value.trim().trim_matches('"').to_string();
            entries.insert(key, value);
        }

        Ok(Self { entries })
    }

    pub(crate) fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        Self::parse(&content)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Get a value only if present and non-empty.
    pub(crate) fn get_non_empty(&self, key: &str) -> Option<&str> {
        self.get(key).filter(|v| !v.is_empty())
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        self.entries.insert(key.to_string(), value.to_string());
    }

    /// Write the config back to a file, preserving comments, blank lines,
    /// and key order from the original file. New keys are appended at the end.
    pub(crate) fn write(&self, original_path: &Path, output_path: &Path) -> Result<()> {
        let original_content =
            std::fs::read_to_string(original_path).map_err(|e| Error::io(original_path, e))?;
        let output = self.render_preserving(&original_content);
        write_atomic(output_path, output.as_bytes())
    }

    fn render_preserving(&self, original_content: &str) -> String {
        let mut written_keys: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut output = String::new();

        for line in original_content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                output.push_str(line);
                output.push('\n');
                continue;
            }

            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                if let Some(value) = self.entries.get(key) {
                    output.push_str(&format!("{key} = \"{value}\"\n"));
                    written_keys.insert(key);
                } else {
                    output.push_str(line);
                    output.push('\n');
                }
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }

        // Append any new keys not in the original file (sorted for determinism)
        let mut new_keys: Vec<&String> = self
            .entries
            .keys()
            .filter(|k| !written_keys.contains(k.as_str()))
            .collect();
        new_keys.sort();
        for key in new_keys {
            let value = &self.entries[key];
            output.push_str(&format!("{key} = \"{value}\"\n"));
        }

        output
    }
}

// ── ReplayConfig: typed access to replay.cfg ─────────────────────

/// Parsed RePlayOS system configuration from `replay.cfg`.
///
/// This is an external file owned by the OS. The app may read all keys
/// but may only write wifi and NFS settings.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    inner: KeyValueFile,
}

impl ReplayConfig {
    /// Parse a `replay.cfg` file from the given path.
    ///
    /// Rejects empty / whitespace-only files with an explicit `ConfigEmpty`
    /// error: callers that adopt an empty config would silently default
    /// `storage_mode` to "sd" and blank wifi/NFS/RA — exactly the regression
    /// the read-side stat-then-parse race used to allow when the file was
    /// truncated mid-rewrite. Centralising the check here means every
    /// production load path (boot, watcher reload, save-side read-modify-write)
    /// shares the same protection, not just the one with the stat guard.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        if content.trim().is_empty() {
            return Err(Error::Other(format!(
                "replay.cfg at {} is empty",
                path.display()
            )));
        }
        Ok(Self {
            inner: KeyValueFile::parse(&content)?,
        })
    }

    /// Parse config from a string. Test-only: production always loads from a
    /// file via [`Self::from_file`]. Synthesizing a config from a string in
    /// request/boot paths (notably `parse("")`) is what blanked live settings,
    /// so it is not available outside tests.
    #[cfg(test)]
    pub fn parse(content: &str) -> Result<Self> {
        Ok(Self {
            inner: KeyValueFile::parse(content)?,
        })
    }

    // ── Read accessors ───────────────────────────────────────────

    pub fn storage_mode(&self) -> &str {
        self.inner.get("system_storage").unwrap_or("sd")
    }

    pub fn wifi_name(&self) -> Option<&str> {
        self.inner.get("wifi_name")
    }

    pub fn wifi_country(&self) -> Option<&str> {
        self.inner.get("wifi_country")
    }

    pub fn wifi_mode(&self) -> Option<&str> {
        self.inner.get("wifi_mode")
    }

    pub fn wifi_hidden(&self) -> bool {
        self.inner.get("wifi_hidden").unwrap_or("false") == "true"
    }

    pub fn nfs_server(&self) -> Option<&str> {
        self.inner.get("nfs_server")
    }

    pub fn nfs_share(&self) -> Option<&str> {
        self.inner.get("nfs_share")
    }

    pub fn nfs_version(&self) -> Option<&str> {
        self.inner.get("nfs_version")
    }

    pub fn replay_http_token(&self) -> Option<&str> {
        self.inner.get_non_empty("replay_http_token")
    }

    pub fn system_net_control_enabled(&self) -> bool {
        self.inner.get("system_net_control") == Some("true")
    }

    pub fn system_kiosk_mode_enabled(&self) -> bool {
        self.inner.get("system_kiosk_mode") == Some("true")
    }

    /// Whether replay.cfg has the `system_net_control` key at all (true OR
    /// false). Doubles as a version marker: the option doesn't exist before
    /// RePlayOS 1.7.3, and newer frontends serialize their full config on
    /// shutdown — key absent ⇒ the installed RePlayOS predates the API.
    pub fn has_net_control_key(&self) -> bool {
        self.inner.get("system_net_control").is_some()
    }

    pub fn video_mode(&self) -> Option<&str> {
        self.inner.get("video_mode")
    }

    pub fn video_connector(&self) -> Option<&str> {
        self.inner.get("video_connector")
    }

    /// Active skin index from `replay.cfg` (0-10 for built-in skins).
    /// Used as fallback when the app has no skin preference in `settings.cfg`
    /// (i.e., sync mode is on). Defaults to 0 (the REPLAY skin).
    pub fn system_skin(&self) -> u32 {
        self.inner
            .get("system_skin")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    pub fn retroachievements_username(&self) -> Option<&str> {
        self.inner.get_non_empty("rcheevos_username")
    }

    pub fn retroachievements_password_configured(&self) -> bool {
        self.inner.get_non_empty("rcheevos_password").is_some()
    }

    // ── Write methods ───────────────────────────────────────────

    /// Enable or disable RePlayOS Net Control. RePlayOS owns token generation;
    /// Replay Control only toggles the feature flag during assisted setup.
    pub fn set_system_net_control(&mut self, enabled: bool) {
        self.inner
            .set("system_net_control", if enabled { "true" } else { "false" });
    }

    /// Write back to disk, preserving comments and key order from the original
    /// file.
    ///
    /// RePlayOS owns the creation of `replay.cfg`; we never create it. If the
    /// original is missing we refuse to write and return an error so the caller
    /// can surface it to the user instead of silently creating a config that
    /// RePlayOS would not recognize.
    pub fn write_to_file(&self, original_path: &Path, output_path: &Path) -> Result<()> {
        if !original_path.exists() {
            tracing::error!(
                path = %original_path.display(),
                "refusing to write config: file does not exist (RePlayOS owns its creation)"
            );
            return Err(Error::Other(format!(
                "config file {} does not exist; it must be created by RePlayOS first",
                original_path.display()
            )));
        }
        if std::fs::metadata(original_path)
            .map_err(|e| Error::io(original_path, e))?
            .len()
            == 0
        {
            tracing::error!(
                path = %original_path.display(),
                "refusing to write config: file is empty"
            );
            return Err(Error::Other(format!(
                "config file {} is empty; refusing to rewrite it",
                original_path.display()
            )));
        }
        self.inner.write(original_path, output_path)
    }
}

// ── AppSettings: typed access to settings.cfg ────────────────────

/// App-specific settings stored in `.replay-control/settings.cfg`.
///
/// This file is entirely owned by the app. All keys have typed accessors.
#[derive(Debug, Clone)]
pub struct AppSettings {
    inner: KeyValueFile,
}

impl AppSettings {
    /// Load settings from an existing file.
    pub fn from_file(path: &Path) -> Result<Self> {
        Ok(Self {
            inner: KeyValueFile::from_file(path)?,
        })
    }

    /// Create empty settings (when the file doesn't exist yet).
    pub fn empty() -> Self {
        Self {
            inner: KeyValueFile::default(),
        }
    }

    /// Save to disk, preserving comments and order if the file already exists.
    /// If the app-owned settings file is missing, recreate it from the current
    /// settings values.
    pub fn save(&self, path: &Path) -> Result<()> {
        if path.exists() {
            self.inner.write(path, path)
        } else {
            let output = self.inner.render_preserving("");
            write_atomic(path, output.as_bytes())
        }
    }

    // ── Read accessors ───────────────────────────────────────────

    pub fn region_preference(&self) -> &str {
        self.inner.get("region_preference").unwrap_or("world")
    }

    pub fn region_preference_secondary(&self) -> Option<&str> {
        self.inner.get_non_empty("region_preference_secondary")
    }

    pub fn font_size(&self) -> &str {
        match self.inner.get("font_size") {
            Some("large") => "large",
            _ => "normal",
        }
    }

    pub fn skin(&self) -> Option<u32> {
        self.inner.get_non_empty("skin")?.parse().ok()
    }

    pub fn language_primary(&self) -> Option<&str> {
        self.inner.get_non_empty("language_primary")
    }

    pub fn language_secondary(&self) -> Option<&str> {
        self.inner.get_non_empty("language_secondary")
    }

    /// Returns the effective locale for rendering (None = auto-detect from browser).
    /// Returns `None` when the stored value is "auto", missing, or unrecognized.
    pub fn locale(&self) -> Option<replay_control_core::locale::Locale> {
        let code = self.inner.get_non_empty("locale")?;
        replay_control_core::locale::Locale::from_code(code).effective()
    }

    /// Returns the stored locale preference including `Auto`.
    /// Defaults to `Auto` when no value is stored.
    pub fn locale_preference(&self) -> replay_control_core::locale::Locale {
        self.inner
            .get_non_empty("locale")
            .map(replay_control_core::locale::Locale::from_code)
            .unwrap_or(replay_control_core::locale::Locale::Auto)
    }

    pub fn github_api_key(&self) -> Option<&str> {
        self.inner.get_non_empty("github_api_key")
    }

    /// RePlayOS Net Control code, stored app-side after onboarding (manual
    /// entry or the assisted post-restart read). Never re-read from replay.cfg
    /// at runtime: TV-side code resets surface as 401 → re-onboard.
    pub fn replay_api_token(&self) -> Option<&str> {
        self.inner.get_non_empty("replay_api_token")
    }

    pub fn update_channel(&self) -> &str {
        self.inner.get("update_channel").unwrap_or("stable")
    }

    pub fn skipped_version(&self) -> Option<&str> {
        self.inner.get_non_empty("skipped_version")
    }

    pub fn install_id(&self) -> Option<&str> {
        self.inner.get_non_empty("install_id")
    }

    pub fn version_last_reported(&self) -> Option<&str> {
        self.inner.get_non_empty("version_last_reported")
    }

    /// Whether analytics is enabled. Default: true (opt-out model).
    pub fn analytics_enabled(&self) -> bool {
        self.inner.get("analytics") != Some("false")
    }

    // ── Write accessors ──────────────────────────────────────────

    pub fn set_region_preference(&mut self, value: &str) {
        self.inner.set("region_preference", value);
    }

    pub fn set_region_preference_secondary(&mut self, value: &str) {
        self.inner.set("region_preference_secondary", value);
    }

    pub fn set_font_size(&mut self, size: &str) {
        let value = if size == "large" { "large" } else { "normal" };
        self.inner.set("font_size", value);
    }

    pub fn set_skin(&mut self, skin: Option<u32>) {
        match skin {
            Some(index) => self.inner.set("skin", &index.to_string()),
            None => self.inner.set("skin", ""),
        }
    }

    pub fn set_language_primary(&mut self, lang: &str) {
        self.inner.set("language_primary", lang);
    }

    pub fn set_language_secondary(&mut self, lang: &str) {
        self.inner.set("language_secondary", lang);
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.inner.set("locale", locale);
    }

    pub fn set_github_api_key(&mut self, key: &str) {
        self.inner.set("github_api_key", key);
    }

    pub fn set_replay_api_token(&mut self, token: &str) {
        self.inner.set("replay_api_token", token);
    }

    pub fn set_update_channel(&mut self, channel: &str) {
        self.inner.set("update_channel", channel);
    }

    pub fn set_skipped_version(&mut self, version: &str) {
        self.inner.set("skipped_version", version);
    }

    pub fn set_install_id(&mut self, id: &str) {
        self.inner.set("install_id", id);
    }

    pub fn set_version_last_reported(&mut self, version: &str) {
        self.inner.set("version_last_reported", version);
    }

    pub fn set_analytics(&mut self, enabled: bool) {
        self.inner
            .set("analytics", if enabled { "true" } else { "false" });
    }

    /// Whether the first-run setup checklist has been dismissed.
    /// Default: false (show the setup banner on first run).
    pub fn setup_dismissed(&self) -> bool {
        self.inner.get("setup_dismissed") == Some("true")
    }

    pub fn set_setup_dismissed(&mut self, dismissed: bool) {
        self.inner
            .set("setup_dismissed", if dismissed { "true" } else { "false" });
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::Other(format!(
            "cannot write config file {}: path has no parent directory",
            path.display()
        ))
    })?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| Error::io(parent, e))?;

    #[cfg(unix)]
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;

        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(
                metadata.permissions().mode(),
            ))
            .map_err(|e| Error::io(path, e))?;
    }

    tmp.write_all(contents).map_err(|e| Error::io(path, e))?;
    tmp.as_file().sync_all().map_err(|e| Error::io(path, e))?;
    tmp.persist(path)
        .map(|_| ())
        .map_err(|e| Error::io(path, e.error))?;

    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_config_path_resolves_storage_override() {
        // Off-device (--storage-path) → <storage>/config/replay.cfg.
        assert_eq!(
            replay_config_path(Some(Path::new("/media/usb"))),
            PathBuf::from("/media/usb/config/replay.cfg")
        );
        // On-device default — the SD card is replay.cfg's only home.
        assert_eq!(replay_config_path(None), PathBuf::from(DEFAULT_REPLAY_CFG));
    }

    #[test]
    fn parse_basic_config() {
        let content = r#"
            system_storage = "usb"
            wifi_name = "MyWifi"
            wifi_pwd = "secret123"
            video_mode = "5"
        "#;

        let config = ReplayConfig::parse(content).unwrap();
        assert_eq!(config.storage_mode(), "usb");
        assert_eq!(config.wifi_name(), Some("MyWifi"));
        assert_eq!(config.video_mode(), Some("5"));
    }

    #[test]
    fn parse_with_comments_and_blanks() {
        let content = "# comment\n\nsystem_storage = \"sd\"\n";
        let config = ReplayConfig::parse(content).unwrap();
        assert_eq!(config.storage_mode(), "sd");
    }

    #[test]
    fn default_storage_mode() {
        let config = ReplayConfig::parse("").unwrap();
        assert_eq!(config.storage_mode(), "sd");
    }

    #[test]
    fn net_control_write_preserves_comments_and_updates_value() {
        use std::io::Write;

        let original = "# RePlayOS config\nsystem_net_control = \"false\"\n";
        let tmp_dir =
            std::env::temp_dir().join(format!("replay-config-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let original_path = tmp_dir.join("original.cfg");
        let output_path = tmp_dir.join("output.cfg");
        std::fs::File::create(&original_path)
            .unwrap()
            .write_all(original.as_bytes())
            .unwrap();

        let mut config = ReplayConfig::parse(original).unwrap();
        config.set_system_net_control(true);
        config.write_to_file(&original_path, &output_path).unwrap();

        let result = std::fs::read_to_string(&output_path).unwrap();
        assert!(result.contains("# RePlayOS config"), "comment preserved");
        assert!(
            result.contains("system_net_control = \"true\""),
            "net control updated"
        );
    }

    #[test]
    fn parse_error_on_malformed_line() {
        let result = ReplayConfig::parse("no_equals_sign");
        assert!(result.is_err());
    }

    #[test]
    fn wifi_hidden_defaults_to_false() {
        let config = ReplayConfig::parse("").unwrap();
        assert!(!config.wifi_hidden());
    }

    #[test]
    fn wifi_hidden_true() {
        let config = ReplayConfig::parse("wifi_hidden = \"true\"").unwrap();
        assert!(config.wifi_hidden());
    }

    #[test]
    fn system_skin_default() {
        let config = ReplayConfig::parse("").unwrap();
        assert_eq!(config.system_skin(), 0);
    }

    #[test]
    fn system_skin_parsed() {
        let config = ReplayConfig::parse("system_skin = \"5\"").unwrap();
        assert_eq!(config.system_skin(), 5);
    }

    #[test]
    fn retroachievements_reads_username_and_password_state() {
        let config =
            ReplayConfig::parse("rcheevos_username = \"player\"\nrcheevos_password = \"secret\"\n")
                .unwrap();
        assert_eq!(config.retroachievements_username(), Some("player"));
        assert!(config.retroachievements_password_configured());
    }

    #[test]
    fn retroachievements_empty_password_is_not_configured() {
        let config =
            ReplayConfig::parse("rcheevos_username = \"player\"\nrcheevos_password = \"\"\n")
                .unwrap();
        assert_eq!(config.retroachievements_username(), Some("player"));
        assert!(!config.retroachievements_password_configured());
    }

    #[test]
    fn write_to_file_refuses_to_create_missing_config() {
        let config = ReplayConfig::parse("rcheevos_username = \"player\"\n").unwrap();
        let missing =
            std::env::temp_dir().join(format!("replay-missing-cfg-{}.cfg", std::process::id()));
        let _ = std::fs::remove_file(&missing);

        // RePlayOS owns creating replay.cfg; we must error rather than create it.
        assert!(config.write_to_file(&missing, &missing).is_err());
        assert!(
            !missing.exists(),
            "must not create a config RePlayOS didn't"
        );
    }

    #[test]
    fn write_to_file_refuses_to_rewrite_empty_config() {
        let tmp_dir = std::env::temp_dir().join(format!("replay-empty-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let path = tmp_dir.join("replay.cfg");
        std::fs::write(&path, "").unwrap();

        let mut config = ReplayConfig::parse("").unwrap();
        config.set_system_net_control(true);

        assert!(config.write_to_file(&path, &path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn write_to_file_failure_does_not_truncate_original_config() {
        use std::io::Write;

        let original = "# RePlayOS config\nwifi_name = \"OldWifi\"\n";
        let tmp_dir =
            std::env::temp_dir().join(format!("replay-config-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let original_path = tmp_dir.join("replay.cfg");
        let output_path = tmp_dir.join("target-dir");
        std::fs::File::create(&original_path)
            .unwrap()
            .write_all(original.as_bytes())
            .unwrap();
        std::fs::create_dir(&output_path).unwrap();

        let mut config = ReplayConfig::parse(original).unwrap();
        config.set_system_net_control(true);

        assert!(config.write_to_file(&original_path, &output_path).is_err());
        assert_eq!(std::fs::read_to_string(&original_path).unwrap(), original);
    }

    // ── AppSettings tests ────────────────────────────────────────

    #[test]
    fn app_settings_empty_defaults() {
        let settings = AppSettings::empty();
        assert_eq!(settings.region_preference(), "world");
        assert_eq!(settings.region_preference_secondary(), None);
        assert_eq!(settings.font_size(), "normal");
        assert_eq!(settings.skin(), None);
        assert_eq!(settings.language_primary(), None);
        assert_eq!(settings.language_secondary(), None);
        assert!(settings.locale().is_none());
        assert_eq!(settings.github_api_key(), None);
        assert_eq!(settings.update_channel(), "stable");
        assert_eq!(settings.skipped_version(), None);
    }

    #[test]
    fn app_settings_roundtrip() {
        let tmp_dir =
            std::env::temp_dir().join(format!("replay-settings-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let path = tmp_dir.join("settings.cfg");

        let mut settings = AppSettings::empty();
        settings.set_region_preference("japan");
        settings.set_skin(Some(5));
        settings.set_language_primary("ja");
        settings.set_update_channel("beta");
        settings.save(&path).unwrap();

        let loaded = AppSettings::from_file(&path).unwrap();
        assert_eq!(loaded.region_preference(), "japan");
        assert_eq!(loaded.skin(), Some(5));
        assert_eq!(loaded.language_primary(), Some("ja"));
        assert_eq!(loaded.update_channel(), "beta");
    }

    #[test]
    fn setup_dismissed_defaults_to_false() {
        let settings = AppSettings::empty();
        assert!(!settings.setup_dismissed());
    }

    #[test]
    fn setup_dismissed_roundtrip() {
        let mut settings = AppSettings::empty();
        assert!(!settings.setup_dismissed());
        settings.set_setup_dismissed(true);
        assert!(settings.setup_dismissed());
        settings.set_setup_dismissed(false);
        assert!(!settings.setup_dismissed());
    }

    #[test]
    fn app_settings_save_preserves_existing() {
        let tmp_dir =
            std::env::temp_dir().join(format!("replay-settings-preserve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let path = tmp_dir.join("settings.cfg");

        // Write initial file with a comment
        std::fs::write(&path, "# My settings\nregion_preference = \"usa\"\n").unwrap();

        let mut settings = AppSettings::from_file(&path).unwrap();
        settings.set_skin(Some(3));
        settings.save(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("# My settings"), "comment preserved");
        assert!(
            content.contains("region_preference = \"usa\""),
            "existing key preserved"
        );
        assert!(content.contains("skin = \"3\""), "new key added");
    }
}
