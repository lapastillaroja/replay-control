//! Host identity shared by the server and browser.

use serde::{Deserialize, Serialize};

/// Where Replay Control is running, independent of storage and installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    #[serde(rename = "device")]
    ReplayOs,
    Standalone,
}

impl HostKind {
    pub fn is_replayos(self) -> bool {
        self == Self::ReplayOs
    }
}

/// Whether a browser can open the host's storage selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageSelectorAccess {
    Unavailable,
    Local,
    Remote,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_wire_shape_is_path_free() {
        assert_eq!(
            serde_json::to_string(&HostKind::ReplayOs).unwrap(),
            "\"device\""
        );
        assert_eq!(
            serde_json::to_string(&HostKind::Standalone).unwrap(),
            "\"standalone\""
        );
        assert_eq!(
            serde_json::from_str::<HostKind>("\"standalone\"").unwrap(),
            HostKind::Standalone
        );
    }
}
