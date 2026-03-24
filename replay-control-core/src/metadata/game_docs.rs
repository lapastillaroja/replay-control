//! In-folder document detection for game directories.
//!
//! Scans a game directory for document files (PDFs, text files, images, HTML)
//! that are bundled alongside game data — especially common in ScummVM and
//! DOS/PC games (manuals, walkthroughs, code cards, extras).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A document file found inside a game directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDocument {
    /// Relative path from ROM directory: "Manual.pdf", "EXTRAS/Art Book.pdf"
    pub relative_path: String,
    /// Display label derived from filename: "Manual", "Walkthrough", "Art Book"
    pub label: String,
    /// File extension for icon/handling: "pdf", "txt", "jpg"
    pub extension: String,
    /// File size in bytes
    pub size_bytes: u64,
    /// Category for sorting: Manual, Walkthrough, Reference, Extra
    pub category: DocumentCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DocumentCategory {
    Manual,
    Walkthrough,
    Reference,
    Extra,
}

/// Document file extensions we recognize.
const DOC_EXTENSIONS: &[&str] = &[
    "pdf", "txt", "doc", "htm", "html", "jpg", "jpeg", "png", "gif",
];

/// Files to exclude from results (case-insensitive basename match).
const BLOCKLIST: &[&str] = &[
    "install.txt",
    "install.doc",
    "install.bat",
    "license.txt",
    "license.doc",
    "copying.txt",
    "copying",
    "drivers.txt",
    "interp.txt",
    "file_id.diz",
    "setup.txt",
    "whatsnew.txt",
    "changes.txt",
    "changelog.txt",
];

/// Extensions to always exclude (scene/technical files).
const BLOCKED_EXTENSIONS: &[&str] = &["nfo", "diz", "bat", "com", "exe", "ini", "cfg"];

/// Known document subdirectories to scan (case-insensitive).
const DOC_SUBDIRS: &[&str] = &["extras", "manual", "manuals", "docs", "doc"];

/// Scan a game directory for document files.
///
/// Scans the root directory and known document subdirectories (EXTRAS/, MANUAL/,
/// DOCS/, etc.) for files matching document extensions, filtering out known
/// technical/installation files via a blocklist.
///
/// Returns documents sorted by category (manuals first) then by filename.
pub fn scan_game_documents(game_dir: &Path) -> Vec<GameDocument> {
    let mut docs = Vec::new();

    if !game_dir.is_dir() {
        return docs;
    }

    // Scan root directory
    scan_directory(game_dir, game_dir, &mut docs);

    // Scan known subdirectories
    if let Ok(entries) = std::fs::read_dir(game_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_lowercase();
                if DOC_SUBDIRS.contains(&dir_name.as_str()) {
                    scan_directory(&path, game_dir, &mut docs);
                }
            }
        }
    }

    // Sort: by category first, then by filename
    docs.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.label.cmp(&b.label))
    });

    docs
}

fn scan_directory(dir: &Path, root: &Path, docs: &mut Vec<GameDocument>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = match entry.file_name().to_str() {
            Some(f) => f.to_string(),
            None => continue,
        };

        let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();

        // Check if this extension is a document type
        if !DOC_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        // Check blocklist on extension
        if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        // Check blocklist on filename (case-insensitive)
        let lower_name = filename.to_lowercase();
        if BLOCKLIST.contains(&lower_name.as_str()) {
            continue;
        }

        // Get file metadata
        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip very small files (likely empty or stubs)
        if metadata.len() < 100 {
            continue;
        }

        // Compute relative path from root
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        // Derive a display label from filename
        let label = derive_label(&filename);
        let category = categorize(&lower_name, &ext);

        docs.push(GameDocument {
            relative_path: relative,
            label,
            extension: ext,
            size_bytes: metadata.len(),
            category,
        });
    }
}

/// Derive a human-readable label from a filename.
fn derive_label(filename: &str) -> String {
    // Strip extension
    let stem = filename
        .rfind('.')
        .map(|i| &filename[..i])
        .unwrap_or(filename);

    // Replace underscores and hyphens with spaces
    stem.replace(['_', '-'], " ")
}

/// Categorize a document based on its filename and extension.
fn categorize(lower_name: &str, ext: &str) -> DocumentCategory {
    // Check for manual keywords
    if lower_name.contains("manual") {
        return DocumentCategory::Manual;
    }

    // Check for walkthrough/solution keywords
    if lower_name.contains("walkthrough")
        || lower_name.contains("solucion")
        || lower_name.contains("solution")
        || lower_name.contains("guia")
        || lower_name.contains("guide")
        || lower_name.contains("hint")
    {
        return DocumentCategory::Walkthrough;
    }

    // Check for reference keywords
    if lower_name.contains("code")
        || lower_name.contains("clave")
        || lower_name.contains("reference")
        || lower_name.contains("map")
        || lower_name.contains("card")
        || lower_name.contains("protec")
    {
        return DocumentCategory::Reference;
    }

    // PDFs that aren't specifically categorized are likely manuals
    if ext == "pdf" {
        return DocumentCategory::Manual;
    }

    // README files are reference material
    if lower_name.starts_with("readme") || lower_name.starts_with("leeme") {
        return DocumentCategory::Reference;
    }

    DocumentCategory::Extra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorize_manual_pdf() {
        assert_eq!(categorize("manual.pdf", "pdf"), DocumentCategory::Manual);
    }

    #[test]
    fn categorize_walkthrough() {
        assert_eq!(
            categorize("walkthrough.txt", "txt"),
            DocumentCategory::Walkthrough
        );
    }

    #[test]
    fn categorize_codes() {
        assert_eq!(
            categorize("copy protection codes.pdf", "pdf"),
            DocumentCategory::Reference
        );
    }

    #[test]
    fn categorize_generic_pdf() {
        assert_eq!(
            categorize("game extras.pdf", "pdf"),
            DocumentCategory::Manual
        );
    }

    #[test]
    fn categorize_readme() {
        assert_eq!(categorize("readme.txt", "txt"), DocumentCategory::Reference);
    }

    #[test]
    fn derive_label_strips_ext_and_underscores() {
        assert_eq!(derive_label("My_Manual.pdf"), "My Manual");
    }

    #[test]
    fn blocklist_filters_install_txt() {
        assert!(BLOCKLIST.contains(&"install.txt"));
    }

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let docs = scan_game_documents(Path::new("/nonexistent/path"));
        assert!(docs.is_empty());
    }
}
