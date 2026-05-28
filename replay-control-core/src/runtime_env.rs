//! Deployment mode: whether the app is running on the RePlayOS device
//! ([`Mode::Device`]) or as a standalone off-device ROM manager
//! ([`Mode::Standalone`]). The variant carries everything the deployment
//! shape needs — `Standalone` owns the storage root the user pointed us at,
//! so there is no separate optional field to keep in sync with `Mode`.
//!
//! Pure + serializable so it can be sent to the client to gate device-only
//! UI; the actual detection lives in `replay-control-core-server` (it
//! touches the filesystem). The wire shape is just the tag (`"device"` /
//! `"standalone"`); the client never sees the standalone storage path.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How the app is deployed. `Device` is RePlayOS; `Standalone` is the
/// off-device ROM-manager mode (a first-class peer, not just a dev fallback).
/// `Standalone` carries the storage root the user supplied with
/// `--storage-path`, so "where does `replay.cfg` live?" is answered by
/// pattern-matching on `Mode` — no parallel `Option<PathBuf>` to drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Running on the RePlayOS device — full feature set, including reading
    /// `replay.cfg` and mutating RePlayOS system settings (Wi-Fi, NFS,
    /// RetroAchievements, hostname, password, reboot, frontend restart).
    Device,
    /// Running off-device as a standalone ROM manager. Library, browsing,
    /// favorites, metadata, etc. work; system-mutation features are disabled
    /// because there is no RePlayOS to configure. The payload is the storage
    /// root supplied via `--storage-path`.
    Standalone { storage_root: PathBuf },
}

impl Mode {
    /// True when running on the RePlayOS device.
    pub fn is_device(&self) -> bool {
        matches!(self, Mode::Device)
    }

    /// Whether system-mutation features apply (only on the device).
    pub fn allows_system_mutations(&self) -> bool {
        self.is_device()
    }

    /// Storage root supplied via `--storage-path`. `Some` for `Standalone`,
    /// `None` for `Device`. The presence of a value is the type-level proof
    /// that this deployment has a startup-fixed storage location — callers
    /// never need to handle the "Standalone but no root" case.
    pub fn standalone_root(&self) -> Option<&Path> {
        match self {
            Mode::Standalone { storage_root } => Some(storage_root.as_path()),
            Mode::Device => None,
        }
    }
}

// ── Wire shape ──────────────────────────────────────────────────────────
//
// The client only needs the variant tag — it never inspects the standalone
// storage path (that path is a server-side filesystem concern, not something
// the browser should know about). We therefore serialise to the bare strings
// `"device"` / `"standalone"`. Deserialising back from the wire fills
// `Standalone` with an empty placeholder path; the client never reads it.
// On the server, `Mode` is always constructed locally via `detect_mode`, so
// the round-trip placeholder doesn't leak into real logic.

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ModeWire {
    Device,
    Standalone,
}

impl Serialize for Mode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Mode::Device => ModeWire::Device,
            Mode::Standalone { .. } => ModeWire::Standalone,
        };
        wire.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match ModeWire::deserialize(d)? {
            ModeWire::Device => Mode::Device,
            ModeWire::Standalone => Mode::Standalone {
                storage_root: PathBuf::new(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standalone() -> Mode {
        Mode::Standalone {
            storage_root: PathBuf::from("/tmp/library"),
        }
    }

    #[test]
    fn mode_predicates() {
        assert!(Mode::Device.is_device());
        assert!(!standalone().is_device());
        assert!(Mode::Device.allows_system_mutations());
        assert!(!standalone().allows_system_mutations());
    }

    #[test]
    fn mode_standalone_root_exposes_path() {
        assert_eq!(Mode::Device.standalone_root(), None);
        assert_eq!(
            standalone().standalone_root(),
            Some(Path::new("/tmp/library"))
        );
    }

    #[test]
    fn mode_serde_wire_shape_is_tag_only() {
        // Wire is the bare variant name — payload never leaks to the client.
        let device_json = serde_json::to_string(&Mode::Device).unwrap();
        let standalone_json = serde_json::to_string(&standalone()).unwrap();
        assert_eq!(device_json, "\"device\"");
        assert_eq!(standalone_json, "\"standalone\"");
    }

    #[test]
    fn mode_serde_round_trip_via_wire() {
        // Deserialising rebuilds Standalone with an empty placeholder path —
        // the client never inspects the path, so this is correct by contract.
        let device_back: Mode = serde_json::from_str("\"device\"").unwrap();
        let standalone_back: Mode = serde_json::from_str("\"standalone\"").unwrap();
        assert_eq!(device_back, Mode::Device);
        assert_eq!(
            standalone_back,
            Mode::Standalone {
                storage_root: PathBuf::new(),
            }
        );
    }
}
