//! Stable storage identifiers.
//!
//! Each ROM storage gets a `<kind>-<8 hex>` id (e.g. `usb-1a2b3c4d`) on first
//! attach, written to `<storage>/.replay-control/storage-id`. The host uses
//! the id to namespace the per-storage `library.db` under the central data
//! directory, so swaps preserve library state across reboots and mount-path
//! churn.
//!
//! The 8 hex chars are the lower 32 bits of a CRC32 over the filesystem's
//! own stable identifier — the volume UUID for block-backed filesystems
//! (exFAT/ext4/etc.), or the `server:/share` source string for NFS. Same
//! storage → same id every time; reformatting rotates the id by changing
//! the underlying FS UUID. Random fallback only when no FS identifier can
//! be obtained (tmpfs, weird mounts).

use std::path::Path;

/// Length of the hex suffix in a storage id.
pub const HEX_LEN: usize = 8;

/// Separator between the kind prefix and the hex suffix.
pub const SEPARATOR: char = '-';

#[derive(Debug, thiserror::Error)]
pub enum IdError {
    #[error("storage id must be <kind>-<8 hex>; got {0:?}")]
    WrongShape(String),
}

/// Validated storage id. The only ways to construct one are
/// [`Self::parse`], [`Self::from_filesystem_id`], and [`Self::generate`],
/// so anywhere this type appears the inner string is guaranteed to match
/// `[a-z]+-[0-9a-f]{8}` — safe to use as a path component.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageId(String);

impl StorageId {
    /// Derive a stable id from a filesystem-level identifier (UUID for
    /// block-backed filesystems, `server:/share` for NFS). Same input
    /// always produces the same id, so a storage that loses its marker
    /// (transient read failure, accidental rm, restored backup) gets the
    /// same id back on the next call.
    ///
    /// `kind` is a short lowercase tag (`"usb"`, `"sd"`, `"nvme"`, `"nfs"`)
    /// that lets a human glancing at `/var/lib/replay-control/storages/`
    /// tell at a glance what kind of drive each entry corresponds to.
    pub fn from_filesystem_id(kind: &str, fs_id: &str) -> Self {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(fs_id.as_bytes());
        Self::from_kind_and_crc(kind, hasher.finalize())
    }

    /// Random-fallback constructor used when no filesystem identifier is
    /// obtainable (tmpfs, overlay, exotic mounts). Will rotate on remount;
    /// callers should prefer [`Self::from_filesystem_id`] when possible.
    pub fn generate(kind: &str) -> Self {
        let mut buf = [0u8; 4];
        // `getrandom` reads from `getrandom(2)` (Linux) which never blocks
        // after early boot. A failure means the kernel RNG itself is broken;
        // there's no useful local fallback so panic loudly.
        getrandom::getrandom(&mut buf).expect("OS RNG must be available");
        Self::from_kind_and_crc(kind, u32::from_le_bytes(buf))
    }

    /// Parse and validate a string into a `StorageId`.
    pub fn parse(s: &str) -> Result<Self, IdError> {
        validate(s)?;
        Ok(Self(s.to_string()))
    }

    /// View as the canonical `<kind>-<hex>` string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_kind_and_crc(kind: &str, crc: u32) -> Self {
        debug_assert!(
            !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase()),
            "storage kind must be lowercase ASCII: {kind:?}"
        );
        Self(format!("{kind}{SEPARATOR}{crc:08x}"))
    }
}

impl std::fmt::Display for StorageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for StorageId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for StorageId {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// True if `s` is a syntactically valid storage id.
pub fn is_valid(s: &str) -> bool {
    validate(s).is_ok()
}

fn validate(s: &str) -> Result<(), IdError> {
    let (kind, hex) = s
        .split_once(SEPARATOR)
        .ok_or_else(|| IdError::WrongShape(s.to_string()))?;
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(IdError::WrongShape(s.to_string()));
    }
    if hex.len() != HEX_LEN || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(IdError::WrongShape(s.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_filesystem_id_is_deterministic() {
        let a = StorageId::from_filesystem_id("usb", "1A2B-3C4D");
        let b = StorageId::from_filesystem_id("usb", "1A2B-3C4D");
        assert_eq!(a, b);
        let c = StorageId::from_filesystem_id("usb", "1A2B-5678");
        assert_ne!(a, c);
        // Different kind with same fs_id must also differ.
        let d = StorageId::from_filesystem_id("sd", "1A2B-3C4D");
        assert_ne!(a, d);
    }

    #[test]
    fn from_filesystem_id_handles_nfs_and_uuid_shapes() {
        for s in [
            StorageId::from_filesystem_id("usb", "1A2B-3C4D").as_str(),
            StorageId::from_filesystem_id("sd", "12345678-1234-5678-1234-567812345678").as_str(),
            StorageId::from_filesystem_id("nfs", "192.168.10.12:/volume1/games/retro").as_str(),
        ] {
            assert!(is_valid(s), "{s:?} should validate");
        }
    }

    #[test]
    fn generate_is_valid() {
        for kind in ["usb", "sd", "nvme", "nfs"] {
            for _ in 0..16 {
                let id = StorageId::generate(kind);
                assert!(is_valid(id.as_str()), "{id:?} should validate");
            }
        }
    }

    #[test]
    fn parse_round_trip() {
        let id = StorageId::parse("usb-1a2b3c4d").expect("parse");
        assert_eq!(id.as_str(), "usb-1a2b3c4d");
    }

    #[test]
    fn parse_rejects_wrong_shape() {
        for bad in [
            "",
            "usb",
            "usb-",
            "usb-1a2b",      // hex too short
            "usb-1a2b3c4dx", // hex too long
            "usb-1a2b3c4G",  // non-hex
            "USB-1a2b3c4d",  // uppercase kind
            "us b-1a2b3c4d", // space in kind
            "usb-1a2b-3c4d", // multi-dash
            "../etc/passwd",
        ] {
            assert!(StorageId::parse(bad).is_err(), "{bad:?} should reject");
        }
    }

    #[test]
    fn generate_collisions_are_rare() {
        // 32-bit keyspace per kind; 64 draws should never collide unless the
        // RNG is broken (probability ≈ 5e-7).
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            assert!(seen.insert(StorageId::generate("usb")));
        }
    }
}
