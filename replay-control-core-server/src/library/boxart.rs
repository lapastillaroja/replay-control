use std::path::Path;

use super::thumbnails::{ThumbnailKind, is_valid_image, resolve_image_on_disk};

/// Resolve the effective box art URL for a ROM.
///
/// Precedence is intentionally shared by detail pages, pickers, and
/// now-playing surfaces: valid user override, library-enriched URL, then
/// filesystem fallback.
pub async fn resolve_effective_box_art_url(
    rc_dir: &Path,
    system: &str,
    rom_filename: &str,
    existing_box_art_url: Option<&str>,
    arcade_display: Option<&str>,
    override_path: Option<&str>,
) -> Option<String> {
    let media_base = rc_dir.join("media").join(system);

    if let Some(override_path) = override_path.filter(|path| !path.is_empty()) {
        let full = media_base.join(override_path);
        if is_valid_image(full).await {
            return Some(format!("/media/{system}/{override_path}"));
        }
    }

    if let Some(url) = existing_box_art_url.filter(|url| !url.is_empty()) {
        return Some(url.to_string());
    }

    resolve_image_on_disk(
        arcade_display.map(str::to_owned),
        media_base,
        ThumbnailKind::Boxart.media_dir(),
        rom_filename.to_string(),
    )
    .await
    .map(|path| format!("/media/{system}/{path}"))
}
