//! Wire types for game documents (manuals, walkthroughs, etc.).
//! Native fs scanning lives in `replay_control_core_server::game_docs`.

use serde::{Deserialize, Serialize};

/// A single game document (manual, walkthrough, reference, extra).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDocument {
    pub relative_path: String,
    pub label: String,
    pub extension: String,
    pub size_bytes: u64,
    pub category: DocumentCategory,
}

/// Classification of a game document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DocumentCategory {
    Manual,
    Walkthrough,
    Reference,
    Extra,
}
