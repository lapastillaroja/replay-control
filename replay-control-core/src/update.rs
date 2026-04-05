use serde::{Deserialize, Serialize};

/// An available update discovered from GitHub releases.
/// Asset download URLs are NOT stored — resolved fresh at download time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvailableUpdate {
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    pub release_notes_url: String,
    pub published_at: String,
    pub binary_size: u64,
    pub site_size: u64,
}

/// Client-side update lifecycle state.
/// Single source of truth for all update UI — provided as app-level context.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum UpdateState {
    /// No update available.
    #[default]
    None,
    /// A newer version was found.
    Available(AvailableUpdate),
    /// Update installed, service is restarting.
    Restarting { expected_version: String },
}

/// Compare two semver version strings.
/// Returns true if `candidate` is strictly newer than `current`.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    let current = semver::Version::parse(current).ok();
    let candidate = semver::Version::parse(candidate).ok();
    match (current, candidate) {
        (Some(c), Some(v)) => v > c,
        _ => false,
    }
}

/// Validate that a version/tag string is safe for shell interpolation.
pub fn validate_version(version: &str) -> bool {
    let v = version.strip_prefix('v').unwrap_or(version);
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Release channel for update checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }

    pub fn from_str_value(s: &str) -> Self {
        match s {
            "beta" => Self::Beta,
            _ => Self::Stable,
        }
    }
}


/// Temp directory for all update runtime state.
pub const UPDATE_DIR: &str = "/var/tmp/replay-control-update";
/// Lock file outside the update directory (survives nukes).
pub const UPDATE_LOCK: &str = "/var/tmp/replay-control-update.lock";
/// Helper script path.
pub const UPDATE_SCRIPT: &str = "/var/tmp/replay-control-do-update.sh";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch() {
        assert!(is_newer("0.1.0", "0.1.1"));
    }

    #[test]
    fn newer_minor() {
        assert!(is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn newer_major() {
        assert!(is_newer("0.1.0", "1.0.0"));
    }

    #[test]
    fn same_version() {
        assert!(!is_newer("0.1.0", "0.1.0"));
    }

    #[test]
    fn older_version() {
        assert!(!is_newer("0.2.0", "0.1.0"));
    }

    #[test]
    fn prerelease_lower_than_release() {
        assert!(!is_newer("0.1.0", "0.1.0-beta.4"));
    }

    #[test]
    fn prerelease_newer_than_older_release() {
        assert!(is_newer("0.1.0", "0.2.0-beta.1"));
    }

    #[test]
    fn prerelease_ordering() {
        assert!(is_newer("0.2.0-beta.1", "0.2.0-beta.2"));
    }

    #[test]
    fn invalid_current() {
        assert!(!is_newer("not-semver", "0.1.0"));
    }

    #[test]
    fn invalid_candidate() {
        assert!(!is_newer("0.1.0", "not-semver"));
    }

    #[test]
    fn both_invalid() {
        assert!(!is_newer("garbage", "also-garbage"));
    }

    #[test]
    fn validate_version_valid() {
        assert!(validate_version("v0.1.0"));
        assert!(validate_version("0.1.0-beta.4"));
        assert!(!validate_version("1.0.0+build123"));
    }

    #[test]
    fn validate_version_invalid() {
        assert!(!validate_version(""));
        assert!(!validate_version("v"));
        assert!(!validate_version("1.0; rm -rf /"));
        assert!(!validate_version("$(evil)"));
    }

    #[test]
    fn update_state_default_is_none() {
        assert_eq!(UpdateState::default(), UpdateState::None);
    }

    #[test]
    fn update_channel_from_str() {
        assert_eq!(
            UpdateChannel::from_str_value("stable"),
            UpdateChannel::Stable
        );
        assert_eq!(UpdateChannel::from_str_value("beta"), UpdateChannel::Beta);
        assert_eq!(
            UpdateChannel::from_str_value("invalid"),
            UpdateChannel::Stable
        );
        assert_eq!(UpdateChannel::from_str_value(""), UpdateChannel::Stable);
    }

    #[test]
    fn update_channel_as_str() {
        assert_eq!(UpdateChannel::Stable.as_str(), "stable");
        assert_eq!(UpdateChannel::Beta.as_str(), "beta");
    }

    #[test]
    fn validate_version_edge_cases() {
        assert!(validate_version("0.1.0"));
        assert!(validate_version("v0.1.0"));
        assert!(validate_version("0.1.0-beta.4"));
        assert!(validate_version("1.0.0-rc1"));
        assert!(!validate_version(""));
        assert!(!validate_version("v"));
        assert!(!validate_version("1.0.0+build")); // + not allowed (shell mismatch)
        assert!(!validate_version("$(cmd)"));
        assert!(!validate_version("1.0; rm -rf /"));
        assert!(!validate_version("v1.0\"malicious"));
    }

    #[test]
    fn update_state_serialization_roundtrip() {
        let states = vec![
            UpdateState::None,
            UpdateState::Available(AvailableUpdate {
                version: "0.3.0".to_string(),
                tag: "v0.3.0".to_string(),
                prerelease: false,
                release_notes_url: String::new(),
                published_at: String::new(),
                binary_size: 10000,
                site_size: 4000,
            }),
            UpdateState::Restarting {
                expected_version: "0.3.0".to_string(),
            },
        ];
        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: UpdateState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }
}
