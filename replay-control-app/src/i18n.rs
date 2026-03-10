use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Supported locales. English is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    #[default]
    En,
}

impl Locale {
    pub fn code(&self) -> &'static str {
        match self {
            Locale::En => "en",
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// Provides the i18n context to the component tree.
/// Call this once at the App root level.
pub fn provide_i18n() {
    let (locale, set_locale) = signal(Locale::default());
    provide_context(I18nContext { locale, set_locale });
}

/// Retrieves the current i18n context.
pub fn use_i18n() -> I18nContext {
    expect_context::<I18nContext>()
}

#[derive(Clone, Copy)]
pub struct I18nContext {
    pub locale: ReadSignal<Locale>,
    pub set_locale: WriteSignal<Locale>,
}

/// Translation function. Returns the localized string for the given key.
/// Currently only English is supported — add match arms per locale to extend.
pub fn t(locale: Locale, key: &str) -> &'static str {
    // All keys fall through to English for now.
    // When adding a new locale, add a match on (locale, key) before the default.
    let _ = locale;
    match key {
        // App chrome
        "app.title" => "Replay Control",
        "nav.home" => "Home",
        "nav.games" => "Games",
        "nav.favorites" => "Favs",
        "nav.more" => "More",

        // Home page
        "home.last_played" => "Last Played",
        "home.recently_played" => "Recently Played",
        "home.library" => "Library",
        "home.systems" => "Systems",
        "home.no_games_played" => "No games played yet",
        "home.no_recent" => "No recent games",
        "home.no_systems" => "No systems with games",

        // Stats
        "stats.games" => "Games",
        "stats.systems" => "Systems",
        "stats.favorites" => "Favorites",
        "stats.used" => "Used",
        "stats.storage" => "Storage",

        // Games page
        "games.systems" => "Systems",
        "games.search_placeholder" => "Search games...",
        "games.back" => "\u{2190} Back",
        "games.no_games" => "No games",
        "games.loading_roms" => "Loading ROMs...",
        "games.delete_confirm" => "Delete this ROM?",
        "games.deleted" => "ROM deleted",
        "games.renamed" => "ROM renamed",
        "games.rename" => "Rename",
        "games.delete" => "Delete",
        "games.cancel" => "Cancel",
        "games.confirm" => "Confirm",
        "games.actions" => "Actions",
        "games.load_more" => "Load more",

        // Favorites page
        "favorites.title" => "Favorites",
        "favorites.view_grouped" => "View: Grouped",
        "favorites.view_flat" => "View: Flat",
        "favorites.empty" => "No favorites yet",
        "favorites.latest_added" => "Latest Added",
        "favorites.recently_added" => "Recently Added",
        "favorites.by_system" => "By System",
        "favorites.all" => "All Favorites",

        // Organize favorites
        "organize.title" => "Organize Favorites",
        "organize.primary" => "Primary",
        "organize.secondary" => "Secondary (optional)",
        "organize.none" => "None",
        "organize.system" => "By System",
        "organize.genre" => "By Genre",
        "organize.players" => "By Players",
        "organize.rating" => "By Rating",
        "organize.alphabetical" => "Alphabetical",
        "organize.keep_originals" => "Keep originals at root",
        "organize.keep_hint" => "Maintains compatibility with ReplayOS UI",
        "organize.apply" => "Organize",
        "organize.organizing" => "Organizing...",
        "organize.flatten" => "Flatten All",
        "organize.flattening" => "Flattening...",
        "organize.done" => "organized",
        "organize.flattened" => "favorites moved to root",
        "organize.already_flat" => "All favorites are already at root",

        // Hostname settings
        "more.hostname" => "Hostname",
        "hostname.title" => "Hostname",
        "hostname.label" => "Hostname",
        "hostname.hint" => "Sets the network name for this RePlayOS system. Use lowercase letters, digits, and hyphens (e.g., replay-living-room).",
        "hostname.saved" => "Hostname updated. Reboot may be needed for mDNS (.local) to fully update.",
        "hostname.invalid" => "Invalid hostname",

        // More page
        "more.title" => "More",
        "more.upload" => "Upload ROMs",
        "more.wifi" => "Wi-Fi Configuration",
        "more.nfs" => "NFS Share Settings",
        "more.system_info" => "System Info",
        "more.storage" => "Storage",
        "more.path" => "Path",
        "more.disk_total" => "Disk Total",
        "more.disk_used" => "Disk Used",
        "more.disk_available" => "Disk Available",
        "more.ethernet_ip" => "Ethernet IP",
        "more.wifi_ip" => "Wi-Fi IP",
        "more.not_connected" => "Not connected",
        "more.refresh_storage" => "Refresh Storage",
        "more.refreshing" => "Refreshing...",
        "more.storage_changed" => "Storage updated",
        "more.storage_unchanged" => "Storage unchanged",

        // Skin
        "more.skin" => "Skin",
        "skin.title" => "Skin",
        "skin.current" => "Current",
        "skin.hint" => "Select a skin. Reload the page to see the changes.",
        "skin.applied" => "Skin saved. Reload to see the new skin.",
        "skin.sync" => "Sync with ReplayOS",
        "skin.sync_hint" => "When enabled, the app skin follows the ReplayOS skin setting.",

        // WiFi configuration
        "wifi.title" => "Wi-Fi Configuration",
        "wifi.ssid" => "Network Name (SSID)",
        "wifi.password" => "Password",
        "wifi.country" => "Country Code",
        "wifi.mode" => "Security Mode",
        "wifi.hidden" => "Hidden Network",

        // NFS settings
        "nfs.title" => "NFS Share Settings",
        "nfs.server" => "Server Address",
        "nfs.share" => "Share Path",
        "nfs.version" => "NFS Version",
        "nfs.hint" => "A reboot is required for NFS changes to take effect.",

        // Settings (shared)
        "settings.save" => "Save",
        "settings.saving" => "Saving...",
        "settings.saved" => "Settings saved",
        "settings.apply_hint" => "Restart ReplayOS to apply changes.",
        "settings.restart_ui" => "Restart ReplayOS",
        "settings.restarting" => "Restarting...",
        "settings.reboot" => "Reboot System",
        "settings.rebooting" => "Rebooting...",
        "settings.reboot_hint" => "A reboot is required for WiFi changes to take effect.",
        "settings.password_enter" => "Enter password",

        // Game detail page
        "game_detail.info" => "Game Info",
        "game_detail.system" => "System",
        "game_detail.filename" => "Filename",
        "game_detail.file_size" => "File Size",
        "game_detail.format" => "Format",
        "game_detail.arcade_info" => "Arcade Info",
        "game_detail.year" => "Year",
        "game_detail.manufacturer" => "Manufacturer",
        "game_detail.players" => "Players",
        "game_detail.rotation" => "Rotation",
        "game_detail.category" => "Category",
        "game_detail.parent_rom" => "Parent ROM",
        "game_detail.metadata" => "Metadata",
        "game_detail.genre" => "Genre",
        "game_detail.developer" => "Developer",
        "game_detail.status" => "Status",
        "game_detail.raw_category" => "Raw Category",
        "game_detail.region" => "Region",
        "game_detail.description" => "Description",
        "game_detail.no_description" => "No description available",
        "game_detail.screenshots" => "Screenshots",
        "game_detail.no_screenshots" => "No screenshots available",
        "game_detail.videos" => "Videos",
        "game_detail.no_videos" => "No videos available",
        "game_detail.music" => "Music / Soundtrack",
        "game_detail.no_music" => "No soundtrack available",
        "game_detail.manual" => "Manual",
        "game_detail.no_manual" => "No manual available",
        "game_detail.actions" => "Actions",
        "game_detail.favorite" => "Favorite",
        "game_detail.unfavorite" => "Unfavorite",
        "game_detail.rename" => "Rename",
        "game_detail.delete" => "Delete",
        "game_detail.confirm_delete" => "Confirm Delete",
        "game_detail.external_metadata" => "Additional Info",
        "game_detail.publisher" => "Publisher",
        "game_detail.rating" => "Rating",

        // Metadata management
        "more.metadata" => "Game Metadata",
        "metadata.title" => "Game Metadata",
        "metadata.descriptions" => "Descriptions & Ratings",
        "metadata.descriptions_hint" => "Game descriptions, ratings, and publishers from LaunchBox. Downloaded and cached locally.",
        "metadata.total_entries" => "Total Entries",
        "metadata.with_description" => "With Description",
        "metadata.with_rating" => "With Rating",
        "metadata.db_size" => "Database Size",
        "metadata.no_data" => "No metadata downloaded yet",
        "metadata.download_metadata" => "Download / Update",
        "metadata.downloading_metadata" => "Downloading...",
        "metadata.downloading_file" => "Downloading metadata...",
        "metadata.matched" => "matched",
        "metadata.clear_images" => "Clear Images",
        "metadata.clearing_images" => "Clearing Images...",
        "metadata.cleared_images" => "Images cleared",
        "metadata.confirm_clear_images" => "Delete all downloaded box art and screenshots?",
        "metadata.data_management" => "Data Management",
        "metadata.building_index" => "Building ROM index...",
        "metadata.parsing_xml" => "Parsing XML...",
        "metadata.import_complete" => "Import complete",
        "metadata.import_failed" => "Import failed",
        "metadata.processed" => "processed",
        "metadata.coverage" => "Coverage by System",
        "metadata.no_coverage" => "No metadata downloaded yet. Use the button above to download.",

        // Images
        "metadata.images" => "Images",
        "metadata.images_hint" => "Download box art and screenshots from libretro-thumbnails. Images are stored on your storage device.",
        "metadata.no_images" => "No images downloaded yet",
        "metadata.no_images_short" => "None",
        "metadata.no_image_systems" => "No supported systems found",
        "metadata.with_boxart" => "Box Art",
        "metadata.with_snap" => "Screenshots",
        "metadata.media_size" => "Media Size",
        "metadata.download_images" => "Download",
        "metadata.update_images" => "Update",
        "metadata.download_all" => "Download All",
        "metadata.downloading_all" => "Downloading...",
        "metadata.cloning_repo" => "Downloading",
        "metadata.copying_images" => "Copying",
        "metadata.images_found" => "found",
        "metadata.stop" => "Stop",
        "metadata.cancelling" => "Cancelling...",
        "metadata.import_cancelled" => "Cancelled",

        "metadata.attribution" => "Attribution",
        "metadata.attribution_text" => "Game descriptions and ratings provided by LaunchBox. Box art and screenshots from libretro-thumbnails. Data is cached locally for offline use and is not redistributed.",

        // Logs
        "more.logs" => "System Logs",
        "logs.title" => "System Logs",
        "logs.refresh" => "Refresh",
        "logs.source_all" => "All Services",
        "logs.source_companion" => "Replay Control",
        "logs.source_replay" => "RePlayOS UI",

        // Common
        "common.loading" => "Loading...",
        "common.error" => "Error",

        _ => "???",
    }
}
