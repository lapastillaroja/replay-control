//! Wire types for retrokit manual recommendations. The per-system folder
//! keys for the retrokit-manuals source live in the `SYSTEMS` table
//! (`systems::System::retrokit_manuals_folder`).

use serde::{Deserialize, Serialize};

/// A manual suggestion discovered via retrokit's manifests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualRecommendation {
    pub source: String,
    pub title: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub language: Option<String>,
    pub source_id: String,
}
