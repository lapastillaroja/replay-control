//! Wire types for retrokit manual recommendations.
//! Native fetch/parse logic lives in `replay_control_core_server::retrokit_manuals`.

use serde::{Deserialize, Serialize};

/// A manual suggestion discovered via retrokit's manifests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualRecommendation {
    pub source: String,
    pub title: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub language: Option<String>,
    pub source_id: String,
}
