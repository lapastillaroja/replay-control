//! Reportable health issues with shipped data assets (`catalog.sqlite`
//! today; future fonts/themes/etc. via the release-asset-manifest plan).
//! Detected at startup and surfaced to the UI via the config-SSE channel.

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
