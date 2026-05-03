//! Reportable health issues with shipped data assets.
//!
//! A "shipped data asset" is anything the binary depends on that lives
//! outside its own image — currently `catalog.sqlite`, eventually fonts,
//! themes, and other artifacts the release bundle will carry once the
//! release-asset-manifest plan lands. Issues are detected at startup
//! (file missing, schema drift, signature invalid, …) and surfaced to
//! the UI through the existing config-SSE channel.
//!
//! v1 ships with a single concrete reporter: catalog schema mismatch.
//! `severity` and `action` fields aren't here yet — every reporter we
//! have today is fatal-to-the-affected-feature with reinstall as the
//! only safe remediation. Add fields when a real Warning-severity or
//! in-app remediation case appears.

use serde::{Deserialize, Serialize};

/// One reported problem with a shipped data asset.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetHealthIssue {
    /// Logical asset name. Matches manifest entries when applicable
    /// ("catalog.sqlite", "site", "themes"). Free-form for non-manifest
    /// assets.
    pub asset: String,
    /// Stable identifier for the failure mode the UI/i18n layer keys on.
    /// Examples: "schema_too_old", "file_missing", "signature_invalid".
    pub kind: String,
    /// Human-readable message — used as the journal entry and as banner
    /// fallback text when the i18n layer has no `kind`-specific copy yet.
    pub message: String,
}
