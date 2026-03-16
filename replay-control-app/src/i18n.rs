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
        "nav.home" => "Games",
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
        "stats.storage_used" => "Storage Used",

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
        "hostname.hint" => {
            "Sets the network name for this RePlayOS system. Use lowercase letters, digits, and hyphens (e.g., replay-living-room)."
        }
        "hostname.saved" => {
            "Hostname updated. Reboot may be needed for mDNS (.local) to fully update."
        }
        "hostname.invalid" => "Invalid hostname",

        // GitHub API key
        "more.github" => "GitHub API Key",
        "github.title" => "GitHub API Key",
        "github.label" => "Personal Access Token",
        "github.hint" => {
            "Optional. Increases the GitHub API rate limit from 60 to 5,000 requests/hour for thumbnail indexing. Create a token at github.com/settings/tokens (no scopes needed)."
        }

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

        // Text size
        "more.text_size" => "Text Size",
        "more.text_size_hint" => "Adjust the app text size",

        // Region preference
        "more.region" => "Region",
        "region.title" => "Region Preference",
        "region.hint" => {
            "ROMs from your preferred region appear first in game lists and search results."
        }
        "region.usa" => "USA",
        "region.europe" => "Europe",
        "region.japan" => "Japan",
        "region.world" => "World",
        "region.saved" => "Region preference saved",
        "region.secondary_title" => "Fallback Region",
        "region.secondary_hint" => {
            "If your preferred region is unavailable, prefer this region next."
        }
        "region.none" => "None (use default order)",

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
        "settings.reboot_hint" => "A reboot is required for changes to take effect.",
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
        "game_detail.my_videos" => "My Videos",
        "game_detail.add_video" => "Add",
        "game_detail.add_video_placeholder" => "Paste a YouTube or Twitch URL...",
        "game_detail.add_video_error" => "Invalid URL. Supported: YouTube, Twitch, Vimeo.",
        "game_detail.add_video_duplicate" => "This video is already saved.",
        "game_detail.video_added" => "Video added",
        "game_detail.remove_video" => "Remove",
        "game_detail.find_trailers" => "Find Trailers",
        "game_detail.find_gameplay" => "Find Gameplay",
        "game_detail.find_1cc" => "Find 1CC",
        "game_detail.searching" => "Searching...",
        "game_detail.no_results" => "No videos found",
        "game_detail.search_error" => "Video search unavailable. Paste URLs directly.",
        "game_detail.pin_video" => "Pin",
        "game_detail.pinned" => "Pinned",
        "game_detail.show_all_videos" => "Show all",
        "game_detail.user_captures" => "Your Captures",
        "game_detail.no_captures" => {
            "Take screenshots during gameplay on your RePlayOS \u{2014} they'll appear here!"
        }
        "game_detail.view_all_captures" => "View all",
        "game_detail.manual" => "Manual",
        "game_detail.no_manual" => "No manual available",
        "game_detail.launch" => "Launch on TV",
        "game_detail.launching" => "Launching...",
        "game_detail.launched" => "Launched!",
        "game_detail.launch_error" => "Failed to launch",
        "game_detail.launch_not_replayos" => "Not running on RePlayOS",
        "game_detail.actions" => "Actions",
        "game_detail.favorite" => "Favorite",
        "game_detail.unfavorite" => "Unfavorite",
        "game_detail.rename" => "Rename",
        "game_detail.delete" => "Delete",
        "game_detail.confirm_delete" => "Confirm Delete",
        "game_detail.regional_variants" => "Regional Variants",
        "game_detail.translations" => "Translations",
        "game_detail.hacks" => "Hacks",
        "game_detail.special_versions" => "Special Versions",
        "game_detail.more_like_this" => "More Like This",
        "game_detail.change_cover" => "Change cover",
        "game_detail.choose_boxart" => "Choose Box Art",
        "game_detail.reset_default" => "Reset to Default",
        "game_detail.downloading" => "Downloading...",
        "game_detail.no_variants" => "No alternative covers found",
        "game_detail.build_index_first" => "Download thumbnail index first",
        "game_detail.external_metadata" => "Additional Info",
        "game_detail.publisher" => "Publisher",
        "game_detail.rating" => "Rating",

        // Metadata management
        "more.metadata" => "Game Metadata",
        "metadata.title" => "Game Data",
        "metadata.data_sources" => "Data Sources",
        "metadata.descriptions_launchbox" => "Descriptions & Ratings (LaunchBox)",
        "metadata.no_data" => "Not imported yet",
        "metadata.entries_summary" => "entries",
        "metadata.last_updated" => "last updated",
        "metadata.download_metadata" => "Update",
        "metadata.downloading_metadata" => "Updating...",
        "metadata.downloading_file" => "Downloading metadata...",
        "metadata.matched" => "matched",
        "metadata.data_management" => "Data Management",
        "metadata.building_index" => "Building ROM index...",
        "metadata.parsing_xml" => "Parsing XML...",
        "metadata.import_complete" => "Import complete",
        "metadata.import_failed" => "Import failed",
        "metadata.processed" => "processed",
        "metadata.system_overview" => "System Overview",
        "metadata.col_system" => "System",
        "metadata.col_games" => "Games",
        "metadata.col_desc" => "Desc.",
        "metadata.col_thumb" => "Thumb.",
        "metadata.no_systems" => "No systems with data yet.",

        // Thumbnails (libretro manifest-based)
        "metadata.thumbnails_libretro" => "Thumbnails (libretro)",
        "metadata.thumbnail_summary" => "boxart",
        "metadata.thumbnail_snaps" => "snaps",
        "metadata.thumbnail_on_disk" => "on disk",
        "metadata.thumbnail_index_summary" => "available across",
        "metadata.thumbnail_systems" => "systems",
        "metadata.thumbnail_update" => "Update",
        "metadata.thumbnail_updating" => "Updating...",
        "metadata.thumbnail_stop" => "Stop",
        "metadata.thumbnail_cancelling" => "Cancelling...",
        "metadata.thumbnail_no_data" => "Not imported yet",
        "metadata.thumbnail_phase_indexing" => "Fetching index...",
        "metadata.thumbnail_phase_downloading" => "Downloading...",
        "metadata.thumbnail_complete" => "Update complete",
        "metadata.thumbnail_failed" => "Update failed",
        "metadata.thumbnail_cancelled" => "Update cancelled",
        "metadata.thumbnail_downloaded" => "downloaded",
        "metadata.thumbnail_indexed" => "indexed",

        // Game library
        "metadata.rebuild_game_library" => "Rebuild Game Library",
        "metadata.rebuilding_game_library" => "Rebuilding...",
        "metadata.game_library_rebuilt" => "Game library rebuilt successfully",
        "metadata.confirm_rebuild_game_library" => {
            "Rebuild the game library? This re-scans all games from disk."
        }

        // Advanced data management
        "metadata.advanced_actions" => "Advanced",

        // Data management
        "metadata.clear_images" => "Clear Downloaded Images",
        "metadata.clearing_images" => "Clearing...",
        "metadata.cleared_images" => "Images cleared",
        "metadata.confirm_clear_images" => "Delete all downloaded box art and screenshots?",
        "metadata.cleanup_orphans" => "Cleanup Orphaned Images",
        "metadata.cleaning_orphans" => "Cleaning up...",
        "metadata.confirm_cleanup_orphans" => {
            "Delete images and metadata for ROMs that no longer exist?"
        }
        "metadata.clear_index" => "Clear Thumbnail Index",
        "metadata.clearing_index" => "Clearing...",
        "metadata.index_cleared" => "Thumbnail index cleared",
        "metadata.confirm_clear_index" => {
            "Delete the thumbnail index? It can be rebuilt by clicking Update."
        }
        "metadata.clear_metadata" => "Clear Metadata",
        "metadata.clearing_metadata" => "Clearing...",
        "metadata.metadata_cleared" => "Metadata cleared",
        "metadata.confirm_clear_metadata" => "Delete all game descriptions and ratings?",

        // Built-in metadata
        "metadata.builtin" => "Built-in Game Data",
        "metadata.builtin_arcade" => "Arcade Database",
        "metadata.builtin_arcade_summary" => "entries, MAME",
        "metadata.builtin_console" => "Console Database",
        "metadata.builtin_console_summary_entries" => "ROM entries across",
        "metadata.builtin_console_summary_systems" => "systems",
        "metadata.builtin_hint" => {
            "Names, genres, player counts, and other metadata compiled into the app. No import needed."
        }

        "metadata.attribution" => "Attribution",
        "metadata.attribution_text" => {
            "Game descriptions and ratings provided by LaunchBox. Box art and screenshots from libretro-thumbnails. Data is cached locally for offline use and is not redistributed."
        }

        // Logs
        "more.logs" => "System Logs",
        "logs.title" => "System Logs",
        "logs.refresh" => "Refresh",
        "logs.source_all" => "All Services",
        "logs.source_companion" => "Replay Control",
        "logs.source_replay" => "RePlayOS UI",

        // Search
        "search.title" => "Search",
        "search.placeholder" => "Search all games...",
        "search.no_results" => "No results found",
        "search.no_results_with_filters" => "No results. Try removing some filters.",
        "search.results_summary" => "results across",
        "search.systems" => "systems",
        "search.browsing_genre" => "Browsing all",
        "search.see_all" => "See all",

        // Filters
        "filter.hide_hacks" => "Hide Hacks",
        "filter.genre" => "Genre",
        "filter.genre_all" => "All Genres",
        "filter.hide_translations" => "Hide Translations",
        "filter.hide_betas" => "Hide Betas",
        "filter.hide_clones" => "Hide Clones",
        "filter.multiplayer" => "Multiplayer",
        "filter.rating_any" => "Any Rating",
        "filter.clear_filters" => "Clear Filters",
        "filter.active_search" => "Search",
        "filter.filtered_results" => "Filtered results",

        // Search extras (Phase 2)
        "search.random_game" => "Random Game",
        "search.recent_searches" => "Recent Searches",
        "search.clear_recent" => "Clear",

        // Recommendations
        "home.discover" => "Discover",
        "home.discover_random" => "Rediscover Your Library",
        "home.discover_multiplayer" => "Multiplayer",
        "home.discover_games" => "games",
        "home.because_you_love" => "Because You Love",
        "home.see_all" => "See all",
        "home.top_rated" => "Top Rated",

        // Metadata busy banner
        "metadata.busy_banner" => {
            "Metadata update in progress \u{2014} some info may be temporarily unavailable"
        }

        // Common
        "common.loading" => "Loading...",
        "common.error" => "Error",

        _ => "???",
    }
}
