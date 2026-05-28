//! `serde::Deserialize` shape of a `data/community/<system>.json` file.
//!
//! Contributors edit these files to add metadata for "external" games not
//! covered by No-Intro / TheGamesDB / MAME / LaunchBox — the trigger case is
//! the AmigaVision Amiga distribution, but the schema is system-agnostic.
//! `build-catalog` reads the files at catalog-build time and writes rows
//! into `canonical_game`, `rom_entry`, and `catalog_game_resource`.

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct CommunityFile {
    pub entries: Vec<CommunityEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommunityEntry {
    pub filename_stem: String,
    pub display_name: String,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub developer: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub players: Option<u8>,
    #[serde(default)]
    pub coop: Option<bool>,
    #[serde(default)]
    pub description: Option<LocalizedText>,
    #[serde(default)]
    pub boxart_url: Option<String>,
    #[serde(default)]
    pub title_image_url: Option<String>,
    #[serde(default)]
    pub screenshot_urls: Vec<String>,
    #[serde(default)]
    pub manuals: Vec<ManualResource>,
    #[serde(default)]
    pub videos: Vec<VideoResource>,
    #[serde(default)]
    pub strategy_guides: Vec<LinkResource>,
    #[serde(default)]
    pub video_indexes: Vec<LinkResource>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub crc32: Option<String>,
    #[serde(default, rename = "override")]
    pub override_existing: bool,
}

/// Either a bare English string or a `{lang: text}` map. The bare form is a
/// shorthand for `{"en": "..."}` and is accepted to keep the common case
/// terse. The `en` key is required in either form — consumers call `en()`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LocalizedText {
    Bare(String),
    Map(HashMap<String, String>),
}

impl LocalizedText {
    pub fn en(&self) -> &str {
        match self {
            LocalizedText::Bare(s) => s.as_str(),
            LocalizedText::Map(m) => m.get("en").map(String::as_str).unwrap_or(""),
        }
    }

    pub fn get(&self, lang: &str) -> Option<&str> {
        match self {
            LocalizedText::Bare(s) if lang == "en" => Some(s.as_str()),
            LocalizedText::Bare(_) => None,
            LocalizedText::Map(m) => m.get(lang).map(String::as_str),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManualResource {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoResource {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LinkResource {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_entry() {
        let json = r#"{
            "entries": [
                {"filename_stem": "AmigaVision", "display_name": "AmigaVision"}
            ]
        }"#;
        let file: CommunityFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.entries.len(), 1);
        let e = &file.entries[0];
        assert_eq!(e.filename_stem, "AmigaVision");
        assert_eq!(e.display_name, "AmigaVision");
        assert!(e.description.is_none());
        assert!(!e.override_existing);
    }

    #[test]
    fn description_bare_string_treated_as_english() {
        let json =
            r#"{"entries":[{"filename_stem":"x","display_name":"X","description":"hello"}]}"#;
        let file: CommunityFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.entries[0].description.as_ref().unwrap().en(), "hello");
    }

    #[test]
    fn description_polyglot_map() {
        let json = r#"{"entries":[{"filename_stem":"x","display_name":"X",
            "description":{"en":"hi","ja":"こんにちは"}}]}"#;
        let file: CommunityFile = serde_json::from_str(json).unwrap();
        let d = file.entries[0].description.as_ref().unwrap();
        assert_eq!(d.en(), "hi");
        assert_eq!(d.get("ja"), Some("こんにちは"));
        assert_eq!(d.get("es"), None);
    }

    #[test]
    fn override_field_renamed_from_keyword() {
        let json = r#"{"entries":[{"filename_stem":"x","display_name":"X","override":true}]}"#;
        let file: CommunityFile = serde_json::from_str(json).unwrap();
        assert!(file.entries[0].override_existing);
    }

    #[test]
    fn resource_arrays_default_to_empty() {
        let json = r#"{"entries":[{"filename_stem":"x","display_name":"X"}]}"#;
        let file: CommunityFile = serde_json::from_str(json).unwrap();
        let e = &file.entries[0];
        assert!(e.manuals.is_empty());
        assert!(e.videos.is_empty());
        assert!(e.strategy_guides.is_empty());
        assert!(e.video_indexes.is_empty());
        assert!(e.screenshot_urls.is_empty());
        assert!(e.tags.is_empty());
    }
}
