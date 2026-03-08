use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};

/// Parsed RePlayOS configuration from `replay.cfg`.
///
/// The config file uses a simple `key = "value"` format where all values
/// are quoted strings.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    entries: HashMap<String, String>,
}

impl ReplayConfig {
    /// Parse a `replay.cfg` file from the given path.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        Self::parse(&content)
    }

    /// Parse config from a string.
    pub fn parse(content: &str) -> Result<Self> {
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

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.entries.insert(key.to_string(), value.to_string());
    }

    /// Write the config back to a file, preserving unknown keys
    /// and adding any new ones at the end.
    pub fn write_to_file(&self, original_path: &Path, output_path: &Path) -> Result<()> {
        let original_content = std::fs::read_to_string(original_path)
            .map_err(|e| Error::io(original_path, e))?;

        let mut written_keys: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
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

        // Append any new keys not in the original file
        for (key, value) in &self.entries {
            if !written_keys.contains(key.as_str()) {
                output.push_str(&format!("{key} = \"{value}\"\n"));
            }
        }

        std::fs::write(output_path, output).map_err(|e| Error::io(output_path, e))
    }

    // Convenience accessors for commonly used settings

    pub fn storage_mode(&self) -> &str {
        self.get("system_storage").unwrap_or("sd")
    }

    pub fn wifi_name(&self) -> Option<&str> {
        self.get("wifi_name")
    }

    pub fn wifi_country(&self) -> Option<&str> {
        self.get("wifi_country")
    }

    pub fn nfs_server(&self) -> Option<&str> {
        self.get("nfs_server")
    }

    pub fn nfs_share(&self) -> Option<&str> {
        self.get("nfs_share")
    }

    pub fn video_mode(&self) -> Option<&str> {
        self.get("video_mode")
    }

    pub fn video_connector(&self) -> Option<&str> {
        self.get("video_connector")
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
    fn set_overrides_value() {
        let mut config = ReplayConfig::parse("wifi_name = \"old\"").unwrap();
        config.set("wifi_name", "new");
        assert_eq!(config.wifi_name(), Some("new"));
    }

    #[test]
    fn default_storage_mode() {
        let config = ReplayConfig::parse("").unwrap();
        assert_eq!(config.storage_mode(), "sd");
    }
}
