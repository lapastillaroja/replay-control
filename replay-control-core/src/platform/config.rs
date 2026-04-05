use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};

/// Internal key=value file parser with comment-preserving write.
/// Not exposed publicly — only used by `SystemConfig` and `AppSettings`.
#[derive(Debug, Clone)]
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
    pub(crate) fn write_preserving(&self, original_path: &Path, output_path: &Path) -> Result<()> {
        let original_content =
            std::fs::read_to_string(original_path).map_err(|e| Error::io(original_path, e))?;

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

        std::fs::write(output_path, output).map_err(|e| Error::io(output_path, e))
    }

    /// Write a fresh file with all entries sorted by key (no original to preserve).
    pub(crate) fn write_fresh(&self, path: &Path) -> Result<()> {
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort();
        let mut output = String::new();
        for key in keys {
            let value = &self.entries[key];
            output.push_str(&format!("{key} = \"{value}\"\n"));
        }
        std::fs::write(path, output).map_err(|e| Error::io(path, e))
    }
}

// ── SystemConfig: typed access to replay.cfg ─────────────────────

/// Parsed RePlayOS system configuration from `replay.cfg`.
///
/// This is an external file owned by the OS. The app may read all keys
/// but may only write wifi and NFS settings.
#[derive(Debug, Clone)]
pub struct SystemConfig {
    inner: KeyValueFile,
}

impl SystemConfig {
    /// Parse a `replay.cfg` file from the given path.
    pub fn from_file(path: &Path) -> Result<Self> {
        Ok(Self {
            inner: KeyValueFile::from_file(path)?,
        })
    }

    /// Parse config from a string.
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

    // ── Write methods (only wifi + NFS) ──────────────────────────

    /// Update wifi settings. Only these keys may be written to `replay.cfg`.
    pub fn set_wifi(&mut self, ssid: &str, password: &str, country: &str, mode: &str, hidden: bool) {
        self.inner.set("wifi_name", ssid);
        self.inner.set("wifi_pwd", password);
        self.inner.set("wifi_country", country);
        self.inner.set("wifi_mode", mode);
        self.inner.set("wifi_hidden", if hidden { "true" } else { "false" });
    }

    /// Update NFS settings. Only these keys may be written to `replay.cfg`.
    pub fn set_nfs(&mut self, server: &str, share: &str, version: &str) {
        self.inner.set("nfs_server", server);
        self.inner.set("nfs_share", share);
        self.inner.set("nfs_version", version);
    }

    /// Write back to disk, preserving comments and key order from the original file.
    pub fn write_to_file(&self, original_path: &Path, output_path: &Path) -> Result<()> {
        self.inner.write_preserving(original_path, output_path)
    }
}

/// Supported UI locales. Used for validation on both read and write.
pub const SUPPORTED_LOCALES: &[&str] = &["en", "es", "ja"];

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
            inner: KeyValueFile::parse("").unwrap(),
        }
    }

    /// Save to disk, preserving comments and order if the file already exists.
    pub fn save(&self, path: &Path) -> Result<()> {
        if path.exists() {
            self.inner.write_preserving(path, path)
        } else {
            self.inner.write_fresh(path)
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

    pub fn locale(&self) -> Option<&str> {
        self.inner
            .get_non_empty("locale")
            .filter(|v| SUPPORTED_LOCALES.contains(v))
    }

    pub fn github_api_key(&self) -> Option<&str> {
        self.inner.get_non_empty("github_api_key")
    }

    pub fn update_channel(&self) -> &str {
        self.inner.get("update_channel").unwrap_or("stable")
    }

    pub fn skipped_version(&self) -> Option<&str> {
        self.inner.get_non_empty("skipped_version")
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

    pub fn set_update_channel(&mut self, channel: &str) {
        self.inner.set("update_channel", channel);
    }

    pub fn set_skipped_version(&mut self, version: &str) {
        self.inner.set("skipped_version", version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_config() {
        let content = r#"
            system_storage = "usb"
            wifi_name = "MyWifi"
            wifi_pwd = "secret123"
            video_mode = "5"
        "#;

        let config = SystemConfig::parse(content).unwrap();
        assert_eq!(config.storage_mode(), "usb");
        assert_eq!(config.wifi_name(), Some("MyWifi"));
        assert_eq!(config.video_mode(), Some("5"));
    }

    #[test]
    fn parse_with_comments_and_blanks() {
        let content = "# comment\n\nsystem_storage = \"sd\"\n";
        let config = SystemConfig::parse(content).unwrap();
        assert_eq!(config.storage_mode(), "sd");
    }

    #[test]
    fn set_wifi_updates_values() {
        let mut config = SystemConfig::parse("wifi_name = \"old\"").unwrap();
        config.set_wifi("new", "pass", "US", "wpa2", false);
        assert_eq!(config.wifi_name(), Some("new"));
        assert_eq!(config.wifi_country(), Some("US"));
        assert_eq!(config.wifi_mode(), Some("wpa2"));
        assert!(!config.wifi_hidden());
    }

    #[test]
    fn default_storage_mode() {
        let config = SystemConfig::parse("").unwrap();
        assert_eq!(config.storage_mode(), "sd");
    }

    #[test]
    fn write_preserves_comments_and_updates_values() {
        use std::io::Write;

        let original = "# RePlayOS config\nwifi_name = \"OldWifi\"\nnfs_server = \"old-server\"\n";
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

        let mut config = SystemConfig::parse(original).unwrap();
        config.set_wifi("NewWifi", "pass", "US", "wpa2", false);
        config.set_nfs("new-server", "/share", "4");
        config.write_to_file(&original_path, &output_path).unwrap();

        let result = std::fs::read_to_string(&output_path).unwrap();
        assert!(result.contains("# RePlayOS config"), "comment preserved");
        assert!(result.contains("wifi_name = \"NewWifi\""), "value updated");
        assert!(
            result.contains("nfs_server = \"new-server\""),
            "nfs updated"
        );
    }

    #[test]
    fn parse_error_on_malformed_line() {
        let result = SystemConfig::parse("no_equals_sign");
        assert!(result.is_err());
    }

    #[test]
    fn wifi_hidden_defaults_to_false() {
        let config = SystemConfig::parse("").unwrap();
        assert!(!config.wifi_hidden());
    }

    #[test]
    fn wifi_hidden_true() {
        let config = SystemConfig::parse("wifi_hidden = \"true\"").unwrap();
        assert!(config.wifi_hidden());
    }

    #[test]
    fn system_skin_default() {
        let config = SystemConfig::parse("").unwrap();
        assert_eq!(config.system_skin(), 0);
    }

    #[test]
    fn system_skin_parsed() {
        let config = SystemConfig::parse("system_skin = \"5\"").unwrap();
        assert_eq!(config.system_skin(), 5);
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
        assert_eq!(settings.locale(), None);
        assert_eq!(settings.github_api_key(), None);
        assert_eq!(settings.update_channel(), "stable");
        assert_eq!(settings.skipped_version(), None);
    }

    #[test]
    fn app_settings_roundtrip() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "replay-settings-rt-{}",
            std::process::id()
        ));
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
    fn app_settings_save_preserves_existing() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "replay-settings-preserve-{}",
            std::process::id()
        ));
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
        assert!(content.contains("region_preference = \"usa\""), "existing key preserved");
        assert!(content.contains("skin = \"3\""), "new key added");
    }
}
